//! Saving into the data file where it is: what is written reads back and passes the check,
//! and a save that fails or is cut short at any point leaves the file as it was.

use crate::check::check;
use crate::edit::onto::{CRASH, FAIL, WASTE};
use crate::tests_edit::{folder, same_mailbox, scratch, whole};
use crate::tests_store::{UID, mail, mailbox, save};
use crate::undo::Undo;
use crate::write::crc;
use crate::{PstStore, Written, journal_beside, original_beside, undo_beside};
use openmailclient_model::{Attachment, Flag, ItemId, Store};
use std::path::{Path, PathBuf};

fn on_disk(name: &str) -> (PathBuf, PathBuf, Vec<u8>) {
    let dir = scratch(name);
    let path = dir.join("box.pst");
    let bytes = save(&mut mailbox()).0;
    std::fs::write(&path, &bytes).unwrap();
    (dir, path, bytes)
}

/// Open a file whose saves go into it from the first one on.
fn open(path: &Path) -> PstStore {
    let mut store = PstStore::open(path).unwrap();
    store.keep_original(false);
    store
}

fn sound(path: &Path) {
    let found = check(&mut std::fs::File::open(path).unwrap()).unwrap();
    assert!(found.sound(), "{:?} {:?}", found.problems, found.inside);
}

/// The first message of the Inbox is marked read, or unread.
fn mark(store: &mut PstStore, read: bool) -> ItemId {
    let inbox = folder(store, "Inbox");
    let first = ItemId(store.list(inbox.id).unwrap()[0].id);
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_read(read);
    store.update(inbox.id, first, item).unwrap();
    first
}

fn is_read(store: &mut PstStore) -> bool {
    let inbox = folder(store, "Inbox");
    let first = ItemId(store.list(inbox.id).unwrap()[0].id);
    store.load(inbox.id, first).unwrap().is_read()
}

/// The file is the mailbox `store` shows, and sound.
fn holds(path: &Path, store: &mut PstStore) {
    sound(path);
    let want = whole(store);
    same_mailbox(&want, &whole(&mut PstStore::open(path).unwrap()));
}

#[test]
fn changes_go_into_the_file_where_it_is() {
    let (dir, path, bytes) = on_disk("onto");
    let mut store = open(&path);
    mark(&mut store, true);
    let want = whole(&mut store);
    assert!(store.save(UID).unwrap().folders > 0);
    assert_eq!((store.saved_in_place(), store.in_place_trouble(), store.unsaved()), (1, None, false));
    // Nothing is left beside the file: no undo file, no journal, no copy.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    same_mailbox(&want, &whole(&mut store));
    holds(&path, &mut store);
    // A flag is a few pages, not the mailbox again: here they fit in the file as it was.
    assert_eq!(std::fs::metadata(&path).unwrap().len(), bytes.len() as u64);
    assert_eq!(store.save(UID).unwrap(), Written::default());

    // Every kind of change, saved several times over.
    let (inbox, sent) = (folder(&mut store, "Inbox"), folder(&mut store, "Sent Items"));
    let listed = store.list(inbox.id).unwrap();
    let (first, second) = (ItemId(listed[0].id), ItemId(listed[1].id));
    let moved = store.remove(inbox.id, second).unwrap();
    store.add(sent.id, moved).unwrap();
    let mut new = mail("Written here", false);
    new.attachments.push(Attachment::file("notes.txt", "text/plain", b"a few notes".to_vec()));
    let new_id = store.add(inbox.id, new).unwrap();
    let made = store.create_folder(inbox.id, "Made here").unwrap();
    store.add(made, mail("In a new folder", true)).unwrap();
    store.save(UID).unwrap();
    holds(&path, &mut store);

    let old = ItemId(store.list(sent.id).unwrap()[0].id);
    store.remove(sent.id, old).unwrap();
    store.rename_folder(made, "Made here, renamed").unwrap();
    store.move_folder(made, sent.id).unwrap();
    let calendar = folder(&mut store, "Calendar");
    store.delete_folder(calendar.id).unwrap();
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_flag(Flag::Flagged);
    item.categories.push("Blue".to_string());
    item.attachments.pop();
    store.update(inbox.id, first, item).unwrap();
    let mut item = store.load(inbox.id, new_id).unwrap();
    item.attachments.clear();
    store.update(inbox.id, new_id, item).unwrap();
    store.save(UID).unwrap();
    holds(&path, &mut store);
    assert_eq!((store.saved_in_place(), store.in_place_trouble()), (3, None));

    // Someone who opens the file now goes on from it, in place too.
    let mut other = open(&path);
    drop(store);
    let inner = other.create_folder(made, "Inside").unwrap();
    let it = other.remove(inbox.id, first).unwrap();
    other.add(inner, it).unwrap();
    other.save(UID).unwrap();
    assert_eq!(other.saved_in_place(), 1);
    holds(&path, &mut other);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    drop(other);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_first_save_keeps_the_file_as_it_was_and_later_ones_go_into_it() {
    let (dir, path, bytes) = on_disk("onto-keep");
    let mut store = PstStore::open(&path).unwrap();
    mark(&mut store, true);
    store.save(UID).unwrap();
    assert_eq!(store.saved_in_place(), 0);
    assert!(std::fs::read(original_beside(&path)).unwrap() == bytes);
    mark(&mut store, false);
    store.save(UID).unwrap();
    assert_eq!(store.saved_in_place(), 1);
    holds(&path, &mut store);
    assert!(std::fs::read(original_beside(&path)).unwrap() == bytes);
    // Said so, every save writes a whole new file.
    store.save_in_place(false);
    mark(&mut store, true);
    store.save(UID).unwrap();
    assert_eq!(store.saved_in_place(), 1);
    holds(&path, &mut store);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_large_attachment_is_not_written_again_for_a_flag() {
    let (dir, path, _) = on_disk("onto-large");
    let mut store = open(&path);
    let inbox = folder(&mut store, "Inbox");
    let mut big = mail("With a large file", false);
    let data: Vec<u8> = (0..1_500_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
    big.attachments.push(Attachment::file("large.bin", "application/octet-stream", data.clone()));
    let id = store.add(inbox.id, big).unwrap();
    store.save(UID).unwrap();
    let was = std::fs::read(&path).unwrap();
    let before = was.len() as u64;
    let mut item = store.load(inbox.id, id).unwrap();
    item.set_read(true);
    item.set_flag(Flag::Flagged);
    store.update(inbox.id, id, item).unwrap();
    store.save(UID).unwrap();
    assert_eq!(store.saved_in_place(), 2);
    let after = std::fs::metadata(&path).unwrap().len();
    assert!(after - before < 600_000, "{before} then {after}");
    // What held mail before the save is where it was: only the start of the file, with its
    // maps, and its last section were written over.
    let now = std::fs::read(&path).unwrap();
    let kept = was.len() - 253_952;
    assert!(kept > 1_000_000 && now[0x4C00..kept] == was[0x4C00..kept]);
    sound(&path);
    let mut fresh = PstStore::open(&path).unwrap();
    let back = fresh.load(inbox.id, id).unwrap();
    assert!(back.is_read() && back.attachments[0].data == data);
    drop((store, fresh));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_ansi_file_is_saved_into_as_an_ansi_file() {
    let dir = scratch("onto-ansi");
    let path = dir.join("old.pst");
    let (out, _) = crate::write_store_ansi(std::io::Cursor::new(Vec::new()), &mut mailbox(), UID, None).unwrap();
    std::fs::write(&path, out.into_inner()).unwrap();
    let mut store = open(&path);
    mark(&mut store, true);
    let inbox = folder(&mut store, "Inbox");
    store.create_folder(inbox.id, "Made here").unwrap();
    store.save(UID).unwrap();
    assert_eq!((store.saved_in_place(), store.in_place_trouble()), (1, None));
    let now = std::fs::read(&path).unwrap();
    assert_eq!((now[10], now[11]), (14, 0));
    holds(&path, &mut store);
    mark(&mut store, false);
    store.save(UID).unwrap();
    assert_eq!(store.saved_in_place(), 2);
    holds(&path, &mut store);
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_that_fails_puts_the_file_back_and_is_done_whole() {
    let (dir, path, bytes) = on_disk("onto-fail");
    for stage in [1u8, 2, 3, 5, 6] {
        std::fs::write(&path, &bytes).unwrap();
        let mut store = open(&path);
        mark(&mut store, true);
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        FAIL.set(Some(stage));
        let done = store.in_place(&path, UID);
        FAIL.set(None);
        // The file is what it was, byte for byte, and says it was written when it was.
        assert!(done.unwrap().is_none(), "stage {stage}");
        assert!(std::fs::read(&path).unwrap() == bytes, "stage {stage}");
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), written, "stage {stage}");
        assert!(!undo_beside(&path).exists() && store.in_place_trouble().is_some() && !store.torn() && store.unsaved());
        assert_eq!(store.saved_in_place(), 0);
        // A save that meets the same trouble writes a whole new file instead.
        FAIL.set(Some(stage));
        let done = store.save(UID);
        FAIL.set(None);
        assert!(done.unwrap().folders > 0 && !store.unsaved(), "stage {stage}");
        assert!(store.saved_in_place() == 0 && store.in_place_trouble().is_some());
        holds(&path, &mut store);
        assert!(is_read(&mut store));
        // The next one goes into the file.
        mark(&mut store, false);
        store.save(UID).unwrap();
        assert_eq!((store.saved_in_place(), store.in_place_trouble()), (1, None));
        holds(&path, &mut store);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_cut_short_leaves_the_file_as_it_was_to_whoever_opens_it() {
    let (dir, path, bytes) = on_disk("onto-crash");
    let mut plain = PstStore::open(&path).unwrap();
    let before = whole(&mut plain);
    drop(plain);
    for stage in 1..=6u8 {
        std::fs::write(&path, &bytes).unwrap();
        let mut store = open(&path);
        mark(&mut store, true);
        let inbox = folder(&mut store, "Inbox");
        store.create_folder(inbox.id, "Made here").unwrap();
        let want = whole(&mut store);
        CRASH.set(Some(stage));
        let done = store.save(UID);
        CRASH.set(None);
        assert!(done.is_err(), "stage {stage}");
        // The run ends here. The undo file is beside the data file, and the data file may
        // be anything a save can leave.
        drop(store);
        assert!(undo_beside(&path).exists() && journal_beside(&path).exists());
        let torn = std::fs::read(&path).unwrap();
        assert_eq!(torn != bytes, stage > 1, "stage {stage}");

        // Opened again, it reads as it was before that save, and opening writes nothing.
        let mut back = PstStore::open(&path).unwrap();
        assert!(back.torn() && back.unsaved() && back.writable());
        same_mailbox(&before, &whole(&mut back));
        assert!(std::fs::read(&path).unwrap() == torn);
        drop(back);
        // A save puts the file back, byte for byte.
        let mut mend = PstStore::open(&path).unwrap();
        assert_eq!(mend.save(UID).unwrap(), Written::default());
        assert!(!mend.torn() && !mend.unsaved() && !undo_beside(&path).exists());
        assert!(std::fs::read(&path).unwrap() == bytes, "stage {stage}");
        drop(mend);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // The same again, with the changes brought back from the journal.
        std::fs::write(&path, &bytes).unwrap();
        let mut store = open(&path);
        mark(&mut store, true);
        let inbox = folder(&mut store, "Inbox");
        store.create_folder(inbox.id, "Made here").unwrap();
        CRASH.set(Some(stage));
        assert!(store.save(UID).is_err());
        CRASH.set(None);
        drop(store);
        let mut back = open(&path);
        assert!(back.torn());
        assert_eq!(back.recover().unwrap().applied, 2, "stage {stage}");
        same_mailbox(&want, &whole(&mut back));
        back.save(UID).unwrap();
        assert!(!back.torn() && !back.unsaved() && back.saved_in_place() == 1, "{:?}", back.in_place_trouble());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        holds(&path, &mut back);
        same_mailbox(&want, &whole(&mut back));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_undo_file_that_is_not_this_files_is_left_out_of_it() {
    let (dir, path, bytes) = on_disk("onto-undo");
    // A sound undo file, taken from a save that was cut short.
    let mut store = open(&path);
    mark(&mut store, true);
    CRASH.set(Some(3));
    assert!(store.save(UID).is_err());
    CRASH.set(None);
    drop(store);
    let undo = std::fs::read(undo_beside(&path)).unwrap();
    let torn = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_file(journal_beside(&path));
    assert!(Undo::decode(&undo).is_some() && undo.len() < 1 << 20);

    // Cut anywhere, or with a byte changed, it is no undo file.
    for cut in (0..undo.len()).step_by(977) {
        assert!(Undo::decode(&undo[..cut]).is_none());
    }
    for at in (0..undo.len()).step_by(1009) {
        let mut bad = undo.clone();
        bad[at] ^= 0x40;
        assert!(Undo::decode(&bad).is_none());
    }
    // Junk with a sum that fits: lengths past the end, parts past the file, no parts.
    let summed = |mut body: Vec<u8>| {
        let sum = crc(&body);
        body.extend(sum.to_le_bytes());
        body
    };
    let head = |len: u64, at: u64| {
        let mut b = b"OMCUNDO1".to_vec();
        b.extend(len.to_le_bytes());
        for _ in 0..2 {
            b.extend(at.to_le_bytes());
            b.extend(0u32.to_le_bytes());
        }
        b
    };
    let mut junk: Vec<Vec<u8>> = vec![Vec::new(), vec![0; 3], summed(Vec::new()), summed(b"OMCUNDO1".to_vec()), summed(head(4096, u64::MAX))];
    for (next, count, to, n) in
        [(0u32, 0u32, 0u64, 0u32), (u32::MAX, 1, 0, 0), (0, u32::MAX, 0, 0), (0, 1, u64::MAX, 8), (0, 1, 0, u32::MAX), (0, 1, 0, 8)]
    {
        let mut b = head(4096, 512);
        b.extend(next.to_le_bytes());
        b.extend(count.to_le_bytes());
        b.extend(to.to_le_bytes());
        b.extend(n.to_le_bytes());
        b.extend([7u8; 8]);
        junk.push(summed(b));
    }
    for bad in &junk {
        assert!(Undo::decode(bad).is_none());
        std::fs::write(undo_beside(&path), bad).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let mut store = PstStore::open(&path).unwrap();
        assert!(!store.torn() && !store.unsaved() && !is_read(&mut store));
    }

    // The undo file of a save in another file: this one is read as it is, and saved.
    std::fs::write(undo_beside(&path), &undo).unwrap();
    let mut other = mailbox();
    let inbox = other.folders().unwrap().into_iter().find(|f| f.name == "Inbox").unwrap();
    other.add(inbox.id, mail("Only in the other file", true)).unwrap();
    let theirs = save(&mut other).0;
    std::fs::write(&path, &theirs).unwrap();
    let mut store = open(&path);
    assert!(!store.torn() && !store.unsaved());
    assert_eq!(folder(&mut store, "Inbox").total, 3);
    assert!(std::fs::read(&path).unwrap() == theirs);
    mark(&mut store, true);
    store.save(UID).unwrap();
    assert!(store.saved_in_place() == 1 && !undo_beside(&path).exists());
    holds(&path, &mut store);
    drop(store);

    // The file it was taken of, changed since by someone else: left alone as well.
    let mut changed = torn.clone();
    changed.truncate(bytes.len() - 600);
    std::fs::write(&path, &changed).unwrap();
    std::fs::write(undo_beside(&path), &undo).unwrap();
    assert!(PstStore::open(&path).map(|s| !s.torn()).unwrap_or(true));
    assert!(std::fs::read(&path).unwrap() == changed);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_with_much_unused_space_is_written_whole() {
    let (dir, path, _) = on_disk("onto-waste");
    let mut store = open(&path);
    WASTE.set(Some(0));
    let (mut whole_at, mut longest) = (None, 0);
    for round in 0..120u32 {
        mark(&mut store, round % 2 == 0);
        let before = store.saved_in_place();
        store.save(UID).unwrap();
        assert!(store.in_place_trouble().is_none());
        let len = std::fs::metadata(&path).unwrap().len();
        if store.saved_in_place() == before {
            // Written whole: the space earlier saves left is given back.
            assert!(len <= longest, "{len} after {longest}");
            whole_at = Some(round);
            break;
        }
        longest = longest.max(len);
    }
    WASTE.set(None);
    assert!(whole_at.is_some_and(|r| r > 0), "{whole_at:?}");
    holds(&path, &mut store);
    // And the saves after it go into the file again.
    let read = is_read(&mut store);
    mark(&mut store, !read);
    let before = store.saved_in_place();
    store.save(UID).unwrap();
    assert_eq!(store.saved_in_place(), before + 1);
    holds(&path, &mut store);
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_done_elsewhere_goes_into_the_file() {
    use openmailclient_model::Progress;
    use std::sync::Arc;
    let (dir, path, _) = on_disk("onto-elsewhere");
    let mut store = open(&path);
    mark(&mut store, true);
    let want = whole(&mut store);
    let work = store.save_begin(UID).unwrap().unwrap();
    let progress = Arc::new(Progress::default());
    let theirs = progress.clone();
    let done = std::thread::spawn(move || work(theirs)).join().unwrap();
    assert!(store.saving() && is_read(&mut store));
    store.save_end(done).unwrap();
    assert!(!store.saving() && !store.unsaved() && store.saved_in_place() == 1 && progress.fraction() > 0.99);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    same_mailbox(&want, &whole(&mut store));
    holds(&path, &mut store);

    // Stopped: the file is put back as it was and the change stays to be saved.
    let saved = std::fs::read(&path).unwrap();
    mark(&mut store, false);
    let work = store.save_begin(UID).unwrap().unwrap();
    let progress = Arc::new(Progress::default());
    progress.stop();
    let done = work(progress);
    assert!(done.is_err() && store.save_end(done).is_err());
    assert!(!store.saving() && store.unsaved() && std::fs::read(&path).unwrap() == saved);
    assert!(!undo_beside(&path).exists());
    // And is saved by the next one, here or elsewhere.
    let work = store.save_begin(UID).unwrap().unwrap();
    store.save_end(work(Arc::new(Progress::default()))).unwrap();
    assert!(store.saved_in_place() == 2 && !store.unsaved() && !is_read(&mut store));
    holds(&path, &mut store);
    mark(&mut store, true);
    store.save(UID).unwrap();
    assert_eq!(store.saved_in_place(), 3);
    holds(&path, &mut store);

    // Another program writes the file after the work is done: the store does not take the
    // file for its own, and says so.
    mark(&mut store, false);
    let work = store.save_begin(UID).unwrap().unwrap();
    let done = work(Arc::new(Progress::default()));
    assert!(done.is_ok());
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();
    assert!(store.save_end(done).is_err());
    assert!(!store.saving() && store.unsaved() && store.saved_in_place() == 3);
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn a_file_another_program_writes_is_not_written_into() {
    let (dir, path, bytes) = on_disk("onto-held");
    let mut store = open(&path);
    mark(&mut store, true);
    let theirs = std::fs::File::options().write(true).open(&path).unwrap();
    assert!(store.in_place(&path, UID).unwrap().is_none());
    assert!(store.save(UID).is_err());
    assert!(store.unsaved() && std::fs::read(&path).unwrap() == bytes && !undo_beside(&path).exists());
    drop(theirs);
    store.save(UID).unwrap();
    assert_eq!(store.saved_in_place(), 1);
    holds(&path, &mut store);
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}
