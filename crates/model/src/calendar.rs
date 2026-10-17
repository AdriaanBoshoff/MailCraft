//! Appointments: what an item of a Calendar folder says of its time, place and state, read from
//! and written to the properties [MS-OXOCAL] gives them.
//!
//! An appointment's times are named properties of the appointment property set
//! (PidLidAppointmentStartWhole and PidLidAppointmentEndWhole, [MS-OXOCAL] 2.2.1.5 and 2.2.1.6),
//! in UTC. An item that lacks them is read from PidTagStartDate and PidTagEndDate
//! ([MS-OXPROPS] 2.1271 and 2.684), which Outlook writes beside them. What is written here is
//! what [MS-OXOCAL] 3.1.4.1 asks a client to set for an appointment that is not a meeting.

use crate::props::{NameId, NamedKey, PropValue, pid, pset};
use crate::{Item, time};

/// PSETID_Appointment {00062002-0000-0000-C000-000000000046} ([MS-OXPROPS] 1.3.2).
pub const PSETID_APPOINTMENT: [u8; 16] = [0x02, 0x20, 0x06, 0x00, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];

/// The message class of an appointment ([MS-OXOCAL] 2.2.1).
pub const CLASS: &str = "IPM.Appointment";

/// Property ids of the appointment property set ([MS-OXOCAL] 2.2.1).
mod lid {
    pub const BUSY_STATUS: u32 = 0x8205;
    pub const LOCATION: u32 = 0x8208;
    pub const START_WHOLE: u32 = 0x820D;
    pub const END_WHOLE: u32 = 0x820E;
    pub const DURATION: u32 = 0x8213;
    pub const SUB_TYPE: u32 = 0x8215;
    pub const RECUR: u32 = 0x8216;
    pub const STATE_FLAGS: u32 = 0x8217;
    pub const RECURRING: u32 = 0x8223;
    pub const IS_RECURRING: u32 = 0x8E2C;
}

/// Property ids of the common property set ([MS-OXCMSG], [MS-OXORMDR]).
mod common {
    pub const REMINDER_DELTA: u32 = 0x8501;
    pub const REMINDER_TIME: u32 = 0x8502;
    pub const REMINDER_SET: u32 = 0x8503;
    pub const COMMON_START: u32 = 0x8516;
    pub const COMMON_END: u32 = 0x8517;
    pub const REMINDER_SIGNAL_TIME: u32 = 0x8560;
}

/// PidTagStartDate and PidTagEndDate.
const START_DATE: u16 = 0x0060;
const END_DATE: u16 = 0x0061;

/// One minute in FILETIME ticks.
pub const MINUTE: i64 = 60 * 10_000_000;
/// One day in FILETIME ticks.
pub const DAY: i64 = 24 * 60 * MINUTE;
/// An appointment lasts no longer than this many days here; a longer one is cut to it.
pub const MAX_DAYS: i64 = 5 * 366;

/// How the time is shown to others ([MS-OXOCAL] 2.2.1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Busy {
    Free,
    Tentative,
    #[default]
    Busy,
    OutOfOffice,
    WorkingElsewhere,
}

impl Busy {
    pub fn code(self) -> i32 {
        match self {
            Self::Free => 0,
            Self::Tentative => 1,
            Self::Busy => 2,
            Self::OutOfOffice => 3,
            Self::WorkingElsewhere => 4,
        }
    }

    pub fn of(code: i32) -> Self {
        match code {
            0 => Self::Free,
            1 => Self::Tentative,
            3 => Self::OutOfOffice,
            4 => Self::WorkingElsewhere,
            _ => Self::Busy,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Tentative => "tentative",
            Self::Busy => "busy",
            Self::OutOfOffice => "outOfOffice",
            Self::WorkingElsewhere => "workingElsewhere",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Free, Self::Tentative, Self::Busy, Self::OutOfOffice, Self::WorkingElsewhere].into_iter().find(|b| b.name().eq_ignore_ascii_case(s))
    }
}

/// What an appointment says of itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Appointment {
    pub subject: String,
    pub location: String,
    /// UTC FILETIME ticks; `end` is never before `start`.
    pub start: i64,
    pub end: i64,
    /// An all-day event: its times are midnights where it was made ([MS-OXOCAL] 2.2.1.9).
    pub all_day: bool,
    pub busy: Busy,
    /// Minutes before the start a reminder is given, when one is set.
    pub reminder: Option<i32>,
    /// It repeats; only its first time is known here until recurrence patterns are read.
    pub recurring: bool,
    /// A meeting: it has attendees ([MS-OXOCAL] 2.2.1.10, asfMeeting).
    pub meeting: bool,
}

fn key(guid: [u8; 16], id: u32) -> NamedKey {
    NamedKey { guid, name: NameId::Id(id) }
}

fn named(item: &Item, guid: [u8; 16], id: u32) -> Option<&PropValue> {
    let want = key(guid, id);
    item.named.iter().find(|(k, _)| *k == want).map(|(_, v)| v)
}

fn set_named(item: &mut Item, guid: [u8; 16], id: u32, value: PropValue) {
    let want = key(guid, id);
    match item.named.iter_mut().find(|(k, _)| *k == want) {
        Some((_, v)) => *v = value,
        None => item.named.push((want, value)),
    }
}

fn remove_named(item: &mut Item, guid: [u8; 16], id: u32) {
    let want = key(guid, id);
    item.named.retain(|(k, _)| *k != want);
}

fn time_of(v: Option<&PropValue>) -> Option<i64> {
    match v {
        Some(PropValue::Time(t)) => Some(*t),
        _ => None,
    }
}

fn bool_of(v: Option<&PropValue>) -> Option<bool> {
    match v {
        Some(PropValue::Bool(b)) => Some(*b),
        Some(PropValue::Int32(n)) => Some(*n != 0),
        Some(PropValue::Int16(n)) => Some(*n != 0),
        _ => None,
    }
}

fn i32_of(v: Option<&PropValue>) -> Option<i32> {
    match v {
        Some(PropValue::Int32(n)) => Some(*n),
        Some(PropValue::Int16(n)) => Some(i32::from(*n)),
        _ => None,
    }
}

/// Whether an item is an appointment or a meeting, by its class.
pub fn is_appointment(item: &Item) -> bool {
    let class = item.message_class().to_ascii_lowercase();
    class == "ipm.appointment" || class.starts_with("ipm.appointment.")
}

impl Appointment {
    /// What `item` says of itself as an appointment; `None` when it gives no start.
    pub fn of(item: &Item) -> Option<Self> {
        let a = PSETID_APPOINTMENT;
        let start = time_of(named(item, a, lid::START_WHOLE)).or_else(|| item.props.time(START_DATE))?;
        let end = time_of(named(item, a, lid::END_WHOLE))
            .or_else(|| item.props.time(END_DATE))
            .or_else(|| i32_of(named(item, a, lid::DURATION)).map(|m| start.saturating_add(i64::from(m.max(0)).saturating_mul(MINUTE))))
            .unwrap_or(start);
        // A hostile or damaged end before its start, or years after it, is held to it.
        let end = end.clamp(start, start.saturating_add(MAX_DAYS.saturating_mul(DAY)));
        let location = match named(item, a, lid::LOCATION) {
            Some(PropValue::String(s)) => s.clone(),
            _ => String::new(),
        };
        let reminder = (bool_of(named(item, pset::PSETID_COMMON, common::REMINDER_SET)) == Some(true))
            .then(|| i32_of(named(item, pset::PSETID_COMMON, common::REMINDER_DELTA)).unwrap_or(15).clamp(0, 60 * 24 * 7 * 52));
        let recurring = bool_of(named(item, a, lid::RECURRING)).or_else(|| bool_of(named(item, a, lid::IS_RECURRING))).unwrap_or(false);
        let meeting = i32_of(named(item, a, lid::STATE_FLAGS)).is_some_and(|f| f & 1 != 0);
        Some(Self {
            subject: item.subject(),
            location,
            start,
            end,
            all_day: bool_of(named(item, a, lid::SUB_TYPE)).unwrap_or(false),
            busy: Busy::of(i32_of(named(item, a, lid::BUSY_STATUS)).unwrap_or(2)),
            reminder,
            recurring,
            meeting,
        })
    }

    /// Minutes it lasts.
    pub fn minutes(&self) -> i64 {
        self.end.saturating_sub(self.start) / MINUTE
    }

    /// Whether any of it falls in `[from, to)`. One with no length falls where it starts.
    pub fn overlaps(&self, from: i64, to: i64) -> bool {
        self.start < to && (self.end > from || (self.end == self.start && self.start >= from))
    }

    /// A new appointment item saying this. Anything else of `item` (its body, attachments and
    /// properties this program does not know) is kept as it was.
    pub fn write(&self, item: &mut Item) {
        let a = PSETID_APPOINTMENT;
        let c = pset::PSETID_COMMON;
        if !is_appointment(item) {
            item.props.set_str(pid::MESSAGE_CLASS, CLASS);
        }
        item.set_subject(&self.subject);
        item.props.set(START_DATE, PropValue::Time(self.start));
        item.props.set(END_DATE, PropValue::Time(self.end));
        set_named(item, a, lid::START_WHOLE, PropValue::Time(self.start));
        set_named(item, a, lid::END_WHOLE, PropValue::Time(self.end));
        set_named(item, c, common::COMMON_START, PropValue::Time(self.start));
        set_named(item, c, common::COMMON_END, PropValue::Time(self.end));
        set_named(item, a, lid::DURATION, PropValue::Int32(i32::try_from(self.minutes()).unwrap_or(i32::MAX)));
        set_named(item, a, lid::LOCATION, PropValue::String(self.location.clone()));
        set_named(item, a, lid::SUB_TYPE, PropValue::Bool(self.all_day));
        set_named(item, a, lid::BUSY_STATUS, PropValue::Int32(self.busy.code()));
        if named(item, a, lid::STATE_FLAGS).is_none() {
            set_named(item, a, lid::STATE_FLAGS, PropValue::Int32(0));
        }
        if named(item, a, lid::RECUR).is_none() {
            set_named(item, a, lid::RECURRING, PropValue::Bool(false));
        }
        match self.reminder {
            Some(minutes) => {
                let minutes = minutes.clamp(0, 60 * 24 * 7 * 52);
                let at = self.start.saturating_sub(i64::from(minutes).saturating_mul(MINUTE));
                set_named(item, c, common::REMINDER_SET, PropValue::Bool(true));
                set_named(item, c, common::REMINDER_DELTA, PropValue::Int32(minutes));
                set_named(item, c, common::REMINDER_TIME, PropValue::Time(self.start));
                set_named(item, c, common::REMINDER_SIGNAL_TIME, PropValue::Time(at));
            }
            None => {
                set_named(item, c, common::REMINDER_SET, PropValue::Bool(false));
                remove_named(item, c, common::REMINDER_SIGNAL_TIME);
            }
        }
    }

    /// A new appointment item.
    pub fn item(&self) -> Item {
        let mut it = Item::default();
        it.props.set_str(pid::MESSAGE_CLASS, CLASS);
        it.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(crate::MSGFLAG_READ));
        it.props.set(pid::IMPORTANCE, PropValue::Int32(1));
        self.write(&mut it);
        it
    }
}

/// How many days a view shows, and from which.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum View {
    Day,
    /// Monday to Friday, as Outlook's default work week is.
    WorkWeek,
    #[default]
    Week,
    Month,
}

impl View {
    pub fn name(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::WorkWeek => "workWeek",
            Self::Week => "week",
            Self::Month => "month",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Day, Self::WorkWeek, Self::Week, Self::Month].into_iter().find(|v| v.name().eq_ignore_ascii_case(s))
    }

    /// The days shown (local day numbers, days since 1970-01-01) when `day` is the day chosen
    /// and weeks begin on `week_start` (0 = Sunday).
    pub fn days(self, day: i64, week_start: u32) -> Vec<i64> {
        let weekday = |d: i64| (d.rem_euclid(7) + 4).rem_euclid(7) as u32;
        let begun = |d: i64| d.saturating_sub(i64::from((weekday(d) + 7 - week_start % 7) % 7));
        match self {
            Self::Day => vec![day],
            Self::Week => (0..7).map(|i| begun(day).saturating_add(i)).collect(),
            Self::WorkWeek => {
                // Monday to Friday of the week `day` is in, whichever day weeks begin on.
                let monday = day.saturating_sub(i64::from((weekday(day) + 6) % 7));
                (0..5).map(|i| monday.saturating_add(i)).collect()
            }
            Self::Month => {
                let c = time::civil(day_start(day, 0), 0);
                let first = day.saturating_sub(i64::from(c.day.saturating_sub(1)));
                let from = begun(first);
                (0..42).map(|i| from.saturating_add(i)).collect()
            }
        }
    }

    /// The day chosen after moving one view forward (`by` 1) or back (-1).
    pub fn step(self, day: i64, by: i64) -> i64 {
        match self {
            Self::Day => day.saturating_add(by),
            Self::WorkWeek | Self::Week => day.saturating_add(by.saturating_mul(7)),
            Self::Month => {
                let c = time::civil(day_start(day, 0), 0);
                let months = c.year.saturating_mul(12).saturating_add(i64::from(c.month) - 1).saturating_add(by);
                let (year, month) = (months.div_euclid(12), months.rem_euclid(12) as u32 + 1);
                let last = days_in(year, month);
                time::to_unix(time::from_civil(year, month, c.day.min(last), 0, 0, 0, 0)).div_euclid(86_400)
            }
        }
    }
}

/// Days in a month.
pub fn days_in(year: i64, month: u32) -> u32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// UTC FILETIME ticks at the local midnight beginning day number `day`.
pub fn day_start(day: i64, offset_minutes: i32) -> i64 {
    time::from_unix(day.saturating_mul(86_400).saturating_sub(i64::from(offset_minutes).saturating_mul(60)))
}

/// The local day number `filetime` falls on.
pub fn day_of(filetime: i64, offset_minutes: i32) -> i64 {
    time::civil(filetime, offset_minutes).day_number
}

/// A local date and time read from text: `YYYY-MM-DD`, then optionally `HH:MM` after a space or
/// a `T`. `None` for anything else, or a date that does not exist.
pub fn parse_local(text: &str, offset_minutes: i32) -> Option<i64> {
    let text = text.trim();
    let (date, clock) = match text.split_once(['T', ' ']) {
        Some((d, c)) => (d, Some(c.trim())),
        None => (text, None),
    };
    let mut parts = date.splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if !(1601..=9999).contains(&year) || !(1..=12).contains(&month) || day == 0 || day > days_in(year, month) {
        return None;
    }
    let (hour, minute) = match clock {
        Some(c) => {
            let c = c.trim_end_matches('Z');
            let mut hm = c.splitn(3, ':');
            let h: u32 = hm.next()?.parse().ok()?;
            let m: u32 = hm.next()?.parse().ok()?;
            if h > 23 || m > 59 {
                return None;
            }
            (h, m)
        }
        None => (0, 0),
    };
    Some(time::from_civil(year, month, day, hour, minute, 0, offset_minutes))
}

/// Appointments laid side by side where they overlap, as a day of a calendar shows them: for
/// each, the column it stands in and how many columns its group of overlapping ones needs.
/// `spans` are (start, end) pairs; one with no length is given a short one so it can be seen.
pub fn columns(spans: &[(i64, i64)], least: i64) -> Vec<(usize, usize)> {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by_key(|&i| spans.get(i).map(|s| (s.0, std::cmp::Reverse(s.1))));
    let end_of = |i: usize| spans.get(i).map_or(0, |&(s, e)| e.max(s.saturating_add(least)));
    let mut out = vec![(0usize, 1usize); spans.len()];
    // A group goes on while some appointment in it has not yet ended.
    let mut group: Vec<usize> = Vec::new();
    let mut ends: Vec<i64> = Vec::new();
    let mut group_end = i64::MIN;
    let close = |group: &mut Vec<usize>, ends: &mut Vec<i64>, out: &mut Vec<(usize, usize)>| {
        let n = ends.len().max(1);
        for &i in group.iter() {
            if let Some(slot) = out.get_mut(i) {
                slot.1 = n;
            }
        }
        group.clear();
        ends.clear();
    };
    for i in order {
        let start = spans.get(i).map_or(0, |s| s.0);
        if start >= group_end && !group.is_empty() {
            close(&mut group, &mut ends, &mut out);
        }
        let column = match ends.iter().position(|&e| e <= start) {
            Some(c) => c,
            None => {
                ends.push(i64::MIN);
                ends.len() - 1
            }
        };
        if let Some(e) = ends.get_mut(column) {
            *e = end_of(i);
        }
        if let Some(slot) = out.get_mut(i) {
            slot.0 = column;
        }
        group.push(i);
        group_end = if group.len() == 1 { end_of(i) } else { group_end.max(end_of(i)) };
    }
    close(&mut group, &mut ends, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> i64 {
        parse_local(text, 0).unwrap()
    }

    #[test]
    fn an_appointment_is_written_and_read_back_as_mapi_lays_it_out() {
        let a = Appointment {
            subject: "Dentist".into(),
            location: "12 High Street".into(),
            start: at("2026-10-20 09:30"),
            end: at("2026-10-20 10:15"),
            all_day: false,
            busy: Busy::OutOfOffice,
            reminder: Some(30),
            recurring: false,
            meeting: false,
        };
        let item = a.item();
        assert!(is_appointment(&item));
        assert_eq!(item.props.time(START_DATE), Some(a.start));
        assert_eq!(named(&item, PSETID_APPOINTMENT, lid::DURATION), Some(&PropValue::Int32(45)));
        assert_eq!(Appointment::of(&item), Some(a.clone()));
        // Written again, nothing is doubled.
        let mut again = item.clone();
        a.write(&mut again);
        assert_eq!(again.named.len(), item.named.len());
        let mut no_reminder = a.clone();
        no_reminder.reminder = None;
        no_reminder.write(&mut again);
        assert_eq!(Appointment::of(&again).unwrap().reminder, None);
    }

    #[test]
    fn an_item_with_only_the_plain_dates_or_odd_values_is_read_safely() {
        let mut it = Item::default();
        it.props.set_str(pid::MESSAGE_CLASS, "IPM.Appointment");
        assert!(Appointment::of(&it).is_none());
        it.props.set(START_DATE, PropValue::Time(at("2026-01-01 10:00")));
        it.props.set(END_DATE, PropValue::Time(at("2025-01-01 10:00")));
        let a = Appointment::of(&it).unwrap();
        assert_eq!(a.end, a.start, "an end before the start is held to it");
        it.props.set(END_DATE, PropValue::Time(i64::MAX));
        assert_eq!(Appointment::of(&it).unwrap().end, a.start + MAX_DAYS * DAY);
        set_named(&mut it, PSETID_APPOINTMENT, lid::BUSY_STATUS, PropValue::Int32(99));
        set_named(&mut it, PSETID_APPOINTMENT, lid::LOCATION, PropValue::Int32(5));
        let a = Appointment::of(&it).unwrap();
        assert_eq!((a.busy, a.location.as_str()), (Busy::Busy, ""));
    }

    #[test]
    fn views_show_the_days_outlook_shows() {
        let day = |t: &str| day_of(at(t), 0);
        let wed = day("2026-10-21");
        assert_eq!(View::Day.days(wed, 0), vec![wed]);
        assert_eq!(View::Week.days(wed, 0).first(), Some(&day("2026-10-18")), "a week from Sunday");
        assert_eq!(View::Week.days(wed, 1).first(), Some(&day("2026-10-19")), "or from Monday");
        assert_eq!(View::WorkWeek.days(day("2026-10-18"), 0), (0..5).map(|i| day("2026-10-12") + i).collect::<Vec<_>>());
        let month = View::Month.days(wed, 0);
        assert_eq!((month.len(), month[0]), (42, day("2026-09-27")));
        assert_eq!(View::Month.step(day("2026-01-31"), 1), day("2026-02-28"));
        assert_eq!(View::Month.step(day("2026-01-15"), -1), day("2025-12-15"));
        assert_eq!(View::Week.step(wed, -1), wed - 7);
    }

    #[test]
    fn local_times_are_read_and_nonsense_is_not() {
        assert_eq!(parse_local("2026-10-20T09:30", 120), Some(time::from_civil(2026, 10, 20, 7, 30, 0, 0)));
        for bad in ["", "2026-13-01", "2026-02-30", "2026-10-20 24:00", "20261020", "2026-10-20 9", "x-y-z", "99999-01-01"] {
            assert!(parse_local(bad, 0).is_none(), "{bad}");
        }
        assert!(parse_local("2028-02-29", 0).is_some());
    }

    #[test]
    fn overlapping_appointments_stand_side_by_side() {
        let h = |n: i64| n * 60 * MINUTE;
        let spans = [(h(9), h(11)), (h(10), h(12)), (h(10), h(10)), (h(13), h(14)), (h(11), h(12))];
        let cols = columns(&spans, h(1) / 2);
        assert_eq!(cols[0], (0, 3));
        assert_eq!(cols[1].1, 3);
        assert_ne!(cols[1].0, cols[2].0);
        assert_eq!(cols[4], (0, 3), "the first column is free again at 11");
        assert_eq!(cols[3], (0, 1));
        assert!(columns(&[], 1).is_empty());
    }
}
