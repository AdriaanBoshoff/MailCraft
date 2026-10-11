//! File commands: opening data files and loose messages, saving a message, closing a data file.

use super::{CommandSpec, always, has_selection, need_str, ok, p_bool, p_str, p_u64};
use crate::io::{self, Opened};
use crate::{CmdResult, EngineError, Session, attachments};
use mailcraft_model::{FolderId, MemStore, StoreError};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;

/// Loose `.msg`/`.eml` files open into this in-memory store.
pub const OPENED_FILES: &str = "Opened Files";

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "file.open",
            label: "Open Outlook Data File",
            tab: "File",
            group: "Open & Export",
            shortcut: "Ctrl+O",
            params: r#"{"path": a .pst, .ost, .msg, .oft or .eml file}; opened read-only"#,
            enabled: always,
            run: open,
        },
        CommandSpec {
            id: "file.openSample",
            label: "Open Sample Mailbox",
            tab: "File",
            group: "Open & Export",
            shortcut: "",
            params: "",
            enabled: always,
            run: open_sample,
        },
        CommandSpec {
            id: "file.saveAs",
            label: "Save As",
            tab: "File",
            group: "Save",
            shortcut: "F12",
            params: r#"{"path": target ending in .msg, .oft or .eml}"#,
            enabled: has_selection,
            run: save_as,
        },
        CommandSpec {
            id: "file.saveAttachment",
            label: "Save Attachment",
            tab: "File",
            group: "Save",
            shortcut: "",
            params: r#"{"index": attachment number, "path": target file, or a folder to save it in under its own name}; returns {"untrusted_content": {"path"}}"#,
            enabled: has_selection,
            run: save_attachment,
        },
        CommandSpec {
            id: "file.saveAllAttachments",
            label: "Save All Attachments",
            tab: "File",
            group: "Save",
            shortcut: "",
            params: r#"{"folder": an existing folder, "inline": also pictures placed in the body (default false)}; no file is replaced; returns {"count", "untrusted_content": {"paths"}}"#,
            enabled: has_selection,
            run: save_all_attachments,
        },
        CommandSpec {
            id: "file.print",
            label: "Print",
            tab: "File",
            group: "Print",
            shortcut: "Ctrl+P",
            params: r#"{"path": target ending in .pdf, "paper": "a4" (default) or "letter", "links": which links are numbered and listed after the body: "concerns" (default: those that are not what their text says), "all" or "none"}; returns {"path", "pages", "truncated", "links"}"#,
            enabled: has_selection,
            run: print,
        },
        CommandSpec {
            id: "file.closeStore",
            label: "Close Data File",
            tab: "Folder",
            group: "Actions",
            shortcut: "",
            params: r#"{"store": index}; default: the current one"#,
            enabled: has_store,
            run: close_store,
        },
    ]
}

fn has_store(s: &Session) -> bool {
    !s.stores.is_empty()
}

fn open(s: &mut Session, p: &Value) -> CmdResult {
    let path = Path::new(need_str(p, "path")?);
    match io::open_path(path)? {
        Opened::Store(store) => {
            let name = store.display_name();
            let idx = s.add_store(store);
            s.select_default_folder(idx);
            s.status = format!("Opened {name} (read-only)");
            Ok(json!({ "store": idx }))
        }
        Opened::Item(item) => {
            let idx = match s.stores.iter().position(|st| st.display_name() == OPENED_FILES && st.writable()) {
                Some(i) => i,
                None => {
                    let mut mem = MemStore::new(OPENED_FILES);
                    mem.push_folder(None, "Files", "IPF.Note");
                    s.add_store(Box::new(mem))
                }
            };
            let folder = s.folders.get(idx).and_then(|f| f.first()).map(|f| f.id).ok_or(StoreError::NoFolder)?;
            let id = s.stores.get_mut(idx).ok_or(StoreError::NoFolder)?.add(folder, *item)?;
            s.refresh_folders(idx);
            s.select_folder(idx, folder)?;
            // Opening a file is not reading it: keep the read state the file had.
            let item = s.stores.get_mut(idx).ok_or(StoreError::NoFolder)?.load(folder, id)?;
            s.selected = Some(id.0);
            s.open = Some(item);
            Ok(json!({ "store": idx, "id": id.0 }))
        }
    }
}

fn open_sample(s: &mut Session, _: &Value) -> CmdResult {
    let idx = s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.select_default_folder(idx);
    Ok(json!({ "store": idx }))
}

fn save_as(s: &mut Session, p: &Value) -> CmdResult {
    let path = Path::new(need_str(p, "path")?);
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    io::save_item(item, path)?;
    s.status = format!("Saved {}", path.display());
    ok()
}

fn save_attachment(s: &mut Session, p: &Value) -> CmdResult {
    let path = Path::new(need_str(p, "path")?);
    let index = usize::try_from(p_u64(p, "index").unwrap_or(0)).unwrap_or(usize::MAX);
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    let att = item
        .attachments
        .get(index)
        .ok_or_else(|| EngineError::BadParams(format!("there is no attachment {index}; the message has {}", item.attachments.len())))?;
    // A folder takes the attachment under its own name, made safe, beside what is there.
    let target = if path.is_dir() {
        attachments::free_path(path, &attachments::file_name(att), &mut HashSet::new()).ok_or_else(|| full(path))?
    } else {
        path.to_path_buf()
    };
    save_one(att, &target)?;
    s.status = format!("Saved {}", target.display());
    Ok(json!({"untrusted_content": {"path": target.display().to_string()}}))
}

fn full(folder: &Path) -> EngineError {
    EngineError::BadParams(format!("{} has too many files of that name", folder.display()))
}

fn save_one(att: &mailcraft_model::Attachment, path: &Path) -> Result<(), EngineError> {
    match &att.embedded {
        Some(inner) => io::save_item(inner, path),
        None => io::write_atomic(path, &att.data),
    }
}

/// Save every attachment of the open message into a folder, each under its own name made safe.
/// No file is replaced: a name that is taken gets a number.
fn save_all_attachments(s: &mut Session, p: &Value) -> CmdResult {
    let folder = Path::new(need_str(p, "folder")?);
    if !folder.is_dir() {
        return Err(EngineError::BadParams(format!("{} is not a folder", folder.display())));
    }
    let inline = p_bool(p, "inline").unwrap_or(false);
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    let mut taken = HashSet::new();
    let mut paths = Vec::new();
    for att in item.attachments.iter().filter(|a| inline || attachments::is_file(a)) {
        let target = attachments::free_path(folder, &attachments::file_name(att), &mut taken).ok_or_else(|| full(folder))?;
        save_one(att, &target)?;
        paths.push(target.display().to_string());
    }
    s.status = match paths.len() {
        0 => "This message has no attachments to save".to_string(),
        1 => format!("Saved 1 attachment to {}", folder.display()),
        n => format!("Saved {n} attachments to {}", folder.display()),
    };
    Ok(json!({"count": paths.len(), "untrusted_content": {"paths": paths}}))
}

/// Print the open message, in Memo Style, to a PDF file.
fn print(s: &mut Session, p: &Value) -> CmdResult {
    let path = Path::new(need_str(p, "path")?);
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        return Err(EngineError::BadParams("printing writes a PDF file: the path must end in .pdf".into()));
    }
    let paper = match p_str(p, "paper") {
        Some(name) => mailcraft_layout::Paper::named(name).ok_or_else(|| EngineError::BadParams(format!("unknown paper {name:?}: a4 or letter")))?,
        None => mailcraft_layout::Paper::A4,
    };
    let links = match p_str(p, "links") {
        Some(name) => {
            crate::print::Links::named(name).ok_or_else(|| EngineError::BadParams(format!("unknown links {name:?}: none, concerns or all")))?
        }
        None => crate::print::Links::default(),
    };
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    let owner = if s.user_name.trim().is_empty() { &s.user_email } else { &s.user_name };
    let printed = crate::print::pdf(item, owner, s.tz_offset_min, paper, links)?;
    io::write_atomic(path, &printed.file)?;
    s.status = format!("Printed {} page{} to {}", printed.pages, if printed.pages == 1 { "" } else { "s" }, path.display());
    Ok(json!({"path": path.display().to_string(), "pages": printed.pages, "truncated": printed.truncated, "links": printed.links}))
}

fn close_store(s: &mut Session, p: &Value) -> CmdResult {
    let idx = match p_u64(p, "store") {
        Some(i) => usize::try_from(i).unwrap_or(usize::MAX),
        None => s.current.map(|(i, _)| i).unwrap_or(0),
    };
    if idx >= s.stores.len() {
        return Err(EngineError::BadParams(format!("there is no data file {idx}")));
    }
    s.stores.remove(idx);
    // The steps name data files by their place in the list, which has just changed.
    s.undo.clear();
    if idx < s.folders.len() {
        s.folders.remove(idx);
    }
    let keep: Option<(usize, FolderId)> = match s.current {
        Some((c, f)) if c < idx => Some((c, f)),
        Some((c, f)) if c > idx => Some((c - 1, f)),
        _ => None,
    };
    match keep {
        Some(cur) => s.current = Some(cur),
        None => {
            s.current = None;
            s.selected = None;
            s.open = None;
            s.rows.clear();
            if !s.stores.is_empty() {
                s.select_default_folder(0);
            }
        }
    }
    ok()
}
