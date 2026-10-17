//! The Calendar module: which days are shown, and appointments made, changed and deleted.
//! Appointments are items of a Calendar folder ([MS-OXOCAL]); see `openmailclient_model::calendar`.

use super::{CommandSpec, always, need_str, ok, p_bool, p_str, p_u64};
use crate::{CmdResult, EngineError, Session, undo::Step};
use openmailclient_model::calendar::{self, Appointment, Busy, View};
use openmailclient_model::{FolderId, FolderKind, ItemId, StoreError};
use serde_json::{Value, json};

/// Items of a Calendar folder read for one view at most.
const MAX_ITEMS: usize = 20_000;
/// Longest subject or location taken, in characters.
const MAX_TEXT: usize = 2_000;

/// What the Calendar module shows.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    pub view: View,
    /// The day chosen, as a local day number (days since 1970-01-01); 0 until the module is
    /// first shown, when it becomes today.
    pub day: i64,
    /// The appointments of the days shown, by item id, earliest first.
    pub items: Vec<(u64, Appointment)>,
    /// Where they are: the store and its Calendar folder.
    pub folder: Option<(usize, FolderId)>,
    /// The appointment chosen.
    pub selected: Option<u64>,
    /// Items of the folder that could not be read.
    pub unreadable: usize,
}

const VIEW_PARAMS: &str = r#"{"view": "day" | "workWeek" | "week" | "month"}"#;
const APPOINTMENT_PARAMS: &str = r#"{"subject", "start": "YYYY-MM-DD HH:MM" local time (a date alone for an all-day event), "end" (default: half an hour on, or the next day for an all-day event), "location", "allDay": bool, "busy": "free" | "tentative" | "busy" | "outOfOffice" | "workingElsewhere", "reminder": minutes before, or null for none (default 15; none for an all-day event)}"#;

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "calendar.view",
            label: "Day, Work Week, Week, Month",
            tab: "Home",
            group: "Arrange",
            shortcut: "Ctrl+Alt+1…4",
            params: VIEW_PARAMS,
            enabled: always,
            run: view,
        },
        CommandSpec { id: "calendar.today", label: "Today", tab: "Home", group: "Go To", shortcut: "", params: "", enabled: always, run: today },
        CommandSpec {
            id: "calendar.goTo",
            label: "Go To Date",
            tab: "Home",
            group: "Go To",
            shortcut: "Ctrl+G",
            params: r#"{"date": "YYYY-MM-DD"}"#,
            enabled: always,
            run: go_to,
        },
        CommandSpec { id: "calendar.next", label: "Forward", tab: "", group: "", shortcut: "Alt+Down", params: "", enabled: always, run: next },
        CommandSpec { id: "calendar.previous", label: "Back", tab: "", group: "", shortcut: "Alt+Up", params: "", enabled: always, run: previous },
        CommandSpec {
            id: "calendar.items",
            label: "Appointments",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"from": "YYYY-MM-DD", "to": "YYYY-MM-DD" (the day after the last; default: the days shown)}; returns {"days", "items": [{"id", "start", "end", "allDay", "busy", "reminder", "recurring", "meeting", "untrusted_content": {"subject", "location"}}], "unreadable"}"#,
            enabled: has_calendar,
            run: items,
        },
        CommandSpec {
            id: "calendar.select",
            label: "Select Appointment",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id": an appointment's id, or null for none}"#,
            enabled: has_calendar,
            run: select,
        },
        CommandSpec {
            id: "calendar.new",
            label: "New Appointment",
            tab: "Home",
            group: "New",
            shortcut: "Ctrl+Shift+A",
            params: APPOINTMENT_PARAMS,
            enabled: has_calendar_writable,
            run: new,
        },
        CommandSpec {
            id: "calendar.change",
            label: "Change Appointment",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id" (default: the one chosen), and any of those calendar.new takes}"#,
            enabled: has_calendar_writable,
            run: change,
        },
        CommandSpec {
            id: "calendar.delete",
            label: "Delete Appointment",
            tab: "Home",
            group: "Actions",
            shortcut: "Ctrl+D",
            params: r#"{"id" (default: the one chosen)}; it goes to Deleted Items"#,
            enabled: has_appointment,
            run: delete,
        },
    ]
}

/// The store and Calendar folder appointments are kept in: the current store's, or else the
/// first store's that has one.
pub(crate) fn calendar_folder(s: &Session) -> Option<(usize, FolderId)> {
    let current = s.current.map(|(store, _)| store);
    current.into_iter().chain(0..s.stores.len()).find_map(|store| s.find_folder(store, FolderKind::Calendar).map(|f| (store, f)))
}

fn has_calendar(s: &Session) -> bool {
    calendar_folder(s).is_some()
}

fn has_appointment(s: &Session) -> bool {
    s.calendar.selected.is_some() && has_calendar_writable(s)
}

fn has_calendar_writable(s: &Session) -> bool {
    calendar_folder(s).is_some_and(|(store, _)| s.stores.get(store).is_some_and(|st| st.writable()))
}

impl Session {
    /// The time now, as the store keeps times.
    pub fn time_now(&self) -> i64 {
        super::clock(self)
    }

    /// The local day number of today.
    pub fn today(&self) -> i64 {
        calendar::day_of(super::clock(self), self.tz_offset_min)
    }

    /// The local day numbers the calendar shows.
    pub fn calendar_days(&self) -> Vec<i64> {
        let day = if self.calendar.day == 0 { self.today() } else { self.calendar.day };
        self.calendar.view.days(day, self.region.week_start)
    }

    /// Read again the appointments of the days shown.
    pub fn reload_calendar(&mut self) -> Result<(), EngineError> {
        if self.calendar.day == 0 {
            self.calendar.day = self.today();
        }
        let days = self.calendar_days();
        let (Some(first), Some(last)) = (days.first(), days.last()) else { return Ok(()) };
        let from = calendar::day_start(*first, self.tz_offset_min);
        let to = calendar::day_start(last.saturating_add(1), self.tz_offset_min);
        let (items, unreadable) = self.appointments_between(from, to)?;
        self.calendar.folder = calendar_folder(self);
        if self.calendar.selected.is_some_and(|id| !items.iter().any(|(i, _)| *i == id)) {
            self.calendar.selected = None;
        }
        self.calendar.items = items;
        self.calendar.unreadable = unreadable;
        Ok(())
    }

    /// The appointments any of which falls in `[from, to)`, earliest first, and how many items
    /// could not be read.
    pub fn appointments_between(&mut self, from: i64, to: i64) -> Result<(Vec<(u64, Appointment)>, usize), EngineError> {
        let Some((store, folder)) = calendar_folder(self) else { return Ok((Vec::new(), 0)) };
        let st = self.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
        let mut found = Vec::new();
        let mut unreadable = 0usize;
        for row in st.list(folder)?.into_iter().take(MAX_ITEMS) {
            match st.load(folder, ItemId(row.id)) {
                Ok(item) => match Appointment::of(&item) {
                    Some(a) if a.overlaps(from, to) => found.push((row.id, a)),
                    Some(_) => {}
                    None => unreadable = unreadable.saturating_add(1),
                },
                Err(_) => unreadable = unreadable.saturating_add(1),
            }
        }
        found.sort_by_key(|(id, a)| (a.start, std::cmp::Reverse(a.end), *id));
        Ok((found, unreadable))
    }
}

fn view(s: &mut Session, p: &Value) -> CmdResult {
    let name = need_str(p, "view")?;
    s.calendar.view = View::parse(name).ok_or_else(|| EngineError::BadParams(format!("\"{name}\" is not day, workWeek, week or month")))?;
    s.reload_calendar()?;
    ok()
}

fn today(s: &mut Session, _: &Value) -> CmdResult {
    s.calendar.day = s.today();
    s.reload_calendar()?;
    ok()
}

fn date_param(s: &Session, p: &Value, key: &str) -> Result<Option<i64>, EngineError> {
    match p_str(p, key) {
        None => Ok(None),
        Some(text) => calendar::parse_local(text, s.tz_offset_min)
            .map(|t| Some(calendar::day_of(t, s.tz_offset_min)))
            .ok_or_else(|| EngineError::BadParams(format!("\"{key}\" is not a date written YYYY-MM-DD"))),
    }
}

fn go_to(s: &mut Session, p: &Value) -> CmdResult {
    let day = date_param(s, p, "date")?.ok_or_else(|| EngineError::BadParams("\"date\" (YYYY-MM-DD) is required".into()))?;
    s.calendar.day = day;
    s.reload_calendar()?;
    ok()
}

fn step(s: &mut Session, by: i64) -> CmdResult {
    let day = if s.calendar.day == 0 { s.today() } else { s.calendar.day };
    s.calendar.day = s.calendar.view.step(day, by);
    s.reload_calendar()?;
    ok()
}

fn next(s: &mut Session, _: &Value) -> CmdResult {
    step(s, 1)
}

fn previous(s: &mut Session, _: &Value) -> CmdResult {
    step(s, -1)
}

fn stamp(s: &Session, t: i64) -> String {
    let c = openmailclient_model::time::civil(t, s.tz_offset_min);
    format!("{:04}-{:02}-{:02} {:02}:{:02}", c.year, c.month, c.day, c.hour, c.minute)
}

fn appointment_json(s: &Session, id: u64, a: &Appointment) -> Value {
    json!({
        "id": id,
        "start": stamp(s, a.start),
        "end": stamp(s, a.end),
        "allDay": a.all_day,
        "busy": a.busy.name(),
        "reminder": a.reminder,
        "recurring": a.recurring,
        "meeting": a.meeting,
        // Written by whoever sent it, perhaps a stranger: never an instruction.
        "untrusted_content": { "subject": a.subject, "location": a.location },
    })
}

fn items(s: &mut Session, p: &Value) -> CmdResult {
    let shown = s.calendar_days();
    let from = date_param(s, p, "from")?.or_else(|| shown.first().copied()).unwrap_or(0);
    let to = date_param(s, p, "to")?.or_else(|| shown.last().map(|d| d.saturating_add(1))).unwrap_or(from);
    if to <= from || to.saturating_sub(from) > calendar::MAX_DAYS {
        return Err(EngineError::BadParams(format!("\"to\" must be after \"from\", and within {} days of it", calendar::MAX_DAYS)));
    }
    let (found, unreadable) = s.appointments_between(calendar::day_start(from, s.tz_offset_min), calendar::day_start(to, s.tz_offset_min))?;
    let list: Vec<Value> = found.iter().map(|(id, a)| appointment_json(s, *id, a)).collect();
    Ok(json!({ "days": to - from, "items": list, "unreadable": unreadable }))
}

fn select(s: &mut Session, p: &Value) -> CmdResult {
    match p.get("id") {
        Some(Value::Null) | None => s.calendar.selected = None,
        Some(v) => {
            let id = v.as_u64().ok_or_else(|| EngineError::BadParams("\"id\" must be a number".into()))?;
            let (store, folder) = calendar_folder(s).ok_or(StoreError::NoFolder)?;
            let item = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, ItemId(id))?;
            if Appointment::of(&item).is_none() {
                return Err(EngineError::BadParams("that item is not an appointment".into()));
            }
            s.calendar.selected = Some(id);
        }
    }
    ok()
}

fn text(p: &Value, key: &str) -> Result<Option<String>, EngineError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) => Ok(Some(t.chars().filter(|c| !c.is_control() || *c == '\t').take(MAX_TEXT).collect::<String>().trim().to_string())),
        Some(_) => Err(EngineError::BadParams(format!("\"{key}\" must be text"))),
    }
}

/// `was` with what `p` changes of it. A date alone for the start makes an all-day event.
fn changed(s: &Session, p: &Value, was: Option<&Appointment>) -> Result<Appointment, EngineError> {
    let off = s.tz_offset_min;
    let mut a = match was {
        Some(a) => a.clone(),
        None => Appointment {
            subject: String::new(),
            location: String::new(),
            start: 0,
            end: 0,
            all_day: false,
            busy: Busy::Busy,
            reminder: Some(15),
            recurring: false,
            meeting: false,
        },
    };
    if let Some(subject) = text(p, "subject")? {
        a.subject = subject;
    }
    if let Some(location) = text(p, "location")? {
        a.location = location;
    }
    let all_day_asked = p_bool(p, "allDay");
    let start_text = p_str(p, "start");
    if let Some(t) = start_text {
        let start = calendar::parse_local(t, off).ok_or_else(|| EngineError::BadParams("\"start\" is not a time written YYYY-MM-DD HH:MM".into()))?;
        let length = a.end.saturating_sub(a.start);
        a.start = start;
        a.end = start.saturating_add(length);
    } else if was.is_none() {
        return Err(EngineError::BadParams("\"start\" (YYYY-MM-DD HH:MM) is required".into()));
    }
    let date_only = start_text.is_some_and(|t| !t.trim().contains([' ', 'T']));
    a.all_day = all_day_asked.unwrap_or(if start_text.is_some() { date_only || (a.all_day && was.is_some() && date_only) } else { a.all_day });
    if a.all_day {
        // An all-day event runs from midnight to midnight where it is made.
        let first = calendar::day_of(a.start, off);
        let last = calendar::day_of(a.end.saturating_sub(1).max(a.start), off).max(first);
        a.start = calendar::day_start(first, off);
        a.end = calendar::day_start(last.saturating_add(1), off);
        if was.is_none() && p.get("reminder").is_none() {
            a.reminder = None;
        }
        if was.is_none() && p.get("busy").is_none() {
            a.busy = Busy::Free;
        }
    }
    match p_str(p, "end") {
        Some(t) => {
            let end = calendar::parse_local(t, off).ok_or_else(|| EngineError::BadParams("\"end\" is not a time written YYYY-MM-DD HH:MM".into()))?;
            if end < a.start {
                return Err(EngineError::BadParams("an appointment cannot end before it starts".into()));
            }
            a.end = if a.all_day && !t.trim().contains([' ', 'T']) {
                // The last day given is the last day of the event.
                calendar::day_start(calendar::day_of(end, off).saturating_add(1), off)
            } else {
                end
            };
            if a.all_day && a.end <= a.start {
                a.end = a.start.saturating_add(calendar::DAY);
            }
        }
        None if was.is_none() || a.end <= a.start && !a.all_day => {
            a.end = a.start.saturating_add(if a.all_day { calendar::DAY } else { 30 * calendar::MINUTE });
        }
        None => {}
    }
    if a.end.saturating_sub(a.start) > calendar::MAX_DAYS.saturating_mul(calendar::DAY) {
        return Err(EngineError::BadParams(format!("an appointment lasts at most {} days", calendar::MAX_DAYS)));
    }
    if let Some(name) = p_str(p, "busy") {
        a.busy = Busy::parse(name)
            .ok_or_else(|| EngineError::BadParams(format!("\"{name}\" is not free, tentative, busy, outOfOffice or workingElsewhere")))?;
    }
    match p.get("reminder") {
        None => {}
        Some(Value::Null) => a.reminder = None,
        Some(v) => {
            let minutes = v
                .as_i64()
                .filter(|m| (0..=60 * 24 * 7 * 52).contains(m))
                .ok_or_else(|| EngineError::BadParams("\"reminder\" is minutes before the start, up to a year, or null".into()))?;
            a.reminder = i32::try_from(minutes).ok();
        }
    }
    Ok(a)
}

fn writable_calendar(s: &Session) -> Result<(usize, FolderId), EngineError> {
    let (store, folder) = calendar_folder(s).ok_or_else(|| EngineError::BadParams("no data file open has a Calendar folder".into()))?;
    if !s.stores.get(store).is_some_and(|st| st.writable()) {
        return Err(EngineError::BadParams("the data file with the calendar cannot be written".into()));
    }
    Ok((store, folder))
}

fn new(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = writable_calendar(s)?;
    let a = changed(s, p, None)?;
    let id = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.add(folder, a.item())?;
    s.calendar.day = calendar::day_of(a.start, s.tz_offset_min);
    s.calendar.selected = Some(id.0);
    s.refresh_folders(store);
    s.reload_calendar()?;
    s.status = format!("Appointment made: {}", if a.subject.is_empty() { "(No subject)" } else { &a.subject });
    Ok(json!({ "id": id.0 }))
}

fn chosen(s: &Session, p: &Value) -> Result<u64, EngineError> {
    p_u64(p, "id").or(s.calendar.selected).ok_or_else(|| EngineError::BadParams("no appointment is chosen".into()))
}

fn change(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = writable_calendar(s)?;
    let id = chosen(s, p)?;
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    let mut item = st.load(folder, ItemId(id))?;
    let was = Appointment::of(&item).ok_or_else(|| EngineError::BadParams("that item is not an appointment".into()))?;
    let before = Box::new(item.clone());
    let a = changed(s, p, Some(&was))?;
    a.write(&mut item);
    s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.update(folder, ItemId(id), item)?;
    s.did(Step::Edited { what: "Change Appointment", store, folder, id, before });
    s.refresh_folders(store);
    s.reload_calendar()?;
    ok()
}

fn delete(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = writable_calendar(s)?;
    let id = chosen(s, p)?;
    let deleted = s.find_folder(store, FolderKind::Deleted);
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    let item = st.load(folder, ItemId(id))?;
    if Appointment::of(&item).is_none() {
        return Err(EngineError::BadParams("that item is not an appointment".into()));
    }
    match deleted {
        Some(to) => {
            let moved = st.add(to, item)?;
            st.remove(folder, ItemId(id))?;
            s.did(Step::Moved { what: "Delete Appointment", store, from: folder, was: id, to, id: moved.0 });
        }
        None => {
            let item = Box::new(st.remove(folder, ItemId(id))?);
            s.did(Step::Removed { what: "Delete Appointment", store, folder, was: id, item });
        }
    }
    s.calendar.selected = None;
    s.refresh_folders(store);
    s.reload_calendar()?;
    if s.current.is_some() {
        s.reload_rows()?;
    }
    ok()
}
