//! The Calendar module: a month to choose days from, the days shown hour by hour or as a month,
//! and the window an appointment is made or changed in. What is shown is the engine's
//! (`Session::calendar`); every change goes through the `calendar.*` commands.

use crate::icons::{self, Icon};
use crate::line_edit::LineEdit;
use crate::{MailUi, bold, clip_text, font, one_line, text_width};
use egui::{Align2, Color32, Painter, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use openmailclient_model::calendar::{self, Appointment, Busy, DAY, MINUTE, View};
use openmailclient_model::time::{self, Civil};
use serde_json::{Value, json};

/// The height of an hour in the day views; half hours are marked between.
const HOUR_H: f32 = 44.0;
/// The column of hours at the left of the day views.
const GUTTER: f32 = 58.0;
/// The row naming each day.
const DAY_HEAD: f32 = 44.0;
/// One row of the band of all-day events.
const BAND_ROW: f32 = 22.0;
/// All-day events shown above the hours before the rest are counted.
const BAND_ROWS: usize = 4;
/// The hours the working day is shaded for, and the first shown.
const WORK: (u32, u32) = (8, 17);
/// One line of an appointment in a month's day.
const MONTH_LINE: f32 = 18.0;

/// The date a local day number is.
fn date_of(day: i64) -> Civil {
    time::civil(calendar::day_start(day, 0), 0)
}

/// Short weekday names, Sunday first.
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// An appointment being made or changed, as typed.
#[derive(Clone, Debug)]
pub(crate) struct AppointmentForm {
    pub(crate) id: Option<u64>,
    pub(crate) subject: String,
    pub(crate) location: String,
    pub(crate) start_date: String,
    pub(crate) start_time: String,
    pub(crate) end_date: String,
    pub(crate) end_time: String,
    pub(crate) all_day: bool,
    pub(crate) busy: Busy,
    pub(crate) reminder: Option<i32>,
    /// What went wrong with the last Save.
    pub(crate) said: Option<String>,
    pub(crate) focus: bool,
}

const REMINDERS: &[(Option<i32>, &str)] = &[
    (None, "None"),
    (Some(0), "0 minutes"),
    (Some(5), "5 minutes"),
    (Some(10), "10 minutes"),
    (Some(15), "15 minutes"),
    (Some(30), "30 minutes"),
    (Some(60), "1 hour"),
    (Some(120), "2 hours"),
    (Some(1440), "1 day"),
    (Some(10080), "1 week"),
];

const SHOW_AS: &[(Busy, &str)] = &[
    (Busy::Free, "Free"),
    (Busy::WorkingElsewhere, "Working Elsewhere"),
    (Busy::Tentative, "Tentative"),
    (Busy::Busy, "Busy"),
    (Busy::OutOfOffice, "Out of Office"),
];

fn show_as(b: Busy) -> &'static str {
    SHOW_AS.iter().find(|(x, _)| *x == b).map_or("Busy", |(_, n)| n)
}

fn reminder_words(r: Option<i32>) -> String {
    match REMINDERS.iter().find(|(x, _)| *x == r) {
        Some((_, n)) => (*n).to_string(),
        None => format!("{} minutes", r.unwrap_or(0)),
    }
}

fn ymd(c: &Civil) -> String {
    format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
}

fn hm(c: &Civil) -> String {
    format!("{:02}:{:02}", c.hour, c.minute)
}

/// How long, in words: "30 minutes", "1 hour 30 minutes", "3 days".
fn length_words(minutes: i64) -> String {
    let (d, h, m) = (minutes / 1440, minutes % 1440 / 60, minutes % 60);
    let part = |n: i64, one: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") };
    let mut out: Vec<String> = Vec::new();
    if d > 0 {
        out.push(part(d, "day"));
    }
    if h > 0 {
        out.push(part(h, "hour"));
    }
    if m > 0 || out.is_empty() {
        out.push(part(m, "minute"));
    }
    out.join(" ")
}

impl AppointmentForm {
    /// A new appointment from `start` (FILETIME), half an hour long, or all day.
    fn new_at(start: i64, all_day: bool, offset: i32) -> Self {
        let end = if all_day { start.saturating_add(DAY) } else { start.saturating_add(30 * MINUTE) };
        let a = Appointment {
            subject: String::new(),
            location: String::new(),
            start,
            end,
            all_day,
            busy: if all_day { Busy::Free } else { Busy::Busy },
            reminder: if all_day { None } else { Some(15) },
            recurring: false,
            meeting: false,
        };
        Self::of(None, &a, offset)
    }

    fn of(id: Option<u64>, a: &Appointment, offset: i32) -> Self {
        let s = time::civil(a.start, offset);
        // An all-day event's last day is the one before the midnight it ends at.
        let e = time::civil(if a.all_day { a.end.saturating_sub(DAY).max(a.start) } else { a.end }, offset);
        Self {
            id,
            subject: one_line(&a.subject),
            location: one_line(&a.location),
            start_date: ymd(&s),
            start_time: hm(&s),
            end_date: ymd(&e),
            end_time: hm(&e),
            all_day: a.all_day,
            busy: a.busy,
            reminder: a.reminder,
            said: None,
            focus: true,
        }
    }

    /// The parameters `calendar.new` and `calendar.change` take.
    pub(crate) fn params(&self) -> Value {
        let at = |d: &str, t: &str| if self.all_day { d.trim().to_string() } else { format!("{} {}", d.trim(), t.trim()) };
        let mut p = json!({
            "subject": self.subject,
            "location": self.location,
            "start": at(&self.start_date, &self.start_time),
            "end": at(&self.end_date, &self.end_time),
            "allDay": self.all_day,
            "busy": self.busy.name(),
            "reminder": self.reminder,
        });
        if let Some(id) = self.id {
            p["id"] = json!(id);
        }
        p
    }

    /// Start and end as typed, when both read as times.
    fn span(&self, offset: i32) -> Option<(i64, i64)> {
        let start =
            calendar::parse_local(&format!("{} {}", self.start_date.trim(), if self.all_day { "00:00" } else { self.start_time.trim() }), offset)?;
        let end = if self.all_day {
            calendar::parse_local(self.end_date.trim(), offset)?.saturating_add(DAY)
        } else {
            calendar::parse_local(&format!("{} {}", self.end_date.trim(), self.end_time.trim()), offset)?
        };
        Some((start, end))
    }
}

/// What a click in the appointment window asked for.
enum Ask {
    Save,
    Delete,
    Close,
}

impl MailUi {
    /// The Calendar module's whole body.
    pub(crate) fn calendar_body(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        ui.painter_at(rect).rect_filled(rect, 0.0, t.surface);
        let mut left = rect.left();
        if self.session.folder_pane {
            let w = self.folder_w.clamp(200.0, 280.0).min(rect.width() * 0.4);
            self.calendar_side(ui, Rect::from_min_size(rect.min, vec2(w, rect.height())));
            left += w;
            ui.painter_at(rect).vline(left + 0.5, rect.y_range(), (1.0, t.line));
        }
        let area = Rect::from_min_max(pos2(left + 1.0, rect.top()), rect.max);
        let head = Rect::from_min_size(area.min, vec2(area.width(), 40.0));
        self.calendar_head(ui, head);
        let days = self.session.calendar_days();
        let grid = Rect::from_min_max(pos2(area.left(), head.bottom()), area.max);
        if self.session.calendar.view == View::Month {
            self.month_grid(ui, grid, &days);
        } else {
            self.time_grid(ui, grid, &days);
        }
    }

    /// The month to choose a day from, and the calendars there are.
    fn calendar_side(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.pane);
        let s = &self.session;
        let today = s.today();
        let chosen = if s.calendar.day == 0 { today } else { s.calendar.day };
        let shown = s.calendar_days();
        let (year, month) = self.cal_nav.unwrap_or_else(|| {
            let c = date_of(chosen);
            (c.year, c.month)
        });
        let first = calendar::day_of(time::from_civil(year, month, 1, 0, 0, 0, 0), 0);
        let grid = View::Month.days(first, s.region.week_start);

        let top = rect.top() + 12.0;
        let title = format!("{} {}", date_of(first).month_name(), year);
        p.text(pos2(rect.center().x, top + 10.0), Align2::CENTER_CENTER, &title, bold(13.0), t.text);
        for (n, x) in [(-1i32, rect.left() + 22.0), (1, rect.right() - 22.0)] {
            let r = Rect::from_center_size(pos2(x, top + 10.0), vec2(22.0, 22.0));
            let resp = ui.interact(r, egui::Id::new(("cal-nav-month", n)), Sense::click());
            if resp.hovered() {
                p.rect_filled(r, 3.0, t.hover);
            }
            chevron(&p, r.center(), n < 0, t.text_dim);
            if resp.clicked() {
                let (mut y, mut m) = (year, i64::from(month) + i64::from(n));
                if m < 1 {
                    (y, m) = (y - 1, 12);
                } else if m > 12 {
                    (y, m) = (y + 1, 1);
                }
                self.cal_nav = Some((y.clamp(1601, 9999), u32::try_from(m).unwrap_or(1)));
            }
        }
        let cell = ((rect.width() - 20.0) / 7.0).min(30.0);
        let x0 = rect.center().x - cell * 3.5;
        let y0 = top + 30.0;
        let week_start = usize::try_from(s.region.week_start % 7).unwrap_or(0);
        for i in 0..7 {
            let name = WEEKDAYS.get((week_start + i) % 7).copied().unwrap_or("");
            let letter: String = name.chars().take(2).collect();
            p.text(pos2(x0 + cell * (i as f32 + 0.5), y0 + 8.0), Align2::CENTER_CENTER, letter, font(11.0), t.text_faint);
        }
        let mut go: Option<i64> = None;
        for (i, day) in grid.iter().enumerate() {
            let (row, col) = (i / 7, i % 7);
            let r = Rect::from_min_size(pos2(x0 + cell * col as f32, y0 + 18.0 + cell * row as f32), vec2(cell, cell));
            let c = date_of(*day);
            let in_month = c.month == month;
            if shown.contains(day) {
                p.rect_filled(r.shrink(1.0), 2.0, t.selected);
            }
            let resp = ui.interact(r, egui::Id::new(("cal-nav-day", *day)), Sense::click());
            if resp.hovered() {
                p.rect_filled(r.shrink(1.0), 2.0, t.hover);
            }
            let color = if *day == today {
                p.circle_filled(r.center(), cell * 0.42, t.accent);
                t.on_accent
            } else if in_month {
                t.text
            } else {
                t.text_faint
            };
            p.text(r.center(), Align2::CENTER_CENTER, c.day.to_string(), if *day == today { bold(11.5) } else { font(11.5) }, color);
            if resp.clicked() {
                go = Some(*day);
            }
        }
        if let Some(day) = go {
            self.cal_nav = None;
            self.go_to_day(day);
        }

        // The calendars there are: for now, the one the appointments are in.
        let y = y0 + 18.0 + cell * 6.0 + 22.0;
        p.text(pos2(rect.left() + 14.0, y), Align2::LEFT_CENTER, "My Calendars", bold(12.5), t.text);
        let name = self
            .session
            .calendar
            .folder
            .and_then(|(store, folder)| {
                let f = self.session.folders.get(store)?.iter().find(|f| f.id == folder)?;
                let st = self.session.stores.get(store)?;
                Some(format!("{} - {}", one_line(&f.name), one_line(&st.display_name())))
            })
            .unwrap_or_else(|| "No calendar in the data files open".to_string());
        let r = Rect::from_min_size(pos2(rect.left() + 26.0, y + 14.0), vec2(12.0, 12.0));
        p.rect_filled(r, 2.0, t.accent);
        icons::paint(&p, r.shrink(1.5), Icon::Check, t.on_accent);
        clip_text(&p, pos2(r.right() + 8.0, r.top() - 1.0), rect.right() - r.right() - 18.0, &name, font(12.0), t.text_dim);
        if self.session.calendar.unreadable > 0 {
            let words = format!("{} items could not be read", self.session.calendar.unreadable);
            clip_text(&p, pos2(rect.left() + 26.0, y + 34.0), rect.width() - 36.0, &words, font(11.0), t.danger);
        }
    }

    fn go_to_day(&mut self, day: i64) {
        let c = date_of(day);
        self.act("calendar.goTo", json!({ "date": ymd(&c) }));
        self.cal_scroll = true;
    }

    /// Back and forward, and which days these are.
    fn calendar_head(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.hline(rect.x_range(), rect.bottom() - 0.5, (1.0, t.line));
        let mut x = rect.left() + 12.0;
        for (back, cmd) in [(true, "calendar.previous"), (false, "calendar.next")] {
            let r = Rect::from_min_size(pos2(x, rect.center().y - 13.0), vec2(26.0, 26.0));
            let resp = ui.interact(r, egui::Id::new(("cal-step", back)), Sense::click());
            if resp.hovered() {
                p.rect_filled(r, 4.0, t.hover);
            }
            chevron(&p, r.center(), back, t.text);
            if resp.clicked() {
                self.act(cmd, Value::Null);
            }
            resp.on_hover_text(if back { "Back (Alt+Up)" } else { "Forward (Alt+Down)" });
            x += 28.0;
        }
        let title = self.range_title();
        p.text(pos2(x + 10.0, rect.center().y), Align2::LEFT_CENTER, title, font(18.0), t.text);
    }

    /// The days shown, in words: "October 4 - 10, 2026", "October 2026".
    fn range_title(&self) -> String {
        let s = &self.session;
        let days = s.calendar_days();
        let (Some(first), Some(last)) = (days.first().copied(), days.last().copied()) else { return String::new() };
        let (a, b) = (date_of(first), date_of(last));
        match s.calendar.view {
            View::Day => s.region.long_date(&a),
            View::Month => {
                let c = date_of(if s.calendar.day == 0 { s.today() } else { s.calendar.day });
                format!("{} {}", c.month_name(), c.year)
            }
            _ if a.year != b.year => format!("{} {}, {} - {} {}, {}", a.month_name(), a.day, a.year, b.month_name(), b.day, b.year),
            _ if a.month != b.month => format!("{} {} - {} {}, {}", a.month_name(), a.day, b.month_name(), b.day, b.year),
            _ => format!("{} {} - {}, {}", a.month_name(), a.day, b.day, b.year),
        }
    }

    /// An hour as the gutter names it: "8 AM", or "08:00" where times are written on 24 hours.
    fn hour_name(&self, hour: u32) -> String {
        let r = &self.session.region;
        if r.short_time.contains('h') {
            let h = if hour.is_multiple_of(12) { 12 } else { hour % 12 };
            format!("{h} {}", if hour < 12 { &r.am } else { &r.pm })
        } else {
            format!("{hour:02}:00")
        }
    }

    /// The day, work week and week views: a column of hours for each day, all-day events above.
    fn time_grid(&mut self, ui: &mut Ui, rect: Rect, days: &[i64]) {
        let t = self.tokens.clone();
        let off = self.session.tz_offset_min;
        let today = self.session.today();
        let n = days.len().max(1);
        let col_w = ((rect.width() - GUTTER - 14.0) / n as f32).max(20.0);
        let col_x = |i: usize| rect.left() + GUTTER + col_w * i as f32;
        let items = self.session.calendar.items.clone();
        let selected = self.session.calendar.selected;
        let first = days.first().copied().unwrap_or(today);
        let last = days.last().copied().unwrap_or(first);
        let shown_from = calendar::day_start(first, off);
        let shown_to = calendar::day_start(last.saturating_add(1), off);

        // The band: all-day events and anything a day or longer, each in the first free row.
        let banded: Vec<&(u64, Appointment)> = items.iter().filter(|(_, a)| a.all_day || a.end.saturating_sub(a.start) >= DAY).collect();
        let mut rows: Vec<Vec<(usize, usize)>> = Vec::new();
        let mut placed: Vec<(usize, usize, usize, &(u64, Appointment))> = Vec::new();
        let mut hidden = vec![0usize; n];
        for item in &banded {
            let a = &item.1;
            let from = calendar::day_of(a.start.max(shown_from), off);
            let to = calendar::day_of(a.end.min(shown_to).saturating_sub(1).max(a.start.max(shown_from)), off);
            let (Some(c0), Some(c1)) = (days.iter().position(|d| *d == from), days.iter().position(|d| *d == to).or(Some(n - 1))) else {
                continue;
            };
            let row = rows.iter().position(|r| r.iter().all(|&(s, e)| c1 < s || c0 > e)).unwrap_or(rows.len());
            if row >= BAND_ROWS {
                for h in hidden.iter_mut().take(c1 + 1).skip(c0) {
                    *h += 1;
                }
                continue;
            }
            if row == rows.len() {
                rows.push(Vec::new());
            }
            if let Some(r) = rows.get_mut(row) {
                r.push((c0, c1));
            }
            placed.push((row, c0, c1, item));
        }
        let band_h = BAND_ROW * rows.len().max(1) as f32 + 6.0 + if hidden.iter().any(|h| *h > 0) { 14.0 } else { 0.0 };

        let p = ui.painter_at(rect);
        // The days' names.
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), DAY_HEAD));
        for (i, day) in days.iter().enumerate() {
            let c = date_of(*day);
            let x = col_x(i);
            let is_today = *day == today;
            let name = WEEKDAYS.get(usize::try_from(c.weekday).unwrap_or(0)).copied().unwrap_or("");
            p.text(pos2(x + 8.0, head.top() + 12.0), Align2::LEFT_CENTER, name, font(11.5), if is_today { t.accent } else { t.text_dim });
            p.text(
                pos2(x + 8.0, head.top() + 30.0),
                Align2::LEFT_CENTER,
                c.day.to_string(),
                if is_today { bold(17.0) } else { font(17.0) },
                if is_today { t.accent } else { t.text },
            );
            if is_today {
                p.rect_filled(Rect::from_min_size(pos2(x + 1.0, head.bottom() - 3.0), vec2(col_w - 2.0, 3.0)), 1.0, t.accent);
            }
            p.vline(x + 0.5, head.top() + 6.0..=head.bottom() + band_h, (1.0, t.line));
            let resp =
                ui.interact(Rect::from_min_size(pos2(x, head.top()), vec2(col_w, DAY_HEAD)), egui::Id::new(("cal-day-head", *day)), Sense::click());
            if resp.clicked() && self.session.calendar.view != View::Day {
                // A day's name opens that day alone.
                self.act("calendar.view", json!({ "view": "day" }));
                self.go_to_day(*day);
            }
        }
        let band = Rect::from_min_size(pos2(rect.left(), head.bottom()), vec2(rect.width(), band_h));
        p.hline(rect.x_range(), band.top() + 0.5, (1.0, t.line));
        p.hline(rect.x_range(), band.bottom() - 0.5, (1.0, t.line));
        let mut band_open: Option<u64> = None;
        let mut band_select: Option<u64> = None;
        for (row, c0, c1, (id, a)) in &placed {
            let r = Rect::from_min_max(
                pos2(col_x(*c0) + 3.0, band.top() + 3.0 + BAND_ROW * *row as f32),
                pos2(col_x(*c1 + 1) - 3.0, band.top() + 1.0 + BAND_ROW * (*row as f32 + 1.0)),
            );
            let resp = self.appointment_box(ui, &p, r, *id, a, selected == Some(*id), "band");
            if resp.double_clicked() {
                band_open = Some(*id);
            } else if resp.clicked() {
                band_select = Some(*id);
            }
        }
        for (i, h) in hidden.iter().enumerate() {
            if *h > 0 {
                p.text(pos2(col_x(i) + 6.0, band.bottom() - 9.0), Align2::LEFT_CENTER, format!("{h} more"), font(10.5), t.text_dim);
            }
        }

        // The hours.
        let hours = Rect::from_min_max(pos2(rect.left(), band.bottom()), rect.max);
        let scroll_to = std::mem::take(&mut self.cal_scroll);
        let mut new_at: Option<i64> = None;
        let mut clicked_empty = false;
        let mut open: Option<u64> = None;
        let mut select: Option<u64> = None;
        ui.scope_builder(UiBuilder::new().max_rect(hours), |ui| {
            ScrollArea::vertical().id_salt("cal-hours").auto_shrink(false).show(ui, |ui| {
                let (canvas, resp) = ui.allocate_exact_size(vec2(hours.width().max(0.0), HOUR_H * 24.0), Sense::click());
                let p = ui.painter_at(canvas.intersect(ui.clip_rect()));
                for (i, day) in days.iter().enumerate() {
                    let c = date_of(*day);
                    let x = canvas.left() + GUTTER + col_w * i as f32;
                    let workday = (1..=5).contains(&c.weekday);
                    let work =
                        Rect::from_min_max(pos2(x, canvas.top() + HOUR_H * WORK.0 as f32), pos2(x + col_w, canvas.top() + HOUR_H * WORK.1 as f32));
                    p.rect_filled(Rect::from_min_size(pos2(x, canvas.top()), vec2(col_w, canvas.height())), 0.0, t.pane);
                    if workday {
                        p.rect_filled(work, 0.0, t.surface);
                    }
                    p.vline(x + 0.5, canvas.y_range(), (1.0, t.line));
                }
                for hour in 0..24u32 {
                    let y = canvas.top() + HOUR_H * hour as f32;
                    p.hline(canvas.left() + GUTTER..=canvas.right(), y + 0.5, (1.0, t.line));
                    p.hline(canvas.left() + GUTTER..=canvas.right(), y + HOUR_H / 2.0 + 0.5, (1.0, t.line.gamma_multiply(0.45)));
                    if hour > 0 {
                        p.text(pos2(canvas.left() + GUTTER - 8.0, y), Align2::RIGHT_CENTER, self.hour_name(hour), font(10.5), t.text_dim);
                    }
                }
                if scroll_to {
                    let y = canvas.top() + HOUR_H * (WORK.0 as f32 - 0.5);
                    ui.scroll_to_rect(Rect::from_min_size(pos2(canvas.left(), y), vec2(1.0, HOUR_H)), Some(egui::Align::TOP));
                }
                // Each day's appointments, those at the same time side by side.
                for (i, day) in days.iter().enumerate() {
                    let from = calendar::day_start(*day, off);
                    let to = from.saturating_add(DAY);
                    let timed: Vec<&(u64, Appointment)> =
                        items.iter().filter(|(_, a)| !(a.all_day || a.end.saturating_sub(a.start) >= DAY) && a.overlaps(from, to)).collect();
                    let spans: Vec<(i64, i64)> = timed.iter().map(|(_, a)| (a.start.max(from), a.end.min(to))).collect();
                    let cols = calendar::columns(&spans, 15 * MINUTE);
                    let x = canvas.left() + GUTTER + col_w * i as f32;
                    for ((item, (s, e)), (col, of)) in timed.iter().zip(&spans).zip(&cols) {
                        let (id, a) = (item.0, &item.1);
                        let y0 = canvas.top() + (s.saturating_sub(from) / MINUTE) as f32 / 60.0 * HOUR_H;
                        let y1 = (canvas.top() + (e.saturating_sub(from) / MINUTE) as f32 / 60.0 * HOUR_H).max(y0 + 18.0);
                        let w = (col_w - 8.0) / (*of).max(1) as f32;
                        let r =
                            Rect::from_min_max(pos2(x + 2.0 + w * *col as f32, y0 + 1.0), pos2(x + 2.0 + w * (*col as f32 + 1.0) - 2.0, y1 - 1.0));
                        let resp = self.appointment_box(ui, &p, r, id, a, selected == Some(id), "hours");
                        if resp.double_clicked() {
                            open = Some(id);
                        } else if resp.clicked() {
                            select = Some(id);
                        }
                    }
                    // Now, on today.
                    if *day == today {
                        let now = self.session.time_now();
                        let y = canvas.top() + (now.saturating_sub(from) / MINUTE) as f32 / 60.0 * HOUR_H;
                        p.hline(x..=x + col_w, y, Stroke::new(1.5, t.danger));
                        p.circle_filled(pos2(x, y), 4.0, t.danger);
                    }
                }
                // A double click where nothing is makes an appointment at that half hour.
                if let Some(at) = resp.interact_pointer_pos().filter(|_| resp.double_clicked() || resp.clicked()) {
                    let i = ((at.x - canvas.left() - GUTTER) / col_w).floor();
                    let half = ((at.y - canvas.top()) / (HOUR_H / 2.0)).floor().clamp(0.0, 47.0) as i64;
                    if i >= 0.0
                        && let Some(day) = days.get(i as usize)
                    {
                        if resp.double_clicked() {
                            new_at = Some(calendar::day_start(*day, off).saturating_add(half * 30 * MINUTE));
                        } else {
                            clicked_empty = true;
                        }
                    }
                }
            });
        });
        if let Some(id) = open.or(band_open) {
            self.open_appointment(id);
        } else if let Some(id) = select.or(band_select) {
            self.act("calendar.select", json!({ "id": id }));
        } else if let Some(start) = new_at {
            self.appointment = Some(AppointmentForm::new_at(start, false, off));
        } else if clicked_empty {
            self.act("calendar.select", json!({ "id": null }));
        }
    }

    /// One appointment: its time, coloured as it is shown as, with its subject and place.
    #[allow(clippy::too_many_arguments)]
    fn appointment_box(&self, ui: &mut Ui, p: &Painter, r: Rect, id: u64, a: &Appointment, chosen: bool, place: &str) -> egui::Response {
        let t = &self.tokens;
        let (fill, bar) = match a.busy {
            Busy::OutOfOffice => (t.away.gamma_multiply(0.22), t.away),
            Busy::Free => (t.surface, t.accent),
            Busy::Tentative => (t.surface, t.accent),
            _ => (t.accent.gamma_multiply(0.2), t.accent),
        };
        // Opaque first, so the hour lines do not show through.
        p.rect_filled(r, 3.0, t.surface);
        p.rect_filled(r, 3.0, fill);
        let bar_r = Rect::from_min_size(r.min, vec2(4.0, r.height()));
        match a.busy {
            // Tentative time is striped, free time an outline, time elsewhere dotted.
            Busy::Tentative => {
                let mut y = bar_r.top();
                while y < bar_r.bottom() {
                    p.rect_filled(Rect::from_min_max(pos2(bar_r.left(), y), pos2(bar_r.right(), (y + 3.0).min(bar_r.bottom()))), 0.0, bar);
                    y += 6.0;
                }
            }
            Busy::Free => {
                p.rect_stroke(bar_r, 0.0, (1.0, bar), egui::StrokeKind::Inside);
            }
            Busy::WorkingElsewhere => {
                let mut y = bar_r.top() + 2.0;
                while y < bar_r.bottom() {
                    p.circle_filled(pos2(bar_r.center().x, y), 1.4, bar);
                    y += 4.0;
                }
            }
            _ => {
                p.rect_filled(bar_r, 0.0, bar);
            }
        }
        p.rect_stroke(
            r,
            3.0,
            if chosen { Stroke::new(2.0, t.accent_dark) } else { Stroke::new(1.0, bar.gamma_multiply(0.5)) },
            egui::StrokeKind::Inside,
        );
        let inner = p.with_clip_rect(r.shrink(2.0).intersect(p.clip_rect()));
        let subject = if a.subject.trim().is_empty() { "(No subject)".to_string() } else { one_line(&a.subject) };
        let w = r.width() - 12.0;
        let x = r.left() + 8.0;
        let one_row = r.height() < 34.0;
        if one_row {
            let mut line = subject;
            if !a.location.is_empty() {
                line = format!("{line}; {}", one_line(&a.location));
            }
            clip_text(&inner, pos2(x, r.center().y - 8.0), w, &line, bold(11.5), t.text);
        } else {
            clip_text(&inner, pos2(x, r.top() + 3.0), w, &subject, bold(11.5), t.text);
            if !a.location.is_empty() {
                clip_text(&inner, pos2(x, r.top() + 19.0), w, &one_line(&a.location), font(11.0), t.text_dim);
            }
        }
        if a.reminder.is_some() && r.width() > 60.0 && !one_row {
            icons::paint(&inner, Rect::from_min_size(pos2(r.right() - 16.0, r.bottom() - 16.0), vec2(11.0, 11.0)), Icon::Bell, t.text_faint);
        }
        let resp = ui.interact(r, egui::Id::new(("cal-appt", place, id)), Sense::click());
        let off = self.session.tz_offset_min;
        let (s, e) = (time::civil(a.start, off), time::civil(a.end, off));
        let r = &self.session.region;
        let when = if a.all_day { "All day".to_string() } else { format!("{} - {}", r.time(&s), r.time(&e)) };
        let place = if a.location.is_empty() { String::new() } else { format!("\n{}", one_line(&a.location)) };
        resp.on_hover_text(format!("{when}\n{}{place}\nShow as: {}", one_line(&a.subject), show_as(a.busy)))
    }

    /// The month view: six weeks, each day with what is on it.
    fn month_grid(&mut self, ui: &mut Ui, rect: Rect, days: &[i64]) {
        let t = self.tokens.clone();
        let off = self.session.tz_offset_min;
        let today = self.session.today();
        let chosen = date_of(if self.session.calendar.day == 0 { today } else { self.session.calendar.day });
        let p = ui.painter_at(rect);
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), 26.0));
        let w = (rect.width() - 8.0) / 7.0;
        let h = ((rect.height() - head.height() - 4.0) / 6.0).max(40.0);
        let ws = usize::try_from(self.session.region.week_start % 7).unwrap_or(0);
        for i in 0..7 {
            let name = WEEKDAYS.get((ws + i) % 7).copied().unwrap_or("");
            p.text(pos2(rect.left() + w * i as f32 + 8.0, head.center().y), Align2::LEFT_CENTER, name, font(11.5), t.text_dim);
        }
        let items = self.session.calendar.items.clone();
        let selected = self.session.calendar.selected;
        let mut open: Option<u64> = None;
        let mut select: Option<Option<u64>> = None;
        let mut new_on: Option<i64> = None;
        let mut go: Option<i64> = None;
        for (i, day) in days.iter().enumerate() {
            let (row, col) = (i / 7, i % 7);
            let cell = Rect::from_min_size(pos2(rect.left() + w * col as f32, head.bottom() + h * row as f32), vec2(w, h));
            let c = date_of(*day);
            let in_month = c.month == chosen.month;
            p.rect_filled(cell, 0.0, if in_month { t.surface } else { t.pane });
            p.rect_stroke(cell, 0.0, (1.0, t.line), egui::StrokeKind::Inside);
            let cell_resp = ui.interact(cell, egui::Id::new(("cal-month-day", *day)), Sense::click());
            let label = if c.day == 1 { format!("{} {}", c.month_name().chars().take(3).collect::<String>(), c.day) } else { c.day.to_string() };
            let is_today = *day == today;
            if is_today {
                let lw = text_width(&p, &label, bold(12.0)) + 10.0;
                p.rect_filled(Rect::from_min_size(pos2(cell.left() + 3.0, cell.top() + 3.0), vec2(lw, 17.0)), 8.0, t.accent);
            }
            p.text(
                pos2(cell.left() + 8.0, cell.top() + 11.5),
                Align2::LEFT_CENTER,
                &label,
                if is_today || c.day == 1 { bold(12.0) } else { font(12.0) },
                if is_today {
                    t.on_accent
                } else if in_month {
                    t.text
                } else {
                    t.text_faint
                },
            );
            let from = calendar::day_start(*day, off);
            let on: Vec<&(u64, Appointment)> = items.iter().filter(|(_, a)| a.overlaps(from, from.saturating_add(DAY))).collect();
            let room = (((cell.height() - 24.0) / MONTH_LINE).floor().max(0.0)) as usize;
            let fits = if on.len() > room { room.saturating_sub(1) } else { on.len() };
            for (k, (id, a)) in on.iter().take(fits).enumerate() {
                let r = Rect::from_min_size(
                    pos2(cell.left() + 3.0, cell.top() + 22.0 + MONTH_LINE * k as f32),
                    vec2(cell.width() - 6.0, MONTH_LINE - 2.0),
                );
                let resp = ui.interact(r, egui::Id::new(("cal-month-appt", *day, *id)), Sense::click());
                let whole = a.all_day || a.end.saturating_sub(a.start) >= DAY;
                let chosen = selected == Some(*id);
                if whole {
                    p.rect_filled(r, 2.0, if a.busy == Busy::OutOfOffice { t.away.gamma_multiply(0.3) } else { t.accent.gamma_multiply(0.2) });
                } else if resp.hovered() {
                    p.rect_filled(r, 2.0, t.hover);
                }
                if chosen {
                    p.rect_stroke(r, 2.0, (1.5, t.accent_dark), egui::StrokeKind::Inside);
                }
                let mut x = r.left() + 4.0;
                if !whole {
                    let s = time::civil(a.start.max(from), off);
                    let when = self.session.region.time(&s);
                    p.text(pos2(x, r.center().y), Align2::LEFT_CENTER, &when, font(11.0), t.text_dim);
                    x += text_width(&p, &when, font(11.0)) + 5.0;
                }
                let subject = if a.subject.trim().is_empty() { "(No subject)".to_string() } else { one_line(&a.subject) };
                clip_text(&p, pos2(x, r.center().y - 7.5), r.right() - x - 2.0, &subject, font(11.5), t.text);
                if resp.double_clicked() {
                    open = Some(*id);
                } else if resp.clicked() {
                    select = Some(Some(*id));
                }
            }
            if on.len() > fits {
                let more = format!("{} more", on.len() - fits);
                let r = Rect::from_min_size(
                    pos2(cell.left() + 3.0, cell.top() + 22.0 + MONTH_LINE * fits as f32),
                    vec2(cell.width() - 6.0, MONTH_LINE - 2.0),
                );
                let resp = ui.interact(r, egui::Id::new(("cal-month-more", *day)), Sense::click());
                p.text(
                    pos2(r.left() + 4.0, r.center().y),
                    Align2::LEFT_CENTER,
                    &more,
                    bold(11.0),
                    if resp.hovered() { t.accent } else { t.text_dim },
                );
                if resp.clicked() {
                    go = Some(*day);
                }
            }
            if cell_resp.double_clicked() {
                new_on = Some(from);
            } else if cell_resp.clicked() {
                select = Some(None);
            }
        }
        if let Some(id) = open {
            self.open_appointment(id);
        } else if let Some(day) = go {
            self.act("calendar.view", json!({ "view": "day" }));
            self.go_to_day(day);
        } else if let Some(sel) = select {
            self.act("calendar.select", json!({ "id": sel }));
        } else if let Some(start) = new_on {
            // A day of the month makes an all-day event, as Outlook's does.
            self.appointment = Some(AppointmentForm::new_at(start, true, off));
        }
    }

    /// Open the appointment `id` in its window.
    pub(crate) fn open_appointment(&mut self, id: u64) {
        let off = self.session.tz_offset_min;
        if let Some((_, a)) = self.session.calendar.items.iter().find(|(i, _)| *i == id) {
            self.appointment = Some(AppointmentForm::of(Some(id), a, off));
            self.act("calendar.select", json!({ "id": id }));
        }
    }

    /// A new appointment at the next half hour of the day chosen.
    pub(crate) fn new_appointment(&mut self) {
        let s = &self.session;
        let off = s.tz_offset_min;
        let now = s.time_now();
        let day = if s.calendar.day == 0 { s.today() } else { s.calendar.day };
        let into_day = now.saturating_sub(calendar::day_start(s.today(), off));
        let half = (into_day / (30 * MINUTE)).saturating_add(1).clamp(0, 47);
        let start = calendar::day_start(day, off).saturating_add(half * 30 * MINUTE);
        self.appointment = Some(AppointmentForm::new_at(start, false, off));
    }

    /// The window an appointment is made or changed in.
    pub(crate) fn appointment_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.appointment.take() else { return };
        let t = self.tokens.clone();
        let off = self.session.tz_offset_min;
        let mut ask: Option<Ask> = None;
        // What else is at that time, named, where Outlook only says there is a conflict.
        let clashes: Vec<String> = form
            .span(off)
            .map(|(s, e)| {
                self.session
                    .calendar
                    .items
                    .iter()
                    .filter(|(id, a)| Some(*id) != form.id && a.busy != Busy::Free && !a.all_day && e > s && a.overlaps(s, e))
                    .take(3)
                    .map(|(_, a)| one_line(&a.subject))
                    .collect()
            })
            .unwrap_or_default();
        let title = if form.id.is_some() { "Appointment" } else { "New Appointment" };
        let shown = crate::dialog::Dialog::new("appointment", title, Icon::NewAppointment).show(ctx, &t, |ui| {
            if !clashes.is_empty() {
                let words = format!("At the same time: {}", clashes.join(", "));
                crate::dialog::callout(ui, &t, t.flag, |ui| {
                    ui.label(egui::RichText::new(words).font(font(12.0)).color(t.text));
                });
                ui.add_space(6.0);
            }
            egui::Grid::new("appointment-form").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                ui.label(egui::RichText::new("Subject").color(t.text_dim));
                let subject = ui.add(LineEdit::new(&mut form.subject, egui::Id::new("appt-subject")).desired_width(380.0).char_limit(255));
                if std::mem::take(&mut form.focus) {
                    subject.request_focus();
                }
                ui.end_row();
                ui.label(egui::RichText::new("Location").color(t.text_dim));
                ui.add(LineEdit::new(&mut form.location, egui::Id::new("appt-location")).desired_width(380.0).char_limit(255));
                ui.end_row();
                for (label, date, clock, key) in
                    [("Start time", &mut form.start_date, &mut form.start_time, "start"), ("End time", &mut form.end_date, &mut form.end_time, "end")]
                {
                    ui.label(egui::RichText::new(label).color(t.text_dim));
                    ui.horizontal(|ui| {
                        ui.add(LineEdit::new(date, egui::Id::new(("appt-date", key))).desired_width(110.0).char_limit(10).hint("YYYY-MM-DD", None));
                        if !form.all_day {
                            ui.add(LineEdit::new(clock, egui::Id::new(("appt-time", key))).desired_width(64.0).char_limit(5).hint("HH:MM", None));
                        }
                    });
                    ui.end_row();
                }
                ui.label("");
                ui.checkbox(&mut form.all_day, "All day event");
                ui.end_row();
                ui.label(egui::RichText::new("Show As").color(t.text_dim));
                egui::ComboBox::from_id_salt("appt-busy").selected_text(show_as(form.busy)).width(180.0).show_ui(ui, |ui| {
                    for (b, name) in SHOW_AS {
                        ui.selectable_value(&mut form.busy, *b, *name);
                    }
                });
                ui.end_row();
                ui.label(egui::RichText::new("Reminder").color(t.text_dim));
                egui::ComboBox::from_id_salt("appt-reminder").selected_text(reminder_words(form.reminder)).width(180.0).show_ui(ui, |ui| {
                    for (r, name) in REMINDERS {
                        ui.selectable_value(&mut form.reminder, *r, *name);
                    }
                });
                ui.end_row();
            });
            ui.add_space(8.0);
            match form.span(off) {
                Some((s, e)) if e > s => {
                    ui.label(egui::RichText::new(format!("Lasts {}", length_words(e.saturating_sub(s) / MINUTE))).font(font(12.0)).color(t.text_dim));
                }
                Some(_) => {
                    ui.label(egui::RichText::new("It ends before it starts").font(font(12.0)).color(t.danger));
                }
                None => {
                    ui.label(egui::RichText::new("Dates are written YYYY-MM-DD and times HH:MM").font(font(12.0)).color(t.text_dim));
                }
            }
            if let Some(said) = &form.said {
                crate::dialog::callout(ui, &t, t.danger, |ui| {
                    ui.label(egui::RichText::new(one_line(said)).font(font(12.0)).color(t.text));
                });
            }
            crate::dialog::foot(ui, &t, |ui| {
                if form.id.is_some() && ui.add(crate::dialog::danger(&t, "Delete")).on_hover_text("Moves it to Deleted Items").clicked() {
                    ask = Some(Ask::Delete);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(crate::dialog::plain("Cancel")).clicked() {
                        ask = Some(Ask::Close);
                    }
                    if ui.add(crate::dialog::primary(&t, "Save & Close")).clicked() {
                        ask = Some(Ask::Save);
                    }
                });
            });
        });
        if shown.closed || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ask = Some(Ask::Close);
        }
        let result = match ask {
            None => {
                self.appointment = Some(form);
                return;
            }
            Some(Ask::Close) => return,
            Some(Ask::Save) => self.execute(if form.id.is_some() { "calendar.change" } else { "calendar.new" }, &form.params()),
            Some(Ask::Delete) => self.execute("calendar.delete", &json!({ "id": form.id })),
        };
        if let Err(e) = result {
            form.said = Some(e);
            self.appointment = Some(form);
        }
    }
}

/// A chevron pointing left (`back`) or right, centred on `c`.
fn chevron(p: &Painter, c: egui::Pos2, back: bool, color: Color32) {
    let d = if back { -1.0 } else { 1.0 };
    let s = Stroke::new(1.6, color);
    p.line_segment([pos2(c.x - 3.0 * d, c.y - 5.5), pos2(c.x + 2.5 * d, c.y)], s);
    p.line_segment([pos2(c.x + 2.5 * d, c.y), pos2(c.x - 3.0 * d, c.y + 5.5)], s);
}
