//! The Calendar module: views, going from day to day, and appointments made, changed, deleted
//! and taken back, in the sample mailbox's calendar.

use crate::{Module, Session};
use openmailclient_model::{FolderKind, MemStore, time};
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

fn subjects(v: &Value) -> Vec<String> {
    v["items"].as_array().unwrap().iter().map(|i| i["untrusted_content"]["subject"].as_str().unwrap().to_string()).collect()
}

#[test]
fn the_calendar_opens_on_this_week_with_the_sample_plans() {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "calendar"}));
    assert_eq!(s.module, Module::Calendar);
    // Today is the system's today; the sample's plans are around the time it was made.
    assert_eq!(s.calendar.day, s.today());
    run(&mut s, "calendar.goTo", json!({"date": "2026-10-07"}));
    let days = s.calendar_days();
    assert_eq!(days.len(), 7);
    // The week of Wednesday 7 October 2026 starts on Sunday the 4th where the week starts on Sunday.
    let first = time::civil(openmailclient_model::calendar::day_start(days[0], 0), 0);
    assert_eq!((first.month, first.day, first.weekday), (10, 4, 0));
    let shown: Vec<&str> = s.calendar.items.iter().map(|(_, a)| a.subject.as_str()).collect();
    for want in ["Team stand-up", "Review the spring catalogue", "Call with the print shop", "Book fair", "Sam's birthday"] {
        assert!(shown.contains(&want), "{want} is not shown: {shown:?}");
    }
    // Earliest first; all-day events start at midnight, so they come before the day's plans.
    let starts: Vec<i64> = s.calendar.items.iter().map(|(_, a)| a.start).collect();
    assert!(starts.windows(2).all(|w| w[0] <= w[1]));

    let today = run(&mut s, "calendar.items", json!({"from": "2026-10-07", "to": "2026-10-08"}));
    assert_eq!(subjects(&today), ["Team stand-up", "Review the spring catalogue", "Call with the print shop"]);
    let review = &today["items"][1];
    assert_eq!(
        (review["start"].as_str(), review["end"].as_str(), review["busy"].as_str()),
        (Some("2026-10-07 14:00"), Some("2026-10-07 15:30"), Some("busy"))
    );
    assert_eq!(today["items"][2]["busy"], "tentative");
}

#[test]
fn views_and_going_from_day_to_day() {
    let mut s = session();
    run(&mut s, "calendar.goTo", json!({"date": "2026-10-07"}));
    run(&mut s, "calendar.view", json!({"view": "day"}));
    assert_eq!(s.calendar_days().len(), 1);
    run(&mut s, "calendar.next", Value::Null);
    let day = time::civil(openmailclient_model::calendar::day_start(s.calendar.day, 0), 0);
    assert_eq!((day.month, day.day), (10, 8));
    assert!(s.calendar.items.iter().any(|(_, a)| a.subject == "Dentist"));

    run(&mut s, "calendar.view", json!({"view": "workWeek"}));
    let days = s.calendar_days();
    assert_eq!(days.len(), 5);
    assert_eq!(time::civil(openmailclient_model::calendar::day_start(days[0], 0), 0).weekday, 1);

    run(&mut s, "calendar.view", json!({"view": "month"}));
    assert_eq!(s.calendar_days().len(), 42);
    run(&mut s, "calendar.next", Value::Null);
    let day = time::civil(openmailclient_model::calendar::day_start(s.calendar.day, 0), 0);
    assert_eq!((day.year, day.month), (2026, 11));
    run(&mut s, "calendar.previous", Value::Null);
    run(&mut s, "calendar.previous", Value::Null);
    let day = time::civil(openmailclient_model::calendar::day_start(s.calendar.day, 0), 0);
    assert_eq!((day.year, day.month), (2026, 9));

    run(&mut s, "calendar.goTo", json!({"date": "2027-02-28"}));
    let day = time::civil(openmailclient_model::calendar::day_start(s.calendar.day, 0), 0);
    assert_eq!((day.year, day.month, day.day), (2027, 2, 28));
    run(&mut s, "calendar.today", Value::Null);
    assert_eq!(s.calendar.day, s.today());

    for bad in [json!({"view": "fortnight"}), json!({}), json!({"view": 3})] {
        assert!(s.run("calendar.view", &bad).is_err());
    }
    for bad in [json!({"date": "2026-13-01"}), json!({"date": "yesterday"}), json!({})] {
        assert!(s.run("calendar.goTo", &bad).is_err());
    }
}

#[test]
fn appointments_are_made_changed_deleted_and_taken_back() {
    let mut s = session();
    s.tz_offset_min = 120;
    run(&mut s, "nav.module", json!({"module": "calendar"}));
    let made = run(&mut s, "calendar.new", json!({"subject": "Design review", "start": "2026-10-09 11:00", "location": "Room 1", "reminder": 10}));
    let id = made["id"].as_u64().unwrap();
    assert_eq!(s.calendar.selected, Some(id));
    let (_, a) = s.calendar.items.iter().find(|(i, _)| *i == id).unwrap().clone();
    // Written in local time, two hours east of UTC; half an hour long when no end is given.
    assert_eq!(a.start, time::from_civil(2026, 10, 9, 9, 0, 0, 0));
    assert_eq!(a.minutes(), 30);
    assert_eq!((a.location.as_str(), a.reminder, a.all_day), ("Room 1", Some(10), false));

    // Moving the start keeps the length; a later end makes it longer.
    run(&mut s, "calendar.change", json!({"start": "2026-10-09 13:00"}));
    run(&mut s, "calendar.change", json!({"id": id, "end": "2026-10-09 14:30", "busy": "outOfOffice", "reminder": null}));
    let got = run(&mut s, "calendar.items", json!({"from": "2026-10-09", "to": "2026-10-10"}));
    let item = got["items"].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap().clone();
    assert_eq!((item["start"].as_str(), item["end"].as_str()), (Some("2026-10-09 13:00"), Some("2026-10-09 14:30")));
    assert_eq!((item["busy"].as_str(), item["reminder"].is_null()), (Some("outOfOffice"), true));
    assert!(s.run("calendar.change", &json!({"id": id, "end": "2026-10-09 12:00"})).is_err(), "it cannot end before it starts");

    // Undone, the change before it is back.
    assert_eq!(run(&mut s, "edit.undo", Value::Null)["undone"], "Change Appointment");
    let (_, a) = s.calendar.items.iter().find(|(i, _)| *i == id).unwrap().clone();
    assert_eq!(a.minutes(), 30);

    // Deleted, it goes to Deleted Items, and comes back when undone.
    run(&mut s, "calendar.delete", json!({"id": id}));
    assert!(!s.calendar.items.iter().any(|(_, a)| a.subject == "Design review"));
    let deleted = s.find_folder(0, FolderKind::Deleted).unwrap();
    assert!(s.stores[0].list(deleted).unwrap().iter().any(|r| r.subject == "Design review"));
    run(&mut s, "edit.undo", Value::Null);
    assert!(s.calendar.items.iter().any(|(_, a)| a.subject == "Design review"));
}

#[test]
fn an_all_day_event_runs_midnight_to_midnight() {
    let mut s = session();
    s.tz_offset_min = -300;
    run(&mut s, "calendar.view", json!({"view": "week"}));
    let id = run(&mut s, "calendar.new", json!({"subject": "Away", "start": "2026-10-12", "end": "2026-10-14"}))["id"].as_u64().unwrap();
    let (_, a) = s.calendar.items.iter().find(|(i, _)| *i == id).unwrap().clone();
    assert!(a.all_day);
    assert_eq!(a.start, time::from_civil(2026, 10, 12, 0, 0, 0, -300));
    // The last day given is the last day of it: three days.
    assert_eq!(a.end, time::from_civil(2026, 10, 15, 0, 0, 0, -300));
    assert_eq!((a.reminder, a.busy.name()), (None, "free"));
}

#[test]
fn no_calendar_and_hostile_appointments_are_errors_not_crashes() {
    let mut s = Session::new();
    s.now = now();
    s.add_store(Box::new(MemStore::with_default_folders("Test")));
    assert!(!s.is_enabled("calendar.new") && !s.is_enabled("calendar.items"));
    run(&mut s, "nav.module", json!({"module": "calendar"}));
    assert!(s.calendar.items.is_empty());
    assert!(s.run("calendar.new", &json!({"subject": "x", "start": "2026-10-09 11:00"})).is_err());

    let mut s = session();
    let long = "x".repeat(100_000);
    for bad in [
        json!({"subject": "no start"}),
        json!({"start": "2026-10-09 25:00"}),
        json!({"start": "2026-10-09 11:00", "end": "9999-12-31 00:00"}),
        json!({"start": "2026-10-09 11:00", "reminder": -5}),
        json!({"start": "2026-10-09 11:00", "reminder": 1e12}),
        json!({"start": "2026-10-09 11:00", "busy": "asleep"}),
        json!({"start": "2026-10-09 11:00", "subject": 7}),
        json!({"start": "\u{0}\u{ffff}"}),
    ] {
        assert!(s.run("calendar.new", &bad).is_err(), "{bad} was taken");
    }
    // A long subject is cut, and control characters are left out.
    let id = run(&mut s, "calendar.new", json!({"subject": format!("a\u{7}b{long}"), "start": "2026-10-09 11:00"}))["id"].as_u64().unwrap();
    let (_, a) = s.calendar.items.iter().find(|(i, _)| *i == id).unwrap().clone();
    assert!(a.subject.starts_with("abx") && a.subject.chars().count() == 2_000);
    assert!(s.run("calendar.items", &json!({"from": "2026-10-09", "to": "2026-10-01"})).is_err());
    assert!(s.run("calendar.items", &json!({"from": "2000-01-01", "to": "2030-01-01"})).is_err());
    assert!(s.run("calendar.select", &json!({"id": 999_999})).is_err());
    assert!(s.run("calendar.delete", &json!({"id": 999_999})).is_err());
}
