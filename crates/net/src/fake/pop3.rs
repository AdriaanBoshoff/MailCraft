//! The fake POP3 server: its maildrop is the Inbox.

use super::{State, lock};
use crate::conn::Conn;
use crate::{End, NetError};
use base64::Engine as _;
use std::sync::Mutex;

pub fn serve(end: End, state: &Mutex<State>, tls: bool) -> Result<(), NetError> {
    let mut c = Conn::new(Box::new(end));
    let mut secure = tls;
    let mut user: Option<String> = None;
    let mut signed_in = false;
    let mut deleted: Vec<u32> = Vec::new();
    c.send(b"+OK fake POP3\r\n")?;
    loop {
        let line = String::from_utf8_lossy(&c.line()?).into_owned();
        lock(state).heard.push(line.clone());
        let mut words = line.splitn(2, ' ');
        let verb = words.next().unwrap_or_default().to_ascii_uppercase();
        let arg = words.next().unwrap_or_default().to_string();
        // The maildrop as numbered this session: uid of the message, size, bytes.
        let drop_list: Vec<(u32, Vec<u8>)> = {
            let mut s = lock(state);
            s.mailbox("INBOX").map(|m| m.messages.iter().map(|m| (m.uid, m.data.clone())).collect()).unwrap_or_default()
        };
        let number = |arg: &str| arg.trim().parse::<usize>().ok().filter(|n| *n >= 1 && *n <= drop_list.len() && !deleted.contains(&(*n as u32)));
        let reply = match verb.as_str() {
            "CAPA" => {
                let caps = lock(state).pop_caps.clone();
                let caps: Vec<String> = caps.into_iter().filter(|c| !(secure && c == "STLS")).collect();
                format!("+OK\r\n{}\r\n.", caps.join("\r\n"))
            }
            "STLS" if !secure => {
                secure = true;
                lock(state).secured += 1;
                "+OK go ahead".into()
            }
            "USER" => {
                user = Some(arg);
                "+OK".into()
            }
            "PASS" => {
                let s = lock(state);
                if secure && user.as_deref() == Some(s.user.as_str()) && arg == s.password {
                    drop(s);
                    signed_in = true;
                    lock(state).logins += 1;
                    "+OK signed in".into()
                } else {
                    "-ERR not accepted".into()
                }
            }
            "AUTH" => {
                let b64 = arg.strip_prefix("PLAIN ").unwrap_or_default();
                let raw = base64::engine::general_purpose::STANDARD.decode(b64.trim()).unwrap_or_default();
                let parts: Vec<&[u8]> = raw.split(|b| *b == 0).collect();
                let s = lock(state);
                if secure && parts.len() == 3 && parts[1] == s.user.as_bytes() && parts[2] == s.password.as_bytes() {
                    drop(s);
                    signed_in = true;
                    lock(state).logins += 1;
                    "+OK signed in".into()
                } else {
                    "-ERR not accepted".into()
                }
            }
            _ if !signed_in && verb != "QUIT" => "-ERR sign in first".into(),
            "UIDL" => {
                let mut r = String::from("+OK");
                for (i, (uid, _)) in drop_list.iter().enumerate() {
                    if !deleted.contains(&(i as u32 + 1)) {
                        r.push_str(&format!("\r\n{} uid-{uid}", i + 1));
                    }
                }
                r + "\r\n."
            }
            "LIST" => {
                let mut r = String::from("+OK");
                for (i, (_, data)) in drop_list.iter().enumerate() {
                    if !deleted.contains(&(i as u32 + 1)) {
                        r.push_str(&format!("\r\n{} {}", i + 1, data.len()));
                    }
                }
                r + "\r\n."
            }
            "RETR" => match number(&arg).and_then(|n| drop_list.get(n - 1)) {
                Some((_, data)) => {
                    let mut out = b"+OK\r\n".to_vec();
                    out.extend_from_slice(&crate::smtp::stuffed(data));
                    c.send(&out)?;
                    continue;
                }
                None => "-ERR no such message".into(),
            },
            "DELE" => match number(&arg) {
                Some(n) => {
                    deleted.push(n as u32);
                    "+OK".into()
                }
                None => "-ERR no such message".into(),
            },
            "QUIT" => {
                let uids: Vec<u32> = deleted.iter().filter_map(|n| drop_list.get(*n as usize - 1).map(|(u, _)| *u)).collect();
                if let Some(inbox) = lock(state).mailbox("INBOX") {
                    inbox.messages.retain(|m| !uids.contains(&m.uid));
                }
                c.send(b"+OK bye\r\n")?;
                return Ok(());
            }
            _ => "-ERR not known".into(),
        };
        c.send(format!("{reply}\r\n").as_bytes())?;
    }
}
