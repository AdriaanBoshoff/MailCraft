//! Handing a message to a submission server: SMTP ([RFC 5321]) on the submission port
//! ([RFC 6409]), made secure with STARTTLS ([RFC 3207]) or TLS from the start ([RFC 8314]),
//! signed in with SASL PLAIN or LOGIN ([RFC 4954]).

use crate::conn::Conn;
use crate::{Connector, Endpoint, Login, MAX_MESSAGE, NetError, Security, one_line, said, sasl_plain};
use base64::Engine as _;

/// Lines of one reply read before it is called broken.
const MAX_REPLY_LINES: usize = 1_000;
/// Recipients of one message.
pub const MAX_RECIPIENTS: usize = 1_000;

/// A server's answer: its code and the text of its lines.
#[derive(Debug)]
pub struct Reply {
    pub code: u16,
    pub lines: Vec<String>,
}

impl Reply {
    fn text(&self) -> String {
        said(&format!("{} {}", self.code, self.lines.join(" ")))
    }
}

/// What a message being sent came to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Sent {
    /// Recipients the server took.
    pub accepted: Vec<String>,
    /// Recipients it refused, each with what it said.
    pub refused: Vec<(String, String)>,
}

pub struct Smtp {
    c: Conn,
    /// Extensions named in the answer to EHLO, upper case, with their parameters.
    ext: Vec<(String, String)>,
    secure: bool,
}

/// The name we give ourselves in EHLO: not this machine's name, which is nobody's business.
const HELO_NAME: &str = "[127.0.0.1]";

impl Smtp {
    /// Connect, make the connection secure, and sign in.
    pub fn open(with: &dyn Connector, to: &Endpoint, login: Option<&Login>) -> Result<Self, NetError> {
        let c = Conn::new(with.connect(to)?);
        let mut s = Self { c, ext: Vec::new(), secure: to.security == Security::Tls };
        let hello = s.reply()?;
        if hello.code != 220 {
            return Err(NetError::Refused(hello.text()));
        }
        s.ehlo()?;
        if to.security == Security::StartTls {
            if !s.has("STARTTLS") {
                return Err(NetError::NotSecure);
            }
            let r = s.command("STARTTLS")?;
            if r.code != 220 {
                return Err(NetError::Refused(r.text()));
            }
            let Self { c, .. } = s;
            s = Self { c: c.secure(with, &to.host)?, ext: Vec::new(), secure: true };
            // What was said before is forgotten ([RFC 3207] 4.2).
            s.ehlo()?;
        }
        if let Some(login) = login {
            s.sign_in(login)?;
        }
        Ok(s)
    }

    pub fn is_secure(&self) -> bool {
        self.secure
    }

    fn has(&self, name: &str) -> bool {
        self.ext.iter().any(|(n, _)| n == name)
    }

    fn param(&self, name: &str) -> Option<&str> {
        self.ext.iter().find(|(n, _)| n == name).map(|(_, p)| p.as_str())
    }

    fn reply(&mut self) -> Result<Reply, NetError> {
        let mut lines = Vec::new();
        loop {
            let line = self.c.line()?;
            let code = line.get(..3).and_then(|d| std::str::from_utf8(d).ok()).and_then(|d| d.parse::<u16>().ok()).filter(|c| (200..600).contains(c));
            let Some(code) = code else {
                return Err(NetError::protocol(format!("not a reply: {}", String::from_utf8_lossy(&line))));
            };
            let more = line.get(3) == Some(&b'-');
            lines.push(String::from_utf8_lossy(line.get(4..).unwrap_or_default()).into_owned());
            if lines.len() > MAX_REPLY_LINES {
                return Err(NetError::TooLarge("a reply from the server"));
            }
            if !more {
                return Ok(Reply { code, lines });
            }
        }
    }

    fn command(&mut self, line: &str) -> Result<Reply, NetError> {
        self.c.send(format!("{line}\r\n").as_bytes())?;
        self.reply()
    }

    fn ehlo(&mut self) -> Result<(), NetError> {
        let r = self.command(&format!("EHLO {HELO_NAME}"))?;
        if r.code != 250 {
            return Err(NetError::Refused(r.text()));
        }
        self.ext = r
            .lines
            .iter()
            .skip(1)
            .map(|l| {
                let (name, rest) = l.split_once(' ').unwrap_or((l.as_str(), ""));
                (name.to_ascii_uppercase(), rest.to_ascii_uppercase())
            })
            .collect();
        Ok(())
    }

    fn sign_in(&mut self, login: &Login) -> Result<(), NetError> {
        if !self.secure {
            // A password is never sent in the clear.
            return Err(NetError::NotSecure);
        }
        one_line("user name", &login.user)?;
        let methods: Vec<&str> = self.param("AUTH").unwrap_or_default().split_whitespace().collect();
        let r = if methods.contains(&"PLAIN") || methods.is_empty() {
            self.command(&format!("AUTH PLAIN {}", sasl_plain(login)))?
        } else if methods.contains(&"LOGIN") {
            let b64 = base64::engine::general_purpose::STANDARD;
            let r = self.command("AUTH LOGIN")?;
            if r.code != 334 {
                return Err(NetError::Auth(r.text()));
            }
            let r = self.command(&b64.encode(login.user.as_bytes()))?;
            if r.code != 334 {
                return Err(NetError::Auth(r.text()));
            }
            self.command(&b64.encode(login.password.expose().as_bytes()))?
        } else {
            return Err(NetError::Unsupported(format!(
                "the server signs in only with {}, which this version does not speak",
                said(&methods.join(", "))
            )));
        };
        match r.code {
            235 => Ok(()),
            _ => Err(NetError::Auth(r.text())),
        }
    }

    /// Send one message: `data` is the whole message as RFC 5322 text.
    pub fn send(&mut self, from: &str, to: &[String], data: &[u8]) -> Result<Sent, NetError> {
        if data.len() > MAX_MESSAGE {
            return Err(NetError::TooLarge("the message"));
        }
        if to.is_empty() {
            return Err(NetError::Unsupported("the message has no recipients".into()));
        }
        if to.len() > MAX_RECIPIENTS {
            return Err(NetError::TooLarge("the list of recipients"));
        }
        path("sender", from)?;
        for r in to {
            path("recipient", r)?;
        }
        if let Some(max) = self.param("SIZE").and_then(|p| p.trim().parse::<usize>().ok()).filter(|m| *m > 0)
            && data.len() > max
        {
            return Err(NetError::Refused(format!("the message is {} bytes and the server takes no more than {max}", data.len())));
        }
        let mut mail = format!("MAIL FROM:<{from}>");
        if self.has("SIZE") {
            mail.push_str(&format!(" SIZE={}", data.len()));
        }
        if !data.is_ascii() {
            if self.has("8BITMIME") {
                mail.push_str(" BODY=8BITMIME");
            }
            if self.has("SMTPUTF8") && !(from.is_ascii() && to.iter().all(|t| t.is_ascii())) {
                mail.push_str(" SMTPUTF8");
            }
        }
        let r = self.command(&mail)?;
        if r.code != 250 {
            return Err(NetError::Refused(r.text()));
        }
        let mut sent = Sent::default();
        for rcpt in to {
            let r = self.command(&format!("RCPT TO:<{rcpt}>"))?;
            if matches!(r.code, 250 | 251) {
                sent.accepted.push(rcpt.clone());
            } else {
                sent.refused.push((rcpt.clone(), r.text()));
            }
        }
        if sent.accepted.is_empty() {
            let _ = self.command("RSET");
            return Err(NetError::Refused(format!("no recipient was taken: {}", sent.refused.first().map(|(_, w)| w.as_str()).unwrap_or_default())));
        }
        let r = self.command("DATA")?;
        if r.code != 354 {
            return Err(NetError::Refused(r.text()));
        }
        self.c.send(&stuffed(data))?;
        let r = self.reply()?;
        if r.code != 250 {
            return Err(NetError::Refused(r.text()));
        }
        Ok(sent)
    }

    pub fn quit(mut self) -> Result<(), NetError> {
        let _ = self.command("QUIT")?;
        Ok(())
    }
}

/// An address put between `<` and `>`: nothing that ends the command or the path early.
fn path(what: &'static str, address: &str) -> Result<(), NetError> {
    let bad = address.is_empty() || address.len() > 320 || address.chars().any(|c| c.is_control() || c.is_whitespace() || matches!(c, '<' | '>'));
    if bad || !address.contains('@') {
        return Err(NetError::Unsupported(format!("the {what} {} is not an address mail can be sent to", said(address))));
    }
    Ok(())
}

/// The message as DATA carries it ([RFC 5321] 4.5.2): every line ends in CRLF, a line that
/// begins with a dot has another put before it, and a line with one dot ends it.
pub fn stuffed(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 50 + 8);
    let mut start = true;
    let mut i = 0;
    while let Some(&b) = data.get(i) {
        if start && b == b'.' {
            out.push(b'.');
        }
        match b {
            b'\r' if data.get(i + 1) == Some(&b'\n') => {
                out.extend_from_slice(b"\r\n");
                i += 1;
                start = true;
            }
            b'\r' | b'\n' => {
                out.extend_from_slice(b"\r\n");
                start = true;
            }
            _ => {
                out.push(b);
                start = false;
            }
        }
        i += 1;
    }
    if !start {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b".\r\n");
    out
}
