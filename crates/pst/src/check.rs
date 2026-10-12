//! Checking a data file's structure without opening it as a mailbox: the header and its
//! checksums, the allocation maps, every page of the two B-trees, every block's trailer and
//! checksum, and the reference counts ([MS-PST] 2.2.2). It only reads.
//!
//! Reading a mailbox follows the two B-trees and trusts what it finds; this looks at all of it,
//! so a file we wrote can be held against the specification, and a damaged file says where.

use crate::PstError;
use crate::ndb::{u16_at, u32_at, u64_at};
use crate::write::{
    BT_AREA, DLIST, FIRST_AMAP, MAX_DATA, PAGE, PAGE_BODY, PT_AMAP, PT_BBT, PT_DLIST, PT_FMAP, PT_FPMAP, PT_NBT, PT_PMAP, SECTION, SLOT, TRAILER,
    crc, maps, section_base, sig,
};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};

/// No more than this many problems are spelled out; the rest are counted.
const MAX_PROBLEMS: usize = 50;
const MAX_SECTIONS: u64 = 1 << 22;
const MAX_PAGES: usize = 4_000_000;
const MAX_ENTRIES: usize = 40_000_000;
const MAX_DEPTH: u8 = 8;

/// What a check found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Check {
    /// B-tree and map pages read.
    pub pages: u64,
    /// Blocks read, each with its checksum.
    pub blocks: u64,
    /// Nodes in the node tree.
    pub nodes: u64,
    /// Bytes the allocation maps call free.
    pub free: u64,
    /// What is wrong, in the order it was found; empty for a sound file.
    pub problems: Vec<String>,
    /// Problems found beyond the ones listed.
    pub more: u64,
}

impl Check {
    pub fn sound(&self) -> bool {
        self.problems.is_empty()
    }

    fn problem(&mut self, what: String) {
        if self.problems.len() < MAX_PROBLEMS {
            self.problems.push(what);
        } else {
            self.more = self.more.saturating_add(1);
        }
    }
}

#[derive(Clone, Copy)]
struct Block {
    ib: u64,
    cb: u16,
    refs: u16,
}

struct Checker<'a, R: Read + Seek> {
    src: &'a mut R,
    len: u64,
    eof: u64,
    out: Check,
    /// The allocation map of each section, its 496 bytes.
    amaps: Vec<Vec<u8>>,
    blocks: HashMap<u64, Block>,
    /// References found to each block.
    found: HashMap<u64, u32>,
    walked: HashSet<u64>,
    pages: HashSet<u64>,
}

impl<R: Read + Seek> Checker<'_, R> {
    fn read(&mut self, at: u64, len: usize) -> Option<Vec<u8>> {
        if at.checked_add(len as u64)? > self.len {
            return None;
        }
        let mut buf = vec![0u8; len];
        self.src.seek(SeekFrom::Start(at)).ok()?;
        self.src.read_exact(&mut buf).ok()?;
        Some(buf)
    }

    /// Whether the allocation maps call `len` bytes at `ib` used. A bit stands for 64 bytes,
    /// the first slot of a section being the top bit of the map's first byte.
    fn allocated(&self, ib: u64, len: usize) -> bool {
        let Some(off) = ib.checked_sub(FIRST_AMAP) else { return false };
        let Some(map) = usize::try_from(off / SECTION as u64).ok().and_then(|i| self.amaps.get(i)) else { return false };
        let first = (off % SECTION as u64) as usize / SLOT;
        let count = len.div_ceil(SLOT);
        (first..first.saturating_add(count)).all(|n| map.get(n / 8).is_some_and(|b| b & (0x80 >> (n % 8)) != 0))
    }

    /// A page with a sound trailer ([MS-PST] 2.2.2.7.1), or nothing and a problem noted.
    fn page(&mut self, ib: u64, ptype: u8, bid: Option<u64>, what: &str) -> Option<Vec<u8>> {
        let Some(page) = self.read(ib, PAGE) else {
            self.out.problem(format!("{what} at {ib:#x} lies outside the file"));
            return None;
        };
        self.out.pages += 1;
        let (kind, again) = (page.get(PAGE_BODY).copied()?, page.get(PAGE_BODY + 1).copied()?);
        if kind != ptype || again != ptype {
            self.out.problem(format!("{what} at {ib:#x} is marked as page type {kind:#x}"));
            return None;
        }
        if u32_at(&page, PAGE_BODY + 4)? != crc(page.get(..PAGE_BODY)?) {
            self.out.problem(format!("{what} at {ib:#x} does not match its checksum"));
            return None;
        }
        let own = u64_at(&page, PAGE_BODY + 8)?;
        let wsig = u16_at(&page, PAGE_BODY + 2)?;
        match bid {
            // A map page: its id is its offset.
            None if own != ib => self.out.problem(format!("{what} at {ib:#x} carries the id {own:#x}")),
            Some(bid) if own != bid => self.out.problem(format!("{what} at {ib:#x} carries the id {own:#x}, not {bid:#x}")),
            Some(bid) if wsig != sig(ib, bid) => self.out.problem(format!("{what} at {ib:#x} has the wrong signature")),
            _ => {}
        }
        Some(page)
    }

    fn maps(&mut self) {
        let sections = (self.eof.saturating_sub(FIRST_AMAP) / SECTION as u64).min(MAX_SECTIONS);
        for index in 0..sections {
            let base = section_base(index);
            let bits = self.page(base, PT_AMAP, None, "the allocation map").and_then(|p| p.get(..PAGE_BODY).map(<[u8]>::to_vec));
            let bits = bits.unwrap_or_else(|| vec![0xFF; PAGE_BODY]);
            let (pmap, fmap, fpmap) = maps(index);
            let mut at = base + PAGE as u64;
            for (there, ptype, what) in [(pmap, PT_PMAP, "the page map"), (fmap, PT_FMAP, "the free map"), (fpmap, PT_FPMAP, "the free page map")] {
                if there {
                    self.page(at, ptype, None, what);
                    at += PAGE as u64;
                }
            }
            let head = usize::try_from(at - base).unwrap_or(PAGE);
            self.amaps.push(bits);
            if !self.allocated(base, head) {
                self.out.problem(format!("the allocation map at {base:#x} does not cover the map pages"));
            }
            let free: u64 = self.amaps.last().map_or(0, |m| m.iter().map(|b| u64::from(b.count_zeros())).sum());
            self.out.free = self.out.free.saturating_add(free * SLOT as u64);
        }
    }

    /// Every leaf entry of a B-tree ([MS-PST] 2.2.2.7.7), in order.
    fn tree(&mut self, bid: u64, ib: u64, ptype: u8, width: usize, what: &str) -> Vec<Vec<u8>> {
        let mut leaves = Vec::new();
        let mut last: Option<u64> = None;
        self.branch(bid, ib, ptype, width, what, None, &mut leaves, &mut last);
        leaves
    }

    #[allow(clippy::too_many_arguments)]
    fn branch(
        &mut self,
        bid: u64,
        ib: u64,
        ptype: u8,
        width: usize,
        what: &str,
        level: Option<u8>,
        leaves: &mut Vec<Vec<u8>>,
        last: &mut Option<u64>,
    ) {
        if self.pages.len() >= MAX_PAGES || leaves.len() >= MAX_ENTRIES {
            return;
        }
        if !ib.is_multiple_of(PAGE as u64) {
            self.out.problem(format!("{what} has a page at {ib:#x}, which is no page boundary"));
            return;
        }
        if !self.pages.insert(ib) {
            self.out.problem(format!("{what} reaches the page at {ib:#x} twice"));
            return;
        }
        let Some(page) = self.page(ib, ptype, Some(bid), what) else { return };
        if !self.allocated(ib, PAGE) {
            self.out.problem(format!("{what} has a page at {ib:#x} that the allocation map calls free"));
        }
        let field = |i: usize| page.get(BT_AREA + i).copied().unwrap_or(0);
        let (count, max, size, depth) = (usize::from(field(0)), usize::from(field(1)), usize::from(field(2)), field(3));
        if level.is_some_and(|l| l != depth) || depth > MAX_DEPTH {
            self.out.problem(format!("{what} has a page at {ib:#x} on the wrong level"));
            return;
        }
        let want = if depth == 0 { width } else { 24 };
        if size != want || count > max || count.saturating_mul(size) > BT_AREA {
            self.out.problem(format!("{what} has a page at {ib:#x} with entries that do not fit it"));
            return;
        }
        for i in 0..count {
            let Some(entry) = page.get(i * size..(i + 1) * size) else { return };
            let Some(key) = u64_at(entry, 0) else { return };
            if depth == 0 {
                if last.is_some_and(|l| l >= key) {
                    self.out.problem(format!("{what} has the key {key:#x} out of order at {ib:#x}"));
                }
                *last = Some(key);
                leaves.push(entry.to_vec());
            } else {
                let (Some(kid), Some(at)) = (u64_at(entry, 8), u64_at(entry, 16)) else { return };
                self.branch(kid, at, ptype, width, what, Some(depth - 1), leaves, last);
            }
        }
    }

    /// One block against its trailer ([MS-PST] 2.2.2.8.1); its bytes as stored.
    fn block(&mut self, bid: u64, b: Block) -> Option<Vec<u8>> {
        let cb = usize::from(b.cb);
        let size = (cb + TRAILER).next_multiple_of(SLOT);
        let off = b.ib.checked_sub(FIRST_AMAP).map(|o| o % SECTION as u64);
        if cb == 0 || cb > MAX_DATA || !b.ib.is_multiple_of(SLOT as u64) || off.is_none_or(|o| o < PAGE as u64 || o + size as u64 > SECTION as u64) {
            self.out.problem(format!("the block {bid:#x} at {:#x} with {cb} bytes is not where a block can be", b.ib));
            return None;
        }
        let Some(raw) = self.read(b.ib, size) else {
            self.out.problem(format!("the block {bid:#x} at {:#x} lies outside the file", b.ib));
            return None;
        };
        self.out.blocks += 1;
        let t = size - TRAILER;
        let data = raw.get(..cb)?;
        if u16_at(&raw, t) != Some(b.cb) || u64_at(&raw, t + 8) != Some(bid) {
            self.out.problem(format!("the block {bid:#x} at {:#x} has a trailer for another block", b.ib));
            return None;
        }
        if u16_at(&raw, t + 2) != Some(sig(b.ib, bid)) {
            self.out.problem(format!("the block {bid:#x} at {:#x} has the wrong signature", b.ib));
        }
        if u32_at(&raw, t + 4) != Some(crc(data)) {
            self.out.problem(format!("the block {bid:#x} at {:#x} does not match its checksum", b.ib));
            return None;
        }
        if !self.allocated(b.ib, size) {
            self.out.problem(format!("the block {bid:#x} at {:#x} is in space the allocation map calls free", b.ib));
        }
        Some(data.to_vec())
    }

    /// Count a reference to `bid` and, the first time, the references it holds itself: an
    /// internal block points at others ([MS-PST] 2.2.2.8.3.2, 2.2.2.8.3.3).
    fn refer(&mut self, bid: u64, bodies: &HashMap<u64, Vec<u8>>, depth: u8, from: &str) {
        if bid == 0 {
            return;
        }
        if !self.blocks.contains_key(&bid) {
            self.out.problem(format!("{from} names the block {bid:#x}, which the block tree does not have"));
            return;
        }
        *self.found.entry(bid).or_insert(0) += 1;
        if bid & 2 == 0 || !self.walked.insert(bid) || depth >= MAX_DEPTH {
            return;
        }
        let Some(body) = bodies.get(&bid) else { return };
        let (kind, level, count) = (body.first().copied().unwrap_or(0), body.get(1).copied().unwrap_or(0), usize::from(u16_at(body, 2).unwrap_or(0)));
        let from = format!("the block {bid:#x}");
        let width = match (kind, level) {
            (1, 1 | 2) => 8,
            (2, 0) => 24,
            (2, 1) => 16,
            _ => {
                self.out.problem(format!("{from} is internal and is neither a data tree nor a subnode tree"));
                return;
            }
        };
        if 8 + count * width > body.len() {
            self.out.problem(format!("{from} counts more entries than it holds"));
            return;
        }
        for i in 0..count {
            let at = 8 + i * width;
            match width {
                8 => self.refer(u64_at(body, at).unwrap_or(0), bodies, depth + 1, &from),
                16 => self.refer(u64_at(body, at + 8).unwrap_or(0), bodies, depth + 1, &from),
                _ => {
                    self.refer(u64_at(body, at + 8).unwrap_or(0), bodies, depth + 1, &from);
                    self.refer(u64_at(body, at + 16).unwrap_or(0), bodies, depth + 1, &from);
                }
            }
        }
    }
}

/// Check the structure of a Unicode data file with 512-byte pages (what Outlook 2003 to 2010
/// write as `.pst`, and what this crate writes). Other kinds are not checked yet and say so.
pub fn check<R: Read + Seek>(src: &mut R) -> Result<Check, PstError> {
    let len = src.seek(SeekFrom::End(0))?;
    let mut c = Checker {
        src,
        len,
        eof: 0,
        out: Check::default(),
        amaps: Vec::new(),
        blocks: HashMap::new(),
        found: HashMap::new(),
        walked: HashSet::new(),
        pages: HashSet::new(),
    };
    let head = c.read(0, 564).ok_or(PstError::NotPst)?;
    if head.get(..4) != Some(b"!BDN") {
        return Err(PstError::NotPst);
    }
    match u16_at(&head, 10) {
        Some(23) => {}
        Some(v) => return Err(PstError::Unsupported(format!("the structure check covers Unicode files with 512-byte pages; this is format {v}"))),
        None => return Err(PstError::NotPst),
    }
    // HEADER ([MS-PST] 2.2.2.6).
    if u32_at(&head, 4) != head.get(8..479).map(crc) {
        c.out.problem("the header does not match its first checksum".into());
    }
    if u32_at(&head, 524) != head.get(8..524).map(crc) {
        c.out.problem("the header does not match its full checksum".into());
    }
    if head.get(512) != Some(&0x80) {
        c.out.problem("the header's sentinel byte is not 0x80".into());
    }
    if head.get(513).is_none_or(|m| *m > 2) {
        c.out.problem("the header names an encoding that does not exist".into());
    }
    if head.get(248).is_none_or(|v| *v == 0) {
        c.out.problem("the allocation maps are marked as not valid: the file was not closed properly".into());
    }
    let eof = u64_at(&head, 184).unwrap_or(0);
    let last = u64_at(&head, 192).unwrap_or(0);
    c.eof = eof.min(len);
    if eof != len {
        c.out.problem(format!("the header says the file ends at {eof:#x}; it is {len:#x} long"));
    }
    if eof < FIRST_AMAP + SECTION as u64 || !(eof - FIRST_AMAP).is_multiple_of(SECTION as u64) {
        c.out.problem(format!("the end of the file at {eof:#x} is not a whole number of sections"));
    } else if last != eof - SECTION as u64 {
        c.out.problem(format!("the last allocation map is said to be at {last:#x}"));
    }
    c.maps();
    if u64_at(&head, 200) != Some(c.out.free) {
        c.out.problem(format!("the header counts {} free bytes; the allocation maps have {}", u64_at(&head, 200).unwrap_or(0), c.out.free));
    }
    // The density list is optional; when its page type is there, it has to be sound.
    if c.read(DLIST + PAGE_BODY as u64, 1).is_some_and(|b| b.first() == Some(&PT_DLIST))
        && let Some(bid) = c.read(DLIST + PAGE_BODY as u64 + 8, 8).and_then(|b| u64_at(&b, 0))
    {
        c.page(DLIST, PT_DLIST, Some(bid), "the density list");
    }

    let (nbt_bid, nbt_ib) = (u64_at(&head, 216).unwrap_or(0), u64_at(&head, 224).unwrap_or(0));
    let (bbt_bid, bbt_ib) = (u64_at(&head, 232).unwrap_or(0), u64_at(&head, 240).unwrap_or(0));
    // BBTENTRY ([MS-PST] 2.2.2.7.7.3). Bit 0 of a block id is not part of the key.
    let mut order: Vec<(u64, u64)> = Vec::new();
    for e in c.tree(bbt_bid, bbt_ib, PT_BBT, 24, "the block tree") {
        let (Some(bid), Some(ib), Some(cb), Some(refs)) = (u64_at(&e, 0), u64_at(&e, 8), u16_at(&e, 16), u16_at(&e, 18)) else { continue };
        c.blocks.insert(bid & !1, Block { ib, cb, refs });
        order.push((ib, bid & !1));
    }
    let next_block = u64_at(&head, 516).unwrap_or(0);
    if let Some(top) = c.blocks.keys().max().filter(|top| **top >= next_block) {
        c.out.problem(format!("the header would hand out the block id {next_block:#x} again; {top:#x} is in use"));
    }
    let nodes = c.tree(nbt_bid, nbt_ib, PT_NBT, 32, "the node tree");
    let mut pages: Vec<u64> = c.pages.iter().copied().collect();
    pages.sort_unstable();
    // Blocks in file order: none may run into the next, or into a page of a tree.
    order.sort_unstable();
    let mut bodies: HashMap<u64, Vec<u8>> = HashMap::new();
    let mut end = 0u64;
    for (ib, bid) in order {
        let Some(b) = c.blocks.get(&bid).copied() else { continue };
        if ib < end {
            c.out.problem(format!("the block {bid:#x} at {ib:#x} overlaps the block before it"));
        }
        end = ib.saturating_add(((usize::from(b.cb) + TRAILER).next_multiple_of(SLOT)) as u64);
        let after = pages.partition_point(|p| p + PAGE as u64 <= ib);
        if pages.get(after).is_some_and(|p| *p < end) {
            c.out.problem(format!("the block {bid:#x} at {ib:#x} overlaps a page of a tree"));
        }
        // Only internal blocks are kept: they say which blocks they point at.
        if let Some(data) = c.block(bid, b)
            && bid & 2 != 0
        {
            bodies.insert(bid, data);
        }
    }

    // NBTENTRY ([MS-PST] 2.2.2.7.7.4).
    for e in nodes {
        let (Some(nid), Some(data), Some(sub)) = (u64_at(&e, 0), u64_at(&e, 8), u64_at(&e, 16)) else { continue };
        c.out.nodes += 1;
        let from = format!("the node {nid:#x}");
        c.refer(data & !1, &bodies, 0, &from);
        c.refer(sub & !1, &bodies, 0, &from);
    }
    // A block counts one reference for its own entry and one for each place that names it
    // ([MS-PST] 2.2.2.7.7.3.1).
    let mut off: Vec<(u64, u16, u32)> = Vec::new();
    for (bid, b) in &c.blocks {
        let want = c.found.get(bid).copied().unwrap_or(0).saturating_add(1);
        if u32::from(b.refs) != want {
            off.push((*bid, b.refs, want));
        }
    }
    off.sort_unstable();
    if let Some((bid, has, want)) = off.first() {
        c.out.problem(format!(
            "{} blocks count other references than were found; the first is {bid:#x} with {has} where {want} were found",
            off.len()
        ));
    }
    Ok(c.out)
}
