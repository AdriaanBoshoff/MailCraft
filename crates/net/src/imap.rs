//! Mail kept on an IMAP server ([RFC 9051], and [RFC 3501], which most servers still speak):
//! folders, the messages in them and their flags. Made secure with STARTTLS or TLS from the
//! start; signed in with SASL PLAIN ([RFC 4616]) or LOGIN.

use crate::conn::{Conn, MAX_LINE};
use crate::{Connector, Endpoint, Login, MAX_MESSAGE, NetError, Security, one_line, said, sasl_plain};

/// Lists inside lists in one response.
const MAX_DEPTH: usize = 32;
/// Literals in one response.
const MAX_LITERALS: usize = 1_000;
/// Untagged responses kept while one command runs.
const MAX_UNTAGGED: usize = 1_000_000;
/// Items in one list.
const MAX_ITEMS: usize = 100_000;
/// Folders taken from LIST.
pub const MAX_FOLDERS: usize = 100_000;

/// A piece of what a server sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tok {
    Atom(String),
    /// A quoted string or a literal.
    Str(Vec<u8>),
    List(Vec<Tok>),
    Nil,
}

impl Tok {
    pub fn atom(&self) -> Option<&str> {
        match self {
            Self::Atom(a) => Some(a),
            _ => None,
        }
    }

    pub fn number(&self) -> Option<u64> {
        self.atom().and_then(|a| a.parse().ok())
    }

    /// An astring or nstring as text.
    pub fn text(&self) -> Option<String> {
        match self {
            Self::Atom(a) => Some(a.clone()),
            Self::Str(s) => Some(String::from_utf8_lossy(s).into_owned()),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ok,
    No,
    Bad,
    Bye,
    Preauth,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resp {
    /// A status response; `tag` is `None` for an untagged one.
    Status { tag: Option<String>, kind: Kind, code: Option<String>, text: String },
    /// Untagged data: `* 3 EXISTS` has the number 3 and the name EXISTS.
    Data { number: Option<u64>, name: String, rest: Vec<Tok> },
    /// `+`: go on.
    Go(String),
}

/// One response, read whole: a line, and while it ends in a literal's count, that many bytes
/// and the line after them.
pub fn read(c: &mut Conn) -> Result<Resp, NetError> {
    parse(&read_raw(c, None)?)
}

/// A response's bytes, literals in place. `go`, when given, is said before each literal is
/// read (the fake server's `+` to a client's synchronizing literal).
pub(crate) fn read_raw(c: &mut Conn, go: Option<&[u8]>) -> Result<Vec<u8>, NetError> {
    let mut raw = Vec::new();
    let mut literals = 0;
    loop {
        let line = c.line()?;
        raw.extend_from_slice(&line);
        let Some(n) = literal_at_end(&line) else { break };
        literals += 1;
        if literals > MAX_LITERALS {
            return Err(NetError::TooLarge("a response from the server"));
        }
        let n = usize::try_from(n)
            .ok()
            .filter(|n| raw.len().saturating_add(*n) <= MAX_MESSAGE + MAX_LINE)
            .ok_or(NetError::TooLarge("a literal from the server"))?;
        raw.extend_from_slice(b"\r\n");
        if let Some(go) = go
            && !line.ends_with(b"+}")
        {
            c.send(go)?;
        }
        raw.extend_from_slice(&c.exact(n)?);
    }
    Ok(raw)
}

/// The count of a literal a line ends with: `{12}` or `{12+}`.
fn literal_at_end(line: &[u8]) -> Option<u64> {
    let body = line.strip_suffix(b"}")?;
    let open = body.iter().rposition(|b| *b == b'{')?;
    let digits = body.get(open + 1..)?;
    let digits = digits.strip_suffix(b"+").unwrap_or(digits);
    if digits.is_empty() || digits.len() > 19 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

/// One response from its bytes, literals in place.
pub fn parse(raw: &[u8]) -> Result<Resp, NetError> {
    let mut p = Parser { b: raw, at: 0 };
    if raw.first() == Some(&b'+') {
        return Ok(Resp::Go(p.rest_text(1)));
    }
    let tag = p.word();
    if tag.is_empty() {
        return Err(NetError::protocol("an empty response"));
    }
    p.space();
    let second = p.word();
    let kind = match second.to_ascii_uppercase().as_str() {
        "OK" => Some(Kind::Ok),
        "NO" => Some(Kind::No),
        "BAD" => Some(Kind::Bad),
        "BYE" => Some(Kind::Bye),
        "PREAUTH" => Some(Kind::Preauth),
        _ => None,
    };
    if let Some(kind) = kind {
        p.space();
        let (code, text) = p.status_text();
        return Ok(Resp::Status { tag: (tag != "*").then_some(tag), kind, code, text });
    }
    if tag != "*" {
        return Err(NetError::protocol(format!("a tagged response that is not a status: {tag} {second}")));
    }
    let (number, name) = match second.parse::<u64>() {
        Ok(n) => {
            p.space();
            (Some(n), p.word().to_ascii_uppercase())
        }
        Err(_) => (None, second.to_ascii_uppercase()),
    };
    let rest = p.items(0, false)?;
    Ok(Resp::Data { number, name, rest })
}

/// The items of a line a client sent, for the fake server.
#[cfg(feature = "fake")]
pub(crate) fn tokens(raw: &[u8]) -> Result<Vec<Tok>, NetError> {
    Parser { b: raw, at: 0 }.items(0, false)
}

struct Parser<'a> {
    b: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.at).copied()
    }

    fn space(&mut self) {
        while self.peek() == Some(b' ') {
            self.at += 1;
        }
    }

    fn word(&mut self) -> String {
        let start = self.at;
        while self.peek().is_some_and(|b| b != b' ') {
            self.at += 1;
        }
        String::from_utf8_lossy(self.b.get(start..self.at).unwrap_or_default()).into_owned()
    }

    fn rest_text(&mut self, from: usize) -> String {
        let text = String::from_utf8_lossy(self.b.get(from.max(self.at)..).unwrap_or_default()).trim().to_string();
        self.at = self.b.len();
        text
    }

    /// `[CODE ...] text`, the code without its brackets.
    fn status_text(&mut self) -> (Option<String>, String) {
        if self.peek() == Some(b'[') {
            let start = self.at + 1;
            let mut depth = 0usize;
            while let Some(b) = self.peek() {
                self.at += 1;
                match b {
                    b'[' => depth += 1,
                    b']' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            let code = String::from_utf8_lossy(self.b.get(start..self.at - 1).unwrap_or_default()).into_owned();
                            return (Some(code), self.rest_text(self.at));
                        }
                    }
                    _ => {}
                }
            }
            self.at = start - 1;
        }
        (None, self.rest_text(self.at))
    }

    /// Items up to the end, or to the `)` that closes a list.
    fn items(&mut self, depth: usize, in_list: bool) -> Result<Vec<Tok>, NetError> {
        if depth > MAX_DEPTH {
            return Err(NetError::TooLarge("lists inside lists"));
        }
        let mut out = Vec::new();
        loop {
            self.space();
            match self.peek() {
                None if in_list => return Err(NetError::protocol("a list that is not closed")),
                None => return Ok(out),
                Some(b')') if in_list => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b')') => return Err(NetError::protocol("a list closed that was not opened")),
                Some(b'(') => {
                    self.at += 1;
                    out.push(Tok::List(self.items(depth + 1, true)?));
                }
                Some(b'"') => out.push(Tok::Str(self.quoted()?)),
                Some(b'{') => out.push(Tok::Str(self.literal()?)),
                Some(_) => {
                    let a = self.atom()?;
                    out.push(if a.eq_ignore_ascii_case("NIL") { Tok::Nil } else { Tok::Atom(a) });
                }
            }
            if out.len() > MAX_ITEMS {
                return Err(NetError::TooLarge("a list from the server"));
            }
        }
    }

    fn quoted(&mut self) -> Result<Vec<u8>, NetError> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None | Some(b'\r' | b'\n') => return Err(NetError::protocol("a quoted string that is not closed")),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    if let Some(b) = self.peek() {
                        out.push(b);
                        self.at += 1;
                    }
                }
                Some(b) => {
                    out.push(b);
                    self.at += 1;
                }
            }
        }
    }

    fn literal(&mut self) -> Result<Vec<u8>, NetError> {
        let close =
            self.b.get(self.at..).and_then(|r| r.iter().position(|b| *b == b'}')).ok_or_else(|| NetError::protocol("a literal without its count"))?;
        let count = self.b.get(self.at + 1..self.at + close).unwrap_or_default();
        let count = count.strip_suffix(b"+").unwrap_or(count);
        let n: usize = std::str::from_utf8(count)
            .ok()
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| NetError::protocol("a literal's count that is not a number"))?;
        self.at += close + 1;
        if self.b.get(self.at..self.at + 2) != Some(b"\r\n") {
            return Err(NetError::protocol("a literal not on a line of its own"));
        }
        self.at += 2;
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len()).ok_or_else(|| NetError::protocol("a literal shorter than its count"))?;
        let out = self.b.get(self.at..end).unwrap_or_default().to_vec();
        self.at = end;
        Ok(out)
    }

    /// An atom, with a `[section]` and a `<partial>` kept in it (`BODY[HEADER.FIELDS (A B)]<0>`).
    fn atom(&mut self) -> Result<String, NetError> {
        let start = self.at;
        let mut depth = 0usize;
        while let Some(b) = self.peek() {
            match b {
                b'[' => depth += 1,
                b']' => depth = depth.saturating_sub(1),
                b' ' | b'(' | b')' | b'\r' | b'\n' if depth == 0 => break,
                b'\r' | b'\n' => return Err(NetError::protocol("a section that is not closed")),
                _ => {}
            }
            self.at += 1;
        }
        if self.at == start {
            return Err(NetError::protocol("an empty atom"));
        }
        Ok(String::from_utf8_lossy(self.b.get(start..self.at).unwrap_or_default()).into_owned())
    }
}

/// A part of a command: text, or bytes sent as a literal.
enum Part {
    Text(String),
    Lit(Vec<u8>),
}

/// A string as a command carries it: quoted when it can be, as a literal otherwise.
fn astring(s: &[u8]) -> Part {
    if s.iter().all(|b| (0x20..0x7F).contains(b)) {
        let mut q = String::with_capacity(s.len() + 2);
        q.push('"');
        for &b in s {
            if b == b'"' || b == b'\\' {
                q.push('\\');
            }
            q.push(char::from(b));
        }
        q.push('"');
        Part::Text(q)
    } else {
        Part::Lit(s.to_vec())
    }
}

/// A folder on the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// The name as it reads.
    pub name: String,
    /// The name as the server knows it, for commands.
    pub raw: String,
    pub delimiter: Option<char>,
    /// `\Noselect`, `\HasChildren`, and the special uses `\Sent`, `\Drafts`, `\Trash`,
    /// `\Junk`, `\Archive` ([RFC 6154]).
    pub attributes: Vec<String>,
}

impl Folder {
    pub fn has(&self, attribute: &str) -> bool {
        self.attributes.iter().any(|a| a.eq_ignore_ascii_case(attribute))
    }

    pub fn selectable(&self) -> bool {
        !self.has("\\Noselect") && !self.has("\\NonExistent")
    }
}

/// A folder once selected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selected {
    pub exists: u64,
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub read_only: bool,
}

/// A message as listed: its id, flags and size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub uid: u32,
    pub flags: Vec<String>,
    pub size: u64,
}

pub struct Imap {
    c: Conn,
    tag: u32,
    caps: Vec<String>,
}

impl Imap {
    pub fn open(with: &dyn Connector, to: &Endpoint, login: &Login) -> Result<Self, NetError> {
        let mut i = Self { c: Conn::new(with.connect(to)?), tag: 0, caps: Vec::new() };
        let preauth = match read(&mut i.c)? {
            Resp::Status { tag: None, kind: Kind::Ok, code, .. } => {
                i.caps_from(code.as_deref());
                false
            }
            Resp::Status { tag: None, kind: Kind::Preauth, .. } => true,
            Resp::Status { tag: None, kind: Kind::Bye, text, .. } => return Err(NetError::Refused(text)),
            _ => return Err(NetError::protocol("not a greeting")),
        };
        let mut secure = to.security == Security::Tls;
        if to.security == Security::StartTls {
            if preauth {
                // Signed in before it could be made secure: nothing about the connection can
                // be trusted ([RFC 9051] 7.1.4).
                return Err(NetError::NotSecure);
            }
            if i.caps.is_empty() {
                i.capability()?;
            }
            if !i.has("STARTTLS") {
                return Err(NetError::NotSecure);
            }
            i.run(vec![Part::Text("STARTTLS".into())])?;
            let Self { c, tag, .. } = i;
            i = Self { c: c.secure(with, &to.host)?, tag, caps: Vec::new() };
            secure = true;
        }
        if !secure {
            return Err(NetError::NotSecure);
        }
        if !preauth {
            i.sign_in(login)?;
        }
        i.capability()?;
        Ok(i)
    }

    fn caps_from(&mut self, code: Option<&str>) {
        if let Some(rest) = code.and_then(|c| c.strip_prefix("CAPABILITY ")) {
            self.caps = rest.split_whitespace().map(|c| c.to_ascii_uppercase()).collect();
        }
    }

    pub fn capabilities(&self) -> &[String] {
        &self.caps
    }

    pub fn has(&self, cap: &str) -> bool {
        self.caps.iter().any(|c| c.eq_ignore_ascii_case(cap))
    }

    fn capability(&mut self) -> Result<(), NetError> {
        let (data, _) = self.run(vec![Part::Text("CAPABILITY".into())])?;
        for d in data {
            if let Resp::Data { name, rest, .. } = d
                && name == "CAPABILITY"
            {
                self.caps = rest.iter().filter_map(|t| t.atom()).map(|c| c.to_ascii_uppercase()).collect();
            }
        }
        Ok(())
    }

    fn sign_in(&mut self, login: &Login) -> Result<(), NetError> {
        one_line("user name", &login.user)?;
        one_line("password", login.password.expose())?;
        let done = if self.has("AUTH=PLAIN") {
            if self.has("SASL-IR") {
                self.run(vec![Part::Text(format!("AUTHENTICATE PLAIN {}", sasl_plain(login)))])
            } else {
                self.authenticate(&sasl_plain(login))
            }
        } else if self.has("LOGINDISABLED") {
            return Err(NetError::Unsupported("the server signs in only in ways this version does not speak".into()));
        } else {
            self.run(vec![
                Part::Text("LOGIN ".into()),
                astring(login.user.as_bytes()),
                Part::Text(" ".into()),
                astring(login.password.expose().as_bytes()),
            ])
        };
        match done {
            Ok((_, code)) => {
                self.caps_from(code.as_deref());
                Ok(())
            }
            Err(NetError::Refused(w)) => Err(NetError::Auth(w)),
            Err(e) => Err(e),
        }
    }

    /// AUTHENTICATE PLAIN without an initial response: the server asks, we answer.
    fn authenticate(&mut self, answer: &str) -> Result<(Vec<Resp>, Option<String>), NetError> {
        let tag = self.next_tag();
        self.c.send(format!("{tag} AUTHENTICATE PLAIN\r\n").as_bytes())?;
        loop {
            match read(&mut self.c)? {
                Resp::Go(_) => {
                    self.c.send(format!("{answer}\r\n").as_bytes())?;
                    return self.finish(&tag, Vec::new());
                }
                Resp::Status { tag: Some(t), kind, text, .. } if t == tag => return Err(status_error(kind, text)),
                _ => {}
            }
        }
    }

    fn next_tag(&mut self) -> String {
        self.tag = self.tag.wrapping_add(1);
        format!("A{:04}", self.tag)
    }

    /// Send a command and read up to its tagged answer: the untagged responses on the way,
    /// and the code of the answer.
    fn run(&mut self, parts: Vec<Part>) -> Result<(Vec<Resp>, Option<String>), NetError> {
        let tag = self.next_tag();
        let literal_plus = self.has("LITERAL+");
        let mut line = format!("{tag} ").into_bytes();
        let mut data = Vec::new();
        for part in parts {
            match part {
                Part::Text(t) => line.extend_from_slice(t.as_bytes()),
                Part::Lit(bytes) => {
                    if literal_plus || (self.has("LITERAL-") && bytes.len() <= 4096) {
                        line.extend_from_slice(format!("{{{}+}}\r\n", bytes.len()).as_bytes());
                    } else {
                        line.extend_from_slice(format!("{{{}}}\r\n", bytes.len()).as_bytes());
                        self.c.send(&line)?;
                        line.clear();
                        // Wait to be told to go on.
                        loop {
                            match read(&mut self.c)? {
                                Resp::Go(_) => break,
                                Resp::Status { tag: Some(t), kind, text, .. } if t == tag => return Err(status_error(kind, text)),
                                Resp::Status { tag: None, kind: Kind::Bye, text, .. } => return Err(NetError::Refused(text)),
                                other => keep(&mut data, other)?,
                            }
                        }
                    }
                    line.extend_from_slice(&bytes);
                }
            }
        }
        line.extend_from_slice(b"\r\n");
        self.c.send(&line)?;
        self.finish(&tag, data)
    }

    fn finish(&mut self, tag: &str, mut data: Vec<Resp>) -> Result<(Vec<Resp>, Option<String>), NetError> {
        loop {
            match read(&mut self.c)? {
                Resp::Status { tag: Some(t), kind, code, text } if t == tag => {
                    return match kind {
                        Kind::Ok => Ok((data, code)),
                        kind => Err(status_error(kind, text)),
                    };
                }
                Resp::Status { tag: Some(t), .. } => return Err(NetError::protocol(format!("an answer to a command not sent: {t}"))),
                Resp::Status { tag: None, kind: Kind::Bye, text, .. } if !tag.is_empty() => {
                    // BYE comes before the answer to LOGOUT; to anything else it ends the session.
                    data.push(Resp::Status { tag: None, kind: Kind::Bye, code: None, text });
                }
                Resp::Go(_) => return Err(NetError::protocol("asked to go on when nothing waits")),
                other => keep(&mut data, other)?,
            }
        }
    }

    /// Every folder, with its special use when the server says it ([RFC 6154]).
    pub fn folders(&mut self) -> Result<Vec<Folder>, NetError> {
        let command = if self.has("SPECIAL-USE") { "LIST \"\" \"*\" RETURN (SPECIAL-USE)" } else { "LIST \"\" \"*\"" };
        let (data, _) = self.run(vec![Part::Text(command.into())])?;
        let mut out = Vec::new();
        for d in data {
            let Resp::Data { name, rest, .. } = d else { continue };
            if name != "LIST" {
                continue;
            }
            let [Tok::List(attrs), delimiter, raw, ..] = rest.as_slice() else {
                return Err(NetError::protocol("a LIST response that is not one"));
            };
            let Some(raw) = raw.text() else { return Err(NetError::protocol("a folder without a name")) };
            let delimiter = delimiter.text().and_then(|d| d.chars().next());
            let attributes = attrs.iter().filter_map(|a| a.atom()).map(str::to_string).collect();
            let name = if raw.eq_ignore_ascii_case("INBOX") { "Inbox".to_string() } else { crate::utf7::decode(&raw) };
            out.push(Folder { name, raw, delimiter, attributes });
            if out.len() > MAX_FOLDERS {
                return Err(NetError::TooLarge("the list of folders"));
            }
        }
        Ok(out)
    }

    pub fn select(&mut self, raw: &str) -> Result<Selected, NetError> {
        let (data, code) = self.run(vec![Part::Text("SELECT ".into()), astring(raw.as_bytes())])?;
        let mut s = Selected { read_only: code.as_deref().is_some_and(|c| c.eq_ignore_ascii_case("READ-ONLY")), ..Selected::default() };
        for d in data {
            match d {
                Resp::Data { number: Some(n), name, .. } if name == "EXISTS" => s.exists = n,
                Resp::Status { tag: None, kind: Kind::Ok, code: Some(code), .. } => {
                    let mut words = code.split_whitespace();
                    let (key, value) = (words.next().unwrap_or_default().to_ascii_uppercase(), words.next().and_then(|v| v.parse::<u32>().ok()));
                    match key.as_str() {
                        "UIDVALIDITY" => s.uid_validity = value,
                        "UIDNEXT" => s.uid_next = value,
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Ok(s)
    }

    /// The messages of the selected folder from `from` on: id, flags and size.
    pub fn list(&mut self, from: u32) -> Result<Vec<Summary>, NetError> {
        let from = from.max(1);
        let (data, _) = self.run(vec![Part::Text(format!("UID FETCH {from}:* (UID FLAGS RFC822.SIZE)"))])?;
        let mut out = Vec::new();
        for d in data {
            let Some(items) = fetched(&d) else { continue };
            let Some(uid) = item(items, "UID").and_then(Tok::number).and_then(|u| u32::try_from(u).ok()) else { continue };
            // `n:*` takes in the last message even when its id is below n ([RFC 9051] 6.4.9).
            if uid < from {
                continue;
            }
            let flags = match item(items, "FLAGS") {
                Some(Tok::List(f)) => f.iter().filter_map(|t| t.atom()).map(str::to_string).collect(),
                _ => Vec::new(),
            };
            let size = item(items, "RFC822.SIZE").and_then(Tok::number).unwrap_or(0);
            out.push(Summary { uid, flags, size });
        }
        out.sort_by_key(|s| s.uid);
        out.dedup_by_key(|s| s.uid);
        Ok(out)
    }

    /// One message of the selected folder, whole, without marking it read.
    pub fn fetch(&mut self, uid: u32) -> Result<Option<Vec<u8>>, NetError> {
        let (data, _) = self.run(vec![Part::Text(format!("UID FETCH {uid} (UID BODY.PEEK[])"))])?;
        for d in data {
            let Some(items) = fetched(&d) else { continue };
            if item(items, "UID").and_then(Tok::number) != Some(u64::from(uid)) {
                continue;
            }
            if let Some(Tok::Str(body)) = item(items, "BODY[]") {
                return Ok(Some(body.clone()));
            }
        }
        Ok(None)
    }

    /// Puts flags on a message of the selected folder, or takes them off.
    pub fn flag(&mut self, uid: u32, on: bool, flags: &[&str]) -> Result<(), NetError> {
        if flags.iter().any(|f| f.is_empty() || f.chars().any(|c| c.is_control() || matches!(c, ' ' | '(' | ')' | '"' | '{'))) {
            return Err(NetError::Unsupported("not a flag".into()));
        }
        let sign = if on { '+' } else { '-' };
        self.run(vec![Part::Text(format!("UID STORE {uid} {sign}FLAGS.SILENT ({})", flags.join(" ")))]).map(|_| ())
    }

    /// Adds a message to a folder; its id there when the server says it ([RFC 4315] UIDPLUS).
    pub fn append(&mut self, raw: &str, flags: &[&str], message: &[u8]) -> Result<Option<u32>, NetError> {
        if message.len() > MAX_MESSAGE {
            return Err(NetError::TooLarge("the message"));
        }
        let mut parts = vec![Part::Text("APPEND ".into()), astring(raw.as_bytes())];
        if !flags.is_empty() {
            parts.push(Part::Text(format!(" ({})", flags.join(" "))));
        }
        parts.push(Part::Text(" ".into()));
        parts.push(Part::Lit(message.to_vec()));
        let (_, code) = self.run(parts)?;
        Ok(code.and_then(|c| {
            let mut w = c.split_whitespace();
            (w.next()?.eq_ignore_ascii_case("APPENDUID")).then_some(())?;
            w.nth(1)?.parse().ok()
        }))
    }

    pub fn logout(mut self) -> Result<(), NetError> {
        self.run(vec![Part::Text("LOGOUT".into())]).map(|_| ())
    }
}

fn status_error(kind: Kind, text: String) -> NetError {
    match kind {
        Kind::Bad => NetError::protocol(format!("the server did not understand: {text}")),
        _ => NetError::Refused(said(&text)),
    }
}

fn keep(data: &mut Vec<Resp>, r: Resp) -> Result<(), NetError> {
    if data.len() >= MAX_UNTAGGED {
        return Err(NetError::TooLarge("the answer to one command"));
    }
    data.push(r);
    Ok(())
}

/// The items of a FETCH response.
fn fetched(r: &Resp) -> Option<&[Tok]> {
    match r {
        Resp::Data { name, rest, .. } if name == "FETCH" => match rest.first() {
            Some(Tok::List(items)) => Some(items),
            _ => None,
        },
        _ => None,
    }
}

/// The value after a name in a FETCH response's list.
fn item<'a>(items: &'a [Tok], name: &str) -> Option<&'a Tok> {
    items.chunks(2).find(|pair| pair.first().and_then(Tok::atom).is_some_and(|a| a.eq_ignore_ascii_case(name))).and_then(|pair| pair.get(1))
}
