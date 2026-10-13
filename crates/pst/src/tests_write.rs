//! The node database writer: what it writes is read back by the reader and held against the
//! structure check.

use crate::check::check;
use crate::ltp::{self, Raw, Row};
use crate::ltp_write::{Subs, encode, write_pc, write_tc};
use crate::ndb::{Ndb, u32_at};
use crate::write::{MAX_DATA, NdbWriter, SECTION, Sub, maps, section_base};
use crate::{PstError, PstStore};
use openmailclient_model::PropValue;
use std::io::Cursor;

fn writer(crypt: u8) -> NdbWriter<Cursor<Vec<u8>>> {
    NdbWriter::new(Cursor::new(Vec::new()), crypt).unwrap()
}

fn done(w: NdbWriter<Cursor<Vec<u8>>>) -> Vec<u8> {
    w.finish().unwrap().into_inner()
}

fn sound(bytes: &[u8]) -> crate::Check {
    let found = check(&mut Cursor::new(bytes)).unwrap();
    // These files hold nodes of plain bytes: only the structure around them is meant.
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    found
}

fn open(bytes: &[u8]) -> Ndb<'static> {
    Ndb::open(Box::new(Cursor::new(bytes.to_vec()))).unwrap()
}

/// Bytes that do not repeat from one block to the next.
fn pattern(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

#[test]
fn a_file_with_nothing_in_it_is_sound() {
    let bytes = done(writer(1));
    assert_eq!(bytes.len(), 0x4400 + SECTION);
    let found = sound(&bytes);
    assert_eq!((found.blocks, found.nodes), (0, 0));
    assert!(open(&bytes).nbt.is_empty());
    // The reader's messaging layer has nothing to stand on, and says so instead of failing.
    let _ = PstStore::from_bytes(bytes);
}

#[test]
fn nodes_come_back_as_written_in_every_encoding() {
    for crypt in 0..=2 {
        let small = pattern(100, 1);
        let large = pattern(30_000, 2);
        let inner = pattern(9_000, 3);
        let mut w = writer(crypt);
        let a = w.stream(&small).unwrap();
        let b = w.stream(&large).unwrap();
        let nested = w.stream(&inner).unwrap();
        let deep = w.subnodes(vec![Sub { nid: 0x41, data: nested, sub: 0 }]).unwrap();
        let first = w.stream(b"first").unwrap();
        let second = w.stream(b"second").unwrap();
        let subs = w.subnodes(vec![Sub { nid: 0x8025, data: second, sub: deep }, Sub { nid: 0x671, data: first, sub: 0 }]).unwrap();
        w.node(0x21, 0, a, 0);
        w.node(0x200024, 0x122, b, subs);
        w.node(0x1E1, 0, 0, 0);
        let bytes = done(w);
        let found = sound(&bytes);
        assert_eq!(found.nodes, 3);

        let mut ndb = open(&bytes);
        let n = *ndb.nbt.get(&0x21).unwrap();
        assert_eq!(ndb.data(n.data).unwrap(), small);
        let n = *ndb.nbt.get(&0x200024).unwrap();
        assert_eq!(n.parent, 0x122);
        assert_eq!(ndb.data(n.data).unwrap(), large);
        let subs = ndb.subnodes(n.sub).unwrap();
        assert_eq!(subs.len(), 2);
        assert_eq!(ndb.data(subs[&0x671].0).unwrap(), b"first");
        let (data, sub) = subs[&0x8025];
        assert_eq!(ndb.data(data).unwrap(), b"second");
        let deep = ndb.subnodes(sub).unwrap();
        assert_eq!(ndb.data(deep[&0x41].0).unwrap(), inner);
        // A node with no data has none, and reads as empty.
        let n = *ndb.nbt.get(&0x1E1).unwrap();
        assert_eq!((n.data, n.sub), (0, 0));
        assert!(ndb.data(0).unwrap().is_empty());
        // The header says where the next node id of each kind starts: past the ones in use.
        assert_eq!(u32_at(&bytes, 44 + 4 * 4), Some(0x10002));
        assert_eq!(u32_at(&bytes, 44 + 4 * 2), Some(0x400));
        assert_eq!(bytes[513], crypt);
    }
}

#[test]
fn a_large_item_spans_sections_and_two_levels_of_data_tree() {
    // 4,100 blocks: more than one XBLOCK holds (1,021), and past the 128 sections whose free
    // map is in the header, so a free map page is written too.
    let blocks: Vec<Vec<u8>> = (0..4_100u32).map(|i| pattern(MAX_DATA, i + 10)).collect();
    let mut w = writer(0);
    let data = w.data(blocks.iter()).unwrap();
    w.node(0x200024, 0x122, data, 0);
    let bytes = done(w);
    assert_eq!((bytes.len() - 0x4400) % SECTION, 0);
    let sections = (bytes.len() - 0x4400) / SECTION;
    assert!(sections > 128);
    let found = sound(&bytes);
    // The leaves, five XBLOCKs and the XXBLOCK.
    assert_eq!(found.blocks, 4_100 + 5 + 1);
    let mut ndb = open(&bytes);
    let n = *ndb.nbt.get(&0x200024).unwrap();
    let back = ndb.data_blocks(n.data).unwrap();
    assert_eq!(back.len(), blocks.len());
    assert!(back == blocks);
    // The map pages stand where `maps` says: a page map before every eighth section, the first
    // free map at section 128.
    assert_eq!(maps(0), (true, false, false));
    assert_eq!(maps(1), (false, false, false));
    assert_eq!(maps(128), (true, true, false));
    assert_eq!(maps(128 + 496), (true, true, false));
    assert_eq!(maps(8192), (true, false, true));
    let at = |ib: u64| bytes[ib as usize + 496];
    assert_eq!(at(section_base(0)), 0x84);
    assert_eq!(at(section_base(0) + 512), 0x83);
    assert_eq!(at(section_base(8) + 512), 0x83);
    assert_eq!(at(section_base(128) + 1024), 0x82);
    assert_eq!(at(0x4200), 0x86);
}

#[test]
fn the_same_bytes_are_stored_once() {
    for crypt in 0..=2 {
        let attachment = pattern(20_000, 7);
        let mut w = writer(crypt);
        for nid in [0x200024u32, 0x200044, 0x200064] {
            // An id from the writer is one reference: it is asked for once for each place it goes.
            let data = w.stream(&attachment).unwrap();
            let inside = w.stream(&attachment).unwrap();
            assert_eq!(data, inside);
            let sub = w.subnodes(vec![Sub { nid: 0x8025, data: inside, sub: 0 }]).unwrap();
            w.node(nid, 0x122, data, sub);
        }
        let other = w.stream(&pattern(20_000, 8)).unwrap();
        w.node(0x200084, 0x122, other, 0);
        let bytes = done(w);
        // Three leaves and an XBLOCK for the attachment, one subnode block, and the same four
        // again for the other bytes: the check also finds every reference count right.
        let found = sound(&bytes);
        assert_eq!(found.blocks, 4 + 1 + 4);
        let mut ndb = open(&bytes);
        for nid in [0x200024u32, 0x200044, 0x200064] {
            let n = *ndb.nbt.get(&nid).unwrap();
            assert_eq!(ndb.data(n.data).unwrap(), attachment);
            let subs = ndb.subnodes(n.sub).unwrap();
            assert_eq!(ndb.data(subs[&0x8025].0).unwrap(), attachment);
        }
    }
}

#[test]
fn many_subnodes_get_an_index_block() {
    let mut w = writer(1);
    let mut subs = Vec::new();
    for i in 0..800u32 {
        let data = w.stream(&i.to_le_bytes()).unwrap();
        subs.push(Sub { nid: (i + 1) << 5 | 0x1F, data, sub: 0 });
    }
    subs.reverse();
    let sub = w.subnodes(subs).unwrap();
    w.node(0x200024, 0x122, 0, sub);
    let bytes = done(w);
    // 800 values, three leaves of at most 340 entries and the index over them.
    assert_eq!(sound(&bytes).blocks, 800 + 3 + 1);
    let mut ndb = open(&bytes);
    let n = *ndb.nbt.get(&0x200024).unwrap();
    let back = ndb.subnodes(n.sub).unwrap();
    assert_eq!(back.len(), 800);
    for i in [0u32, 339, 340, 799] {
        assert_eq!(ndb.data(back[&((i + 1) << 5 | 0x1F)].0).unwrap(), i.to_le_bytes());
    }
}

#[test]
fn many_nodes_get_an_index_of_several_levels() {
    let mut w = writer(2);
    for i in 0..1_000u32 {
        let data = w.stream(&pattern(40, i)).unwrap();
        w.node((i + 0x400) << 5 | 4, 0x122, data, 0);
    }
    let bytes = done(w);
    let found = sound(&bytes);
    assert_eq!((found.nodes, found.blocks), (1_000, 1_000));
    let mut ndb = open(&bytes);
    assert_eq!(ndb.nbt.len(), 1_000);
    let n = *ndb.nbt.get(&((999 + 0x400) << 5 | 4)).unwrap();
    assert_eq!(ndb.data(n.data).unwrap(), pattern(40, 999));
}

#[test]
fn what_the_format_cannot_hold_is_refused() {
    assert!(matches!(NdbWriter::new(Cursor::new(Vec::new()), 3), Err(PstError::Write(_))));
    let mut w = writer(1);
    assert!(matches!(w.data([vec![0u8; MAX_DATA + 1]]), Err(PstError::Write(_))));
    let two = vec![Sub { nid: 0x41, data: 0, sub: 0 }, Sub { nid: 0x41, data: 0, sub: 0 }];
    assert!(matches!(w.subnodes(two), Err(PstError::Write(_))));
    // No data and no subnodes are no blocks at all.
    assert_eq!(w.stream(&[]).unwrap(), 0);
    assert_eq!(w.subnodes(Vec::new()).unwrap(), 0);
    w.node(0x21, 0, 0, 0);
    w.node(0x21, 0, 0, 0);
    assert!(matches!(w.finish(), Err(PstError::Write(_))));
}

#[test]
fn damage_is_found_and_never_panics() {
    let mut w = writer(1);
    let big = w.stream(&pattern(30_000, 5)).unwrap();
    let small = w.stream(&pattern(300, 6)).unwrap();
    let sub = w.subnodes(vec![Sub { nid: 0x671, data: small, sub: 0 }]).unwrap();
    w.node(0x200024, 0x122, big, sub);
    let bytes = done(w);
    sound(&bytes);
    // The used part of the file: the header and what follows the first allocation map.
    let used = 0x4400 + 512 + 512 + 30_000 + 4 * 64 + 512 + 3 * 512;
    let mut noticed = 0;
    let mut x = 12_345u32;
    for _ in 0..400 {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let at = (x >> 8) as usize % used;
        let mut bad = bytes.clone();
        bad[at] ^= 0x5A;
        if check(&mut Cursor::new(&bad)).is_ok_and(|c| !c.sound()) {
            noticed += 1;
        }
        if let Ok(mut ndb) = Ndb::open(Box::new(Cursor::new(bad))) {
            for n in ndb.nbt.values().copied().collect::<Vec<_>>() {
                let _ = ndb.data(n.data);
                let _ = ndb.subnodes(n.sub);
            }
        }
    }
    // Most of the header's first 0x4400 bytes are unused; a change in a block or a page shows.
    assert!(noticed > 150, "{noticed}");
    let mut bad = bytes.clone();
    bad[0x4400 + 1024 + 100] ^= 1;
    let found = check(&mut Cursor::new(&bad)).unwrap();
    assert!(found.problems.iter().any(|p| p.contains("checksum")), "{:?}", found.problems);
    for len in [0, 3, 563, 564, 0x4400, 0x4400 + 700, bytes.len() - 1] {
        let _ = check(&mut Cursor::new(&bytes[..len]));
    }
    assert!(matches!(check(&mut Cursor::new(b"not a data file".to_vec())), Err(PstError::NotPst)));
    // A file of a format the check does not know is not called damaged: it is not checked.
    // (One that says it has 4K pages and has not is looked at as such, and found wanting.)
    let mut other = bytes.clone();
    other[10] = 99;
    assert!(matches!(check(&mut Cursor::new(other)), Err(PstError::Unsupported(_))));
}

/// Write one node with a property list and read the list back.
fn pc_round_trip(props: &[(u16, PropValue)]) -> (Vec<(u16, PropValue)>, crate::Check) {
    let mut w = writer(1);
    let raws: Vec<Raw> = props.iter().map(|(id, v)| encode(*id, v)).collect();
    let mut subs = Subs::new(1);
    let data = write_pc(&mut w, &raws, &mut subs).unwrap();
    let sub = w.subnodes(subs.list).unwrap();
    w.node(0x200024, 0x122, data, sub);
    let bytes = done(w);
    let found = sound(&bytes);
    let mut ndb = open(&bytes);
    let n = *ndb.nbt.get(&0x200024).unwrap();
    let subs = ndb.subnodes(n.sub).unwrap();
    let back = ltp::read_pc(&mut ndb, n.data, &subs).unwrap();
    (back.into_iter().map(|raw| ltp::decode(raw, 1252).unwrap()).collect(), found)
}

#[test]
fn a_property_list_comes_back_as_written() {
    let props = vec![
        (0x0017, PropValue::Int32(-2)),
        (0x0E07, PropValue::Int32(0x0001_0031)),
        (0x0E1B, PropValue::Bool(true)),
        (0x0E06, PropValue::Time(0x01DB_1234_5678_9ABC)),
        (0x0037, PropValue::String("Budget \u{e9}t\u{e9} \u{5e74}".into())),
        (0x003D, PropValue::String(String::new())),
        (0x0E08, PropValue::Int64(1 << 40)),
        (0x1000, PropValue::String("x".repeat(1_790))),
        (0x1013, PropValue::Binary(pattern(3_581, 1))),
        (0x3701, PropValue::Binary(pattern(100_000, 2))),
        (0x0FF9, PropValue::Binary(pattern(3_580, 3))),
        (0x8001, PropValue::MultiString(vec!["Red".into(), String::new(), "Blue".into()])),
        (0x8002, PropValue::MultiBinary(vec![vec![1, 2, 3], vec![]])),
        (0x8003, PropValue::MultiInt32(vec![1, -1, 70_000])),
        (0x8004, PropValue::Guid([7; 16])),
        (0x8005, PropValue::Other { ty: 0x00FB, raw: vec![9, 8, 7] }),
        (0x8006, PropValue::Int16(-3)),
        (0x8007, PropValue::Float64(2.5)),
    ];
    let (back, found) = pc_round_trip(&props);
    let mut want = props.clone();
    want.sort_by_key(|p| p.0);
    assert_eq!(back, want);
    // The heap in one block, the two values kept in subnodes (one block, and thirteen with
    // their XBLOCK), and the subnode list.
    assert_eq!(found.blocks, 1 + 1 + 14 + 1);
    // Nothing at all is a list too.
    assert!(pc_round_trip(&[]).0.is_empty());
}

#[test]
fn a_long_property_list_fills_many_heap_blocks() {
    // 3,000 properties: an index of several pages, more allocations than one block may number
    // (2,047), and past the eighth block, where the second kind of block header starts.
    let props: Vec<(u16, PropValue)> = (0..3_000u16)
        .map(|i| (0x1000 + i, if i % 2 == 0 { PropValue::String(format!("value {i} {}", "y".repeat(40))) } else { PropValue::Binary(vec![i as u8]) }))
        .collect();
    let (back, _) = pc_round_trip(&props);
    assert!(back == props);
    let tiny: Vec<(u16, PropValue)> = (0..2_500u16).map(|i| (0x1000 + i, PropValue::Binary(vec![i as u8]))).collect();
    assert!(pc_round_trip(&tiny).0 == tiny);
}

type Rows = Vec<(u32, Vec<(u16, PropValue)>)>;

fn tc_round_trip(columns: &[(u16, u16)], rows: &Rows) -> Rows {
    let mut w = writer(2);
    let raw: Vec<Row> = rows.iter().map(|(id, cells)| Row { id: *id, cells: cells.iter().map(|(id, v)| encode(*id, v)).collect() }).collect();
    let mut subs = Subs::new(1);
    let data = write_tc(&mut w, columns, &raw, &mut subs).unwrap();
    let sub = w.subnodes(subs.list).unwrap();
    w.node(0x12E, 0, data, sub);
    let bytes = done(w);
    sound(&bytes);
    let mut ndb = open(&bytes);
    let n = *ndb.nbt.get(&0x12E).unwrap();
    let subs = ndb.subnodes(n.sub).unwrap();
    let back = ltp::read_tc(&mut ndb, n.data, &subs).unwrap();
    back.into_iter()
        .map(|row| {
            // Every row carries its id and a version; the rest is what was put in.
            assert!(row.cells.iter().any(|c| c.id == 0x67F2 && c.data == row.id.to_le_bytes()));
            assert!(row.cells.iter().any(|c| c.id == 0x67F3));
            let cells = row.cells.into_iter().filter(|c| c.id != 0x67F2 && c.id != 0x67F3).map(|c| ltp::decode(c, 1252).unwrap()).collect();
            (row.id, cells)
        })
        .collect()
}

#[test]
fn a_table_comes_back_as_written() {
    let columns = [(0x3001u16, 0x001Fu16), (0x3602, 0x0003), (0x360A, 0x000B), (0x0E08, 0x0014), (0x0017, 0x0002), (0x0E30, 0x0102)];
    let rows: Rows = vec![
        (
            0x8022,
            vec![
                (0x0017, PropValue::Int16(1)),
                (0x0E08, PropValue::Int64(-5)),
                (0x3001, PropValue::String("Inbox".into())),
                (0x3602, PropValue::Int32(12)),
                (0x360A, PropValue::Bool(true)),
            ],
        ),
        // A row with cells missing, and one cell of a column the table was not told about.
        (0x8042, vec![(0x3001, PropValue::String(String::new())), (0x65E0, PropValue::Time(99))]),
        (0x2223, vec![(0x0E30, PropValue::Binary(pattern(5_000, 4))), (0x360A, PropValue::Bool(false))]),
    ];
    assert_eq!(tc_round_trip(&columns, &rows), rows);
    // No rows: the columns alone.
    assert!(tc_round_trip(&columns, &Vec::new()).is_empty());
}

#[test]
fn a_long_table_keeps_its_rows_in_blocks_of_whole_rows() {
    let columns = [(0x0037u16, 0x001Fu16), (0x0E08, 0x0003), (0x0E06, 0x0040), (0x0E17, 0x0003), (0x0E1B, 0x000B)];
    let rows: Rows = (0..6_000u32)
        .map(|i| {
            let id = 0x20_0004 + ((6_000 - i) << 5);
            let cells = vec![
                (0x0037, PropValue::String(format!("Subject {i}"))),
                (0x0E06, PropValue::Time(i64::from(i) << 20)),
                (0x0E08, PropValue::Int32(i as i32)),
                (0x0E1B, PropValue::Bool(i % 3 == 0)),
            ];
            (id, cells)
        })
        .collect();
    let back = tc_round_trip(&columns, &rows);
    assert_eq!(back.len(), rows.len());
    assert!(back == rows);
}

#[test]
fn lists_and_tables_the_format_cannot_hold_are_refused() {
    let mut w = writer(1);
    let twice = vec![encode(0x37, &PropValue::Int32(1)), encode(0x37, &PropValue::Int32(2))];
    assert!(matches!(write_pc(&mut w, &twice, &mut Subs::new(1)), Err(PstError::Write(_))));
    let row = |id| Row { id, cells: vec![encode(0x37, &PropValue::Int32(1))] };
    assert!(matches!(write_tc(&mut w, &[], &[row(5), row(5)], &mut Subs::new(1)), Err(PstError::Write(_))));
    // With the row id, its version and the row's own column, 256 columns.
    let wide: Vec<(u16, u16)> = (0..253u16).map(|i| (0x1000 + i, 3)).collect();
    assert!(matches!(write_tc(&mut w, &wide, &[row(5)], &mut Subs::new(1)), Err(PstError::Write(_))));
    assert!(write_tc(&mut w, wide.get(..252).unwrap(), &[row(5)], &mut Subs::new(1)).is_ok());
}
