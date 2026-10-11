//! FILETIME ticks ↔ calendar dates. The model stores UTC; a local offset is applied only for display.

/// Seconds between 1601-01-01 and 1970-01-01.
const UNIX_DIFF_SECS: i64 = 11_644_473_600;
const TICKS_PER_SEC: i64 = 10_000_000;

pub const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
pub const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

pub fn from_unix(secs: i64) -> i64 {
    secs.saturating_add(UNIX_DIFF_SECS).saturating_mul(TICKS_PER_SEC)
}

pub fn to_unix(filetime: i64) -> i64 {
    filetime.div_euclid(TICKS_PER_SEC).saturating_sub(UNIX_DIFF_SECS)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    /// 1–12
    pub month: u32,
    /// 1–31
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 = Sunday
    pub weekday: u32,
    /// Days since 1970-01-01 in the same offset; handy for "today / yesterday" grouping.
    pub day_number: i64,
}

impl Civil {
    pub fn weekday_name(&self) -> &'static str {
        WEEKDAYS.get(self.weekday as usize).copied().unwrap_or("")
    }

    pub fn month_name(&self) -> &'static str {
        MONTHS.get(self.month.saturating_sub(1) as usize).copied().unwrap_or("")
    }

    /// 12-hour clock: (hour 1–12, "AM"/"PM").
    pub fn hour12(&self) -> (u32, &'static str) {
        let h = match self.hour % 12 {
            0 => 12,
            h => h,
        };
        (h, if self.hour < 12 { "AM" } else { "PM" })
    }

    /// "9:14 AM"
    pub fn time_short(&self) -> String {
        let (h, ap) = self.hour12();
        format!("{h}:{:02} {ap}", self.minute)
    }

    /// "Mon 10/5/2026 9:14 AM", the reading pane's date.
    pub fn long_stamp(&self) -> String {
        format!("{} {}/{}/{} {}", self.weekday_name().get(..3).unwrap_or(""), self.month, self.day, self.year, self.time_short())
    }

    /// "Monday, October 5, 2026 9:14 AM", the "Sent:" line of a reply header.
    pub fn full_stamp(&self) -> String {
        format!("{}, {} {}, {} {}", self.weekday_name(), self.month_name(), self.day, self.year, self.time_short())
    }
}

/// Break UTC FILETIME ticks into a calendar date at `offset_minutes` east of UTC.
pub fn civil(filetime: i64, offset_minutes: i32) -> Civil {
    let secs = to_unix(filetime).saturating_add(i64::from(offset_minutes).saturating_mul(60));
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    Civil {
        year,
        month,
        day,
        hour: (rem / 3600) as u32,
        minute: (rem % 3600 / 60) as u32,
        second: (rem % 60) as u32,
        weekday: (days.rem_euclid(7) + 4).rem_euclid(7) as u32,
        day_number: days,
    }
}

/// UTC FILETIME ticks for a calendar date given at `offset_minutes` east of UTC.
pub fn from_civil(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32, offset_minutes: i32) -> i64 {
    let days = days_from_civil(year, month, day);
    let secs = days
        .saturating_mul(86_400)
        .saturating_add(i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second))
        .saturating_sub(i64::from(offset_minutes).saturating_mul(60));
    from_unix(secs)
}

// Days ↔ civil date, proleptic Gregorian (Howard Hinnant's public-domain algorithms).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_date() {
        // 2026-10-05 09:14:00 UTC was a Monday.
        let ft = from_civil(2026, 10, 5, 9, 14, 0, 0);
        let c = civil(ft, 0);
        assert_eq!((c.year, c.month, c.day, c.hour, c.minute, c.weekday), (2026, 10, 5, 9, 14, 1));
        assert_eq!(c.long_stamp(), "Mon 10/5/2026 9:14 AM");
        assert_eq!(c.full_stamp(), "Monday, October 5, 2026 9:14 AM");
    }

    #[test]
    fn unix_epoch_and_offset() {
        assert_eq!(to_unix(from_unix(0)), 0);
        let c = civil(from_unix(0), 120);
        assert_eq!((c.year, c.month, c.day, c.hour, c.weekday), (1970, 1, 1, 2, 4));
        assert_eq!(civil(from_unix(0), -60).day, 31);
    }

    #[test]
    fn extreme_ticks_do_not_panic() {
        for ft in [i64::MIN, -1, 0, i64::MAX] {
            let _ = civil(ft, i32::MAX);
            let _ = civil(ft, i32::MIN);
        }
        assert_eq!(civil(from_civil(2024, 2, 29, 23, 59, 59, 0), 0).day, 29);
    }
}
