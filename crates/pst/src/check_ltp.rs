//! The structure check, inside the nodes: every heap-on-node, BTree-on-heap, Property Context
//! and Table Context ([MS-PST] 2.3) held against what the specification says of it. It only
//! reads, and says the first thing wrong with each node.
//!
//! The reader in `ltp` takes what it can from a damaged heap and goes on; this takes nothing
//! for granted, so that a file we wrote is known to be what another program expects.

use crate::ndb::{Ndb, Subnodes, Ver, u16_at, u32_at};
use openmailclient_model::ptype;
use std::collections::HashSet;

/// Nodes looked into, subnodes below one another, records of one index, rows of one table.
const MAX_NODES: usize = 40_000_000;
const MAX_DEPTH: u8 = 8;
const MAX_RECORDS: usize = 1 << 20;
const MAX_ROWS: usize = 4_000_000;

/// The bytes a data block holds ([MS-PST] 2.2.2.8.3.1): 8 KB less the trailer, which is
/// twelve bytes in an ANSI file and sixteen in a Unicode one. A file with 4K pages is not
/// in the specification, and how full its blocks are said to be is not held against anything.
fn block_size(ver: Ver) -> Option<usize> {
    match ver {
        Ver::Ansi => Some(8180),
        Ver::Unicode => Some(8176),
        Ver::Unicode4k => None,
    }
}

/// How full a block with `free` bytes left is said to be ([MS-PST] 2.3.1.2, rgbFillLevel):
/// 0 with at least 3584 bytes free, and so on to 15 with fewer than 8.
fn level(free: usize) -> u8 {
    const AT_LEAST: [usize; 15] = [3584, 2560, 2048, 1792, 1536, 1280, 1024, 768, 512, 256, 128, 64, 32, 16, 8];
    AT_LEAST.iter().position(|least| free >= *least).unwrap_or(15) as u8
}

const SIG_HEAP: u8 = 0xEC;
const SIG_PC: u8 = 0xBC;
const SIG_TC: u8 = 0x7C;
const SIG_BTH: u8 = 0xB5;

/// The store and the name-to-id map ([MS-PST] 2.4.1): property lists under ids of the
/// internal kind.
const NID_STORE: u32 = 0x21;
const NID_NAMES: u32 = 0x61;

/// What a node holds, by the kind in its id ([MS-PST] 2.2.2.1).
#[derive(Clone, Copy, PartialEq)]
enum Holds {
    Properties,
    Table,
    Other,
}

fn holds(nid: u32) -> Holds {
    match nid & 0x1F {
        // Folders, search folders, messages, attachments, hidden messages.
        0x02..=0x05 | 0x08 => Holds::Properties,
        // The tables of a folder, of a search folder, of attachments and of recipients.
        0x0D..=0x12 => Holds::Table,
        _ if nid == NID_STORE || nid == NID_NAMES => Holds::Properties,
        _ => Holds::Other,
    }
}

/// A heap-on-node whose every block has been held against [MS-PST] 2.3.1.
struct Heap {
    blocks: Vec<Vec<u8>>,
    client: u8,
    root: u32,
    ver: Ver,
}

impl Heap {
    /// The heap of a Unicode file with 512-byte pages.
    #[cfg(test)]
    fn new(blocks: Vec<Vec<u8>>) -> Result<Self, String> {
        Self::of(blocks, Ver::Unicode)
    }

    fn of(blocks: Vec<Vec<u8>>, ver: Ver) -> Result<Self, String> {
        let first = blocks.first().ok_or("its heap has no data")?;
        // HNHDR ([MS-PST] 2.3.1.2): ibHnpm, bSig, bClientSig, hidUserRoot, rgbFillLevel.
        if first.get(2) != Some(&SIG_HEAP) {
            return Err("its heap does not start with the heap's signature".into());
        }
        let client = first.get(3).copied().unwrap_or(0);
        let root = u32_at(first, 4).ok_or("its heap's header is cut short")?;
        for (i, b) in blocks.iter().enumerate() {
            // The first block has the header; every 128th after the eighth has a fill map
            // (HNBITMAPHDR, 2.3.1.4); the others have only the place of their page map (2.3.1.3).
            let head = match i {
                0 => 12,
                _ if i >= 8 && (i - 8).is_multiple_of(128) => 66,
                _ => 2,
            };
            // HNPAGEMAP ([MS-PST] 2.3.1.5): cAlloc, cFree, rgibAlloc[cAlloc + 1].
            let map = usize::from(u16_at(b, 0).ok_or(format!("block {i} of its heap is empty"))?);
            let count = usize::from(u16_at(b, map).ok_or(format!("block {i} of its heap has its page map outside the block"))?);
            if map < head {
                return Err(format!("block {i} of its heap has its page map inside its header"));
            }
            let mut before = head;
            for n in 0..=count {
                let at = usize::from(u16_at(b, map + 4 + n * 2).ok_or(format!("block {i} of its heap has a page map that runs past the block"))?);
                if at < before || at > map {
                    return Err(format!("block {i} of its heap has allocation {} out of order or outside the block", n + 1));
                }
                before = at;
            }
        }
        // Fill levels: four bits a block, the earlier block in the low half of a byte. The
        // first eight are in the header (2.3.1.2); every 128 from the eighth on are in the
        // first of them (2.3.1.4). A block that is not there has level 0.
        let said = |i: usize| -> Option<u8> {
            let (holder, at) = if i < 8 { (0, 8) } else { (8 + (i - 8) / 128 * 128, 2) };
            let byte = *blocks.get(holder)?.get(at + (i - holder) / 2)?;
            Some(if (i - holder).is_multiple_of(2) { byte & 15 } else { byte >> 4 })
        };
        let last = if blocks.len() <= 8 { 8 } else { 8 + (blocks.len() - 8).div_ceil(128) * 128 };
        let size = block_size(ver);
        for i in 0..if size.is_some() { last } else { 0 } {
            let Some(said) = said(i) else { continue };
            let Some(b) = blocks.get(i) else {
                if said != 0 {
                    return Err(format!("its heap gives a fill level to block {i}, which it does not have"));
                }
                continue;
            };
            // What is free after the page map; allocations given up leave more inside, and
            // the map counts those. How the edges of a level are counted by other programs
            // is not known, so a level may be one off; further off it misleads whoever
            // looks for room.
            let is = level(size.unwrap_or(0).saturating_sub(b.len()));
            let given_up = u16_at(b, usize::from(u16_at(b, 0).unwrap_or(0)) + 2).unwrap_or(0);
            if said > is + 1 {
                return Err(format!("block {i} of its heap is said to be fuller than it is (level {said}, and it is at {is})"));
            }
            if given_up == 0 && said + 1 < is {
                return Err(format!("block {i} of its heap is said to have more room than it has (level {said}, and it is at {is})"));
            }
        }
        Ok(Self { blocks, client, root, ver })
    }

    /// One allocation by HID ([MS-PST] 2.3.1.1): five bits of kind (zero), eleven of a
    /// 1-based index, sixteen of the block. A file with 4K pages has more allocations in
    /// its larger blocks: there the index is taken as fourteen bits and the block as
    /// thirteen, as the reader takes them.
    fn alloc(&self, hid: u32) -> Option<&[u8]> {
        if hid & 0x1F != 0 {
            return None;
        }
        let (block, index) = if self.ver == Ver::Unicode4k {
            ((hid >> 19) as usize, ((hid >> 5) & 0x3FFF) as usize)
        } else {
            ((hid >> 16) as usize, ((hid >> 5) & 0x7FF) as usize)
        };
        let b = self.blocks.get(block)?;
        let map = usize::from(u16_at(b, 0)?);
        if index == 0 || index > usize::from(u16_at(b, map)?) {
            return None;
        }
        let at = map + 4 + (index - 1) * 2;
        b.get(usize::from(u16_at(b, at)?)..usize::from(u16_at(b, at + 2)?))
    }
}

/// Every record of a BTree-on-heap ([MS-PST] 2.3.2), held against its header: `key` and
/// `ent` are the sizes its user needs.
fn bth<'a>(heap: &'a Heap, hid: u32, key: usize, ent: usize, what: &str) -> Result<Vec<&'a [u8]>, String> {
    // BTHHEADER ([MS-PST] 2.3.2.1): bType, cbKey, cbEnt, bIdxLevels, hidRoot.
    let header = heap.alloc(hid).ok_or(format!("{what} is not in the heap"))?;
    if header.len() != 8 || header.first() != Some(&SIG_BTH) {
        return Err(format!("{what} does not start with an index header"));
    }
    let levels = header.get(3).copied().unwrap_or(0);
    if header.get(1).map(|k| usize::from(*k)) != Some(key) || header.get(2).map(|e| usize::from(*e)) != Some(ent) {
        return Err(format!("{what} has keys or entries of the wrong size"));
    }
    let root = u32_at(header, 4).unwrap_or(0);
    let mut out: Vec<&[u8]> = Vec::new();
    if root == 0 {
        return Ok(out);
    }
    if levels > 8 {
        return Err(format!("{what} has more levels than an index can have"));
    }
    let mut stack = vec![(root, levels)];
    let mut seen = 0usize;
    while let Some((hid, level)) = stack.pop() {
        seen += 1;
        if seen > MAX_RECORDS {
            return Err(format!("{what} is implausibly large"));
        }
        let data = heap.alloc(hid).ok_or(format!("{what} points at {hid:#x}, which is not in the heap"))?;
        let width = if level == 0 { key + ent } else { key + 4 };
        if !data.len().is_multiple_of(width) {
            return Err(format!("{what} has records that are cut short"));
        }
        if level == 0 {
            for rec in data.chunks_exact(width) {
                // Keys are numbers stored low byte first, and ascend.
                let ascends = out.last().is_none_or(|last| last.get(..key).map(number) < rec.get(..key).map(number));
                if !ascends {
                    return Err(format!("{what} has keys out of order"));
                }
                out.push(rec);
                if out.len() > MAX_RECORDS {
                    return Err(format!("{what} is implausibly large"));
                }
            }
        } else {
            let kids: Vec<u32> = data.chunks_exact(width).filter_map(|rec| u32_at(rec, key)).collect();
            stack.extend(kids.into_iter().rev().map(|k| (k, level - 1)));
        }
    }
    Ok(out)
}

fn number(key: &[u8]) -> u128 {
    key.iter().rev().take(16).fold(0, |n, b| (n << 8) | u128::from(*b))
}

/// What an HNID names has to be there ([MS-PST] 2.3.3.2): an allocation of the heap, or a
/// subnode. Zero names nothing, which is an empty value.
fn named<'a>(heap: &'a Heap, subs: &Subnodes, id: u32, what: &str) -> Result<Option<&'a [u8]>, String> {
    if id == 0 {
        return Ok(None);
    }
    if id & 0x1F == 0 {
        return heap.alloc(id).map(Some).ok_or(format!("{what} is kept at {id:#x}, which is not in the heap"));
    }
    if subs.contains_key(&id) { Ok(None) } else { Err(format!("{what} is kept in the subnode {id:#x}, which is not there")) }
}

/// A value of several strings or byte runs ([MS-PST] 2.3.3.4.2): a count, where each
/// starts, and the values, in order and inside the allocation.
fn several(data: &[u8], what: &str) -> Result<(), String> {
    let count = u32_at(data, 0).ok_or(format!("{what} has no count"))? as usize;
    let mut before = count.checked_add(1).and_then(|n| n.checked_mul(4)).ok_or(format!("{what} counts more values than it can hold"))?;
    for i in 0..count {
        let at = u32_at(data, 4 + i * 4).ok_or(format!("{what} counts more values than it holds"))? as usize;
        if at < before || at > data.len() {
            return Err(format!("{what} has value {i} out of order or outside the allocation"));
        }
        before = at;
    }
    Ok(())
}

/// A Property Context ([MS-PST] 2.3.3).
fn properties(heap: &Heap, subs: &Subnodes) -> Result<(), String> {
    if heap.client != SIG_PC {
        return Err("it is to hold properties, and its heap is of another kind".into());
    }
    // A record ([MS-PST] 2.3.3.3): wPropId, wPropType, dwValueHnid.
    for rec in bth(heap, heap.root, 2, 6, "its property index")? {
        let (Some(id), Some(ty), Some(value)) = (u16_at(rec, 0), u16_at(rec, 2), u32_at(rec, 4)) else { continue };
        let what = format!("its property {id:#06x}");
        let fixed = ptype::fixed_size(ty);
        if fixed.is_some_and(|n| n <= 4) {
            continue;
        }
        let Some(data) = named(heap, subs, value, &what)? else { continue };
        match ty {
            _ if fixed.is_some_and(|n| n != data.len()) => {
                return Err(format!("{what} is of a kind with a size of its own, and has {} bytes", data.len()));
            }
            ptype::STRING if !data.len().is_multiple_of(2) => {
                return Err(format!("{what} is text of two bytes a letter, and has an odd number of bytes"));
            }
            ptype::MULTI_STRING | ptype::MULTI_STRING8 | ptype::MULTI_BINARY => several(data, &what)?,
            // An object ([MS-PST] 2.3.3.5): the subnode it is in, and its size.
            ptype::OBJECT => {
                let nid = u32_at(data, 0).filter(|_| data.len() == 8).ok_or(format!("{what} is an object without the eight bytes that say where"))?;
                if !subs.contains_key(&nid) {
                    return Err(format!("{what} is an object in the subnode {nid:#x}, which is not there"));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// A Table Context ([MS-PST] 2.3.4).
fn table(ndb: &mut Ndb, heap: &Heap, subs: &Subnodes) -> Result<(), String> {
    if heap.client != SIG_TC {
        return Err("it is to hold a table, and its heap is of another kind".into());
    }
    // TCINFO ([MS-PST] 2.3.4.1): bType, cCols, rgib[4], hidRowIndex, hnidRows, hidIndex, rgTCOLDESC.
    let info = heap.alloc(heap.root).ok_or("its table has no header")?;
    let cols = usize::from(info.get(1).copied().unwrap_or(0));
    if info.first() != Some(&SIG_TC) || info.len() != 22 + cols * 8 {
        return Err("its table's header is not the size its columns need".into());
    }
    let end = |i: usize| usize::from(u16_at(info, 2 + i * 2).unwrap_or(0));
    let (four, two, one, row_size) = (end(0), end(1), end(2), end(3));
    if four > two || two > one || one > row_size || row_size - one != cols.div_ceil(8) {
        return Err("its table's rows are not laid out as four, two and one bytes and a bit for each column".into());
    }
    // TCOLDESC ([MS-PST] 2.3.4.2): tag, ibData, cbData, iBit; in the order of their tags.
    let mut columns: Vec<(u16, usize, usize, usize)> = Vec::with_capacity(cols);
    let (mut before, mut bits) = (None, HashSet::new());
    for i in 0..cols {
        let d = info.get(22 + i * 8..30 + i * 8).unwrap_or(&[]);
        let (Some(tag), Some(at)) = (u32_at(d, 0), u16_at(d, 4)) else { continue };
        let (at, size, bit) = (usize::from(at), usize::from(d.get(6).copied().unwrap_or(0)), usize::from(d.get(7).copied().unwrap_or(0)));
        let what = format!("the column {tag:#010x} of its table");
        if before.is_some_and(|b| b >= tag) {
            return Err(format!("{what} is out of order"));
        }
        before = Some(tag);
        let ty = tag as u16;
        // A value of up to eight bytes is in the row; any other is named there ([MS-PST] 2.3.4.4.2).
        let want = match ptype::fixed_size(ty) {
            _ if ty == ptype::BOOL => 1,
            Some(n) if n <= 8 => n,
            _ => 4,
        };
        let part = match size {
            4 | 8 => 0..four,
            2 => four..two,
            1 => two..one,
            _ => 0..0,
        };
        if size != want || at < part.start || at + size > part.end {
            return Err(format!("{what} is not where a value of its size is kept in a row"));
        }
        if bit >= cols || !bits.insert(bit) {
            return Err(format!("{what} has a bit that is another column's, or none"));
        }
        columns.push((ty, at, size, bit));
    }
    // A row's id and its version are columns of every table, at fixed places ([MS-PST] 2.3.4.4.1).
    let fixed = |tag: u32, at: u16, bit: u8| {
        (0..cols)
            .filter_map(|i| info.get(22 + i * 8..30 + i * 8))
            .any(|d| u32_at(d, 0) == Some(tag) && u16_at(d, 4) == Some(at) && d.get(7) == Some(&bit))
    };
    if cols > 0 && !(fixed(0x67F2_0003, 0, 0) && fixed(0x67F3_0003, 4, 1)) {
        return Err("its table does not have the columns for a row's id and version where they belong".into());
    }
    let (row_index, rows_hnid) = (u32_at(info, 10).unwrap_or(0), u32_at(info, 14).unwrap_or(0));
    // The rows: in the heap, or in a subnode where no row spans two blocks ([MS-PST] 2.3.4.4).
    let blocks: Vec<Vec<u8>> = match rows_hnid {
        0 => Vec::new(),
        h if h & 0x1F == 0 => {
            let rows = heap.alloc(h).ok_or("its table's rows are not in the heap")?;
            if row_size == 0 || !rows.len().is_multiple_of(row_size) {
                return Err("its table's rows do not fill their allocation".into());
            }
            vec![rows.to_vec()]
        }
        h => {
            let (data, _) = subs.get(&h).ok_or(format!("its table's rows are in the subnode {h:#x}, which is not there"))?;
            ndb.data_blocks(*data).map_err(|e| format!("its table's rows cannot be read: {e}"))?
        }
    };
    let rows: Vec<&[u8]> = if row_size == 0 { Vec::new() } else { blocks.iter().flat_map(|b| b.chunks_exact(row_size)).take(MAX_ROWS).collect() };
    // The row index ([MS-PST] 2.3.4.3): a row's id, and which row that is.
    let mut taken = HashSet::new();
    // TCROWID ([MS-PST] 2.3.4.3.1): the place of a row is four bytes, and two in an ANSI file.
    let place = if heap.ver == Ver::Ansi { 2 } else { 4 };
    for rec in bth(heap, row_index, 4, place, "its table's row index")? {
        let n = if place == 2 { u16_at(rec, 4).map(u32::from) } else { u32_at(rec, 4) };
        let (Some(id), Some(n)) = (u32_at(rec, 0), n) else { continue };
        let row = rows.get(n as usize).ok_or(format!("its table's row index puts the row {id:#x} at {n}, past the last row"))?;
        if !taken.insert(n) {
            return Err(format!("its table's row index puts two rows at {n}"));
        }
        if u32_at(row, 0) != Some(id) {
            return Err(format!("its table's row {n} is not the row {id:#x} the row index says it is"));
        }
        for &(ty, at, size, bit) in &columns {
            // The cell existence block: a bit for each column, the highest first.
            let there = row.get(one + bit / 8).is_some_and(|b| b & (1 << (7 - bit % 8)) != 0);
            if !there || size != 4 || ptype::fixed_size(ty).is_some_and(|n| n <= 8) {
                continue;
            }
            let Some(value) = u32_at(row, at) else { continue };
            named(heap, subs, value, &format!("a cell of its table's row {id:#x}"))?;
        }
    }
    Ok(())
}

struct Inside<'a, 'n> {
    ndb: &'a mut Ndb<'n>,
    seen: usize,
    problems: Vec<String>,
    limit: usize,
    more: u64,
}

impl Inside<'_, '_> {
    fn problem(&mut self, what: String) {
        if self.problems.len() < self.limit {
            self.problems.push(what);
        } else {
            self.more = self.more.saturating_add(1);
        }
    }

    /// One node, and the nodes under it.
    fn node(&mut self, nid: u32, data: u64, sub: u64, depth: u8, name: &str) {
        self.seen += 1;
        if self.seen > MAX_NODES || depth > MAX_DEPTH {
            return;
        }
        let subs = match self.ndb.subnodes(sub) {
            Ok(subs) => subs,
            Err(e) => return self.problem(format!("{name}: its subnodes cannot be read: {e}")),
        };
        let kind = holds(nid);
        // A node without data has nothing to hold against the rules.
        if kind != Holds::Other && data != 0 {
            let found = match self.ndb.data_blocks(data) {
                Err(e) => Err(format!("its data cannot be read: {e}")),
                Ok(blocks) => Heap::of(blocks, self.ndb.ver).and_then(|heap| match kind {
                    Holds::Table => table(self.ndb, &heap, &subs),
                    _ => properties(&heap, &subs),
                }),
            };
            if let Err(what) = found {
                self.problem(format!("{name}: {what}"));
            }
        }
        let mut below: Vec<(u32, (u64, u64))> = subs.into_iter().filter(|(nid, _)| holds(*nid) != Holds::Other).collect();
        below.sort_unstable();
        for (kid, (data, sub)) in below {
            self.node(kid, data, sub, depth + 1, &format!("the subnode {kid:#x} of {name}"));
        }
    }
}

/// Look inside every node of a file, or inside those of `only`. Gives the problems found, at most `limit` of them spelled
/// out, and how many more there were.
pub(crate) fn inside(
    ndb: &mut Ndb,
    limit: usize,
    stopped: &dyn Fn() -> bool,
    only: Option<&std::collections::HashSet<u32>>,
) -> Result<(Vec<String>, u64), crate::PstError> {
    let mut nodes: Vec<(u32, u64, u64)> =
        ndb.nbt.iter().filter(|(nid, _)| only.is_none_or(|o| o.contains(nid))).map(|(nid, e)| (*nid, e.data, e.sub)).collect();
    nodes.sort_unstable();
    let mut c = Inside { ndb, seen: 0, problems: Vec::new(), limit, more: 0 };
    for (nid, data, sub) in nodes {
        if stopped() {
            return Err(crate::PstError::Stopped);
        }
        c.node(nid, data, sub, 0, &format!("the node {nid:#x}"));
    }
    Ok((c.problems, c.more))
}

#[cfg(test)]
#[path = "tests_check.rs"]
mod tests;
