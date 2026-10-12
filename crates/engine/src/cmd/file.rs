//! File commands: opening data files and loose messages, saving a message, closing a data file.

use super::{CommandSpec, always, has_selection, need_str, ok, p_bool, p_str, p_u64};
use crate::io::{self, Opened};
use crate::{CmdResult, EngineError, Session, attachments};
use openmailclient_model::{FolderId, MemStore, StoreError};
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
            params: r#"{"path": target ending in .msg, .oft or .eml, or a folder to save it in under its subject without replacing anything; "format": "msg" (default), "eml" or "oft" for a folder}"#,
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
            params: r#"{"path": target ending in .pdf, "links": which links are numbered and listed after the body: "concerns" (default: those that are not what their text says), "all" or "none"; and, for this print only, anything file.pageSetup takes: "paper", "orientation", "margins", "header", "footer"}; returns {"path", "pages", "truncated", "links"}"#,
            enabled: has_selection,
            run: print,
        },
        CommandSpec {
            id: "file.printPreview",
            label: "Print Preview",
            tab: "File",
            group: "Print",
            shortcut: "",
            params: r#"{"page": which page, from 1 (default 1), "path": where to write that page as a picture, ending in .png (left out: nothing is written), "dpi": pixels to the inch of the picture, 24 to 600 (default 96), "links" as file.print; and, for this look only, anything file.pageSetup takes}; the page is drawn from what file.print writes; returns {"pages", "page", "truncated", "links", "setup"} and with a path {"path", "width", "height"} in pixels"#,
            enabled: has_selection,
            run: print_preview,
        },
        CommandSpec {
            id: "file.pageSetup",
            label: "Page Setup",
            tab: "File",
            group: "Print",
            shortcut: "",
            params: r#"{"paper": "a4", "letter", "legal", "a3", "a5", "b5", "executive" or "tabloid", "orientation": "portrait" or "landscape", "margins": millimetres, one number for every edge or {"top", "right", "bottom", "left"}, "header" and "footer": a text for the middle, or {"left", "center", "right"}; in them [Page], [Pages], [Page of Pages], [Date], [Time], [User] and [Subject] are filled in, "reset": true to start from the default}; nothing asked for changes nothing; returns the setup"#,
            enabled: always,
            run: page_setup,
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
    // A data file that is open already is gone to, not opened a second time beside itself.
    if let Ok(real) = std::fs::canonicalize(path)
        && let Some(idx) = s.stores.iter().position(|st| st.path().and_then(|p| std::fs::canonicalize(p).ok()).is_some_and(|p| p == real))
    {
        s.select_default_folder(idx);
        return Ok(json!({ "store": idx, "already": true }));
    }
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
    // A folder takes the message under its subject, made safe, beside what is there.
    let target = if path.is_dir() {
        let kind = match p_str(p, "format").map(str::to_ascii_lowercase).as_deref() {
            None | Some("msg") => "msg",
            Some("eml") => "eml",
            Some("oft") => "oft",
            Some(other) => return Err(EngineError::BadParams(format!("\"{other}\" is not msg, eml or oft"))),
        };
        let subject = item.subject();
        let stem = attachments::safe_name(if subject.trim().is_empty() { "No subject" } else { subject.trim() });
        // A subject's last dot is not a file ending.
        let name = format!("{}.{kind}", stem.replace('.', "_"));
        attachments::free_path(path, &name, &mut HashSet::new()).ok_or_else(|| full(path))?
    } else {
        path.to_path_buf()
    };
    io::save_item(item, &target)?;
    s.status = format!("Saved {}", target.display());
    Ok(json!({"untrusted_content": {"path": target.display().to_string()}}))
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

fn save_one(att: &openmailclient_model::Attachment, path: &Path) -> Result<(), EngineError> {
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

/// Which links a print lists, as the parameters ask.
fn print_links(p: &Value) -> Result<crate::print::Links, EngineError> {
    match p_str(p, "links") {
        Some(name) => {
            crate::print::Links::named(name).ok_or_else(|| EngineError::BadParams(format!("unknown links {name:?}: none, concerns or all")))
        }
        None => Ok(crate::print::Links::default()),
    }
}

/// Lay the open message out as it would be printed and say how many pages it takes; with a
/// path, write one page as a picture.
fn print_preview(s: &mut Session, p: &Value) -> CmdResult {
    let path = match p_str(p, "path") {
        Some(path) if Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) => Some(Path::new(path)),
        Some(_) => return Err(EngineError::BadParams("a page is written as a PNG picture: the path must end in .png".into())),
        None => None,
    };
    let page = match p.get("page") {
        None => 1,
        Some(v) => v
            .as_u64()
            .filter(|n| *n >= 1)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| EngineError::BadParams("\"page\" is a page's number, from 1".into()))?,
    };
    let dpi = match p.get("dpi") {
        None => 96.0,
        Some(v) => {
            v.as_f64().filter(|d| (24.0..=600.0).contains(d)).ok_or_else(|| EngineError::BadParams("\"dpi\" is a number from 24 to 600".into()))?
        }
    };
    let setup = crate::print::setup_with(&s.page_setup, p)?;
    let links = print_links(p)?;
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    let laid = crate::print::laid(item, &crate::print::job(s, &setup, links))?;
    let pages = laid.printed.pages.len();
    if page > pages {
        return Err(EngineError::BadParams(format!("there is no page {page}: the message takes {pages}")));
    }
    let mut out =
        json!({"pages": pages, "page": page, "truncated": laid.printed.truncated, "links": laid.links, "setup": crate::print::setup_json(&setup)});
    if let Some(path) = path {
        let image =
            crate::print::raster(&laid, page - 1, dpi as f32 / 72.0).ok_or_else(|| EngineError::Format("the page could not be drawn".into()))?;
        io::write_atomic(path, &crate::print::png(&image)?)?;
        out["path"] = json!(path.display().to_string());
        out["width"] = json!(image.width);
        out["height"] = json!(image.height);
        s.status = format!("Page {page} of {pages} written to {}", path.display());
    }
    Ok(out)
}

/// Print the open message, in Memo Style, to a PDF file.
fn print(s: &mut Session, p: &Value) -> CmdResult {
    let path = Path::new(need_str(p, "path")?);
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        return Err(EngineError::BadParams("printing writes a PDF file: the path must end in .pdf".into()));
    }
    let setup = crate::print::setup_with(&s.page_setup, p)?;
    let links = print_links(p)?;
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    let printed = crate::print::pdf(item, &crate::print::job(s, &setup, links))?;
    io::write_atomic(path, &printed.file)?;
    s.status = format!("Printed {} page{} to {}", printed.pages, if printed.pages == 1 { "" } else { "s" }, path.display());
    Ok(json!({"path": path.display().to_string(), "pages": printed.pages, "truncated": printed.truncated, "links": printed.links}))
}

/// Change how pages are set up for printing, and say how they are.
fn page_setup(s: &mut Session, p: &Value) -> CmdResult {
    let from = if p_bool(p, "reset") == Some(true) { openmailclient_layout::Setup::default() } else { s.page_setup.clone() };
    let setup = crate::print::setup_with(&from, p)?;
    if setup != s.page_setup {
        s.page_setup = setup;
        s.status = "Page setup changed".to_string();
    }
    Ok(crate::print::setup_json(&s.page_setup))
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
