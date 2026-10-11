//! View commands: module switching, arrangement, panes, filter and search.

use super::{CommandSpec, always, has_folder, need_str, ok, p_bool, p_str};
use crate::{CmdResult, EngineError, Module, ReadingPane, Session, SortKey};
use serde_json::Value;

const MAX_QUERY: usize = 512;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "nav.module",
            label: "Switch Module",
            tab: "",
            group: "",
            shortcut: "Ctrl+1…4",
            params: r#"{"module": "mail" | "calendar" | "people" | "tasks"}"#,
            enabled: always,
            run: module,
        },
        CommandSpec {
            id: "view.sortBy",
            label: "Arrange By",
            tab: "View",
            group: "Arrangement",
            shortcut: "",
            params: r#"{"key": "date" | "from" | "to" | "subject" | "size" | "importance" | "flag" | "attachments"}; optional "descending""#,
            enabled: has_folder,
            run: sort_by,
        },
        CommandSpec {
            id: "view.reverseSort",
            label: "Reverse Sort",
            tab: "View",
            group: "Arrangement",
            shortcut: "",
            params: "",
            enabled: has_folder,
            run: reverse_sort,
        },
        CommandSpec {
            id: "view.readingPane",
            label: "Reading Pane",
            tab: "View",
            group: "Layout",
            shortcut: "",
            params: r#"{"position": "right" | "bottom" | "off"}"#,
            enabled: always,
            run: reading_pane,
        },
        CommandSpec {
            id: "view.folderPane",
            label: "Folder Pane",
            tab: "View",
            group: "Layout",
            shortcut: "",
            params: r#"{"show": true | false}; default toggles"#,
            enabled: always,
            run: folder_pane,
        },
        CommandSpec {
            id: "view.filterUnread",
            label: "Unread",
            tab: "Home",
            group: "Find",
            shortcut: "",
            params: r#"{"on": true | false}; default toggles"#,
            enabled: has_folder,
            run: filter_unread,
        },
        CommandSpec {
            id: "search.query",
            label: "Search",
            tab: "",
            group: "",
            shortcut: "Ctrl+E",
            params: r#"{"text": words to find in the current folder}"#,
            enabled: has_folder,
            run: search,
        },
        CommandSpec {
            id: "search.clear",
            label: "Close Search",
            tab: "Search",
            group: "Close",
            shortcut: "Esc",
            params: "",
            enabled: has_folder,
            run: clear_search,
        },
    ]
}

fn module(s: &mut Session, p: &Value) -> CmdResult {
    let name = need_str(p, "module")?;
    s.module = Module::parse(name).ok_or_else(|| EngineError::BadParams(format!("\"{name}\" is not mail, calendar, people or tasks")))?;
    ok()
}

fn sort_by(s: &mut Session, p: &Value) -> CmdResult {
    let name = need_str(p, "key")?;
    let key = SortKey::parse(name).ok_or_else(|| EngineError::BadParams(format!("\"{name}\" is not a column to arrange by")))?;
    // Choosing the current arrangement again flips its order, like clicking a column header.
    s.sort_desc = match p_bool(p, "descending") {
        Some(d) => d,
        None if key == s.sort => !s.sort_desc,
        None => matches!(key, SortKey::Date | SortKey::Size | SortKey::Importance | SortKey::Flag | SortKey::Attachments),
    };
    s.sort = key;
    s.reload_rows()?;
    ok()
}

fn reverse_sort(s: &mut Session, _: &Value) -> CmdResult {
    s.sort_desc = !s.sort_desc;
    s.reload_rows()?;
    ok()
}

fn reading_pane(s: &mut Session, p: &Value) -> CmdResult {
    s.reading_pane = match need_str(p, "position")?.to_ascii_lowercase().as_str() {
        "right" => ReadingPane::Right,
        "bottom" => ReadingPane::Bottom,
        "off" => ReadingPane::Off,
        other => return Err(EngineError::BadParams(format!("\"{other}\" is not right, bottom or off"))),
    };
    ok()
}

fn folder_pane(s: &mut Session, p: &Value) -> CmdResult {
    s.folder_pane = p_bool(p, "show").unwrap_or(!s.folder_pane);
    ok()
}

fn filter_unread(s: &mut Session, p: &Value) -> CmdResult {
    s.unread_only = p_bool(p, "on").unwrap_or(!s.unread_only);
    s.reload_rows()?;
    ok()
}

fn search(s: &mut Session, p: &Value) -> CmdResult {
    s.search = p_str(p, "text").unwrap_or("").chars().take(MAX_QUERY).collect();
    s.reload_rows()?;
    ok()
}

fn clear_search(s: &mut Session, _: &Value) -> CmdResult {
    s.search.clear();
    s.reload_rows()?;
    ok()
}
