//! Changes to an open data file.
//!
//! Nothing is written while changes are made: they are kept beside the open file, which stays
//! as it was. Saving writes a whole new file. Every node the changes do not touch is copied as
//! it is stored, block for block, so what this program does not understand (hidden items,
//! views, search folders, values of types the model has no place for) goes across untouched.
//! A changed message keeps every stored property that was not changed. The new file is checked,
//! read back and only then put in the old one's place.

use crate::journal::{self, Diff, Found, Journal, Op};
use crate::ltp::{self, Raw, Row};
use crate::ltp_write::{Subs, encode, write_pc};
use crate::ndb::{NbtEntry, Ndb, Subnodes, Ver};
use crate::store_write::{ASSOC, CONTENTS, CONTENTS_MORE, HIERARCHY, MAX_CELL, MAX_INDEX, Names, Out, Written, copy, count, flag, int, text};
use crate::write::{NdbWriter, Sub};
use crate::{LTP_INTERNAL, NID_ATTACHMENT_TABLE, NID_NAME_TO_ID_MAP, NID_RECIPIENT_TABLE, PstError, PstStore};
use openmailclient_model::{
    FolderId, FolderInfo, FolderKind, Item, ItemId, MSGFLAG_HASATTACH, NamedKey, PropValue, Recovery, StoreError, Summary, pid,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Node types ([MS-PST] 2.2.2.1).
const TYPE_FOLDER: u32 = 0x02;
const TYPE_MESSAGE: u32 = 0x04;
const TYPE_ATTACHMENT: u32 = 0x05;
const TYPE_HIERARCHY: u32 = 0x0D;
const TYPE_CONTENTS: u32 = 0x0E;
const TYPE_ASSOC: u32 = 0x0F;
const TYPE_SEARCH_CONTENTS: u32 = 0x10;
const TYPE_LTP: u32 = 0x1F;

/// Subnodes are copied this deep; a message attached to a message goes two levels down.
const MAX_COPY_DEPTH: usize = 40;
/// Messages taken out of a folder are remembered this many at a time, so that one put into
/// another folder is the same message still.
const MAX_LIMBO: usize = 32;

/// One message of a folder that was looked into.
struct Entry {
    nid: u32,
    summary: Summary,
    /// The message as changed here; `None` while it is as the file has it.
    item: Option<Box<Item>>,
    /// True when the file has no such message yet.
    new: bool,
}

struct EFolder {
    info: FolderInfo,
    nid: u32,
    new: bool,
    /// The messages, once the folder was looked into; `None` while they are as the file has them.
    items: Option<Vec<Entry>>,
    /// What the file lists in this folder, when `items` is there.
    had: Vec<u32>,
}

impl EFolder {
    fn recount(&mut self) {
        if let Some(items) = &self.items {
            self.info.total = u32::try_from(items.len()).unwrap_or(u32::MAX);
            self.info.unread = u32::try_from(items.iter().filter(|e| !e.summary.read).count()).unwrap_or(u32::MAX);
        }
    }

    /// True when the folder's list of messages is not what the file has.
    fn contents_changed(&self) -> bool {
        self.new
            || self.items.as_ref().is_some_and(|items| {
                items.len() != self.had.len() || items.iter().zip(&self.had).any(|(e, had)| e.nid != *had || e.item.is_some() || e.new)
            })
    }
}

/// What a folder was when the file was opened.
struct Was {
    parent: Option<u32>,
    name: String,
    has_children: bool,
}

/// The changes made to an open data file and not yet saved.
pub(crate) struct Edits {
    /// The visible folders, each followed by what is inside it.
    folders: Vec<EFolder>,
    was: HashMap<u32, Was>,
    limbo: Vec<(Entry, Item)>,
    /// The next index for the node of a new folder or message.
    next: u32,
    pub(crate) dirty: bool,
}

impl Edits {
    fn at(&self, folder: FolderId) -> Result<usize, StoreError> {
        self.folders.iter().position(|f| f.info.id == folder).ok_or(StoreError::NoFolder)
    }

    fn fresh(&mut self, ty: u32) -> Result<u32, StoreError> {
        let index = self.next;
        if index > MAX_INDEX {
            return Err(StoreError::Format("the data file has no room for another item".into()));
        }
        self.next = index + 1;
        Ok(index << 5 | ty)
    }

    /// A folder and every folder inside it, as places in the list, in list order.
    fn subtree(&self, folder: FolderId) -> Vec<FolderId> {
        let mut all = vec![folder];
        let mut i = 0;
        while let Some(&cur) = all.get(i) {
            let inside: Vec<FolderId> =
                self.folders.iter().filter(|f| f.info.parent == Some(cur) && !all.contains(&f.info.id)).map(|f| f.info.id).collect();
            all.extend(inside);
            i += 1;
        }
        all
    }

    fn mark_parents(&mut self) {
        let parents: HashSet<FolderId> = self.folders.iter().filter_map(|f| f.info.parent).collect();
        for f in &mut self.folders {
            f.info.has_children = parents.contains(&f.info.id);
        }
    }

    /// Where a folder put under `parent` goes in the list: after the last folder inside it.
    fn end_of(&self, parent: FolderId) -> usize {
        let mut under = vec![parent];
        let mut after = 0;
        for (at, f) in self.folders.iter().enumerate() {
            if f.info.id == parent || f.info.parent.is_some_and(|p| under.contains(&p)) {
                under.push(f.info.id);
                after = at + 1;
            }
        }
        after.min(self.folders.len())
    }
}

fn nid_of(id: u64) -> Option<u32> {
    u32::try_from(id).ok()
}

/// A file's length and when it was last written: enough to see that another program wrote it.
fn stamp(path: &Path) -> Option<(u64, std::time::SystemTime)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

impl PstStore {
    /// True when changes to this data file can be kept: a Unicode `.pst`. An `.ost` is a copy
    /// of a mailbox on a server and is only ever read.
    pub(crate) fn can_edit(&self) -> bool {
        self.ndb.ver == Ver::Unicode && !self.path.as_deref().is_some_and(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ost")))
    }

    pub(crate) fn note_stamp(&mut self) {
        self.stamp = self.path.as_deref().and_then(stamp);
        self.ident = self.path.as_deref().and_then(journal::ident);
    }

    /// Write a request that was done into the journal.
    fn log(&mut self, op: &Op) {
        self.journal.log(self.path.as_deref(), self.ident, op);
    }

    pub(crate) fn e_journal_trouble(&self) -> Option<String> {
        self.journal.trouble.clone().filter(|_| self.e_unsaved())
    }

    /// Throw the unsaved changes away, and their journal with them.
    pub(crate) fn e_discard(&mut self) {
        self.edits = None;
        self.journal.finish(self.path.as_deref());
    }

    /// Bring back the changes a run that ended without saving left in the journal beside the
    /// file. They are unsaved again, as they were; the data file is not written. Playing back
    /// stops at the first change that is damaged or cannot be made; when any is lost that
    /// way, the journal as it was found is kept beside the file under another name.
    pub fn recover(&mut self) -> Result<Recovery, PstError> {
        let mut out = Recovery::default();
        let (Some(path), Some(ident)) = (self.path.clone(), self.ident) else { return Ok(out) };
        if !self.can_edit() || self.e_unsaved() || self.journal.open() {
            return Ok(out);
        }
        match Journal::read(&path, ident)? {
            Found::Nothing => {}
            Found::Stale => out.stale = true,
            Found::Held => {
                out.held = true;
                self.journal.trouble = Some("another running OpenMailClient has this data file's journal".to_string());
            }
            Found::Records { mut file, records, torn } => {
                let total = records.len();
                let (mut end, mut applied) = (journal::HEADER, 0usize);
                self.journal.playing = true;
                for (payload, ends) in records {
                    if Op::decode(&payload).and_then(|op| self.play(op).ok()).is_none() {
                        break;
                    }
                    (end, applied) = (ends, applied.saturating_add(1));
                }
                self.journal.playing = false;
                out.applied = u32::try_from(applied).unwrap_or(u32::MAX);
                out.lost = u32::try_from(total.saturating_sub(applied)).unwrap_or(u32::MAX).saturating_add(u32::from(torn && applied == total));
                if out.lost > 0 {
                    journal::keep_aside(&mut file, &path);
                }
                if applied == 0 {
                    // Nothing to go on from: no journal until there is a change to write.
                    self.edits = None;
                    drop(file);
                    let _ = std::fs::remove_file(journal::journal_beside(&path));
                } else if let Err(e) = self.journal.resume(file, end, out.applied) {
                    self.journal.trouble = Some(format!("the journal could not be gone on with ({e})"));
                }
            }
        }
        Ok(out)
    }

    /// Do one journalled request again.
    fn play(&mut self, op: Op) -> Result<(), StoreError> {
        match op {
            Op::Update { folder, id, diff } => {
                let (folder, id) = (FolderId(folder), ItemId(id));
                self.looked_into(folder)?;
                let mut item = self.e_load(folder, id)?;
                diff.apply(&mut item);
                self.e_update(folder, id, item)
            }
            Op::Add { folder, item } => self.e_add(FolderId(folder), item).map(|_| ()),
            Op::Put { folder, nid } => {
                let at = self.looked_into(FolderId(folder))?;
                let edits = self.edits()?;
                let i = edits.limbo.iter().position(|(e, _)| e.nid == nid).ok_or(StoreError::NoItem)?;
                let entry = edits.limbo.swap_remove(i).0;
                self.entries(at)?.push(entry);
                self.touched(at);
                Ok(())
            }
            Op::Remove { folder, id } => self.e_remove(FolderId(folder), ItemId(id)).map(|_| ()),
            Op::CreateFolder { parent, name } => self.e_create_folder(FolderId(parent), &name).map(|_| ()),
            Op::RenameFolder { folder, name } => self.e_rename_folder(FolderId(folder), &name),
            Op::DeleteFolder { folder } => self.e_delete_folder(FolderId(folder)),
            Op::MoveFolder { folder, parent } => self.e_move_folder(FolderId(folder), FolderId(parent)),
        }
    }

    fn edits(&mut self) -> Result<&mut Edits, StoreError> {
        if !self.can_edit() {
            return Err(StoreError::ReadOnly);
        }
        if self.edits.is_none() {
            let listed = self.file_folders()?;
            let top = self.ndb.nbt.keys().map(|n| n >> 5).max().unwrap_or(0);
            let mut was = HashMap::new();
            let mut folders = Vec::with_capacity(listed.len());
            for info in listed {
                let Some(nid) = nid_of(info.id.0) else { continue };
                was.insert(nid, Was { parent: info.parent.and_then(|p| nid_of(p.0)), name: info.name.clone(), has_children: info.has_children });
                folders.push(EFolder { info, nid, new: false, items: None, had: Vec::new() });
            }
            let mut edits = Edits { folders, was, limbo: Vec::new(), next: top.saturating_add(1).max(0x1_0000), dirty: false };
            // What the file says of subfolders may be stale; the list is what counts.
            edits.mark_parents();
            for f in &edits.folders {
                if let Some(w) = edits.was.get_mut(&f.nid) {
                    w.has_children = f.info.has_children;
                }
            }
            self.edits = Some(edits);
        }
        self.edits.as_mut().ok_or(StoreError::ReadOnly)
    }

    /// The place of a folder in the list of changes, its messages looked into.
    fn looked_into(&mut self, folder: FolderId) -> Result<usize, StoreError> {
        let at = self.edits()?.at(folder)?;
        let known = self.edits.as_ref().and_then(|e| e.folders.get(at)).is_some_and(|f| f.items.is_some());
        if !known {
            let new = self.edits.as_ref().and_then(|e| e.folders.get(at)).is_some_and(|f| f.new);
            let listed = if new { Vec::new() } else { self.file_list(folder)? };
            if let Some(f) = self.edits.as_mut().and_then(|e| e.folders.get_mut(at)) {
                f.had = listed.iter().filter_map(|s| nid_of(s.id)).collect();
                f.items = Some(
                    listed.into_iter().filter_map(|summary| Some(Entry { nid: nid_of(summary.id)?, summary, item: None, new: false })).collect(),
                );
            }
        }
        Ok(at)
    }

    fn entries(&mut self, at: usize) -> Result<&mut Vec<Entry>, StoreError> {
        self.edits.as_mut().and_then(|e| e.folders.get_mut(at)).and_then(|f| f.items.as_mut()).ok_or(StoreError::NoFolder)
    }

    fn touched(&mut self, at: usize) {
        if let Some(e) = self.edits.as_mut() {
            e.dirty = true;
            if let Some(f) = e.folders.get_mut(at) {
                f.recount();
            }
        }
    }

    pub(crate) fn e_unsaved(&self) -> bool {
        self.edits.as_ref().is_some_and(|e| e.dirty)
    }

    pub(crate) fn e_folders(&mut self) -> Result<Vec<FolderInfo>, StoreError> {
        match &self.edits {
            Some(e) => Ok(e.folders.iter().map(|f| f.info.clone()).collect()),
            None => self.file_folders(),
        }
    }

    pub(crate) fn e_list(&mut self, folder: FolderId) -> Result<Vec<Summary>, StoreError> {
        let Some(e) = &self.edits else { return self.file_list(folder) };
        match &e.folders.get(e.at(folder)?).ok_or(StoreError::NoFolder)?.items {
            Some(items) => Ok(items.iter().map(|e| e.summary.clone()).collect()),
            None => self.file_list(folder),
        }
    }

    pub(crate) fn e_load(&mut self, folder: FolderId, id: ItemId) -> Result<Item, StoreError> {
        let Some(e) = &self.edits else { return self.file_load(id) };
        if let Some(items) = &e.folders.get(e.at(folder)?).ok_or(StoreError::NoFolder)?.items {
            let entry = items.iter().find(|e| u64::from(e.nid) == id.0).ok_or(StoreError::NoItem)?;
            if let Some(item) = &entry.item {
                return Ok((**item).clone());
            }
        } else if e.folders.iter().any(|f| f.items.is_some() && f.had.iter().any(|n| u64::from(*n) == id.0)) {
            // It was in a folder that was looked into, and is there no longer.
            return Err(StoreError::NoItem);
        }
        self.file_load(id)
    }

    pub(crate) fn e_update(&mut self, folder: FolderId, id: ItemId, item: Item) -> Result<(), StoreError> {
        let at = self.looked_into(folder)?;
        let on = self.journal.on();
        let entry = self.entries(at)?.iter().find(|e| u64::from(e.nid) == id.0).ok_or(StoreError::NoItem)?;
        // The journal takes what changed in the message, not the message.
        let op = if on {
            let diff = match entry.item.as_deref().map(|was| Diff::between(was, &item)) {
                Some(diff) => Ok(diff),
                None => self.file_load(id).map(|was| Diff::between(&was, &item)),
            };
            match diff {
                Ok(diff) => Some(Op::Update { folder: folder.0, id: id.0, diff }),
                Err(e) => {
                    self.journal.trouble = Some(format!("a message could not be read to journal a change to it ({e})"));
                    None
                }
            }
        } else {
            None
        };
        let entry = self.entries(at)?.iter_mut().find(|e| u64::from(e.nid) == id.0).ok_or(StoreError::NoItem)?;
        entry.summary = item.summary(id.0);
        entry.item = Some(Box::new(item));
        self.touched(at);
        if let Some(op) = op {
            self.log(&op);
        }
        Ok(())
    }

    pub(crate) fn e_add(&mut self, folder: FolderId, item: Item) -> Result<ItemId, StoreError> {
        let at = self.looked_into(folder)?;
        let edits = self.edits()?;
        // A message just taken out of a folder is the same message when it is put into one:
        // it keeps its node, and with it everything the file holds for it.
        let (entry, op) = match edits.limbo.iter().position(|(_, was)| *was == item) {
            Some(i) => {
                let entry = edits.limbo.swap_remove(i).0;
                let op = Op::Put { folder: folder.0, nid: entry.nid };
                (entry, op)
            }
            None => {
                let nid = edits.fresh(TYPE_MESSAGE)?;
                let summary = item.summary(u64::from(nid));
                // The journal's copy is made only when there is a journal to write it to.
                let op = Op::Add { folder: folder.0, item: if self.journal.on() { item.clone() } else { Item::default() } };
                (Entry { nid, summary, item: Some(Box::new(item)), new: true }, op)
            }
        };
        let id = ItemId(u64::from(entry.nid));
        self.entries(at)?.push(entry);
        self.touched(at);
        self.log(&op);
        Ok(id)
    }

    pub(crate) fn e_remove(&mut self, folder: FolderId, id: ItemId) -> Result<Item, StoreError> {
        let at = self.looked_into(folder)?;
        let items = self.entries(at)?;
        let pos = items.iter().position(|e| u64::from(e.nid) == id.0).ok_or(StoreError::NoItem)?;
        let item = match items.get(pos).and_then(|e| e.item.as_deref()) {
            Some(item) => item.clone(),
            None => self.file_load(id)?,
        };
        let entry = self.entries(at)?.remove(pos);
        let edits = self.edits()?;
        if edits.limbo.len() >= MAX_LIMBO {
            edits.limbo.remove(0);
        }
        edits.limbo.push((entry, item.clone()));
        self.touched(at);
        self.log(&Op::Remove { folder: folder.0, id: id.0 });
        Ok(item)
    }

    pub(crate) fn e_create_folder(&mut self, parent: FolderId, name: &str) -> Result<FolderId, StoreError> {
        let edits = self.edits()?;
        let class = edits.folders.get(edits.at(parent)?).map(|f| f.info.container_class.clone()).unwrap_or_default();
        let nid = edits.fresh(TYPE_FOLDER)?;
        let id = FolderId(u64::from(nid));
        let info = FolderInfo {
            id,
            parent: Some(parent),
            name: name.to_string(),
            kind: FolderKind::guess(name, &class),
            container_class: class,
            total: 0,
            unread: 0,
            has_children: false,
        };
        let at = edits.end_of(parent);
        edits.folders.insert(at, EFolder { info, nid, new: true, items: Some(Vec::new()), had: Vec::new() });
        edits.mark_parents();
        edits.dirty = true;
        self.log(&Op::CreateFolder { parent: parent.0, name: name.to_string() });
        Ok(id)
    }

    pub(crate) fn e_rename_folder(&mut self, folder: FolderId, name: &str) -> Result<(), StoreError> {
        let edits = self.edits()?;
        let at = edits.at(folder)?;
        if let Some(f) = edits.folders.get_mut(at) {
            f.info.name = name.to_string();
        }
        edits.dirty = true;
        self.log(&Op::RenameFolder { folder: folder.0, name: name.to_string() });
        Ok(())
    }

    pub(crate) fn e_delete_folder(&mut self, folder: FolderId) -> Result<(), StoreError> {
        let edits = self.edits()?;
        edits.at(folder)?;
        let doomed = edits.subtree(folder);
        edits.folders.retain(|f| !doomed.contains(&f.info.id));
        edits.mark_parents();
        edits.dirty = true;
        self.log(&Op::DeleteFolder { folder: folder.0 });
        Ok(())
    }

    pub(crate) fn e_move_folder(&mut self, folder: FolderId, parent: FolderId) -> Result<(), StoreError> {
        let edits = self.edits()?;
        let at = edits.at(folder)?;
        edits.at(parent)?;
        let moved = edits.subtree(folder);
        if moved.contains(&parent) {
            return Err(StoreError::Format("a folder cannot be moved into itself or a folder inside it".into()));
        }
        if let Some(f) = edits.folders.get_mut(at) {
            f.info.parent = Some(parent);
        }
        let (mut taken, rest): (Vec<EFolder>, Vec<EFolder>) =
            std::mem::take(&mut edits.folders).into_iter().partition(|f| moved.contains(&f.info.id));
        taken.sort_by_key(|f| moved.iter().position(|id| *id == f.info.id));
        edits.folders = rest;
        let after = edits.end_of(parent);
        let tail = edits.folders.split_off(after);
        edits.folders.extend(taken);
        edits.folders.extend(tail);
        edits.mark_parents();
        edits.dirty = true;
        self.log(&Op::MoveFolder { folder: folder.0, parent: parent.0 });
        Ok(())
    }

    /// Write the data file with the changes made so far to `out`, a whole new file. The open
    /// file is only read, and the changes stay as they are.
    pub fn save_to<W: Read + Write + Seek>(&mut self, out: W, seed: [u8; 16]) -> Result<(W, Written), PstError> {
        self.save_with(out, seed, Vec::new())
    }

    /// As `save_to`. `put` are nodes (id, parent, properties) written in place of what the
    /// file has under those ids: the tests make files with content the model cannot make.
    pub(crate) fn save_with<W: Read + Write + Seek>(
        &mut self,
        out: W,
        seed: [u8; 16],
        put: Vec<(u32, u32, Vec<Raw>)>,
    ) -> Result<(W, Written), PstError> {
        if self.ndb.ver != Ver::Unicode {
            return Err(PstError::Write("only a Unicode .pst can be written by this version".to_string()));
        }
        let edits = self.edits.take();
        let done = self.write(out, seed, edits.as_ref(), put);
        self.edits = edits;
        done
    }

    fn write<W: Read + Write + Seek>(
        &mut self,
        out: W,
        seed: [u8; 16],
        edits: Option<&Edits>,
        put: Vec<(u32, u32, Vec<Raw>)>,
    ) -> Result<(W, Written), PstError> {
        // Data blocks are permuted, as in the data files Outlook makes ([MS-PST] 2.2.2.6).
        let mut o = Out { w: NdbWriter::new(out, 1)?, names: Names::seeded(&self.names), uid: seed, serial: 0 };
        let mut budget = self.ndb.block_count().saturating_mul(16).saturating_add(4096);
        let root = self.root;
        let none: &[EFolder] = &[];
        let folders = edits.map_or(none, |e| e.folders.as_slice());
        let parent_of = |f: &EFolder| f.info.parent.and_then(|p| nid_of(p.0)).unwrap_or(root);

        // Where each message of a folder that was looked into goes, and what is gone.
        let view: HashMap<u32, &EFolder> = folders.iter().map(|f| (f.nid, f)).collect();
        let mut place: HashMap<u32, (u32, &Entry)> = HashMap::new();
        for f in folders {
            for e in f.items.iter().flatten() {
                place.insert(e.nid, (f.nid, e));
            }
        }
        let mut gone: HashSet<u32> = folders.iter().flat_map(|f| &f.had).filter(|n| !place.contains_key(n)).copied().collect();
        let dead: HashSet<u32> = edits.map(|e| e.was.keys().filter(|n| !view.contains_key(n)).copied().collect()).unwrap_or_default();
        for (nid, e) in &self.ndb.nbt {
            let ty = nid & 0x1F;
            let own = matches!(ty, TYPE_FOLDER | TYPE_HIERARCHY | TYPE_CONTENTS | TYPE_ASSOC) && dead.contains(&(nid & !0x1F | TYPE_FOLDER));
            if !place.contains_key(nid) && (own || (ty != TYPE_FOLDER && dead.contains(&e.parent))) {
                gone.insert(*nid);
            }
        }
        // The folders whose own node changes, and those whose table of subfolders does.
        let changed = |f: &EFolder| {
            f.new
                || f.contents_changed()
                || edits.and_then(|e| e.was.get(&f.nid)).is_none_or(|w| {
                    w.name != f.info.name || w.parent != f.info.parent.and_then(|p| nid_of(p.0)) || w.has_children != f.info.has_children
                })
        };
        let mut tables: HashSet<u32> = HashSet::new();
        for f in folders {
            if f.new {
                tables.insert(f.nid);
            }
            if changed(f) {
                tables.insert(parent_of(f));
                tables.insert(edits.and_then(|e| e.was.get(&f.nid)).and_then(|w| w.parent).unwrap_or(root));
            }
        }
        for nid in &dead {
            tables.insert(edits.and_then(|e| e.was.get(nid)).and_then(|w| w.parent).unwrap_or(root));
        }
        let put_ids: HashSet<u32> = put.iter().map(|p| p.0).collect();

        // What a changed or new message shows in its folder's table: its short values, and
        // the properties it no longer has.
        let mut shown: HashMap<u32, (Vec<Raw>, Vec<u16>)> = HashMap::new();
        let mut nids: Vec<u32> = self.ndb.nbt.keys().copied().collect();
        nids.sort_unstable();
        for nid in nids {
            let Some(e) = self.ndb.nbt.get(&nid).copied() else { continue };
            if put_ids.contains(&nid) {
                continue;
            }
            if let Some((folder, entry)) = place.get(&nid) {
                match &entry.item {
                    Some(item) => {
                        let (data, sub, cells, removed) = self.merged(&mut o, e, item, &mut budget)?;
                        o.w.node(nid, *folder, data, sub);
                        shown.insert(nid, (cells, removed));
                    }
                    None => self.copy_node(&mut o, nid, *folder, e, &mut budget)?,
                }
                continue;
            }
            if gone.contains(&nid) {
                continue;
            }
            let ty = nid & 0x1F;
            let folder = nid & !0x1F | TYPE_FOLDER;
            if edits.is_some() {
                if nid == NID_NAME_TO_ID_MAP {
                    continue;
                }
                if ty == TYPE_HIERARCHY && tables.contains(&folder) {
                    continue;
                }
                if ty == TYPE_CONTENTS && view.get(&folder).is_some_and(|f| f.contents_changed()) {
                    continue;
                }
                if ty == TYPE_FOLDER
                    && let Some(f) = view.get(&nid).filter(|f| changed(f))
                {
                    let (mut props, kept) = self.open_pc(e)?;
                    for raw in folder_cells(f) {
                        props.insert(raw.id, raw);
                    }
                    let (data, sub) = self.close_pc(&mut o, props, &kept, Vec::new(), &mut budget)?;
                    o.w.node(nid, parent_of(f), data, sub);
                    continue;
                }
                if ty == TYPE_SEARCH_CONTENTS && !gone.is_empty() && self.without_rows(&mut o, nid, e, &gone)? {
                    continue;
                }
            }
            self.copy_node(&mut o, nid, e.parent, e, &mut budget)?;
        }
        for (nid, parent, props) in &put {
            o.pc_node(*nid, *parent, props)?;
        }

        // What is new: messages, then folders.
        for f in folders {
            for entry in f.items.iter().flatten().filter(|e| !self.ndb.nbt.contains_key(&e.nid)) {
                let Some(item) = &entry.item else { continue };
                let (data, sub, cells) = o.message(item, 0)?;
                o.w.node(entry.nid, f.nid, data, sub);
                shown.insert(entry.nid, (cells, Vec::new()));
            }
            if f.new {
                o.pc_node(f.nid, parent_of(f), &folder_cells(f))?;
                o.tc_node(f.nid & !0x1F | TYPE_ASSOC, ASSOC, &[])?;
            }
        }
        // The tables of messages that changed ([MS-PST] 2.4.4.5).
        for f in folders.iter().filter(|f| f.contents_changed()) {
            let table = f.nid & !0x1F | TYPE_CONTENTS;
            let (columns, rows) = self.table_of(table).unwrap_or_else(|| (CONTENTS.iter().chain(CONTENTS_MORE).copied().collect(), Vec::new()));
            let mut by: HashMap<u32, Vec<Raw>> = rows.into_iter().map(|r| (r.id, r.cells)).collect();
            let fits = |r: &Raw| columns.iter().any(|(id, ty)| *id == r.id && *ty == r.ty);
            let mut rows = Vec::new();
            for entry in f.items.iter().flatten() {
                let cells = match (shown.get(&entry.nid), by.remove(&entry.nid)) {
                    // A changed message: its row as it was, less what it lost, under what it has now.
                    (Some((now, removed)), was) => {
                        let mut cells: BTreeMap<u16, Raw> =
                            was.unwrap_or_default().into_iter().filter(|c| !removed.contains(&c.id)).map(|c| (c.id, c)).collect();
                        cells.extend(now.iter().filter(|r| fits(r)).map(|r| (r.id, copy(r))));
                        cells.into_values().collect()
                    }
                    (None, Some(was)) => was,
                    // A message that came from another folder: what its own properties give.
                    (None, None) => {
                        let stored = self.ndb.nbt.get(&entry.nid).copied().and_then(|e| self.open_pc(e).ok()).map(|p| p.0).unwrap_or_default();
                        stored.into_values().filter(|r| r.data.len() <= MAX_CELL && fits(r)).collect()
                    }
                };
                rows.push(Row { id: entry.nid, cells });
            }
            o.tc_node(table, &columns, &rows)?;
        }
        // The tables of subfolders that changed ([MS-PST] 2.4.4.4).
        for parent in tables.iter().filter(|n| !dead.contains(n)) {
            let table = parent & !0x1F | TYPE_HIERARCHY;
            let (columns, rows) = self.table_of(table).unwrap_or_else(|| (HIERARCHY.to_vec(), Vec::new()));
            let mut by: HashMap<u32, Vec<Raw>> = rows.into_iter().map(|r| (r.id, r.cells)).collect();
            let mut rows = Vec::new();
            for f in folders.iter().filter(|f| parent_of(f) == *parent) {
                // A folder that came from elsewhere brings what its own properties give.
                let was = match by.remove(&f.nid) {
                    Some(was) => was,
                    None => {
                        let stored = self.ndb.nbt.get(&f.nid).copied().and_then(|e| self.open_pc(e).ok()).map(|p| p.0).unwrap_or_default();
                        stored
                            .into_values()
                            .filter(|r| r.data.len() <= MAX_CELL && columns.iter().any(|(id, ty)| *id == r.id && *ty == r.ty))
                            .collect()
                    }
                };
                let mut cells: BTreeMap<u16, Raw> = was.into_iter().map(|c| (c.id, c)).collect();
                cells.extend(folder_cells(f).into_iter().map(|r| (r.id, r)));
                rows.push(Row { id: f.nid, cells: cells.into_values().collect() });
            }
            // A folder this program does not list (it is hidden, or its row is damaged) stays.
            rows.extend(by.into_iter().filter(|(nid, _)| !dead.contains(nid) && !view.contains_key(nid)).map(|(id, cells)| Row { id, cells }));
            rows.sort_by_key(|r| folders.iter().position(|f| f.nid == r.id).unwrap_or(usize::MAX));
            o.tc_node(table, &columns, &rows)?;
        }
        // The names of named properties, with those that are new ([MS-PST] 2.4.7).
        if edits.is_some()
            && !put_ids.contains(&NID_NAME_TO_ID_MAP)
            && let Some(e) = self.ndb.nbt.get(&NID_NAME_TO_ID_MAP).copied()
        {
            if o.names.grew() {
                let (props, kept) = self.open_pc(e)?;
                let mut props: Vec<Raw> = props.into_values().collect();
                o.names.add_to(&mut props);
                let props: BTreeMap<u16, Raw> = props.into_iter().map(|r| (r.id, r)).collect();
                let (data, sub) = self.close_pc(&mut o, props, &kept, Vec::new(), &mut budget)?;
                o.w.node(NID_NAME_TO_ID_MAP, e.parent, data, sub);
            } else {
                self.copy_node(&mut o, NID_NAME_TO_ID_MAP, e.parent, e, &mut budget)?;
            }
        }
        let done = match edits {
            Some(_) => Written { folders: folders.len() as u64, items: folders.iter().map(|f| u64::from(f.info.total)).sum(), unreadable: 0 },
            None => Written::default(),
        };
        Ok((o.w.finish()?, done))
    }

    /// A table's columns and rows, the table's own bookkeeping left out; `None` when the
    /// file has no such table or it cannot be read.
    fn table_of(&mut self, nid: u32) -> Option<(Vec<(u16, u16)>, Vec<Row>)> {
        let e = self.ndb.nbt.get(&nid).copied()?;
        let subs = self.ndb.subnodes(e.sub).ok()?;
        let (mut columns, mut rows) = ltp::read_table(&mut self.ndb, e.data, &subs).ok()?;
        let own = |id: u16| id == 0x67F2 || id == 0x67F3;
        columns.retain(|c| !own(c.0));
        for row in &mut rows {
            row.cells.retain(|c| !own(c.id));
        }
        Some((columns, rows))
    }

    /// Write a search folder's table without the rows of messages that are gone; false when
    /// it has none of them (or cannot be read) and is to be copied as it is.
    fn without_rows<W: Read + Write + Seek>(&mut self, o: &mut Out<W>, nid: u32, e: NbtEntry, gone: &HashSet<u32>) -> Result<bool, PstError> {
        let Some((columns, mut rows)) = self.table_of(nid) else { return Ok(false) };
        if !rows.iter().any(|r| gone.contains(&r.id)) {
            return Ok(false);
        }
        rows.retain(|r| !gone.contains(&r.id));
        let mut subs = Subs::new(1);
        let data = crate::ltp_write::write_tc(&mut o.w, &columns, &rows, &mut subs)?;
        let sub = o.w.subnodes(subs.list)?;
        o.w.node(nid, e.parent, data, sub);
        Ok(true)
    }

    /// Copy a node's data as it is stored, block for block.
    fn copy_data<W: Read + Write + Seek>(&mut self, w: &mut NdbWriter<W>, bid: u64, budget: &mut usize) -> Result<u64, PstError> {
        if bid == 0 {
            return Ok(0);
        }
        let blocks = self.ndb.data_blocks(bid)?;
        *budget = budget
            .checked_sub(blocks.len().max(1))
            .ok_or_else(|| PstError::Damaged("the file's blocks point at each other without end".to_string()))?;
        w.data(&blocks)
    }

    /// Copy a node's subnodes, and theirs.
    fn copy_subs<W: Read + Write + Seek>(&mut self, w: &mut NdbWriter<W>, bid: u64, depth: usize, budget: &mut usize) -> Result<u64, PstError> {
        if bid == 0 {
            return Ok(0);
        }
        if depth > MAX_COPY_DEPTH {
            return Err(PstError::Damaged("items are nested deeper than this version follows".to_string()));
        }
        let mut subs: Vec<(u32, (u64, u64))> = self.ndb.subnodes(bid)?.into_iter().collect();
        subs.sort_unstable();
        let mut list = Vec::with_capacity(subs.len());
        for (nid, (data, sub)) in subs {
            list.push(Sub { nid, data: self.copy_data(w, data, budget)?, sub: self.copy_subs(w, sub, depth + 1, budget)? });
        }
        w.subnodes(list)
    }

    fn copy_node<W: Read + Write + Seek>(&mut self, o: &mut Out<W>, nid: u32, parent: u32, e: NbtEntry, budget: &mut usize) -> Result<(), PstError> {
        let data = self.copy_data(&mut o.w, e.data, budget)?;
        let sub = self.copy_subs(&mut o.w, e.sub, 0, budget)?;
        o.w.node(nid, parent, data, sub);
        Ok(())
    }

    /// A node's properties as they are stored, and its subnodes other than those that only
    /// hold the values of those properties.
    pub(crate) fn open_pc(&mut self, e: NbtEntry) -> Result<(BTreeMap<u16, Raw>, Subnodes), PstError> {
        let mut subs = self.ndb.subnodes(e.sub)?;
        let props = ltp::read_pc(&mut self.ndb, e.data, &subs)?;
        for nid in ltp::pc_value_subnodes(&mut self.ndb, e.data)? {
            subs.remove(&nid);
        }
        Ok((props.into_iter().map(|r| (r.id, r)).collect(), subs))
    }

    /// Write properties as a node's data and subnodes: `kept` are copied from the file as they
    /// are, `more` were written already.
    fn close_pc<W: Read + Write + Seek>(
        &mut self,
        o: &mut Out<W>,
        props: BTreeMap<u16, Raw>,
        kept: &Subnodes,
        more: Vec<Sub>,
        budget: &mut usize,
    ) -> Result<(u64, u64), PstError> {
        // Values too large for the heap get subnodes of their own, numbered past those kept.
        let used = kept.keys().copied().chain(more.iter().map(|s| s.nid)).filter(|n| n & 0x1F == TYPE_LTP).map(|n| n >> 5).max().unwrap_or(0);
        let mut subs = Subs::new(used.saturating_add(1));
        let props: Vec<Raw> = props.into_values().collect();
        let data = write_pc(&mut o.w, &props, &mut subs)?;
        let mut kept: Vec<(u32, (u64, u64))> = kept.iter().map(|(n, e)| (*n, *e)).collect();
        kept.sort_unstable();
        for (nid, (data, sub)) in kept {
            subs.list.push(Sub { nid, data: self.copy_data(&mut o.w, data, budget)?, sub: self.copy_subs(&mut o.w, sub, 1, budget)? });
        }
        subs.list.extend(more);
        Ok((data, o.w.subnodes(subs.list)?))
    }

    /// A changed message: every stored property that was not changed stays as it is stored,
    /// and so do the recipients and the attachments when they are the same. Gives the node's
    /// data and subnodes, its short values, and the properties taken away.
    fn merged<W: Read + Write + Seek>(
        &mut self,
        o: &mut Out<W>,
        e: NbtEntry,
        new: &Item,
        budget: &mut usize,
    ) -> Result<(u64, u64, Vec<Raw>, Vec<u16>), PstError> {
        let old = self.message(e.data, e.sub, 0)?;
        let (mut props, mut kept) = self.open_pc(e)?;
        let mut removed = Vec::new();
        let ids: HashSet<u16> =
            old.props.iter().chain(new.props.iter()).map(|(id, _)| id).filter(|id| *id < 0x8000 && !LTP_INTERNAL.contains(id)).collect();
        for id in ids {
            match (old.props.get(id), new.props.get(id)) {
                (was, Some(now)) if was != Some(now) => {
                    props.insert(id, encode(id, now));
                }
                (Some(_), None) => {
                    props.remove(&id);
                    removed.push(id);
                }
                _ => {}
            }
        }
        if old.categories != new.categories {
            let id = o.names.id_for(&NamedKey::keywords())?;
            if new.categories.is_empty() {
                props.remove(&id);
            } else {
                props.insert(id, encode(id, &PropValue::MultiString(new.categories.clone())));
            }
        }
        for (key, _) in &old.named {
            if !new.named.iter().any(|(k, _)| k == key)
                && let Some(id) = o.names.known(key)
            {
                props.remove(&id);
            }
        }
        for (key, now) in &new.named {
            if old.named.iter().find(|(k, _)| k == key).map(|(_, v)| v) != Some(now) {
                let id = o.names.id_for(key)?;
                props.insert(id, encode(id, now));
            }
        }
        let mut more = Vec::new();
        if old.recipients != new.recipients {
            kept.remove(&NID_RECIPIENT_TABLE);
            more.push(o.recipients(new)?);
        }
        if old.attachments != new.attachments {
            kept.remove(&NID_ATTACHMENT_TABLE);
            kept.retain(|nid, _| nid & 0x1F != TYPE_ATTACHMENT);
            more.extend(o.attachments(new, 0)?);
            let has = !new.attachments.is_empty();
            let flags = if has { new.flags() | MSGFLAG_HASATTACH } else { new.flags() & !MSGFLAG_HASATTACH };
            props.insert(pid::MESSAGE_FLAGS, int(pid::MESSAGE_FLAGS, flags));
            props.insert(pid::HASATTACH, flag(pid::HASATTACH, has));
        }
        let cells: Vec<Raw> = props.values().filter(|r| r.data.len() <= MAX_CELL).map(copy).collect();
        let (data, sub) = self.close_pc(o, props, &kept, more, budget)?;
        Ok((data, sub, cells, removed))
    }

    /// Put the changes made so far into the data file. The new file is written beside it,
    /// held against the structure check, read back and compared with what was meant, and only
    /// then put in the old one's place in one step. Whatever goes wrong, the data file stays
    /// as it was and the changes stay to be saved again.
    pub fn save(&mut self, seed: [u8; 16]) -> Result<Written, PstError> {
        if !self.e_unsaved() {
            return Ok(Written::default());
        }
        let path = self.path.clone().ok_or_else(|| PstError::Write("this data file is not kept in a file".to_string()))?;
        if stamp(&path) != self.stamp {
            return Err(PstError::Write("another program changed the data file since it was opened; nothing was saved".to_string()));
        }
        let name = path.file_name().ok_or_else(|| PstError::Write("the path has no file name".to_string()))?;
        let mut tmp_name = std::ffi::OsString::from(".");
        tmp_name.push(name);
        tmp_name.push(format!(".{}.tmp", std::process::id()));
        let tmp = path.with_file_name(tmp_name);
        let done = self.swap(&path, &tmp, seed);
        match done {
            // The changes are in the file: the journal of them has done its work.
            Ok(_) => self.journal.finish(Some(&path)),
            Err(_) => {
                let _ = std::fs::remove_file(&tmp);
            }
        }
        done
    }

    fn swap(&mut self, path: &Path, tmp: &PathBuf, seed: [u8; 16]) -> Result<Written, PstError> {
        let file = std::fs::File::options().read(true).write(true).create(true).truncate(true).open(tmp)?;
        let (mut file, done) = self.save_to(file, seed)?;
        file.sync_all()?;
        file.seek(SeekFrom::Start(0))?;
        let found = crate::check(&mut std::io::BufReader::new(&mut file))?;
        if let Some(problem) = found.problems.first() {
            return Err(PstError::Write(format!("the new file did not pass its own check ({problem}); nothing was replaced")));
        }
        drop(file);
        // Read it back: the folders and their counts are what was meant, and each folder that
        // was looked into lists as many messages as it should.
        let mut fresh = Self::open(tmp)?;
        let key = |f: &FolderInfo| (f.id, f.parent, f.name.clone(), f.total, f.unread);
        let mut want: Vec<_> = self.e_folders().map_err(|e| PstError::Source(e.to_string()))?.iter().map(key).collect();
        let mut got: Vec<_> = fresh.file_folders().map_err(|e| PstError::Source(e.to_string()))?.iter().map(key).collect();
        want.sort();
        got.sort();
        let mut same = want == got;
        for f in self.edits.iter().flat_map(|e| &e.folders).filter(|f| f.items.is_some()) {
            let listed: Vec<u64> = fresh.file_list(f.info.id).map(|l| l.iter().map(|s| s.id).collect()).unwrap_or_default();
            same &= f.items.iter().flatten().map(|e| u64::from(e.nid)).eq(listed);
        }
        if !same {
            return Err(PstError::Write("the new file does not read back as the mailbox that was saved; nothing was replaced".to_string()));
        }
        // Let go of the old file, then put the new one in its place.
        let old = std::mem::replace(&mut self.ndb, fresh.ndb);
        drop(old);
        // The first time a data file is replaced, the file as it was stays beside it under
        // another name: a second name for the same bytes, so it costs no copying.
        let moved = free_to_replace(path).and_then(|()| {
            let kept = original_beside(path);
            if !kept.exists() {
                let _ = std::fs::hard_link(path, &kept);
            }
            std::fs::rename(tmp, path)
        });
        if let Err(e) = moved {
            let back = std::fs::File::open(path).map_err(PstError::Io).and_then(|f| Ndb::open(Box::new(f)));
            return match back {
                Ok(ndb) => {
                    self.ndb = ndb;
                    Err(PstError::Write(format!("the data file could not be replaced ({e}); it is as it was, and the changes are still unsaved")))
                }
                // The old file cannot be read any more: the new one stays open under its own name.
                Err(_) => {
                    self.path = Some(tmp.clone());
                    self.finish_swap(fresh.name, fresh.names, fresh.root, fresh.special);
                    Ok(done)
                }
            };
        }
        self.finish_swap(fresh.name, fresh.names, fresh.root, fresh.special);
        Ok(done)
    }

    fn finish_swap(&mut self, name: String, names: Vec<Option<NamedKey>>, root: u32, special: HashMap<u32, FolderKind>) {
        if !name.is_empty() {
            self.name = name;
        }
        (self.names, self.root, self.special) = (names, root, special);
        self.edits = None;
        self.note_stamp();
    }
}

/// Where the file as it was before its first save here is kept: beside it.
pub fn original_beside(path: &Path) -> PathBuf {
    let mut name = path.file_stem().map(std::ffi::OsString::from).unwrap_or_default();
    name.push(".before-openmailclient.pst");
    path.with_file_name(name)
}

/// An error when another program has the file open: its data file is not replaced under it.
#[cfg(windows)]
fn free_to_replace(path: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    // Opening for writing and sharing with no one succeeds only when nobody else has it.
    std::fs::File::options().write(true).share_mode(0).open(path).map(|_| ())
}

#[cfg(not(windows))]
fn free_to_replace(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// What a folder's own node and its row in its parent's table say of it ([MS-PST] 2.4.4.1.1).
fn folder_cells(f: &EFolder) -> Vec<Raw> {
    let mut out = vec![text(pid::DISPLAY_NAME, &f.info.name), flag(pid::SUBFOLDERS, f.info.has_children)];
    if f.items.is_some() {
        out.push(int(pid::CONTENT_COUNT, count(u64::from(f.info.total))));
        out.push(int(pid::CONTENT_UNREAD_COUNT, count(u64::from(f.info.unread))));
    }
    if f.new && !f.info.container_class.is_empty() {
        out.push(text(pid::CONTAINER_CLASS, &f.info.container_class));
    }
    out
}

impl PstStore {
    pub(crate) fn e_save(&mut self, seed: [u8; 16]) -> Result<(), StoreError> {
        self.save(seed).map(|_| ()).map_err(StoreError::from)
    }
}
