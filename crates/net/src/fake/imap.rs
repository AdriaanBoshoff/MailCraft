//! The fake IMAP server.

use super::{State, lock};
use crate::conn::Conn;
use crate::imap::{Tok, read_raw, tokens};
use crate::{End, NetError};
use base64::Engine as _;
use std::sync::Mutex;

fn caps(state: &Mutex<State>, secure: bool, signed_in: bool) -> String {
    let s = lock(state);
    let mut caps = vec!["IMAP4rev1".to_string()];
    if !secure {
        caps.push("STARTTLS".into());
    }
    if !signed_in {
        caps.push("AUTH=PLAIN".into());
    }
    caps.extend(s.imap_caps.iter().cloned());
    caps.retain(|c| !s.imap_without.contains(c));
    caps.join(" ")
}

/// A set of ids as a client sends it (`1,4:7,9:*`), `*` as the highest there is.
fn set_of(text: &str) -> Vec<(u32, u32)> {
    let n = |t: &str| if t == "*" { u32::MAX } else { t.parse().unwrap_or(0) };
    text.split(',')
        .map(|part| match part.split_once(':') {
            Some((a, b)) => (n(a).min(n(b)), n(a).max(n(b))),
            None => (n(part), n(part)),
        })
        .collect()
}

fn in_set(set: &[(u32, u32)], uid: u32) -> bool {
    set.iter().any(|(lo, hi)| (*lo..=*hi).contains(&uid))
}

/// Ids as a server sends them, runs joined: `3,5:9`.
fn runs(mut uids: Vec<u32>) -> String {
    uids.sort_unstable();
    let mut out: Vec<String> = Vec::new();
    let mut at = 0;
    while let Some(&lo) = uids.get(at) {
        let mut hi = lo;
        while uids.get(at + 1) == Some(&(hi + 1)) {
            hi += 1;
            at += 1;
        }
        out.push(if lo == hi { lo.to_string() } else { format!("{lo}:{hi}") });
        at += 1;
    }
    out.join(",")
}

/// Waiting in IDLE: what changes in the folder meanwhile is told as it comes, until DONE.
fn idle(c: &mut Conn, state: &Mutex<State>, selected: Option<&str>, qresync: bool) -> Result<(), NetError> {
    lock(state).idling += 1;
    let look = |state: &Mutex<State>| {
        let mut s = lock(state);
        selected.and_then(|n| s.mailbox(n)).map(|m| (m.modseq, m.messages.len(), m.clone())).unwrap_or_else(|| (0, 0, super::Mailbox::new("", &[])))
    };
    let (mut at, mut count, _) = look(state);
    let was = c.set_wait(std::time::Duration::from_millis(20))?;
    loop {
        match c.line() {
            Ok(line) => {
                lock(state).heard.push(String::from_utf8_lossy(&line).into_owned());
                if line.eq_ignore_ascii_case(b"DONE") {
                    break;
                }
                c.send(b"* BAD only DONE ends IDLE\r\n")?;
            }
            Err(NetError::TimedOut) => {}
            Err(e) => return Err(e),
        }
        let (now, n, m) = look(state);
        if now == at {
            continue;
        }
        let mut out = String::new();
        for (uid, _) in m.vanished.iter().filter(|(_, when)| *when > at) {
            out.push_str(&if qresync { format!("* VANISHED {uid}\r\n") } else { "* 1 EXPUNGE\r\n".to_string() });
        }
        if n != count || !out.is_empty() {
            out.push_str(&format!("* {n} EXISTS\r\n"));
        }
        for (i, msg) in m.messages.iter().enumerate().filter(|(_, msg)| msg.modseq > at) {
            out.push_str(&format!("* {} FETCH (UID {} FLAGS ({}))\r\n", i + 1, msg.uid, msg.flags.join(" ")));
        }
        (at, count) = (now, n);
        c.send(out.as_bytes())?;
    }
    if let Some(was) = was {
        c.set_wait(was)?;
    }
    Ok(())
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn serve(end: End, state: &Mutex<State>, tls: bool) -> Result<(), NetError> {
    let mut c = Conn::new(Box::new(end));
    let mut secure = tls;
    let mut signed_in = false;
    let mut selected: Option<String> = None;
    // QRESYNC turned on by ENABLE ([RFC 7162] 3.2.3): expunges are told as VANISHED.
    let mut qresync = false;
    c.send(format!("* OK [CAPABILITY {}] fake IMAP\r\n", caps(state, secure, false)).as_bytes())?;
    loop {
        let raw = read_raw(&mut c, Some(b"+ go on\r\n"))?;
        lock(state).heard.push(String::from_utf8_lossy(&raw).into_owned());
        let toks = tokens(&raw).unwrap_or_default();
        let tag = toks.first().and_then(Tok::text).unwrap_or_else(|| "*".into());
        let mut verb = toks.get(1).and_then(Tok::atom).unwrap_or_default().to_ascii_uppercase();
        let mut args: &[Tok] = toks.get(2..).unwrap_or_default();
        let uid = verb == "UID";
        if uid {
            verb = args.first().and_then(Tok::atom).unwrap_or_default().to_ascii_uppercase();
            args = args.get(1..).unwrap_or_default();
        }
        let mut out = String::new();
        let done: Result<String, String> = match verb.as_str() {
            "CAPABILITY" => {
                out.push_str(&format!("* CAPABILITY {}\r\n", caps(state, secure, signed_in)));
                Ok("done".into())
            }
            "STARTTLS" if !secure => {
                c.send(format!("{tag} OK begin TLS\r\n").as_bytes())?;
                secure = true;
                lock(state).secured += 1;
                continue;
            }
            "LOGIN" => {
                let (user, pass) = (args.first().and_then(Tok::text), args.get(1).and_then(Tok::text));
                let s = lock(state);
                if s.imap_without.iter().any(|c| c == "LOGIN") {
                    Err("NO LOGIN is turned off".into())
                } else if secure && user.as_deref() == Some(s.user.as_str()) && pass.as_deref() == Some(s.password.as_str()) {
                    drop(s);
                    signed_in = true;
                    lock(state).logins += 1;
                    Ok("signed in".into())
                } else {
                    Err("NO [AUTHENTICATIONFAILED] not accepted".into())
                }
            }
            "AUTHENTICATE" => {
                let b64 = match args.get(1).and_then(Tok::atom) {
                    Some(initial) => initial.to_string(),
                    None => {
                        c.send(b"+ \r\n")?;
                        String::from_utf8_lossy(&c.line()?).into_owned()
                    }
                };
                let raw = base64::engine::general_purpose::STANDARD.decode(b64.trim()).unwrap_or_default();
                let parts: Vec<&[u8]> = raw.split(|b| *b == 0).collect();
                let s = lock(state);
                if secure && parts.len() == 3 && parts[1] == s.user.as_bytes() && parts[2] == s.password.as_bytes() {
                    drop(s);
                    signed_in = true;
                    lock(state).logins += 1;
                    Ok("signed in".into())
                } else {
                    Err("NO [AUTHENTICATIONFAILED] not accepted".into())
                }
            }
            "LOGOUT" => {
                c.send(format!("* BYE see you\r\n{tag} OK logged out\r\n").as_bytes())?;
                return Ok(());
            }
            _ if !signed_in => Err("NO sign in first".into()),
            "ENABLE" => {
                let has_qresync = lock(state).imap_caps.iter().any(|c| c == "QRESYNC");
                if has_qresync && args.iter().any(|a| a.atom().is_some_and(|a| a.eq_ignore_ascii_case("QRESYNC"))) {
                    qresync = true;
                    out.push_str("* ENABLED QRESYNC\r\n");
                } else {
                    out.push_str("* ENABLED\r\n");
                }
                Ok("enabled".into())
            }
            "IDLE" => {
                c.send(b"+ idling\r\n")?;
                idle(&mut c, state, selected.as_deref(), qresync)?;
                Ok("idle done".into())
            }
            "LIST" => {
                let s = lock(state);
                for m in &s.mailboxes {
                    out.push_str(&format!("* LIST ({}) \"/\" {}\r\n", m.attributes.join(" "), quoted(&m.name)));
                }
                Ok("listed".into())
            }
            "SELECT" | "EXAMINE" => {
                let name = args.first().and_then(Tok::text).unwrap_or_default();
                // `(QRESYNC (validity modseq))` after the name: what changed since, told now.
                let since = match args.get(1) {
                    Some(Tok::List(p)) if p.first().and_then(Tok::atom).is_some_and(|a| a.eq_ignore_ascii_case("QRESYNC")) => match p.get(1) {
                        Some(Tok::List(q)) => q.first().and_then(Tok::number).zip(q.get(1).and_then(Tok::number)),
                        _ => None,
                    },
                    _ => None,
                };
                let mut s = lock(state);
                let modseqs = s.imap_caps.iter().any(|c| c == "CONDSTORE" || c == "QRESYNC");
                let objectid = s.imap_caps.iter().any(|c| c == "OBJECTID");
                match s.mailbox(&name) {
                    Some(m) => {
                        out.push_str(&format!(
                            "* {} EXISTS\r\n* OK [UIDVALIDITY {}] ids\r\n* OK [UIDNEXT {}] next\r\n* FLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)\r\n",
                            m.messages.len(),
                            m.uid_validity,
                            m.uid_next
                        ));
                        if modseqs {
                            out.push_str(&format!("* OK [HIGHESTMODSEQ {}] changes\r\n", m.modseq));
                        }
                        if objectid {
                            out.push_str(&format!("* OK [MAILBOXID ({})] id\r\n", m.id));
                        }
                        if let Some((_, modseq)) = since.filter(|(v, _)| qresync && *v == u64::from(m.uid_validity)) {
                            let gone: Vec<u32> = m.vanished.iter().filter(|(_, at)| *at > modseq).map(|(uid, _)| *uid).collect();
                            if !gone.is_empty() {
                                out.push_str(&format!("* VANISHED (EARLIER) {}\r\n", runs(gone)));
                            }
                            for (i, msg) in m.messages.iter().enumerate().filter(|(_, msg)| msg.modseq > modseq) {
                                out.push_str(&format!(
                                    "* {} FETCH (UID {} FLAGS ({}) MODSEQ ({}))\r\n",
                                    i + 1,
                                    msg.uid,
                                    msg.flags.join(" "),
                                    msg.modseq
                                ));
                            }
                        }
                        selected = Some(m.name.clone());
                        Ok(if verb == "EXAMINE" { "[READ-ONLY] selected".into() } else { "[READ-WRITE] selected".into() })
                    }
                    None => Err("NO no such folder".into()),
                }
            }
            "FETCH" if uid => {
                let range = args.first().and_then(Tok::atom).unwrap_or_default().to_string();
                let want_body = args.get(1).is_some_and(|t| matches!(t, Tok::List(items) if items.iter().any(|i| i.atom() == Some("BODY.PEEK[]"))));
                let want_id = args.get(1).is_some_and(
                    |t| matches!(t, Tok::List(items) if items.iter().any(|i| i.atom().is_some_and(|a| a.starts_with("BODY.PEEK[HEADER.FIELDS")))),
                );
                // `(CHANGEDSINCE n)` after the items ([RFC 7162] 3.1.4).
                let changed_since = match args.get(2) {
                    Some(Tok::List(p)) if p.first().and_then(Tok::atom).is_some_and(|a| a.eq_ignore_ascii_case("CHANGEDSINCE")) => {
                        p.get(1).and_then(Tok::number)
                    }
                    _ => None,
                };
                let mut s = lock(state);
                let messages = selected.clone().and_then(|n| s.mailbox(&n).map(|m| m.messages.clone())).unwrap_or_default();
                let set = set_of(&range);
                let mut chosen: Vec<(usize, &super::Message)> =
                    messages.iter().enumerate().filter(|(_, m)| in_set(&set, m.uid) && changed_since.is_none_or(|n| m.modseq > n)).collect();
                // `n:*` takes in the last message even when its id is below n.
                if chosen.is_empty()
                    && changed_since.is_none()
                    && set.len() == 1
                    && set.first().is_some_and(|(_, hi)| *hi == u32::MAX)
                    && let Some(last) = messages.len().checked_sub(1).and_then(|i| messages.get(i).map(|m| (i, m)))
                {
                    chosen.push(last);
                }
                let mut bytes = out.into_bytes();
                for (i, m) in chosen {
                    if want_id {
                        let text = String::from_utf8_lossy(&m.data);
                        let line = text
                            .split("\r\n")
                            .take_while(|l| !l.is_empty())
                            .find(|l| l.to_ascii_lowercase().starts_with("message-id:"))
                            .map(|l| format!("{l}\r\n\r\n"))
                            .unwrap_or_else(|| "\r\n".into());
                        bytes.extend_from_slice(
                            format!("* {} FETCH (UID {} BODY[HEADER.FIELDS (MESSAGE-ID)] {{{}}}\r\n", i + 1, m.uid, line.len()).as_bytes(),
                        );
                        bytes.extend_from_slice(line.as_bytes());
                        bytes.extend_from_slice(b")\r\n");
                    } else if want_body {
                        bytes.extend_from_slice(format!("* {} FETCH (UID {} BODY[] {{{}}}\r\n", i + 1, m.uid, m.data.len()).as_bytes());
                        bytes.extend_from_slice(&m.data);
                        bytes.extend_from_slice(b")\r\n");
                    } else if changed_since.is_some() {
                        bytes.extend_from_slice(
                            format!("* {} FETCH (UID {} FLAGS ({}) MODSEQ ({}))\r\n", i + 1, m.uid, m.flags.join(" "), m.modseq).as_bytes(),
                        );
                    } else {
                        bytes.extend_from_slice(
                            format!("* {} FETCH (UID {} FLAGS ({}) RFC822.SIZE {})\r\n", i + 1, m.uid, m.flags.join(" "), m.data.len()).as_bytes(),
                        );
                    }
                }
                bytes.extend_from_slice(format!("{tag} OK fetched\r\n").as_bytes());
                c.send(&bytes)?;
                continue;
            }
            "STORE" if uid => {
                let id: u32 = args.first().and_then(Tok::number).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
                let how = args.get(1).and_then(Tok::atom).unwrap_or_default().to_ascii_uppercase();
                let flags: Vec<String> = match args.get(2) {
                    Some(Tok::List(f)) => f.iter().filter_map(Tok::atom).map(str::to_string).collect(),
                    _ => Vec::new(),
                };
                let mut s = lock(state);
                if let Some(mailbox) = selected.clone().and_then(|n| s.mailbox(&n))
                    && let Some(m) = mailbox.messages.iter().find(|m| m.uid == id)
                {
                    let mut now = m.flags.clone();
                    if how.starts_with('+') {
                        for f in flags {
                            if !now.contains(&f) {
                                now.push(f);
                            }
                        }
                    } else if how.starts_with('-') {
                        now.retain(|f| !flags.contains(f));
                    }
                    let now: Vec<&str> = now.iter().map(String::as_str).collect();
                    mailbox.set_flags(id, &now);
                }
                Ok("stored".into())
            }
            "MOVE" | "COPY" if uid => {
                let id: u32 = args.first().and_then(Tok::number).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
                let to = args.get(1).and_then(Tok::text).unwrap_or_default();
                let mut s = lock(state);
                let from = selected.clone().unwrap_or_default();
                let message = s.mailbox(&from).and_then(|m| m.messages.iter().find(|m| m.uid == id).cloned());
                match (message, s.mailbox(&to).is_some()) {
                    (Some(message), true) => {
                        let f: Vec<&str> = message.flags.iter().map(String::as_str).filter(|f| *f != "\\Deleted").collect();
                        let (validity, new) = match s.mailbox(&to) {
                            Some(m) => (m.uid_validity, m.add(&message.data, &f)),
                            None => (0, 0),
                        };
                        let code = format!("[COPYUID {validity} {id} {new}]");
                        if verb == "MOVE" {
                            if let Some(at) = s.mailbox(&from).and_then(|m| m.remove(id)) {
                                out.push_str(&if qresync { format!("* VANISHED {id}\r\n") } else { format!("* {at} EXPUNGE\r\n") });
                            }
                            out.insert_str(0, &format!("* OK {code} moved\r\n"));
                            Ok("moved".into())
                        } else {
                            Ok(format!("{code} copied"))
                        }
                    }
                    (None, _) => Ok("nothing by that id".into()),
                    (_, false) => Err("NO [TRYCREATE] no such folder".into()),
                }
            }
            "EXPUNGE" => {
                let only: Option<Vec<u32>> =
                    uid.then(|| args.first().and_then(Tok::atom).unwrap_or_default().split(',').filter_map(|n| n.parse().ok()).collect());
                let mut s = lock(state);
                if let Some(m) = selected.clone().and_then(|n| s.mailbox(&n)) {
                    let going: Vec<u32> = m
                        .messages
                        .iter()
                        .filter(|msg| msg.flags.iter().any(|f| f == "\\Deleted") && only.as_ref().is_none_or(|o| o.contains(&msg.uid)))
                        .map(|msg| msg.uid)
                        .collect();
                    // Each number as it stands once those before it are gone.
                    for uid in going {
                        if let Some(at) = m.remove(uid) {
                            out.push_str(&if qresync { format!("* VANISHED {uid}\r\n") } else { format!("* {at} EXPUNGE\r\n") });
                        }
                    }
                }
                Ok("expunged".into())
            }
            "CREATE" => {
                let name = args.first().and_then(Tok::text).unwrap_or_default();
                let mut s = lock(state);
                if name.is_empty() || s.mailbox(&name).is_some() {
                    Err("NO it is there already".into())
                } else {
                    let mut m = super::Mailbox::new(&name, &[]);
                    m.uid_validity = s.next_validity;
                    m.id = format!("M{}", s.next_validity);
                    s.next_validity = s.next_validity.saturating_add(1);
                    s.mailboxes.push(m);
                    Ok("created".into())
                }
            }
            "RENAME" => {
                let (from, to) = (args.first().and_then(Tok::text).unwrap_or_default(), args.get(1).and_then(Tok::text).unwrap_or_default());
                let mut s = lock(state);
                let inside = format!("{from}/");
                if from.eq_ignore_ascii_case("INBOX") || to.is_empty() || s.mailbox(&from).is_none() {
                    Err("NO no such folder".into())
                } else if s.mailbox(&to).is_some() || to.starts_with(&inside) {
                    Err("NO [ALREADYEXISTS] there is one by that name".into())
                } else {
                    // The folders inside it go with it ([RFC 9051] 6.3.6).
                    for m in s.mailboxes.iter_mut() {
                        if m.name.eq_ignore_ascii_case(&from) {
                            m.name = to.clone();
                        } else if let Some(rest) = m.name.strip_prefix(&inside) {
                            m.name = format!("{to}/{rest}");
                        }
                    }
                    Ok("renamed".into())
                }
            }
            "DELETE" => {
                let name = args.first().and_then(Tok::text).unwrap_or_default();
                let mut s = lock(state);
                if name.eq_ignore_ascii_case("INBOX") || s.mailbox(&name).is_none() {
                    Err("NO no such folder".into())
                } else {
                    s.mailboxes.retain(|m| !m.name.eq_ignore_ascii_case(&name));
                    if selected.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(&name)) {
                        selected = None;
                    }
                    Ok("deleted".into())
                }
            }
            "STATUS" => {
                let name = args.first().and_then(Tok::text).unwrap_or_default();
                let mut s = lock(state);
                let objectid = s.imap_caps.iter().any(|c| c == "OBJECTID");
                match s.mailbox(&name) {
                    Some(m) => {
                        let id = if objectid { format!(" MAILBOXID ({})", m.id) } else { String::new() };
                        out.push_str(&format!(
                            "* STATUS {} (MESSAGES {} UIDNEXT {} UIDVALIDITY {}{id})\r\n",
                            quoted(&m.name),
                            m.messages.len(),
                            m.uid_next,
                            m.uid_validity
                        ));
                        Ok("status".into())
                    }
                    None => Err("NO no such folder".into()),
                }
            }
            "APPEND" => {
                let name = args.first().and_then(Tok::text).unwrap_or_default();
                let flags: Vec<String> = match args.get(1) {
                    Some(Tok::List(f)) => f.iter().filter_map(Tok::atom).map(str::to_string).collect(),
                    _ => Vec::new(),
                };
                let data = match args.last() {
                    Some(Tok::Str(d)) => d.clone(),
                    _ => Vec::new(),
                };
                let mut s = lock(state);
                match s.mailbox(&name) {
                    Some(m) => {
                        let f: Vec<&str> = flags.iter().map(String::as_str).collect();
                        let id = m.add(&data, &f);
                        Ok(format!("[APPENDUID {} {id}] appended", m.uid_validity))
                    }
                    None => Err("NO [TRYCREATE] no such folder".into()),
                }
            }
            _ => Err("BAD not known".into()),
        };
        match done {
            Ok(text) => out.push_str(&format!("{tag} OK {text}\r\n")),
            Err(text) => out.push_str(&format!("{tag} {text}\r\n")),
        }
        c.send(out.as_bytes())?;
    }
}
