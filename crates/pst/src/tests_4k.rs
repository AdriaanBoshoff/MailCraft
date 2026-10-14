//! Data files with 4K pages from our own writer: written, read back, changed and saved as the
//! kind they are, and held against the structure check each time. The layout is in no
//! specification; what is written is what files Outlook wrote were measured to be.

use crate::check::check;
use crate::ndb::{Ndb, Ver};
use crate::tests_edit::{folder, same_mailbox, scratch, whole};
use crate::tests_store::{UID, mail, mailbox, save};
use crate::write::{NdbWriter, Sub, big};
use crate::{PstError, PstStore, write_store_4k};
use openmailclient_model::{Attachment, FolderKind, ItemId, MemStore, Store};
use std::io::Cursor;

fn sound(bytes: &[u8]) {
    let found = check(&mut Cursor::new(bytes)).unwrap();
    assert!(found.sound(), "{:?} {:?}", found.problems, found.inside);
}

fn four_k(store: &mut dyn Store) -> Vec<u8> {
    let (out, _) = write_store_4k(Cursor::new(Vec::new()), store, UID, None).unwrap();
    let bytes = out.into_inner();
    sound(&bytes);
    bytes
}

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

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

#[test]
fn a_mailbox_written_with_4k_pages_is_the_same_mailbox() {
    let bytes = four_k(&mut mailbox());
    // HEADER: "SM", wVer 36, wVerClient 19 (what Outlook's repair tool asks of a file with
    // 4K pages), blocks not encoded (bCryptMethod 0, as in the files measured), the two maps of the header zero, the end of the
    // file at 184 and one section of 4072 * 8 * 512 bytes after the first map at 0x22000.
    assert_eq!(&bytes[8..10], b"SM");
    assert_eq!((u16_at(&bytes, 10), u16_at(&bytes, 12), bytes[513]), (36, 19, 0));
    assert!(bytes[256..512].iter().all(|b| *b == 0));
    assert_eq!(u64_at(&bytes, 184) as usize, bytes.len());
    assert_eq!(bytes.len(), 0x22000 + big::SECTION);
    // The allocation map and the density list end in trailers of 24 bytes; the density list
    // is empty and carries the header's bidNextP.
    assert_eq!(&bytes[0x22000 + 4072..0x22000 + 4074], &[0x84, 0x84]);
    assert_eq!(u64_at(&bytes, 0x22000 + 4080), 0x22000);
    let dlist = &bytes[0x21000..0x22000];
    assert!(dlist[..4072].iter().all(|b| *b == 0));
    assert_eq!((dlist[4072], u64_at(dlist, 4080), u16_at(dlist, 4088)), (0x86, u64_at(&bytes, 32), 1));

    let mut back = PstStore::from_bytes(bytes).unwrap();
    assert_eq!(back.ndb.ver, Ver::Unicode4k);
    assert_eq!(back.display_name(), "Synthetic mailbox");
    // The same mailbox as the Unicode file made from it, to the last property.
    let mut wide = PstStore::from_bytes(save(&mut mailbox()).0).unwrap();
    same_mailbox(&whole(&mut wide), &whole(&mut back));

    // Every contents table, the template new ones are made from too, has the subnodes 0x521
    // and 0x541: empty heaps with the client signature 0xA5.
    let contents: Vec<u32> = back.ndb.nbt.keys().copied().filter(|n| n & 0x1F == 0x0E).collect();
    assert!(contents.contains(&0x60E) && contents.len() > 5);
    for nid in contents {
        let e = *back.ndb.nbt.get(&nid).unwrap();
        let subs = back.ndb.subnodes(e.sub).unwrap();
        for at in [0x521u32, 0x541] {
            let s = subs.get(&at).unwrap_or_else(|| panic!("{nid:X} has no {at:X}"));
            let heap = back.ndb.data(s.0).unwrap();
            assert_eq!((heap.len(), heap[2], heap[3]), (18, 0xEC, 0xA5));
        }
    }
}

#[test]
fn large_parts_take_blocks_of_64k_and_trees_of_4k_pages() {
    let mut w = NdbWriter::of(Cursor::new(Vec::new()), 1, Ver::Unicode4k).unwrap();
    assert_eq!(w.max_data(), 65512);
    assert!(matches!(w.data([vec![0u8; 65513]]), Err(PstError::Write(_))));
    // 300 full blocks under an XBLOCK (more than one section of 16 MB holds), and a node tree and a block tree of more than one
    // page (126 and 169 entries a page), so both have a level above their leaves.
    let blocks: Vec<Vec<u8>> = (0..300u32).map(|i| pattern(65512, i + 10)).collect();
    let data = w.data(blocks.iter()).unwrap();
    let mut subs = Vec::new();
    for i in 0..300u32 {
        subs.push(Sub { nid: (0x1000 + i) << 5 | 0x1F, data: w.data([pattern(40, 7 + i)]).unwrap(), sub: 0 });
    }
    let sub = w.subnodes(subs).unwrap();
    w.node(0x20_0024, 0x122, data, sub);
    for i in 0..200u32 {
        let d = w.data([pattern(64, 500 + i)]).unwrap();
        w.node((0x2000 + i) << 5 | 4, 0x122, d, 0);
    }
    let bytes = w.finish().unwrap().into_inner();
    // Two sections.
    assert_eq!(bytes.len(), 0x22000 + 2 * big::SECTION);
    let found = check(&mut Cursor::new(&bytes)).unwrap();
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    assert_eq!(found.nodes, 201);
    // The root of the node tree is a branch page: cEnt and cEntMax are u16, then cbEnt 24
    // and cLevel 1.
    let root = u64_at(&bytes, 224) as usize;
    let page = &bytes[root..root + 4096];
    assert_eq!((u16_at(page, 4056), u16_at(page, 4058), page[4060], page[4061]), (2, 169, 24, 1));
    let mut ndb = Ndb::open(Box::new(Cursor::new(bytes))).unwrap();
    assert_eq!((ndb.ver, ndb.nbt.len()), (Ver::Unicode4k, 201));
    let n = *ndb.nbt.get(&0x20_0024).unwrap();
    assert!(ndb.data_blocks(n.data).unwrap() == blocks);
    assert_eq!(ndb.subnodes(n.sub).unwrap().len(), 300);
    for i in [0u32, 57, 199] {
        let e = *ndb.nbt.get(&((0x2000 + i) << 5 | 4)).unwrap();
        assert_eq!(ndb.data(e.data).unwrap(), pattern(64, 500 + i));
    }
}

/// A folder of many messages: its contents table has more rows than one block of 64 KB
/// holds, and its heap more than one block.
#[test]
fn a_large_folder_and_a_large_attachment_are_read_back() {
    let mut s = MemStore::with_default_folders("Big");
    let inbox = s.folder_by_kind(FolderKind::Inbox).unwrap();
    for i in 0..1500u32 {
        s.push_item(inbox, mail(&format!("Message {i:04}"), i % 3 == 0));
    }
    let mut it = mail("With a large attachment", false);
    it.attachments.push(Attachment::file("big.bin", "application/octet-stream", pattern(300_000, 3)));
    s.push_item(inbox, it);
    let bytes = four_k(&mut s);
    let mut back = PstStore::from_bytes(bytes).unwrap();
    let inbox = folder(&mut back, "Inbox");
    assert_eq!((inbox.total, inbox.unread), (1501, 1001));
    let rows = back.list(inbox.id).unwrap();
    assert_eq!(rows.len(), 1501);
    let mut subjects: Vec<String> = rows.iter().map(|r| r.subject.clone()).collect();
    subjects.sort();
    assert_eq!(subjects[0], "Message 0000");
    assert_eq!(subjects[1499], "Message 1499");
    // The contents table's rows are in blocks of their own, more than one.
    let nid = inbox.id.0 as u32 & !0x1F | 0x0E;
    let (_, table) = back.table_of(nid).unwrap();
    assert_eq!(table.len(), 1501);
    // Its heap has more than one block, so its HIDs name blocks past the first.
    let e = *back.ndb.nbt.get(&nid).unwrap();
    assert!(back.ndb.data_blocks(e.data).unwrap().len() > 1);
    let subs = back.ndb.subnodes(e.sub).unwrap();
    let most = subs.values().map(|s| back.ndb.data_blocks(s.0).unwrap().len()).max().unwrap();
    assert!(most > 1);
    let big_one = rows.iter().find(|r| r.subject == "With a large attachment").unwrap();
    let item = back.load(inbox.id, ItemId(big_one.id)).unwrap();
    assert_eq!(item.attachments[0].data, pattern(300_000, 3));
}

#[test]
fn a_4k_data_file_takes_changes_and_is_saved_as_it_is() {
    let dir = scratch("4k-save");
    let path = dir.join("box.pst");
    std::fs::write(&path, four_k(&mut mailbox())).unwrap();
    let mut store = PstStore::open(&path).unwrap();
    assert!(store.can_edit());
    let inbox = folder(&mut store, "Inbox");
    let first = store.list(inbox.id).unwrap()[0].id;
    let mut item = store.load(inbox.id, ItemId(first)).unwrap();
    item.set_read(true);
    store.update(inbox.id, ItemId(first), item).unwrap();
    store.save(UID).unwrap();
    // Written whole: still a file with 4K pages, sound, and the change is there.
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(u16_at(&bytes, 10), 36);
    sound(&bytes);
    let mut back = PstStore::open(&path).unwrap();
    let inbox = folder(&mut back, "Inbox");
    assert!(back.load(inbox.id, ItemId(first)).unwrap().is_read());
    assert_eq!(back.saved_in_place(), 0);
    drop(back);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn data_blocks_larger_than_a_slot_are_stored_deflated() {
    // Two messages with the same attachment, which shrinks when deflated, and one whose
    // attachment does not.
    let mut store = mailbox();
    let inbox = store.folder_by_kind(FolderKind::Inbox).unwrap();
    let same = "A line that comes again and again. ".repeat(3_000).into_bytes();
    for subject in ["Once", "Twice"] {
        let mut m = mail(subject, false);
        m.attachments.push(Attachment::file("again.txt", "text/plain", same.clone()));
        store.push_item(inbox, m);
    }
    let mut m = mail("Noise", false);
    m.attachments.push(Attachment::file("noise.bin", "", pattern(20_000, 9)));
    store.push_item(inbox, m);
    let want = whole(&mut PstStore::from_bytes(save(&mut store).0).unwrap());
    let bytes = four_k(&mut store);
    let ndb = Ndb::open(Box::new(Cursor::new(bytes.clone()))).unwrap();
    let deflated: Vec<_> = ndb.bbt.iter().filter(|(_, e)| e.cb != e.inflated).collect();
    assert!(deflated.len() >= 2, "{}", deflated.len());
    for (bid, e) in &deflated {
        // Data blocks only, each larger than one slot holds, each smaller stored.
        assert_eq!(*bid & 2, 0);
        assert!(e.inflated as usize + big::TRAILER > 512);
        assert!(e.cb < e.inflated);
    }
    // What fits in one slot is stored as it is.
    assert!(ndb.bbt.values().filter(|e| e.inflated as usize + big::TRAILER <= 512).all(|e| e.cb == e.inflated));
    // Deflated, the two attachments together take less room than one stored as it is.
    let stored: u64 = ndb.bbt.values().map(|e| u64::from(e.cb)).sum();
    let inflated: u64 = ndb.bbt.values().map(|e| u64::from(e.inflated)).sum();
    assert!(inflated - stored > same.len() as u64, "{stored} {inflated}");
    let mut back = PstStore::from_bytes(bytes).unwrap();
    same_mailbox(&want, &whole(&mut back));
    // Saved again, still deflated and the same.
    let dir = scratch("4k-deflated");
    let path = dir.join("box.pst");
    std::fs::write(&path, four_k(&mut store)).unwrap();
    let mut open = PstStore::open(&path).unwrap();
    let inbox = folder(&mut open, "Inbox");
    let first = ItemId(open.list(inbox.id).unwrap()[0].id);
    let mut item = open.load(inbox.id, first).unwrap();
    item.set_read(!item.is_read());
    open.update(inbox.id, first, item).unwrap();
    open.save(UID).unwrap();
    drop(open);
    let bytes = std::fs::read(&path).unwrap();
    sound(&bytes);
    let ndb = Ndb::open(Box::new(Cursor::new(bytes))).unwrap();
    assert!(ndb.bbt.values().any(|e| e.cb != e.inflated));
    let _ = std::fs::remove_dir_all(&dir);
}
