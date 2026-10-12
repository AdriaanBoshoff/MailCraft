//! Changes to an open `.pst`: made, saved to a new file, read back.

use crate::check::check;
use crate::ltp::Raw;
use crate::tests_store::{UID, mail, mailbox, same, save, tree};
use crate::{PstStore, Written};
use openmailclient_model::{Attachment, Flag, FolderId, FolderInfo, FolderKind, Item, ItemId, PropValue, Store, StoreError, Summary, pid, ptype};
use std::io::Cursor;

type Whole = Vec<(String, FolderInfo, Vec<(Summary, Item)>)>;

fn open() -> PstStore {
    PstStore::from_bytes(save(&mut mailbox()).0).unwrap()
}

fn folder(store: &mut PstStore, name: &str) -> FolderInfo {
    store.folders().unwrap().into_iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no folder {name}"))
}

/// Save the changes to memory and open what was written.
fn saved(store: &mut PstStore) -> PstStore {
    let (out, _) = store.save_to(Cursor::new(Vec::new()), UID).unwrap();
    let bytes = out.into_inner();
    let found = check(&mut Cursor::new(&bytes)).unwrap();
    assert!(found.sound(), "{:?}", found.problems);
    PstStore::from_bytes(bytes).unwrap()
}

/// A store's folders and messages, each message loaded.
fn whole(store: &mut PstStore) -> Whole {
    tree(store)
        .into_iter()
        .map(|(path, f, list)| {
            let items = list.into_iter().map(|s| (s.clone(), store.load(f.id, ItemId(s.id)).unwrap())).collect();
            (path, f, items)
        })
        .collect()
}

/// What was saved reads back as what was shown before saving.
fn same_mailbox(want: &Whole, got: &Whole) {
    assert_eq!(got.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), want.iter().map(|f| f.0.as_str()).collect::<Vec<_>>());
    for ((path, w, w_items), (_, g, g_items)) in want.iter().zip(got) {
        assert_eq!((g.id, g.parent, g.total, g.unread, g.has_children), (w.id, w.parent, w.total, w.unread, w.has_children), "{path}");
        assert_eq!(&g.container_class, &w.container_class, "{path}");
        assert_eq!(g_items.len(), w_items.len(), "{path}");
        assert_eq!(g.total as usize, g_items.len(), "{path}");
        for ((ws, wi), (gs, gi)) in w_items.iter().zip(g_items) {
            assert_eq!(
                (gs.id, &gs.subject, &gs.from, &gs.to, gs.time, gs.read, gs.flag),
                (ws.id, &ws.subject, &ws.from, &ws.to, ws.time, ws.read, ws.flag),
                "{path}"
            );
            assert_eq!((gs.importance, &gs.message_class), (ws.importance, &ws.message_class), "{path}");
            same(wi, gi, path);
        }
    }
}

#[test]
fn a_file_copied_without_changes_is_the_same_mailbox() {
    let mut store = open();
    assert!(store.writable() && !store.unsaved());
    let want = whole(&mut store);
    let mut back = saved(&mut store);
    same_mailbox(&want, &whole(&mut back));
    assert!(!store.unsaved());
}

#[test]
fn changes_are_kept_when_saved() {
    let mut store = open();
    let (inbox, sent) = (folder(&mut store, "Inbox"), folder(&mut store, "Sent Items"));
    let listed = store.list(inbox.id).unwrap();
    let (first, second) = (ItemId(listed[0].id), ItemId(listed[1].id));
    assert_eq!((inbox.total, inbox.unread), (2, 1));

    // A message is read and flagged, another goes to Sent Items and keeps who it is.
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_read(true);
    item.set_flag(Flag::Complete);
    item.categories.push("Blue Category".into());
    store.update(inbox.id, first, item).unwrap();
    assert!(store.unsaved());
    let moved = store.remove(inbox.id, second).unwrap();
    assert_eq!(store.add(sent.id, moved).unwrap(), second);
    assert_eq!(store.list(sent.id).unwrap().len(), 2);
    assert!(matches!(store.load(inbox.id, second), Err(StoreError::NoItem)));

    // A message is deleted for good, a new one arrives.
    let old_sent = ItemId(store.list(sent.id).unwrap()[0].id);
    store.remove(sent.id, old_sent).unwrap();
    let mut new = mail("Written here", false);
    new.attachments.push(Attachment::file("note.txt", "text/plain", b"new".to_vec()));
    let new_id = store.add(inbox.id, new).unwrap();
    assert_ne!(new_id, old_sent);

    // Folders: one made, one renamed, one moved with what is in it, one deleted.
    let made = store.create_folder(inbox.id, "Made here").unwrap();
    let projects = folder(&mut store, "Projects");
    store.rename_folder(projects.id, "Work").unwrap();
    let deep = store.folders().unwrap().into_iter().find(|f| f.parent == Some(projects.id)).unwrap();
    store.move_folder(deep.id, made).unwrap();
    assert!(store.move_folder(inbox.id, deep.id).is_err());
    let calendar = folder(&mut store, "Calendar");
    store.delete_folder(calendar.id).unwrap();
    let in_made = store.add(made, mail("In the new folder", true)).unwrap();

    let now = store.folders().unwrap();
    let by = |name: &str| now.iter().find(|f| f.name == name).unwrap();
    assert_eq!((by("Inbox").total, by("Inbox").unread, by("Inbox").has_children), (2, 1, true));
    assert_eq!((by("Sent Items").total, by("Work").has_children, by("Made here").has_children), (1, false, true));
    assert_eq!(by(&deep.name).parent, Some(made));
    assert!(!now.iter().any(|f| f.name == "Calendar" || f.name == "Projects"));
    // Each folder is followed by what is inside it.
    let at = |id: FolderId| now.iter().position(|f| f.id == id).unwrap();
    assert!(at(inbox.id) < at(made) && at(made) < at(deep.id));

    let want = whole(&mut store);
    let mut back = saved(&mut store);
    same_mailbox(&want, &whole(&mut back));
    assert!(back.list(made).unwrap().iter().any(|s| s.id == in_made.0));
    let read = back.load(inbox.id, first).unwrap();
    assert!(read.is_read() && read.flag() == Flag::Complete && read.categories.contains(&"Blue Category".to_string()));
    assert_eq!(read.attachments.len(), 4);
    assert_eq!(back.load(inbox.id, new_id).unwrap().attachments.len(), 1);
    // The deleted folder's nodes and the deleted message are not in the new file.
    assert!(!back.ndb.nbt.contains_key(&(calendar.id.0 as u32)) && !back.ndb.nbt.contains_key(&(old_sent.0 as u32)));
    // The special folders are still the ones the store names.
    assert!(back.folders().unwrap().iter().any(|f| f.kind == FolderKind::Deleted));

    // The file that was saved can be changed and saved again.
    let again = back.create_folder(made, "Deeper").unwrap();
    let it = back.remove(inbox.id, first).unwrap();
    back.add(again, it).unwrap();
    let want = whole(&mut back);
    let mut third = saved(&mut back);
    same_mailbox(&want, &whole(&mut third));
    assert_eq!(third.load(again, first).unwrap().attachments.len(), 4);
}

#[test]
fn what_this_program_does_not_understand_is_kept() {
    // A file with what our own writer never makes: a hidden (associated) message in the Inbox,
    // a property with an id no name stands for, and a folder property the model has no place for.
    let mut plain = open();
    let inbox = folder(&mut plain, "Inbox");
    let listed = plain.list(inbox.id).unwrap();
    let (inbox_nid, msg) = (inbox.id.0 as u32, listed[1].id as u32);
    let hidden = (0x7000 << 5) | 0x08;
    let text = |id: u16, s: &str| Raw { id, ty: ptype::STRING, data: s.encode_utf16().flat_map(u16::to_le_bytes).collect() };
    let entry = *plain.ndb.nbt.get(&msg).unwrap();
    let mut props: Vec<Raw> = plain.open_pc(entry).unwrap().0.into_values().collect();
    props.push(Raw { id: 0x9F00, ty: ptype::INT32, data: vec![7, 0, 0, 0] });
    let folder_entry = *plain.ndb.nbt.get(&inbox_nid).unwrap();
    let mut folder_props: Vec<Raw> = plain.open_pc(folder_entry).unwrap().0.into_values().collect();
    folder_props.push(Raw { id: 0x36DA, ty: ptype::BINARY, data: vec![9; 300] });
    let put = vec![
        (hidden, inbox_nid, vec![text(pid::MESSAGE_CLASS, "IPM.Configuration.Thing"), text(pid::SUBJECT, "hidden")]),
        (msg, inbox_nid, props),
        (inbox_nid, folder_entry.parent, folder_props),
    ];
    let (out, _) = plain.save_with(Cursor::new(Vec::new()), UID, put).unwrap();
    let mut store = PstStore::from_bytes(out.into_inner()).unwrap();
    assert!(store.load(inbox.id, ItemId(u64::from(msg))).unwrap().props.get(0x9F00).is_none());

    // The message is changed, another is added to its folder, the folder is renamed.
    let mut item = store.load(inbox.id, ItemId(u64::from(msg))).unwrap();
    item.set_read(false);
    item.props.set_str(pid::BODY, "A new body.");
    item.props.0.remove(&pid::DISPLAY_TO);
    store.update(inbox.id, ItemId(u64::from(msg)), item).unwrap();
    store.add(inbox.id, mail("One more", false)).unwrap();
    store.rename_folder(inbox.id, "In").unwrap();
    let mut back = saved(&mut store);

    let raw = |store: &mut PstStore, nid: u32| {
        let e = *store.ndb.nbt.get(&nid).unwrap();
        (e.parent, store.open_pc(e).unwrap().0)
    };
    let (parent, props) = raw(&mut back, hidden);
    assert_eq!((parent, props.len()), (inbox_nid, 2));
    let (_, props) = raw(&mut back, msg);
    assert_eq!(props.get(&0x9F00).map(|r| r.data.clone()), Some(vec![7, 0, 0, 0]));
    assert!(!props.contains_key(&pid::DISPLAY_TO));
    let (_, props) = raw(&mut back, inbox_nid);
    assert_eq!(props.get(&0x36DA).map(|r| r.data.len()), Some(300));
    let now = folder(&mut back, "In");
    assert_eq!((now.total, now.unread), (3, 3));
    let got = back.load(inbox.id, ItemId(u64::from(msg))).unwrap();
    assert_eq!((got.props.str(pid::BODY), got.is_read(), got.recipients.len()), (Some("A new body."), false, 0));
    // The list shows the change, and no longer what the message lost.
    let row = back.list(inbox.id).unwrap().into_iter().find(|s| s.id == u64::from(msg)).unwrap();
    assert!(!row.read && row.to.is_empty());
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("omc-edit-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_file_on_disk_is_replaced_whole_or_not_at_all() {
    let dir = scratch("save");
    let path = dir.join("box.pst");
    let bytes = save(&mut mailbox()).0;
    std::fs::write(&path, &bytes).unwrap();
    let mut store = PstStore::open(&path).unwrap();
    assert!(store.writable());
    // Looking does not write.
    let inbox = folder(&mut store, "Inbox");
    let first = ItemId(store.list(inbox.id).unwrap()[0].id);
    assert_eq!(store.save(UID).unwrap(), Written::default());
    assert!(std::fs::read(&path).unwrap() == bytes);

    let mut item = store.load(inbox.id, first).unwrap();
    item.set_read(true);
    store.update(inbox.id, first, item).unwrap();
    assert!(std::fs::read(&path).unwrap() == bytes, "a change is not written until it is saved");
    let done = store.save(UID).unwrap();
    assert!(done.folders > 0 && !store.unsaved());
    // Beside the file there is now the file as it was, and nothing else.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    assert!(std::fs::read(crate::original_beside(&path)).unwrap() == bytes);
    // The open store goes on from the new file, and so does anyone who opens it.
    assert!(store.load(inbox.id, first).unwrap().is_read());
    assert_eq!(folder(&mut store, "Inbox").unread, 0);
    let mut fresh = PstStore::open(&path).unwrap();
    assert!(fresh.load(inbox.id, first).unwrap().is_read());
    assert_eq!(store.save(UID).unwrap(), Written::default());
    drop(fresh);

    // Another program writes the file: the next save is refused and the file is left alone.
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_read(false);
    store.update(inbox.id, first, item).unwrap();
    let theirs = std::fs::read(&path).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();
    assert!(store.save(UID).is_err());
    assert!(store.unsaved() && std::fs::read(&path).unwrap() == theirs);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    // The file as it first was is not written over by later saves.
    assert!(std::fs::read(crate::original_beside(&path)).unwrap() == bytes);
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn a_file_another_program_has_open_is_not_replaced() {
    let dir = scratch("held");
    let path = dir.join("box.pst");
    std::fs::write(&path, save(&mut mailbox()).0).unwrap();
    let mut store = PstStore::open(&path).unwrap();
    let inbox = folder(&mut store, "Inbox");
    let first = ItemId(store.list(inbox.id).unwrap()[0].id);
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_read(true);
    store.update(inbox.id, first, item).unwrap();
    let before = std::fs::read(&path).unwrap();
    let theirs = std::fs::File::open(&path).unwrap();
    assert!(store.save(UID).is_err());
    assert!(store.unsaved() && std::fs::read(&path).unwrap() == before);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    // The store still reads the file it had, with the change on top.
    assert!(store.load(inbox.id, first).unwrap().is_read());
    drop(theirs);
    store.save(UID).unwrap();
    assert!(PstStore::open(&path).unwrap().load(inbox.id, first).unwrap().is_read());
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_ost_is_only_read() {
    let dir = scratch("ost");
    let path = dir.join("copy.ost");
    std::fs::write(&path, save(&mut mailbox()).0).unwrap();
    let mut store = PstStore::open(&path).unwrap();
    assert!(!store.writable());
    let inbox = folder(&mut store, "Inbox");
    let first = ItemId(store.list(inbox.id).unwrap()[0].id);
    let item = store.load(inbox.id, first).unwrap();
    assert!(matches!(store.update(inbox.id, first, item), Err(StoreError::ReadOnly)));
    assert!(matches!(store.create_folder(inbox.id, "No"), Err(StoreError::ReadOnly)));
    assert!(matches!(store.remove(inbox.id, first), Err(StoreError::ReadOnly)));
    assert!(!store.unsaved());
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_ids_are_refused() {
    let mut store = open();
    let inbox = folder(&mut store, "Inbox");
    for id in [0, 1, u64::from(u32::MAX), u64::MAX, inbox.id.0] {
        assert!(store.update(inbox.id, ItemId(id), Item::default()).is_err());
        assert!(store.remove(inbox.id, ItemId(id)).is_err());
        if id != inbox.id.0 {
            assert!(store.add(FolderId(id), Item::default()).is_err());
            assert!(store.rename_folder(FolderId(id), "x").is_err());
            assert!(store.delete_folder(FolderId(id)).is_err());
            assert!(store.move_folder(FolderId(id), inbox.id).is_err());
            assert!(store.move_folder(inbox.id, FolderId(id)).is_err());
            assert!(store.create_folder(FolderId(id), "x").is_err());
        }
    }
    assert!(store.move_folder(inbox.id, inbox.id).is_err());
    // A message of junk is written or refused; either way the save does not fall over.
    let mut junk = Item::default();
    junk.props.set(pid::SUBJECT, PropValue::Binary(vec![0xFF; 70_000]));
    junk.props.set(0x7FFF, PropValue::Other { ty: 0xFFFF, raw: vec![1; 9] });
    store.add(inbox.id, junk).unwrap();
    let _ = store.save_to(Cursor::new(Vec::new()), UID);
}

#[test]
fn a_damaged_file_is_changed_and_saved_or_refused_never_a_panic() {
    let bytes = save(&mut mailbox()).0;
    let step = (bytes.len() / 240).max(1);
    let mut opened = 0;
    for at in (0..bytes.len()).step_by(step) {
        let mut bad = bytes.clone();
        for b in bad.iter_mut().skip(at).take(3) {
            *b ^= 0xA5;
        }
        let Ok(mut store) = PstStore::from_bytes(bad) else { continue };
        opened += 1;
        let Ok(folders) = store.folders() else { continue };
        for f in folders.iter().take(6) {
            let Ok(list) = store.list(f.id) else { continue };
            if let Some(s) = list.first() {
                if let Ok(mut item) = store.load(f.id, ItemId(s.id)) {
                    item.set_read(!item.is_read());
                    let _ = store.update(f.id, ItemId(s.id), item);
                }
                if let (Some(s), Some(to)) = (list.get(1), folders.last())
                    && let Ok(item) = store.remove(f.id, ItemId(s.id))
                {
                    let _ = store.add(to.id, item);
                }
            }
            let _ = store.create_folder(f.id, "New");
        }
        if let Some(f) = folders.get(2) {
            let _ = store.delete_folder(f.id);
        }
        if let Ok((out, _)) = store.save_to(Cursor::new(Vec::new()), UID) {
            // What comes out may be missing what the damage took; it is still read without a panic.
            if let Ok(mut back) = PstStore::from_bytes(out.into_inner()) {
                let folders = back.folders().unwrap_or_default();
                for f in folders {
                    let _ = back.list(f.id);
                }
            }
        }
    }
    assert!(opened > 20);
}
