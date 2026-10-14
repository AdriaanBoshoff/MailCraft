//! The Node Database layer ([MS-PST] 2.2): header, the node and block B-trees, blocks, data trees
//! and subnode trees. Everything read here is treated as hostile: offsets are checked against the
//! file length, sizes and counts are capped, and tree walks are bounded and cycle-safe.

use crate::PstError;
use crate::crypt;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};

pub(crate) trait Source: Read + Seek + Send {}
impl<T: Read + Seek + Send> Source for T {}

/// File format generation ([MS-PST] 2.2.2.6 wVer). The 4K variant (wVer 36) is what Outlook
/// 2013+ uses for `.ost` files; it is not in the specification and is read on a best-effort basis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ver {
    Ansi,
    Unicode,
    Unicode4k,
}

/// No block is larger than this once decoded.
const MAX_BLOCK: usize = 1 << 20;
/// B-tree pages, B-tree entries, data-tree blocks and subnodes are capped.
const MAX_PAGES: usize = 4_000_000;
const MAX_ENTRIES: usize = 40_000_000;
const MAX_TREE_BLOCKS: usize = 1 << 20;
const MAX_SUBNODES: usize = 1 << 20;
/// One node's data is never assembled past this size.
pub(crate) const MAX_NODE_BYTES: usize = 1 << 30;

#[derive(Clone, Copy, Debug)]
pub(crate) struct NbtEntry {
    pub data: u64,
    pub sub: u64,
    pub parent: u32,
}

#[derive(Clone, Copy, Debug)]
struct BbtEntry {
    ib: u64,
    cb: u32,
    inflated: u32,
}

/// (data block id, subnode block id) of one subnode.
pub(crate) type Subnodes = HashMap<u32, (u64, u64)>;

pub(crate) struct Ndb<'a> {
    src: Box<dyn Source + 'a>,
    len: u64,
    pub ver: Ver,
    crypt: u8,
    pub nbt: HashMap<u32, NbtEntry>,
    bbt: HashMap<u64, BbtEntry>,
    /// dwUnique and rgnid of the header ([MS-PST] 2.2.2.6): how often the header was
    /// written, and for each kind of node id the next one to give out.
    pub counters: (u32, [u32; 32]),
}

pub(crate) fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

pub(crate) fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

pub(crate) fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at.checked_add(8)?)?.try_into().ok()?))
}

fn damaged(what: &str) -> PstError {
    PstError::Damaged(what.to_string())
}

impl<'a> Ndb<'a> {
    pub(crate) fn open(mut src: Box<dyn Source + 'a>) -> Result<Self, PstError> {
        let len = src.seek(SeekFrom::End(0))?;
        let mut ndb = Self { src, len, ver: Ver::Unicode, crypt: 0, nbt: HashMap::new(), bbt: HashMap::new(), counters: (0, [0; 32]) };
        let head = ndb.read_at(0, 564.min(usize::try_from(len).unwrap_or(564)))?;
        // dwMagic "!BDN" and wMagicClient "SM" ([MS-PST] 2.2.2.6). Offline stores (.ost) carry
        // "SO" there instead (observed in files; the NDB layout is otherwise the same).
        if head.get(..4) != Some(b"!BDN") || !matches!(head.get(8..10), Some(b"SM" | b"SO")) {
            return Err(PstError::NotPst);
        }
        let wver = u16_at(&head, 10).ok_or(PstError::NotPst)?;
        ndb.ver = match wver {
            14 | 15 => Ver::Ansi,
            36 => Ver::Unicode4k,
            v if v >= 23 => Ver::Unicode,
            v => return Err(PstError::Unsupported(format!("file format version {v}"))),
        };
        // ROOT: BREFNBT then BREFBBT; bCryptMethod follows the two free maps.
        let (nbt_ib, bbt_ib, crypt_at) = match ndb.ver {
            Ver::Ansi => (u32_at(&head, 188).map(u64::from), u32_at(&head, 196).map(u64::from), 461),
            _ => (u64_at(&head, 224), u64_at(&head, 240), 513),
        };
        // dwUnique, then rgnid: after the 8-byte ids of a Unicode file, the 4-byte ones of an ANSI.
        let unique_at = if ndb.ver == Ver::Ansi { 32 } else { 40 };
        ndb.counters.0 = u32_at(&head, unique_at).unwrap_or(0);
        for (kind, next) in ndb.counters.1.iter_mut().enumerate() {
            *next = u32_at(&head, unique_at + 4 + kind * 4).unwrap_or(0);
        }
        ndb.crypt = head.get(crypt_at).copied().ok_or_else(|| damaged("the header is cut short"))?;
        if ndb.crypt > 2 {
            // 0x10 is Windows Information Protection: the blocks are encrypted with a key we do not have.
            return Err(PstError::Unsupported("this data file is encrypted".to_string()));
        }
        let (nbt_ib, bbt_ib) = (nbt_ib.ok_or_else(|| damaged("the header is cut short"))?, bbt_ib.ok_or_else(|| damaged("the header is cut short"))?);
        ndb.load_btree(bbt_ib, false)?;
        ndb.load_btree(nbt_ib, true)?;
        Ok(ndb)
    }

    fn read_at(&mut self, at: u64, len: usize) -> Result<Vec<u8>, PstError> {
        let end = at.checked_add(len as u64).ok_or_else(|| damaged("an offset is out of range"))?;
        if end > self.len || len > MAX_BLOCK {
            return Err(damaged("an offset points past the end of the file"));
        }
        let mut buf = vec![0u8; len];
        self.src.seek(SeekFrom::Start(at))?;
        self.src.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn page_size(&self) -> usize {
        if self.ver == Ver::Unicode4k { 4096 } else { 512 }
    }

    /// Walk a B-tree of pages ([MS-PST] 2.2.2.7.7) and collect its leaf entries.
    fn load_btree(&mut self, root: u64, is_nbt: bool) -> Result<(), PstError> {
        let mut stack = vec![(root, 0u32)];
        let mut seen: HashSet<u64> = HashSet::new();
        while let Some((ib, depth)) = stack.pop() {
            if depth > 24 || !seen.insert(ib) || seen.len() > MAX_PAGES {
                continue;
            }
            let Ok(page) = self.read_at(ib, self.page_size()) else { continue };
            // cEnt, cEntMax, cbEnt, cLevel sit after the entry area.
            let (count, size, level) = match self.ver {
                Ver::Ansi => (page.get(496).map(|b| usize::from(*b)), page.get(498), page.get(499)),
                Ver::Unicode => (page.get(488).map(|b| usize::from(*b)), page.get(490), page.get(491)),
                Ver::Unicode4k => (u16_at(&page, 4056).map(usize::from), page.get(4060), page.get(4061)),
            };
            let (Some(count), Some(&size), Some(&level)) = (count, size, level) else { continue };
            let size = usize::from(size);
            let wide = self.ver != Ver::Ansi;
            let min = match (level > 0, is_nbt, wide) {
                (true, _, true) | (false, false, true) => 24,
                (true, _, false) | (false, false, false) => 12,
                (false, true, true) => 32,
                (false, true, false) => 16,
            };
            if size < min {
                continue;
            }
            for i in 0..count {
                let Some(e) = i.checked_mul(size).and_then(|at| page.get(at..at.checked_add(size)?)) else { break };
                if level > 0 {
                    // BTENTRY: key, then BREF (bid, ib).
                    let child = if wide { u64_at(e, 16) } else { u32_at(e, 8).map(u64::from) };
                    if let Some(child) = child {
                        stack.push((child, depth + 1));
                    }
                } else if is_nbt {
                    // NBTENTRY: nid, bidData, bidSub, nidParent.
                    let entry = if wide {
                        (u32_at(e, 0), u64_at(e, 8), u64_at(e, 16), u32_at(e, 24))
                    } else {
                        (u32_at(e, 0), u32_at(e, 4).map(u64::from), u32_at(e, 8).map(u64::from), u32_at(e, 12))
                    };
                    if let (Some(nid), Some(data), Some(sub), Some(parent)) = entry {
                        self.nbt.insert(nid, NbtEntry { data, sub, parent });
                    }
                } else {
                    // BBTENTRY: BREF (bid, ib), cb, and in 4K files the inflated size.
                    let entry = match self.ver {
                        Ver::Ansi => (u32_at(e, 0).map(u64::from), u32_at(e, 4).map(u64::from), u16_at(e, 8), u16_at(e, 8)),
                        Ver::Unicode => (u64_at(e, 0), u64_at(e, 8), u16_at(e, 16), u16_at(e, 16)),
                        Ver::Unicode4k => (u64_at(e, 0), u64_at(e, 8), u16_at(e, 16), u16_at(e, 18)),
                    };
                    if let (Some(bid), Some(ib), Some(cb), Some(inflated)) = entry {
                        self.bbt.insert(bid & !1, BbtEntry { ib, cb: u32::from(cb), inflated: u32::from(inflated) });
                    }
                }
                if self.nbt.len() + self.bbt.len() > MAX_ENTRIES {
                    return Err(damaged("the file's index is implausibly large"));
                }
            }
        }
        Ok(())
    }

    /// One block's bytes, inflated and decoded ([MS-PST] 2.2.2.8). Internal blocks (bit 1 of the
    /// id) are never encoded.
    fn block(&mut self, bid: u64) -> Result<Vec<u8>, PstError> {
        let e = *self.bbt.get(&(bid & !1)).ok_or_else(|| damaged("a block is missing from the index"))?;
        let mut data = self.read_at(e.ib, e.cb as usize)?;
        if self.ver == Ver::Unicode4k && e.inflated != e.cb && e.inflated != 0 {
            data = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&data, MAX_BLOCK)
                .map_err(|_| damaged("a compressed block does not inflate"))?;
        }
        if bid & 2 == 0 {
            match self.crypt {
                1 => crypt::permute_decode(&mut data),
                2 => crypt::cyclic(&mut data, bid as u32),
                _ => {}
            }
        }
        Ok(data)
    }

    /// The data blocks of a node, in order: one block, or the leaves of an XBLOCK / XXBLOCK tree
    /// ([MS-PST] 2.2.2.8.3.2).
    pub(crate) fn data_blocks(&mut self, bid: u64) -> Result<Vec<Vec<u8>>, PstError> {
        if bid == 0 {
            return Ok(Vec::new());
        }
        if bid & 2 == 0 {
            return Ok(vec![self.block(bid)?]);
        }
        let mut out = Vec::new();
        let mut total = 0usize;
        // (block id, levels allowed below it)
        let mut stack = vec![(bid, 2u8)];
        let mut visited = 0usize;
        while let Some((b, budget)) = stack.pop() {
            visited += 1;
            if visited > MAX_TREE_BLOCKS {
                return Err(damaged("a data tree is implausibly large"));
            }
            if b & 2 == 0 {
                let block = self.block(b)?;
                total = total.saturating_add(block.len());
                if total > MAX_NODE_BYTES {
                    return Err(damaged("an item is larger than this version can load"));
                }
                out.push(block);
                continue;
            }
            let x = self.block(b)?;
            // btype 0x01, cLevel, cEnt, lcbTotal, then the child ids.
            if x.first() != Some(&1) || budget == 0 {
                return Err(damaged("a data tree block is not valid"));
            }
            let count = usize::from(u16_at(&x, 2).unwrap_or(0));
            let width = if self.ver == Ver::Ansi { 4 } else { 8 };
            let mut kids = Vec::with_capacity(count);
            for i in 0..count {
                let at = 8 + i * width;
                let kid = if width == 8 { u64_at(&x, at) } else { u32_at(&x, at).map(u64::from) };
                match kid {
                    Some(k) => kids.push(k),
                    None => break,
                }
            }
            // The stack pops from the end; push in reverse to keep file order.
            stack.extend(kids.into_iter().rev().map(|k| (k, budget - 1)));
        }
        Ok(out)
    }

    /// How many bytes of data a node's data holds, without reading more than its first block:
    /// a block's size, inflated where it is compressed, or the total an XBLOCK or XXBLOCK gives
    /// (lcbTotal, [MS-PST] 2.2.2.8.3.2.1).
    pub(crate) fn data_len(&mut self, bid: u64) -> Result<u64, PstError> {
        if bid == 0 {
            return Ok(0);
        }
        if bid & 2 == 0 {
            let e = *self.bbt.get(&(bid & !1)).ok_or_else(|| damaged("a block is missing from the index"))?;
            let n = if self.ver == Ver::Unicode4k && e.inflated != 0 { e.inflated } else { e.cb };
            return Ok(u64::from(n));
        }
        let x = self.block(bid)?;
        if x.first() != Some(&1) {
            return Err(damaged("a data tree block is not valid"));
        }
        Ok(u64::from(u32_at(&x, 4).unwrap_or(0)))
    }

    /// How many blocks the file has.
    pub(crate) fn block_count(&self) -> usize {
        self.bbt.len()
    }

    /// A node's data as one buffer.
    pub(crate) fn data(&mut self, bid: u64) -> Result<Vec<u8>, PstError> {
        Ok(self.data_blocks(bid)?.concat())
    }

    /// The subnodes under a node: SLBLOCK leaves, optionally below one SIBLOCK ([MS-PST] 2.2.2.8.3.3).
    pub(crate) fn subnodes(&mut self, bid: u64) -> Result<Subnodes, PstError> {
        let mut out = Subnodes::new();
        if bid == 0 {
            return Ok(out);
        }
        let mut stack = vec![(bid, 0u8)];
        let mut visited = 0usize;
        while let Some((b, depth)) = stack.pop() {
            visited += 1;
            if depth > 3 || visited > MAX_TREE_BLOCKS {
                return Err(damaged("a subnode tree is not valid"));
            }
            let x = self.block(b)?;
            if x.first() != Some(&2) {
                return Err(damaged("a subnode block is not valid"));
            }
            let level = x.get(1).copied().unwrap_or(0);
            let count = usize::from(u16_at(&x, 2).unwrap_or(0));
            let wide = self.ver != Ver::Ansi;
            let start = if wide { 8 } else { 4 };
            for i in 0..count {
                if level == 0 {
                    // SLENTRY: nid, bidData, bidSub.
                    let at = start + i * if wide { 24 } else { 12 };
                    let e = if wide {
                        (u32_at(&x, at), u64_at(&x, at + 8), u64_at(&x, at + 16))
                    } else {
                        (u32_at(&x, at), u32_at(&x, at + 4).map(u64::from), u32_at(&x, at + 8).map(u64::from))
                    };
                    let (Some(nid), Some(data), Some(sub)) = e else { break };
                    out.insert(nid, (data, sub));
                    if out.len() > MAX_SUBNODES {
                        return Err(damaged("a subnode tree is implausibly large"));
                    }
                } else {
                    // SIENTRY: nid, bid of the next level.
                    let at = start + i * if wide { 16 } else { 8 };
                    let child = if wide { u64_at(&x, at + 8) } else { u32_at(&x, at + 4).map(u64::from) };
                    let Some(child) = child else { break };
                    stack.push((child, depth + 1));
                }
            }
        }
        Ok(out)
    }
}
