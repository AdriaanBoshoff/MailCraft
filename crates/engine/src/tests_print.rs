//! Printing on a printer, through a printer that keeps what it is sent: nothing reaches paper.

use crate::Session;
use crate::printers::{Printer, Recorder};
use serde_json::{Value, json};
use std::sync::Arc;

fn printers() -> Vec<Printer> {
    vec![Printer { name: "Hall Laser".into(), default: false }, Printer { name: "Desk Inkjet".into(), default: true }]
}

fn session(recorder: &Arc<Recorder>) -> Session {
    let mut s = Session::new();
    s.now = openmailclient_model::time::from_civil(2026, 10, 7, 14, 30, 0, 0);
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.spooler = Arc::new(Arc::clone(recorder));
    s.run("mail.select", &json!({"folder": "Receipts", "index": 0})).unwrap();
    s
}

fn sent(recorder: &Recorder) -> Vec<(String, String, u32, usize, usize, usize, bool)> {
    recorder.sent.lock().unwrap().clone()
}

#[test]
fn printers_are_listed_and_one_is_chosen() {
    let recorder = Arc::new(Recorder { listed: printers(), ..Recorder::default() });
    let mut s = session(&recorder);
    let listed = s.run("file.printers", &Value::Null).unwrap();
    assert_eq!(listed["printers"].as_array().unwrap().len(), 2);
    // None chosen: the default is printed on.
    assert_eq!((&listed["printer"], &listed["chosen"]), (&json!("Desk Inkjet"), &Value::Null));
    let chose = s.run("file.printers", &json!({"choose": "Hall Laser"})).unwrap();
    assert_eq!((&chose["printer"], &chose["chosen"]), (&json!("Hall Laser"), &json!("Hall Laser")));
    for bad in [json!({"choose": "Nowhere"}), json!({"choose": 3})] {
        assert!(s.run("file.printers", &bad).is_err(), "{bad}");
    }
    assert_eq!(s.printer.as_deref(), Some("Hall Laser"));
    let back = s.run("file.printers", &json!({"choose": null})).unwrap();
    assert_eq!(back["printer"], "Desk Inkjet");
    // A session with no printers lists none, and prints on none.
    let mut none = Session::new();
    assert_eq!(none.run("file.printers", &Value::Null).unwrap()["printer"], Value::Null);
    assert!(none.run("file.print", &json!({"printer": "Desk Inkjet"})).is_err());
}

#[test]
fn a_print_goes_to_the_printer_with_its_copies_and_pages() {
    let recorder = Arc::new(Recorder { listed: printers(), ..Recorder::default() });
    let mut s = session(&recorder);
    let done = s.run("file.print", &json!({"printer": "Hall Laser", "copies": 3, "bodyOnly": true})).unwrap();
    assert_eq!((&done["printer"], &done["copies"], &done["pages"]), (&json!("Hall Laser"), &json!(3), &json!(1)), "{done}");
    let jobs = sent(&recorder);
    assert_eq!(jobs.len(), 1);
    let (printer, title, copies, from, to, laid, whole) = jobs[0].clone();
    assert_eq!((printer.as_str(), copies, from, to, laid, whole), ("Hall Laser", 3, 0, 0, 1, true));
    assert!(!title.is_empty());
    // The printer printed on is the one chosen next time.
    assert_eq!(s.printer.as_deref(), Some("Hall Laser"));
    // A page past the last is the last; a first page past the last is refused.
    s.run("file.print", &json!({"printer": "Desk Inkjet", "from": 1, "to": 40})).unwrap();
    assert_eq!(sent(&recorder)[1].3..=sent(&recorder)[1].4, 0..=0);
    for bad in [
        json!({"printer": "Nowhere"}),
        json!({"printer": "Desk Inkjet", "from": 5}),
        json!({"printer": "Desk Inkjet", "from": 0}),
        json!({"printer": "Desk Inkjet", "copies": 0}),
        json!({"printer": "Desk Inkjet", "copies": 100}),
        json!({"printer": 7}),
    ] {
        assert!(s.run("file.print", &bad).is_err(), "{bad}");
    }
    assert_eq!(sent(&recorder).len(), 2, "nothing refused was sent");
}

#[test]
fn a_printer_that_fails_says_so_and_a_print_runs_apart() {
    let recorder = Arc::new(Recorder { listed: printers(), fails: Some("out of paper".into()), ..Recorder::default() });
    let mut s = session(&recorder);
    let failed = s.run("file.print", &json!({"printer": "Desk Inkjet"})).unwrap();
    assert_eq!((&failed["ok"], &failed["error"]), (&json!(false), &json!("out of paper")));
    assert!(s.status.contains("out of paper"), "{}", s.status);

    let recorder = Arc::new(Recorder { listed: printers(), ..Recorder::default() });
    let mut s = session(&recorder);
    let started = s.run("file.print", &json!({"printer": "Desk Inkjet", "wait": false})).unwrap();
    assert_eq!(started["started"], true, "{started}");
    let mut waited = 0;
    while !s.jobs.is_empty() {
        s.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
        assert!(waited < 1000, "the print never ended: {}", s.status);
    }
    assert_eq!(sent(&recorder).len(), 1);
    assert!(s.status.starts_with("Sent 1 page to Desk Inkjet"), "{}", s.status);
}

#[test]
fn the_printer_chosen_is_kept_in_the_settings() {
    let recorder = Arc::new(Recorder { listed: printers(), ..Recorder::default() });
    let mut s = session(&recorder);
    s.run("file.printers", &json!({"choose": "Hall Laser"})).unwrap();
    let dir = std::env::temp_dir().join(format!("openmailclient-printer-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    s.run("settings.save", &json!({"path": path})).unwrap();
    let mut again = session(&recorder);
    again.run("settings.load", &json!({"path": path})).unwrap();
    assert_eq!(again.printer.as_deref(), Some("Hall Laser"));
    std::fs::remove_dir_all(&dir).unwrap();
}
