//! IMAP kept in step both ways, against the in-process fake server: read and flagged each way,
//! moves, deletions for good, messages put into the account from elsewhere, the first pass that
//! finds the messages taken down before, and a folder numbered anew.

use crate::Session;
use crate::cmd::imap_sync::Place;
use openmailclient_model::{FolderKind, ItemId};
use openmailclient_net::fake::{Fake, Mailbox};
use serde_json::{Value, json};
use std::sync::Arc;

fn message(subject: &str) -> Vec<u8> {
    format!("From: June <june@example.test>\r\nTo: pat@example.test\r\nSubject: {subject}\r\nDate: Tue, 13 Oct 2026 09:00:00 +0000\r\nMessage-ID: <{subject}@example.test>\r\n\r\nHello.\r\n").into_bytes()
}

/// A session with an IMAP account whose mail is in a data file of its own, store 1, and the
/// folder it was made in, to be cleared away at the end.
fn account(fake: &Fake, name: &str) -> (Session, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("omc-sync-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::new();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.connector = Some(Arc::new(fake.clone()));
    s.account_dir = Some(dir.join("Accounts"));
    s.journal_dir = Some(dir.join("journals"));
    s.run(
        "account.add",
        &json!({ "email": "pat@example.test", "kind": "imap",
                 "incoming": { "host": "mail.example.test", "security": "starttls" },
                 "outgoing": { "host": "mail.example.test", "security": "starttls" }, "password": "not a real password" }),
    )
    .unwrap();
    assert_eq!(s.stores.len(), 2);
    (s, dir)
}

fn done(mut s: Session, dir: std::path::PathBuf) {
    settle(&mut s);
    let _ = s.run("file.closeStore", &json!({ "store": 1, "discard": true }));
    let _ = std::fs::remove_dir_all(&dir);
}

fn settle(s: &mut Session) {
    let mut waited = 0;
    while !(s.jobs.is_empty() && s.saves.is_empty()) {
        s.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
        assert!(waited < 1000, "never settled: {}", s.status);
    }
}

fn receive(s: &mut Session) -> Value {
    let got = s.run("account.receive", &Value::Null).unwrap();
    settle(s);
    got
}

fn folder(s: &Session, name: &str) -> openmailclient_model::FolderId {
    s.folders[1].iter().find(|f| f.name == name).map(|f| f.id).unwrap_or_else(|| panic!("no {name}: {:?}", s.folders[1]))
}

fn count(s: &Session, name: &str) -> u32 {
    s.folders[1].iter().find(|f| f.name == name).map_or(0, |f| f.total)
}

/// Select the message of that subject in a folder of the account's data file.
fn pick(s: &mut Session, name: &str, subject: &str) -> u64 {
    let f = folder(s, name);
    s.select_folder(1, f).unwrap();
    let id = s.rows.iter().find(|r| r.subject == subject).map(|r| r.id).unwrap_or_else(|| panic!("no {subject} in {name}"));
    s.select_item(id).unwrap();
    id
}

fn flags(fake: &Fake, mailbox: &str, subject: &str) -> Vec<String> {
    let mut state = fake.state();
    let m = state.mailbox(mailbox).unwrap();
    let wanted = format!("Subject: {subject}\r\n");
    m.messages
        .iter()
        .find(|m| String::from_utf8_lossy(&m.data).contains(&wanted))
        .map(|m| m.flags.clone())
        .unwrap_or_else(|| vec!["(not there)".into()])
}

fn on_server(fake: &Fake, mailbox: &str) -> usize {
    fake.state().mailbox(mailbox).map_or(0, |m| m.messages.len())
}

#[test]
fn read_and_flagged_go_both_ways() {
    let fake = Fake::default();
    fake.state().mailbox("INBOX").unwrap().add(&message("one"), &[]);
    fake.state().mailbox("INBOX").unwrap().add(&message("two"), &[]);
    let (mut s, dir) = account(&fake, "flags");
    assert_eq!(receive(&mut s)["received"], 2, "{}", s.status);
    assert!(s.accounts[0].received.matched, "the first pass looked through every folder");
    let id = pick(&mut s, "Inbox", "one");
    let inbox = folder(&s, "Inbox");
    let item = s.stores[1].load(inbox, ItemId(id)).unwrap();
    assert!(matches!(Place::of(&item), Some(Place::On { ref raw, uid: 1, seen: false, .. }) if raw == "INBOX"), "{:?}", Place::of(&item));

    // Read here, and flagged there.
    s.run("mail.markRead", &Value::Null).unwrap();
    {
        let mut state = fake.state();
        let inbox = state.mailbox("INBOX").unwrap();
        inbox.messages[1].flags.push("\\Flagged".into());
    }
    let got = s.run("account.receive", &Value::Null).unwrap();
    assert!(s.status.contains("2 changes kept in step with the server"), "{}", s.status);
    settle(&mut s);
    assert_eq!(flags(&fake, "INBOX", "one"), ["\\Seen"], "{}", s.status);
    assert_eq!(got["accounts"][0]["inStep"]["pushed"], 1, "{got}");
    assert_eq!(got["accounts"][0]["inStep"]["changed"], 1, "{got}");
    pick(&mut s, "Inbox", "two");
    assert_eq!(s.open.as_ref().unwrap().flag(), openmailclient_model::Flag::Flagged);
    // Opened, as in Outlook, it is read: that goes too.
    receive(&mut s);
    assert_eq!(flags(&fake, "INBOX", "two"), ["\\Flagged", "\\Seen"]);

    // Nothing changed: nothing goes either way, and no message is taken down twice.
    let heard = fake.state().heard.len();
    let got = receive(&mut s);
    assert_eq!((got["received"].as_u64(), got["accounts"][0]["inStep"]["pushed"].as_u64()), (Some(0), Some(0)), "{got}");
    assert!(!fake.state().heard[heard..].iter().any(|c| c.contains("STORE")), "{:?}", &fake.state().heard[heard..]);
    assert_eq!(count(&s, "Inbox"), 2);

    // Unread there and unflagged here: each side has the other's change.
    s.run("mail.flag", &Value::Null).unwrap();
    assert_eq!(s.open.as_ref().unwrap().flag(), openmailclient_model::Flag::None);
    fake.state().mailbox("INBOX").unwrap().messages[0].flags.clear();
    receive(&mut s);
    assert_eq!(flags(&fake, "INBOX", "two"), ["\\Seen"]);
    s.select_folder(1, folder(&s, "Inbox")).unwrap();
    assert!(!s.rows.iter().find(|r| r.subject == "one").unwrap().read, "unread there, so unread here");
    done(s, dir);
}

#[test]
fn a_move_here_is_a_move_there() {
    for with_move in [true, false] {
        let fake = Fake::default();
        if with_move {
            fake.state().imap_caps.push("MOVE".into());
        }
        {
            let mut state = fake.state();
            state.mailbox("INBOX").unwrap().add(&message("keep"), &[]);
            state.mailbox("INBOX").unwrap().add(&message("file"), &[]);
            state.mailboxes.push(Mailbox::new("Projects", &[]));
        }
        let (mut s, dir) = account(&fake, if with_move { "move" } else { "copy" });
        receive(&mut s);
        pick(&mut s, "Inbox", "file");
        s.run("mail.move", &json!({ "folderId": folder(&s, "Projects").0 })).unwrap();
        let got = receive(&mut s);
        assert_eq!((on_server(&fake, "INBOX"), on_server(&fake, "Projects")), (1, 1), "{} {got}", s.status);
        assert_eq!(got["received"], 0, "the moved message is not taken down again: {got}");
        assert_eq!((count(&s, "Inbox"), count(&s, "Projects")), (1, 1));
        let heard = fake.state().heard.join("\n");
        assert_eq!(heard.contains("UID MOVE"), with_move, "{heard}");
        assert_eq!(heard.contains("UID COPY"), !with_move, "{heard}");
        // Its new place is known: a change to it goes to the folder it is in now.
        pick(&mut s, "Projects", "file");
        s.run("mail.markRead", &Value::Null).unwrap();
        receive(&mut s);
        assert_eq!(flags(&fake, "Projects", "file"), ["\\Seen"]);
        assert_eq!(count(&s, "Projects"), 1);

        // To a folder made here: it is made there too.
        let projects = folder(&s, "Projects");
        let st = &mut s.stores[1];
        st.create_folder(projects, "Plans").unwrap();
        s.refresh_folders(1);
        pick(&mut s, "Projects", "file");
        s.run("mail.move", &json!({ "folderId": folder(&s, "Plans").0 })).unwrap();
        let got = receive(&mut s);
        assert_eq!(got["accounts"][0]["inStep"]["created"], json!(["Projects/Plans"]), "{got}");
        assert_eq!((on_server(&fake, "Projects"), on_server(&fake, "Projects/Plans")), (0, 1));
        assert_eq!(count(&s, "Plans"), 1);
        done(s, dir);
    }
}

#[test]
fn a_message_deleted_for_good_goes_from_the_server_and_one_the_server_lost_goes_from_here() {
    let fake = Fake::default();
    {
        let mut state = fake.state();
        state.mailbox("INBOX").unwrap().add(&message("unwanted"), &[]);
        state.mailbox("INBOX").unwrap().add(&message("lost there"), &[]);
        state.mailbox("INBOX").unwrap().add(&message("stays"), &[]);
    }
    let (mut s, dir) = account(&fake, "delete");
    receive(&mut s);
    // Deleted: it moves to Deleted Items here, and to the server's Trash.
    pick(&mut s, "Inbox", "unwanted");
    s.run("mail.delete", &Value::Null).unwrap();
    receive(&mut s);
    assert_eq!((on_server(&fake, "INBOX"), on_server(&fake, "Trash")), (2, 1), "{}", s.status);
    // Deleted from Deleted Items: gone for good, there too.
    pick(&mut s, "Deleted Items", "unwanted");
    s.run("mail.delete", &Value::Null).unwrap();
    assert_eq!(s.accounts[0].received.gone.len(), 1);
    // Taken back before the next receive, it stays on the server.
    s.run("edit.undo", &Value::Null).unwrap();
    assert!(s.accounts[0].received.gone.is_empty());
    pick(&mut s, "Deleted Items", "unwanted");
    s.run("mail.delete", &Value::Null).unwrap();
    receive(&mut s);
    assert_eq!(on_server(&fake, "Trash"), 0, "{}", s.status);
    assert!(s.accounts[0].received.gone.is_empty(), "done, so forgotten");

    // The server lost one: it goes from here, and only it.
    fake.state().mailbox("INBOX").unwrap().messages.retain(|m| !String::from_utf8_lossy(&m.data).contains("lost there"));
    let got = receive(&mut s);
    assert_eq!(got["accounts"][0]["inStep"]["removed"], 1, "{got}");
    assert_eq!(count(&s, "Inbox"), 1);
    pick(&mut s, "Inbox", "stays");
    done(s, dir);
}

#[test]
fn a_message_put_into_the_account_from_elsewhere_goes_on_the_server_once() {
    let fake = Fake::default();
    fake.state().mailbox("INBOX").unwrap().add(&message("first"), &[]);
    let (mut s, dir) = account(&fake, "upload");
    receive(&mut s);
    // From the sample's Inbox into the account's.
    let from = s.find_folder(0, FolderKind::Inbox).unwrap();
    let row = s.stores[0].list(from).unwrap()[0].clone();
    let item = s.stores[0].load(from, ItemId(row.id)).unwrap();
    let inbox = folder(&s, "Inbox");
    s.stores[1].add(inbox, item).unwrap();
    s.refresh_folders(1);
    let got = receive(&mut s);
    assert_eq!(on_server(&fake, "INBOX"), 2, "{} {got}", s.status);
    assert_eq!(got["received"], 0, "{got}");
    assert_eq!(count(&s, "Inbox"), 2);
    let again = receive(&mut s);
    assert_eq!((on_server(&fake, "INBOX"), count(&s, "Inbox"), again["received"].as_u64()), (2, 2, Some(0)));
    done(s, dir);
}

#[test]
fn mail_taken_down_before_is_found_on_the_server_and_none_of_it_is_put_there_again() {
    let fake = Fake::default();
    fake.state().mailbox("INBOX").unwrap().add(&message("old"), &[]);
    fake.state().mailbox("Sent").unwrap().add(&message("went"), &["\\Seen"]);
    let (mut s, dir) = account(&fake, "match");
    receive(&mut s);
    // As a receive before this program knew places left it.
    for f in [folder(&s, "Inbox"), folder(&s, "Sent Items")] {
        for r in s.stores[1].list(f).unwrap() {
            let mut item = s.stores[1].load(f, ItemId(r.id)).unwrap();
            Place::clear(&mut item);
            s.stores[1].update(f, ItemId(r.id), item).unwrap();
        }
    }
    s.accounts[0].received.matched = false;
    s.places = Default::default();
    // One kept only here: it stays so.
    let inbox = folder(&s, "Inbox");
    let mut only = openmailclient_model::Item::new_mail();
    only.set_subject("only here");
    s.stores[1].add(inbox, only).unwrap();
    s.refresh_folders(1);
    let got = receive(&mut s);
    assert!(s.accounts[0].received.matched, "{} {got}", s.status);
    assert_eq!((on_server(&fake, "INBOX"), on_server(&fake, "Sent")), (1, 1));
    let id = pick(&mut s, "Inbox", "old");
    assert!(matches!(Place::of(&s.stores[1].load(inbox, ItemId(id)).unwrap()), Some(Place::On { uid: 1, .. })));
    let id = pick(&mut s, "Inbox", "only here");
    assert_eq!(Place::of(&s.stores[1].load(inbox, ItemId(id)).unwrap()), Some(Place::Here));
    // Found, a change goes to it.
    pick(&mut s, "Inbox", "old");
    s.run("mail.markRead", &Value::Null).unwrap();
    receive(&mut s);
    assert_eq!(flags(&fake, "INBOX", "old"), ["\\Seen"]);
    assert_eq!(on_server(&fake, "INBOX"), 1);
    done(s, dir);
}

#[test]
fn a_folder_numbered_anew_is_found_again_by_message_id() {
    let fake = Fake::default();
    fake.state().mailbox("INBOX").unwrap().add(&message("one"), &[]);
    let (mut s, dir) = account(&fake, "validity");
    receive(&mut s);
    pick(&mut s, "Inbox", "one");
    s.run("mail.markRead", &Value::Null).unwrap();
    fake.state().mailbox("INBOX").unwrap().uid_validity = 4242;
    let got = receive(&mut s);
    assert_eq!(count(&s, "Inbox"), 1, "not kept twice: {} {got}", s.status);
    let id = pick(&mut s, "Inbox", "one");
    let inbox = folder(&s, "Inbox");
    let place = Place::of(&s.stores[1].load(inbox, ItemId(id)).unwrap());
    assert!(matches!(place, Some(Place::On { uid_validity: 4242, .. })), "{place:?}");
    // The change made before goes with the next receive.
    receive(&mut s);
    assert_eq!(flags(&fake, "INBOX", "one"), ["\\Seen"]);
    done(s, dir);
}

#[test]
fn places_that_make_no_sense_are_no_places() {
    let mut item = openmailclient_model::Item::new_mail();
    for junk in ["", "on", "on 1", "on x 2 S- INBOX", "on 1 0 S- INBOX", "on 1 2 S-", "nowhere", "on 1 2 S- IN\u{7}BOX", &"x".repeat(5000)] {
        item.named.retain(|_| false);
        item.named.push((
            openmailclient_model::NamedKey {
                guid: crate::cmd::imap_sync::PS_OPENMAILCLIENT,
                name: openmailclient_model::NameId::Name("ImapPlace".into()),
            },
            openmailclient_model::PropValue::String(junk.to_string()),
        ));
        assert_eq!(Place::of(&item), None, "{junk:?}");
    }
    for place in [
        Place::On { raw: "Projects/Two words".into(), uid_validity: 7, uid: 9, seen: true, flagged: false },
        Place::Pending { raw: String::new() },
        Place::Here,
    ] {
        place.put(&mut item);
        assert_eq!(Place::of(&item), Some(place));
    }
}

#[test]
fn a_flag_set_in_a_folder_written_empty_shows_in_the_list_after_a_save() {
    // The account's data file is made with empty folders, whose tables have only the columns
    // every table has; a save into it in place gives them the flag's column.
    let fake = Fake::default();
    let (mut s, dir) = account(&fake, "flag-column");
    let inbox = folder(&s, "Inbox");
    let mut it = openmailclient_model::Item::new_mail();
    it.set_subject("x");
    let id = s.stores[1].add(inbox, it).unwrap();
    s.run("file.save", &json!({ "store": 1 })).unwrap();
    let flags = |s: &mut Session| s.stores[1].list(inbox).unwrap().iter().map(|r| r.flag).collect::<Vec<_>>();
    let mut it = s.stores[1].load(inbox, id).unwrap();
    it.set_flag(openmailclient_model::Flag::Flagged);
    s.stores[1].update(inbox, id, it).unwrap();
    s.run("file.save", &json!({ "store": 1 })).unwrap();
    assert_eq!(flags(&mut s), [openmailclient_model::Flag::Flagged]);
    let mut it = s.stores[1].load(inbox, id).unwrap();
    it.set_read(true);
    s.stores[1].update(inbox, id, it).unwrap();
    s.run("file.save", &json!({ "store": 1 })).unwrap();
    assert_eq!(flags(&mut s), [openmailclient_model::Flag::Flagged]);
    done(s, dir);
}

/// The commands a receive sent the server, from `from` on.
fn heard_since(fake: &Fake, from: usize) -> Vec<String> {
    fake.state().heard[from..].to_vec()
}

fn whole_lists(heard: &[String]) -> usize {
    heard.iter().filter(|c| c.contains("UID FETCH 1:* (UID FLAGS RFC822.SIZE)")).count()
}

#[test]
fn with_mod_sequences_only_what_changed_is_asked_for() {
    for caps in [&["CONDSTORE"][..], &["CONDSTORE", "QRESYNC", "ENABLE"][..]] {
        let qresync = caps.contains(&"QRESYNC");
        let fake = Fake::default();
        {
            let mut state = fake.state();
            state.imap_caps.extend(caps.iter().map(|c| c.to_string()));
            for n in ["one", "two", "three", "four"] {
                state.mailbox("INBOX").unwrap().add(&message(n), &[]);
            }
            state.mailbox("Sent").unwrap().add(&message("went"), &["\\Seen"]);
        }
        let (mut s, dir) = account(&fake, if qresync { "qresync" } else { "condstore" });
        receive(&mut s);
        assert_eq!(count(&s, "Inbox"), 4);
        let steps = &s.accounts[0].received.steps;
        assert!(steps.iter().any(|t| t.raw == "INBOX" && t.modseq > 0 && t.exists == 4), "{steps:?}");

        // Nothing changed: each folder costs its SELECT, and no list is read.
        let from = fake.state().heard.len();
        let got = receive(&mut s);
        let heard = heard_since(&fake, from);
        assert_eq!(whole_lists(&heard), 0, "{heard:?}");
        assert!(!heard.iter().any(|c| c.contains("CHANGEDSINCE")), "{heard:?}");
        assert!(got["accounts"][0]["inStep"]["unchanged"].as_u64().unwrap() >= 2, "{got}");

        // Flagged and read there, and one taken away there: only that is asked for, and comes.
        {
            let mut state = fake.state();
            let inbox = state.mailbox("INBOX").unwrap();
            inbox.set_flags(2, &["\\Flagged"]);
            inbox.set_flags(3, &["\\Seen"]);
            inbox.remove(4);
        }
        let from = fake.state().heard.len();
        let got = receive(&mut s);
        let heard = heard_since(&fake, from);
        // Without QRESYNC nothing says what went; the count does not add up, so the whole list
        // of the Inbox is read, and nothing is asked for first that it would tell again.
        assert_eq!(whole_lists(&heard), usize::from(!qresync), "{heard:?}");
        assert!(!heard.iter().any(|c| c.contains("CHANGEDSINCE")), "{heard:?}");
        assert_eq!(heard.iter().any(|c| c.contains("(QRESYNC (")), qresync, "{heard:?}");
        assert_eq!(got["accounts"][0]["inStep"]["changed"], 2, "{got}");
        if qresync {
            // What the server said went was made sure of before it went here.
            assert!(heard.iter().any(|c| c.contains("UID FETCH 4 (UID)")), "{heard:?}");
            assert_eq!(got["accounts"][0]["inStep"]["removed"], 1, "{got}");
            assert_eq!(count(&s, "Inbox"), 3);
        } else {
            assert_eq!(count(&s, "Inbox"), 3, "{got}");
        }
        pick(&mut s, "Inbox", "two");
        assert_eq!(s.open.as_ref().unwrap().flag(), openmailclient_model::Flag::Flagged);
        s.select_folder(1, folder(&s, "Inbox")).unwrap();
        assert!(s.rows.iter().find(|r| r.subject == "three").unwrap().read);

        // New mail with a flag changed: the new one is taken down, the change comes, and no
        // list of the whole folder is read, as the count adds up.
        {
            let mut state = fake.state();
            let inbox = state.mailbox("INBOX").unwrap();
            inbox.add(&message("five"), &[]);
            inbox.set_flags(1, &["\\Seen"]);
        }
        let from = fake.state().heard.len();
        let got = receive(&mut s);
        let heard = heard_since(&fake, from);
        assert_eq!(whole_lists(&heard), 0, "{heard:?}");
        assert_eq!(heard.iter().any(|c| c.contains("CHANGEDSINCE")), !qresync, "{heard:?}");
        assert_eq!((got["received"].as_u64(), got["accounts"][0]["inStep"]["changed"].as_u64()), (Some(1), Some(1)), "{got}");
        assert_eq!(count(&s, "Inbox"), 4);

        // A change made here goes there, and is not taken back as the server's next time.
        pick(&mut s, "Inbox", "five");
        s.run("mail.markRead", &Value::Null).unwrap();
        receive(&mut s);
        assert_eq!(flags(&fake, "INBOX", "five"), ["\\Seen"]);
        let got = receive(&mut s);
        assert_eq!(got["accounts"][0]["inStep"]["changed"], 0, "{got}");
        done(s, dir);
    }
}

#[test]
fn a_server_that_says_a_message_went_that_is_still_there_is_not_believed() {
    let fake = Fake::default();
    {
        let mut state = fake.state();
        state.imap_caps.extend(["CONDSTORE", "QRESYNC", "ENABLE"].map(String::from));
        for n in ["kept", "also kept"] {
            state.mailbox("INBOX").unwrap().add(&message(n), &[]);
        }
    }
    let (mut s, dir) = account(&fake, "vanished-wrongly");
    receive(&mut s);
    // The server names both as gone, and every id there might be, though it has them still.
    {
        let mut state = fake.state();
        let inbox = state.mailbox("INBOX").unwrap();
        inbox.modseq += 1;
        let at = inbox.modseq;
        inbox.vanished.extend([(1, at), (2, at), (3, at)]);
    }
    let got = receive(&mut s);
    assert_eq!(got["accounts"][0]["inStep"]["removed"], 0, "{got}");
    assert_eq!(count(&s, "Inbox"), 2);
    done(s, dir);
}

#[test]
fn where_each_folder_was_left_is_kept_in_the_settings_and_nonsense_is_dropped() {
    use crate::accounts::{Received, Step};
    let mut r = Received::default();
    r.stepped(Step { raw: "INBOX".into(), uid_validity: 7, modseq: 9_007_199_254_740_993, exists: 3, uid_next: 4 });
    r.stepped(Step { raw: "INBOX".into(), uid_validity: 7, modseq: 9_007_199_254_740_995, exists: 3, uid_next: 4 });
    let json = r.to_json();
    assert_eq!(json["steps"][0]["modseq"], "9007199254740995", "a mod-sequence past what a JSON number holds is kept whole");
    let back = Received::from_json(Some(&json));
    assert_eq!(back.steps, r.steps);
    for bad in [
        json!({"raw": "INBOX", "uidValidity": 7, "modseq": "0"}),
        json!({"raw": "INBOX", "uidValidity": 0, "modseq": "5"}),
        json!({"raw": "", "uidValidity": 7, "modseq": "5"}),
        json!({"raw": "INBOX", "uidValidity": 7, "modseq": "9223372036854775808"}),
        json!({"raw": "INBOX", "uidValidity": 7, "modseq": 5}),
    ] {
        let r = Received::from_json(Some(&json!({ "steps": [bad] })));
        assert!(r.steps.is_empty(), "{bad} was taken: {:?}", r.steps);
    }
}
