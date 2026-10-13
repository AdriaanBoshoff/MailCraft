//! OpenMailClient engine: session, stores, command registry, Outlook feature catalog, sample mailbox.
//!
//! Everything a person can do is a command with an id (`cmd::registry`). The ribbon, shortcuts,
//! the CLI and agents all go through [`Session::run`]; nothing here opens a dialog or a socket.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod arrange;
pub mod attachments;
pub mod catalog;
pub mod cmd;
pub mod io;
pub mod links;
pub mod outgoing;
pub mod pictures;
pub mod print;
pub mod sample;
pub mod search;
#[cfg(test)]
mod tests;
pub mod undo;

use openmailclient_model::{Attachment, FolderId, FolderInfo, FolderKind, Item, ItemId, Store, StoreError, Summary};
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
    /// The message this answers reached its writer as a hidden recipient, and it answers all.
    pub was_hidden: bool,
    /// Its writer has put something into it.
    pub touched: bool,
}

/// A save of a data file that is being done on a thread of its own.
pub struct Saving {
    /// Which data file, by its place in `Session::stores`; no data file is closed meanwhile.
    pub store: usize,
    pub name: String,
    pub progress: std::sync::Arc<openmailclient_model::Progress>,
    done: std::sync::mpsc::Receiver<Result<openmailclient_model::SaveDone, StoreError>>,
}

impl Saving {
    pub(crate) fn new(
        store: usize,
        name: String,
        progress: std::sync::Arc<openmailclient_model::Progress>,
        done: std::sync::mpsc::Receiver<Result<openmailclient_model::SaveDone, StoreError>>,
    ) -> Self {
        Self { store, name, progress, done }
    }
}

impl Drop for Saving {
    fn drop(&mut self) {
        // Nobody is left to end it: it stops, and writes nothing over the data file.
        self.progress.stop();
    }
}

/// An export of a data file that is being done on a thread of its own.
pub struct Exporting {
    /// Which data file, by its place in `Session::stores`; no data file is closed meanwhile.
    pub store: usize,
    /// The file being exported to.
    pub to: std::path::PathBuf,
    /// The file beside it that is written first.
    tmp: std::path::PathBuf,
    id: [u8; 16],
    pub progress: std::sync::Arc<openmailclient_model::Progress>,
    done: std::sync::mpsc::Receiver<Result<io::Saved, EngineError>>,
}

impl Exporting {
    pub(crate) fn new(
        store: usize,
        to: std::path::PathBuf,
        tmp: std::path::PathBuf,
        id: [u8; 16],
        progress: std::sync::Arc<openmailclient_model::Progress>,
        done: std::sync::mpsc::Receiver<Result<io::Saved, EngineError>>,
    ) -> Self {
        Self { store, to, tmp, id, progress, done }
    }
}

impl Drop for Exporting {
    fn drop(&mut self) {
        // Nobody is left to end it: it stops, and what it wrote beside the target goes. It is
        // waited for, so that no file is finished after this has looked.
        self.progress.stop();
        if self.done.recv().is_ok_and(|done| done.is_ok()) {
            let _ = std::fs::remove_file(&self.tmp);
        }
    }
}

pub struct Session {
    /// Saves of data files that are being done away from the caller: `file.save` with
    /// `"wait": false` starts them, `Session::poll` ends them.
    pub saves: Vec<Saving>,
    /// Exports that are being done away from the caller, likewise (`file.export`).
    pub exports: Vec<Exporting>,
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
    /// What was done to messages and can be taken back, the newest last.
    pub undo: Vec<undo::Step>,
    /// Messages that were being written and were discarded or made way for another, the newest
    /// last: `compose.restore` brings one back.
    pub set_aside: Vec<Draft>,
    /// How pages are set up for printing: `file.pageSetup` changes it.
    pub page_setup: openmailclient_layout::Setup,
    /// What is printed: the open message, or the folder's list. `file.pageSetup` changes it.
    pub print_style: print::Style,
    /// A folder of the program's own where the journal of a data file's unsaved changes is
    /// kept when it cannot be kept beside the file. Set by the host; `None` keeps none there.
    pub journal_dir: Option<std::path::PathBuf>,
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
            saves: Vec::new(),
            exports: Vec::new(),
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
            now: openmailclient_model::time::from_unix(i64::try_from(unix).unwrap_or(0)),
            user_name: "Alex Morgan".to_string(),
            user_email: "alex.morgan@example.org".to_string(),
            undo: Vec::new(),
            set_aside: Vec::new(),
            page_setup: openmailclient_layout::Setup::default(),
            journal_dir: None,
            print_style: print::Style::default(),
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
        let query = search::parse(&self.search, self.tz_offset_min);
        if !query.is_empty() {
            rows.retain(|r| query.matches(r));
        }
        if self.unread_only {
            rows.retain(|r| !r.read);
        }
        arrange::sort(&mut rows, self.sort, self.sort_desc);
        self.rows = rows;
        if let Some(sel) = self.selected
            && !self.rows.iter().any(|r| r.id == sel)
        {
            self.selected = None;
            self.open = None;
        }
        Ok(())
    }

    /// The groups the listed rows stand in, as they are arranged.
    pub fn groups(&self) -> Vec<arrange::Group> {
        arrange::groups(&self.rows, self.sort, self.now, self.tz_offset_min)
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
        // While its data file is being saved a message is read without being marked read:
        // reading is never held up by a save.
        if !item.is_read() && store.writable() && !store.saving() {
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
    /// `what` is the command's label, for taking it back.
    pub fn edit_selected(&mut self, what: &'static str, edit: impl FnOnce(&mut Item)) -> Result<(), EngineError> {
        let (s, f) = self.current.ok_or(StoreError::NoFolder)?;
        let id = self.selected.ok_or(StoreError::NoItem)?;
        let store = self.stores.get_mut(s).ok_or(StoreError::NoFolder)?;
        let mut item = store.load(f, ItemId(id))?;
        let before = Box::new(item.clone());
        edit(&mut item);
        store.update(f, ItemId(id), item.clone())?;
        self.open = Some(item);
        self.did(undo::Step::Edited { what, store: s, folder: f, id, before });
        self.refresh_folders(s);
        self.reload_rows()
    }

    /// Move the selected item to another folder of the same store; selection moves to the next row.
    /// `what` is the command's label, for taking it back.
    pub fn move_selected(&mut self, what: &'static str, to: FolderId) -> Result<(), EngineError> {
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
        let moved = match store.add(to, item.clone()) {
            Ok(moved) => moved.0,
            Err(e) => {
                // A message that could not be put where it was going goes back where it was.
                store.add(f, item)?;
                self.refresh_folders(s);
                self.reload_rows()?;
                return Err(e.into());
            }
        };
        self.did(undo::Step::Moved { what, store: s, from: f, was: id, to, id: moved });
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

    /// The settings kept between runs, as `settings.save` writes them. `ui` is what the
    /// window keeps for itself.
    pub fn settings(&self, ui: Option<&Value>) -> Value {
        cmd::settings::current(self, ui)
    }

    /// True when unsaved changes to a data file are in memory only: not also in a journal
    /// that would bring them back after a run that ends badly.
    pub fn unjournalled(&self) -> bool {
        self.stores.iter().any(|s| s.unsaved() && s.journal_trouble().is_some())
    }

    /// Have the disk keep what the journals of unsaved changes hold so far. The host asks for
    /// it now and then (it has the clock): a journal is otherwise forced to disk every so
    /// many changes only. True when there was something to keep.
    pub fn settle(&mut self) -> bool {
        // Every one of them, not only up to the first that had something.
        let mut any = false;
        for store in &mut self.stores {
            any |= store.settle();
        }
        any
    }

    /// True when a data file has changes that are not in the file yet.
    pub fn unsaved(&self) -> bool {
        self.stores.iter().any(|s| s.unsaved())
    }

    /// End the saves whose work is done: each new file takes its data file's place, or the
    /// status says why not. The host asks every so often while `saves` is not empty. True
    /// when one ended.
    pub fn poll(&mut self) -> bool {
        let saved = self.end_saves(false);
        self.end_exports(false) || saved
    }

    /// Wait for every save and export that is running, and end it. The host asks before it
    /// leaves.
    pub fn finish_saves(&mut self) {
        self.end_saves(true);
        self.end_exports(true);
    }

    fn end_exports(&mut self, wait: bool) -> bool {
        let mut ended = false;
        let mut at = 0;
        while let Some(export) = self.exports.get(at) {
            let lost = || Err(EngineError::Format("the export ended without saying how".to_string()));
            let done = if wait {
                export.done.recv().unwrap_or_else(|_| lost())
            } else {
                match export.done.try_recv() {
                    Ok(done) => done,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        at += 1;
                        continue;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => lost(),
                }
            };
            let export = self.exports.remove(at);
            ended = true;
            let to = export.to.clone();
            // A data file that was opened here meanwhile is not written over.
            let open_here = std::fs::canonicalize(&to)
                .is_ok_and(|real| self.stores.iter().any(|st| st.path().and_then(|p| std::fs::canonicalize(p).ok()).is_some_and(|p| p == real)));
            let done = match done {
                Ok(_) if open_here => {
                    let _ = std::fs::remove_file(&export.tmp);
                    Err(EngineError::BadParams("that data file was opened here meanwhile".into()))
                }
                Ok(saved) => io::export_end(&export.tmp, &to).map(|()| saved),
                Err(e) if export.progress.stopped() => Err(e),
                // What cannot be copied as it is stored is written from what is read, here.
                Err(_) => match self.stores.get_mut(export.store) {
                    Some(store) => io::save_store(store.as_mut(), &to, export.id),
                    None => lost(),
                },
            };
            self.status = match done {
                Ok(saved) => saved.says(&to),
                Err(_) if export.progress.stopped() => format!("The export to {} was stopped; nothing was written there", to.display()),
                Err(e) => format!("Nothing was exported to {}: {e}", to.display()),
            };
        }
        ended
    }

    /// What is being saved or exported on a thread of its own, in words, and how far it is
    /// from 0 to 1; `None` when nothing is.
    pub fn working(&self) -> Option<(String, f32)> {
        if let Some(done) = self.saving() {
            return Some((format!("Saving {}", self.saves.first().map_or("", |s| s.name.as_str())), done));
        }
        let export = self.exports.first()?;
        let name = export.to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Some((format!("Exporting to {name}"), export.progress.fraction().clamp(0.0, 1.0)))
    }

    fn end_saves(&mut self, wait: bool) -> bool {
        let mut ended = false;
        let mut at = 0;
        while let Some(saving) = self.saves.get(at) {
            let lost = || Err(StoreError::Format("the save ended without saying how; nothing was replaced".to_string()));
            let done = if wait {
                saving.done.recv().unwrap_or_else(|_| lost())
            } else {
                match saving.done.try_recv() {
                    Ok(done) => done,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        at += 1;
                        continue;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => lost(),
                }
            };
            let saving = self.saves.remove(at);
            ended = true;
            let Some(store) = self.stores.get_mut(saving.store) else { continue };
            self.status = match store.save_end(done) {
                Ok(()) => format!("Saved {}", saving.name),
                // Stopping was asked for: not a failure, and the file is as it was.
                Err(_) if saving.progress.stopped() => format!("Saving {} was stopped; its changes are still unsaved", saving.name),
                Err(e) => format!("{} was not saved: {e}", saving.name),
            };
            self.refresh_folders(saving.store);
        }
        ended
    }

    /// How far the saves that are running are, from 0 to 1; `None` when none is.
    pub fn saving(&self) -> Option<f32> {
        let least = self.saves.iter().map(|s| s.progress.fraction()).fold(f32::INFINITY, f32::min);
        (!self.saves.is_empty()).then_some(least.clamp(0.0, 1.0))
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
                    "unsaved": s.unsaved(),
                    "journalled": s.unsaved() && s.journal_trouble().is_none(),
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
                "attachments": it.attachments.iter().map(|a| json!({
                    "name": a.name(), "size": a.size(), "mime": a.mime(), "file": attachments::is_file(a),
                    "caution": attachments::caution_of(a).map(|c| c.words()),
                })).collect::<Vec<_>>(),
                "untrusted_content": {
                    "subject": it.subject(), "from": it.sender_name(), "from_email": it.sender_email(),
                    "to": it.display_recipients(openmailclient_model::RECIP_TO), "cc": it.display_recipients(openmailclient_model::RECIP_CC),
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
            "groups": self.groups().iter().map(|g| json!({
                "start": g.start, "count": g.count, "unread": g.unread, "untrusted_content": { "label": g.label },
            })).collect::<Vec<_>>(),
            "search": self.search,
            "unread_only": self.unread_only,
            "undo": self.undo.last().map(|step| step.what()),
            "setAside": self.set_aside.len(),
            "composing": self.compose.as_ref().map(|d| json!({
                "kind": d.kind, "to": d.to, "cc": d.cc, "subject": d.subject, "attachments": d.attachments.len(),
                "cautions": outgoing::cautions(d).iter().map(|c| c.words()).collect::<Vec<_>>(),
            })),
            "status": self.status,
        })
    }
}
