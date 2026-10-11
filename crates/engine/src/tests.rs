use crate::{EngineError, Session, catalog, registry};
use mailcraft_model::{FolderKind, RECIP_CC, RECIP_TO, time};
use serde_json::{Value, json};

/// 2026-10-07 14:30 UTC, a Wednesday.
fn now() -> i64 {
    time::from_civil(2026, 10, 7, 14, 30, 0, 0)
}

fn session() -> Session {
    let mut s = Session::new();
    s.now = now();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.run(id, &params).unwrap_or_else(|e| panic!("{id} failed: {e}"))
}

fn folder(s: &Session, name: &str) -> (u32, u32) {
    let f = s.folders[0].iter().find(|f| f.name == name).unwrap();
    (f.total, f.unread)
}

#[test]
fn command_ids_are_unique_and_described() {
    let mut ids: Vec<&str> = registry().iter().map(|c| c.id).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "two commands share an id");
    for c in registry() {
        assert!(!c.label.is_empty() && c.id.contains('.'), "{} needs a label and a dotted id", c.id);
    }
}

#[test]
fn catalog_names_real_commands_and_parity_only_rises() {
    for f in catalog::FEATURES {
        assert!(f.command.is_empty() || f.implemented(), "{} points at a command that does not exist: {}", f.name, f.command);
    }
    let (done, total) = catalog::summary();
    // The floor only goes up: raise it when features land.
    assert!(done >= 33, "parity fell to {done} of {total}");
    assert!(catalog::checklist().contains("- [x] New: New Email"));
}

/// Every command, with every kind of junk for parameters, on a session with and without a
/// selection and with a compose window open: it may refuse, it may not panic.
#[test]
fn hostile_params_never_panic() {
    let junk = [
        Value::Null,
        json!(42),
        json!("text"),
        json!([1, 2, 3]),
        json!({}),
        json!({"index": -1, "id": -5, "store": -1, "folderId": -9}),
        json!({"index": 1e300, "id": 1.5, "store": 1e99, "folderId": 2.5}),
        json!({"index": u64::MAX, "id": u64::MAX, "store": u64::MAX, "folderId": u64::MAX}),
        json!({"index": "0", "id": [], "store": {}, "folderId": null}),
        json!({"path": 7, "folder": 7, "name": 7, "field": 7, "value": 7, "key": 7, "module": 7, "position": 7, "level": 7, "category": 7, "text": 7}),
        json!({"path": "", "folder": "", "name": "", "field": "", "value": "", "key": "", "module": "", "position": "", "level": "", "category": "", "text": ""}),
        json!({"path": "\u{0}", "folder": "a/b/c/d/../..", "name": "x/y", "field": "body", "value": "é".repeat(40_000), "key": "\u{202e}", "category": "c".repeat(4000), "text": "\u{0}\u{ffff}"}),
        json!({"path": "Z:\\no\\such\\dir\\file.msg", "folder": "/", "name": "\u{7}", "field": "to", "value": "<<<>>>;;;,,,\"", "index": 99999}),
        json!({"show": "yes", "on": 3, "descending": "no", "clear": "true"}),
    ];
    let setups: [fn(&mut Session); 4] = [
        |_| {},
        |s| {
            let _ = s.run("mail.select", &json!({"index": 0}));
        },
        |s| {
            let _ = s.run("mail.select", &json!({"index": 1}));
            let _ = s.run("mail.replyAll", &Value::Null);
        },
        |s| {
            while !s.stores.is_empty() {
                let _ = s.run("file.closeStore", &Value::Null);
            }
        },
    ];
    for setup in setups {
        for c in registry() {
            for p in &junk {
                let mut s = session();
                setup(&mut s);
                let result = s.run(c.id, p);
                assert!(!matches!(result, Err(EngineError::Internal(_))), "{} panicked on {p}", c.id);
                // Whatever happened, the session still answers.
                let _ = s.inspect();
                assert!(!matches!(s.run("session.inspect", &Value::Null), Err(EngineError::Internal(_))));
            }
        }
    }
    assert!(matches!(session().run("no.such.command", &Value::Null), Err(EngineError::UnknownCommand(_))));
}

#[test]
fn opens_on_the_inbox_newest_first() {
    let s = session();
    assert_eq!(s.current_folder().unwrap().kind, FolderKind::Inbox);
    assert_eq!(s.rows.len(), 9);
    assert_eq!(s.rows[0].subject, "Launch checklist: three items left");
    assert!(s.rows.windows(2).all(|w| w[0].time >= w[1].time));
    assert_eq!(folder(&s, "Inbox"), (9, 3));
    assert!(s.selected.is_none() && s.open.is_none());
}

#[test]
fn select_can_go_to_a_folder_first() {
    let mut s = session();
    run(&mut s, "mail.select", json!({ "folder": "Sent Items", "index": 0 }));
    assert_eq!(s.current_folder().unwrap().kind, FolderKind::Sent);
    assert_eq!(s.open.as_ref().unwrap().subject(), "RE: Launch checklist: three items left");
    assert!(s.run("mail.select", &json!({ "folder": "No/Such", "index": 0 })).is_err());
    assert_eq!(s.current_folder().unwrap().kind, FolderKind::Sent);
    // A folder alone goes there and opens nothing.
    run(&mut s, "mail.select", json!({ "folder": "Inbox" }));
    assert_eq!(s.current_folder().unwrap().kind, FolderKind::Inbox);
    assert!(s.open.is_none());
    assert!(s.run("mail.select", &json!({})).is_err());
}

#[test]
fn selecting_marks_read_and_delete_moves_to_deleted_items() {
    let mut s = session();
    run(&mut s, "mail.select", json!({"index": 0}));
    assert_eq!(folder(&s, "Inbox"), (9, 2), "the reading pane marks the message read");
    assert!(s.rows[0].read);
    run(&mut s, "mail.markUnread", Value::Null);
    assert_eq!(folder(&s, "Inbox"), (9, 3));

    let second = s.rows[1].id;
    run(&mut s, "mail.delete", Value::Null);
    assert_eq!(folder(&s, "Inbox"), (8, 2));
    assert_eq!(folder(&s, "Deleted Items").0, 2);
    assert_eq!(s.selected, Some(second), "selection moves to the next message");
    assert!(!s.rows[0].read, "moving on does not mark the neighbour read");

    // Deleting inside Deleted Items removes for good.
    run(&mut s, "folder.select", json!({"folder": "Deleted Items"}));
    run(&mut s, "mail.select", json!({"index": 0}));
    run(&mut s, "mail.delete", Value::Null);
    assert_eq!(folder(&s, "Deleted Items").0, 1);
    run(&mut s, "folder.empty", Value::Null);
    assert_eq!(folder(&s, "Deleted Items").0, 0);
}

#[test]
fn flags_categories_and_moves() {
    let mut s = session();
    run(&mut s, "mail.select", json!({"index": 1}));
    run(&mut s, "mail.flag", Value::Null);
    run(&mut s, "mail.categorize", json!({"category": "Green category"}));
    let row = &s.rows[1];
    assert_eq!((row.flag, row.categories.as_slice()), (mailcraft_model::Flag::Flagged, ["Green category".to_string()].as_slice()));
    run(&mut s, "mail.categorize", json!({"category": "green CATEGORY"}));
    assert!(s.rows[1].categories.is_empty(), "choosing a category again removes it");
    run(&mut s, "mail.move", json!({"folder": "Inbox/Projects"}));
    assert_eq!(folder(&s, "Projects").0, 3);
    assert!(s.run("mail.move", &json!({"folder": "Nowhere"})).is_err());
}

#[test]
fn reply_reply_all_and_forward_follow_outlook() {
    let mut s = session();
    run(&mut s, "mail.select", json!({"index": 0}));
    run(&mut s, "mail.reply", Value::Null);
    let d = s.compose.clone().unwrap();
    assert_eq!(
        (d.kind, d.to.as_str(), d.cc.as_str(), d.subject.as_str()),
        ("Reply", "Priya Raman <priya.raman@example.org>", "", "RE: Launch checklist: three items left")
    );
    assert!(d.attachments.is_empty(), "a reply leaves attachments behind");
    let header = "\n\n-----Original Message-----\nFrom: Priya Raman <priya.raman@example.org>\nSent: Wednesday, October 7, 2026 2:05 PM\nTo: Alex Morgan\nCc: Tomás Lindqvist\nSubject: Launch checklist: three items left\n\nHi Alex,";
    assert!(d.body.starts_with(header), "{}", d.body);

    run(&mut s, "mail.replyAll", Value::Null);
    let d = s.compose.clone().unwrap();
    assert_eq!(
        (d.to.as_str(), d.cc.as_str()),
        ("Priya Raman <priya.raman@example.org>", "Tomás Lindqvist <tomas.lindqvist@example.org>"),
        "the user's own address is left out"
    );

    run(&mut s, "mail.forward", Value::Null);
    let d = s.compose.clone().unwrap();
    assert_eq!((d.to.as_str(), d.subject.as_str(), d.attachments.len()), ("", "FW: Launch checklist: three items left", 1));

    // "RE:" is not stacked.
    run(&mut s, "mail.select", json!({"index": 1}));
    run(&mut s, "mail.reply", Value::Null);
    assert_eq!(s.compose.as_ref().unwrap().subject, "RE: Pricing page wording");
}

#[test]
fn send_files_in_the_outbox_and_never_leaves_the_machine() {
    let mut s = session();
    run(&mut s, "mail.new", Value::Null);
    assert!(s.run("compose.send", &Value::Null).is_err(), "a message with no recipient is not sent");
    run(&mut s, "compose.setField", json!({"field": "to", "value": "June Okafor <june.okafor@example.org>; tomas.lindqvist@example.org"}));
    run(&mut s, "compose.setField", json!({"field": "cc", "value": "helena.brandt@example.org"}));
    run(&mut s, "compose.setField", json!({"field": "subject", "value": "Rota\r\nlooks good"}));
    run(&mut s, "compose.setField", json!({"field": "body", "value": "Thanks, June.\nAlex"}));
    run(&mut s, "compose.send", Value::Null);
    assert!(s.compose.is_none());
    assert_eq!(folder(&s, "Outbox").0, 1);
    run(&mut s, "folder.select", json!({"folder": "Outbox"}));
    run(&mut s, "mail.select", json!({"index": 0}));
    let it = s.open.clone().unwrap();
    assert_eq!(it.subject(), "Rota looks good");
    assert_eq!(it.display_recipients(RECIP_TO), "June Okafor; tomas.lindqvist@example.org");
    assert_eq!(it.display_recipients(RECIP_CC), "helena.brandt@example.org");
    assert_eq!((it.sender_email().as_str(), it.is_unsent(), it.body_text().as_str()), ("alex.morgan@example.org", false, "Thanks, June.\nAlex"));

    run(&mut s, "mail.new", Value::Null);
    run(&mut s, "compose.setField", json!({"field": "subject", "value": "Later"}));
    run(&mut s, "compose.saveDraft", Value::Null);
    assert_eq!(folder(&s, "Drafts").0, 2);
}

#[test]
fn sort_search_and_filter() {
    let mut s = session();
    run(&mut s, "view.sortBy", json!({"key": "from"}));
    let from: Vec<&str> = s.rows.iter().map(|r| r.from.as_str()).collect();
    assert_eq!((from[0], from[8]), ("Facilities Desk", "Travel Bookings"));
    run(&mut s, "view.sortBy", json!({"key": "from"}));
    assert_eq!(s.rows[0].from, "Travel Bookings", "arranging by the same column again reverses it");
    run(&mut s, "view.sortBy", json!({"key": "date"}));

    run(&mut s, "search.query", json!({"text": "ROTA"}));
    assert_eq!(s.rows.iter().map(|r| r.subject.as_str()).collect::<Vec<_>>(), ["Launch checklist: three items left", "Support rota draft"]);
    run(&mut s, "search.clear", Value::Null);
    run(&mut s, "view.filterUnread", Value::Null);
    assert_eq!(s.rows.len(), 3);
    run(&mut s, "view.filterUnread", json!({"on": false}));
    assert_eq!(s.rows.len(), 9);
}

#[test]
fn folders_can_be_made_renamed_and_deleted_but_not_the_inbox() {
    let mut s = session();
    assert!(!s.is_enabled("folder.rename") && !s.is_enabled("folder.delete"), "the Inbox is not the user's to rename or delete");
    let id = run(&mut s, "folder.new", json!({"name": "Clients"}))["folderId"].as_u64().unwrap();
    assert!(s.run("folder.new", &json!({"name": "clients"})).is_err(), "names are unique inside a folder");
    run(&mut s, "folder.select", json!({"folderId": id}));
    run(&mut s, "folder.rename", json!({"name": "Customers"}));
    assert_eq!(s.current_folder().unwrap().name, "Customers");
    run(&mut s, "folder.delete", Value::Null);
    assert_eq!(s.current_folder().unwrap().name, "Inbox");
    assert!(s.folders[0].iter().all(|f| f.name != "Customers"));
}

#[test]
fn messages_save_and_reopen_as_msg_and_eml() {
    let dir = std::env::temp_dir().join(format!("mailcraft-engine-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = session();
    run(&mut s, "mail.select", json!({"index": 0}));
    for ext in ["msg", "eml", "oft"] {
        let path = dir.join(format!("launch.{ext}"));
        run(&mut s, "file.saveAs", json!({"path": path}));
        let mut other = Session::new();
        let opened = run(&mut other, "file.open", json!({"path": path}));
        assert_eq!(opened["store"], 0);
        let it = other.open.clone().unwrap();
        assert_eq!(it.subject(), "Launch checklist: three items left", "{ext}");
        assert_eq!(it.sender_name(), "Priya Raman", "{ext}");
        assert!(it.body_text().contains("Support rota for the first week"), "{ext}");
        assert_eq!(it.attachments.len(), 1, "{ext}");
        assert_eq!(it.attachments[0].name(), "launch-timeline.csv", "{ext}");
        assert!(!other.stores[0].display_name().is_empty());
    }
    // A refused save leaves the existing file as it was.
    let keep = dir.join("keep.pst");
    std::fs::write(&keep, b"original").unwrap();
    assert!(s.run("file.saveAs", &json!({"path": keep})).is_err());
    assert_eq!(std::fs::read(&keep).unwrap(), b"original");
    run(&mut s, "file.saveAttachment", json!({"index": 0, "path": dir.join("timeline.csv")}));
    assert!(std::fs::read_to_string(dir.join("timeline.csv")).unwrap().starts_with("milestone,owner,due"));
    assert!(matches!(s.run("file.open", &json!({"path": dir.join("missing.msg")})), Err(EngineError::Io(_))));
    std::fs::write(dir.join("junk.msg"), b"not a compound file").unwrap();
    assert!(matches!(s.run("file.open", &json!({"path": dir.join("junk.msg")})), Err(EngineError::Format(_))));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn closing_data_files_keeps_the_session_usable() {
    let mut s = session();
    run(&mut s, "file.openSample", Value::Null);
    assert_eq!((s.stores.len(), s.current.unwrap().0), (2, 1));
    run(&mut s, "file.closeStore", json!({"store": 0}));
    assert_eq!((s.stores.len(), s.current.unwrap().0), (1, 0));
    run(&mut s, "file.closeStore", Value::Null);
    assert!(s.stores.is_empty() && s.current.is_none() && s.rows.is_empty());
    assert!(!s.is_enabled("mail.select") && s.is_enabled("file.open"));
}

#[test]
fn inspect_marks_message_text_as_untrusted() {
    let mut s = session();
    run(&mut s, "mail.select", json!({"index": 0}));
    let v = run(&mut s, "session.inspect", Value::Null);
    assert!(v["open"]["untrusted_content"]["body"].as_str().unwrap().contains("launch checklist"));
    assert!(v["open"].get("body").is_none() && v["rows"][0].get("subject").is_none());
}

#[test]
fn a_message_kept_only_as_rtf_has_text_a_preview_and_survives_as_html() {
    use mailcraft_model::{Item, PropValue, pid, rtf};
    let mut s = session();
    run(&mut s, "mail.select", json!({ "folder": "Inbox/Receipts", "index": 0 }));
    let item = s.open.as_ref().unwrap();
    assert!(item.props.get(pid::BODY).is_none() && item.props.get(pid::HTML).is_none());
    let text = item.body_text();
    assert!(text.starts_with("Riverside Car Park\nReceipt for permit RP-20418, paid by card."), "{text}");
    assert!(text.contains("Monthly permit, bay 14\t62.00"));
    assert!(!text.contains("HYPERLINK") && !text.contains("Arial"));
    assert!(s.rows[0].preview.starts_with("Riverside Car Park Receipt for permit"));
    assert_eq!(item.body_html(), None, "ordinary RTF is not HTML");

    // RTF that wraps HTML ([MS-OXRTFEX]) is the message's HTML body wherever one is asked for.
    let mut wrapped = Item::new_mail();
    let body = b"{\\rtf1\\ansi\\fromhtml1 {\\*\\htmltag64 <p>}\\htmlrtf {\\htmlrtf0 Hello {\\*\\htmltag84 <b>}you{\\*\\htmltag92 </b>}\\htmlrtf }\\htmlrtf0{\\*\\htmltag72 </p>}}";
    wrapped.props.set(pid::RTF_COMPRESSED, PropValue::Binary(rtf::store(body)));
    assert_eq!(wrapped.body_html().as_deref(), Some("<p>Hello <b>you</b></p>"));
    assert_eq!(wrapped.body_text(), "Hello you");
    // Damaged or foreign bytes in the property are no body, not an error.
    wrapped.props.set(pid::RTF_COMPRESSED, PropValue::Binary(vec![0xAB; 40]));
    assert_eq!((wrapped.body_html(), wrapped.body_text()), (None, String::new()));
}

#[test]
fn a_picture_sent_with_a_message_is_an_attachment_the_body_shows_not_a_listed_one() {
    let mut s = session();
    run(&mut s, "mail.select", json!({ "index": 4 }));
    let item = s.open.as_ref().unwrap();
    assert_eq!(item.attachments.len(), 1);
    let proof = &item.attachments[0];
    assert_eq!((proof.name().as_str(), proof.mime()), ("banner-proof.png", "image/png"));
    assert!(proof.data.starts_with(b"\x89PNG\r\n\x1a\n") && proof.data.ends_with(b"IEND\xAE\x42\x60\x82"));
    assert!(item.body_html().unwrap().contains(&format!("cid:{}", proof.content_id().unwrap())));
    assert_eq!(item.visible_attachments().count(), 0, "the body shows it, so the attachment list does not");
}
