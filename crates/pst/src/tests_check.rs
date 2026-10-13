//! The structure check inside nodes: what is wrong is found and named, nothing panics, and a
//! save is stopped by what it did wrong and not by what was wrong before.

use super::{Heap, Holds, holds, properties, table};
use crate::PstStore;
use crate::check::check;
use crate::ltp::Raw;
use crate::ndb::{Ndb, Subnodes, u16_at, u32_at};
use crate::store_write::{PLANTED, write_store};
use crate::tests_edit::{folder, scratch};
use crate::tests_store::{UID, mail, mailbox, save};
use openmailclient_model::{Attachment, ItemId, Store, ptype};
use std::io::Cursor;

fn look(ndb: &mut Ndb, blocks: Vec<Vec<u8>>, subs: &Subnodes, kind: Holds) -> Result<(), String> {
    Heap::new(blocks).and_then(|heap| if kind == Holds::Table { table(ndb, &heap, subs) } else { properties(&heap, subs) })
}

/// Where `part` starts in `block`, which it is a part of.
fn at(block: &[u8], part: &[u8]) -> usize {
    part.as_ptr() as usize - block.as_ptr() as usize
}

/// The first node of a kind whose heap is one block and has what `fits` asks for.
fn node(ndb: &mut Ndb, kind: u32, fits: impl Fn(&Heap) -> bool) -> (Vec<Vec<u8>>, Subnodes) {
    let mut nodes: Vec<_> = ndb.nbt.iter().filter(|(nid, _)| *nid & 0x1F == kind).map(|(nid, e)| (*nid, *e)).collect();
    nodes.sort_by_key(|n| n.0);
    for (_, e) in nodes {
        let blocks = ndb.data_blocks(e.data).unwrap();
        if blocks.len() == 1 && Heap::new(blocks.clone()).is_ok_and(|h| fits(&h)) {
            return (blocks, ndb.subnodes(e.sub).unwrap());
        }
    }
    panic!("the mailbox has no such node");
}

#[test]
fn what_is_wrong_inside_a_node_is_found_and_named() {
    let mut ndb = Ndb::open(Box::new(Cursor::new(save(&mut mailbox()).0))).unwrap();

    // A message's properties.
    // One whose index is a single level, so that its records are where the header says.
    let (blocks, subs) = node(&mut ndb, 0x04, |h| h.alloc(h.root).is_some_and(|header| header.get(3) == Some(&0)));
    assert_eq!(look(&mut ndb, blocks.clone(), &subs, Holds::Properties), Ok(()));
    let heap = Heap::new(blocks.clone()).unwrap();
    let block = &heap.blocks[0];
    let map = usize::from(u16_at(block, 0).unwrap());
    let header = at(block, heap.alloc(heap.root).unwrap());
    let leaf_hid = u32_at(block, header + 4).unwrap();
    let leaf = at(block, heap.alloc(leaf_hid).unwrap());
    let records = heap.alloc(leaf_hid).unwrap().len() / 8;
    // A record whose value is in the heap.
    let in_heap = (0..records)
        .find(|i| {
            let (ty, value) = (u16_at(block, leaf + i * 8 + 2).unwrap(), u32_at(block, leaf + i * 8 + 4).unwrap());
            ptype::fixed_size(ty).is_none() && value != 0 && value & 0x1F == 0
        })
        .unwrap();
    let swapped = |b: &mut Vec<u8>| {
        let (first, second) = (b[leaf..leaf + 8].to_vec(), b[leaf + 8..leaf + 16].to_vec());
        b[leaf..leaf + 8].copy_from_slice(&second);
        b[leaf + 8..leaf + 16].copy_from_slice(&first);
    };
    type Change<'a> = Box<dyn Fn(&mut Vec<u8>) + 'a>;
    let cases: Vec<(Change, &str)> = vec![
        (Box::new(|b| b[2] = 0), "heap's signature"),
        (Box::new(|b| b[3] = 0x7C), "another kind"),
        (Box::new(|b| b[..2].copy_from_slice(&[0xFF, 0xFF])), "page map outside"),
        (Box::new(|b| b[..2].copy_from_slice(&[4, 0])), "inside its header"),
        (Box::new(|b| b[map + 6..map + 8].copy_from_slice(&[0xFF, 0xFF])), "out of order or outside"),
        (Box::new(|b| b[map] = 0xFF), "runs past the block"),
        (Box::new(|b| b[header] = 0), "index header"),
        (Box::new(|b| b[header + 1] = 4), "wrong size"),
        (Box::new(|b| b[header + 3] = 9), "more levels"),
        (Box::new(|b| b[header + 4..header + 8].copy_from_slice(&(0x7FFu32 << 5).to_le_bytes())), "not in the heap"),
        (Box::new(swapped), "keys out of order"),
        (Box::new(|b| b[leaf + in_heap * 8 + 4..leaf + in_heap * 8 + 8].copy_from_slice(&(0x7FFu32 << 5).to_le_bytes())), "not in the heap"),
        (Box::new(|b| b[leaf + in_heap * 8 + 4..leaf + in_heap * 8 + 8].copy_from_slice(&0x7FFFu32.to_le_bytes())), "which is not there"),
    ];
    for (change, want) in &cases {
        let mut bad = blocks.clone();
        change(&mut bad[0]);
        let found = look(&mut ndb, bad, &subs, Holds::Properties);
        assert!(found.as_ref().is_err_and(|e| e.contains(want)), "{want}: {found:?}");
    }

    // A folder's table of messages, with rows.
    let rows_of = |h: &Heap| {
        h.alloc(h.root)
            .and_then(|info| u32_at(info, 10))
            .and_then(|index| h.alloc(index))
            .filter(|i| i.get(3) == Some(&0))
            .and_then(|i| u32_at(i, 4))
            .is_some_and(|r| r != 0)
    };
    let (blocks, subs) = node(&mut ndb, 0x0E, rows_of);
    assert_eq!(look(&mut ndb, blocks.clone(), &subs, Holds::Table), Ok(()));
    let heap = Heap::new(blocks.clone()).unwrap();
    let block = &heap.blocks[0];
    let info = at(block, heap.alloc(heap.root).unwrap());
    let index = at(block, heap.alloc(u32_at(block, info + 10).unwrap()).unwrap());
    let leaf_hid = u32_at(block, index + 4).unwrap();
    let leaf = at(block, heap.alloc(leaf_hid).unwrap());
    let last = heap.alloc(leaf_hid).unwrap().len() - 8;
    let cases: Vec<(Change, &str)> = vec![
        (Box::new(|b| b[info] = 0), "header is not the size"),
        (Box::new(|b| b[info + 1] += 1), "header is not the size"),
        (Box::new(|b| b[info + 8] += 1), "laid out"),
        (Box::new(|b| b[info + 2..info + 4].copy_from_slice(&[0xFF, 0xFF])), "laid out"),
        (
            Box::new(|b| {
                let first = b[info + 22..info + 30].to_vec();
                let second = b[info + 30..info + 38].to_vec();
                b[info + 22..info + 30].copy_from_slice(&second);
                b[info + 30..info + 38].copy_from_slice(&first);
            }),
            "out of order",
        ),
        (Box::new(|b| b[info + 22 + 4] = 0xF0), "not where a value of its size"),
        (Box::new(|b| b[info + 22 + 6] = 3), "not where a value of its size"),
        (Box::new(|b| b[info + 22 + 7] = b[info + 30 + 7]), "a bit that is another column's"),
        (Box::new(|b| b[info + 22 + 7] = 0xFF), "a bit that is another column's"),
        (Box::new(|b| b[info + 10..info + 14].copy_from_slice(&(0x7FFu32 << 5).to_le_bytes())), "not in the heap"),
        (Box::new(|b| b[info + 14..info + 18].copy_from_slice(&(0x7FFu32 << 5).to_le_bytes())), "rows are not in the heap"),
        (Box::new(|b| b[info + 14..info + 18].copy_from_slice(&0x7FFFu32.to_le_bytes())), "which is not there"),
        (Box::new(|b| b[leaf + 4..leaf + 8].copy_from_slice(&0xFFFFu32.to_le_bytes())), "past the last row"),
        (Box::new(|b| b[leaf + last + 3] = 0x7F), "the row index says it is"),
    ];
    for (change, want) in &cases {
        let mut bad = blocks.clone();
        change(&mut bad[0]);
        let found = look(&mut ndb, bad, &subs, Holds::Table);
        assert!(found.as_ref().is_err_and(|e| e.contains(want)), "{want}: {found:?}");
    }
}

#[test]
fn garbage_inside_a_node_is_found_or_passed_never_a_panic() {
    let bytes = save(&mut mailbox()).0;
    let mut ndb = Ndb::open(Box::new(Cursor::new(bytes))).unwrap();
    let nodes: Vec<_> = ndb.nbt.iter().map(|(nid, e)| (*nid, *e)).filter(|(nid, _)| holds(*nid) != Holds::Other).collect();
    let (mut tried, mut found) = (0u32, 0u32);
    for (nid, e) in nodes {
        let blocks = ndb.data_blocks(e.data).unwrap();
        let subs = ndb.subnodes(e.sub).unwrap();
        let len = blocks[0].len();
        // Every byte of a small heap, and a spread of a large one.
        for i in (0..len).step_by(len / 400 + 1) {
            for value in [0u8, 0xFF, blocks[0][i] ^ 1, blocks[0][i] ^ 0x80] {
                let mut bad = blocks.clone();
                bad[0][i] = value;
                tried += 1;
                found += u32::from(look(&mut ndb, bad, &subs, holds(nid)).is_err());
            }
        }
        // Cut short anywhere, and nothing at all.
        for cut in (0..len).step_by(len / 50 + 1) {
            let _ = look(&mut ndb, vec![blocks[0][..cut].to_vec()], &subs, holds(nid));
        }
        let _ = look(&mut ndb, Vec::new(), &subs, holds(nid));
    }
    // Much of a heap is the values themselves, which may be anything.
    assert!(tried > 1000 && found > tried / 20, "{found} of {tried}");
}

#[test]
fn a_save_is_stopped_by_what_it_did_wrong_and_not_by_what_was_wrong_before() {
    // Text of two bytes a letter with an odd number of bytes, on every attachment: what a
    // file from another program might have, and what this program never writes.
    let odd = || PLANTED.with(|p| *p.borrow_mut() = vec![Raw { id: 0x9F02, ty: ptype::STRING, data: vec![0x41, 0, 0x42] }]);
    let even = || PLANTED.with(|p| p.borrow_mut().clear());
    let inside = |bytes: &[u8]| {
        let found = check(&mut Cursor::new(bytes)).unwrap();
        assert!(found.problems.is_empty(), "{:?}", found.problems);
        found.inside
    };
    odd();
    let bytes = write_store(Cursor::new(Vec::new()), &mut mailbox(), UID).unwrap().0.into_inner();
    even();
    let was = inside(&bytes);
    assert!(!was.is_empty() && was.iter().all(|p| p.contains("subnode") && p.contains("odd number of bytes")), "{was:?}");

    // Such a file is changed and saved all the same: its nodes go across as they are stored.
    let dir = scratch("check-inside");
    let path = dir.join("box.pst");
    std::fs::write(&path, &bytes).unwrap();
    let mut store = PstStore::open(&path).unwrap();
    let inbox = folder(&mut store, "Inbox");
    let listed = store.list(inbox.id).unwrap();
    for s in &listed {
        let id = ItemId(s.id);
        let mut item = store.load(inbox.id, id).unwrap();
        item.set_read(!item.is_read());
        if !item.attachments.is_empty() {
            item.attachments.push(Attachment::file("more.txt", "text/plain", b"more".to_vec()));
        }
        store.update(inbox.id, id, item).unwrap();
    }
    store.save(UID).unwrap();
    assert!(!store.unsaved());
    let now = std::fs::read(&path).unwrap();
    assert!(now != bytes);
    assert_eq!(inside(&now), was);

    // What a save gets wrong itself stops it, and the file stays as it was.
    odd();
    let mut item = mail("With something wrong", false);
    item.attachments.push(Attachment::file("odd.txt", "text/plain", b"odd".to_vec()));
    store.add(inbox.id, item).unwrap();
    let refused = store.save(UID);
    even();
    assert!(matches!(&refused, Err(e) if e.to_string().contains("did not pass its own check")), "{refused:?}");
    assert!(store.unsaved() && std::fs::read(&path).unwrap() == now);
    // The same change without the mistake is saved.
    store.save(UID).unwrap();
    assert_eq!(inside(&std::fs::read(&path).unwrap()), was);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_heap_says_how_full_each_of_its_blocks_is() {
    // Two blocks: the first nearly three quarters full, the second with one value in it.
    let mut heap = crate::ltp_write::Heap::new(0xBC);
    let mut root = 0;
    for _ in 0..3 {
        root = heap.alloc(&[7u8; 3000]).unwrap();
    }
    let blocks = heap.finish(root);
    assert_eq!(blocks.len(), 2);
    assert!(Heap::new(blocks.clone()).is_ok());
    // rgbFillLevel ([MS-PST] 2.3.1.2): the first block in the low half of the first byte.
    assert_eq!(blocks[0][8..12], [0x02, 0, 0, 0]);

    let wrong = |at: usize, value: u8| {
        let mut bad = blocks.clone();
        bad[0][at] = value;
        Heap::new(bad).err().unwrap()
    };
    // The two halves the other way round: each block is said to be what the other is.
    assert!(wrong(8, 0x20).contains("more room than it has"), "{}", wrong(8, 0x20));
    assert!(wrong(8, 0x0F).contains("fuller than it is"));
    assert!(wrong(8, 0x52).contains("block 1") && wrong(8, 0x52).contains("fuller than it is"));
    // A level for a block the heap does not have.
    assert!(wrong(9, 0x01).contains("block 2, which it does not have"));
    assert!(wrong(11, 0x10).contains("block 7, which it does not have"));
    // One step off is how another program may count the edge of a level.
    let mut near = blocks.clone();
    near[0][8] = 0x13;
    assert!(Heap::new(near).is_ok());
    // A block that has given allocations up may have more room than its length shows.
    let mut freed = blocks.clone();
    let map = usize::from(u16_at(&freed[0], 0).unwrap());
    freed[0][map + 2] = 1;
    freed[0][8] = 0x00;
    assert!(Heap::new(freed).is_ok());
}

#[test]
fn a_check_that_is_told_to_stop_says_nothing_of_the_file() {
    let bytes = save(&mut mailbox()).0;
    let progress = openmailclient_model::Progress::default();
    assert!(crate::check_until(&mut Cursor::new(&bytes), Some(&progress)).unwrap().sound());
    progress.stop();
    assert!(matches!(crate::check_until(&mut Cursor::new(&bytes), Some(&progress)), Err(crate::PstError::Stopped)));
    // Asked without anyone to stop it, it runs to the end as before.
    assert!(check(&mut Cursor::new(&bytes)).unwrap().sound());
}
