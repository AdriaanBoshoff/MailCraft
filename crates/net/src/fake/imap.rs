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

fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn serve(end: End, state: &Mutex<State>, tls: bool) -> Result<(), NetError> {
    let mut c = Conn::new(Box::new(end));
    let mut secure = tls;
    let mut signed_in = false;
    let mut selected: Option<String> = None;
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
            "LIST" => {
                let s = lock(state);
                for m in &s.mailboxes {
                    out.push_str(&format!("* LIST ({}) \"/\" {}\r\n", m.attributes.join(" "), quoted(&m.name)));
                }
                Ok("listed".into())
            }
            "SELECT" | "EXAMINE" => {
                let name = args.first().and_then(Tok::text).unwrap_or_default();
                let mut s = lock(state);
                match s.mailbox(&name) {
                    Some(m) => {
                        out.push_str(&format!(
                            "* {} EXISTS\r\n* OK [UIDVALIDITY {}] ids\r\n* OK [UIDNEXT {}] next\r\n* FLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)\r\n",
                            m.messages.len(),
                            m.uid_validity,
                            m.uid_next
                        ));
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
                let mut s = lock(state);
                let messages = selected.clone().and_then(|n| s.mailbox(&n).map(|m| m.messages.clone())).unwrap_or_default();
                let (lo, hi) = match range.split_once(':') {
                    Some((lo, "*")) => (lo.parse().unwrap_or(1), u32::MAX),
                    Some((lo, hi)) => (lo.parse().unwrap_or(1), hi.parse().unwrap_or(0)),
                    None => (range.parse().unwrap_or(0), range.parse().unwrap_or(0)),
                };
                let mut chosen: Vec<(usize, &super::Message)> = messages.iter().enumerate().filter(|(_, m)| m.uid >= lo && m.uid <= hi).collect();
                // `n:*` takes in the last message even when its id is below n.
                if chosen.is_empty()
                    && hi == u32::MAX
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
                if let Some(m) = selected.clone().and_then(|n| s.mailbox(&n)).and_then(|m| m.messages.iter_mut().find(|m| m.uid == id)) {
                    if how.starts_with('+') {
                        for f in flags {
                            if !m.flags.contains(&f) {
                                m.flags.push(f);
                            }
                        }
                    } else if how.starts_with('-') {
                        m.flags.retain(|f| !flags.contains(f));
                    }
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
                            if let Some(m) = s.mailbox(&from) {
                                if let Some(at) = m.messages.iter().position(|m| m.uid == id) {
                                    out.push_str(&format!("* {} EXPUNGE\r\n", at + 1));
                                }
                                m.messages.retain(|m| m.uid != id);
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
                    let mut at = 0;
                    let mut gone = Vec::new();
                    m.messages.retain(|msg| {
                        at += 1;
                        let go = msg.flags.iter().any(|f| f == "\\Deleted") && only.as_ref().is_none_or(|o| o.contains(&msg.uid));
                        if go {
                            gone.push(at);
                        }
                        !go
                    });
                    // Each number as it stands once those before it are gone.
                    for (n, at) in gone.iter().enumerate() {
                        out.push_str(&format!("* {} EXPUNGE\r\n", at - n));
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
                    s.mailboxes.push(super::Mailbox::new(&name, &[]));
                    Ok("created".into())
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
