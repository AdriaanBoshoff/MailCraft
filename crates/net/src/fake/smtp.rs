//! The fake SMTP submission server.

use super::{State, Submitted, lock};
use crate::conn::Conn;
use crate::{End, NetError};
use base64::Engine as _;
use std::sync::Mutex;

pub fn serve(end: End, state: &Mutex<State>, tls: bool) -> Result<(), NetError> {
    let mut c = Conn::new(Box::new(end));
    let mut secure = tls;
    let mut signed_in = false;
    let mut from: Option<String> = None;
    let mut to: Vec<String> = Vec::new();
    c.send(b"220 mail.example.test ESMTP fake\r\n")?;
    loop {
        let line = String::from_utf8_lossy(&c.line()?).into_owned();
        lock(state).heard.push(line.clone());
        let upper = line.to_ascii_uppercase();
        let reply: String = if upper.starts_with("EHLO ") {
            let ext: Vec<String> = lock(state).smtp_ext.iter().filter(|e| !(secure && e.as_str() == "STARTTLS")).cloned().collect();
            let mut r = String::from("250-mail.example.test\r\n");
            for (i, e) in ext.iter().enumerate() {
                r.push_str(&format!("250{}{e}\r\n", if i + 1 == ext.len() { ' ' } else { '-' }));
            }
            if ext.is_empty() {
                r = "250 mail.example.test\r\n".into();
            }
            c.send(r.as_bytes())?;
            continue;
        } else if upper == "STARTTLS" && !secure {
            c.send(b"220 go ahead\r\n")?;
            secure = true;
            lock(state).secured += 1;
            continue;
        } else if let Some(b64) = upper.strip_prefix("AUTH PLAIN ").map(|_| line.get(11..).unwrap_or_default().to_string()) {
            let raw = base64::engine::general_purpose::STANDARD.decode(b64.trim()).unwrap_or_default();
            let parts: Vec<&[u8]> = raw.split(|b| *b == 0).collect();
            let s = lock(state);
            if secure && parts.len() == 3 && parts[1] == s.user.as_bytes() && parts[2] == s.password.as_bytes() {
                drop(s);
                signed_in = true;
                lock(state).logins += 1;
                "235 signed in".into()
            } else {
                "535 not accepted".into()
            }
        } else if upper.starts_with("MAIL FROM:<") {
            if !signed_in {
                "530 sign in first".into()
            } else {
                from = line.get(11..).and_then(|r| r.split('>').next()).map(str::to_string);
                to.clear();
                "250 ok".into()
            }
        } else if upper.starts_with("RCPT TO:<") {
            let rcpt = line.get(9..).and_then(|r| r.split('>').next()).unwrap_or_default().to_string();
            if from.is_none() {
                "503 sender first".into()
            } else if rcpt.ends_with("@refused.test") {
                "550 no such user".into()
            } else {
                to.push(rcpt);
                "250 ok".into()
            }
        } else if upper == "DATA" {
            if to.is_empty() {
                "503 recipients first".into()
            } else {
                c.send(b"354 go on\r\n")?;
                let mut data = Vec::new();
                loop {
                    let l = c.line()?;
                    if l == b"." {
                        break;
                    }
                    data.extend_from_slice(if l.first() == Some(&b'.') { l.get(1..).unwrap_or_default() } else { &l });
                    data.extend_from_slice(b"\r\n");
                }
                lock(state).submitted.push(Submitted { from: from.take().unwrap_or_default(), to: std::mem::take(&mut to), data });
                "250 queued".into()
            }
        } else if upper == "RSET" {
            from = None;
            to.clear();
            "250 ok".into()
        } else if upper == "QUIT" {
            c.send(b"221 bye\r\n")?;
            return Ok(());
        } else {
            "500 not known".into()
        };
        c.send(format!("{reply}\r\n").as_bytes())?;
    }
}
