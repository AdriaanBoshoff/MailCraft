//! Taking mail from a POP3 server ([RFC 1939]), made secure with STLS ([RFC 2595] 4) or TLS
//! from the start, signed in with USER and PASS or SASL PLAIN ([RFC 5034]); CAPA ([RFC 2449]).

use crate::conn::Conn;
use crate::{Connector, Endpoint, Login, MAX_MESSAGE, NetError, Security, one_line, said, sasl_plain};

/// Lines of a multi-line listing read before it is called broken.
const MAX_LIST: usize = 1_000_000;
/// The longest unique id ([RFC 1939] 7: 1 to 70 characters).
const MAX_UID: usize = 70;

pub struct Pop3 {
    c: Conn,
    caps: Vec<String>,
}

/// A message in the maildrop: its number this session, its unique id and its size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub number: u32,
    pub uid: String,
    pub size: u64,
}

impl Pop3 {
    pub fn open(with: &dyn Connector, to: &Endpoint, login: &Login) -> Result<Self, NetError> {
        let mut p = Self::reach(with, to)?;
        p.sign_in(login)?;
        Ok(p)
    }

    /// Whether the server greets us as a POP3 server and the connection is made secure;
    /// nothing is signed in to, and the connection is left with QUIT.
    pub fn greets(with: &dyn Connector, to: &Endpoint) -> Result<(), NetError> {
        Self::reach(with, to)?.quit()
    }

    /// The greeting, and the connection made secure.
    fn reach(with: &dyn Connector, to: &Endpoint) -> Result<Self, NetError> {
        let mut p = Self { c: Conn::new(with.connect(to)?), caps: Vec::new() };
        p.status()?;
        p.caps = p.capabilities().unwrap_or_default();
        let mut secure = to.security == Security::Tls;
        if to.security == Security::StartTls {
            if !p.caps.iter().any(|c| c == "STLS") {
                return Err(NetError::NotSecure);
            }
            p.command("STLS")?;
            let Self { c, .. } = p;
            p = Self { c: c.secure(with, to)?, caps: Vec::new() };
            p.caps = p.capabilities().unwrap_or_default();
            secure = true;
        }
        if !secure {
            return Err(NetError::NotSecure);
        }
        Ok(p)
    }

    /// One `+OK` or `-ERR` line; `-ERR` is an error carrying what the server said.
    fn status(&mut self) -> Result<String, NetError> {
        let line = String::from_utf8_lossy(&self.c.line()?).into_owned();
        if let Some(rest) = line.strip_prefix("+OK") {
            Ok(rest.trim().to_string())
        } else if let Some(rest) = line.strip_prefix("-ERR") {
            Err(NetError::Refused(said(rest.trim())))
        } else {
            Err(NetError::protocol(format!("not a status: {line}")))
        }
    }

    fn command(&mut self, line: &str) -> Result<String, NetError> {
        self.c.send(format!("{line}\r\n").as_bytes())?;
        self.status()
    }

    /// The lines of a multi-line answer, dots taken off ([RFC 1939] 3), no more than `max`
    /// bytes in all.
    fn lines(&mut self, max: usize) -> Result<Vec<Vec<u8>>, NetError> {
        let (mut out, mut total) = (Vec::new(), 0usize);
        loop {
            let line = self.c.line()?;
            if line == b"." {
                return Ok(out);
            }
            let line = if line.first() == Some(&b'.') { line.get(1..).unwrap_or_default().to_vec() } else { line };
            total = total.saturating_add(line.len() + 2);
            if total > max || out.len() >= MAX_LIST {
                return Err(NetError::TooLarge("an answer from the server"));
            }
            out.push(line);
        }
    }

    fn capabilities(&mut self) -> Result<Vec<String>, NetError> {
        self.command("CAPA")?;
        Ok(self.lines(64 * 1024)?.iter().map(|l| String::from_utf8_lossy(l).trim().to_ascii_uppercase()).collect())
    }

    fn sign_in(&mut self, login: &Login) -> Result<(), NetError> {
        one_line("user name", &login.user)?;
        one_line("password", login.password.expose())?;
        let sasl = self.caps.iter().any(|c| c.starts_with("SASL ") && c.split_whitespace().any(|m| m == "PLAIN"));
        let done = if sasl {
            self.command(&format!("AUTH PLAIN {}", sasl_plain(login)))
        } else {
            self.command(&format!("USER {}", login.user)).and_then(|_| self.command(&format!("PASS {}", login.password.expose())))
        };
        done.map(|_| ()).map_err(|e| match e {
            NetError::Refused(w) => NetError::Auth(w),
            e => e,
        })
    }

    /// Every message in the maildrop, with its unique id ([RFC 1939] 7, UIDL) and size (LIST).
    pub fn list(&mut self) -> Result<Vec<Listed>, NetError> {
        self.command("UIDL")?;
        let mut uids = std::collections::BTreeMap::new();
        for line in self.lines(MAX_LIST * 80)? {
            let text = String::from_utf8_lossy(&line);
            let mut parts = text.split_whitespace();
            let (Some(n), Some(uid)) = (parts.next().and_then(|n| n.parse::<u32>().ok()), parts.next()) else {
                return Err(NetError::protocol(format!("not a line of UIDL: {text}")));
            };
            if uid.len() > MAX_UID || !uid.bytes().all(|b| (0x21..=0x7E).contains(&b)) {
                return Err(NetError::protocol("a unique id that is not one"));
            }
            uids.insert(n, uid.to_string());
        }
        self.command("LIST")?;
        let mut out = Vec::with_capacity(uids.len());
        for line in self.lines(MAX_LIST * 40)? {
            let text = String::from_utf8_lossy(&line);
            let mut parts = text.split_whitespace();
            let (Some(n), Some(size)) = (parts.next().and_then(|n| n.parse::<u32>().ok()), parts.next().and_then(|s| s.parse::<u64>().ok())) else {
                return Err(NetError::protocol(format!("not a line of LIST: {text}")));
            };
            if let Some(uid) = uids.remove(&n) {
                out.push(Listed { number: n, uid, size });
            }
        }
        Ok(out)
    }

    /// One message, whole.
    pub fn retrieve(&mut self, number: u32) -> Result<Vec<u8>, NetError> {
        self.command(&format!("RETR {number}"))?;
        let mut out = Vec::new();
        for line in self.lines(MAX_MESSAGE)? {
            out.extend_from_slice(&line);
            out.extend_from_slice(b"\r\n");
        }
        Ok(out)
    }

    /// Marks a message to be removed when the session ends with QUIT.
    pub fn delete(&mut self, number: u32) -> Result<(), NetError> {
        self.command(&format!("DELE {number}")).map(|_| ())
    }

    pub fn quit(mut self) -> Result<(), NetError> {
        self.command("QUIT").map(|_| ())
    }
}
