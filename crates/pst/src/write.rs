//! Writing the Node Database layer ([MS-PST] 2.2): a whole Unicode data file from its blocks and
//! nodes, front to back.
//!
//! A file is always written whole, to a place of the caller's choosing (a temporary file that is
//! swapped in when it is complete); nothing here changes a file in place. Space is handed out in
//! file order, so the allocation maps are simply "everything up to here": each 253,952-byte
//! section ([MS-PST] 2.2.2.7.2) is built in memory and written once, with its map in front.
//!
//! A block that is byte for byte the same as one already written is not written again: the first
//! one gains a reference ([MS-PST] 2.2.2.7.7.3.1). The same attachment in fifty messages is one
//! set of blocks.
//!
//! An id the writer hands back (`data`, `stream`, `subnodes`) stands for one reference: it goes
//! into one node or one subnode entry. For a second place, ask again with the same bytes; the
//! writer finds the blocks and counts the reference.

use crate::{PstError, crypt};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};

/// The first allocation map is at 0x4400 and one follows every 496 * 8 * 64 bytes ([MS-PST] 2.2.2.7.2).
pub(crate) const FIRST_AMAP: u64 = 0x4400;
pub(crate) const SECTION: usize = 496 * 8 * 64;
/// The density list's fixed place ([MS-PST] 2.2.2.7.4).
pub(crate) const DLIST: u64 = 0x4200;
pub(crate) const PAGE: usize = 512;
/// A page's bytes before its trailer.
pub(crate) const PAGE_BODY: usize = PAGE - TRAILER;
/// Blocks are multiples of 64 bytes, at most 8 KB with their trailer ([MS-PST] 2.2.2.8).
pub(crate) const SLOT: usize = 64;
pub(crate) const TRAILER: usize = 16;
pub(crate) const MAX_DATA: usize = 8192 - TRAILER;
/// The entry area of a B-tree page ([MS-PST] 2.2.2.7.7.1).
pub(crate) const BT_AREA: usize = 488;

/// Page types ([MS-PST] 2.2.2.7.1).
pub(crate) const PT_BBT: u8 = 0x80;
pub(crate) const PT_NBT: u8 = 0x81;
pub(crate) const PT_FMAP: u8 = 0x82;
pub(crate) const PT_PMAP: u8 = 0x83;
pub(crate) const PT_AMAP: u8 = 0x84;
pub(crate) const PT_FPMAP: u8 = 0x85;
pub(crate) const PT_DLIST: u8 = 0x86;

/// How many block ids fit an XBLOCK, and how many entries a subnode leaf and a subnode index
/// block ([MS-PST] 2.2.2.8.3.2.1, 2.2.2.8.3.3.1.2, 2.2.2.8.3.3.2.2).
const PER_X: usize = (MAX_DATA - 8) / 8;
const PER_SL: usize = (MAX_DATA - 8) / 24;
const PER_SI: usize = (MAX_DATA - 8) / 16;
/// A block's reference count is 16 bits; past this a new copy is written instead.
const MAX_REFS: u16 = 0xFFF0;

/// Which map pages stand at the head of section `index`, after its allocation map: a page map
/// before every eighth ([MS-PST] 2.2.2.7.3), a free map where the 128 bytes in the header and
/// then each 496-byte page run out ([MS-PST] 1.3.2.7), a free page map likewise ([MS-PST]
/// 1.3.2.8). The specification gives the coverage of the last two, not their offsets: these are
/// worked out from the coverage and have not been compared with a file Outlook wrote.
pub(crate) fn maps(index: u64) -> (bool, bool, bool) {
    const FMAP_FIRST: u64 = 128;
    const FMAP_EVERY: u64 = 496;
    const FPMAP_FIRST: u64 = 128 * 8 * 8;
    const FPMAP_EVERY: u64 = 496 * 8 * 8;
    let pmap = index.is_multiple_of(8);
    let fmap = index >= FMAP_FIRST && (index - FMAP_FIRST).is_multiple_of(FMAP_EVERY);
    let fpmap = index >= FPMAP_FIRST && (index - FPMAP_FIRST).is_multiple_of(FPMAP_EVERY);
    (pmap, fmap, fpmap)
}

pub(crate) fn section_base(index: u64) -> u64 {
    FIRST_AMAP.saturating_add(index.saturating_mul(SECTION as u64))
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static CRC: [u32; 256] = crc_table();

/// The checksum of [MS-PST] 5.3: the reflected CRC-32 polynomial, started at zero and not
/// inverted at the end (the specification's eight tables are the one-table sum taken eight
/// bytes at a time).
pub(crate) fn crc(data: &[u8]) -> u32 {
    let mut c = 0u32;
    for b in data {
        c = CRC.get(((c ^ u32::from(*b)) & 0xFF) as usize).copied().unwrap_or(0) ^ (c >> 8);
    }
    c
}

/// The signature of a block or page ([MS-PST] 5.5).
pub(crate) fn sig(ib: u64, bid: u64) -> u16 {
    let x = (ib ^ bid) as u32;
    ((x >> 16) ^ x) as u16
}

fn cannot(what: &str) -> PstError {
    PstError::Write(what.to_string())
}

fn put(buf: &mut [u8], at: usize, bytes: &[u8]) {
    if let Some(slot) = at.checked_add(bytes.len()).and_then(|end| buf.get_mut(at..end)) {
        slot.copy_from_slice(bytes);
    }
}

/// A 512-byte page: its body, then the trailer ([MS-PST] 2.2.2.7.1).
pub(crate) fn page(body: &[u8], ptype: u8, wsig: u16, bid: u64) -> Vec<u8> {
    let mut out = vec![0u8; PAGE];
    put(&mut out, 0, body.get(..PAGE_BODY.min(body.len())).unwrap_or_default());
    let sum = crc(out.get(..PAGE_BODY).unwrap_or_default());
    put(&mut out, PAGE_BODY, &[ptype, ptype]);
    put(&mut out, PAGE_BODY + 2, &wsig.to_le_bytes());
    put(&mut out, PAGE_BODY + 4, &sum.to_le_bytes());
    put(&mut out, PAGE_BODY + 8, &bid.to_le_bytes());
    out
}

/// A map page: its id is its own offset and it carries no signature ([MS-PST] 2.2.2.7.1).
fn map_page(body: &[u8], ptype: u8, ib: u64) -> Vec<u8> {
    page(body, ptype, 0, ib)
}

fn hash(data: &[u8], internal: bool) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ u64::from(internal);
    for b in data {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[derive(Clone, Copy)]
struct Block {
    bid: u64,
    ib: u64,
    cb: u16,
    refs: u16,
}

/// One entry of a subnode tree: a node inside a node.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sub {
    pub nid: u32,
    pub data: u64,
    pub sub: u64,
}

pub(crate) struct NdbWriter<W: Read + Write + Seek> {
    out: W,
    crypt: u8,
    /// The section being filled, from its allocation map on, and its number.
    section: Vec<u8>,
    index: u64,
    /// Free 64-byte slots of every section already written.
    free: Vec<u16>,
    /// Sections that carry a free map, filled in at the end.
    fmaps: Vec<u64>,
    /// Every block, in the order of its id.
    blocks: Vec<Block>,
    /// What a block's bytes hash to, to find one written before.
    seen: HashMap<u64, u32>,
    nodes: Vec<(u32, u64, u64, u32)>,
    next_block: u64,
    next_page: u64,
    /// The highest index used so far for each of the 32 kinds of node id.
    last: [u32; 32],
}

impl<W: Read + Write + Seek> NdbWriter<W> {
    /// `crypt` is how data blocks are encoded: 0 not at all, 1 permuted, 2 cyclic
    /// ([MS-PST] 2.2.2.6 bCryptMethod).
    pub(crate) fn new(out: W, crypt: u8) -> Result<Self, PstError> {
        if crypt > 2 {
            return Err(cannot("an encoding this version does not write"));
        }
        let mut w = Self {
            out,
            crypt,
            section: Vec::with_capacity(SECTION),
            index: 0,
            free: Vec::new(),
            fmaps: Vec::new(),
            blocks: Vec::new(),
            seen: HashMap::new(),
            nodes: Vec::new(),
            next_block: 1,
            next_page: 1,
            last: [0; 32],
        };
        w.open_section();
        Ok(w)
    }

    fn base(&self) -> u64 {
        section_base(self.index)
    }

    fn open_section(&mut self) {
        self.section.clear();
        // The allocation map is filled in when the section is full.
        self.section.resize(PAGE, 0);
        let (pmap, fmap, fpmap) = maps(self.index);
        // The page maps are kept for older readers only: every bit set says "no page free here"
        // ([MS-PST] 2.2.2.7.3.1, 2.2.2.7.6.1).
        if pmap {
            let ib = self.base() + self.section.len() as u64;
            self.section.extend(map_page(&[0xFF; PAGE_BODY], PT_PMAP, ib));
        }
        if fmap {
            self.fmaps.push(self.index);
            self.section.resize(self.section.len() + PAGE, 0);
        }
        if fpmap {
            let ib = self.base() + self.section.len() as u64;
            self.section.extend(map_page(&[0xFF; PAGE_BODY], PT_FPMAP, ib));
        }
    }

    /// Write the section out with its allocation map. Space was handed out in order, so the map
    /// is a run of set bits; the run is rounded up to a whole byte (eight slots, one page), so
    /// that up to 448 bytes at the end of the used part count as used.
    fn flush(&mut self) -> Result<(), PstError> {
        let used = self.section.len().next_multiple_of(PAGE).min(SECTION);
        let mut bits = vec![0u8; PAGE_BODY];
        for b in bits.iter_mut().take(used / SLOT / 8) {
            *b = 0xFF;
        }
        let base = self.base();
        let amap = map_page(&bits, PT_AMAP, base);
        put(&mut self.section, 0, &amap);
        self.section.resize(SECTION, 0);
        self.out.seek(SeekFrom::Start(base))?;
        self.out.write_all(&self.section)?;
        self.free.push(u16::try_from((SECTION - used) / SLOT).unwrap_or(0));
        Ok(())
    }

    /// Room for `size` bytes at a multiple of `align`, in this section or the next.
    fn room(&mut self, size: usize, align: usize) -> Result<usize, PstError> {
        let mut at = self.section.len().next_multiple_of(align);
        if at.saturating_add(size) > SECTION {
            self.flush()?;
            self.index = self.index.checked_add(1).ok_or_else(|| cannot("the file would be too large"))?;
            self.open_section();
            at = self.section.len().next_multiple_of(align);
            if at.saturating_add(size) > SECTION {
                return Err(cannot("a block that does not fit a section"));
            }
        }
        self.section.resize(at + size, 0);
        Ok(at)
    }

    /// Bytes written earlier: from the section in memory, or back from the file.
    fn stored(&mut self, ib: u64, len: usize) -> Result<Vec<u8>, PstError> {
        let base = self.base();
        if ib >= base {
            let at = usize::try_from(ib - base).map_err(|_| cannot("an offset out of range"))?;
            return at.checked_add(len).and_then(|end| self.section.get(at..end)).map(<[u8]>::to_vec).ok_or_else(|| cannot("an offset out of range"));
        }
        let mut buf = vec![0u8; len];
        self.out.seek(SeekFrom::Start(ib))?;
        self.out.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// One block ([MS-PST] 2.2.2.8): its id, and whether it was there already.
    fn block(&mut self, data: &[u8], internal: bool) -> Result<(u64, bool), PstError> {
        if data.is_empty() || data.len() > MAX_DATA {
            return Err(cannot("a block of a size the format does not have"));
        }
        let key = hash(data, internal);
        if let Some(&i) = self.seen.get(&key)
            && let Some(b) = self.blocks.get(i as usize).copied()
            && usize::from(b.cb) == data.len()
            && (b.bid & 2 != 0) == internal
            && b.refs < MAX_REFS
        {
            let mut old = self.stored(b.ib, data.len())?;
            if !internal {
                match self.crypt {
                    1 => crypt::permute_decode(&mut old),
                    2 => crypt::cyclic(&mut old, b.bid as u32),
                    _ => {}
                }
            }
            if old == data {
                if let Some(b) = self.blocks.get_mut(i as usize) {
                    b.refs += 1;
                }
                return Ok((b.bid, true));
            }
        }
        // Block ids go up by four; bit 1 marks an internal block ([MS-PST] 2.2.2.2).
        let bid = self.next_block << 2 | if internal { 2 } else { 0 };
        self.next_block = self.next_block.checked_add(1).filter(|n| *n < 1 << 61).ok_or_else(|| cannot("too many blocks"))?;
        let size = (data.len() + TRAILER).next_multiple_of(SLOT);
        let at = self.room(size, SLOT)?;
        let ib = self.base() + at as u64;
        let mut body = data.to_vec();
        if !internal {
            // Only data blocks are encoded ([MS-PST] 2.2.2.8.3.1.1).
            match self.crypt {
                1 => crypt::permute_encode(&mut body),
                2 => crypt::cyclic(&mut body, bid as u32),
                _ => {}
            }
        }
        let cb = body.len() as u16;
        // BLOCKTRAILER ([MS-PST] 2.2.2.8.1): cb, wSig, dwCRC of the cb bytes as stored, bid.
        let mut trailer = [0u8; TRAILER];
        put(&mut trailer, 0, &cb.to_le_bytes());
        put(&mut trailer, 2, &sig(ib, bid).to_le_bytes());
        put(&mut trailer, 4, &crc(&body).to_le_bytes());
        put(&mut trailer, 8, &bid.to_le_bytes());
        put(&mut self.section, at, &body);
        put(&mut self.section, at + size - TRAILER, &trailer);
        // A block starts with two references: its entry in the block tree and whoever asked for it.
        self.seen.insert(key, u32::try_from(self.blocks.len()).map_err(|_| cannot("too many blocks"))?);
        self.blocks.push(Block { bid, ib, cb, refs: 2 });
        Ok((bid, false))
    }

    /// Give back a reference that was taken for a block which turned out not to be needed.
    fn release(&mut self, bid: u64) {
        if bid == 0 {
            return;
        }
        if let Ok(i) = self.blocks.binary_search_by_key(&bid, |b| b.bid)
            && let Some(b) = self.blocks.get_mut(i)
            && b.refs > 2
        {
            b.refs -= 1;
        }
    }

    /// An internal block that points at others. When the same block exists already, the
    /// references just taken on its children are given back: they are held once, by the one block.
    fn internal(&mut self, body: &[u8], kids: impl Iterator<Item = u64>) -> Result<u64, PstError> {
        let (bid, shared) = self.block(body, true)?;
        if shared {
            for kid in kids {
                self.release(kid);
            }
        }
        Ok(bid)
    }

    /// XBLOCK (level 1) or XXBLOCK (level 2) over `kids`, each with the bytes below it
    /// ([MS-PST] 2.2.2.8.3.2.1, 2.2.2.8.3.2.2).
    fn xblock(&mut self, level: u8, kids: &[(u64, u64)]) -> Result<u64, PstError> {
        let total: u64 = kids.iter().map(|k| k.1).sum();
        let total = u32::try_from(total).map_err(|_| cannot("an item larger than 4 GB"))?;
        let mut body = Vec::with_capacity(8 + kids.len() * 8);
        body.extend([1, level]);
        body.extend((kids.len() as u16).to_le_bytes());
        body.extend(total.to_le_bytes());
        for (bid, _) in kids {
            body.extend(bid.to_le_bytes());
        }
        self.internal(&body, kids.iter().map(|k| k.0))
    }

    /// A node's data from its blocks, each at most 8,176 bytes: one block, or a data tree over
    /// them ([MS-PST] 2.2.2.8.3.2). No data at all is block id 0.
    pub(crate) fn data<I, B>(&mut self, chunks: I) -> Result<u64, PstError>
    where
        I: IntoIterator<Item = B>,
        B: AsRef<[u8]>,
    {
        let mut leaves: Vec<(u64, u64)> = Vec::new();
        for chunk in chunks {
            let chunk = chunk.as_ref();
            if chunk.is_empty() {
                continue;
            }
            if leaves.len() >= PER_X * PER_X {
                return Err(cannot("an item with more blocks than a data tree holds"));
            }
            let (bid, _) = self.block(chunk, false)?;
            leaves.push((bid, chunk.len() as u64));
        }
        match leaves.as_slice() {
            [] => Ok(0),
            [(bid, _)] => Ok(*bid),
            all if all.len() <= PER_X => self.xblock(1, all),
            all => {
                let mut tops = Vec::with_capacity(all.len() / PER_X + 1);
                for group in all.chunks(PER_X) {
                    tops.push((self.xblock(1, group)?, group.iter().map(|k| k.1).sum()));
                }
                self.xblock(2, &tops)
            }
        }
    }

    /// A node's data from one run of bytes, cut where blocks end.
    pub(crate) fn stream(&mut self, bytes: &[u8]) -> Result<u64, PstError> {
        self.data(bytes.chunks(MAX_DATA))
    }

    /// The subnode tree over `subs`: one SLBLOCK, or an SIBLOCK over several
    /// ([MS-PST] 2.2.2.8.3.3). No subnodes is block id 0.
    pub(crate) fn subnodes(&mut self, mut subs: Vec<Sub>) -> Result<u64, PstError> {
        subs.sort_by_key(|s| s.nid);
        if subs.windows(2).any(|w| w.first().map(|s| s.nid) == w.get(1).map(|s| s.nid)) {
            return Err(cannot("two subnodes with one id"));
        }
        if subs.len() > PER_SL * PER_SI {
            return Err(cannot("more subnodes than a subnode tree holds"));
        }
        let mut leaves: Vec<(u32, u64)> = Vec::new();
        for group in subs.chunks(PER_SL) {
            // btype 2, cLevel 0, cEnt, padding; then SLENTRY: nid (in 8 bytes), bidData, bidSub.
            let mut body = Vec::with_capacity(8 + group.len() * 24);
            body.extend([2, 0]);
            body.extend((group.len() as u16).to_le_bytes());
            body.extend([0; 4]);
            for s in group {
                self.note(s.nid);
                body.extend(u64::from(s.nid).to_le_bytes());
                body.extend(s.data.to_le_bytes());
                body.extend(s.sub.to_le_bytes());
            }
            let bid = self.internal(&body, group.iter().flat_map(|s| [s.data, s.sub]))?;
            leaves.push((group.first().map_or(0, |s| s.nid), bid));
        }
        match leaves.as_slice() {
            [] => Ok(0),
            [(_, bid)] => Ok(*bid),
            all => {
                // btype 2, cLevel 1, cEnt, padding; then SIENTRY: nid (in 8 bytes), bid.
                let mut body = Vec::with_capacity(8 + all.len() * 16);
                body.extend([2, 1]);
                body.extend((all.len() as u16).to_le_bytes());
                body.extend([0; 4]);
                for (nid, bid) in all {
                    body.extend(u64::from(*nid).to_le_bytes());
                    body.extend(bid.to_le_bytes());
                }
                self.internal(&body, all.iter().map(|l| l.1))
            }
        }
    }

    fn note(&mut self, nid: u32) {
        if let Some(last) = self.last.get_mut((nid & 0x1F) as usize) {
            *last = (*last).max(nid >> 5);
        }
    }

    /// A top-level node ([MS-PST] 2.2.2.7.7.4). `data` and `sub` are ids this writer gave out.
    pub(crate) fn node(&mut self, nid: u32, parent: u32, data: u64, sub: u64) {
        self.note(nid);
        self.nodes.push((nid, data, sub, parent));
    }

    fn put_page(&mut self, ptype: u8, body: &[u8]) -> Result<(u64, u64), PstError> {
        let at = self.room(PAGE, PAGE)?;
        let ib = self.base() + at as u64;
        // Pages take their ids from a counter of their own ([MS-PST] 2.2.2.6 bidNextP).
        let bid = self.next_page;
        self.next_page += 1;
        let bytes = page(body, ptype, sig(ib, bid), bid);
        put(&mut self.section, at, &bytes);
        Ok((bid, ib))
    }

    /// A B-tree of pages over `leaves` (key, entry bytes of `width`), which are in key order
    /// ([MS-PST] 2.2.2.7.7). Returns the root's BREF.
    fn tree(&mut self, ptype: u8, width: usize, leaves: Vec<(u64, [u8; 32])>) -> Result<(u64, u64), PstError> {
        let mut items = leaves;
        let mut width = width;
        let mut level = 0u8;
        loop {
            let per = BT_AREA / width;
            let mut up: Vec<(u64, [u8; 32])> = Vec::with_capacity(items.len() / per + 1);
            let mut groups: Vec<&[(u64, [u8; 32])]> = items.chunks(per).collect();
            if groups.is_empty() {
                groups.push(&[]);
            }
            for group in groups {
                let mut body = vec![0u8; PAGE_BODY];
                for (i, (_, entry)) in group.iter().enumerate() {
                    put(&mut body, i * width, entry.get(..width).unwrap_or_default());
                }
                // cEnt, cEntMax, cbEnt, cLevel, then four bytes of padding.
                put(&mut body, BT_AREA, &[group.len() as u8, per as u8, width as u8, level]);
                let (bid, ib) = self.put_page(ptype, &body)?;
                // BTENTRY ([MS-PST] 2.2.2.7.7.2): the first key below, then the page's BREF.
                let key = group.first().map_or(0, |g| g.0);
                let mut entry = [0u8; 32];
                put(&mut entry, 0, &key.to_le_bytes());
                put(&mut entry, 8, &bid.to_le_bytes());
                put(&mut entry, 16, &ib.to_le_bytes());
                up.push((key, entry));
            }
            if let [(_, root)] = up.as_slice() {
                let bid = u64::from_le_bytes(root.get(8..16).and_then(|b| b.try_into().ok()).unwrap_or_default());
                let ib = u64::from_le_bytes(root.get(16..24).and_then(|b| b.try_into().ok()).unwrap_or_default());
                return Ok((bid, ib));
            }
            items = up;
            width = 24;
            level = level.checked_add(1).ok_or_else(|| cannot("an index that is too deep"))?;
        }
    }

    /// Write the two B-trees, the maps, the density list and the header, and hand the file back.
    pub(crate) fn finish(mut self) -> Result<W, PstError> {
        self.nodes.sort_by_key(|n| n.0);
        if self.nodes.windows(2).any(|w| w.first().map(|n| n.0) == w.get(1).map(|n| n.0)) {
            return Err(cannot("two nodes with one id"));
        }
        // BBTENTRY ([MS-PST] 2.2.2.7.7.3): BREF, cb, cRef, padding.
        let blocks: Vec<(u64, [u8; 32])> = self
            .blocks
            .iter()
            .map(|b| {
                let mut e = [0u8; 32];
                put(&mut e, 0, &b.bid.to_le_bytes());
                put(&mut e, 8, &b.ib.to_le_bytes());
                put(&mut e, 16, &b.cb.to_le_bytes());
                put(&mut e, 18, &b.refs.to_le_bytes());
                (b.bid, e)
            })
            .collect();
        let (bbt_bid, bbt_ib) = self.tree(PT_BBT, 24, blocks)?;
        // NBTENTRY ([MS-PST] 2.2.2.7.7.4): nid (in 8 bytes), bidData, bidSub, nidParent, padding.
        let nodes: Vec<(u64, [u8; 32])> = self
            .nodes
            .iter()
            .map(|(nid, data, sub, parent)| {
                let mut e = [0u8; 32];
                put(&mut e, 0, &u64::from(*nid).to_le_bytes());
                put(&mut e, 8, &data.to_le_bytes());
                put(&mut e, 16, &sub.to_le_bytes());
                put(&mut e, 24, &parent.to_le_bytes());
                (u64::from(*nid), e)
            })
            .collect();
        let (nbt_bid, nbt_ib) = self.tree(PT_NBT, 32, nodes)?;
        self.flush()?;
        let last_amap = self.base();
        let eof = last_amap + SECTION as u64;

        // Free maps: one byte for each allocation map from this section on, the longest run of
        // free slots there, at most 255 ([MS-PST] 2.2.2.7.5.1).
        for &at in &self.fmaps.clone() {
            let mut body = vec![0u8; PAGE_BODY];
            for (i, b) in body.iter_mut().enumerate() {
                let free = usize::try_from(at).ok().and_then(|at| self.free.get(at + i)).copied().unwrap_or(0);
                *b = free.min(255) as u8;
            }
            let ib = section_base(at) + if maps(at).0 { 2 * PAGE as u64 } else { PAGE as u64 };
            self.out.seek(SeekFrom::Start(ib))?;
            self.out.write_all(&map_page(&body, PT_FMAP, ib))?;
        }

        let mut head = vec![0u8; FIRST_AMAP as usize];
        // Density list ([MS-PST] 2.2.2.7.4.2): the allocation maps with room, the emptiest first.
        // An entry is the map's number in 20 bits and its free slots in 12.
        let mut roomy: Vec<(usize, u16)> = self.free.iter().copied().enumerate().filter(|(i, f)| *f > 0 && *i < 1 << 20).collect();
        roomy.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        roomy.truncate(476 / 4);
        let mut dlist = vec![0u8; PAGE_BODY];
        // DFL_BACKFILL_COMPLETE, the count, padding, then the map the next allocation tries.
        put(&mut dlist, 0, &[1, roomy.len() as u8]);
        put(&mut dlist, 4, &(roomy.first().map_or(0, |r| r.0) as u32).to_le_bytes());
        for (i, (index, free)) in roomy.iter().enumerate() {
            let entry = (*index as u32 & 0xF_FFFF) | u32::from(*free & 0xFFF) << 20;
            put(&mut dlist, 8 + i * 4, &entry.to_le_bytes());
        }
        let dlist_bid = self.next_page;
        self.next_page += 1;
        put(&mut head, DLIST as usize, &page(&dlist, PT_DLIST, sig(DLIST, dlist_bid), dlist_bid));

        // HEADER, Unicode ([MS-PST] 2.2.2.6), with ROOT at 180 ([MS-PST] 2.2.2.5).
        put(&mut head, 0, b"!BDN");
        put(&mut head, 8, b"SM");
        put(&mut head, 10, &23u16.to_le_bytes());
        put(&mut head, 12, &19u16.to_le_bytes());
        put(&mut head, 14, &[1, 1]);
        put(&mut head, 32, &self.next_page.to_le_bytes());
        put(&mut head, 40, &1u32.to_le_bytes());
        // rgnid: for each kind of node id, where the next one starts.
        for kind in 0..32usize {
            let start: u32 = match kind {
                0x03 => 0x4000,
                0x04 => 0x1_0000,
                0x08 => 0x8000,
                _ => 0x400,
            };
            let next = self.last.get(kind).copied().unwrap_or(0).saturating_add(1).max(start);
            put(&mut head, 44 + kind * 4, &next.to_le_bytes());
        }
        let free_bytes: u64 = self.free.iter().map(|f| u64::from(*f) * SLOT as u64).sum();
        put(&mut head, 184, &eof.to_le_bytes());
        put(&mut head, 192, &last_amap.to_le_bytes());
        put(&mut head, 200, &free_bytes.to_le_bytes());
        put(&mut head, 216, &nbt_bid.to_le_bytes());
        put(&mut head, 224, &nbt_ib.to_le_bytes());
        put(&mut head, 232, &bbt_bid.to_le_bytes());
        put(&mut head, 240, &bbt_ib.to_le_bytes());
        // fAMapValid: VALID_AMAP2.
        put(&mut head, 248, &[2]);
        // The two maps kept in the header are no longer used and are all ones.
        put(&mut head, 256, &[0xFF; 256]);
        put(&mut head, 512, &[0x80, self.crypt]);
        put(&mut head, 516, &(self.next_block << 2).to_le_bytes());
        // dwCRCPartial: 471 bytes from offset 8; dwCRCFull: 516 bytes from offset 8.
        let partial = crc(head.get(8..479).unwrap_or_default());
        put(&mut head, 4, &partial.to_le_bytes());
        let full = crc(head.get(8..524).unwrap_or_default());
        put(&mut head, 524, &full.to_le_bytes());
        self.out.seek(SeekFrom::Start(0))?;
        self.out.write_all(&head)?;
        self.out.flush()?;
        Ok(self.out)
    }
}
