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
    assert!(done >= 34, "parity fell to {done} of {total}");
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
fn search_understands_fields_states_dates_and_sizes() {
    use crate::search::{fold, parse};
    use mailcraft_model::{Flag, Summary, time};
    assert_eq!(fold("Zöe ÅNGSTRÖM, Tomás; cafe\u{301} Ærø Łódź ß"), "zoe angstrom, tomas; cafe ærø łodz ß");

    let at = |y, m, d, h| time::from_civil(y, m, d, h, 0, 0, 120);
    let row = |subject: &str, from: &str, to: &str, time: i64| Summary {
        subject: subject.into(),
        from: from.into(),
        to: to.into(),
        time,
        read: true,
        importance: 1,
        ..Summary::default()
    };
    let rows = [
        Summary {
            has_attachments: true,
            size: 3 << 20,
            categories: vec!["Red category".into()],
            ..row("Launch checklist", "Priya Raman", "Alex Morgan", at(2026, 10, 5, 9))
        },
        Summary {
            read: false,
            flag: Flag::Flagged,
            size: 900,
            ..row("RE: Pricing page wording", "Tomás Okafor", "Alex Morgan; Zöe Ångström", at(2026, 10, 4, 23))
        },
        Summary {
            importance: 2,
            flag: Flag::Complete,
            preview: "The lift is out until noon: use the stairs".into(),
            size: 2048,
            ..row("Lift maintenance", "Facilities Desk", "Everyone", at(2026, 9, 30, 8))
        },
        Summary { importance: 0, ..row("from: a subject with a colon, 10:30", "June Park", "Alex Morgan", 0) },
    ];
    // Dates are the user's days: here two hours east of UTC.
    let find = |text: &str| -> Vec<usize> {
        let query = parse(text, 120);
        rows.iter().enumerate().filter(|(_, r)| query.matches(r)).map(|(n, _)| n).collect()
    };
    let all = vec![0, 1, 2, 3];
    for (text, want) in [
        ("", all.clone()),
        ("   ", all.clone()),
        // Words: all of them, in any field, in either case, with or without accents.
        ("alex", vec![0, 1, 3]),
        ("ALEX pricing", vec![1]),
        ("tomas", vec![1]),
        ("Tomás", vec![1]),
        ("zoe angstrom", vec![1]),
        ("stairs", vec![2]),
        ("red", vec![0]),
        ("\"page wording\"", vec![1]),
        ("\"wording page\"", vec![]),
        ("page wording", vec![1]),
        // Leaving out, and either side.
        ("alex -pricing", vec![0, 3]),
        ("alex NOT pricing", vec![0, 3]),
        ("lift OR launch", vec![0, 2]),
        ("lift OR", vec![2]),
        ("OR", all.clone()),
        ("alex AND pricing", vec![1]),
        ("\"OR\"", vec![0, 1, 3]),
        ("or", vec![0, 1, 3]),
        ("-", vec![]),
        // Fields.
        ("from:priya", vec![0]),
        ("from:\"facilities desk\"", vec![2]),
        ("to:alex", vec![0, 1, 3]),
        ("to:alex -from:june", vec![0, 1]),
        ("subject:lift", vec![2]),
        ("subject:stairs", vec![]),
        ("category:red", vec![0]),
        ("-category:red", vec![1, 2, 3]),
        // What a message is.
        ("has:attachment", vec![0]),
        ("hasattachments:yes", vec![0]),
        ("hasattachments:no", vec![1, 2, 3]),
        ("is:unread", vec![1]),
        ("read:no", vec![1]),
        ("is:read", vec![0, 2, 3]),
        ("is:flagged", vec![1]),
        ("isflagged:yes", vec![1]),
        ("is:complete", vec![2]),
        ("is:important", vec![2]),
        ("importance:high", vec![2]),
        ("importance:low", vec![3]),
        ("IS:Unread", vec![1]),
        // Days, months and years; a message with no time is on none of them.
        ("on:2026-10-05", vec![0]),
        ("received:2026-10-04", vec![1]),
        ("on:2026-10", vec![0, 1]),
        ("on:2026", vec![0, 1, 2]),
        ("before:2026-10-05", vec![1, 2]),
        ("after:2026-10-04", vec![0]),
        ("after:2026-09", vec![0, 1]),
        ("before:2026", vec![]),
        ("after:2026-12", vec![]),
        ("-on:2026", vec![3]),
        // Sizes.
        ("size:>2mb", vec![0]),
        ("size:<1kb", vec![1, 3]),
        ("size:1kb", vec![0, 2]),
        ("size:>=2048", vec![0]),
        // What is not understood is looked for as it stands: nothing is found, not everything.
        ("colour:blue", vec![]),
        ("is:purple", vec![]),
        ("on:someday", vec![]),
        ("on:2026-13", vec![]),
        ("on:2026-10-32", vec![]),
        ("size:big", vec![]),
        ("size:>99999999999999999999mb", vec![]),
        ("from:", vec![3]),
        ("10:30", vec![3]),
        ("\"from: a subject\"", vec![3]),
    ] {
        assert_eq!(find(text), want, "{text:?}");
    }

    // Through the command, on the sample mailbox.
    let mut s = session();
    let found = run(&mut s, "search.query", json!({"text": "from:priya has:attachment"}));
    assert_eq!(found, json!({"count": 1}));
    assert_eq!((s.rows[0].subject.as_str(), s.status.as_str()), ("Launch checklist: three items left", "1 item found"));
    assert_eq!(run(&mut s, "search.query", json!({"text": "from:tomas OR from:helena"}))["count"], json!(s.rows.len()));
    assert!(s.rows.len() >= 2 && s.rows.iter().all(|r| r.from.starts_with("Tomás") || r.from.starts_with("Helena")));
    run(&mut s, "search.query", json!({"text": "no such words anywhere"}));
    assert_eq!((s.rows.len(), s.status.as_str()), (0, "Nothing found"));
    run(&mut s, "search.clear", Value::Null);
    assert_eq!(s.rows.len(), 9);
}

#[test]
fn hostile_queries_never_panic() {
    use crate::search::parse;
    let row = mailcraft_model::Summary { subject: "é\u{301}\u{0}".into(), time: i64::MAX, size: u32::MAX, ..Default::default() };
    let pieces = [
        "\"",
        "-",
        ":",
        "--",
        "OR",
        "NOT",
        "AND",
        "on:",
        "9999-12-31",
        "0000",
        "size:",
        ">",
        "<=",
        "é",
        "\u{0}",
        "\u{202e}",
        "\u{17f}",
        "\u{c0}",
        "\u{180}",
        "-\"",
        "a:b:c",
        "from:\"",
        "1601",
        "99999999999999999999",
    ];
    for a in pieces {
        for b in pieces {
            for text in [
                format!("{a}{b}"),
                format!("{a} {b}"),
                format!("on:{a}{b}"),
                format!("size:{a}{b}"),
                format!("-{a}:{b}"),
                format!("before:{a}-{b} after:{b}-{a}"),
            ] {
                for tz in [0, i32::MIN, i32::MAX] {
                    let _ = parse(&text, tz).matches(&row);
                }
            }
        }
    }
    // Far more clauses than are kept: those past the cap are left out, and it still answers.
    let long = "word ".repeat(10_000);
    assert!(parse(&long, 0).matches(&mailcraft_model::Summary { subject: "a word".into(), ..Default::default() }));
    let _ = parse(&"\"".repeat(100_001), 0);
    let _ = parse(&"OR ".repeat(50_000), 0);
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
fn an_attached_object_is_shown_by_the_best_picture_it_has() {
    use mailcraft_formats::rich::{Block, Picture};
    use mailcraft_model::{Attachment, Item, PropValue, pid, rtf};
    use std::io::Write;
    let mut dib = Vec::new();
    for n in [40u32, 2, 2] {
        dib.extend_from_slice(&n.to_le_bytes());
    }
    dib.extend_from_slice(&[1, 0, 24, 0]);
    dib.extend_from_slice(&[0; 24]);
    dib.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0]);
    let object = |kind: &str, bitmap: &[u8]| {
        let mut a = Attachment::file("Card", "", mailcraft_msg::ole::picture_object(kind, bitmap).unwrap());
        a.props.set(pid::ATTACH_METHOD, PropValue::Int32(6));
        a.props.set(pid::RENDERING_POSITION, PropValue::Int32(0));
        a
    };
    let show = |a: Attachment| {
        let mut holder = Item::default();
        holder.props.set(pid::RTF_COMPRESSED, PropValue::Binary(rtf::store(b"{\\rtf1\\ansi\\pard \\objattph\\'20\\par}")));
        let mut doc = mailcraft_formats::rtf::parse(&holder.body_rtf().unwrap());
        crate::pictures::settle(&mut doc, &Item { attachments: vec![a], ..Item::default() });
        let Some(Block::Para(para)) = doc.blocks.first() else { panic!("a paragraph") };
        let run = para.runs.iter().find(|r| r.picture.is_some() || r.text.contains("Card")).unwrap().clone();
        let file = match run.picture {
            Some(Picture::Inline(Some(n))) => Some(doc.written[n].to_vec()),
            _ => None,
        };
        (run.text, file)
    };
    // The picture the object keeps of itself.
    let (text, file) = show(object("Permit card", &dib));
    assert_eq!(text, "Card");
    let pixels = crate::pictures::decode(&file.unwrap()).unwrap();
    assert_eq!((pixels.width, pixels.height), (2, 2));
    // One that cannot be read: the attachment is named, with what the object calls itself.
    let (text, file) = show(object("Permit\u{7} card", &dib[..30]));
    assert_eq!((text.as_str(), file), ("[Attachment: Card (Permit card)]", None));
    // The object's own data, when that is a picture file, goes before what it keeps for showing:
    // here a bitmap file of one pixel beside a presentation of two by two.
    let mut bmp = b"BM".to_vec();
    bmp.extend_from_slice(&[58, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0]);
    for n in [40u32, 1, 1] {
        bmp.extend_from_slice(&n.to_le_bytes());
    }
    bmp.extend_from_slice(&[1, 0, 24, 0]);
    bmp.extend_from_slice(&[0; 24]);
    bmp.extend_from_slice(&[0, 0, 255, 0]);
    let mut a = object("Bitmap", &dib);
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(std::mem::take(&mut a.data))).unwrap();
    let mut native = u32::try_from(bmp.len()).unwrap().to_le_bytes().to_vec();
    native.extend_from_slice(&bmp);
    cf.create_stream("/\u{1}Ole10Native").unwrap().write_all(&native).unwrap();
    cf.flush().unwrap();
    a.data = cf.into_inner().into_inner();
    let (_, file) = show(a);
    assert_eq!(file.as_deref(), Some(&bmp[..]));
    // Data that only claims to be a picture is passed over for the next one that reads.
    let mut a = object("Bitmap", &dib);
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(std::mem::take(&mut a.data))).unwrap();
    cf.create_stream("/\u{1}Ole10Native").unwrap().write_all(&[4, 0, 0, 0, b'B', b'M', 1, 2]).unwrap();
    cf.flush().unwrap();
    a.data = cf.into_inner().into_inner();
    let pixels = crate::pictures::decode(&show(a).1.unwrap()).unwrap();
    assert_eq!((pixels.width, pixels.height), (2, 2));
}

#[test]
fn attachments_stand_in_their_places_in_an_rtf_body() {
    use mailcraft_formats::rich::{Block, Picture};
    use mailcraft_model::{Attachment, Item, PropValue, pid, rtf};
    // A bitmap of two by two pixels, the lower left one blue, drawn by a metafile (META_STRETCHDIB).
    let mut dib = Vec::new();
    for n in [40u32, 2, 2] {
        dib.extend_from_slice(&n.to_le_bytes());
    }
    dib.extend_from_slice(&[1, 0, 24, 0]);
    dib.extend_from_slice(&[0; 24]);
    dib.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0]);
    let mut wmf = vec![1, 0, 9, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    wmf.extend_from_slice(&u32::try_from((28 + dib.len()) / 2).unwrap().to_le_bytes());
    wmf.extend_from_slice(&[0x43, 0x0F, 0x20, 0, 0xCC, 0, 0, 0]);
    wmf.extend_from_slice(&[0; 16]);
    wmf.extend_from_slice(&dib);
    wmf.extend_from_slice(&[3, 0, 0, 0, 0, 0]);

    let at = |mut a: Attachment, place: i32| {
        a.props.set(pid::RENDERING_POSITION, PropValue::Int32(place));
        a
    };
    let mut chart = at(Attachment::file("chart", "", Vec::new()), 20);
    chart.props.set(pid::ATTACH_METHOD, PropValue::Int32(6));
    chart.props.set(pid::ATTACH_RENDERING, PropValue::Binary(wmf));
    let mut hidden = at(Attachment::file("hidden.png", "image/png", vec![1]), 1);
    hidden.props.set(pid::ATTACHMENT_HIDDEN, PropValue::Bool(true));
    let attachments = vec![
        at(Attachment::file("notes\u{7}.txt", "text/plain", b"notes".to_vec()), 500),
        chart,
        hidden,
        at(Attachment::file("unplaced.txt", "text/plain", vec![2]), -1),
        Attachment::file("from-mime.txt", "text/plain", vec![3]),
    ];
    let mut item = Item { attachments, ..Item::default() };
    let names: Vec<String> = crate::pictures::standing(&item).iter().map(|a| a.name()).collect();
    assert_eq!(names, ["chart", "notes\u{7}.txt"], "by rendering position; hidden ones and those with no place are left out");

    let read = |places: usize, item: &Item| {
        let body = format!("{{\\rtf1\\ansi\\pard See {}\\par}}", "\\objattph\\'20".repeat(places));
        let mut holder = Item::default();
        holder.props.set(pid::RTF_COMPRESSED, PropValue::Binary(rtf::store(body.as_bytes())));
        let mut doc = mailcraft_formats::rtf::parse(&holder.body_rtf().unwrap());
        crate::pictures::settle(&mut doc, item);
        doc
    };
    let doc = read(2, &item);
    let Some(Block::Para(para)) = doc.blocks.first() else { panic!("{doc:?}") };
    let stands: Vec<(Option<Picture>, &str)> =
        para.runs.iter().map(|r| (r.picture.clone(), r.text.as_str())).filter(|(p, t)| p.is_some() || t.contains('[')).collect();
    assert_eq!(stands, [(Some(Picture::Inline(Some(0))), "chart"), (None, "[Attachment: notes.txt]")]);
    // The object's own picture is a picture file the reading pane and print can decode.
    let pixels = crate::pictures::decode(&doc.written[0]).unwrap();
    assert_eq!((pixels.width, pixels.height), (2, 2));
    assert_eq!(&pixels.rgba[8..12], &[0, 0, 255, 255]);

    // The body and the attachments do not agree: the places are given up, nothing is shown there.
    for places in [1, 3, 300] {
        let doc = read(places, &item);
        assert!(doc.written.is_empty());
        assert_eq!(doc.text().trim(), "See", "{places}");
        let Some(Block::Para(para)) = doc.blocks.first() else { panic!("{doc:?}") };
        assert!(para.runs.iter().all(|r| r.picture.is_none()));
    }
    // A rendering that is not a metafile of one bitmap: the object is named.
    item.attachments[1].props.set(pid::ATTACH_RENDERING, PropValue::Binary(vec![0xFF; 64]));
    assert!(read(2, &item).text().contains("[Attachment: chart]"));
}

#[test]
fn a_message_prints_to_a_pdf_file() {
    let dir = std::env::temp_dir().join(format!("mailcraft-engine-print-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = session();
    // Nothing is open: there is nothing to print.
    assert!(!s.is_enabled("file.print"));
    run(&mut s, "mail.select", json!({"index": 0}));
    // The target and the paper are checked before anything is laid out or written.
    let wrong = dir.join("print.msg");
    assert!(matches!(s.run("file.print", &json!({"path": wrong})), Err(EngineError::BadParams(_))));
    assert!(matches!(s.run("file.print", &json!({"path": dir.join("p.pdf"), "paper": "foolscap"})), Err(EngineError::BadParams(_))));
    assert!(!wrong.exists() && !dir.join("p.pdf").exists());

    // With the test font the pages are known; the installed fonts differ from machine to machine.
    let item = s.open.clone().unwrap();
    let library = mailcraft_fonts::Library::empty();
    assert!(matches!(crate::print::layout(&item, "Alex Morgan", 0, mailcraft_layout::Paper::A4, &library).map(|_| ()), Err(EngineError::Format(_))));
    let printed = {
        let mut fonts = mailcraft_layout::Fonts::new(&library);
        let chars: String = (' '..='~').chain(['\u{e1}', '\u{2022}']).collect();
        fonts.give("Arial", false, false, mailcraft_fonts::Face { data: mailcraft_fonts::boxes::font(&chars), index: 0 });
        let fields = [("Subject", item.subject())];
        let body = mailcraft_formats::html::parse(&item.body_html().unwrap());
        mailcraft_layout::memo(
            &mailcraft_layout::Memo { owner: "Alex Morgan", fields: &fields, body: &body, plain: false, sizes: &|_| None },
            mailcraft_layout::Paper::A4,
            &mut fonts,
        )
        .unwrap()
    };
    let text = printed.pages[0].text();
    assert!(text.starts_with("Alex Morgan\nSubject:Launch checklist: three items left\nHi Alex,"), "{text}");
    assert!(text.contains("Support rota for the first week") && text.ends_with("\n1"));
    let file = crate::print::write(&printed, &crate::print::Pictures::default(), &item.subject()).unwrap();
    assert!(file.starts_with(b"%PDF-1.7") && file.ends_with(b"%%EOF\n"));

    // A picture the message carries is printed at the size the body asks for; one on a server
    // is named, and nothing is fetched for it.
    run(&mut s, "mail.select", json!({"index": 4}));
    let item = s.open.clone().unwrap();
    let body = mailcraft_formats::html::parse(&item.body_html().unwrap());
    let pictures = crate::print::Pictures::of(&body, &item);
    let printed = {
        let mut fonts = mailcraft_layout::Fonts::new(&library);
        fonts.give("Arial", false, false, mailcraft_fonts::Face { data: mailcraft_fonts::boxes::font("ab "), index: 0 });
        let sizes = |picture: &mailcraft_layout::Picture| pictures.get(picture).map(|p| p.natural);
        let memo = mailcraft_layout::Memo { owner: "a", fields: &[], body: &body, plain: false, sizes: &sizes };
        mailcraft_layout::memo(&memo, mailcraft_layout::Paper::A4, &mut fonts).unwrap()
    };
    let placed: Vec<_> = printed.pages.iter().flat_map(|p| &p.pictures).collect();
    assert_eq!(placed.len(), 1);
    assert_eq!((placed[0].width, placed[0].height), (180.0, 72.0));
    let file = crate::print::write(&printed, &pictures, "t").unwrap();
    let has = |needle: &[u8]| file.windows(needle.len()).any(|w| w == needle);
    assert!(has(b"/Subtype /Image /Width 240 /Height 96") && has(b"/XObject << /Im0 "));
    assert!(!crate::print::write(&printed, &crate::print::Pictures::default(), "t").unwrap().windows(15).any(|w| w == b"/Subtype /Image"));
    run(&mut s, "mail.select", json!({"index": 0}));

    // Through the command: a file, or the word that no font is installed. Never a half-written one.
    let path = dir.join("launch.pdf");
    match s.run("file.print", &json!({"path": path, "paper": "Letter"})) {
        Ok(result) => {
            assert_eq!(result["pages"], 1);
            assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF-1.7"));
        }
        Err(e) => {
            assert!(matches!(e, EngineError::Format(_)), "{e}");
            assert!(!path.exists());
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
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
    // Without this Outlook Classic shows an RTF-only message with an empty body.
    assert_eq!(item.props.get(pid::RTF_IN_SYNC), Some(&PropValue::Bool(true)));
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
