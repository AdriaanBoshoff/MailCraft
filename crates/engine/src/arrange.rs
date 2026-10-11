//! The order a folder's messages are listed in, and the groups they stand in.

use std::cmp::Ordering;

use mailcraft_model::{Flag, Summary, strip_reply_prefix, time};

use crate::SortKey;
use crate::search::fold;

/// Characters of a name or a subject that are compared, and of a group's label.
const MAX_COMPARED: usize = 512;
const MAX_LABEL: usize = 80;

/// Two texts as a reader orders them: numbers inside them by their value, so that `Invoice 9`
/// stands before `Invoice 10`. Both are taken as already folded ([`fold`]).
pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let digits = |chars: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut run = String::new();
                    while let Some(d) = chars.peek().copied().filter(char::is_ascii_digit) {
                        run.push(d);
                        chars.next();
                    }
                    run
                };
                let (x, y) = (digits(&mut a), digits(&mut b));
                // Without their leading zeros, the longer number is the larger one.
                let (plain_x, plain_y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = plain_x.len().cmp(&plain_y.len()).then_with(|| plain_x.cmp(plain_y));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                a.next();
                b.next();
            }
        }
    }
}

/// What a row is compared by when it is arranged by a text: folded, a subject without its reply
/// and forward prefixes.
fn compared(row: &Summary, key: SortKey) -> Option<String> {
    let text = match key {
        SortKey::From => row.from.as_str(),
        SortKey::To => row.to.as_str(),
        SortKey::Subject => strip_reply_prefix(&row.subject),
        _ => return None,
    };
    Some(fold(&text.trim().chars().take(MAX_COMPARED).collect::<String>()))
}

/// Put `rows` in order by `key`. Rows that are equal by it stand newest first whichever way the
/// order runs, and rows that are equal in everything keep the order they came in.
pub fn sort(rows: &mut Vec<Summary>, key: SortKey, descending: bool) {
    let mut keyed: Vec<(Option<String>, Summary)> = std::mem::take(rows).into_iter().map(|row| (compared(&row, key), row)).collect();
    keyed.sort_by(|(text_a, a), (text_b, b)| {
        let order = match key {
            SortKey::Date => a.time.cmp(&b.time),
            SortKey::From | SortKey::To | SortKey::Subject => natural(text_a.as_deref().unwrap_or(""), text_b.as_deref().unwrap_or("")),
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::Importance => a.importance.cmp(&b.importance),
            SortKey::Flag => (a.flag as u8).cmp(&(b.flag as u8)),
            SortKey::Attachments => a.has_attachments.cmp(&b.has_attachments),
        };
        let order = if descending { order.reverse() } else { order };
        order.then_with(|| b.time.cmp(&a.time))
    });
    *rows = keyed.into_iter().map(|(_, row)| row).collect();
}

/// Rows that follow one another and belong together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub label: String,
    /// The first row of the group, counted from the top of the list.
    pub start: usize,
    pub count: usize,
    pub unread: usize,
}

/// The day a message is of, as a heading: the days of this week and the last by name, then the
/// month, then the year.
fn day_label(now: &time::Civil, row_time: i64, tz_offset_min: i32) -> String {
    if row_time <= 0 {
        return "No date".to_string();
    }
    let day = time::civil(row_time, tz_offset_min);
    match now.day_number.saturating_sub(day.day_number) {
        i64::MIN..=0 => "Today".to_string(),
        1 => "Yesterday".to_string(),
        2..=6 => "This Week".to_string(),
        7..=13 => "Last Week".to_string(),
        _ if day.year == now.year && day.month == now.month => "Earlier This Month".to_string(),
        _ if day.year == now.year => day.month_name().to_string(),
        // The months of the year before are still told apart while they are less than a year ago.
        _ if day.year == now.year.saturating_sub(1) && day.month > now.month => format!("{} {}", day.month_name(), day.year),
        _ => day.year.to_string(),
    }
}

fn size_label(size: u32) -> &'static str {
    match size {
        0..=10_239 => "Under 10 KB",
        10_240..=102_399 => "10 KB to 100 KB",
        102_400..=1_048_575 => "100 KB to 1 MB",
        1_048_576..=10_485_759 => "1 MB to 10 MB",
        _ => "Over 10 MB",
    }
}

fn named(text: &str, none: &str) -> String {
    let text: String = text.trim().chars().filter(|c| !c.is_control()).take(MAX_LABEL).collect();
    if text.is_empty() { none.to_string() } else { text }
}

/// The heading `row` stands under when the list is arranged by `key`.
fn label(row: &Summary, key: SortKey, now: &time::Civil, tz_offset_min: i32) -> String {
    match key {
        SortKey::Date => day_label(now, row.time, tz_offset_min),
        SortKey::From => named(&row.from, "No sender"),
        SortKey::To => named(&row.to, "No recipient"),
        SortKey::Subject => named(strip_reply_prefix(&row.subject), "No subject"),
        SortKey::Size => size_label(row.size).to_string(),
        SortKey::Importance => match row.importance {
            i32::MIN..=0 => "Low importance",
            1 => "Normal importance",
            _ => "High importance",
        }
        .to_string(),
        SortKey::Flag => match row.flag {
            Flag::Flagged => "Flagged",
            Flag::Complete => "Completed",
            Flag::None => "Not flagged",
        }
        .to_string(),
        SortKey::Attachments => if row.has_attachments { "With attachments" } else { "No attachments" }.to_string(),
    }
}

/// The groups `rows` stand in, arranged by `key`: rows that follow one another under the same
/// heading, with how many they are and how many of them are unread. Names and subjects that
/// differ only in case or accents are one group, under the heading of its first row.
pub fn groups(rows: &[Summary], key: SortKey, now: i64, tz_offset_min: i32) -> Vec<Group> {
    let now = time::civil(now, tz_offset_min);
    let mut groups: Vec<Group> = Vec::new();
    let mut last: Option<String> = None;
    for (at, row) in rows.iter().enumerate() {
        let label = label(row, key, &now, tz_offset_min);
        let same = fold(&label);
        match groups.last_mut() {
            Some(group) if last.as_deref() == Some(same.as_str()) => {
                group.count = group.count.saturating_add(1);
                group.unread = group.unread.saturating_add(usize::from(!row.read));
            }
            _ => {
                groups.push(Group { label, start: at, count: 1, unread: usize::from(!row.read) });
                last = Some(same);
            }
        }
    }
    groups
}
