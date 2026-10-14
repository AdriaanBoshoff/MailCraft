//! Receiving: new mail is taken down from the in-process fake server into the Inbox, nothing
//! twice, and the server is changed only as the account says and only once mail is kept.

use crate::Session;
use openmailclient_net::fake::Fake;
use serde_json::{Value, json};
use std::sync::Arc;

fn message(subject: &str) -> Vec<u8> {
    format!("From: June <june@example.test>\r\nTo: pat@example.test\r\nSubject: {subject}\r\nDate: Tue, 13 Oct 2026 09:00:00 +0000\r\nMessage-ID: <{subject}@example.test>\r\n\r\nHello.\r\n.\r\n..and a dot\r\n").into_bytes()
}

fn with(fake: &Fake, kind: &str, extra: Value) -> Session {
    let mut s = Session::new();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.connector = Some(Arc::new(fake.clone()));
    let mut account = json!({
        "email": "pat@example.test", "kind": kind,
        "incoming": { "host": "mail.example.test", "security": "starttls" },
        "outgoing": { "host": "mail.example.test", "security": "starttls" },
        "password": "not a real password",
    });
    if let (Some(a), Some(e)) = (account.as_object_mut(), extra.as_object()) {
        a.extend(e.clone());
    }
    s.run("account.add", &account).unwrap();
    s
}

fn inbox(s: &Session) -> (u32, u32) {
    s.folders[0].iter().find(|f| f.name == "Inbox").map_or((0, 0), |f| (f.total, f.unread))
}

fn add(fake: &Fake, subject: &str, flags: &[&str]) {
    fake.state().mailbox("INBOX").unwrap().add(&message(subject), flags);
}

#[test]
fn imap_mail_is_taken_down_once_and_left_as_it_was_on_the_server() {
    let fake = Fake::default();
    add(&fake, "one", &[]);
    add(&fake, "two", &["\\Seen"]);
    let mut s = with(&fake, "imap", json!({}));
    let (total, unread) = inbox(&s);
    let got = s.run("account.receive", &Value::Null).unwrap();
    assert_eq!(got["received"], 2, "{got}");
    assert_eq!(inbox(&s), (total + 2, unread + 1), "what the server has as read comes as read");
    assert!(s.status.starts_with("2 new messages"), "{}", s.status);
    // Read without being marked read, and nothing else said to change the server.
    let state = fake.state();
    assert!(state.mailboxes[0].messages[0].flags.is_empty());
    assert!(!state.heard.iter().any(|l| l.contains("STORE") || l.contains("EXPUNGE") || l.contains("BODY[]")), "{:?}", state.heard);
    drop(state);

    // Again: nothing new. Then one more comes, and only it is taken down.
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0);
    assert_eq!(s.status, "No new mail");
    add(&fake, "three", &[]);
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 1);
    assert_eq!(inbox(&s).0, total + 3);

    // The message came whole: its dotted lines too.
    s.run("folder.select", &json!({ "folder": "Inbox" })).unwrap();
    let at = s.rows.iter().position(|r| r.subject == "three").unwrap();
    s.run("mail.select", &json!({ "index": at })).unwrap();
    let it = s.open.clone().unwrap();
    assert!(it.body_text().contains(".and a dot"), "{}", it.body_text());

    // The server numbered its messages anew: all of them are new again.
    fake.state().mailbox("INBOX").unwrap().uid_validity = 99;
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 3);
}

#[test]
fn pop3_mail_leaves_the_server_only_once_it_is_saved_here() {
    let dir = std::env::temp_dir().join(format!("omc-receive-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("box.pst");
    let mut sample = Session::new();
    sample.add_store(Box::new(crate::sample::sample_store(sample.now)));
    sample.run("file.export", &json!({ "path": path })).unwrap();

    let fake = Fake::default();
    add(&fake, "one", &[]);
    add(&fake, "two", &[]);
    let mut s = Session::new();
    s.connector = Some(Arc::new(fake.clone()));
    s.run("file.open", &json!({ "path": path })).unwrap();
    s.run(
        "account.add",
        &json!({ "email": "pat@example.test", "kind": "pop3", "incoming": { "host": "mail.example.test", "security": "starttls" }, "leaveOnServer": false, "password": "not a real password" }),
    )
    .unwrap();
    let (total, _) = inbox(&s);
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 2);
    assert_eq!(inbox(&s).0, total + 2);
    // Not yet saved: the server keeps them, and they are not taken down again.
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0);
    assert_eq!(fake.state().mailboxes[0].messages.len(), 2);
    s.run("file.save", &Value::Null).unwrap();
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0);
    assert!(fake.state().mailboxes[0].messages.is_empty(), "saved here, so taken off the server");
    assert!(s.accounts[0].received.seen.is_empty(), "ids the server no longer has are forgotten");
    assert_eq!(inbox(&s).0, total + 2);
    s.run("file.closeStore", &json!({ "discard": true })).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pop3_mail_left_on_the_server_is_remembered_between_runs() {
    let fake = Fake::default();
    add(&fake, "one", &[]);
    let mut s = with(&fake, "pop3", json!({}));
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 1);
    let settings = s.settings(None);
    assert_eq!(settings["accounts"][0]["received"]["seen"][0], "uid-1", "{settings}");
    // account.list does not give what was taken down; the settings file keeps it.
    assert!(s.run("account.list", &Value::Null).unwrap()["accounts"][0].get("received").is_none());

    let dir = std::env::temp_dir().join(format!("omc-receive-settings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("settings.json");
    s.run("settings.save", &json!({ "path": file.display().to_string() })).unwrap();
    let mut again = with(&fake, "imap", json!({ "email": "other@example.test" }));
    again.run("settings.load", &json!({ "path": file.display().to_string() })).unwrap();
    again.run("account.setPassword", &json!({ "id": "account-1", "password": "not a real password" })).unwrap();
    assert_eq!(again.run("account.receive", &Value::Null).unwrap()["received"], 0, "taken down before, so not again");
    assert_eq!(fake.state().mailboxes[0].messages.len(), 1, "left on the server, as the account says");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_receive_that_fails_says_why_and_takes_nothing() {
    let fake = Fake::default();
    add(&fake, "one", &[]);
    let mut s = with(&fake, "imap", json!({ "password": "wrong" }));
    let got = s.run("account.receive", &Value::Null).unwrap();
    assert_eq!(got["received"], 0);
    assert!(s.status.contains("not accepted"), "{}", s.status);
    assert!(!s.status.contains("wrong") && !got.to_string().contains("wrong"));
    assert_eq!(s.accounts[0].received, crate::accounts::Received::default());
    assert!(s.run("account.receive", &json!({ "id": "account-9" })).is_err());
    assert!(s.run("account.receive", &json!({ "id": 7 })).is_err());
}

#[test]
fn a_message_that_cannot_be_read_is_kept_whole() {
    let fake = Fake::default();
    fake.state().mailbox("INBOX").unwrap().add(b"\x00\x01\x02 not mail at all", &[]);
    let mut s = with(&fake, "imap", json!({}));
    let (total, _) = inbox(&s);
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 1);
    assert_eq!(inbox(&s).0, total + 1);
    s.run("folder.select", &json!({ "folder": "Inbox" })).unwrap();
    let at = s.rows.iter().position(|r| r.subject.starts_with("A message that could not be read")).unwrap();
    s.run("mail.select", &json!({ "index": at })).unwrap();
    assert_eq!(s.open.as_ref().unwrap().attachments[0].data, b"\x00\x01\x02 not mail at all");
}

#[test]
fn send_and_receive_runs_on_threads_of_their_own() {
    let fake = Fake::default();
    add(&fake, "one", &[]);
    let mut s = with(&fake, "imap", json!({}));
    s.run("mail.new", &Value::Null).unwrap();
    s.run("compose.setField", &json!({ "field": "to", "value": "june@example.test" })).unwrap();
    s.run("compose.setField", &json!({ "field": "subject", "value": "Hi" })).unwrap();
    s.run("compose.setField", &json!({ "field": "body", "value": "x" })).unwrap();
    s.run("compose.saveDraft", &Value::Null).unwrap();
    let total = inbox(&s).0;
    // From the window: F9, not waited for.
    let started = s.run("mail.sendReceive", &json!({ "wait": false })).unwrap();
    assert_eq!(started["receive"]["started"], true, "{started}");
    assert!(s.run("account.receive", &json!({ "wait": false })).is_err(), "one receive at a time");
    let mut waited = 0;
    while !s.jobs.is_empty() {
        s.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
        assert!(waited < 1000, "the receive never ended: {}", s.status);
    }
    assert_eq!(inbox(&s).0, total + 1);
    assert!(fake.state().submitted.is_empty(), "a draft is not sent");
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

fn count_in(s: &Session, store: usize, name: &str) -> u32 {
    s.folders[store].iter().find(|f| f.name == name).map_or(0, |f| f.total)
}

#[test]
fn an_account_keeps_its_mail_in_a_data_file_of_its_own() {
    let dir = std::env::temp_dir().join(format!("omc-account-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fake = Fake::default();
    add(&fake, "one", &[]);
    add(&fake, "two", &[]);
    let mut s = Session::new();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.connector = Some(Arc::new(fake.clone()));
    s.account_dir = Some(dir.join("Accounts"));
    s.journal_dir = Some(dir.join("journals"));
    let sample_inbox = inbox(&s);
    s.run(
        "account.add",
        &json!({ "email": "pat@example.test", "kind": "pop3", "leaveOnServer": false,
                 "incoming": { "host": "mail.example.test", "security": "starttls" },
                 "outgoing": { "host": "mail.example.test", "security": "starttls" }, "password": "not a real password" }),
    )
    .unwrap();
    assert!(s.status.contains("data file of its own"), "{}", s.status);
    let file = s.accounts[0].data_file.clone().unwrap();
    assert_eq!(file, dir.join("Accounts").join("pat@example.test.pst"));
    assert!(file.exists() && s.stores.len() == 2);

    // The mail goes in the account's own Inbox, never in the data file that was open.
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 2);
    assert_eq!(inbox(&s), sample_inbox, "the sample keeps only its own mail");
    assert_eq!(count_in(&s, 1, "Inbox"), 2);
    // Saved on its own, so POP3 mail can leave the server on the next receive.
    settle(&mut s);
    assert!(!s.stores[1].unsaved(), "{}", s.status);
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0);
    assert!(fake.state().mailboxes[0].messages.is_empty());

    // What went is kept in the account's own Sent Items.
    s.run("mail.new", &Value::Null).unwrap();
    s.run("compose.setField", &json!({ "field": "to", "value": "june@example.test" })).unwrap();
    s.run("compose.setField", &json!({ "field": "subject", "value": "Mine" })).unwrap();
    let sent_before = count_in(&s, 0, "Sent Items");
    assert_eq!(s.run("compose.send", &json!({ "anyway": true })).unwrap()["sent"], 1);
    assert_eq!((count_in(&s, 0, "Sent Items"), count_in(&s, 1, "Sent Items")), (sent_before, 1));
    settle(&mut s);

    // Next run: the settings name the file, and it opens again with what it holds.
    let settings = s.settings(None);
    assert_eq!(settings["accounts"][0]["dataFile"], file.display().to_string(), "{settings}");
    let path = dir.join("settings.json");
    s.run("settings.save", &json!({ "path": path.display().to_string() })).unwrap();
    let mut again = Session::new();
    again.account_dir = Some(dir.join("Accounts"));
    again.run("settings.load", &json!({ "path": path.display().to_string() })).unwrap();
    assert_eq!(again.stores.len(), 1, "{}", again.status);
    assert_eq!((count_in(&again, 0, "Inbox"), count_in(&again, 0, "Sent Items")), (2, 1));

    // Removing the account keeps its mail.
    again.run("account.remove", &json!({ "id": "account-1" })).unwrap();
    assert!(again.status.contains("its mail stays"), "{}", again.status);
    assert!(file.exists());
    again.run("file.closeStore", &json!({ "discard": true })).unwrap();
    s.run("file.closeStore", &json!({ "store": 1, "discard": true })).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_data_file_named_in_the_settings_must_be_a_pst_by_its_full_path() {
    let mut s = Session::new();
    for bad in ["relative.pst", "C:/x/mail.txt", "/x/mail.txt", "\u{0}.pst"] {
        let settings = json!({ "accounts": [{ "id": "account-1", "email": "pat@example.test", "kind": "imap", "incoming": { "host": "h.test" }, "dataFile": bad }] });
        let dir = std::env::temp_dir().join(format!("omc-bad-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, settings.to_string()).unwrap();
        s.run("settings.load", &json!({ "path": path.display().to_string() })).unwrap();
        assert!(s.accounts.first().is_none_or(|a| a.data_file.is_none()), "{bad}");
        assert!(s.stores.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
