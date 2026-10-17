//! How dates and times are written here, as the system's region settings say: Outlook writes
//! every date the way they do, and so do we.

use openmailclient_model::time::Region;

/// The region as Windows keeps it for the person signed in (`Control Panel\International`).
/// Anything it does not say, or says wrongly, is left as it is by default.
#[cfg(windows)]
pub fn of_this_system() -> Region {
    let Ok(key) = windows_registry::CURRENT_USER.open(r"Control Panel\International") else { return Region::default() };
    let get = |name: &str| key.get_string(name).unwrap_or_default();
    // Windows counts the days of the week from Monday; we from Sunday.
    let week_start = get("iFirstDayOfWeek").trim().parse::<u32>().map_or(0, |monday_first| monday_first.saturating_add(1) % 7);
    Region::of(&get("sShortDate"), &get("sLongDate"), &get("sShortTime"), &get("s1159"), &get("s2359"), week_start)
}

/// Elsewhere the region is not read yet: dates are written as in the United States.
#[cfg(not(windows))]
pub fn of_this_system() -> Region {
    Region::default()
}
