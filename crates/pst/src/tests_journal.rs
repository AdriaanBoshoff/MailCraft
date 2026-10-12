//! The journal of unsaved changes: written as changes are made, played back after a run that
//! ended without saving, and doubted like any other file.

use crate::journal::{Diff, Op};
use crate::tests_edit::{Whole, folder, same_mailbox, scratch, whole};
use crate::tests_store::{UID, mail, mailbox, save};
use crate::{PstStore, journal_beside, set_aside};
use openmailclient_model::{Attachment, Flag, Item, ItemId, NameId, NamedKey, PropBag, PropValue, Recovery, Store, pid};
use std::path::{Path, PathBuf};

fn on_disk(name: &str) -> (PathBuf, PathBuf, Vec<u8>) {
    let dir = scratch(name);
    let path = dir.join("box.pst");
    let bytes = save(&mut mailbox()).0;
    std::fs::write(&path, &bytes).unwrap();
    (dir, path, bytes)
}

/// Every kind of change, several of each. Returns how many requests that was.
fn busy(store: &mut PstStore) -> u32 {
    let (inbox, sent) = (folder(store, "Inbox"), folder(store, "Sent Items"));
    let listed = store.list(inbox.id).unwrap();
    let (first, second) = (ItemId(listed[0].id), ItemId(listed[1].id));
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_read(!item.is_read());
    item.set_flag(Flag::Flagged);
    item.categories = vec!["Blue".to_string(), "Later".to_string()];
    store.update(inbox.id, first, item).unwrap();
    // A message moved: taken out of one folder and put into another.
    let moved = store.remove(inbox.id, second).unwrap();
    store.add(sent.id, moved).unwrap();
    // A new message, changed once more after it was added.
    let mut new = mail("Written here", false);
    new.attachments.push(Attachment::file("notes.txt", "text/plain", b"a few notes".to_vec()));
    let id = store.add(inbox.id, new.clone()).unwrap();
    new.set_subject("Written here, and changed");
    new.attachments.clear();
    store.update(inbox.id, id, new).unwrap();
    // A message deleted for good.
    let last = ItemId(store.list(sent.id).unwrap()[0].id);
    store.remove(sent.id, last).unwrap();
    // Folders made, renamed, moved and deleted.
    let made = store.create_folder(inbox.id, "Made here").unwrap();
    let inner = store.create_folder(made, "Inside").unwrap();
    store.add(inner, mail("In a new folder", true)).unwrap();
    store.rename_folder(made, "Made here, renamed").unwrap();
    store.move_folder(made, sent.id).unwrap();
    let doomed = store.create_folder(inbox.id, "Short-lived").unwrap();
    store.delete_folder(doomed).unwrap();
    13
}

/// What was played back is what was there: every folder and every message, value for value.
fn alike(want: &Whole, got: &Whole) {
    assert_eq!(got.len(), want.len());
    for ((path, w, w_items), (g_path, g, g_items)) in want.iter().zip(got) {
        assert_eq!(g_path, path);
        assert_eq!(
            (g.id, g.parent, &g.name, g.total, g.unread, g.has_children),
            (w.id, w.parent, &w.name, w.total, w.unread, w.has_children),
            "{path}"
        );
        assert_eq!(g_items.len(), w_items.len(), "{path}");
        for ((ws, wi), (gs, gi)) in w_items.iter().zip(g_items) {
            assert_eq!((gs.id, &gs.subject, gs.read, gs.flag, &gs.categories), (ws.id, &ws.subject, ws.read, ws.flag, &ws.categories), "{path}");
            assert!(gi == wi, "{path}: {}", ws.subject);
        }
    }
}

fn none() -> Recovery {
    Recovery::default()
}

#[test]
fn changes_come_back_after_a_run_that_ended_without_saving() {
    let (dir, path, bytes) = on_disk("journal-crash");
    let mut store = PstStore::open(&path).unwrap();
    // Nothing to bring back, and looking for it leaves nothing behind.
    assert_eq!(store.recover().unwrap(), none());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    let made = busy(&mut store);
    let want = whole(&mut store);
    assert!(journal_beside(&path).exists() && store.journal_trouble().is_none());
    assert!(std::fs::read(&path).unwrap() == bytes, "the data file is not written by a change");
    // The run ends here, nothing saved.
    drop(store);

    let mut back = PstStore::open(&path).unwrap();
    assert!(!back.unsaved());
    assert_eq!(back.recover().unwrap(), Recovery { applied: made, ..none() });
    assert!(back.unsaved());
    alike(&want, &whole(&mut back));
    assert!(std::fs::read(&path).unwrap() == bytes);
    // Asking again does nothing: the changes are there.
    assert_eq!(back.recover().unwrap(), none());

    // The journal goes on from where it was: one more change, one more run that ends badly.
    let inbox = folder(&mut back, "Inbox");
    back.create_folder(inbox.id, "After the first crash").unwrap();
    let want = whole(&mut back);
    drop(back);
    let mut again = PstStore::open(&path).unwrap();
    assert_eq!(again.recover().unwrap(), Recovery { applied: made + 1, ..none() });
    alike(&want, &whole(&mut again));

    // Saved: the changes are in the file and the journal is gone. Only the file as it was
    // before its first save stays beside it.
    again.save(UID).unwrap();
    assert!(!journal_beside(&path).exists() && !set_aside(&path).exists());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    drop(again);
    let mut saved = PstStore::open(&path).unwrap();
    assert_eq!(saved.recover().unwrap(), none());
    same_mailbox(&want, &whole(&mut saved));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_journal_cut_short_gives_what_came_before_the_cut() {
    let (dir, path, _) = on_disk("journal-torn");
    let mut store = PstStore::open(&path).unwrap();
    let made = busy(&mut store);
    let want = whole(&mut store);
    let inbox = folder(&mut store, "Inbox");
    store.create_folder(inbox.id, "The last thing asked for").unwrap();
    drop(store);
    // The last record did not reach the disk whole.
    let journal = journal_beside(&path);
    let full = std::fs::read(&journal).unwrap();
    std::fs::write(&journal, &full[..full.len() - 3]).unwrap();

    let mut back = PstStore::open(&path).unwrap();
    assert_eq!(back.recover().unwrap(), Recovery { applied: made, lost: 1, ..none() });
    alike(&want, &whole(&mut back));
    // What was found is kept beside the file, and the journal is cut back to what is whole.
    assert_eq!(std::fs::read(set_aside(&path)).unwrap().len(), full.len() - 3);
    drop(back);
    let cut = std::fs::read(&journal).unwrap();
    assert!(cut.len() < full.len() - 3 && full.starts_with(&cut));

    // A record damaged in the middle: what is before it stands, the rest is lost, and nothing
    // of it is half done.
    let mut bad = cut.clone();
    let at = bad.len() / 2;
    bad[at] ^= 0x40;
    std::fs::write(&journal, &bad).unwrap();
    let mut back = PstStore::open(&path).unwrap();
    let found = back.recover().unwrap();
    assert!(found.applied > 0 && found.applied < made && found.lost >= 1, "{found:?}");
    whole(&mut back);
    drop(back);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_journal_for_another_file_is_left_alone() {
    let (dir, path, _) = on_disk("journal-stale");
    let mut store = PstStore::open(&path).unwrap();
    busy(&mut store);
    drop(store);
    let journal = journal_beside(&path);
    let old = std::fs::read(&journal).unwrap();
    // The data file is written by someone else before it is opened here again.
    let mut other = mailbox();
    let inbox = other.folder_by_kind(openmailclient_model::FolderKind::Inbox).unwrap();
    other.add(inbox, mail("Arrived in the meantime", false)).unwrap();
    std::fs::write(&path, save(&mut other).0).unwrap();

    let mut store = PstStore::open(&path).unwrap();
    let before = whole(&mut store);
    assert_eq!(store.recover().unwrap(), Recovery { stale: true, ..none() });
    assert!(!store.unsaved());
    alike(&before, &whole(&mut store));
    assert!(std::fs::read(&journal).unwrap() == old);
    // A change starts a journal of its own; the old one is kept under another name.
    let inbox = folder(&mut store, "Inbox");
    store.create_folder(inbox.id, "New").unwrap();
    assert!(std::fs::read(set_aside(&path)).unwrap() == old);
    // Thrown away: the changes and their journal go, the old journal stays.
    store.discard();
    assert!(!store.unsaved() && !journal.exists() && set_aside(&path).exists());
    alike(&before, &whole(&mut store));
    drop(store);

    // Not a journal at all, and one whose records say nonsense under a good checksum.
    let ident = crate::journal::ident(&path).unwrap();
    let mut head = b"OMCJRNL1".to_vec();
    head.extend_from_slice(&ident.0.to_le_bytes());
    head.extend_from_slice(&ident.1.to_le_bytes());
    let record = |payload: &[u8]| {
        let mut r = u32::try_from(payload.len()).unwrap().to_le_bytes().to_vec();
        r.extend_from_slice(&crate::write::crc(payload).to_le_bytes());
        r.extend_from_slice(payload);
        r
    };
    let mut huge = head.clone();
    huge.extend_from_slice(&[0xFF; 8]);
    let cases: Vec<(Vec<u8>, Recovery)> = vec![
        (Vec::new(), Recovery { stale: true, ..none() }),
        (b"OMCJRNL1 and then something else entirely".to_vec(), Recovery { stale: true, ..none() }),
        (head.clone(), none()),
        (huge, Recovery { lost: 1, ..none() }),
        ([head.clone(), record(&[])].concat(), Recovery { lost: 1, ..none() }),
        ([head.clone(), record(&[9, 9, 9])].concat(), Recovery { lost: 1, ..none() }),
        // A request that cannot be done: a folder that is not there, a message no one took out.
        ([head.clone(), record(&Op::DeleteFolder { folder: 0xDEAD_BEEF }.encode().unwrap())].concat(), Recovery { lost: 1, ..none() }),
        ([head.clone(), record(&Op::Put { folder: 0x122, nid: 77 }.encode().unwrap()), record(&[1])].concat(), Recovery { lost: 2, ..none() }),
        // A count that asks for more than the record holds.
        ([head.clone(), record(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF])].concat(), Recovery { lost: 1, ..none() }),
    ];
    for (bytes, want) in cases {
        std::fs::write(&journal, &bytes).unwrap();
        let mut store = PstStore::open(&path).unwrap();
        assert_eq!(store.recover().unwrap(), want, "{bytes:?}");
        assert!(!store.unsaved());
        alike(&before, &whole(&mut store));
        drop(store);
        // A journal nothing could be taken from is not left to be tried again.
        assert_eq!(journal.exists(), want.stale, "{bytes:?}");
        let _ = std::fs::remove_file(&journal);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_change_to_a_message_costs_what_changed() {
    let (dir, path, _) = on_disk("journal-small");
    let mut store = PstStore::open(&path).unwrap();
    let inbox = folder(&mut store, "Inbox");
    let mut big = mail("With a large attachment", false);
    big.attachments.push(Attachment::file("large.bin", "application/octet-stream", vec![0x5A; 600_000]));
    let id = store.add(inbox.id, big).unwrap();
    store.save(UID).unwrap();
    let size = |path: &Path| std::fs::metadata(journal_beside(path)).map(|m| m.len()).unwrap_or(0);
    assert_eq!(size(&path), 0);

    // Read, flagged, and moved to another folder: none of it writes the attachment again.
    let mut item = store.load(inbox.id, id).unwrap();
    item.set_read(true);
    item.set_flag(Flag::Flagged);
    store.update(inbox.id, id, item).unwrap();
    let sent = folder(&mut store, "Sent Items");
    let moved = store.remove(inbox.id, id).unwrap();
    store.add(sent.id, moved).unwrap();
    let want = whole(&mut store);
    assert!(size(&path) < 1_000, "{}", size(&path));
    drop(store);
    let mut back = PstStore::open(&path).unwrap();
    assert_eq!(back.recover().unwrap(), Recovery { applied: 3, ..none() });
    alike(&want, &whole(&mut back));
    assert_eq!(back.load(sent.id, id).unwrap().attachments[0].data.len(), 600_000);
    drop(back);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn two_runs_do_not_write_one_journal() {
    let (dir, path, _) = on_disk("journal-held");
    let mut first = PstStore::open(&path).unwrap();
    let made = busy(&mut first);
    let want = whole(&mut first);
    // A second run opens the same file while the first has unsaved changes.
    let mut second = PstStore::open(&path).unwrap();
    assert_eq!(second.recover().unwrap(), Recovery { held: true, ..none() });
    assert!(!second.unsaved() && second.journal_trouble().is_none());
    let inbox = folder(&mut second, "Inbox");
    second.create_folder(inbox.id, "From the second run").unwrap();
    // Its change is made, and it says that it is in memory only.
    assert!(second.unsaved() && second.journal_trouble().is_some());
    drop(second);
    // One that never looked for a journal does not take the first run's away either.
    let mut third = PstStore::open(&path).unwrap();
    let inbox = folder(&mut third, "Inbox");
    third.create_folder(inbox.id, "From the third run").unwrap();
    assert!(third.journal_trouble().is_some() && !set_aside(&path).exists());
    drop(third);
    drop(first);
    let mut back = PstStore::open(&path).unwrap();
    assert_eq!(back.recover().unwrap(), Recovery { applied: made, ..none() });
    alike(&want, &whole(&mut back));
    drop(back);
    std::fs::remove_dir_all(&dir).unwrap();
}

fn every_kind() -> Item {
    let mut inner = mail("Attached message", true);
    inner.attachments.push(Attachment::file("deep.txt", "text/plain", vec![1, 2, 3]));
    let mut item = mail("Every kind of value", false);
    for (id, v) in [
        (0x6001, PropValue::Int16(-2)),
        (0x6002, PropValue::Int32(i32::MIN)),
        (0x6003, PropValue::Int64(i64::MIN)),
        (0x6004, PropValue::Bool(true)),
        (0x6005, PropValue::Float64(-0.5)),
        (0x6006, PropValue::String("\u{6771}\u{4eac} \u{1F4EC}".to_string())),
        (0x6007, PropValue::Binary(vec![0, 255, 7])),
        (0x6008, PropValue::Time(-1)),
        (0x6009, PropValue::Guid([7; 16])),
        (0x600A, PropValue::MultiInt32(vec![1, -1, 0])),
        (0x600B, PropValue::MultiString(vec![String::new(), "two".to_string()])),
        (0x600C, PropValue::MultiBinary(vec![vec![], vec![9; 40]])),
        (0x600D, PropValue::Other { ty: 0x0006, raw: vec![1, 2, 3, 4, 5, 6, 7, 8] }),
    ] {
        item.props.set(id, v);
    }
    item.add_recipient(1, "Robin Vale", "robin@example.org");
    item.recipients.push(PropBag::default());
    item.categories = vec!["Blue".to_string()];
    item.named = vec![
        (NamedKey { guid: [3; 16], name: NameId::Id(0x8501) }, PropValue::Int32(5)),
        (NamedKey { guid: [4; 16], name: NameId::Name("x-of-ours".to_string()) }, PropValue::String("yes".to_string())),
    ];
    item.attachments.push(Attachment::file("a.bin", "application/octet-stream", vec![0xAB; 300]));
    item.attachments.push(Attachment { props: PropBag::default(), data: Vec::new(), embedded: Some(Box::new(inner)) });
    item
}

#[test]
fn records_say_what_was_asked_for_and_junk_is_no_record() {
    let item = every_kind();
    let mut changed = item.clone();
    changed.props.remove(0x6001);
    changed.props.set(0x6002, PropValue::Int32(4));
    changed.recipients.pop();
    changed.attachments.remove(0);
    changed.categories.clear();
    changed.named.pop();
    let diff = Diff::between(&item, &changed);
    let mut made = item.clone();
    Diff::between(&item, &changed).apply(&mut made);
    assert_eq!(made, changed);
    // Nothing changed: nothing but the two ids is written.
    let nothing = Op::Update { folder: 1, id: 2, diff: Diff::between(&item, &item) };
    assert!(nothing.encode().unwrap().len() < 40);

    let ops = [
        Op::Update { folder: u64::MAX, id: 7, diff },
        Op::Add { folder: 3, item },
        Op::Put { folder: 3, nid: u32::MAX },
        Op::Remove { folder: 3, id: 9 },
        Op::CreateFolder { parent: 1, name: "\u{6771}\u{4eac}".to_string() },
        Op::RenameFolder { folder: 1, name: String::new() },
        Op::DeleteFolder { folder: 1 },
        Op::MoveFolder { folder: 1, parent: 2 },
    ];
    for op in &ops {
        let bytes = op.encode().unwrap();
        assert_eq!(Op::decode(&bytes).as_ref(), Some(op));
        // Cut anywhere, or with something after it, it is no record; with any byte changed it
        // is another request or none, and reading it never fails.
        for cut in 0..bytes.len() {
            assert!(Op::decode(&bytes[..cut]).is_none(), "{op:?} cut at {cut}");
        }
        assert!(Op::decode(&[bytes.clone(), vec![0]].concat()).is_none());
        for at in 0..bytes.len() {
            for bit in [0x01, 0x80, 0xFF] {
                let mut bad = bytes.clone();
                bad[at] ^= bit;
                let _ = Op::decode(&bad);
            }
        }
    }
    // A message attached to a message attached to a message, further down than is followed,
    // is not journalled, and such a record is not read.
    let mut deep = mail("bottom", true);
    for _ in 0..40 {
        let mut outer = mail("outer", true);
        outer.attachments.push(Attachment { props: PropBag::default(), data: Vec::new(), embedded: Some(Box::new(deep)) });
        deep = outer;
    }
    assert!(Op::Add { folder: 1, item: deep }.encode().is_none());
    assert_eq!(pid::SUBJECT, 0x0037);
}
