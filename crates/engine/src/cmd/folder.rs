//! Folder commands: selecting, creating, renaming, deleting, marking all read.

use super::{CommandSpec, has_folder, has_folder_writable, need_str, ok, p_str, p_u64};
use crate::{CmdResult, EngineError, Session};
use mailcraft_model::{FolderId, FolderKind, ItemId, StoreError};
use serde_json::{Value, json};

const MAX_NAME: usize = 255;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "folder.select",
            label: "Go to Folder",
            tab: "",
            group: "",
            shortcut: "Ctrl+Y",
            params: r#"{"folder": "Inbox/Projects"} or {"folderId": id}; optional "store": index"#,
            enabled: has_store,
            run: select,
        },
        CommandSpec {
            id: "folder.new",
            label: "New Folder",
            tab: "Folder",
            group: "New",
            shortcut: "Ctrl+Shift+E",
            params: r#"{"name": text}; created inside the current folder"#,
            enabled: has_folder_writable,
            run: new,
        },
        CommandSpec {
            id: "folder.rename",
            label: "Rename Folder",
            tab: "Folder",
            group: "Actions",
            shortcut: "F2",
            params: r#"{"name": text}"#,
            enabled: user_folder,
            run: rename,
        },
        CommandSpec {
            id: "folder.delete",
            label: "Delete Folder",
            tab: "Folder",
            group: "Actions",
            shortcut: "",
            params: "",
            enabled: user_folder,
            run: delete,
        },
        CommandSpec {
            id: "folder.markAllRead",
            label: "Mark All as Read",
            tab: "Folder",
            group: "Clean Up",
            shortcut: "",
            params: "",
            enabled: has_folder_writable,
            run: mark_all_read,
        },
        CommandSpec {
            id: "folder.empty",
            label: "Empty Folder",
            tab: "Folder",
            group: "Clean Up",
            shortcut: "",
            params: "",
            enabled: can_empty,
            run: empty,
        },
        CommandSpec {
            id: "folder.properties",
            label: "Folder Properties",
            tab: "Folder",
            group: "Properties",
            shortcut: "",
            params: "",
            enabled: has_folder,
            run: properties,
        },
    ]
}

fn has_store(s: &Session) -> bool {
    !s.stores.is_empty()
}

/// Outlook's own folders (Inbox, Sent Items…) can't be renamed or deleted; folders people made can.
fn user_folder(s: &Session) -> bool {
    has_folder_writable(s) && s.current_folder().is_some_and(|f| matches!(f.kind, FolderKind::Mail | FolderKind::Other))
}

fn can_empty(s: &Session) -> bool {
    has_folder_writable(s) && s.current_folder().is_some_and(|f| matches!(f.kind, FolderKind::Deleted | FolderKind::Junk))
}

fn select(s: &mut Session, p: &Value) -> CmdResult {
    let store = match p_u64(p, "store") {
        Some(i) => usize::try_from(i).unwrap_or(usize::MAX),
        None => s.current.map(|(i, _)| i).unwrap_or(0),
    };
    if store >= s.stores.len() {
        return Err(EngineError::BadParams(format!("there is no data file {store}")));
    }
    let folder = match (p_u64(p, "folderId"), p_str(p, "folder")) {
        (Some(id), _) => FolderId(id),
        (None, Some(path)) => s.folder_by_path(store, path).ok_or_else(|| EngineError::BadParams(format!("there is no folder \"{path}\"")))?,
        _ => return Err(EngineError::BadParams("\"folder\" (path) or \"folderId\" is required".into())),
    };
    s.select_folder(store, folder)?;
    ok()
}

fn valid_name(p: &Value) -> Result<String, EngineError> {
    let name = need_str(p, "name")?.trim().to_string();
    if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(|c| c.is_control() || c == '/' || c == '\\') {
        return Err(EngineError::BadParams("a folder name is 1 to 255 characters without slashes".into()));
    }
    Ok(name)
}

fn new(s: &mut Session, p: &Value) -> CmdResult {
    let name = valid_name(p)?;
    let (store, parent) = s.current.ok_or(StoreError::NoFolder)?;
    let clash = s.folders.get(store).is_some_and(|l| l.iter().any(|f| f.parent == Some(parent) && f.name.eq_ignore_ascii_case(&name)));
    if clash {
        return Err(EngineError::Format(format!("a folder named \"{name}\" already exists here")));
    }
    let id = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.create_folder(parent, &name)?;
    s.refresh_folders(store);
    Ok(json!({ "folderId": id.0 }))
}

fn rename(s: &mut Session, p: &Value) -> CmdResult {
    let name = valid_name(p)?;
    let (store, folder) = s.current.ok_or(StoreError::NoFolder)?;
    s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.rename_folder(folder, &name)?;
    s.refresh_folders(store);
    ok()
}

fn delete(s: &mut Session, _: &Value) -> CmdResult {
    let (store, folder) = s.current.ok_or(StoreError::NoFolder)?;
    let parent = s.folder_info(store, folder).and_then(|f| f.parent);
    s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.delete_folder(folder)?;
    s.refresh_folders(store);
    s.current = None;
    s.selected = None;
    s.open = None;
    s.rows.clear();
    match parent {
        Some(p) => s.select_folder(store, p)?,
        None => s.select_default_folder(store),
    }
    ok()
}

fn mark_all_read(s: &mut Session, _: &Value) -> CmdResult {
    let (store, folder) = s.current.ok_or(StoreError::NoFolder)?;
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    let unread: Vec<u64> = st.list(folder)?.into_iter().filter(|r| !r.read).map(|r| r.id).collect();
    for id in unread {
        let mut item = st.load(folder, ItemId(id))?;
        item.set_read(true);
        st.update(folder, ItemId(id), item)?;
    }
    if let Some(open) = s.open.as_mut() {
        open.set_read(true);
    }
    s.refresh_folders(store);
    s.reload_rows()?;
    ok()
}

fn empty(s: &mut Session, _: &Value) -> CmdResult {
    let (store, folder) = s.current.ok_or(StoreError::NoFolder)?;
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    let ids: Vec<u64> = st.list(folder)?.into_iter().map(|r| r.id).collect();
    let count = ids.len();
    for id in ids {
        st.remove(folder, ItemId(id))?;
    }
    s.selected = None;
    s.open = None;
    s.refresh_folders(store);
    s.reload_rows()?;
    Ok(json!({ "removed": count }))
}

fn properties(s: &mut Session, _: &Value) -> CmdResult {
    let (store, _) = s.current.ok_or(StoreError::NoFolder)?;
    let f = s.current_folder().ok_or(StoreError::NoFolder)?;
    Ok(
        json!({ "store": store, "id": f.id.0, "name": f.name, "kind": format!("{:?}", f.kind), "class": f.container_class, "total": f.total, "unread": f.unread }),
    )
}
