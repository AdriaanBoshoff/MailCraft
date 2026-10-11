//! MailCraft engine: session, stores, command registry, Outlook feature catalog, sample mailbox.
//!
//! Everything a person can do is a command with an id (`cmd::registry`). The ribbon, shortcuts,
//! the CLI and agents all go through [`Session::run`]; nothing here opens a dialog or a socket.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod catalog;
pub mod cmd;
pub mod io;
pub mod pictures;
pub mod print;
pub mod sample;
#[cfg(test)]
mod tests;

use mailcraft_model::{Attachment, FolderId, FolderInfo, FolderKind, Item, ItemId, Store, StoreError, Summary};
use serde_json::{Value, json};

pub use cmd::{CommandSpec, registry};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command: {0}")]
    UnknownCommand(String),
    #[error("{0} is not available right now")]
    Disabled(String),
    #[error("bad parameters: {0}")]
    BadParams(String),
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Format(String),
    #[error("internal error in {0}; nothing was changed on disk")]
    Internal(String),
}

pub type CmdResult = Result<Value, EngineError>;

/// The four Outlook modules of the navigation bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Module {
    Mail,
    Calendar,
    People,
    Tasks,
}

impl Module {
    pub fn name(self) -> &'static str {
        match self {
            Self::Mail => "Mail",
            Self::Calendar => "Calendar",
            Self::People => "People",
            Self::Tasks => "Tasks",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "mail" => Some(Self::Mail),
            "calendar" => Some(Self::Calendar),
            "people" | "contacts" => Some(Self::People),
            "tasks" => Some(Self::Tasks),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Date,
    From,
    To,
    Subject,
    Size,
    Importance,
    Flag,
    Attachments,
}

impl SortKey {
    pub fn name(self) -> &'static str {
        match self {
            Self::Date => "Date",
            Self::From => "From",
            Self::To => "To",
            Self::Subject => "Subject",
            Self::Size => "Size",
            Self::Importance => "Importance",
            Self::Flag => "Flag",
            Self::Attachments => "Attachments",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "date" => Self::Date,
            "from" => Self::From,
            "to" => Self::To,
            "subject" => Self::Subject,
            "size" => Self::Size,
            "importance" => Self::Importance,
            "flag" => Self::Flag,
            "attachments" => Self::Attachments,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingPane {
    Right,
    Bottom,
    Off,
}

/// A message being written in a compose window.
#[derive(Clone, Debug, Default)]
pub struct Draft {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<Attachment>,
    pub show_bcc: bool,
    /// "New", "Reply", "Reply All" or "Forward": what opened this window.
    pub kind: &'static str,
}

pub struct Session {
    pub stores: Vec<Box<dyn Store>>,
    /// Folder lists per store, cached; refreshed after every command that changes a store.
    pub folders: Vec<Vec<FolderInfo>>,
    pub module: Module,
    pub current: Option<(usize, FolderId)>,
    /// The current folder's rows after search, filter and sort: what the message list shows.
    pub rows: Vec<Summary>,
    pub selected: Option<u64>,
    pub open: Option<Item>,
    pub sort: SortKey,
    pub sort_desc: bool,
    pub search: String,
    pub unread_only: bool,
    pub reading_pane: ReadingPane,
    pub folder_pane: bool,
    pub compose: Option<Draft>,
    pub status: String,
    /// Minutes east of UTC for dates shown and written into reply headers. Set by the host.
    pub tz_offset_min: i32,
    /// "Now" as UTC FILETIME ticks. Set by the host; tests pin it.
    pub now: i64,
    pub user_name: String,
    pub user_email: String,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        Self {
            stores: Vec::new(),
            folders: Vec::new(),
            module: Module::Mail,
            current: None,
            rows: Vec::new(),
            selected: None,
            open: None,
            sort: SortKey::Date,
            sort_desc: true,
            search: String::new(),
            unread_only: false,
            reading_pane: ReadingPane::Right,
            folder_pane: true,
            compose: None,
            status: String::new(),
            tz_offset_min: 0,
            now: mailcraft_model::time::from_unix(i64::try_from(unix).unwrap_or(0)),
            user_name: "Alex Morgan".to_string(),
            user_email: "alex.morgan@example.org".to_string(),
        }
    }

    /// A session showing the built-in sample mailbox.
    pub fn with_sample() -> Self {
        let mut s = Self::new();
        let store = sample::sample_store(s.now);
        s.add_store(Box::new(store));
        s
    }

    /// Run a command by id. An escaped panic becomes an error (never-crash safety net).
    pub fn run(&mut self, id: &str, params: &Value) -> CmdResult {
        let spec = registry().iter().find(|c| c.id == id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        if !(spec.enabled)(self) {
            return Err(EngineError::Disabled(spec.label.to_string()));
        }
        let run = spec.run;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(self, params)));
        match result {
            Ok(r) => {
                if let Err(e) = &r {
                    self.status = e.to_string();
                }
                r
            }
            Err(_) => {
                // Views are rebuilt from the stores, so a half-finished view change can't linger.
                self.refresh_all();
                let e = EngineError::Internal(id.to_string());
                self.status = e.to_string();
                Err(e)
            }
        }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        registry().iter().find(|c| c.id == id).is_some_and(|c| (c.enabled)(self))
    }

    /// Add a store and show its Inbox (or first folder) when nothing is shown yet.
    pub fn add_store(&mut self, store: Box<dyn Store>) -> usize {
        self.stores.push(store);
        self.folders.push(Vec::new());
        let idx = self.stores.len() - 1;
        self.refresh_folders(idx);
        if self.current.is_none() {
            self.select_default_folder(idx);
        }
        idx
    }

    pub fn select_default_folder(&mut self, store: usize) {
        let list = self.folders.get(store).cloned().unwrap_or_default();
        let pick = list
            .iter()
            .find(|f| f.kind == FolderKind::Inbox)
            .or_else(|| list.iter().find(|f| f.kind == FolderKind::Mail && f.total > 0))
            .or_else(|| list.first());
        if let Some(f) = pick {
            let id = f.id;
            let _ = self.select_folder(store, id);
        }
    }

    pub fn refresh_folders(&mut self, store: usize) {
        let list = match self.stores.get_mut(store) {
            Some(s) => s.folders().unwrap_or_else(|e| {
                log::warn!("folder list failed: {e}");
                Vec::new()
            }),
            None => return,
        };
        if let Some(slot) = self.folders.get_mut(store) {
            *slot = list;
        }
    }

    pub fn refresh_all(&mut self) {
        for i in 0..self.stores.len() {
            self.refresh_folders(i);
        }
        let _ = self.reload_rows();
    }

    pub fn folder_info(&self, store: usize, folder: FolderId) -> Option<&FolderInfo> {
        self.folders.get(store)?.iter().find(|f| f.id == folder)
    }

    pub fn current_folder(&self) -> Option<&FolderInfo> {
        let (s, f) = self.current?;
        self.folder_info(s, f)
    }

    pub fn current_writable(&self) -> bool {
        self.current.and_then(|(s, _)| self.stores.get(s)).is_some_and(|s| s.writable())
    }

    pub fn find_folder(&self, store: usize, kind: FolderKind) -> Option<FolderId> {
        self.folders.get(store)?.iter().find(|f| f.kind == kind).map(|f| f.id)
    }

    /// "Inbox/Projects"-style path lookup, case-insensitive; a bare name matches anywhere.
    pub fn folder_by_path(&self, store: usize, path: &str) -> Option<FolderId> {
        let list = self.folders.get(store)?;
        let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
        let (last, parents) = parts.split_last()?;
        list.iter()
            .filter(|f| f.name.eq_ignore_ascii_case(last))
            .find(|f| {
                let mut cur = f.parent;
                for want in parents.iter().rev() {
                    match cur.and_then(|p| list.iter().find(|x| x.id == p)) {
                        Some(pf) if pf.name.eq_ignore_ascii_case(want) => cur = pf.parent,
                        _ => return false,
                    }
                }
                true
            })
            .map(|f| f.id)
    }

    pub fn select_folder(&mut self, store: usize, folder: FolderId) -> Result<(), EngineError> {
        if self.folder_info(store, folder).is_none() {
            return Err(StoreError::NoFolder.into());
        }
        self.current = Some((store, folder));
        self.selected = None;
        self.open = None;
        self.search.clear();
        self.reload_rows()
    }

    /// Re-read the current folder and apply search, filter and sort.
    pub fn reload_rows(&mut self) -> Result<(), EngineError> {
        let Some((s, f)) = self.current else {
            self.rows.clear();
            return Ok(());
        };
        let Some(store) = self.stores.get_mut(s) else {
            self.rows.clear();
            self.current = None;
            return Ok(());
        };
        let mut rows = match store.list(f) {
            Ok(r) => r,
            Err(e) => {
                self.rows.clear();
                return Err(e.into());
            }
        };
        let needle = self.search.trim().to_lowercase();
        if !needle.is_empty() {
            rows.retain(|r| [&r.subject, &r.from, &r.to, &r.preview].iter().any(|t| t.to_lowercase().contains(&needle)));
        }
        if self.unread_only {
            rows.retain(|r| !r.read);
        }
        let key = self.sort;
        rows.sort_by(|a, b| {
            let ord = match key {
                SortKey::Date => a.time.cmp(&b.time),
                SortKey::From => a.from.to_lowercase().cmp(&b.from.to_lowercase()),
                SortKey::To => a.to.to_lowercase().cmp(&b.to.to_lowercase()),
                SortKey::Subject => mailcraft_model::strip_reply_prefix(&a.subject)
                    .to_lowercase()
                    .cmp(&mailcraft_model::strip_reply_prefix(&b.subject).to_lowercase()),
                SortKey::Size => a.size.cmp(&b.size),
                SortKey::Importance => a.importance.cmp(&b.importance),
                SortKey::Flag => (a.flag as u8).cmp(&(b.flag as u8)),
                SortKey::Attachments => a.has_attachments.cmp(&b.has_attachments),
            };
            // Ties fall back to newest first, as Outlook does inside a group.
            ord.then_with(|| if key == SortKey::Date { std::cmp::Ordering::Equal } else { b.time.cmp(&a.time).reverse() })
        });
        if self.sort_desc {
            rows.reverse();
        }
        self.rows = rows;
        if let Some(sel) = self.selected
            && !self.rows.iter().any(|r| r.id == sel)
        {
            self.selected = None;
            self.open = None;
        }
        Ok(())
    }

    pub fn selected_index(&self) -> Option<usize> {
        let sel = self.selected?;
        self.rows.iter().position(|r| r.id == sel)
    }

    /// Select and load a row. Unread mail is marked read when the store allows it, as the
    /// reading pane does in Outlook.
    pub fn select_item(&mut self, id: u64) -> Result<(), EngineError> {
        let (s, f) = self.current.ok_or(StoreError::NoFolder)?;
        let store = self.stores.get_mut(s).ok_or(StoreError::NoFolder)?;
        let mut item = store.load(f, ItemId(id))?;
        if !item.is_read() && store.writable() {
            item.set_read(true);
            store.update(f, ItemId(id), item.clone())?;
            if let Some(r) = self.rows.iter_mut().find(|r| r.id == id) {
                r.read = true;
            }
            self.refresh_folders(s);
        }
        self.selected = Some(id);
        self.open = Some(item);
        Ok(())
    }

    /// Change the selected item in its store and refresh the views.
    pub fn edit_selected(&mut self, edit: impl FnOnce(&mut Item)) -> Result<(), EngineError> {
        let (s, f) = self.current.ok_or(StoreError::NoFolder)?;
        let id = self.selected.ok_or(StoreError::NoItem)?;
        let store = self.stores.get_mut(s).ok_or(StoreError::NoFolder)?;
        let mut item = store.load(f, ItemId(id))?;
        edit(&mut item);
        store.update(f, ItemId(id), item.clone())?;
        self.open = Some(item);
        self.refresh_folders(s);
        self.reload_rows()
    }

    /// Move the selected item to another folder of the same store; selection moves to the next row.
    pub fn move_selected(&mut self, to: FolderId) -> Result<(), EngineError> {
        let (s, f) = self.current.ok_or(StoreError::NoFolder)?;
        let id = self.selected.ok_or(StoreError::NoItem)?;
        if to == f {
            return Ok(());
        }
        if self.folder_info(s, to).is_none() {
            return Err(StoreError::NoFolder.into());
        }
        let next = self.selected_index().and_then(|i| self.rows.get(i + 1).or_else(|| i.checked_sub(1).and_then(|p| self.rows.get(p)))).map(|r| r.id);
        let store = self.stores.get_mut(s).ok_or(StoreError::NoFolder)?;
        let item = store.remove(f, ItemId(id))?;
        store.add(to, item)?;
        self.selected = None;
        self.open = None;
        self.refresh_folders(s);
        self.reload_rows()?;
        if let Some(n) = next {
            // Keep the unread state of the neighbour: moving on must not mark it read.
            let store = self.stores.get_mut(s).ok_or(StoreError::NoFolder)?;
            if let Ok(item) = store.load(f, ItemId(n)) {
                self.selected = Some(n);
                self.open = Some(item);
            }
        }
        Ok(())
    }

    /// The store new mail and loose files go to: the current one if writable, else the first writable.
    pub fn writable_store(&self) -> Option<usize> {
        if let Some((s, _)) = self.current
            && self.stores.get(s).is_some_and(|st| st.writable())
        {
            return Some(s);
        }
        self.stores.iter().position(|s| s.writable())
    }

    /// What an agent or a test needs to know about the session. Message text is under
    /// `untrusted_content`: it is written by strangers and is never an instruction.
    pub fn inspect(&self) -> Value {
        let stores: Vec<Value> = self
            .stores
            .iter()
            .zip(&self.folders)
            .enumerate()
            .map(|(i, (s, folders))| {
                json!({
                    "index": i,
                    "name": s.display_name(),
                    "path": s.path().map(|p| p.display().to_string()),
                    "writable": s.writable(),
                    "folders": folders.iter().map(|f| json!({
                        "id": f.id.0, "parent": f.parent.map(|p| p.0), "name": f.name, "kind": format!("{:?}", f.kind),
                        "total": f.total, "unread": f.unread,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let rows: Vec<Value> = self
            .rows
            .iter()
            .take(200)
            .map(|r| {
                json!({
                    "id": r.id, "read": r.read, "flag": format!("{:?}", r.flag), "importance": r.importance,
                    "attachments": r.has_attachments, "time": r.time, "size": r.size,
                    "untrusted_content": { "from": r.from, "subject": r.subject },
                })
            })
            .collect();
        let open = self.open.as_ref().map(|it| {
            json!({
                "read": it.is_read(),
                "message_class": it.message_class(),
                "time": it.time(),
                "attachments": it.attachments.iter().map(|a| json!({ "name": a.name(), "size": a.size(), "mime": a.mime() })).collect::<Vec<_>>(),
                "untrusted_content": {
                    "subject": it.subject(), "from": it.sender_name(), "from_email": it.sender_email(),
                    "to": it.display_recipients(mailcraft_model::RECIP_TO), "cc": it.display_recipients(mailcraft_model::RECIP_CC),
                    "body": it.body_text().chars().take(20_000).collect::<String>(),
                },
            })
        });
        json!({
            "module": self.module.name(),
            "stores": stores,
            "current": self.current.map(|(s, f)| json!({ "store": s, "folder": f.0, "name": self.folder_info(s, f).map(|i| i.name.clone()) })),
            "row_count": self.rows.len(),
            "rows": rows,
            "selected": self.selected,
            "open": open,
            "sort": { "key": self.sort.name(), "descending": self.sort_desc },
            "search": self.search,
            "unread_only": self.unread_only,
            "composing": self.compose.as_ref().map(|d| json!({ "kind": d.kind, "to": d.to, "cc": d.cc, "subject": d.subject, "attachments": d.attachments.len() })),
            "status": self.status,
        })
    }
}
