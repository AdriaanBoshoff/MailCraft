//! Accounts: kept, changed, tested against the in-process fake server, and kept between runs
//! without their passwords. No socket is opened and no real account is touched.

use crate::Session;
use openmailclient_net::fake::Fake;
use serde_json::{Value, json};
use std::sync::Arc;

const PASSWORD: &str = "not a real password";

fn with_fake(fake: Fake) -> Session {
    let mut s = Session::new();
    s.connector = Some(Arc::new(fake));
    s
}

fn imap_account() -> Value {
    json!({
        "email": "pat@example.test", "displayName": "Pat Lee", "kind": "imap",
        "incoming": { "host": "mail.example.test", "security": "starttls" },
        "outgoing": { "host": "mail.example.test", "security": "starttls" },
        "password": PASSWORD,
    })
}

/// Nothing a session says, keeps or shows holds the password.
fn never_shows_the_password(s: &mut Session) {
    let listed = s.run("account.list", &Value::Null).unwrap();
    for text in [listed.to_string(), s.inspect().to_string(), s.settings(None).to_string(), s.status.clone()] {
        assert!(!text.contains(PASSWORD), "{text}");
    }
}

#[test]
fn accounts_are_added_listed_changed_and_removed() {
    let mut s = Session::new();
    let added = s.run("account.add", &imap_account()).unwrap();
    assert_eq!(added["id"], "account-1");
    assert_eq!(added["passwordKept"], true);
    assert!(!added.to_string().contains(PASSWORD));
    // What was not given follows from what was.
    let a = &added["account"];
    assert_eq!((a["name"].as_str(), a["user"].as_str()), (Some("pat@example.test"), Some("pat@example.test")));
    assert_eq!((a["incoming"]["port"].as_u64(), a["outgoing"]["port"].as_u64()), (Some(143), Some(587)));
    never_shows_the_password(&mut s);
    let listed = s.run("account.list", &Value::Null).unwrap();
    assert_eq!(listed["accounts"][0]["hasPassword"], true);

    // A second account for the same address is refused; another address is not.
    assert!(s.run("account.add", &imap_account()).is_err());
    let pop = json!({ "email": "sam@example.test", "kind": "pop3", "incoming": { "host": "pop.example.test" } });
    let added = s.run("account.add", &pop).unwrap();
    assert_eq!(added["id"], "account-2");
    assert_eq!(added["passwordKept"], false);
    assert_eq!(added["account"]["incoming"]["port"], 995);
    assert_eq!(added["account"]["outgoing"]["host"], "pop.example.test");

    // A change keeps what it does not name; a new security brings its own port.
    let changed = s.run("account.edit", &json!({ "id": "account-2", "name": "Home", "incoming": { "security": "starttls" } })).unwrap();
    assert_eq!(changed["account"]["name"], "Home");
    assert_eq!(changed["account"]["incoming"]["host"], "pop.example.test");
    assert_eq!(changed["account"]["incoming"]["port"], 110);
    assert!(s.run("account.edit", &json!({ "id": "account-2", "email": "pat@example.test" })).is_err());

    assert_eq!(s.run("account.setPassword", &json!({ "id": "account-2", "password": "x" })).unwrap()["passwordKept"], true);
    assert_eq!(s.run("account.setPassword", &json!({ "id": "account-2", "password": "" })).unwrap()["passwordKept"], false);
    assert!(s.vault.get("account-2").unwrap().is_none());

    s.run("account.remove", &json!({ "id": "account-1" })).unwrap();
    assert!(s.vault.get("account-1").unwrap().is_none(), "a removed account's password goes with it");
    assert_eq!(s.accounts.len(), 1);
    // A new account does not take the id of one removed while another is kept.
    assert_eq!(s.run("account.add", &imap_account()).unwrap()["id"], "account-1");
}

#[test]
fn accounts_that_make_no_sense_are_refused() {
    let mut s = Session::new();
    let base = imap_account();
    let with = |key: &str, v: Value| {
        let mut p = base.clone();
        p[key] = v;
        p
    };
    for bad in [
        with("email", json!("not an address")),
        with("email", json!("a@b@c")),
        with("email", json!("pat@example.test\r\nRCPT TO:<x@y>")),
        with("kind", json!("exchange")),
        with("incoming", json!({ "host": "" })),
        with("incoming", json!({ "host": "mail example" })),
        with("incoming", json!({ "host": "mail.example.test", "port": 0 })),
        with("incoming", json!({ "host": "mail.example.test", "port": 70000 })),
        with("incoming", json!({ "host": "mail.example.test", "security": "maybe" })),
        with("incoming", json!("mail.example.test")),
        with("name", json!("n".repeat(101))),
        with("password", json!("two\nlines")),
        with("leaveOnServer", json!("yes")),
        json!([]),
        Value::Null,
    ] {
        assert!(s.run("account.add", &bad).is_err(), "{bad}");
    }
    assert!(s.accounts.is_empty());
    assert!(s.run("account.edit", &json!({ "id": "account-9" })).is_err());
}

#[test]
fn a_test_signs_in_to_both_servers_and_sends_nothing() {
    for kind in ["imap", "pop3"] {
        let fake = Fake::default();
        let mut s = with_fake(fake.clone());
        let mut account = imap_account();
        account["kind"] = json!(kind);
        let id = s.run("account.add", &account).unwrap()["id"].as_str().unwrap().to_string();
        let tested = s.run("account.test", &json!({ "id": id })).unwrap();
        assert_eq!(tested["incoming"]["ok"], true, "{kind}: {tested}");
        assert_eq!(tested["outgoing"]["ok"], true, "{kind}: {tested}");
        assert!(s.status.contains("nothing was sent"), "{}", s.status);
        assert!(fake.state().submitted.is_empty());
        never_shows_the_password(&mut s);
    }
}

#[test]
fn a_test_that_fails_says_which_server_and_why() {
    let mut s = with_fake(Fake::default());
    let mut account = imap_account();
    account["password"] = json!("wrong");
    s.run("account.add", &account).unwrap();
    let tested = s.run("account.test", &json!({ "id": "account-1" })).unwrap();
    assert_eq!((tested["incoming"]["ok"].as_bool(), tested["outgoing"]["ok"].as_bool()), (Some(false), Some(false)));
    assert!(tested["incoming"]["said"].as_str().unwrap().contains("not accepted"), "{tested}");
    assert!(!tested.to_string().contains("wrong"), "{tested}");
    // No password at all is said as that, before any server is asked.
    s.run("account.setPassword", &json!({ "id": "account-1", "password": null })).unwrap();
    let tested = s.run("account.test", &json!({ "id": "account-1" })).unwrap();
    assert!(tested["incoming"]["said"].as_str().unwrap().contains("no password"), "{tested}");
    // Without a way to reach servers, nothing is tried.
    let mut alone = Session::new();
    alone.run("account.add", &imap_account()).unwrap();
    assert!(alone.run("account.test", &json!({ "id": "account-1" })).is_err());
}

#[test]
fn a_test_runs_on_a_thread_of_its_own_and_ends_in_poll() {
    let mut s = with_fake(Fake::default());
    s.run("account.add", &imap_account()).unwrap();
    let started = s.run("account.test", &json!({ "id": "account-1", "wait": false })).unwrap();
    assert_eq!(started["started"], true);
    assert!(s.working().is_some());
    let mut waited = 0;
    while !s.poll() {
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
        assert!(waited < 1000, "the test never ended");
    }
    assert!(s.jobs.is_empty());
    let last = s.finished.last().unwrap();
    assert_eq!(last["result"]["incoming"]["ok"], true, "{last}");
    assert!(s.status.contains("nothing was sent"), "{}", s.status);
}

#[test]
fn accounts_are_kept_between_runs_without_their_passwords() {
    let dir = std::env::temp_dir().join(format!("omc-accounts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    let mut s = Session::new();
    s.run("account.add", &imap_account()).unwrap();
    s.run(
        "account.add",
        &json!({ "email": "sam@example.test", "kind": "pop3", "incoming": { "host": "pop.example.test", "port": 1995 }, "leaveOnServer": false }),
    )
    .unwrap();
    s.run("settings.save", &json!({ "path": path.display().to_string() })).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains(PASSWORD));

    let mut again = Session::new();
    again.run("settings.load", &json!({ "path": path.display().to_string() })).unwrap();
    assert_eq!(again.accounts, s.accounts);

    // A file written by someone else: what makes no sense is left out, the rest is taken.
    let mut all: Value = serde_json::from_str(&text).unwrap();
    let good = all["accounts"][0].clone();
    all["accounts"] = json!([
        good, 7, null, {}, { "id": "account-1", "email": "dup@example.test", "incoming": { "host": "x.test" } },
        { "id": "../../etc", "email": "k@example.test", "incoming": { "host": "x.test" } },
        { "id": "account-5", "email": "k@example.test", "incoming": { "host": "x.test", "port": -1 } },
    ]);
    std::fs::write(&path, all.to_string()).unwrap();
    let mut third = Session::new();
    let loaded = third.run("settings.load", &json!({ "path": path.display().to_string() })).unwrap();
    assert_eq!(third.accounts.len(), 1);
    assert!(loaded["ignored"].as_u64().unwrap() >= 6, "{loaded}");
    let _ = std::fs::remove_dir_all(&dir);
}
