//! Compose commands: new mail, reply, reply all, forward, and the compose window's own actions.
//! Nothing here touches a network: "Send" files the message in the Outbox.

use super::{CommandSpec, always, has_selection, need_str, ok, p_bool, p_u64};
use crate::{CmdResult, Draft, EngineError, Session, outgoing};
use mailcraft_model::{
    Attachment, FolderKind, Item, MSGFLAG_READ, MSGFLAG_UNSENT, PropValue, RECIP_BCC, RECIP_CC, RECIP_TO, StoreError, pid, recipient_email,
    recipient_label, time,
};
use serde_json::{Value, json};
use std::collections::HashSet;

/// One attached file is at most this large, and a draft carries at most this many.
const MAX_ATTACHMENT: u64 = 150 * 1024 * 1024;
const MAX_ATTACHMENTS: usize = 500;
/// Messages set aside that are kept; an older one is forgotten when a newer one comes.
const MAX_SET_ASIDE: usize = 20;
/// Field text is clamped: a subject line, an address line, a body.
const MAX_LINE: usize = 32 * 1024;
const MAX_BODY: usize = 16 * 1024 * 1024;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec { id: "mail.new", label: "New Email", tab: "Home", group: "New", shortcut: "Ctrl+N", params: "", enabled: always, run: new },
        CommandSpec {
            id: "mail.reply",
            label: "Reply",
            tab: "Home",
            group: "Respond",
            shortcut: "Ctrl+R",
            params: "",
            enabled: has_selection,
            run: reply,
        },
        CommandSpec {
            id: "mail.replyAll",
            label: "Reply All",
            tab: "Home",
            group: "Respond",
            shortcut: "Ctrl+Shift+R",
            params: "",
            enabled: has_selection,
            run: reply_all,
        },
        CommandSpec {
            id: "mail.forward",
            label: "Forward",
            tab: "Home",
            group: "Respond",
            shortcut: "Ctrl+F",
            params: "",
            enabled: has_selection,
            run: forward,
        },
        CommandSpec {
            id: "compose.setField",
            label: "Set Field",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"field": "to" | "cc" | "bcc" | "subject" | "body", "value": text}"#,
            enabled: composing,
            run: set_field,
        },
        CommandSpec {
            id: "compose.showBcc",
            label: "Bcc",
            tab: "Options",
            group: "Show Fields",
            shortcut: "",
            params: r#"{"show": true | false}; default toggles"#,
            enabled: composing,
            run: show_bcc,
        },
        CommandSpec {
            id: "compose.attach",
            label: "Attach File",
            tab: "Message",
            group: "Include",
            shortcut: "",
            params: r#"{"path": file to attach}"#,
            enabled: composing,
            run: attach,
        },
        CommandSpec {
            id: "compose.removeAttachment",
            label: "Remove Attachment",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"index": attachment number}"#,
            enabled: composing,
            run: remove_attachment,
        },
        CommandSpec {
            id: "compose.check",
            label: "Check Before Sending",
            tab: "Message",
            group: "Message",
            shortcut: "",
            params: r#"returns {"cautions": what the writer should know before the message goes}"#,
            enabled: composing,
            run: check,
        },
        CommandSpec {
            id: "compose.send",
            label: "Send",
            tab: "Message",
            group: "",
            shortcut: "Ctrl+Enter",
            params: "",
            enabled: composing,
            run: send,
        },
        CommandSpec {
            id: "compose.saveDraft",
            label: "Save",
            tab: "Message",
            group: "",
            shortcut: "Ctrl+S",
            params: "",
            enabled: composing,
            run: save_draft,
        },
        CommandSpec {
            id: "compose.discard",
            label: "Discard",
            tab: "Message",
            group: "",
            shortcut: "Esc",
            params: "what was written is set aside, and Undo or compose.restore brings it back",
            enabled: composing,
            run: discard,
        },
        CommandSpec {
            id: "compose.restore",
            label: "Restore Discarded Message",
            tab: "Message",
            group: "",
            shortcut: "",
            params: r#"brings back the message last discarded, or replaced by a new one, as it was; returns {"left": how many more are set aside}"#,
            enabled: has_set_aside,
            run: restore,
        },
    ]
}

fn composing(s: &Session) -> bool {
    s.compose.is_some()
}

fn has_set_aside(s: &Session) -> bool {
    !s.set_aside.is_empty()
}

/// Close the message being written. What its writer put into it is kept, so that starting
/// another message, or discarding this one, loses nothing. Says whether there was anything.
fn set_aside(s: &mut Session) -> bool {
    match s.compose.take() {
        Some(draft) if draft.touched => {
            if s.set_aside.len() >= MAX_SET_ASIDE {
                s.set_aside.remove(0);
            }
            s.set_aside.push(draft);
            true
        }
        _ => false,
    }
}

/// Start a new message to one mailbox, setting aside any that was being written.
pub(super) fn start_to(s: &mut Session, to: String) {
    set_aside(s);
    s.compose = Some(Draft { kind: "New", to, ..Draft::default() });
}

fn new(s: &mut Session, _: &Value) -> CmdResult {
    set_aside(s);
    s.compose = Some(Draft { kind: "New", ..Draft::default() });
    ok()
}

/// "Name <address>", or just the address.
fn mailbox(name: &str, email: &str) -> String {
    if name.is_empty() || name == email {
        email.to_string()
    } else if email.is_empty() {
        name.to_string()
    } else {
        format!("{name} <{email}>")
    }
}

/// Add `prefix` unless the subject already starts with it (Outlook does not stack "RE: RE:").
fn prefixed(prefix: &str, subject: &str) -> String {
    let has = subject.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix));
    if has { subject.to_string() } else { format!("{prefix} {subject}").trim_end().to_string() }
}

/// The header block Outlook puts above the quoted text of a plain-text reply or forward.
fn quoted(s: &Session, item: &Item) -> String {
    let mut out = format!("\n\n{}\n", outgoing::ORIGINAL);
    out.push_str(&format!("From: {}\n", mailbox(&item.sender_name(), &item.sender_email())));
    if item.time() > 0 {
        out.push_str(&format!("Sent: {}\n", time::civil(item.time(), s.tz_offset_min).full_stamp()));
    }
    out.push_str(&format!("To: {}\n", item.display_recipients(RECIP_TO)));
    let cc = item.display_recipients(RECIP_CC);
    if !cc.is_empty() {
        out.push_str(&format!("Cc: {cc}\n"));
    }
    out.push_str(&format!("Subject: {}\n\n", item.subject()));
    out.push_str(&item.body_text());
    out
}

/// The recipients of `item` of one kind, each address once: one that is in `seen` already is
/// left out, and the others are added to it.
fn recipients_of(item: &Item, kind: i32, seen: &mut HashSet<String>) -> Vec<String> {
    item.recipients
        .iter()
        .filter(|r| r.i32(pid::RECIPIENT_TYPE).unwrap_or(RECIP_TO) == kind)
        .filter(|r| {
            let address = recipient_email(r).trim().to_lowercase();
            address.is_empty() || seen.insert(address)
        })
        .map(|r| mailbox(&recipient_label(r), &recipient_email(r)))
        .collect()
}

fn respond(s: &mut Session, kind: &'static str) -> CmdResult {
    let item = s.open.clone().ok_or(StoreError::NoItem)?;
    let sender = mailbox(&item.sender_name(), &item.sender_email());
    let mut draft = Draft { kind, body: quoted(s, &item), ..Draft::default() };
    match kind {
        "Forward" => {
            draft.subject = prefixed("FW:", &item.subject());
            draft.attachments = item.attachments.clone();
        }
        _ => {
            draft.subject = prefixed("RE:", &item.subject());
            let own = s.user_email.trim().to_lowercase();
            let from = item.sender_email().trim().to_lowercase();
            // Nobody is named twice, and the writer not at all, however the message listed them.
            let mut seen: HashSet<String> = [own.clone(), from.clone()].into_iter().filter(|a| !a.is_empty()).collect();
            if !own.is_empty() && own == from {
                // An answer to one's own message goes to those it went to, not back to oneself.
                draft.to = recipients_of(&item, RECIP_TO, &mut seen).join("; ");
                if kind == "Reply All" {
                    draft.cc = recipients_of(&item, RECIP_CC, &mut seen).join("; ");
                }
            } else {
                draft.to = sender.clone();
                if kind == "Reply All" {
                    let named = |kind: i32| {
                        item.recipients
                            .iter()
                            .filter(|r| r.i32(pid::RECIPIENT_TYPE).unwrap_or(RECIP_TO) == kind)
                            .any(|r| recipient_email(r).trim().to_lowercase() == own)
                    };
                    draft.was_hidden = !own.is_empty() && !item.recipients.is_empty() && !named(RECIP_TO) && !named(RECIP_CC);
                    let others = recipients_of(&item, RECIP_TO, &mut seen);
                    if !others.is_empty() {
                        draft.to = format!("{sender}; {}", others.join("; "));
                    }
                    draft.cc = recipients_of(&item, RECIP_CC, &mut seen).join("; ");
                }
            }
        }
    }
    set_aside(s);
    s.compose = Some(draft);
    ok()
}

fn reply(s: &mut Session, _: &Value) -> CmdResult {
    respond(s, "Reply")
}

fn reply_all(s: &mut Session, _: &Value) -> CmdResult {
    respond(s, "Reply All")
}

fn forward(s: &mut Session, _: &Value) -> CmdResult {
    respond(s, "Forward")
}

fn clamp(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("").to_string()
}

fn set_field(s: &mut Session, p: &Value) -> CmdResult {
    let field = need_str(p, "field")?.to_ascii_lowercase();
    let value = need_str(p, "value")?;
    let draft = s.compose.as_mut().ok_or_else(|| EngineError::Disabled("Set Field".into()))?;
    let one_line = clamp(&value.replace("\r\n", " ").replace(['\r', '\n'], " "), MAX_LINE);
    match field.as_str() {
        "to" => draft.to = one_line,
        "cc" => draft.cc = one_line,
        "bcc" => {
            draft.bcc = one_line;
            draft.show_bcc = true;
        }
        "subject" => draft.subject = one_line,
        "body" => draft.body = clamp(value, MAX_BODY),
        other => return Err(EngineError::BadParams(format!("\"{other}\" is not to, cc, bcc, subject or body"))),
    }
    draft.touched = true;
    ok()
}

fn show_bcc(s: &mut Session, p: &Value) -> CmdResult {
    if let Some(d) = s.compose.as_mut() {
        d.show_bcc = p_bool(p, "show").unwrap_or(!d.show_bcc);
    }
    ok()
}

fn attach(s: &mut Session, p: &Value) -> CmdResult {
    let path = std::path::Path::new(need_str(p, "path")?);
    let draft = s.compose.as_mut().ok_or_else(|| EngineError::Disabled("Attach File".into()))?;
    if draft.attachments.len() >= MAX_ATTACHMENTS {
        return Err(EngineError::Format("this message already has as many attachments as it can carry".into()));
    }
    let about = std::fs::metadata(path)?;
    if about.is_dir() {
        return Err(EngineError::BadParams(format!("{} is a folder, not a file", path.display())));
    }
    if about.len() > MAX_ATTACHMENT {
        return Err(EngineError::Format("this file is too large to attach".into()));
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "attachment".to_string());
    let data = std::fs::read(path)?;
    // The same file a second time is a slip, not a wish.
    if draft.attachments.iter().any(|a| a.embedded.is_none() && a.name() == name && a.data == data) {
        return Err(EngineError::Format(format!("{name} is attached already")));
    }
    let size = data.len();
    draft.attachments.push(Attachment::file(&name, "", data));
    draft.touched = true;
    let total = draft.attachments.iter().fold(0usize, |sum, a| sum.saturating_add(a.size()));
    Ok(json!({ "index": draft.attachments.len().saturating_sub(1), "size": size, "total": total }))
}

fn remove_attachment(s: &mut Session, p: &Value) -> CmdResult {
    let index = usize::try_from(p_u64(p, "index").unwrap_or(0)).unwrap_or(usize::MAX);
    let draft = s.compose.as_mut().ok_or_else(|| EngineError::Disabled("Remove Attachment".into()))?;
    if index >= draft.attachments.len() {
        return Err(EngineError::BadParams(format!("there is no attachment {index}")));
    }
    draft.attachments.remove(index);
    draft.touched = true;
    ok()
}

/// Turn the draft into an item. `sent` stamps the submit time and clears the unsent flag.
fn build(s: &Session, draft: &Draft, sent: bool) -> Item {
    let mut item = Item::new_mail();
    item.set_subject(&draft.subject);
    item.set_body_text(&draft.body);
    item.set_sender(&s.user_name, &s.user_email);
    for (kind, line) in [(RECIP_TO, &draft.to), (RECIP_CC, &draft.cc), (RECIP_BCC, &draft.bcc)] {
        for (name, email) in mailcraft_mime::parse_addresses(line) {
            item.add_recipient(kind, &name, &email);
        }
    }
    item.refresh_display_recipients();
    item.attachments = draft.attachments.clone();
    item.props.set(pid::CREATION_TIME, PropValue::Time(s.now));
    item.props.set(pid::LAST_MODIFICATION_TIME, PropValue::Time(s.now));
    if sent {
        item.props.set(pid::CLIENT_SUBMIT_TIME, PropValue::Time(s.now));
        item.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(MSGFLAG_READ));
    } else {
        item.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(MSGFLAG_READ | MSGFLAG_UNSENT));
    }
    item
}

fn file_draft(s: &mut Session, kind: FolderKind, sent: bool) -> Result<(), EngineError> {
    let draft = s.compose.clone().ok_or_else(|| EngineError::Disabled("Send".into()))?;
    let store = s.writable_store().ok_or_else(|| EngineError::Format("there is no writable data file to keep the message in".into()))?;
    let folder = s.find_folder(store, kind).ok_or_else(|| EngineError::Format(format!("this data file has no {kind:?} folder")))?;
    let item = build(s, &draft, sent);
    s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.add(folder, item)?;
    s.compose = None;
    s.refresh_folders(store);
    s.reload_rows()
}

fn check(s: &mut Session, _: &Value) -> CmdResult {
    let draft = s.compose.as_ref().ok_or_else(|| EngineError::Disabled("Check Before Sending".into()))?;
    Ok(json!({"cautions": outgoing::cautions(draft).iter().map(|c| c.words()).collect::<Vec<_>>()}))
}

fn send(s: &mut Session, p: &Value) -> CmdResult {
    let has_recipient = s.compose.as_ref().is_some_and(|d| [&d.to, &d.cc, &d.bcc].iter().any(|l| !mailcraft_mime::parse_addresses(l).is_empty()));
    if !has_recipient {
        return Err(EngineError::Format("there must be at least one name or address in the To, Cc or Bcc box".into()));
    }
    // What the writer should know is said before the message goes, not after; it goes as it is
    // when that is asked for.
    let cautions = s.compose.as_ref().map(outgoing::cautions).unwrap_or_default();
    if !cautions.is_empty() && !p_bool(p, "anyway").unwrap_or(false) {
        let words: Vec<String> = cautions.iter().map(|c| c.words()).collect();
        return Err(EngineError::Format(format!("{} It was not sent; send it with \"anyway\": true to send it as it is.", words.join(" "))));
    }
    file_draft(s, FolderKind::Outbox, true)?;
    s.status = "The message is in the Outbox. Sending needs an account, which this version does not have yet.".to_string();
    ok()
}

fn save_draft(s: &mut Session, _: &Value) -> CmdResult {
    file_draft(s, FolderKind::Drafts, false)?;
    s.status = "Saved to Drafts".to_string();
    ok()
}

fn discard(s: &mut Session, _: &Value) -> CmdResult {
    if set_aside(s) {
        s.did(crate::undo::Step::Discarded);
        s.status = "Discarded. Undo (Ctrl+Z) brings it back.".to_string();
    }
    ok()
}

fn restore(s: &mut Session, _: &Value) -> CmdResult {
    let draft = s.set_aside.pop().ok_or_else(|| EngineError::Disabled("Restore Discarded Message".into()))?;
    // What is being written now makes way, and is kept in its turn.
    set_aside(s);
    s.compose = Some(draft);
    Ok(json!({ "left": s.set_aside.len() }))
}
