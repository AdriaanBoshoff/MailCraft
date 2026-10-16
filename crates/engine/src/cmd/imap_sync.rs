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
//! - a message the server no longer has goes from here too.
//!
//! Nothing is taken off the server that was not deleted or moved here by the owner, and a message
//! goes from here only when the server's whole list of a folder, as many as it says the folder
//! holds, lacks it. The first time, the messages taken down before they could know their place
//! are found on the server by their Message-ID; none of them is put there anew.
//!
//! Where the server keeps a mod-sequence for every change (CONDSTORE and QRESYNC, [RFC 7162]),
//! each folder remembers the one it was last kept in step at (`Step`): a folder that has not
//! changed since costs its SELECT and nothing more, and one that has is asked only for what
//! changed after it. That a message went is still made sure of before it goes here: a
//! VANISHED the server sends is checked against the folder, and where the server says nothing
//! of what went, the folder's count must add up or its whole list is read as before.

use super::receive::{is_view, kind_of, path_of};
use crate::Session;
use crate::accounts::{Gone, Kind, Received, Step};
use openmailclient_model::{Flag, FolderId, FolderKind, Item, ItemId, NameId, NamedKey, PropValue};
use openmailclient_net::NetError;
use openmailclient_net::imap::{Folder, Imap, Selected, Summary};
use std::collections::{HashMap, HashSet};

/// The property set of what this program keeps on an item for itself.
pub(crate) const PS_OPENMAILCLIENT: [u8; 16] = [0x4F, 0x4D, 0x43, 0x31, 0x8E, 0x2A, 0x4B, 0x7D, 0x9C, 0x15, 0x3A, 0x60, 0xD2, 0x7E, 0x51, 0xA4];
const PLACE: &str = "ImapPlace";

/// Messages put on a server in one Send/Receive, and their bytes; the rest go with the next.
const MAX_UPLOADS: usize = 300;
const MAX_UPLOAD_BYTES: u64 = 64 * 1024 * 1024;
/// Items whose place is remembered between receives, across every data file.
const MAX_KNOWN: usize = 1_000_000;

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

#[derive(Clone)]
struct Known {
    time: i64,
    subject: String,
    place: Option<Place>,
    message_id: String,
}

impl Places {
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
}

/// The account's data file as a receive began.
#[derive(Clone, Debug, Default)]
pub(super) struct Local {
    pub folders: Vec<LocalFolder>,
    pub items: Vec<LocalItem>,
    pub gone: Vec<Gone>,
    /// The messages taken down before they knew their place were found on the server.
    pub matched: bool,
}

impl Local {
    fn kind(&self, id: FolderId) -> Option<FolderKind> {
        self.folders.iter().find(|f| f.id == id).map(|f| f.kind)
    }
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

/// Folders whose mail goes on the server: not the Outbox, whose mail is not sent yet, nor Drafts,
/// which are still being written.
fn goes_up(kind: FolderKind) -> bool {
    matches!(
        kind,
        FolderKind::Inbox | FolderKind::Sent | FolderKind::Deleted | FolderKind::Junk | FolderKind::Archive | FolderKind::Mail | FolderKind::Other
    )
}

fn flagged(flag: Flag) -> bool {
    flag == Flag::Flagged
}

/// What the account's data file holds, for a receive to keep in step with the server: read once
/// for each item and remembered after.
pub(super) fn local(s: &mut Session, store: usize, received: &Received) -> Local {
    let path = s.stores.get(store).and_then(|st| st.path()).map(|p| p.display().to_string()).unwrap_or_default();
    let folders: Vec<LocalFolder> = s
        .folders
        .get(store)
        .map(|l| l.iter().map(|f| LocalFolder { id: f.id, parent: f.parent, name: f.name.clone(), kind: f.kind }).collect())
        .unwrap_or_default();
    if s.places.by.len() > MAX_KNOWN {
        s.places.by.clear();
    }
    let (mut items, mut uploads, mut bytes) = (Vec::new(), 0usize, 0u64);
    for f in folders.iter().filter(|f| holds_mail(f.kind)) {
        let Some(st) = s.stores.get_mut(store) else { break };
        let Ok(rows) = st.list(f.id) else { continue };
        for r in rows {
            let key = (path.clone(), f.id.0, r.id);
            let known = match s.places.by.get(&key).filter(|k| k.time == r.time && k.subject == r.subject) {
                Some(k) => k.clone(),
                None => {
                    let Ok(item) = st.load(f.id, ItemId(r.id)) else { continue };
                    let k = Known {
                        time: r.time,
                        subject: r.subject.clone(),
                        place: Place::of(&item),
                        message_id: item.props.str(openmailclient_model::pid::INTERNET_MESSAGE_ID).map(str::trim).unwrap_or_default().to_string(),
                    };
                    s.places.by.insert(key, k.clone());
                    k
                }
            };
            let mut upload = None;
            if known.place.is_none()
                && received.matched
                && goes_up(f.kind)
                && uploads < MAX_UPLOADS
                && bytes < MAX_UPLOAD_BYTES
                && let Ok(item) = st.load(f.id, ItemId(r.id))
                && !item.is_unsent()
                && item.message_class().to_ascii_uppercase().starts_with("IPM.")
            {
                let data = openmailclient_mime::write(&item);
                uploads += 1;
                bytes = bytes.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
                upload = Some(data);
            }
            items.push(LocalItem {
                folder: f.id,
                id: ItemId(r.id),
                place: known.place,
                read: r.read,
                flagged: flagged(r.flag),
                message_id: known.message_id,
                upload,
            });
        }
    }
    Local { folders, items, gone: received.gone.clone(), matched: received.matched }
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

    fn raw(&self, here: FolderId) -> Option<String> {
        self.pairs.iter().find(|(_, h)| *h == here).map(|(r, _)| r.clone())
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
    let Some(f) = local.folders.iter().find(|f| f.id == here) else { return Ok(None) };
    if depth > 32 || !goes_up(f.kind) || f.kind == FolderKind::Deleted || f.kind == FolderKind::Inbox {
        return Ok(None);
    }
    let top = local.folders.iter().find(|l| l.kind == FolderKind::Inbox).and_then(|l| l.parent);
    let name = openmailclient_net::utf7::encode(&f.name);
    if name.is_empty() || f.name.contains(|c: char| c.is_control() || Some(c) == map.delimiter || matches!(c, '*' | '%')) {
        return Ok(None);
    }
    let raw = if f.parent == top {
        match map.delimiter {
            Some(d) if map.under_inbox => format!("INBOX{d}{name}"),
            _ => name,
        }
    } else {
        let (Some(parent), Some(d)) = (f.parent, map.delimiter) else { return Ok(None) };
        let Some(above) = make_on_server(imap, local, map, parent, depth + 1, done)? else { return Ok(None) };
        format!("{above}{d}{name}")
    };
    if tolerate(imap.create(&raw))?.is_none() {
        return Ok(None);
    }
    map.pairs.push((raw.clone(), here));
    done.created.push(raw.clone());
    Ok(Some(raw))
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
    let mut out = Applied::default();
    let mut edits: Vec<(FolderId, ItemId, Box<dyn Fn(&mut Item) -> bool + '_>)> = Vec::new();
    for (folder, id, place) in &done.placed {
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
