//! The Lists, Tables and Properties layer ([MS-PST] 2.3): the heap-on-node, the BTree-on-heap,
//! the Property Context (one object's properties) and the Table Context (a table of rows).

use crate::PstError;
use crate::ndb::{Ndb, Subnodes, Ver, u16_at, u32_at};
use openmailclient_model::{PropValue, codepage, ptype};

/// Records of one BTree-on-heap, rows of one table, values of one multi-valued property.
const MAX_BTH_RECORDS: usize = 1 << 20;
const MAX_ROWS: usize = 4_000_000;
const MAX_VALUES: usize = 65_536;

const SIG_HEAP: u8 = 0xEC;
const SIG_PC: u8 = 0xBC;
const SIG_TC: u8 = 0x7C;
const SIG_BTH: u8 = 0xB5;

fn damaged(what: &str) -> PstError {
    PstError::Damaged(what.to_string())
}

/// A heap-on-node ([MS-PST] 2.3.1): allocations spread over the data blocks of one node.
struct Heap {
    blocks: Vec<Vec<u8>>,
    client_sig: u8,
    user_root: u32,
    wide_index: bool,
}

impl Heap {
    fn new(blocks: Vec<Vec<u8>>, ver: Ver) -> Result<Self, PstError> {
        // HNHDR: ibHnpm, bSig, bClientSig, hidUserRoot.
        let first = blocks.first().ok_or_else(|| damaged("a heap has no data"))?;
        if first.get(2) != Some(&SIG_HEAP) {
            return Err(damaged("a heap does not start with its signature"));
        }
        let client_sig = first.get(3).copied().unwrap_or(0);
        let user_root = u32_at(first, 4).ok_or_else(|| damaged("a heap header is cut short"))?;
        Ok(Self { blocks, client_sig, user_root, wide_index: ver == Ver::Unicode4k })
    }

    /// One allocation by HID ([MS-PST] 2.3.1.1): 5 bits of type, an 11-bit 1-based index and a
    /// 16-bit block index. Files with 4K pages are not specified; there the index is taken as 14
    /// bits and the block as 13, falling back to the documented split.
    fn alloc(&self, hid: u32) -> Option<&[u8]> {
        if hid & 0x1F != 0 {
            return None;
        }
        let narrow = ((hid >> 16) as usize, ((hid >> 5) & 0x7FF) as usize);
        let wide = ((hid >> 19) as usize, ((hid >> 5) & 0x3FFF) as usize);
        let order = if self.wide_index { [wide, narrow] } else { [narrow, wide] };
        order.into_iter().find_map(|(block, index)| self.alloc_at(block, index))
    }

    fn alloc_at(&self, block: usize, index: usize) -> Option<&[u8]> {
        let b = self.blocks.get(block)?;
        // Every block starts with ibHnpm; the HNPAGEMAP there is cAlloc, cFree, rgibAlloc[cAlloc + 1].
        let map = usize::from(u16_at(b, 0)?);
        let count = usize::from(u16_at(b, map)?);
        if index == 0 || index > count {
            return None;
        }
        let at = map.checked_add(4)?.checked_add((index - 1).checked_mul(2)?)?;
        let (start, end) = (usize::from(u16_at(b, at)?), usize::from(u16_at(b, at + 2)?));
        b.get(start..end)
    }
}

/// Every (key, data) record of a BTree-on-heap ([MS-PST] 2.3.2), in key order.
fn bth(heap: &Heap, hid: u32) -> Result<Vec<(Vec<u8>, Vec<u8>)>, PstError> {
    // BTHHEADER: bType, cbKey, cbEnt, bIdxLevels, hidRoot.
    let header = heap.alloc(hid).ok_or_else(|| damaged("a property index is missing"))?;
    if header.first() != Some(&SIG_BTH) {
        return Err(damaged("a property index does not start with its signature"));
    }
    let (key, ent, levels) = (usize::from(*header.get(1).unwrap_or(&0)), usize::from(*header.get(2).unwrap_or(&0)), *header.get(3).unwrap_or(&0));
    let root = u32_at(header, 4).ok_or_else(|| damaged("a property index header is cut short"))?;
    let mut out = Vec::new();
    if root == 0 {
        return Ok(out);
    }
    if key == 0 || levels > 8 {
        return Err(damaged("a property index is not valid"));
    }
    let mut stack = vec![(root, levels)];
    let mut visited = 0usize;
    while let Some((hid, level)) = stack.pop() {
        visited += 1;
        if visited > MAX_BTH_RECORDS {
            return Err(damaged("a property index is implausibly large"));
        }
        let Some(data) = heap.alloc(hid) else { continue };
        if level == 0 {
            for rec in data.chunks_exact(key + ent) {
                let (k, d) = rec.split_at(key);
                out.push((k.to_vec(), d.to_vec()));
                if out.len() > MAX_BTH_RECORDS {
                    return Err(damaged("a property index is implausibly large"));
                }
            }
        } else {
            // Intermediate records: key, then the HID of the next level. Reversed to keep key order.
            let kids: Vec<u32> = data.chunks_exact(key + 4).filter_map(|rec| u32_at(rec, key)).collect();
            stack.extend(kids.into_iter().rev().map(|k| (k, level - 1)));
        }
    }
    Ok(out)
}

/// One property as stored: its id, its type, and the bytes of its value.
pub(crate) struct Raw {
    pub id: u16,
    pub ty: u16,
    pub data: Vec<u8>,
}

/// The bytes an HNID names ([MS-PST] 2.3.3.2): a heap allocation, or a whole subnode.
fn hnid(ndb: &mut Ndb, heap: &Heap, subs: &Subnodes, id: u32) -> Vec<u8> {
    if id == 0 {
        return Vec::new();
    }
    if id & 0x1F == 0 {
        return heap.alloc(id).map(<[u8]>::to_vec).unwrap_or_default();
    }
    match subs.get(&id) {
        Some((data, _)) => ndb.data(*data).unwrap_or_default(),
        None => Vec::new(),
    }
}

/// Read a Property Context ([MS-PST] 2.3.3).
pub(crate) fn read_pc(ndb: &mut Ndb, data: u64, subs: &Subnodes) -> Result<Vec<Raw>, PstError> {
    let heap = Heap::new(ndb.data_blocks(data)?, ndb.ver)?;
    if heap.client_sig != SIG_PC {
        return Err(damaged("an item's properties are not where the index says"));
    }
    let mut out = Vec::new();
    // PC BTH record: wPropId, then wPropType and dwValueHnid.
    for (k, d) in bth(&heap, heap.user_root)? {
        let (Some(id), Some(ty), Some(value)) = (u16_at(&k, 0), u16_at(&d, 0), u32_at(&d, 2)) else { continue };
        let inline = ptype::fixed_size(ty).is_some_and(|n| n <= 4);
        let data = if inline { value.to_le_bytes().to_vec() } else { hnid(ndb, &heap, subs, value) };
        out.push(Raw { id, ty, data });
    }
    Ok(out)
}

pub(crate) struct Row {
    pub id: u32,
    pub cells: Vec<Raw>,
}

/// Read a Table Context ([MS-PST] 2.3.4), rows in table order.
pub(crate) fn read_tc(ndb: &mut Ndb, data: u64, subs: &Subnodes) -> Result<Vec<Row>, PstError> {
    let heap = Heap::new(ndb.data_blocks(data)?, ndb.ver)?;
    if heap.client_sig != SIG_TC {
        return Err(damaged("a table is not where the index says"));
    }
    // TCINFO: bType, cCols, rgib[4], hidRowIndex, hnidRows, hidIndex, then one TCOLDESC per column.
    let info = heap.alloc(heap.user_root).ok_or_else(|| damaged("a table has no header"))?;
    if info.first() != Some(&SIG_TC) {
        return Err(damaged("a table header does not start with its signature"));
    }
    let cols = usize::from(*info.get(1).unwrap_or(&0));
    let (Some(ceb), Some(row_size), Some(row_index), Some(rows_hnid)) = (u16_at(info, 6), u16_at(info, 8), u32_at(info, 10), u32_at(info, 14)) else {
        return Err(damaged("a table header is cut short"));
    };
    let (ceb, row_size) = (usize::from(ceb), usize::from(row_size));
    // TCOLDESC: tag (type in the low word, id in the high), ibData, cbData, iBit.
    let columns: Vec<(u16, u16, usize, usize, usize)> = (0..cols)
        .filter_map(|i| {
            let d = info.get(22 + i * 8..30 + i * 8)?;
            let tag = u32_at(d, 0)?;
            Some(((tag >> 16) as u16, tag as u16, usize::from(u16_at(d, 4)?), usize::from(*d.get(6)?), usize::from(*d.get(7)?)))
        })
        .collect();
    if row_size == 0 || rows_hnid == 0 {
        return Ok(Vec::new());
    }
    // The row matrix: rows never span a block ([MS-PST] 2.3.4.4.1).
    let blocks: Vec<Vec<u8>> = if rows_hnid & 0x1F == 0 {
        vec![heap.alloc(rows_hnid).map(<[u8]>::to_vec).unwrap_or_default()]
    } else {
        match subs.get(&rows_hnid) {
            Some((data, _)) => ndb.data_blocks(*data)?,
            None => Vec::new(),
        }
    };
    let matrix: Vec<&[u8]> = blocks.iter().flat_map(|b| b.chunks_exact(row_size)).take(MAX_ROWS).collect();
    // The row index maps dwRowID to the row's position; a row it does not name is not in the table.
    let mut order: Vec<(usize, u32)> = bth(&heap, row_index)?
        .iter()
        .filter_map(|(k, d)| {
            let index = if d.len() >= 4 { u32_at(d, 0)? as usize } else { usize::from(u16_at(d, 0)?) };
            Some((index, u32_at(k, 0)?))
        })
        .collect();
    order.sort_unstable();
    let mut out = Vec::with_capacity(order.len().min(matrix.len()));
    for (index, id) in order {
        let Some(row) = matrix.get(index) else { continue };
        let mut cells = Vec::new();
        for &(pid, ty, at, size, bit) in &columns {
            // Cell existence block: one bit per column, most significant bit first.
            let exists = row.get(ceb + bit / 8).is_some_and(|b| b & (1 << (7 - bit % 8)) != 0);
            let Some(cell) = row.get(at..at + size).filter(|_| exists) else { continue };
            // Values of up to 8 bytes sit in the row; the rest are HNIDs ([MS-PST] 2.3.4.4.2).
            let inline = ptype::fixed_size(ty).is_some_and(|n| n <= 8);
            let data = match (inline, u32_at(cell, 0)) {
                (true, _) => cell.to_vec(),
                (false, Some(h)) => hnid(ndb, &heap, subs, h),
                (false, None) => continue,
            };
            cells.push(Raw { id: pid, ty, data });
        }
        out.push(Row { id, cells });
    }
    Ok(out)
}

/// Turn a stored value into a typed one. `cp` is the code page of 8-bit strings.
pub(crate) fn decode(raw: Raw, cp: u32) -> Option<(u16, PropValue)> {
    let Raw { id, ty, data } = raw;
    let strip = |s: String| s.trim_end_matches('\0').to_string();
    let value = match ty {
        _ if ptype::fixed_size(ty).is_some() => PropValue::from_fixed(ty, &data)?,
        ptype::STRING => PropValue::String(strip(codepage::utf16le(&data))),
        ptype::STRING8 => PropValue::String(strip(codepage::decode(cp, &data))),
        ptype::BINARY => PropValue::Binary(data),
        ptype::MULTI_INT32 => {
            PropValue::MultiInt32(data.chunks_exact(4).take(MAX_VALUES).filter_map(|c| Some(i32::from_le_bytes(c.try_into().ok()?))).collect())
        }
        ptype::MULTI_STRING => PropValue::MultiString(multi(&data).into_iter().map(|v| strip(codepage::utf16le(v))).collect()),
        ptype::MULTI_STRING8 => PropValue::MultiString(multi(&data).into_iter().map(|v| strip(codepage::decode(cp, v))).collect()),
        ptype::MULTI_BINARY => PropValue::MultiBinary(multi(&data).into_iter().map(<[u8]>::to_vec).collect()),
        _ => PropValue::Other { ty, raw: data },
    };
    Some((id, value))
}

/// A multi-valued variable-size property ([MS-PST] 2.3.3.4.2): ulCount, one offset per value, data.
fn multi(data: &[u8]) -> Vec<&[u8]> {
    let count = u32_at(data, 0).map_or(0, |n| n as usize).min(MAX_VALUES);
    let offset = |i: usize| if i == count { Some(data.len()) } else { u32_at(data, 4 + i * 4).map(|o| o as usize) };
    (0..count).filter_map(|i| data.get(offset(i)?..offset(i + 1)?)).collect()
}
