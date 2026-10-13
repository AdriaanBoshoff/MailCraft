//! The structure check on the kinds of data file this crate does not write: an ANSI file and
//! a file with 4K pages, each put together here byte by byte (the first from [MS-PST], the
//! second from what was measured), so that the check is held against something its own
//! tables did not produce.

use super::{Check, check};
use crate::write::{crc, sig};
use std::io::Cursor;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Ansi,
    FourK,
}

struct Built {
    bytes: Vec<u8>,
    /// Where the first data block's bytes are.
    plain: usize,
    /// Where the header counts the free bytes.
    free_at: usize,
}

struct Maker {
    kind: Kind,
    out: Vec<u8>,
    slot: usize,
    page: usize,
    at: usize,
    /// (id, offset, size, inflated size, references)
    blocks: Vec<(u64, usize, usize, usize, u16)>,
    used: Vec<(usize, usize)>,
}

impl Maker {
    fn wide(&self) -> bool {
        self.kind == Kind::FourK
    }

    fn word(&self, v: u64) -> Vec<u8> {
        if self.wide() { v.to_le_bytes().to_vec() } else { (v as u32).to_le_bytes().to_vec() }
    }

    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.out[at..at + bytes.len()].copy_from_slice(bytes);
    }

    /// A block as it is stored, `inflated` being what its entry is to say it comes to.
    fn block(&mut self, bid: u64, data: &[u8], inflated: usize) -> usize {
        let trailer = if self.wide() { 24 } else { 12 };
        let size = (data.len() + trailer).next_multiple_of(self.slot);
        let ib = self.at;
        self.put(ib, data);
        let mut t = (data.len() as u16).to_le_bytes().to_vec();
        t.extend(sig(ib as u64, bid).to_le_bytes());
        if self.wide() {
            t.extend(crc(data).to_le_bytes());
            t.extend(bid.to_le_bytes());
        } else {
            t.extend((bid as u32).to_le_bytes());
            t.extend(crc(data).to_le_bytes());
        }
        self.put(ib + size - trailer, &t);
        self.blocks.push((bid, ib, data.len(), inflated, 2));
        self.used.push((ib, size));
        self.at += size;
        ib
    }

    /// A page: its body, then the trailer. A map page's id is its offset and it has no signature.
    fn page(&mut self, ib: usize, body: &[u8], ptype: u8, bid: Option<u64>) {
        let before = if self.wide() { 4072 } else { 500 };
        let mut p = vec![0u8; self.page];
        p[..body.len()].copy_from_slice(body);
        let sum = crc(&p[..before]);
        let own = bid.unwrap_or(ib as u64);
        let wsig = bid.map_or(0, |b| sig(ib as u64, b));
        let mut t = vec![ptype, ptype];
        t.extend(wsig.to_le_bytes());
        if self.wide() {
            t.extend(sum.to_le_bytes());
            t.extend(own.to_le_bytes());
        } else {
            t.extend((own as u32).to_le_bytes());
            t.extend(sum.to_le_bytes());
        }
        p[before..before + t.len()].copy_from_slice(&t);
        self.put(ib, &p);
        self.used.push((ib, self.page));
    }

    /// A leaf page of one of the two trees.
    fn leaf(&mut self, entries: &[Vec<u8>], ptype: u8, bid: u64) -> usize {
        self.at = self.at.next_multiple_of(512);
        let ib = self.at;
        let size = entries[0].len();
        let mut body: Vec<u8> = entries.concat();
        if self.wide() {
            body.resize(4056, 0);
            body.extend((entries.len() as u16).to_le_bytes());
            body.extend((4056 / size as u16).to_le_bytes());
            body.extend([size as u8, 0]);
        } else {
            body.resize(496, 0);
            body.extend([entries.len() as u8, (496 / size) as u8, size as u8, 0]);
        }
        self.page(ib, &body, ptype, Some(bid));
        self.at += self.page;
        ib
    }
}

/// A small data file of a kind: a node of one block, a node whose data is two blocks under an
/// XBLOCK and which has a subnode, a folder with an empty property list, and in a file with 4K
/// pages a node whose block is stored deflated (said to come to `lie` bytes more than it does).
fn built(kind: Kind, lie: usize) -> Built {
    let wide = kind == Kind::FourK;
    let (first, slot, page, bits) = if wide { (0x22000, 512, 4096, 4072) } else { (0x4400, 64, 512, 496) };
    let len = first + bits * 8 * slot;
    let mut m = Maker { kind, out: vec![0u8; len], slot, page, at: first + page, blocks: Vec::new(), used: Vec::new() };
    if !wide {
        // The page map of the first section ([MS-PST] 2.2.2.7.3), after four bytes of padding.
        let mut body = vec![0u8; 4];
        body.resize(500, 0xFF);
        m.page(first + 512, &body, 0x83, None);
        m.at += 512;
    }

    let plain = m.block(4, b"a node of one block", 19);
    m.block(8, &[7u8; 300], 300);
    m.block(12, &[9u8; 40], 40);
    // XBLOCK ([MS-PST] 2.2.2.8.3.2.1): btype, cLevel, cEnt, lcbTotal, the ids.
    let mut x = vec![1u8, 1, 2, 0];
    x.extend(340u32.to_le_bytes());
    x.extend(m.word(8));
    x.extend(m.word(12));
    m.block(0x12, &x, x.len());
    m.block(0x18, b"a subnode", 9);
    // SLBLOCK ([MS-PST] 2.2.2.8.3.3.1.2): btype, cLevel, cEnt, padding in a Unicode file, the entries.
    let mut sl = vec![2u8, 0, 1, 0];
    if wide {
        sl.extend([0u8; 4]);
    }
    sl.extend(m.word(0x8001));
    sl.extend(m.word(0x18));
    sl.extend(m.word(0));
    m.block(0x16, &sl, sl.len());
    // A heap with a property list of no records ([MS-PST] 2.3.1.2, 2.3.2.1, 2.3.1.5).
    let mut heap = vec![20u8, 0, 0xEC, 0xBC, 0x20, 0, 0, 0, 0, 0, 0, 0];
    heap.extend([0xB5, 2, 6, 0, 0, 0, 0, 0]);
    heap.extend([1u8, 0, 0, 0, 12, 0, 20, 0]);
    m.block(0x20, &heap, heap.len());
    let mut nodes: Vec<(u32, u64, u64)> = vec![(0x201, 4, 0), (0x221, 0x12, 0x16), (0x122, 0x20, 0)];
    if wide {
        let whole = vec![0u8; 600];
        let stored = miniz_oxide::deflate::compress_to_vec_zlib(&whole, 6);
        m.block(0x24, &stored, whole.len() + lie);
        nodes.push((0x241, 0x24, 0));
    }

    let mut blocks = m.blocks.clone();
    blocks.sort_unstable();
    let entries: Vec<Vec<u8>> = blocks
        .iter()
        .map(|(bid, ib, cb, inflated, refs)| {
            let mut e = m.word(*bid);
            e.extend(m.word(*ib as u64));
            e.extend((*cb as u16).to_le_bytes());
            if wide {
                e.extend((*inflated as u16).to_le_bytes());
            }
            e.extend(refs.to_le_bytes());
            e.resize(if wide { 24 } else { 12 }, 0);
            e
        })
        .collect();
    let bbt = m.leaf(&entries, 0x80, 0x101);
    nodes.sort_unstable();
    let entries: Vec<Vec<u8>> = nodes
        .iter()
        .map(|(nid, data, sub)| {
            let mut e = m.word(u64::from(*nid));
            e.extend(m.word(*data));
            e.extend(m.word(*sub));
            e.extend(0u32.to_le_bytes());
            e.resize(if wide { 32 } else { 16 }, 0);
            e
        })
        .collect();
    let nbt = m.leaf(&entries, 0x81, 0x105);

    // The allocation map: a bit for a slot, the highest bit of a byte first.
    let mut map = vec![0u8; bits];
    for (ib, size) in std::iter::once((first, page)).chain(m.used.clone()) {
        for n in (ib - first) / slot..(ib - first + size).div_ceil(slot) {
            map[n / 8] |= 0x80 >> (n % 8);
        }
    }
    let free = map.iter().map(|b| u64::from(b.count_zeros())).sum::<u64>() * slot as u64;
    let body = if wide { map } else { [vec![0u8; 4], map].concat() };
    m.page(first, &body, 0x84, None);

    // HEADER ([MS-PST] 2.2.2.6).
    m.put(0, b"!BDN");
    m.put(8, if wide { b"SO" } else { b"SM" });
    m.put(10, &(if wide { 36u16 } else { 14 }).to_le_bytes());
    let free_at;
    if wide {
        m.put(184, &(len as u64).to_le_bytes());
        m.put(192, &(first as u64).to_le_bytes());
        m.put(200, &free.to_le_bytes());
        free_at = 200;
        m.put(216, &0x105u64.to_le_bytes());
        m.put(224, &(nbt as u64).to_le_bytes());
        m.put(232, &0x101u64.to_le_bytes());
        m.put(240, &(bbt as u64).to_le_bytes());
        m.put(248, &[2]);
        m.put(512, &[0x80, 0]);
        m.put(516, &0x100u64.to_le_bytes());
    } else {
        m.put(24, &0x100u32.to_le_bytes());
        m.put(168, &(len as u32).to_le_bytes());
        m.put(172, &(first as u32).to_le_bytes());
        m.put(176, &(free as u32).to_le_bytes());
        free_at = 176;
        m.put(184, &0x105u32.to_le_bytes());
        m.put(188, &(nbt as u32).to_le_bytes());
        m.put(192, &0x101u32.to_le_bytes());
        m.put(196, &(bbt as u32).to_le_bytes());
        m.put(200, &[2]);
        m.put(204, &[0xFF; 256]);
        m.put(460, &[0x80, 0]);
    }
    seal(&mut m.out, wide);
    Built { bytes: m.out, plain, free_at }
}

/// The header's checksums over what it holds now.
fn seal(bytes: &mut [u8], wide: bool) {
    let partial = crc(&bytes[8..479]);
    bytes[4..8].copy_from_slice(&partial.to_le_bytes());
    if wide {
        let full = crc(&bytes[8..524]);
        bytes[524..528].copy_from_slice(&full.to_le_bytes());
    }
}

fn looked(bytes: &[u8]) -> Check {
    check(&mut Cursor::new(bytes)).unwrap()
}

#[test]
fn an_ansi_file_is_held_against_its_own_layout() {
    let Built { mut bytes, plain, free_at } = built(Kind::Ansi, 0);
    let found = looked(&bytes);
    assert_eq!((found.problems.clone(), found.inside.clone()), (vec![], vec![]));
    // The allocation map and the page map; a page of each tree.
    assert_eq!((found.pages, found.blocks, found.nodes), (4, 7, 3));

    bytes[plain] ^= 1;
    assert!(looked(&bytes).problems.iter().any(|p| p.contains("the block 0x4") && p.contains("does not match its checksum")));
    bytes[plain] ^= 1;

    bytes[free_at] ^= 0x40;
    seal(&mut bytes, false);
    assert!(looked(&bytes).problems.iter().any(|p| p.contains("free bytes")));
    bytes[free_at] ^= 0x40;
    seal(&mut bytes, false);

    // The page map has to be there and sound.
    bytes[0x4600 + 100] ^= 1;
    assert!(looked(&bytes).problems.iter().any(|p| p.contains("the page map") && p.contains("checksum")));
    bytes[0x4600 + 100] ^= 1;

    // The heap's first block is said to be nearly full, and it is nearly empty.
    assert_eq!(looked(&bytes), found);
    let heap = bytes.windows(4).position(|w| w == [20, 0, 0xEC, 0xBC]).unwrap();
    bytes[heap + 8] = 9;
    let sum = crc(&bytes[heap..heap + 28]);
    bytes[heap + 60..heap + 64].copy_from_slice(&sum.to_le_bytes());
    let inside = looked(&bytes).inside;
    assert!(inside.iter().any(|p| p.contains("the node 0x122") && p.contains("fuller than it is")), "{inside:?}");
}

#[test]
fn a_file_with_4k_pages_is_held_against_what_such_files_were_seen_to_be() {
    let Built { mut bytes, plain, free_at } = built(Kind::FourK, 0);
    let found = looked(&bytes);
    assert_eq!((found.problems.clone(), found.inside.clone()), (vec![], vec![]));
    assert_eq!((found.pages, found.blocks, found.nodes), (3, 8, 4));

    bytes[plain + 3] ^= 0x10;
    assert!(looked(&bytes).problems.iter().any(|p| p.contains("the block 0x4") && p.contains("does not match its checksum")));
    bytes[plain + 3] ^= 0x10;

    bytes[free_at + 1] ^= 2;
    seal(&mut bytes, true);
    assert!(looked(&bytes).problems.iter().any(|p| p.contains("free bytes")));
    bytes[free_at + 1] ^= 2;
    seal(&mut bytes, true);
    assert_eq!(looked(&bytes), found);

    // A block that is stored deflated comes to what its entry says.
    let lied = looked(&built(Kind::FourK, 1).bytes);
    assert!(lied.problems.iter().any(|p| p.contains("the block 0x24") && p.contains("inflates to 600 bytes, not the 601")), "{:?}", lied.problems);
}

#[test]
fn a_cut_or_smashed_file_of_either_kind_is_found_or_passed_never_a_panic() {
    for kind in [Kind::Ansi, Kind::FourK] {
        let whole = built(kind, 0).bytes;
        let first = if kind == Kind::FourK { 0x22000 } else { 0x4400 };
        // Cut short: in the header, in the maps, in the blocks, in the trees.
        for len in [0, 3, 11, 12, 200, 563, 564, first, first + 100, first + 5000, first + 9000, whole.len() - 1] {
            let _ = check(&mut Cursor::new(&whole[..len]));
        }
        // Smashed: a run of bytes of the part that is in use, and of the header, set to each
        // of a few values. The header's checksums are made good again, so that what it then
        // says is believed and followed.
        let mut bytes = whole.clone();
        let mut seed = 0x2545_F491u32;
        for round in 0..400 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let (base, span) = if round % 4 == 0 { (8, 540) } else { (first, 12_000) };
            let at = base + seed as usize % span;
            let run = 1 + (seed >> 20) as usize % 8;
            let was = bytes[at..at + run].to_vec();
            bytes[at..at + run].fill([0u8, 0xFF, 0x80, 1][round % 4]);
            if round % 4 == 0 && round % 8 == 0 {
                seal(&mut bytes, kind == Kind::FourK);
            }
            let _ = check(&mut Cursor::new(&bytes[..]));
            bytes[at..at + run].copy_from_slice(&was);
        }
    }
}
