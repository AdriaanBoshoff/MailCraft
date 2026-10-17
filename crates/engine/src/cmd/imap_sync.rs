//! IMAP kept in step both ways. Each message of an IMAP account whose mail is in a data file
//! of its own knows its place on the server: the folder, the numbering of its ids, its id there,
//! and the flags both sides last agreed on (`Place`, a named property of this program's own).
//! At each Send/Receive what was done here goes to the server first, and then what was done
//! there comes here:
//!
//! - read and flagged, each way; where both sides changed the same flag, this side wins;
//! - a message moved here to another folder is moved there (MOVE, [RFC 6851], or COPY, then
//!   marked deleted and expunged), to a folder made on the server when it has none yet;
//! - a message deleted here for good (`Received::gone`) is deleted there;
//! - a message put here into a folder of the account from elsewhere is put on the server;
//! - a draft saved here is put in the server's Drafts folder, marked `\Draft` ([RFC 9051]
//!   2.3.2), and one saved again or sent goes from there, so that the server holds each draft as
//!   it is now; one written elsewhere comes here as a draft, to be written on and sent;
//! - a message the server no longer has goes from here too.
//!
//! Nothing is taken off the server that was not deleted or moved here by the owner, and a message
//! goes from here only when the server's whole list of a folder, as many as it says the folder
//! holds, lacks it. The first time, the messages taken down before they could know their place
//! are found on the server by their Message-ID; none of them is put there anew.
//!
//! Folders follow too, before any message does (`follow`): a folder renamed or moved on the
//! server is renamed or moved here, found by the server's own id for it ([RFC 8474]) or by its
//! UIDVALIDITY, and its messages keep their places; one the server no longer has at all is kept
//! here, with the deleted mail. A folder renamed or moved here is renamed or moved there (RENAME,
//! into the server's Trash when it is put with the deleted mail), one made here is made there,
//! and one deleted here for good (`Received::folders_gone`) is deleted there. A folder only
//! missing here deletes nothing.
//!
//! Where the server keeps a mod-sequence for every change (CONDSTORE and QRESYNC, [RFC 7162]),
//! each folder remembers the one it was last kept in step at (`Step`): a folder that has not
//! changed since costs its SELECT and nothing more, and one that has is asked only for what
//! changed after it. That a message went is still made sure of before it goes here: a
//! VANISHED the server sends is checked against the folder, and where the server says nothing
//! of what went, the folder's count must add up or its whole list is read as before.

use super::receive::{is_view, kind_of, path_of};
use crate::Session;
use crate::accounts::{Gone, GoneFolder, Kind, Mark, Received, Step, renamed_raw};
use openmailclient_model::{Flag, FolderId, FolderKind, Item, ItemId, NameId, NamedKey, PropValue};
use openmailclient_net::NetError;
use openmailclient_net::imap::{Folder, Imap, Selected, Status, Summary};
use std::collections::{HashMap, HashSet};

/// The property set of what this program keeps on an item for itself.
pub(crate) const PS_OPENMAILCLIENT: [u8; 16] = [0x4F, 0x4D, 0x43, 0x31, 0x8E, 0x2A, 0x4B, 0x7D, 0x9C, 0x15, 0x3A, 0x60, 0xD2, 0x7E, 0x51, 0xA4];
const PLACE: &str = "ImapPlace";

/// Messages put on a server in one Send/Receive, and their bytes; the rest go with the next.
const MAX_UPLOADS: usize = 300;
const MAX_UPLOAD_BYTES: u64 = 64 * 1024 * 1024;
/// Items whose place is remembered between receives, across every data file.
const MAX_KNOWN: usize = 1_000_000;
/// Folders asked of with STATUS in one receive, to find where folders went.
const MAX_STATUS: usize = 50;
/// Folders made on the server in one receive as they were made here; the rest go with the next.
const MAX_MADE: usize = 50;

/// Where a message of an IMAP account is on the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// In a folder of the server: its UIDVALIDITY, the message's id, and its read and flagged
    /// state as both sides last had it.
    On { raw: String, uid_validity: u32, uid: u32, seen: bool, flagged: bool },
    /// Put in a folder of the server (none named: its Sent folder) without being told its id
    /// there; found by its Message-ID when that folder is next taken down.
    Pending { raw: String },
    /// Kept only here.
    Here,
}

fn key() -> NamedKey {
    NamedKey { guid: PS_OPENMAILCLIENT, name: NameId::Name(PLACE.to_string()) }
}

impl Place {
    pub(crate) fn of(item: &Item) -> Option<Self> {
        let k = key();
        item.named.iter().find(|(n, _)| *n == k).and_then(|(_, v)| match v {
            PropValue::String(s) => Self::parse(s),
            _ => None,
        })
    }

    pub(crate) fn put(&self, item: &mut Item) {
        let (k, v) = (key(), PropValue::String(self.text()));
        match item.named.iter_mut().find(|(n, _)| *n == k) {
            Some(slot) => slot.1 = v,
            None => item.named.push((k, v)),
        }
    }

    pub(crate) fn clear(item: &mut Item) {
        let k = key();
        item.named.retain(|(n, _)| *n != k);
    }

    fn text(&self) -> String {
        match self {
            Self::On { raw, uid_validity, uid, seen, flagged } => {
                format!("on {uid_validity} {uid} {}{} {raw}", if *seen { 'S' } else { '-' }, if *flagged { 'F' } else { '-' })
            }
            Self::Pending { raw } => format!("pending {raw}"),
            Self::Here => "here".to_string(),
        }
    }

    /// As an item gives it: what makes no sense is no place at all.
    fn parse(t: &str) -> Option<Self> {
        if t.len() > 2000 || t.chars().any(char::is_control) {
            return None;
        }
        let (word, rest) = t.split_once(' ').unwrap_or((t, ""));
        match word {
            "here" => Some(Self::Here),
            "pending" => Some(Self::Pending { raw: rest.to_string() }),
            "on" => {
                let mut w = rest.splitn(4, ' ');
                let uid_validity = w.next()?.parse().ok()?;
                let uid = w.next()?.parse().ok()?;
                let flags = w.next()?;
                let raw = w.next()?.to_string();
                (!raw.is_empty() && uid > 0).then_some(Self::On { raw, uid_validity, uid, seen: flags.contains('S'), flagged: flags.contains('F') })
            }
            _ => None,
        }
    }
}

/// The places of items read before, so that a receive does not read every message again: by
/// data file, folder and item, with the time and subject that tell the item is the same one.
#[derive(Default)]
pub struct Places {
    by: HashMap<(String, u64, u64), Known>,
}

#[derive(Clone, Debug)]
struct Known {
    time: i64,
    subject: String,
    place: Option<Place>,
    message_id: String,
}

impl Places {
    /// How many items are remembered.
    pub fn len(&self) -> usize {
        self.by.len()
    }

    /// Whether none is.
    pub fn is_empty(&self) -> bool {
        self.by.is_empty()
    }

    fn forget(&mut self, path: &str, folder: FolderId, id: ItemId) {
        self.by.remove(&(path.to_string(), folder.0, id.0));
    }
}

/// A folder of the account's data file.
#[derive(Clone, Debug)]
pub(super) struct LocalFolder {
    pub id: FolderId,
    pub parent: Option<FolderId>,
    pub name: String,
    pub kind: FolderKind,
}

/// A message of the account's data file as a receive began.
#[derive(Clone, Debug)]
pub(super) struct LocalItem {
    pub folder: FolderId,
    pub id: ItemId,
    pub place: Option<Place>,
    pub read: bool,
    pub flagged: bool,
    pub message_id: String,
    /// The message as it goes on the server, when it is to be put there.
    pub upload: Option<Vec<u8>>,
    /// It is a draft, not sent yet: it goes up marked `\Draft`.
    pub draft: bool,
}

/// The account's data file as a receive began.
#[derive(Clone, Debug, Default)]
pub(super) struct Local {
    pub folders: Vec<LocalFolder>,
    pub items: Vec<LocalItem>,
    pub gone: Vec<Gone>,
    /// The messages taken down before they knew their place were found on the server.
    pub matched: bool,
    /// The folders of the server paired with folders here, as remembered.
    pub marks: Vec<Mark>,
    /// Folders deleted here for good that are to go from the server.
    pub folders_gone: Vec<GoneFolder>,
    /// What was learnt of messages read apart, remembered when the receive ends.
    known: Vec<((String, u64, u64), Known)>,
}

impl Local {
    fn kind(&self, id: FolderId) -> Option<FolderKind> {
        self.folder(id).map(|f| f.kind)
    }

    fn folder(&self, id: FolderId) -> Option<&LocalFolder> {
        self.folders.iter().find(|f| f.id == id)
    }

    /// Where the server's top is here: where the Inbox stands.
    fn top(&self) -> Option<FolderId> {
        self.folders.iter().find(|l| l.kind == FolderKind::Inbox).and_then(|l| l.parent)
    }

    /// Whether `id` is `of` or a folder inside it.
    fn inside(&self, id: FolderId, of: FolderId) -> bool {
        let mut at = Some(id);
        for _ in 0..64 {
            match at {
                Some(a) if a == of => return true,
                Some(a) => at = self.folder(a).and_then(|f| f.parent),
                None => return false,
            }
        }
        false
    }

    /// A folder of the server renamed `from` to `to`, with those inside it: the messages kept in
    /// step with it and what is remembered of it take the new name, and the messages' places are
    /// rewritten as they are now.
    fn renamed(&mut self, from: &str, to: &str, d: Option<char>, done: &mut Synced) {
        for i in &mut self.items {
            let (Some(Place::On { raw, .. }) | Some(Place::Pending { raw })) = &mut i.place else { continue };
            let Some(new) = renamed_raw(raw, from, to, d) else { continue };
            *raw = new;
            if let Some(place) = &i.place {
                done.replaced.push((i.folder, i.id, place.clone()));
            }
        }
        let new = |raw: &mut String| {
            if let Some(r) = renamed_raw(raw, from, to, d) {
                *raw = r;
            }
        };
        self.gone.iter_mut().for_each(|g| new(&mut g.raw));
        self.marks.iter_mut().for_each(|m| new(&mut m.raw));
        self.folders_gone.iter_mut().for_each(|g| new(&mut g.raw));
    }
}

/// Folders the owner makes, names and moves: not those of a kind the server has its own of.
fn own_folder(kind: FolderKind) -> bool {
    matches!(kind, FolderKind::Mail | FolderKind::Other)
}

/// The name of the folder a server's folder is in, by its delimiter.
fn parent_raw(raw: &str, d: Option<char>) -> Option<&str> {
    raw.rsplit_once(d?).map(|(p, _)| p)
}

/// Folders mail is kept in, as against appointments, contacts and the like.
fn holds_mail(kind: FolderKind) -> bool {
    !matches!(
        kind,
        FolderKind::Calendar
            | FolderKind::Contacts
            | FolderKind::Tasks
            | FolderKind::Notes
            | FolderKind::Journal
            | FolderKind::Search
            | FolderKind::Root
    )
}

/// Folders whose mail goes on the server: not the Outbox, whose mail is not sent yet. Drafts go
/// too, as they are when saved.
fn goes_up(kind: FolderKind) -> bool {
    matches!(
        kind,
        FolderKind::Inbox
            | FolderKind::Sent
            | FolderKind::Deleted
            | FolderKind::Junk
            | FolderKind::Archive
            | FolderKind::Mail
            | FolderKind::Other
            | FolderKind::Drafts
    )
}

fn flagged(flag: Flag) -> bool {
    flag == Flag::Flagged
}

/// What the account's data file holds, for a receive to keep in step with the server: what is
/// remembered of each message is taken here, at once; the messages not read before (each of
/// them, the first time) and those to be put on the server are read by `Gather::finish` on the
/// receive's own thread, from the mailbox as it stands now (`Store::apart`), so that the window
/// is not held while a large data file is read through. A store in no file is read here.
pub(super) fn local(s: &mut Session, store: usize, received: &Received) -> Gather {
    let path = s.stores.get(store).and_then(|st| st.path()).map(|p| p.display().to_string()).unwrap_or_default();
    let folders: Vec<LocalFolder> = s
        .folders
        .get(store)
        .map(|l| l.iter().map(|f| LocalFolder { id: f.id, parent: f.parent, name: f.name.clone(), kind: f.kind }).collect())
        .unwrap_or_default();
    if s.places.by.len() > MAX_KNOWN {
        s.places.by.clear();
    }
    let (mut items, mut unread) = (Vec::new(), Vec::new());
    for f in folders.iter().filter(|f| holds_mail(f.kind)) {
        let Some(st) = s.stores.get_mut(store) else { break };
        let Ok(rows) = st.list(f.id) else { continue };
        for r in rows {
            let key = (path.clone(), f.id.0, r.id);
            let known = s.places.by.get(&key).filter(|k| k.time == r.time && k.subject == r.subject).cloned();
            let up = received.matched && goes_up(f.kind) && known.as_ref().is_none_or(|k| k.place.is_none());
            if known.is_none() || up {
                let learn = known.is_none();
                unread.push(Unread {
                    at: items.len(),
                    folder: f.id,
                    kind: f.kind,
                    id: ItemId(r.id),
                    time: r.time,
                    subject: r.subject.clone(),
                    learn,
                });
            }
            let (place, message_id) = known.map(|k| (k.place, k.message_id)).unwrap_or_default();
            items.push(LocalItem {
                folder: f.id,
                id: ItemId(r.id),
                place,
                read: r.read,
                flagged: flagged(r.flag),
                message_id,
                upload: None,
                draft: false,
            });
        }
    }
    let marks = received.folders.iter().filter(|m| m.here != 0).cloned().collect();
    let local = Local {
        folders,
        items,
        gone: received.gone.clone(),
        matched: received.matched,
        marks,
        folders_gone: received.folders_gone.clone(),
        known: Vec::new(),
    };
    let mut g = Gather { local, path, unread, apart: None };
    if g.unread.is_empty() {
        return g;
    }
    let Some(st) = s.stores.get_mut(store) else { return g };
    // A data file is read apart; a store in no file (or one that cannot be) is read here, and
    // what is read of it is remembered at once.
    g.apart = if st.path().is_some() { st.apart().ok().flatten() } else { None };
    if g.apart.is_none() {
        g.read(st.as_mut(), None);
        for (key, known) in g.local.known.drain(..) {
            s.places.by.insert(key, known);
        }
    }
    g
}

/// A message `Gather::finish` reads: where it is among the items, and what tells it again.
struct Unread {
    at: usize,
    folder: FolderId,
    kind: FolderKind,
    id: ItemId,
    time: i64,
    subject: String,
    /// It was not read before: what it says of its place is to be learnt.
    learn: bool,
}

/// The account's data file as a receive began, part of it still to be read (`local`).
pub(super) struct Gather {
    local: Local,
    path: String,
    unread: Vec<Unread>,
    /// The mailbox as it stood, to be read on the receive's own thread.
    apart: Option<openmailclient_model::Apart>,
}

impl Gather {
    /// The rest read, on the receive's own thread: the data file through a handle of its own,
    /// with the changes as they were when the receive began. One that cannot be read is not
    /// kept in step this time, rather than being taken for a file without those messages.
    pub(super) fn finish(mut self, progress: &openmailclient_model::Progress) -> Result<Local, NetError> {
        if let Some(apart) = self.apart.take() {
            let mut st = apart(std::sync::Arc::new(openmailclient_model::Progress::default())).map_err(|e| {
                NetError::Unsupported(format!(
                    "the account's data file could not be read apart ({e}); it is kept in step with the server at the next Send/Receive"
                ))
            })?;
            if !self.read(st.as_mut(), Some(progress)) {
                return Err(NetError::Unsupported("stopped before the account's data file was read".into()));
            }
        }
        Ok(self.local)
    }

    /// Read each message waiting: its place, when it was not known, and what goes on the
    /// server. False when stopped.
    fn read(&mut self, st: &mut dyn openmailclient_model::Store, progress: Option<&openmailclient_model::Progress>) -> bool {
        let (mut uploads, mut bytes) = (0usize, 0u64);
        let mut lost = HashSet::new();
        for u in &self.unread {
            if progress.is_some_and(openmailclient_model::Progress::stopped) {
                return false;
            }
            let Some(i) = self.local.items.get_mut(u.at) else { continue };
            let Ok(item) = st.load(u.folder, u.id) else {
                // One not read before that cannot be read now is left out, as if not there.
                if u.learn {
                    lost.insert(u.at);
                }
                continue;
            };
            if u.learn {
                let known = Known {
                    time: u.time,
                    subject: u.subject.clone(),
                    place: Place::of(&item),
                    message_id: item.props.str(openmailclient_model::pid::INTERNET_MESSAGE_ID).map(str::trim).unwrap_or_default().to_string(),
                };
                i.place = known.place.clone();
                i.message_id = known.message_id.clone();
                if self.local.known.len() < MAX_KNOWN {
                    self.local.known.push(((self.path.clone(), u.folder.0, u.id.0), known));
                }
            }
            if i.place.is_none()
                && self.local.matched
                && goes_up(u.kind)
                && uploads < MAX_UPLOADS
                && bytes < MAX_UPLOAD_BYTES
                // A message not sent yet goes up only as a draft, from Drafts.
                && (!item.is_unsent() || u.kind == FolderKind::Drafts)
                && item.message_class().to_ascii_uppercase().starts_with("IPM.")
            {
                let data = openmailclient_mime::write(&item);
                uploads += 1;
                bytes = bytes.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
                i.upload = Some(data);
                i.draft = item.is_unsent();
            }
        }
        if !lost.is_empty() {
            let mut at = 0usize;
            self.local.items.retain(|_| {
                at += 1;
                !lost.contains(&(at - 1))
            });
        }
        true
    }
}

/// The folder of the account's data file a server's folder is kept in, when it is there: the
/// one of its kind, or the one of the same name and place. As `receive::local_folder` finds it,
/// without making anything.
pub(super) fn here_of(folders: &[LocalFolder], f: &Folder) -> Option<FolderId> {
    let inbox = folders.iter().find(|l| l.kind == FolderKind::Inbox);
    if f.raw.eq_ignore_ascii_case("INBOX") {
        return inbox.map(|l| l.id);
    }
    let path = path_of(f);
    if let Some(kind) = kind_of(f, &path)
        && let Some(l) = folders.iter().find(|l| l.kind == kind)
    {
        return Some(l.id);
    }
    let mut parent = inbox.and_then(|l| l.parent);
    let mut at = None;
    for part in &path {
        let wanted = part.to_lowercase();
        let l = folders.iter().find(|l| l.parent == parent && l.name.to_lowercase() == wanted)?;
        if !holds_mail(l.kind) || l.kind == FolderKind::Outbox {
            return None;
        }
        parent = Some(l.id);
        at = Some(l.id);
    }
    at
}

/// The pairing of the server's folders with the account's.
pub(super) struct Map {
    pub pairs: Vec<(String, FolderId)>,
    delimiter: Option<char>,
    /// Every folder of the server is under its Inbox, as some servers want them.
    under_inbox: bool,
}

impl Map {
    pub(super) fn of(folders: &[Folder], local: &Local) -> Self {
        let usable: Vec<&Folder> = folders.iter().filter(|f| f.selectable() && !is_view(f)).collect();
        let mut pairs: Vec<(String, FolderId)> = Vec::new();
        // The pairs remembered first, so that a folder renamed on either side stays itself.
        remember(&mut pairs, folders, local);
        for f in &usable {
            if let Some(here) = here_of(&local.folders, f)
                && !pairs.iter().any(|(r, _)| r.eq_ignore_ascii_case(&f.raw))
            {
                pairs.push((if f.raw.eq_ignore_ascii_case("INBOX") { "INBOX".to_string() } else { f.raw.clone() }, here));
            }
        }
        let delimiter = folders.iter().find_map(|f| f.delimiter);
        let others: Vec<&&Folder> = usable.iter().filter(|f| !f.raw.eq_ignore_ascii_case("INBOX")).collect();
        let under_inbox =
            delimiter.is_some_and(|d| !others.is_empty() && others.iter().all(|f| f.raw.to_ascii_uppercase().starts_with(&format!("INBOX{d}"))));
        Self { pairs, delimiter, under_inbox }
    }

    pub(super) fn here(&self, raw: &str) -> Option<FolderId> {
        self.pairs.iter().find(|(r, _)| r == raw).map(|(_, h)| *h)
    }

    /// A folder of the server renamed, with those inside it.
    fn renamed(&mut self, from: &str, to: &str) {
        for (raw, _) in &mut self.pairs {
            if let Some(r) = renamed_raw(raw, from, to, self.delimiter) {
                *raw = r;
            }
        }
    }

    fn raw(&self, here: FolderId) -> Option<String> {
        self.pairs.iter().find(|(_, h)| *h == here).map(|(r, _)| r.clone())
    }
}

/// The folders of the server paired as remembered with folders here that are still there.
fn remember(pairs: &mut Vec<(String, FolderId)>, folders: &[Folder], local: &Local) {
    for m in &local.marks {
        let here = FolderId(m.here);
        let listed = folders.iter().any(|f| f.raw == m.raw && f.selectable() && !is_view(f));
        if listed && local.folder(here).is_some_and(|f| holds_mail(f.kind)) && !pairs.iter().any(|(r, _)| *r == m.raw) {
            pairs.push((m.raw.clone(), here));
        }
    }
}

/// What keeping in step with the server came to, for the session to take in.
#[derive(Default)]
pub(super) struct Synced {
    /// Items given another place: their changes went to the server, or they went there.
    pub placed: Vec<(FolderId, ItemId, Place)>,
    /// What the server has that is to be taken here.
    pub pulled: Vec<Pull>,
    /// Items the server no longer has, with the id they had there.
    pub removed: Vec<(FolderId, ItemId, u32)>,
    /// Deletions that went to the server, or can no longer go.
    pub deleted: Vec<Gone>,
    /// The first time: the Message-ID of each message on the server, by folder.
    pub found: Vec<Found>,
    /// Every folder was looked through for the first time without trouble.
    pub matched: bool,
    /// Messages this receive put on the server, by folder and id: not taken down again.
    pub ours: HashSet<(String, u32)>,
    /// Changes that went to the server.
    pub pushed: usize,
    /// Folders made on the server.
    pub created: Vec<String>,
    /// Where each folder kept in step was left, to start from next time.
    pub steps: Vec<Step>,
    /// Folders found unchanged since last time, and those of which only what changed was read.
    pub unchanged: usize,
    pub changes_only: usize,
    /// Items whose place went with their folder's new name: taken in before anything else.
    pub replaced: Vec<(FolderId, ItemId, Place)>,
    /// Folders of the server renamed or moved, by either side: the name before, the name now,
    /// and the server's delimiter.
    pub renamed: Vec<(String, String, Option<char>)>,
    /// Folders here to be moved or renamed as the server has them now: the folder, the folder to
    /// put it in (none: where it is), and its name.
    pub moved_here: Vec<(FolderId, Option<Option<FolderId>>, String)>,
    /// Folders here whose folder the server no longer has: put with the deleted mail, kept.
    pub kept_here: Vec<FolderId>,
    /// Folders deleted on the server as asked here, or no longer there to delete.
    pub folders_deleted: Vec<String>,
    /// Each folder paired, with where its folder here stands and its name as they are now, to be
    /// remembered as agreed.
    pub agreed: Vec<(String, FolderId, Option<FolderId>, String)>,
}

/// A change the server has for an item here.
pub(super) struct Pull {
    pub folder: FolderId,
    pub id: ItemId,
    /// The item's state as the receive began: what is taken applies only while it is so.
    pub was_read: bool,
    pub was_flagged: bool,
    pub read: Option<bool>,
    pub flagged: Option<bool>,
    pub place: Place,
}

/// A message on the server, by its Message-ID.
pub(super) struct Found {
    pub raw: String,
    pub here: FolderId,
    pub uid_validity: u32,
    pub uid: u32,
    pub message_id: String,
    pub seen: bool,
    pub flagged: bool,
}

/// A refusal of one change is passed over; it is tried again at the next receive.
fn tolerate<T>(r: Result<T, NetError>) -> Result<Option<T>, NetError> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(NetError::Refused(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

fn has(m: &Summary, flag: &str) -> bool {
    m.flags.iter().any(|f| f.eq_ignore_ascii_case(flag))
}

/// Make on the server the folder that is kept here as `here`, and its parents that are not
/// there either. Its name there when it was made.
fn make_on_server(
    imap: &mut Imap,
    local: &Local,
    map: &mut Map,
    here: FolderId,
    depth: usize,
    done: &mut Synced,
) -> Result<Option<String>, NetError> {
    if let Some(raw) = map.raw(here) {
        return Ok(Some(raw));
    }
    let Some(f) = local.folder(here) else { return Ok(None) };
    // A folder paired before that the server no longer has is not made there again.
    if depth > 32 || !goes_up(f.kind) || f.kind == FolderKind::Deleted || f.kind == FolderKind::Inbox || local.marks.iter().any(|m| m.here == here.0)
    {
        return Ok(None);
    }
    let f = f.clone();
    let Some(raw) = wanted_raw(imap, local, map, &f, depth, done)? else { return Ok(None) };
    if tolerate(imap.create(&raw))?.is_none() {
        return Ok(None);
    }
    map.pairs.push((raw.clone(), here));
    done.created.push(raw.clone());
    Ok(Some(raw))
}

/// The name on the server a folder here has, standing where it stands: under the folder its
/// parent is kept in step with, made there when it is not there yet, or at the top.
fn wanted_raw(imap: &mut Imap, local: &Local, map: &mut Map, f: &LocalFolder, depth: usize, done: &mut Synced) -> Result<Option<String>, NetError> {
    let name = openmailclient_net::utf7::encode(&f.name);
    if name.is_empty() || f.name.contains(|c: char| c.is_control() || Some(c) == map.delimiter || matches!(c, '*' | '%')) {
        return Ok(None);
    }
    if f.parent == local.top() {
        return Ok(Some(match map.delimiter {
            Some(d) if map.under_inbox => format!("INBOX{d}{name}"),
            _ => name,
        }));
    }
    let (Some(parent), Some(d)) = (f.parent, map.delimiter) else { return Ok(None) };
    let Some(above) = make_on_server(imap, local, map, parent, depth + 1, done)? else { return Ok(None) };
    Ok(Some(format!("{above}{d}{name}")))
}

/// A folder of the server renamed, by either side: everything that knows it by its name takes
/// the new one.
fn follow_rename(from: &str, to: &str, local: &mut Local, received: &mut Received, map: &mut Map, done: &mut Synced) {
    let d = map.delimiter;
    done.renamed.push((from.to_string(), to.to_string(), d));
    received.renamed(from, to, d);
    local.renamed(from, to, d, done);
    map.renamed(from, to);
}

/// Folders follow, each way, before any message does. On the server's side: a folder paired
/// before that the server no longer has by its name was renamed or moved there, when one of
/// its new folders has its id ([RFC 8474]) or, where the server keeps none, alone its
/// UIDVALIDITY; then it is renamed or moved here. One that no folder there can be is gone, and
/// what it holds is kept here, with the deleted mail. Where it cannot be told, nothing is done.
/// On this side: a folder renamed or moved here since the two last agreed is renamed or moved
/// there; one deleted here for good is deleted there, while the server's folder is still the
/// one it was; one made here is made there. True when the server's folders changed.
pub(super) fn follow(
    imap: &mut Imap,
    listed: &[Folder],
    map: &mut Map,
    local: &mut Local,
    received: &mut Received,
    done: &mut Synced,
    progress: &openmailclient_model::Progress,
) -> Result<bool, NetError> {
    let is_listed = |raw: &str| listed.iter().any(|f| f.raw == raw && f.selectable() && !is_view(f));
    let mut changed = false;
    let mut asked: HashMap<String, Status> = HashMap::new();
    let mut claimed: HashSet<String> = HashSet::new();
    // Those nearest the top first: a folder renamed with the one it is in is followed with it.
    let mut order: Vec<u64> = local.marks.iter().filter(|m| !is_listed(&m.raw)).map(|m| m.here).collect();
    order.sort_by_key(|h| local.marks.iter().find(|m| m.here == *h).map_or(0, |m| m.raw.len()));
    order.dedup();
    for h in order {
        let Some(m) = local.marks.iter().find(|m| m.here == h).cloned() else { continue };
        let here = FolderId(h);
        if is_listed(&m.raw) || m.raw.eq_ignore_ascii_case("INBOX") || progress.stopped() {
            continue;
        }
        let Some(f) = local.folder(here).cloned() else { continue };
        let candidates: Vec<&Folder> = listed
            .iter()
            .filter(|c| {
                c.selectable()
                    && !is_view(c)
                    && !c.raw.eq_ignore_ascii_case("INBOX")
                    && map.here(&c.raw).is_none()
                    && !claimed.contains(&c.raw)
                    && !local.marks.iter().any(|x| x.raw == c.raw)
            })
            .collect();
        let mut unknown = false;
        for c in &candidates {
            if asked.contains_key(&c.raw) {
                continue;
            }
            if asked.len() >= MAX_STATUS {
                unknown = true;
                continue;
            }
            match tolerate(imap.status(&c.raw))? {
                Some(st) => {
                    asked.insert(c.raw.clone(), st);
                }
                None => unknown = true,
            }
        }
        // By the server's id where it keeps them; by UIDVALIDITY where it does not.
        let matches: Vec<&&Folder> = candidates
            .iter()
            .filter(|c| {
                let st = asked.get(&c.raw);
                if m.mailbox_id.is_empty() {
                    m.uid_validity != 0 && st.and_then(|s| s.uid_validity) == Some(m.uid_validity)
                } else {
                    st.and_then(|s| s.mailbox_id.as_deref()) == Some(m.mailbox_id.as_str())
                }
            })
            .collect();
        match matches.as_slice() {
            [one] => {
                let to = one.raw.clone();
                claimed.insert(to.clone());
                follow_rename(&m.raw, &to, local, received, map, done);
                if !map.pairs.iter().any(|(r, _)| *r == to) {
                    map.pairs.push((to.clone(), here));
                }
                if own_folder(f.kind) {
                    let name = path_of(one).last().cloned().unwrap_or_default();
                    let top = local.top();
                    let parent = match parent_raw(&to, map.delimiter) {
                        None => Some(top),
                        Some(p) if map.under_inbox && p.eq_ignore_ascii_case("INBOX") => Some(top),
                        Some(p) => map.here(p).map(Some),
                    };
                    // Never into itself or a folder inside it.
                    let parent = parent.filter(|p| *p != f.parent && p.is_none_or(|p| !local.inside(p, here)));
                    if !name.is_empty() && (parent.is_some() || name != f.name) {
                        done.moved_here.push((here, parent, name));
                    }
                }
            }
            [] if !unknown => {
                // Gone from the server: what it holds stays here only.
                for i in &mut local.items {
                    let in_it = match &i.place {
                        Some(Place::On { raw, .. } | Place::Pending { raw }) => renamed_raw(raw, &m.raw, &m.raw, map.delimiter).is_some(),
                        _ => false,
                    };
                    if in_it {
                        i.place = Some(Place::Here);
                        done.replaced.push((i.folder, i.id, Place::Here));
                    }
                }
                if own_folder(f.kind) && !done.kept_here.iter().any(|k| local.inside(here, *k)) {
                    done.kept_here.push(here);
                }
            }
            // Two or more could be it, or not every one could be asked: left until it can be told.
            _ => {}
        }
    }
    remember(&mut map.pairs, listed, local);

    // This side: folders renamed or moved here since the two last agreed.
    let heres: Vec<u64> = local.marks.iter().map(|m| m.here).collect();
    for h in heres {
        if progress.stopped() {
            break;
        }
        let Some(m) = local.marks.iter().find(|m| m.here == h).cloned() else { continue };
        let (Some(f), Some((parent, name))) = (local.folder(FolderId(h)).cloned(), m.agreed.clone()) else { continue };
        let followed = done.moved_here.iter().any(|(x, ..)| x.0 == h) || done.renamed.iter().any(|(_, to, _)| *to == m.raw);
        if !own_folder(f.kind) || followed || !is_listed(&m.raw) || (f.parent.map(|p| p.0) == parent && f.name == name) {
            continue;
        }
        let Some(to) = wanted_raw(imap, local, map, &f, 0, done)? else { continue };
        if to == m.raw || listed.iter().any(|x| x.raw.eq_ignore_ascii_case(&to)) {
            continue;
        }
        if tolerate(imap.rename(&m.raw, &to))?.is_some() {
            follow_rename(&m.raw, &to, local, received, map, done);
            done.pushed += 1;
            changed = true;
        }
    }

    // Folders deleted here for good, those deepest first; only while the server's folder is
    // still the one deleted here.
    let mut going = local.folders_gone.clone();
    going.sort_by_key(|g| std::cmp::Reverse(g.raw.len()));
    for g in going {
        if progress.stopped() {
            break;
        }
        if !listed.iter().any(|f| f.raw == g.raw) {
            done.folders_deleted.push(g.raw);
            continue;
        }
        let Some(st) = tolerate(imap.status(&g.raw))? else { continue };
        if st.uid_validity != Some(g.uid_validity) || g.uid_validity == 0 {
            // Another folder by that name: it stays.
            done.folders_deleted.push(g.raw);
            continue;
        }
        if tolerate(imap.delete(&g.raw))?.is_some() {
            done.pushed += 1;
            changed = true;
            done.folders_deleted.push(g.raw);
        }
    }

    // Folders made here, as Outlook makes them on the server at once; not those with the deleted
    // mail, nor those the server lost.
    let bin = local.folders.iter().find(|f| f.kind == FolderKind::Deleted).map(|f| f.id);
    let new: Vec<FolderId> = local
        .folders
        .iter()
        .filter(|f| {
            own_folder(f.kind)
                && map.raw(f.id).is_none()
                && !bin.is_some_and(|b| local.inside(f.id, b))
                && !done.kept_here.iter().any(|k| local.inside(f.id, *k))
        })
        .map(|f| f.id)
        .collect();
    let before = done.created.len();
    for id in new {
        if progress.stopped() || done.created.len().saturating_sub(before) >= MAX_MADE {
            break;
        }
        make_on_server(imap, local, map, id, 0, done)?;
    }
    changed |= done.created.len() > before;

    // What each pair is as of now, to be remembered as agreed.
    for (raw, here) in &map.pairs {
        let Some(f) = local.folder(*here) else { continue };
        if raw.eq_ignore_ascii_case("INBOX") {
            continue;
        }
        let (parent, name) = match done.moved_here.iter().find(|(h, ..)| h == here) {
            Some((_, p, n)) => (p.unwrap_or(f.parent), n.clone()),
            None => (f.parent, f.name.clone()),
        };
        done.agreed.push((raw.clone(), *here, parent, name));
    }
    Ok(changed)
}

/// What was done here goes to the server: flags, moves, deletions and messages put here.
pub(super) fn push(
    imap: &mut Imap,
    map: &mut Map,
    local: &Local,
    done: &mut Synced,
    progress: &openmailclient_model::Progress,
) -> Result<(), NetError> {
    // Folders made here that mail is to go into are made on the server first.
    let mut wanted: Vec<FolderId> = Vec::new();
    for i in &local.items {
        let moved = matches!(&i.place, Some(Place::On { raw, .. }) if map.here(raw).is_some_and(|h| h != i.folder));
        if (moved || i.upload.is_some()) && map.raw(i.folder).is_none() && !wanted.contains(&i.folder) {
            wanted.push(i.folder);
        }
    }
    for here in wanted {
        make_on_server(imap, local, map, here, 0, done)?;
    }
    let pairs = map.pairs.clone();
    for (raw, here) in &pairs {
        if progress.stopped() {
            break;
        }
        let mine: Vec<&LocalItem> = local.items.iter().filter(|i| matches!(&i.place, Some(Place::On { raw: r, .. }) if r == raw)).collect();
        let changed = mine.iter().any(|i| match &i.place {
            Some(Place::On { seen, flagged, .. }) => i.folder != *here || i.read != *seen || i.flagged != *flagged,
            _ => false,
        });
        let gone: Vec<&Gone> = local.gone.iter().filter(|g| g.raw == *raw).collect();
        let uploads: Vec<&LocalItem> = local.items.iter().filter(|i| i.folder == *here && i.place.is_none() && i.upload.is_some()).collect();
        if !changed && gone.is_empty() && uploads.is_empty() {
            continue;
        }
        let Some(selected) = tolerate(imap.select(raw))? else { continue };
        let Some(v) = selected.uid_validity.filter(|_| !selected.read_only) else { continue };
        let mut expunge = Vec::new();
        for i in mine {
            let Some(Place::On { uid_validity, uid, seen, flagged, .. }) = &i.place else { continue };
            let uid = *uid;
            if *uid_validity != v {
                // The server numbered the folder anew: the message is found by its Message-ID
                // as the folder is taken down again.
                done.placed.push((i.folder, i.id, Place::Pending { raw: raw.clone() }));
                continue;
            }
            // Flags first, so that a message moved keeps what was done to it.
            let (mut seen, mut flagged) = (*seen, *flagged);
            let mut changed = false;
            if i.read != seen && tolerate(imap.flag(uid, i.read, &["\\Seen"]))?.is_some() {
                seen = i.read;
                changed = true;
            }
            if i.flagged != flagged && tolerate(imap.flag(uid, i.flagged, &["\\Flagged"]))?.is_some() {
                flagged = i.flagged;
                changed = true;
            }
            done.pushed += usize::from(changed);
            if i.folder == *here {
                if changed {
                    done.placed.push((i.folder, i.id, Place::On { raw: raw.clone(), uid_validity: v, uid, seen, flagged }));
                }
                continue;
            }
            let place = match map.raw(i.folder) {
                Some(to) => {
                    let Some(moved) = tolerate(imap.move_to(uid, &to))? else { continue };
                    if moved.left {
                        expunge.push(uid);
                    }
                    match moved.there {
                        Some((v2, u2)) => {
                            done.ours.insert((to.clone(), u2));
                            Place::On { raw: to, uid_validity: v2, uid: u2, seen, flagged }
                        }
                        None => Place::Pending { raw: to },
                    }
                }
                // Into Deleted Items where the server has no folder for deleted mail: deleted
                // there.
                None if local.kind(i.folder) == Some(FolderKind::Deleted) => {
                    if tolerate(imap.flag(uid, true, &["\\Deleted"]))?.is_none() {
                        continue;
                    }
                    expunge.push(uid);
                    Place::Here
                }
                // Into a folder whose mail stays here, such as Drafts: the server keeps it
                // where it was, and the two are no longer one.
                None => Place::Here,
            };
            done.pushed += 1;
            done.placed.push((i.folder, i.id, place));
        }
        for g in gone {
            if g.uid_validity == v && tolerate(imap.flag(g.uid, true, &["\\Deleted"]))?.is_some() {
                expunge.push(g.uid);
                done.pushed += 1;
            }
            // Done, or numbered anew since and no longer to be found by its id.
            done.deleted.push(g.clone());
        }
        tolerate(imap.expunge(&expunge))?;
        for i in uploads {
            let Some(data) = &i.upload else { continue };
            let mut flags = Vec::new();
            if i.read {
                flags.push("\\Seen");
            }
            if i.flagged {
                flags.push("\\Flagged");
            }
            if i.draft {
                flags.push("\\Draft");
            }
            let Some(put) = tolerate(imap.append(raw, &flags, data))? else { continue };
            let place = match put {
                Some(uid) => {
                    done.ours.insert((raw.clone(), uid));
                    Place::On { raw: raw.clone(), uid_validity: v, uid, seen: i.read, flagged: i.flagged }
                }
                None => Place::Pending { raw: raw.clone() },
            };
            done.pushed += 1;
            done.placed.push((i.folder, i.id, place));
        }
    }
    Ok(())
}

/// Whether the whole list of a folder is wanted: some messages here have their place in it, or
/// they are to be found there the first time.
pub(super) fn wants_list(local: &Local, raw: &str) -> bool {
    !local.matched || local.items.iter().any(|i| matches!(&i.place, Some(Place::On { raw: r, .. }) if r == raw))
}

/// What was done on the server to a folder comes here: flags, and messages it no longer has.
/// `listed` is the folder's whole list as the server gave it. True when it was the whole list,
/// so that nothing it lacks is still there.
pub(super) fn pull(
    imap: &mut Imap,
    raw: &str,
    here: FolderId,
    selected: &Selected,
    listed: &[Summary],
    local: &Local,
    done: &mut Synced,
) -> Result<bool, NetError> {
    let Some(v) = selected.uid_validity else { return Ok(false) };
    let on: HashMap<u32, &Summary> = listed.iter().map(|m| (m.uid, m)).collect();
    // The list is the folder's whole list only when it holds as many as the folder does.
    let whole = u64::try_from(listed.len()).is_ok_and(|n| n == selected.exists);
    compare(raw, here, v, &on, |_| whole, local, done);
    if !local.matched {
        let upto = listed.iter().map(|m| m.uid).max().unwrap_or(0);
        for (uid, message_id) in imap.message_ids(upto)? {
            let Some(m) = on.get(&uid) else { continue };
            done.found.push(Found {
                raw: raw.to_string(),
                here,
                uid_validity: v,
                uid,
                message_id,
                seen: has(m, "\\Seen"),
                flagged: has(m, "\\Flagged"),
            });
        }
    }
    Ok(whole)
}

/// What changed in a folder since it was last kept in step, where the server keeps
/// mod-sequences: the messages whose flags changed, and those that went, made sure of. None
/// when what went is not known from what the server says, and the whole list is to be read.
pub(super) fn since(imap: &mut Imap, selected: &Selected, was: &Step, now: &Step, local: &Local) -> Result<Option<Delta>, NetError> {
    let counted = now.uid_next > 0 && was.uid_next > 0;
    // Nothing changed: no flag, as the mod-sequence says, and nothing came or went, as the
    // count and the next id say.
    if counted && now.modseq == was.modseq && now.exists == was.exists && now.uid_next == was.uid_next {
        return Ok(Some(Delta::default()));
    }
    let known: Vec<u32> = local
        .items
        .iter()
        .filter_map(|i| match &i.place {
            Some(Place::On { raw, uid_validity, uid, .. }) if *raw == now.raw && *uid_validity == now.uid_validity => Some(*uid),
            _ => None,
        })
        .collect();
    let gone = match selected.since.as_ref().filter(|s| !s.too_much) {
        // The server said what went: what it named that is here is made sure of, as it may
        // name ids that are still there by mistake.
        Some(s) => {
            let named: Vec<u32> = known.iter().copied().filter(|u| s.vanished.contains(*u)).collect();
            if named.is_empty() {
                HashSet::new()
            } else {
                let there = imap.still_there(&named)?;
                named.into_iter().filter(|u| !there.contains(u)).collect()
            }
        }
        // As many more as ids were given out since: none went. Otherwise it is not known.
        None if counted && now.uid_next >= was.uid_next && now.exists == was.exists.saturating_add(u64::from(now.uid_next - was.uid_next)) => {
            HashSet::new()
        }
        None => return Ok(None),
    };
    // Asked for only once it is known the whole list need not be read.
    let changed = match &selected.since {
        Some(s) => s.changed.clone(),
        None if now.modseq == was.modseq => Vec::new(),
        None => imap.changed_since(was.modseq)?,
    };
    Ok(Some(Delta { changed, gone }))
}

/// What changed in a folder since it was last kept in step.
#[derive(Default)]
pub(super) struct Delta {
    pub changed: Vec<Summary>,
    pub gone: HashSet<u32>,
}

/// What a folder's changes since it was last kept in step come to here.
pub(super) fn pull_since(raw: &str, here: FolderId, uid_validity: u32, delta: &Delta, local: &Local, done: &mut Synced) {
    let on: HashMap<u32, &Summary> = delta.changed.iter().map(|m| (m.uid, m)).collect();
    compare(raw, here, uid_validity, &on, |uid| delta.gone.contains(&uid), local, done);
}

/// Each message here placed in a folder of the server, against that folder's messages `on`
/// it: flags the server changed come here, and a message it lacks goes when `absent` says it
/// is gone.
fn compare(raw: &str, here: FolderId, v: u32, on: &HashMap<u32, &Summary>, absent: impl Fn(u32) -> bool, local: &Local, done: &mut Synced) {
    let touched: HashSet<ItemId> = done.placed.iter().map(|(_, id, _)| *id).collect();
    for i in &local.items {
        let Some(Place::On { raw: r, uid_validity, uid, seen, flagged }) = &i.place else { continue };
        if r != raw || *uid_validity != v || touched.contains(&i.id) || i.folder != here {
            continue;
        }
        match on.get(uid) {
            Some(m) => {
                let (s_seen, s_flagged) = (has(m, "\\Seen"), has(m, "\\Flagged"));
                if s_seen == *seen && s_flagged == *flagged {
                    continue;
                }
                done.pulled.push(Pull {
                    folder: i.folder,
                    id: i.id,
                    was_read: i.read,
                    was_flagged: i.flagged,
                    read: (s_seen != *seen && i.read == *seen).then_some(s_seen),
                    flagged: (s_flagged != *flagged && i.flagged == *flagged).then_some(s_flagged),
                    place: Place::On { raw: raw.to_string(), uid_validity: v, uid: *uid, seen: s_seen, flagged: s_flagged },
                });
            }
            None if absent(*uid) => done.removed.push((i.folder, i.id, *uid)),
            None => {}
        }
    }
}

/// What the session took in from keeping in step.
#[derive(Default)]
pub(super) struct Applied {
    /// Messages changed or taken away here.
    pub changed: usize,
    pub removed: usize,
    pub why: Option<String>,
}

/// Take in what keeping in step came to: places, the server's flags, messages it no longer
/// has, and the first time, the place of each message taken down before.
pub(super) fn apply(s: &mut Session, store: usize, local: &Local, done: &Synced, received: &mut Received) -> Applied {
    let path = s.stores.get(store).and_then(|st| st.path()).map(|p| p.display().to_string()).unwrap_or_default();
    // What was read apart is remembered first, so that what changes below is forgotten after.
    for (key, known) in &local.known {
        if s.places.by.len() < MAX_KNOWN {
            s.places.by.insert(key.clone(), known.clone());
        }
    }
    let mut out = Applied::default();
    let mut edits: Vec<(FolderId, ItemId, Box<dyn Fn(&mut Item) -> bool + '_>)> = Vec::new();
    for (folder, id, place) in done.replaced.iter().chain(&done.placed) {
        edits.push((
            *folder,
            *id,
            Box::new(move |item: &mut Item| {
                place.put(item);
                false
            }),
        ));
    }
    for p in &done.pulled {
        edits.push((
            p.folder,
            p.id,
            Box::new(move |item: &mut Item| {
                let mut changed = false;
                // Taken only while the item is as it was: a change made here meanwhile goes to
                // the server next time.
                if let Some(read) = p.read
                    && item.is_read() == p.was_read
                {
                    item.set_read(read);
                    changed = true;
                }
                if let Some(on) = p.flagged
                    && flagged(item.flag()) == p.was_flagged
                {
                    item.set_flag(if on { Flag::Flagged } else { Flag::None });
                    changed = true;
                }
                p.place.put(item);
                changed
            }),
        ));
    }
    // The first time: each message here that came before it knew its place is found on the
    // server by its Message-ID, each message there standing for one here.
    let mut used: HashSet<(&str, u32)> = HashSet::new();
    if !local.matched {
        let mut found: Vec<(&LocalItem, Place)> = Vec::new();
        for i in local.items.iter().filter(|i| i.place.is_none() && !i.message_id.is_empty()) {
            if let Some(f) =
                done.found.iter().find(|f| f.here == i.folder && f.message_id == i.message_id && !used.contains(&(f.raw.as_str(), f.uid)))
            {
                used.insert((f.raw.as_str(), f.uid));
                found.push((i, Place::On { raw: f.raw.clone(), uid_validity: f.uid_validity, uid: f.uid, seen: f.seen, flagged: f.flagged }));
            }
        }
        for (i, place) in found {
            edits.push((
                i.folder,
                i.id,
                Box::new(move |item: &mut Item| {
                    place.put(item);
                    false
                }),
            ));
        }
        if done.matched {
            // What is not on the server stays here only: it is not put there as if it were new.
            for i in local.items.iter().filter(|i| i.place.is_none()) {
                if !edits.iter().any(|(f, id, _)| *f == i.folder && *id == i.id) {
                    edits.push((
                        i.folder,
                        i.id,
                        Box::new(|item: &mut Item| {
                            if Place::of(item).is_none() {
                                Place::Here.put(item);
                            }
                            false
                        }),
                    ));
                }
            }
            received.matched = true;
        }
    }
    for (folder, id, edit) in edits {
        let Some(st) = s.stores.get_mut(store) else { break };
        let Ok(mut item) = st.load(folder, id) else { continue };
        let changed = edit(&mut item);
        match st.update(folder, id, item) {
            Ok(()) => out.changed += usize::from(changed),
            Err(e) => {
                out.why.get_or_insert(format!("a message could not be changed as the server has it: {e}"));
            }
        }
        s.places.forget(&path, folder, id);
    }
    for (folder, id, uid) in &done.removed {
        let Some(st) = s.stores.get_mut(store) else { break };
        // Only the message that had that place: an item here since by the same id stays.
        let same = st.load(*folder, *id).ok().and_then(|item| Place::of(&item)).is_some_and(|p| matches!(p, Place::On { uid: u, .. } if u == *uid));
        if same && st.remove(*folder, *id).is_ok() {
            out.removed += 1;
        }
        s.places.forget(&path, *folder, *id);
    }
    received.gone.retain(|g| !done.deleted.contains(g));
    // Where each folder was left is kept only when all it came to was taken in: what was not
    // is asked for again next time.
    if out.why.is_none() {
        for step in &done.steps {
            received.stepped(step.clone());
        }
    }
    out
}

/// A message taken down from a folder that was put on the server from here without its id
/// being known: the item here that stands for it, found by its Message-ID, is given its place
/// and the message is not kept twice. True when one was found.
pub(super) fn adopt(
    s: &mut Session,
    store: usize,
    local: &Local,
    adopted: &mut HashSet<ItemId>,
    folder: FolderId,
    raw: &str,
    message_id: &str,
    place: &Place,
) -> bool {
    if message_id.is_empty() {
        return false;
    }
    let sent = local.kind(folder) == Some(FolderKind::Sent);
    let Place::On { uid_validity: now, .. } = place else { return false };
    let Some(i) = local.items.iter().find(|i| {
        i.folder == folder
            && i.message_id == message_id
            && !adopted.contains(&i.id)
            && match &i.place {
                Some(Place::Pending { raw: r }) => r == raw || (r.is_empty() && sent),
                // The folder was numbered anew since.
                Some(Place::On { raw: r, uid_validity, .. }) => r == raw && uid_validity != now,
                _ => false,
            }
    }) else {
        return false;
    };
    let path = s.stores.get(store).and_then(|st| st.path()).map(|p| p.display().to_string()).unwrap_or_default();
    let Some(st) = s.stores.get_mut(store) else { return false };
    let Ok(mut item) = st.load(i.folder, i.id) else { return false };
    place.put(&mut item);
    let ok = st.update(i.folder, i.id, item).is_ok();
    s.places.forget(&path, i.folder, i.id);
    if ok {
        adopted.insert(i.id);
    }
    ok
}

/// The IMAP account whose own data file is the store, if one is.
fn account_of(s: &mut Session, store: usize) -> Option<&mut crate::accounts::Account> {
    let path = s.stores.get(store)?.path()?.display().to_string();
    s.accounts.iter_mut().find(|a| a.kind == Kind::Imap && !a.received.into.is_empty() && a.received.into == path)
}

/// Folders deleted for good from an IMAP account's data file are deleted from the server too at
/// the next Send/Receive: each that was paired with a folder there. Asked before they go.
pub(crate) fn forget_folders(s: &mut Session, store: usize, folders: &[FolderId]) {
    let Some(a) = account_of(s, store) else { return };
    let going: Vec<GoneFolder> = a
        .received
        .folders
        .iter()
        .filter(|m| m.here != 0 && folders.contains(&FolderId(m.here)) && !m.raw.eq_ignore_ascii_case("INBOX"))
        .map(|m| GoneFolder { raw: m.raw.clone(), uid_validity: m.uid_validity })
        .collect();
    for g in going {
        a.received.delete_folder_on_server(g);
    }
}

/// A message deleted for good from an IMAP account's data file is deleted from the server too
/// at the next Send/Receive.
pub(crate) fn forget(s: &mut Session, store: usize, item: &Item) {
    let Some(Place::On { raw, uid_validity, uid, .. }) = Place::of(item) else { return };
    if let Some(a) = account_of(s, store) {
        a.received.delete_on_server(Gone { raw, uid_validity, uid });
    }
}

/// A message deleted for good is brought back: it is kept from being deleted from the server,
/// or, when that was done already, put there again as new.
pub(crate) fn back(s: &mut Session, store: usize, item: &mut Item) {
    let Some(Place::On { raw, uid_validity, uid, .. }) = Place::of(item) else { return };
    let g = Gone { raw, uid_validity, uid };
    let Some(a) = account_of(s, store) else { return };
    if a.received.gone.contains(&g) {
        a.received.gone.retain(|x| *x != g);
    } else {
        Place::clear(item);
    }
}
