//! Import from Outlook: profiles written by hand as Outlook keeps them, data files written by our
//! own writer, and Outlook's own files left byte for byte as they were.

use crate::Session;
use crate::outlook::{self, Raw, RawProfile, Section};
use openmailclient_model::{FolderKind, MemStore, Store};
use openmailclient_net::fake::Fake;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn wide(s: &str) -> Raw {
    let mut b: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    b.extend([0, 0]);
    Raw::Bytes(b)
}

fn text(s: &str) -> Raw {
    Raw::Text(s.to_string())
}

fn section(name: &str, values: Vec<(&str, Raw)>) -> Section {
    Section { name: name.to_string(), values: values.into_iter().map(|(n, v)| (n.to_string(), v)).collect() }
}

fn message(subject: &str) -> Vec<u8> {
    format!("From: June <june@example.test>\r\nTo: pat@example.test\r\nSubject: {subject}\r\nDate: Tue, 13 Oct 2026 09:00:00 +0000\r\nMessage-ID: <{subject}@example.test>\r\n\r\nHello.\r\n").into_bytes()
}

/// A data file as Outlook might hold one: our own writer's, with mail in its Inbox.
fn data_file(path: &Path, name: &str, subjects: &[&str]) {
    let mut store = MemStore::with_default_folders(name);
    let inbox = store.folders().unwrap().iter().find(|f| f.kind == FolderKind::Inbox).unwrap().id;
    for s in subjects {
        store.add(inbox, openmailclient_mime::parse(&message(s)).unwrap()).unwrap();
    }
    let pst = path.with_extension("pst");
    crate::io::save_store(&mut store, &pst, [7; 16]).unwrap();
    if pst != path {
        std::fs::rename(&pst, path).unwrap();
    }
}

const IMAP_UID: [u8; 16] = [0x11; 16];
const POP_UID: [u8; 16] = [0x22; 16];

/// A profile with an IMAP account, a POP3 account, an Exchange one, the address book, an
/// archive opened beside them (named twice), and a few sections that make no sense.
fn profile(dir: &Path) -> RawProfile {
    let path = |name: &str| dir.join(name).display().to_string();
    RawProfile {
        name: "Outlook".into(),
        default: true,
        sections: vec![
            section("11111111111111111111111111111111", vec![("001f3d09", wide("INTERSTOR")), ("001f6610", wide(&path("work.ost")))]),
            section("abababababababababababababababab", vec![("01023d0c", Raw::Bytes(POP_UID.to_vec())), ("001f6700", text(&path("home.pst")))]),
            section("33333333333333333333333333333333", vec![("001f3001", wide("Archive 2025")), ("001f6700", wide(&path("archive.pst")))]),
            section("44444444444444444444444444444444", vec![("001f6700", wide(&path("archive.pst").to_uppercase()))]),
            section("55555555555555555555555555555555", vec![("001f6700", wide("relative.pst"))]),
            section("66666666666666666666666666666666", vec![("001f6700", wide(&path("notes.txt")))]),
        ],
        accounts: vec![
            section("00000001", vec![("Account Name", text("Outlook Address Book")), ("Service UID", Raw::Bytes(vec![9; 16]))]),
            section(
                "00000002",
                vec![
                    ("Account Name", text("Work")),
                    ("Email", wide("pat@example.test")),
                    ("Display Name", wide("Pat Example")),
                    ("IMAP Server", text("Mail.Example.Test")),
                    ("IMAP User", text("pat")),
                    ("IMAP Port", Raw::Number(143)),
                    ("IMAP Secure Connection", Raw::Number(2)),
                    ("SMTP Server", text("mail.example.test")),
                    ("SMTP Port", Raw::Number(587)),
                    ("SMTP Secure Connection", Raw::Number(2)),
                    ("SMTP Use Auth", Raw::Number(1)),
                    ("IMAP Password", Raw::Bytes(vec![1, 2, 3, 4])),
                    ("Service UID", Raw::Bytes(IMAP_UID.to_vec())),
                ],
            ),
            section(
                "00000003",
                vec![
                    ("Account Name", text("Home")),
                    ("Email", text("kim@example.test")),
                    ("POP3 Server", text("pop.example.test")),
                    ("POP3 Port", Raw::Bytes(110u32.to_le_bytes().to_vec())),
                    ("POP3 Use SSL", Raw::Number(0)),
                    ("SMTP Server", text("smtp.example.test")),
                    ("SMTP Port", Raw::Number(465)),
                    ("SMTP Use SSL", Raw::Number(1)),
                    ("Service UID", Raw::Bytes(POP_UID.to_vec())),
                ],
            ),
            section("00000004", vec![("Account Name", text("Corp")), ("Email", text("corp@example.test"))]),
            section("00000005", vec![("Account Name", text("Odd")), ("Email", text("not an address")), ("IMAP Server", text("bad host!"))]),
        ],
    }
}

#[test]
fn a_profile_is_read_as_outlook_keeps_it() {
    let dir = Path::new(if cfg!(windows) { r"C:\omc" } else { "/omc" });
    let p = outlook::read(&profile(dir), 0);
    assert!(p.default);
    assert_eq!(p.accounts.iter().map(|a| a.email.as_str()).collect::<Vec<_>>(), ["pat@example.test", "kim@example.test", "not an address"]);
    let work = &p.accounts[0];
    assert_eq!((work.name.as_str(), work.display_name.as_str(), work.user.as_str()), ("Work", "Pat Example", "pat"));
    assert_eq!((work.incoming.host.as_str(), work.incoming.port, work.incoming.security), ("mail.example.test", Some(143), "starttls"));
    assert_eq!(work.outgoing.as_ref().map(|o| (o.port, o.security)), Some((Some(587), "starttls")));
    assert_eq!(work.data_file.as_deref(), Some(dir.join("work.ost").as_path()), "found by its Service UID");
    assert!(work.sign_in_to_send && work.notes.is_empty());
    let home = &p.accounts[1];
    assert_eq!(home.kind, crate::accounts::Kind::Pop3);
    assert_eq!((home.incoming.port, home.incoming.security), (Some(110), "starttls"));
    assert_eq!(home.outgoing.as_ref().map(|o| o.security), Some("tls"));
    assert_eq!(home.data_file.as_deref(), Some(dir.join("home.pst").as_path()), "found through a provider of its service");
    assert_eq!(home.notes.len(), 2, "no TLS, and copies left on the server: {:?}", home.notes);
    // The archive, once, by the name the profile gives it; nothing whose path makes no sense.
    assert_eq!(p.files.len(), 1, "{:?}", p.files);
    assert_eq!((p.files[0].name.as_str(), p.files[0].path.clone()), ("Archive 2025", dir.join("archive.pst")));
    assert_eq!(p.skipped.len(), 1);
    assert_eq!(p.skipped[0].name, "Corp");
}

#[test]
fn junk_in_a_profile_is_read_without_harm() {
    // Values of every kind, of every length, with every byte; numbers where words go and words
    // where numbers go.
    let mut seed = 0x2545_f491_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let names = [
        "Email",
        "Account Name",
        "IMAP Server",
        "IMAP Port",
        "IMAP Use SSL",
        "POP3 Server",
        "SMTP Port",
        "SMTP Secure Connection",
        "Service UID",
        "001f6700",
        "001f6610",
        "01023d0c",
        "001f3001",
    ];
    let mut raw = RawProfile { name: "\u{0}\u{202e}".repeat(500), ..RawProfile::default() };
    for n in 0..300 {
        let mut values = Vec::new();
        for _ in 0..(next() % 20) {
            let name = names[(next() as usize) % names.len()];
            let len = (next() % 600) as usize;
            let v = match next() % 4 {
                0 => Raw::Number(next()),
                1 => Raw::Bytes((0..len).map(|_| (next() % 256) as u8).collect()),
                2 => Raw::Text((0..len).map(|_| char::from_u32(next() % 0x3000).unwrap_or('?')).collect()),
                _ => wide(&"@".repeat(len)),
            };
            values.push((name.to_string(), v));
        }
        let s = Section { name: format!("{n:032x}"), values };
        if n % 2 == 0 { raw.accounts.push(s) } else { raw.sections.push(s) }
    }
    let p = outlook::read(&raw, 3);
    assert!(p.accounts.len() <= outlook::MAX_ACCOUNTS);
    assert!(p.name.chars().count() <= 200);
    for a in &p.accounts {
        assert!(a.email.chars().count() <= 1024 && !a.email.chars().any(char::is_control));
        assert!(a.incoming.port != Some(0));
        let _ = crate::accounts::Account::from_json(&json!({ "email": a.email, "incoming": { "host": a.incoming.host } }), "x", None);
    }
    assert_eq!(outlook::utf16(&[0x41, 0, 0x42]), "A", "an odd byte at the end is left out");
    assert_eq!(outlook::utf16(&[0x41, 0, 0, 0, 0x42, 0]), "A", "words end at the first NUL");
}

fn hashes(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut all: Vec<(PathBuf, Vec<u8>)> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_file()).map(|p| (p.clone(), std::fs::read(&p).unwrap())).collect();
    all.sort();
    all
}

fn settle(s: &mut Session) {
    let mut waited = 0;
    while !(s.jobs.is_empty() && s.saves.is_empty()) {
        s.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
        assert!(waited < 2000, "never settled: {}", s.status);
    }
}

fn inbox_of(s: &mut Session, path: &Path) -> u32 {
    let real = std::fs::canonicalize(path).unwrap();
    let st = s.stores.iter_mut().find(|st| st.path().and_then(|p| std::fs::canonicalize(p).ok()) == Some(real.clone())).unwrap();
    st.folders().unwrap().iter().find(|f| f.kind == FolderKind::Inbox).map_or(0, |f| f.total)
}

/// Outlook's files in `dir/outlook`, and a session that reads its profile.
fn outlook_here(dir: &Path) -> (Session, PathBuf) {
    let _ = std::fs::remove_dir_all(dir);
    let theirs = dir.join("outlook");
    std::fs::create_dir_all(&theirs).unwrap();
    data_file(&theirs.join("work.ost"), "pat@example.test", &["one", "two"]);
    data_file(&theirs.join("home.pst"), "kim@example.test", &["three"]);
    data_file(&theirs.join("archive.pst"), "Archive 2025", &["four", "five", "six"]);
    let raw = profile(&theirs);
    let mut s = Session::new();
    s.account_dir = Some(dir.join("app").join("Accounts"));
    s.journal_dir = Some(dir.join("app").join("journals"));
    s.outlook = Some(Arc::new(move || vec![raw.clone()]));
    (s, theirs)
}

fn ids(preview: &Value) -> (Vec<String>, Vec<String>) {
    let of = |key: &str| preview["profiles"][0][key].as_array().unwrap().iter().map(|v| v["id"].as_str().unwrap().to_string()).collect();
    (of("accounts"), of("dataFiles"))
}

#[test]
fn accounts_and_data_files_are_imported_and_outlook_is_left_as_it_was() {
    let dir = std::env::temp_dir().join(format!("omc-import-{}", std::process::id()));
    let (mut s, theirs) = outlook_here(&dir);
    let before = hashes(&theirs);

    // What would be imported is shown, and nothing is done yet.
    let preview = s.run("file.importOutlook", &Value::Null).unwrap();
    let (accounts, files) = ids(&preview);
    assert_eq!((accounts.len(), files.len()), (3, 1), "{preview}");
    let work = &preview["profiles"][0]["accounts"][0];
    assert_eq!((work["dataFile"]["state"].as_str(), work["here"].as_bool()), (Some("ready"), Some(false)), "{work}");
    assert!(work.get("password").is_none() && !preview.to_string().contains("Password"));
    assert_eq!(preview["profiles"][0]["skipped"][0]["name"], "Corp");
    assert!(s.accounts.is_empty() && s.stores.is_empty());

    let mut all = accounts.clone();
    all.extend(files.clone());
    let done = s.run("file.importOutlook", &json!({ "import": all })).unwrap();
    assert_eq!(done["accounts"].as_array().unwrap().len(), 2, "{done}");
    assert_eq!(done["dataFiles"].as_array().unwrap().len(), 1, "{done}");
    assert_eq!(done["failed"].as_array().unwrap().len(), 1, "the account that makes no sense: {done}");
    assert!(s.status.contains("password"), "{}", s.status);

    // Each account as Outlook had it, without its password, its mail in a data file of its own.
    let pat = s.accounts.iter().find(|a| a.email == "pat@example.test").unwrap().clone();
    assert_eq!((pat.name.as_str(), pat.user.as_str(), pat.incoming.port, pat.outgoing.port), ("Work", "pat", 143, 587));
    assert!(s.vault.get(&pat.id).unwrap().is_none(), "no password is brought");
    let work_file = pat.data_file.clone().unwrap();
    assert!(work_file.starts_with(dir.join("app").join("Accounts")) && work_file.extension().unwrap() == "pst", "{work_file:?}");
    assert_eq!(inbox_of(&mut s, &work_file), 2);
    let kim = s.accounts.iter().find(|a| a.email == "kim@example.test").unwrap().clone();
    assert_eq!(inbox_of(&mut s, kim.data_file.as_ref().unwrap()), 1);
    let archive = PathBuf::from(done["dataFiles"][0]["path"].as_str().unwrap());
    assert!(archive.starts_with(dir.join("app").join("Imported")), "{archive:?}");
    assert_eq!(inbox_of(&mut s, &archive), 3);
    assert!(s.stores.iter().all(|st| st.writable()));
    // Kept for the next run as any data file.
    assert!(s.settings(None)["dataFiles"].as_array().unwrap().iter().any(|p| p.as_str() == Some(&archive.display().to_string())));

    // Again: an account here already brings its data file beside it, under a name of its own.
    let again = s.run("file.importOutlook", &json!({ "import": [accounts[0]], "wait": false })).unwrap();
    assert_eq!(again["started"], true);
    settle(&mut s);
    assert_eq!(s.accounts.len(), 2);
    let ended = s.finished.last().unwrap()["result"].clone();
    assert_eq!(ended["dataFiles"][0]["name"], "Work", "{ended}");
    s.run("file.importOutlook", &json!({ "import": [files[0]] })).unwrap();
    assert!(s.stores.iter().any(|st| st.path().is_some_and(|p| p.ends_with("Archive 2025 (2).pst"))));
    // Only the settings are asked for: no data file comes.
    let bare = s.run("file.importOutlook", &json!({ "import": [accounts[0]], "withData": false })).unwrap();
    assert_eq!(bare["failed"][0]["why"], "there is an account for pat@example.test here already");

    // Outlook's files are as they were, and nothing was put beside them.
    assert_eq!(hashes(&theirs), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_cannot_be_imported_is_refused_and_changes_nothing() {
    let dir = std::env::temp_dir().join(format!("omc-import-junk-{}", std::process::id()));
    let (mut s, theirs) = outlook_here(&dir);
    for p in [
        json!({ "import": "p0a1" }),
        json!({ "import": [] }),
        json!({ "import": [7] }),
        json!({ "import": ["p9a9"] }),
        json!({ "import": ["x".repeat(100)] }),
        json!({ "import": vec!["p0a1"; 600] }),
    ] {
        assert!(s.run("file.importOutlook", &p).is_err(), "{p}");
    }
    assert!(s.accounts.is_empty() && s.stores.is_empty());
    // A run that keeps no data files of its own imports none.
    s.account_dir = None;
    assert!(s.run("file.importOutlook", &json!({ "import": ["p0a1"] })).is_err());
    // Nor does one that does not read Outlook's settings.
    let mut none = Session::new();
    assert!(none.run("file.importOutlook", &Value::Null).is_err());
    // A data file that is not there any more is said, and the account comes without it.
    s.account_dir = Some(dir.join("app").join("Accounts"));
    std::fs::remove_file(theirs.join("work.ost")).unwrap();
    let preview = s.run("file.importOutlook", &Value::Null).unwrap();
    assert_eq!(preview["profiles"][0]["accounts"][0]["dataFile"]["state"], "missing");
    let done = s.run("file.importOutlook", &json!({ "import": ["p0a1"] })).unwrap();
    assert_eq!(done["accounts"].as_array().unwrap().len(), 1);
    assert!(done["failed"][0]["why"].as_str().unwrap().starts_with("its data file was not brought"), "{done}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn a_data_file_outlook_has_open_is_not_copied() {
    let dir = std::env::temp_dir().join(format!("omc-import-open-{}", std::process::id()));
    let (mut s, theirs) = outlook_here(&dir);
    // Held open to be written, as Outlook holds its data files while it runs.
    let held = std::fs::OpenOptions::new().read(true).write(true).open(theirs.join("archive.pst")).unwrap();
    let preview = s.run("file.importOutlook", &Value::Null).unwrap();
    assert_eq!(preview["profiles"][0]["dataFiles"][0]["state"], "inUse", "{preview}");
    let (_, files) = ids(&preview);
    let done = s.run("file.importOutlook", &json!({ "import": files })).unwrap();
    assert!(done["failed"][0]["why"].as_str().unwrap().contains("close Outlook"), "{done}");
    assert!(std::fs::read_dir(dir.join("app").join("Imported")).unwrap().next().is_none(), "no copy is left behind");
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mail_brought_from_outlook_is_not_taken_down_again() {
    let dir = std::env::temp_dir().join(format!("omc-import-sync-{}", std::process::id()));
    let (mut s, _) = outlook_here(&dir);
    let fake = Fake::default();
    for subject in ["one", "two"] {
        fake.state().mailbox("INBOX").unwrap().add(&message(subject), &[]);
    }
    s.connector = Some(Arc::new(fake.clone()));
    s.run("file.importOutlook", &json!({ "import": ["p0a1"] })).unwrap();
    let id = s.accounts[0].id.clone();
    s.run("account.setPassword", &json!({ "id": id, "password": "not a real password" })).unwrap();
    s.run("account.receive", &Value::Null).unwrap();
    settle(&mut s);
    let file = s.accounts[0].data_file.clone().unwrap();
    assert_eq!(inbox_of(&mut s, &file), 2, "the two on the server are the two brought: {}", s.status);
    let _ = std::fs::remove_dir_all(&dir);
}
