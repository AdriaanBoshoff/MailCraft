//! Writing the Lists, Tables and Properties layer ([MS-PST] 2.3): a heap-on-node, the
//! BTree-on-heap, and on them a Property Context and a Table Context.
//!
//! Values go in and come out in the same form the reader hands them over (`Raw`: id, type and
//! the bytes as stored), so a property this code does not understand is written as it was read.

use crate::PstError;
use crate::ltp::{Raw, Row};
use crate::write::{MAX_DATA, NdbWriter, Sub};
use openmailclient_model::{PropValue, codepage, ptype};
use std::collections::HashMap;
use std::io::{Read, Seek, Write};

/// The largest allocation a heap holds; a larger value lives in a subnode ([MS-PST] 2.3.3.3).
pub(crate) const MAX_ALLOC: usize = 3580;
/// An HID has 11 bits of index within a block and 16 of block ([MS-PST] 2.3.1.1).
const MAX_PER_BLOCK: usize = 0x7FF;
const MAX_BLOCKS: usize = 0x1_0000;
/// Subnodes that hold values are of this type ([MS-PST] 2.2.2.1 NID_TYPE_LTP).
const NID_TYPE_LTP: u32 = 0x1F;
/// The two columns every table has ([MS-PST] 2.3.4.4.1).
const ROW_ID: u32 = 0x67F2_0003;
const ROW_VER: u32 = 0x67F3_0003;

fn cannot(what: &str) -> PstError {
    PstError::Write(what.to_string())
}

/// A heap-on-node being filled ([MS-PST] 2.3.1): allocations, block by block.
pub(crate) struct Heap {
    sig: u8,
    blocks: Vec<Vec<Vec<u8>>>,
}

impl Heap {
    pub(crate) fn new(sig: u8) -> Self {
        Self { sig, blocks: vec![Vec::new()] }
    }

    /// The header of data block `block`: HNHDR in the first, HNBITMAPHDR in the eighth and every
    /// 128th after it, HNPAGEHDR in the others ([MS-PST] 2.3.1.2 to 2.3.1.4).
    fn head(block: usize) -> usize {
        match block {
            0 => 12,
            b if b >= 8 && (b - 8).is_multiple_of(128) => 66,
            _ => 2,
        }
    }

    /// The bytes block `block` takes with `allocs` and one more allocation of `extra` bytes:
    /// header, allocations, then the page map on an even offset ([MS-PST] 2.3.1.5).
    fn size(block: usize, allocs: &[Vec<u8>], extra: Option<usize>) -> usize {
        let data: usize = allocs.iter().map(Vec::len).sum::<usize>() + extra.unwrap_or(0);
        let count = allocs.len() + usize::from(extra.is_some());
        (Self::head(block) + data).next_multiple_of(2) + 4 + 2 * (count + 1)
    }

    /// Put `bytes` in the heap; its HID.
    pub(crate) fn alloc(&mut self, bytes: &[u8]) -> Result<u32, PstError> {
        if bytes.is_empty() || bytes.len() > MAX_ALLOC {
            return Err(cannot("a heap allocation of a size the format does not have"));
        }
        let block = self.blocks.len().saturating_sub(1);
        let fits = self.blocks.last().is_some_and(|b| b.len() < MAX_PER_BLOCK && Self::size(block, b, Some(bytes.len())) <= MAX_DATA);
        if !fits {
            if self.blocks.len() >= MAX_BLOCKS {
                return Err(cannot("more values than one heap holds"));
            }
            self.blocks.push(Vec::new());
        }
        let block = self.blocks.len().saturating_sub(1);
        let allocs = self.blocks.last_mut().ok_or_else(|| cannot("a heap without blocks"))?;
        allocs.push(bytes.to_vec());
        Ok((allocs.len() as u32) << 5 | (block as u32) << 16)
    }

    /// Replace an allocation by bytes of the same length (a header that is known last).
    fn set(&mut self, hid: u32, bytes: &[u8]) {
        let slot = self.blocks.get_mut((hid >> 16) as usize).and_then(|b| b.get_mut((((hid >> 5) & 0x7FF) as usize).wrapping_sub(1)));
        if let Some(slot) = slot.filter(|s| s.len() == bytes.len()) {
            slot.copy_from_slice(bytes);
        }
    }

    /// How full a block with `free` bytes left is ([MS-PST] 2.3.1.2 rgbFillLevel).
    fn level(free: usize) -> u8 {
        const AT_LEAST: [usize; 15] = [3584, 2560, 2048, 1792, 1536, 1280, 1024, 768, 512, 256, 128, 64, 32, 16, 8];
        AT_LEAST.iter().position(|least| free >= *least).unwrap_or(15) as u8
    }

    /// The data blocks of the node. `root` is the HID the layer above starts from.
    pub(crate) fn finish(self, root: u32) -> Vec<Vec<u8>> {
        let levels: Vec<u8> = self.blocks.iter().enumerate().map(|(i, b)| Self::level(MAX_DATA.saturating_sub(Self::size(i, b, None)))).collect();
        // Fill levels are four bits a block. The specification does not say which half of a
        // byte comes first; the earlier block is put in the low half, as bit fields are laid
        // out elsewhere in the format. Not compared with a file Outlook wrote.
        let fill = |from: usize, bytes: usize| -> Vec<u8> {
            (0..bytes).map(|i| levels.get(from + 2 * i).copied().unwrap_or(0) | levels.get(from + 2 * i + 1).copied().unwrap_or(0) << 4).collect()
        };
        let mut out = Vec::with_capacity(self.blocks.len());
        for (i, allocs) in self.blocks.iter().enumerate() {
            let mut body = vec![0u8; 2];
            match Self::head(i) {
                12 => {
                    body.extend([0xEC, self.sig]);
                    body.extend(root.to_le_bytes());
                    body.extend(fill(0, 4));
                }
                66 => body.extend(fill(i, 64)),
                _ => {}
            }
            let mut offsets = Vec::with_capacity(allocs.len() + 1);
            for a in allocs {
                offsets.push(body.len() as u16);
                body.extend(a);
            }
            offsets.push(body.len() as u16);
            if body.len() % 2 == 1 {
                body.push(0);
            }
            // ibHnpm, then HNPAGEMAP: cAlloc, cFree, rgibAlloc.
            let map = (body.len() as u16).to_le_bytes();
            if let Some(slot) = body.get_mut(..2) {
                slot.copy_from_slice(&map);
            }
            body.extend((allocs.len() as u16).to_le_bytes());
            body.extend(0u16.to_le_bytes());
            for o in offsets {
                body.extend(o.to_le_bytes());
            }
            out.push(body);
        }
        out
    }
}

/// A BTree-on-heap ([MS-PST] 2.3.2) over `records` (key, data), which are in key order. The
/// header goes into the allocation `header`, made before with eight bytes.
fn bth(heap: &mut Heap, header: u32, key: usize, ent: usize, records: &[(Vec<u8>, Vec<u8>)]) -> Result<(), PstError> {
    let mut level = 0u8;
    let mut root = 0u32;
    if !records.is_empty() {
        // Leaves: as many records as an allocation holds.
        let mut items: Vec<(Vec<u8>, u32)> = Vec::new();
        for group in records.chunks((MAX_ALLOC / (key + ent)).max(1)) {
            let bytes: Vec<u8> = group.iter().flat_map(|(k, d)| k.iter().chain(d.iter()).copied()).collect();
            items.push((group.first().map(|r| r.0.clone()).unwrap_or_default(), heap.alloc(&bytes)?));
        }
        // Index levels: the first key below, then the HID of the next level down.
        while items.len() > 1 {
            let mut up = Vec::new();
            for group in items.chunks(MAX_ALLOC / (key + 4)) {
                let bytes: Vec<u8> = group.iter().flat_map(|(k, hid)| k.iter().copied().chain(hid.to_le_bytes())).collect();
                up.push((group.first().map(|r| r.0.clone()).unwrap_or_default(), heap.alloc(&bytes)?));
            }
            items = up;
            level = level.checked_add(1).ok_or_else(|| cannot("an index that is too deep"))?;
        }
        root = items.first().map_or(0, |i| i.1);
    }
    // BTHHEADER: bType, cbKey, cbEnt, bIdxLevels, hidRoot.
    let mut head = vec![0xB5, key as u8, ent as u8, level];
    head.extend(root.to_le_bytes());
    heap.set(header, &head);
    Ok(())
}

/// The subnodes of one node as they come about: values too large for the heap, and whatever
/// the layer above puts there.
pub(crate) struct Subs {
    next: u32,
    pub list: Vec<Sub>,
}

impl Subs {
    /// `first` is the index the first value subnode gets.
    pub(crate) fn new(first: u32) -> Self {
        Self { next: first, list: Vec::new() }
    }

    /// A subnode of this node holding `blocks`, each at most 8,176 bytes; its id.
    pub(crate) fn value<W, I, B>(&mut self, w: &mut NdbWriter<W>, blocks: I) -> Result<u32, PstError>
    where
        W: Read + Write + Seek,
        I: IntoIterator<Item = B>,
        B: AsRef<[u8]>,
    {
        let nid = self.next.checked_shl(5).filter(|n| n >> 5 == self.next).ok_or_else(|| cannot("too many large values in one item"))? | NID_TYPE_LTP;
        self.next += 1;
        let data = w.data(blocks)?;
        self.list.push(Sub { nid, data, sub: 0 });
        Ok(nid)
    }
}

/// Where a value that does not sit in its record goes ([MS-PST] 2.3.3.2): nowhere when it is
/// empty, the heap up to 3,580 bytes, a subnode beyond.
fn hnid<W: Read + Write + Seek>(w: &mut NdbWriter<W>, heap: &mut Heap, subs: &mut Subs, bytes: &[u8]) -> Result<u32, PstError> {
    match bytes.len() {
        0 => Ok(0),
        n if n <= MAX_ALLOC => heap.alloc(bytes),
        _ => {
            let blocks: Vec<Vec<u8>> = bytes.chunks(MAX_DATA).map(<[u8]>::to_vec).collect();
            subs.value(w, &blocks)
        }
    }
}

/// Write a Property Context ([MS-PST] 2.3.3); the block id of the node's data. Large values
/// are added to `subs`.
pub(crate) fn write_pc<W: Read + Write + Seek>(w: &mut NdbWriter<W>, props: &[Raw], subs: &mut Subs) -> Result<u64, PstError> {
    let mut order: Vec<&Raw> = props.iter().collect();
    order.sort_by_key(|p| p.id);
    if order.windows(2).any(|p| p.first().map(|p| p.id) == p.get(1).map(|p| p.id)) {
        return Err(cannot("one property twice in an item"));
    }
    let mut heap = Heap::new(0xBC);
    let header = heap.alloc(&[0; 8])?;
    let mut records = Vec::with_capacity(order.len());
    for p in order {
        // A fixed size of up to four bytes sits in the record; the rest is an HNID.
        let value = if ptype::fixed_size(p.ty).is_some_and(|n| n <= 4) {
            let mut v = [0u8; 4];
            for (to, from) in v.iter_mut().zip(&p.data) {
                *to = *from;
            }
            u32::from_le_bytes(v)
        } else {
            hnid(w, &mut heap, subs, &p.data)?
        };
        // wPropId is the key; wPropType and dwValueHnid the data ([MS-PST] 2.3.3.3).
        let mut data = p.ty.to_le_bytes().to_vec();
        data.extend(value.to_le_bytes());
        records.push((p.id.to_le_bytes().to_vec(), data));
    }
    bth(&mut heap, header, 2, 6, &records)?;
    w.data(heap.finish(header))
}

/// How many bytes a column takes in a row ([MS-PST] 2.3.4.4.2): the value up to eight bytes, a
/// Boolean one, an HNID otherwise.
fn cell_size(ty: u16) -> usize {
    match ty {
        ptype::BOOL => 1,
        ty => ptype::fixed_size(ty).filter(|n| *n <= 8).unwrap_or(4),
    }
}

/// Write a Table Context ([MS-PST] 2.3.4); the block id of the node's data. `columns` (id,
/// type) are the columns the table has even when no row fills them; columns the rows bring are
/// added. Rows keep their order; a row matrix or a value too large for the heap goes to `subs`.
pub(crate) fn write_tc<W: Read + Write + Seek>(w: &mut NdbWriter<W>, columns: &[(u16, u16)], rows: &[Row], subs: &mut Subs) -> Result<u64, PstError> {
    let mut tags: Vec<u32> = columns.iter().map(|(id, ty)| u32::from(*id) << 16 | u32::from(*ty)).collect();
    tags.extend(rows.iter().flat_map(|r| r.cells.iter().map(|c| u32::from(c.id) << 16 | u32::from(c.ty))));
    tags.extend([ROW_ID, ROW_VER]);
    // TCOLDESCs are sorted by tag ([MS-PST] 2.3.4.1).
    tags.sort_unstable();
    tags.dedup();
    if tags.len() > 255 {
        return Err(cannot("a table with more than 255 columns"));
    }
    // The row: the row id at 0 and its version at 4, the other 8- and 4-byte cells, the 2-byte
    // ones, the 1-byte ones, then one bit a column ([MS-PST] 2.3.4.4.1).
    let mut place: HashMap<u32, (usize, usize, usize)> = HashMap::new();
    place.insert(ROW_ID, (0, 4, 0));
    place.insert(ROW_VER, (4, 4, 1));
    let (mut at, mut bit) = (8usize, 2usize);
    let mut ends = [0usize; 3];
    for (group, end) in [(4usize, 0usize), (2, 1), (1, 2)] {
        for tag in tags.iter().filter(|t| **t != ROW_ID && **t != ROW_VER) {
            let size = cell_size(*tag as u16);
            if size.min(4) == group {
                place.insert(*tag, (at, size, bit));
                at += size;
                bit += 1;
            }
        }
        if let Some(e) = ends.get_mut(end) {
            *e = at;
        }
    }
    let ceb = at;
    let row_size = ceb + tags.len().div_ceil(8);

    let mut heap = Heap::new(0x7C);
    let info_size = 22 + 8 * tags.len();
    // The header of the row index comes first and the table's own second, as in the files
    // Outlook writes.
    let index_hid = heap.alloc(&[0; 8])?;
    let info_hid = heap.alloc(&vec![0; info_size])?;

    let mut matrix: Vec<u8> = Vec::new();
    let mut index: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(rows.len());
    for (n, r) in rows.iter().enumerate() {
        let n = u32::try_from(n).map_err(|_| cannot("a table with too many rows"))?;
        let mut row = vec![0u8; row_size];
        let cell = |row: &mut Vec<u8>, tag: u32, bytes: &[u8]| {
            let Some(&(at, size, bit)) = place.get(&tag) else { return };
            for (to, from) in row.iter_mut().skip(at).take(size).zip(bytes) {
                *to = *from;
            }
            if let Some(b) = row.get_mut(ceb + bit / 8) {
                *b |= 1 << (7 - bit % 8);
            }
        };
        // The version is the row's own when it has one, and its place in the table otherwise.
        cell(&mut row, ROW_VER, &n.saturating_add(1).to_le_bytes());
        for c in &r.cells {
            let tag = u32::from(c.id) << 16 | u32::from(c.ty);
            if cell_size(c.ty) == 4 && ptype::fixed_size(c.ty).is_none_or(|s| s > 8) {
                let value = hnid(w, &mut heap, subs, &c.data)?;
                cell(&mut row, tag, &value.to_le_bytes());
            } else {
                cell(&mut row, tag, &c.data);
            }
        }
        cell(&mut row, ROW_ID, &r.id.to_le_bytes());
        matrix.extend(row);
        // TCROWID, Unicode: dwRowID, dwRowIndex ([MS-PST] 2.3.4.3.1).
        index.push((r.id.to_le_bytes().to_vec(), n.to_le_bytes().to_vec()));
    }
    index.sort_by_key(|(k, _)| k.first_chunk::<4>().map_or(0, |k| u32::from_le_bytes(*k)));
    if index.windows(2).any(|p| p.first().map(|p| &p.0) == p.get(1).map(|p| &p.0)) {
        return Err(cannot("two rows with one id in a table"));
    }
    bth(&mut heap, index_hid, 4, 4, &index)?;

    // The row matrix: in the heap when it fits, else a subnode whose blocks each hold whole
    // rows; every block but the last is a full 8 KB one ([MS-PST] 2.3.4.4).
    let rows_hnid = if matrix.is_empty() {
        0
    } else if matrix.len() <= MAX_ALLOC {
        heap.alloc(&matrix)?
    } else {
        let per = (MAX_DATA / row_size).max(1) * row_size;
        let count = matrix.len().div_ceil(per);
        let blocks: Vec<Vec<u8>> = matrix
            .chunks(per)
            .enumerate()
            .map(|(i, rows)| {
                let mut block = rows.to_vec();
                if i + 1 < count {
                    block.resize(MAX_DATA, 0);
                }
                block
            })
            .collect();
        subs.value(w, &blocks)?
    };

    // TCINFO: bType, cCols, rgib[4], hidRowIndex, hnidRows, hidIndex, then the TCOLDESCs.
    let mut info = vec![0x7C, tags.len() as u8];
    for v in [ends[0], ends[1], ends[2], row_size] {
        info.extend((v as u16).to_le_bytes());
    }
    info.extend(index_hid.to_le_bytes());
    info.extend(rows_hnid.to_le_bytes());
    info.extend(0u32.to_le_bytes());
    for tag in &tags {
        let (at, size, bit) = place.get(tag).copied().unwrap_or_default();
        info.extend(tag.to_le_bytes());
        info.extend((at as u16).to_le_bytes());
        info.extend([size as u8, bit as u8]);
    }
    heap.set(info_hid, &info);
    w.data(heap.finish(info_hid))
}

/// A value as a Unicode data file stores it: the other way of `ltp::decode`.
pub(crate) fn encode(id: u16, value: &PropValue) -> Raw {
    let multi = |values: Vec<Vec<u8>>| -> Vec<u8> {
        // ulCount, an offset for each value, then the values ([MS-PST] 2.3.3.4.2).
        let mut out = (values.len() as u32).to_le_bytes().to_vec();
        let mut at = 4 + values.len() * 4;
        for v in &values {
            out.extend((at as u32).to_le_bytes());
            at += v.len();
        }
        out.extend(values.concat());
        out
    };
    let data = match value {
        PropValue::String(s) => codepage::to_utf16le(s),
        PropValue::Binary(b) => b.clone(),
        PropValue::Guid(g) => g.to_vec(),
        PropValue::MultiInt32(list) => list.iter().flat_map(|v| v.to_le_bytes()).collect(),
        PropValue::MultiString(list) => multi(list.iter().map(|s| codepage::to_utf16le(s)).collect()),
        PropValue::MultiBinary(list) => multi(list.clone()),
        PropValue::Other { raw, .. } => raw.clone(),
        fixed => {
            let size = ptype::fixed_size(fixed.ty()).unwrap_or(8).min(8);
            fixed.fixed_bytes().and_then(|b| b.get(..size).map(<[u8]>::to_vec)).unwrap_or_default()
        }
    };
    Raw { id, ty: value.ty(), data }
}
