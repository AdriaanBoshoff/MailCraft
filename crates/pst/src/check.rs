//! Checking a data file's structure without opening it as a mailbox: the header and its
//! checksums, the allocation maps, every page of the two B-trees, every block's trailer and
//! checksum, and the reference counts ([MS-PST] 2.2.2); then, in a file sound that far, every
//! heap, property list and table inside the nodes ([MS-PST] 2.3, `check_ltp`). It only reads.
//!
//! Reading a mailbox follows the two B-trees and trusts what it finds; this looks at all of it,
//! so a file we wrote can be held against the specification, and a damaged file says where.
//!
//! Three kinds of file are checked: Unicode and ANSI files with 512-byte pages, both as the
//! specification lays them out, and files with 4K pages (wVer 36, the offline stores of
//! Outlook 2013 and later). The last kind is not in the specification: what is held against
//! it here was found by measuring such files, and what could not be measured is not checked.

use crate::PstError;
use crate::ndb::{Ver, u16_at, u32_at, u64_at};
use crate::write::{DLIST, PT_AMAP, PT_BBT, PT_DLIST, PT_FMAP, PT_FPMAP, PT_NBT, PT_PMAP, crc, maps, sig};
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
    /// What is wrong with the file's structure, in the order it was found.
    pub problems: Vec<String>,
    /// What is wrong inside nodes (a heap, a property list, a table), one line for a node.
    /// Looked for only when `problems` is empty.
    pub inside: Vec<String>,
    /// Problems found beyond the ones listed.
    pub more: u64,
}

impl Check {
    pub fn sound(&self) -> bool {
        self.problems.is_empty() && self.inside.is_empty()
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
    /// What the bytes come to once inflated; `cb` again where the kind of file has no such thing.
    inflated: u16,
    refs: u16,
}

/// How one kind of data file is laid out.
#[derive(Clone, Copy)]
struct Geom {
    ver: Ver,
    /// A page, and its bytes before the trailer ([MS-PST] 2.2.2.7.1).
    page: usize,
    body: usize,
    /// Where a map page's bits begin (an ANSI map has four bytes of padding before them,
    /// [MS-PST] 2.2.2.7.2.1), how many bytes of them there are, and the bytes one bit of an
    /// allocation map stands for.
    bits_at: usize,
    bits: usize,
    slot: usize,
    first_amap: u64,
    /// The entry area of a B-tree page; its counts follow it ([MS-PST] 2.2.2.7.7.1).
    area: usize,
    /// A block's trailer, and the most data before it ([MS-PST] 2.2.2.8.1).
    trailer: usize,
    max_data: usize,
    /// The bytes of an id and of an offset ([MS-PST] 2.2.2.2, 2.2.2.3).
    word: usize,
}

impl Geom {
    fn of(wver: u16) -> Option<Self> {
        Some(match wver {
            14 | 15 => Self {
                ver: Ver::Ansi,
                page: 512,
                body: 500,
                bits_at: 4,
                bits: 496,
                slot: 64,
                first_amap: 0x4400,
                area: 496,
                trailer: 12,
                max_data: 8180,
                word: 4,
            },
            23 => Self {
                ver: Ver::Unicode,
                page: 512,
                body: 496,
                bits_at: 0,
                bits: 496,
                slot: 64,
                first_amap: 0x4400,
                area: 488,
                trailer: 16,
                max_data: 8176,
                word: 8,
            },
            // Measured: pages of 4096 bytes with a trailer of 24, the first sixteen of them as
            // in a Unicode file; a map bit for 512 bytes; the first map at 0x22000; blocks of
            // up to 64 KB with a trailer of 24, again the first sixteen as in a Unicode file.
            36 => Self {
                ver: Ver::Unicode4k,
                page: 4096,
                body: 4072,
                bits_at: 0,
                bits: 4072,
                slot: 512,
                first_amap: 0x22000,
                area: 4056,
                trailer: 24,
                max_data: 65536 - 24,
                word: 8,
            },
            _ => return None,
        })
    }

    /// The bytes one allocation map covers, itself included ([MS-PST] 2.2.2.7.2).
    fn section(&self) -> u64 {
        (self.bits * 8 * self.slot) as u64
    }

    /// An id or an offset, the `n`th of them from `at`.
    fn word(&self, b: &[u8], at: usize, n: usize) -> Option<u64> {
        let at = at.checked_add(n.checked_mul(self.word)?)?;
        if self.word == 8 { u64_at(b, at) } else { u32_at(b, at).map(u64::from) }
    }

    /// The checksum and the id in a trailer whose first four bytes are behind `at`: an ANSI
    /// file has the id first, the others the checksum ([MS-PST] 2.2.2.7.1, 2.2.2.8.1).
    fn sum_and_id(&self, b: &[u8], at: usize) -> Option<(u32, u64)> {
        if self.word == 8 { Some((u32_at(b, at + 4)?, u64_at(b, at + 8)?)) } else { Some((u32_at(b, at + 8)?, u64::from(u32_at(b, at + 4)?))) }
    }

    /// A block with its trailer, as many whole slots as that takes.
    fn stored(&self, cb: u16) -> usize {
        (usize::from(cb) + self.trailer).next_multiple_of(self.slot)
    }
}

struct Checker<'a, R: Read + Seek> {
    src: &'a mut R,
    g: Geom,
    len: u64,
    eof: u64,
    out: Check,
    /// The bits of each section's allocation map.
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

    /// Whether the allocation maps call `len` bytes at `ib` used. A bit stands for one slot,
    /// the first slot of a section being the top bit of the map's first byte.
    fn allocated(&self, ib: u64, len: usize) -> bool {
        let Some(off) = ib.checked_sub(self.g.first_amap) else { return false };
        let Some(map) = usize::try_from(off / self.g.section()).ok().and_then(|i| self.amaps.get(i)) else { return false };
        let first = (off % self.g.section()) as usize / self.g.slot;
        let count = len.div_ceil(self.g.slot);
        (first..first.saturating_add(count)).all(|n| map.get(n / 8).is_some_and(|b| b & (0x80 >> (n % 8)) != 0))
    }

    fn section_base(&self, index: u64) -> u64 {
        self.g.first_amap.saturating_add(index.saturating_mul(self.g.section()))
    }

    /// A page with a sound trailer ([MS-PST] 2.2.2.7.1), or nothing and a problem noted.
    fn page(&mut self, ib: u64, ptype: u8, bid: Option<u64>, what: &str) -> Option<Vec<u8>> {
        let body = self.g.body;
        let Some(page) = self.read(ib, self.g.page) else {
            self.out.problem(format!("{what} at {ib:#x} lies outside the file"));
            return None;
        };
        self.out.pages += 1;
        let (kind, again) = (page.get(body).copied()?, page.get(body + 1).copied()?);
        if kind != ptype || again != ptype {
            self.out.problem(format!("{what} at {ib:#x} is marked as page type {kind:#x}"));
            return None;
        }
        let (sum, own) = self.g.sum_and_id(&page, body)?;
        if sum != crc(page.get(..body)?) {
            self.out.problem(format!("{what} at {ib:#x} does not match its checksum"));
            return None;
        }
        let wsig = u16_at(&page, body + 2)?;
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
        let g = self.g;
        let sections = (self.eof.saturating_sub(g.first_amap) / g.section()).min(MAX_SECTIONS);
        for index in 0..sections {
            let base = self.section_base(index);
            let bits = self.page(base, PT_AMAP, None, "the allocation map").and_then(|p| p.get(g.bits_at..g.bits_at + g.bits).map(<[u8]>::to_vec));
            let bits = bits.unwrap_or_else(|| vec![0xFF; g.bits]);
            // The other maps stand where a file with 512-byte pages has them. A file with 4K
            // pages was seen with an allocation map only (and never with a second section,
            // so whether a later one has more at its head is not known).
            let (pmap, fmap, fpmap) = if g.ver == Ver::Unicode4k { (false, false, false) } else { maps(index) };
            let mut at = base + g.page as u64;
            for (there, ptype, what) in [(pmap, PT_PMAP, "the page map"), (fmap, PT_FMAP, "the free map"), (fpmap, PT_FPMAP, "the free page map")] {
                if there {
                    self.page(at, ptype, None, what);
                    at += g.page as u64;
                }
            }
            let head = usize::try_from(at - base).unwrap_or(g.page);
            self.amaps.push(bits);
            if !self.allocated(base, head) {
                self.out.problem(format!("the allocation map at {base:#x} does not cover the map pages"));
            }
            let free: u64 = self.amaps.last().map_or(0, |m| m.iter().map(|b| u64::from(b.count_zeros())).sum());
            self.out.free = self.out.free.saturating_add(free * g.slot as u64);
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
        let g = self.g;
        // A page of a file with 4K pages was seen on a 512-byte boundary, as its blocks are.
        if !ib.is_multiple_of(512) {
            self.out.problem(format!("{what} has a page at {ib:#x}, which is no page boundary"));
            return;
        }
        if !self.pages.insert(ib) {
            self.out.problem(format!("{what} reaches the page at {ib:#x} twice"));
            return;
        }
        let Some(page) = self.page(ib, ptype, Some(bid), what) else { return };
        if !self.allocated(ib, g.page) {
            self.out.problem(format!("{what} has a page at {ib:#x} that the allocation map calls free"));
        }
        // cEnt, cEntMax, cbEnt, cLevel: a byte each, and the first two a word each where a
        // page is 4K (measured).
        let field = |i: usize| page.get(g.area + i).copied().unwrap_or(0);
        let (count, max, size, depth) = if g.ver == Ver::Unicode4k {
            (usize::from(u16_at(&page, g.area).unwrap_or(0)), usize::from(u16_at(&page, g.area + 2).unwrap_or(0)), usize::from(field(4)), field(5))
        } else {
            (usize::from(field(0)), usize::from(field(1)), usize::from(field(2)), field(3))
        };
        if level.is_some_and(|l| l != depth) || depth > MAX_DEPTH {
            self.out.problem(format!("{what} has a page at {ib:#x} on the wrong level"));
            return;
        }
        // BTENTRY ([MS-PST] 2.2.2.7.7.2): a key, an id and an offset.
        let want = if depth == 0 { width } else { g.word * 3 };
        if size != want || count > max || count.saturating_mul(size) > g.area {
            self.out.problem(format!("{what} has a page at {ib:#x} with entries that do not fit it"));
            return;
        }
        for i in 0..count {
            let Some(entry) = page.get(i * size..(i + 1) * size) else { return };
            let Some(key) = g.word(entry, 0, 0) else { return };
            if depth == 0 {
                if last.is_some_and(|l| l >= key) {
                    self.out.problem(format!("{what} has the key {key:#x} out of order at {ib:#x}"));
                }
                *last = Some(key);
                leaves.push(entry.to_vec());
            } else {
                let (Some(kid), Some(at)) = (g.word(entry, 0, 1), g.word(entry, 0, 2)) else { return };
                self.branch(kid, at, ptype, width, what, Some(depth - 1), leaves, last);
            }
        }
    }

    /// One block against its trailer ([MS-PST] 2.2.2.8.1); its bytes as stored.
    fn block(&mut self, bid: u64, b: Block) -> Option<Vec<u8>> {
        let g = self.g;
        let cb = usize::from(b.cb);
        let size = g.stored(b.cb);
        let off = b.ib.checked_sub(g.first_amap).map(|o| o % g.section());
        if cb == 0 || cb > g.max_data || !b.ib.is_multiple_of(g.slot as u64) || off.is_none_or(|o| o < g.page as u64 || o + size as u64 > g.section())
        {
            self.out.problem(format!("the block {bid:#x} at {:#x} with {cb} bytes is not where a block can be", b.ib));
            return None;
        }
        let Some(raw) = self.read(b.ib, size) else {
            self.out.problem(format!("the block {bid:#x} at {:#x} lies outside the file", b.ib));
            return None;
        };
        self.out.blocks += 1;
        let t = size - g.trailer;
        let data = raw.get(..cb)?;
        let (sum, own) = g.sum_and_id(&raw, t)?;
        // The id is held without its lowest bit, which is not part of it ([MS-PST] 2.2.2.2).
        if u16_at(&raw, t) != Some(b.cb) || own & !1 != bid {
            self.out.problem(format!("the block {bid:#x} at {:#x} has a trailer for another block", b.ib));
            return None;
        }
        if u16_at(&raw, t + 2) != Some(sig(b.ib, own)) {
            self.out.problem(format!("the block {bid:#x} at {:#x} has the wrong signature", b.ib));
        }
        // The checksum is of the bytes as they are stored, deflated or not (measured).
        if sum != crc(data) {
            self.out.problem(format!("the block {bid:#x} at {:#x} does not match its checksum", b.ib));
            return None;
        }
        if !self.allocated(b.ib, size) {
            self.out.problem(format!("the block {bid:#x} at {:#x} is in space the allocation map calls free", b.ib));
        }
        if b.inflated == b.cb {
            return Some(data.to_vec());
        }
        // A block of a file with 4K pages may be stored deflated; what it says it comes to
        // is held against what it does come to.
        match miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, 1 << 16) {
            Ok(whole) if whole.len() == usize::from(b.inflated) => Some(whole),
            Ok(whole) => {
                self.out.problem(format!(
                    "the block {bid:#x} at {:#x} inflates to {} bytes, not the {} its entry says",
                    b.ib,
                    whole.len(),
                    b.inflated
                ));
                None
            }
            Err(_) => {
                self.out.problem(format!("the block {bid:#x} at {:#x} is said to be deflated and does not inflate", b.ib));
                None
            }
        }
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
        let g = self.g;
        // Where the entries begin, how many ids or offsets wide one is, and which of those
        // name blocks: an XBLOCK's ids after eight bytes ([MS-PST] 2.2.2.8.3.2.1); an
        // SLENTRY's node, data and subnodes (2.2.2.8.3.3.1.1); an SIENTRY's node and block
        // (2.2.2.8.3.3.2.1). The subnode blocks of an ANSI file have no padding after their
        // count, so their entries begin after four bytes.
        let subs_at = if g.word == 8 { 8 } else { 4 };
        let (start, words, named): (usize, usize, &[usize]) = match (kind, level) {
            (1, 1 | 2) => (8, 1, &[0]),
            (2, 0) => (subs_at, 3, &[1, 2]),
            (2, 1) => (subs_at, 2, &[1]),
            _ => {
                self.out.problem(format!("{from} is internal and is neither a data tree nor a subnode tree"));
                return;
            }
        };
        let width = words * g.word;
        if start + count * width > body.len() {
            self.out.problem(format!("{from} counts more entries than it holds"));
            return;
        }
        for i in 0..count {
            for n in named {
                self.refer(g.word(body, start + i * width, *n).unwrap_or(0) & !1, bodies, depth + 1, &from);
            }
        }
    }
}

/// Check the structure of a data file: a Unicode one with 512-byte pages (what Outlook 2003
/// and later write as `.pst`, and what this crate writes), an ANSI one (what Outlook 97 to
/// 2002 wrote), or one with 4K pages (an `.ost` of Outlook 2013 and later).
pub fn check<R: Read + Seek + Send>(src: &mut R) -> Result<Check, PstError> {
    check_until(src, None)
}

/// `check`, for a caller that may ask it to stop: it looks at `stop` before each block and
/// each node, and gives `PstError::Stopped` when it is told to. A check that was stopped
/// says nothing of the file.
pub fn check_until<R: Read + Seek + Send>(src: &mut R, stop: Option<&openmailclient_model::Progress>) -> Result<Check, PstError> {
    within(src, stop, 0, None)
}

/// The check of a file that was written into from `since` on: a data block that lies
/// before that is where it was and is not read again (that it is in space the maps call
/// used is still seen), every block that names others is read, so every count of
/// references is held against what is found, and only the nodes of `only` are looked into.
pub(crate) fn check_since<R: Read + Seek + Send>(
    src: &mut R,
    stop: Option<&openmailclient_model::Progress>,
    since: u64,
    only: &HashSet<u32>,
) -> Result<Check, PstError> {
    within(src, stop, since, Some(only))
}

/// What is wrong inside the nodes of `only`, as the check says it.
pub(crate) fn inside_of(ndb: &mut crate::ndb::Ndb, only: &HashSet<u32>) -> Vec<String> {
    crate::check_ltp::inside(ndb, MAX_PROBLEMS, &|| false, Some(only)).map(|found| found.0).unwrap_or_default()
}

fn within<R: Read + Seek + Send>(
    src: &mut R,
    stop: Option<&openmailclient_model::Progress>,
    since: u64,
    only: Option<&HashSet<u32>>,
) -> Result<Check, PstError> {
    let stopped = || stop.is_some_and(openmailclient_model::Progress::stopped);
    let mut out = structure(src, &stopped, since)?;
    if out.problems.is_empty() {
        src.seek(SeekFrom::Start(0))?;
        match crate::ndb::Ndb::open(Box::new(&mut *src)) {
            Ok(mut ndb) => {
                let (inside, more) = crate::check_ltp::inside(&mut ndb, MAX_PROBLEMS, &stopped, only)?;
                out.inside = inside;
                out.more = out.more.saturating_add(more);
            }
            Err(e) => out.inside.push(format!("the file cannot be opened as a mailbox: {e}")),
        }
    }
    Ok(out)
}

fn structure<R: Read + Seek>(src: &mut R, stopped: &dyn Fn() -> bool, since: u64) -> Result<Check, PstError> {
    let len = src.seek(SeekFrom::End(0))?;
    let mut start = [0u8; 12];
    src.seek(SeekFrom::Start(0))?;
    if src.read_exact(&mut start).is_err() || start.get(..4) != Some(b"!BDN") {
        return Err(PstError::NotPst);
    }
    let wver = u16_at(&start, 10).ok_or(PstError::NotPst)?;
    let g = Geom::of(wver).ok_or_else(|| PstError::Unsupported(format!("the structure check does not know the file format {wver}")))?;
    let (page, slot, section) = (g.page as u64, g.slot, g.section());
    let mut c = Checker {
        src,
        g,
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
    // HEADER ([MS-PST] 2.2.2.6). An ANSI file has ids and offsets of four bytes, so its root
    // begins earlier and is shorter; it has the first checksum only. Where a field is, in the
    // order: the end of the file, the last allocation map, the free bytes, the node tree's
    // id, the block tree's id (each followed by its offset), whether the maps are valid, the
    // sentinel, the encoding, the next block id.
    let ansi = g.ver == Ver::Ansi;
    let at: [usize; 9] = if ansi { [168, 172, 176, 184, 192, 200, 460, 461, 24] } else { [184, 192, 200, 216, 232, 248, 512, 513, 516] };
    let field = |n: usize| at.get(n).copied().unwrap_or(0);
    if u32_at(&head, 4) != head.get(8..479).map(crc) {
        c.out.problem("the header does not match its first checksum".into());
    }
    if !ansi && u32_at(&head, 524) != head.get(8..524).map(crc) {
        c.out.problem("the header does not match its full checksum".into());
    }
    if head.get(field(6)) != Some(&0x80) {
        c.out.problem("the header's sentinel byte is not 0x80".into());
    }
    if head.get(field(7)).is_none_or(|m| *m > 2) {
        c.out.problem("the header names an encoding that does not exist".into());
    }
    if head.get(field(5)).is_none_or(|v| *v == 0) {
        c.out.problem("the allocation maps are marked as not valid: the file was not closed properly".into());
    }
    let eof = g.word(&head, field(0), 0).unwrap_or(0);
    let last = g.word(&head, field(1), 0).unwrap_or(0);
    c.eof = eof.min(len);
    if eof != len {
        c.out.problem(format!("the header says the file ends at {eof:#x}; it is {len:#x} long"));
    }
    if eof < g.first_amap + section || !(eof - g.first_amap).is_multiple_of(section) {
        c.out.problem(format!("the end of the file at {eof:#x} is not a whole number of sections"));
    } else if last != eof - section {
        c.out.problem(format!("the last allocation map is said to be at {last:#x}"));
    }
    c.maps();
    if g.word(&head, field(2), 0) != Some(c.out.free) {
        c.out.problem(format!("the header counts {} free bytes; the allocation maps have {}", g.word(&head, field(2), 0).unwrap_or(0), c.out.free));
    }
    // The density list is optional; when its page type is there, it has to be sound. It
    // came with the Unicode file, and a file with 4K pages was not seen to have one.
    if g.ver == Ver::Unicode
        && c.read(DLIST + g.body as u64, 1).is_some_and(|b| b.first() == Some(&PT_DLIST))
        && let Some(bid) = c.read(DLIST + g.body as u64 + 8, 8).and_then(|b| u64_at(&b, 0))
    {
        c.page(DLIST, PT_DLIST, Some(bid), "the density list");
    }

    let (nbt_bid, nbt_ib) = (g.word(&head, field(3), 0).unwrap_or(0), g.word(&head, field(3), 1).unwrap_or(0));
    let (bbt_bid, bbt_ib) = (g.word(&head, field(4), 0).unwrap_or(0), g.word(&head, field(4), 1).unwrap_or(0));
    // BBTENTRY ([MS-PST] 2.2.2.7.7.3): id, offset, size, references; a file with 4K pages
    // has the inflated size between the last two (measured). Bit 0 of a block id is not
    // part of the key.
    let mut order: Vec<(u64, u64)> = Vec::new();
    for e in c.tree(bbt_bid, bbt_ib, PT_BBT, g.word * 3, "the block tree") {
        let sizes = g.word * 2;
        let (Some(bid), Some(ib), Some(cb)) = (g.word(&e, 0, 0), g.word(&e, 0, 1), u16_at(&e, sizes)) else { continue };
        let (inflated, refs) =
            if g.ver == Ver::Unicode4k { (u16_at(&e, sizes + 2), u16_at(&e, sizes + 4)) } else { (Some(cb), u16_at(&e, sizes + 2)) };
        let (Some(inflated), Some(refs)) = (inflated, refs) else { continue };
        c.blocks.insert(bid & !1, Block { ib, cb, inflated, refs });
        order.push((ib, bid & !1));
    }
    let next_block = g.word(&head, field(8), 0).unwrap_or(0);
    if let Some(top) = c.blocks.keys().max().filter(|top| **top >= next_block) {
        c.out.problem(format!("the header would hand out the block id {next_block:#x} again; {top:#x} is in use"));
    }
    // NBTENTRY ([MS-PST] 2.2.2.7.7.4): the node, its data, its subnodes, its parent.
    let nodes = c.tree(nbt_bid, nbt_ib, PT_NBT, g.word * 4, "the node tree");
    let mut pages: Vec<u64> = c.pages.iter().copied().collect();
    pages.sort_unstable();
    // Blocks in file order: none may run into the next, or into a page of a tree.
    order.sort_unstable();
    let mut bodies: HashMap<u64, Vec<u8>> = HashMap::new();
    let mut end = 0u64;
    for (ib, bid) in order {
        if stopped() {
            return Err(PstError::Stopped);
        }
        let Some(b) = c.blocks.get(&bid).copied() else { continue };
        if ib < end {
            c.out.problem(format!("the block {bid:#x} at {ib:#x} overlaps the block before it"));
        }
        end = ib.saturating_add(g.stored(b.cb).max(slot) as u64);
        let after = pages.partition_point(|p| p + page <= ib);
        if pages.get(after).is_some_and(|p| *p < end) {
            c.out.problem(format!("the block {bid:#x} at {ib:#x} overlaps a page of a tree"));
        }
        // A data block from before the file was written into is as it was.
        if bid & 2 == 0 && end <= since {
            if !c.allocated(ib, g.stored(b.cb)) {
                c.out.problem(format!("the block {bid:#x} at {ib:#x} is in space the allocation map calls free"));
            }
            c.out.blocks += 1;
            continue;
        }
        // Only internal blocks are kept: they say which blocks they point at.
        if let Some(data) = c.block(bid, b)
            && bid & 2 != 0
        {
            bodies.insert(bid, data);
        }
    }

    for e in nodes {
        if stopped() {
            return Err(PstError::Stopped);
        }
        let (Some(nid), Some(data), Some(sub)) = (g.word(&e, 0, 0), g.word(&e, 0, 1), g.word(&e, 0, 2)) else { continue };
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

#[cfg(test)]
#[path = "tests_check_kinds.rs"]
mod tests;
