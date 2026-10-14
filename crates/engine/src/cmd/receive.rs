//! Receiving: new mail is taken down from each account's incoming server into the Inbox of the
//! account's own data file (`account::account_store`), or, where the host keeps none, of the
//! data file new mail is kept in, on a thread of its own. An account's own data file is saved
//! once mail has come into it. Nothing is taken down twice: an
//! account remembers what it took (`accounts::Received`), and that is moved on only once the
//! message is in the Inbox.
//!
//! The server is changed as little as can be: IMAP messages are read without marking them read
//! (`BODY.PEEK`), and a POP3 message is taken off the server, when the account says so, only on
//! a later receive, once the data file it went into has been saved; a message is never removed
//! from a server before it is kept here.

use super::account::Reach;
use super::{CommandSpec, p_bool};
use crate::accounts::{Kind, MAX_SEEN, Received};
use crate::jobs::Ended;
use crate::{CmdResult, EngineError, Session};
use openmailclient_model::{Attachment, FolderId, FolderKind, Item, MSGFLAG_READ, PropValue, pid};
use openmailclient_net::NetError;
use openmailclient_net::imap::Imap;
use openmailclient_net::pop3::Pop3;
use serde_json::{Value, json};

/// Messages taken down from one account in one receive, and how many bytes of them; the rest
/// come with the next.
const MAX_PER_RUN: usize = 300;
const MAX_BYTES: u64 = 256 * 1024 * 1024;

/// The words a receive job begins with; one is under way while a job so named is.
const RECEIVING: &str = "Receiving";

fn has_accounts(s: &Session) -> bool {
    !s.accounts.is_empty()
}

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "mail.sendReceive",
            label: "Send/Receive All Folders",
            tab: "Send / Receive",
            group: "Send & Receive",
            shortcut: "F9",
            params: r#"{"wait": false to do it on threads of their own}; sends what is in the Outbox, then takes new mail down from every account; returns {"send", "receive"} as mail.sendAll and account.receive give them"#,
            enabled: has_accounts,
            run: send_receive,
        },
        CommandSpec {
            id: "account.receive",
            label: "Receive",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id": one account (every account when not given), "wait": false to receive on a thread of its own}; takes new mail down into the Inbox; returns {"received", "accounts": [{"id", "received", "waiting": what is left on the server for next time, "said": why it stopped, if it did}]}"#,
            enabled: has_accounts,
            run: receive,
        },
    ]
}

fn send_receive(s: &mut Session, p: &Value) -> CmdResult {
    let wait = p_bool(p, "wait") != Some(false);
    // An Outbox with nothing in it, or none at all, is not a reason to take no mail down.
    let send = super::send::send_outbox(s, wait).unwrap_or_else(|e| json!({ "said": e.to_string() }));
    let status = s.status.clone();
    let receive = receive_from(s, None, wait)?;
    if wait && !status.is_empty() {
        s.status = format!("{status}; {}", s.status);
    }
    Ok(json!({ "send": send, "receive": receive }))
}

fn receive(s: &mut Session, p: &Value) -> CmdResult {
    let one = match p.get("id") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) => Some(
            s.accounts
                .iter()
                .position(|a| &a.id == id)
                .ok_or_else(|| EngineError::BadParams(format!("there is no account {}", id.chars().take(64).collect::<String>())))?,
        ),
        Some(_) => return Err(EngineError::BadParams("\"id\" is an account's id".into())),
    };
    receive_from(s, one, p_bool(p, "wait") != Some(false))
}

/// One account's part of a receive.
struct Taking {
    reach: Reach,
    received: Received,
    /// POP3: messages taken down before are taken off the server this time.
    take_off: bool,
}

/// A message taken down.
struct Got {
    /// The IMAP id, or the POP3 unique id.
    uid: Uid,
    /// IMAP: the server has it as read.
    read: bool,
    data: Vec<u8>,
}

enum Uid {
    Imap(u32),
    Pop(String),
}

/// What one account's part came to.
struct Took {
    id: String,
    name: String,
    got: Vec<Got>,
    /// IMAP: the Inbox's UIDVALIDITY as read now.
    uid_validity: Option<u32>,
    /// POP3: every unique id the server has now, when it said them.
    on_server: Option<Vec<String>>,
    /// Messages left on the server for the next receive.
    waiting: usize,
    error: Option<NetError>,
}

/// Where an account's new mail goes: the Inbox of a data file, and whether the file is the
/// account's own.
#[derive(Clone, Copy)]
struct Target {
    store: usize,
    folder: FolderId,
    own: bool,
}

/// The data file mail is taken down into, as `Received::into` names it.
fn into_of(s: &Session, target: Target) -> String {
    if !target.own {
        return String::new();
    }
    s.stores.get(target.store).and_then(|st| st.path()).map(|p| p.display().to_string()).unwrap_or_default()
}

/// The data file and Inbox new mail goes in.
fn inbox(s: &Session) -> Option<(usize, FolderId)> {
    let store = s.writable_store()?;
    Some((store, s.find_folder(store, FolderKind::Inbox)?))
}

pub(crate) fn receive_from(s: &mut Session, one: Option<usize>, wait: bool) -> CmdResult {
    if s.accounts.is_empty() {
        return Err(EngineError::Format("there is no account to receive from; add one with account.add".into()));
    }
    if s.jobs.iter().any(|j| j.what.starts_with(RECEIVING)) {
        return Err(EngineError::Format("mail is being received already".into()));
    }
    let mut taking = Vec::new();
    let mut targets: Vec<(String, Target)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for at in 0..s.accounts.len() {
        if one.is_some_and(|o| o != at) {
            continue;
        }
        let target = match super::account::account_store(s, at) {
            Ok(Some(store)) => s.find_folder(store, FolderKind::Inbox).map(|folder| Target { store, folder, own: true }),
            Ok(None) => inbox(s).map(|(store, folder)| Target { store, folder, own: false }),
            Err(e) => {
                let name = s.accounts.get(at).map(|a| a.name.clone()).unwrap_or_default();
                skipped.push(format!("{name}: its data file could not be opened ({e})"));
                continue;
            }
        };
        let Some(account) = s.accounts.get(at) else { continue };
        let Some(target) = target else {
            skipped.push(format!("{}: there is no Inbox in a data file that can be written", account.name));
            continue;
        };
        // Taken off a POP3 server only what is kept for good: the data file is a file, and
        // holds no change that is not saved.
        let saved = s.stores.get(target.store).is_some_and(|st| st.path().is_some() && !st.unsaved());
        let (received, take_off) = (account.received.kept_in(&into_of(s, target)), !account.leave_on_server && saved);
        targets.push((account.id.clone(), target));
        taking.push(Taking { reach: Reach::of(s, at)?, received, take_off });
    }
    if taking.is_empty() {
        return Err(EngineError::Format(skipped.first().cloned().unwrap_or_else(|| "there is no account to receive from".into())));
    }
    let what = match taking.as_slice() {
        [one] => format!("{RECEIVING} from {}", one.reach.account.name),
        all => format!("{RECEIVING} from {} accounts", all.len()),
    };
    s.begin(what, wait, move |progress| {
        progress.begin(u64::try_from(taking.len()).unwrap_or(u64::MAX));
        let mut took = Vec::new();
        for t in taking {
            if progress.stopped() {
                break;
            }
            took.push(match t.reach.account.kind {
                Kind::Imap => from_imap(&t, progress),
                Kind::Pop3 => from_pop3(&t, progress),
            });
            progress.step();
        }
        let step: Ended = Box::new(move |s: &mut Session| ended(s, &targets, took, skipped));
        step
    })
}

fn took(t: &Taking) -> Took {
    Took {
        id: t.reach.account.id.clone(),
        name: t.reach.account.name.clone(),
        got: Vec::new(),
        uid_validity: None,
        on_server: None,
        waiting: 0,
        error: None,
    }
}

fn from_imap(t: &Taking, progress: &openmailclient_model::Progress) -> Took {
    let mut out = took(t);
    if let Err(e) = imap_into(t, progress, &mut out) {
        out.error = Some(e);
    }
    out
}

fn imap_into(t: &Taking, progress: &openmailclient_model::Progress, out: &mut Took) -> Result<(), NetError> {
    let login = t.reach.login()?;
    let mut imap = Imap::open(&*t.reach.connector, &t.reach.account.incoming_endpoint(), &login)?;
    let inbox = imap.select("INBOX")?;
    out.uid_validity = inbox.uid_validity;
    // Numbered anew, or never read: every message is new.
    let from = if inbox.uid_validity.is_some_and(|v| v == t.received.uid_validity) { t.received.last_uid.saturating_add(1) } else { 1 };
    let listed = if inbox.exists == 0 { Vec::new() } else { imap.list(from)? };
    let mut bytes = 0u64;
    for (n, m) in listed.iter().enumerate() {
        if n >= MAX_PER_RUN || (bytes > 0 && bytes.saturating_add(m.size) > MAX_BYTES) || progress.stopped() {
            out.waiting = listed.len().saturating_sub(n);
            break;
        }
        // One that is gone by now is passed over.
        if let Some(data) = imap.fetch(m.uid)? {
            bytes = bytes.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
            out.got.push(Got { uid: Uid::Imap(m.uid), read: m.flags.iter().any(|f| f.eq_ignore_ascii_case("\\Seen")), data });
        }
    }
    imap.logout()
}

fn from_pop3(t: &Taking, progress: &openmailclient_model::Progress) -> Took {
    let mut out = took(t);
    if let Err(e) = pop3_into(t, progress, &mut out) {
        out.error = Some(e);
    }
    out
}

fn pop3_into(t: &Taking, progress: &openmailclient_model::Progress, out: &mut Took) -> Result<(), NetError> {
    let login = t.reach.login()?;
    let mut pop = Pop3::open(&*t.reach.connector, &t.reach.account.incoming_endpoint(), &login)?;
    let listed = pop.list()?;
    let seen: std::collections::HashSet<&str> = t.received.seen.iter().map(String::as_str).collect();
    let mut bytes = 0u64;
    let mut new = 0usize;
    let mut staying = Vec::with_capacity(listed.len());
    for m in &listed {
        if seen.contains(m.uid.as_str()) {
            // Kept here, and saved, before: it may go from the server now.
            if t.take_off {
                pop.delete(m.number)?;
            } else {
                staying.push(m.uid.clone());
            }
            continue;
        }
        staying.push(m.uid.clone());
        if new >= MAX_PER_RUN || (bytes > 0 && bytes.saturating_add(m.size) > MAX_BYTES) || progress.stopped() {
            out.waiting += 1;
            continue;
        }
        let data = pop.retrieve(m.number)?;
        bytes = bytes.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
        new += 1;
        out.got.push(Got { uid: Uid::Pop(m.uid.clone()), read: false, data });
    }
    // Messages marked to go are taken off only when the session ends well ([RFC 1939] 6);
    // until then the server has them all.
    pop.quit()?;
    out.on_server = Some(staying);
    Ok(())
}

/// A message as it is kept: read as mail, or, when it cannot be, kept whole as an attachment so
/// nothing of it is lost.
fn item_of(got: &Got, now: i64) -> Item {
    let mut item = openmailclient_mime::parse(&got.data).unwrap_or_else(|e| {
        let mut item = Item::new_mail();
        item.set_subject(&format!("A message that could not be read ({e})"));
        item.set_body_text("The message the server gave is attached as it came.");
        item.attachments.push(Attachment::file("message.eml", "message/rfc822", got.data.clone()));
        item
    });
    if item.props.time(pid::MESSAGE_DELIVERY_TIME).is_none_or(|t| t <= 0 || t > now) {
        item.props.set(pid::MESSAGE_DELIVERY_TIME, PropValue::Time(now));
    }
    let flags = item.props.i32(pid::MESSAGE_FLAGS).unwrap_or(0) & !MSGFLAG_READ & !openmailclient_model::MSGFLAG_UNSENT;
    item.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(if got.read { flags | MSGFLAG_READ } else { flags }));
    item
}

/// Take back what a receive came to: each message into the Inbox, and the account moved on
/// past it only once it is there.
fn ended(s: &mut Session, targets: &[(String, Target)], took: Vec<Took>, mut said: Vec<String>) -> (String, Value) {
    let now = s.now.max(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_secs()).ok())
            .map_or(0, openmailclient_model::time::from_unix),
    );
    let mut all = 0usize;
    let mut unread = 0usize;
    let mut accounts = Vec::new();
    let mut touched: Vec<Target> = Vec::new();
    for t in took {
        let Some(&(_, Target { store, folder, own })) = targets.iter().find(|(id, _)| *id == t.id) else { continue };
        let into = into_of(s, Target { store, folder, own });
        let mut received = s.accounts.iter().find(|a| a.id == t.id).map(|a| a.received.kept_in(&into)).unwrap_or_default();
        if let Some(v) = t.uid_validity
            && v != received.uid_validity
        {
            received = Received { uid_validity: v, into, ..Received::default() };
        }
        let mut kept = 0usize;
        let mut trouble = t.error.as_ref().map(ToString::to_string);
        let mut new_seen = Vec::new();
        for got in &t.got {
            let item = item_of(got, now);
            let is_unread = !got.read;
            match s.stores.get_mut(store).map(|st| st.add(folder, item)) {
                Some(Ok(_)) => {
                    kept += 1;
                    unread += usize::from(is_unread);
                    match &got.uid {
                        Uid::Imap(uid) => received.last_uid = received.last_uid.max(*uid),
                        Uid::Pop(uid) => new_seen.push(uid.clone()),
                    }
                }
                // What could not be kept is not counted as taken down, and comes again.
                Some(Err(e)) => {
                    trouble = Some(format!("a message could not be kept: {e}"));
                    break;
                }
                None => break,
            }
        }
        // The POP3 ids remembered are those the server still has; ids it no longer has are
        // forgotten, which keeps the list as short as the server's.
        if let Some(on_server) = &t.on_server {
            let there: std::collections::HashSet<&str> = on_server.iter().map(String::as_str).collect();
            received.seen.retain(|u| there.contains(u.as_str()));
        }
        received.seen.extend(new_seen);
        if received.seen.len() > MAX_SEEN {
            let over = received.seen.len() - MAX_SEEN;
            received.seen.drain(..over);
        }
        if let Some(a) = s.accounts.iter_mut().find(|a| a.id == t.id) {
            a.received = received;
        }
        all += kept;
        if !touched.iter().any(|x| x.store == store) {
            touched.push(Target { store, folder, own: own && kept > 0 });
        } else if let Some(x) = touched.iter_mut().find(|x| x.store == store) {
            x.own |= own && kept > 0;
        }
        if let Some(why) = &trouble {
            said.push(format!("{}: {why}", t.name));
        }
        accounts.push(json!({ "id": t.id, "received": kept, "waiting": t.waiting, "said": trouble }));
    }
    for x in &touched {
        s.refresh_folders(x.store);
    }
    let _ = s.reload_rows();
    // An account's own data file is saved once mail is in it, on a thread of its own: the
    // owner is never asked to save mail that only came, and POP3 mail can leave the server.
    for x in touched.iter().filter(|x| x.own) {
        let status = std::mem::take(&mut s.status);
        let _ = super::file::save_in_background(s, x.store);
        s.status = status;
    }
    let mut status = match all {
        0 => "No new mail".to_string(),
        1 => "1 new message".to_string(),
        n => format!("{n} new messages"),
    };
    if unread > 0 && unread < all {
        status.push_str(&format!(", {unread} unread"));
    }
    let waiting: usize = accounts.iter().filter_map(|a| a["waiting"].as_u64()).map(|n| usize::try_from(n).unwrap_or(0)).sum();
    if waiting > 0 {
        status.push_str(&format!("; {waiting} more wait on the server for the next Send/Receive"));
    }
    if !said.is_empty() {
        status.push_str(&format!("; {}", said.join("; ")));
    }
    (status, json!({ "received": all, "accounts": accounts }))
}
