//! Mail accounts: where mail comes from (IMAP or POP3) and where it goes out (SMTP), as the
//! owner set them up. An account is kept in the settings file without its password, which is
//! kept in the system's credential store under the account's id (`openmailclient_net::Vault`).
//!
//! What an account says comes from a file or a command and is checked as anything else from
//! outside is: a value that makes no sense is refused, never kept.

use crate::EngineError;
use openmailclient_net::{Endpoint, Fingerprint, Protocol, Security, Seen};
use serde_json::{Map, Value, json};

/// Accounts kept at most.
pub const MAX_ACCOUNTS: usize = 64;

/// How mail is taken from the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Folders stay on the server and are kept in step with it.
    Imap,
    /// New mail is taken down into the Inbox.
    Pop3,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Imap => "imap",
            Self::Pop3 => "pop3",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "imap" => Some(Self::Imap),
            "pop3" | "pop" => Some(Self::Pop3),
            _ => None,
        }
    }

    fn protocol(self) -> Protocol {
        match self {
            Self::Imap => Protocol::Imap,
            Self::Pop3 => Protocol::Pop3,
        }
    }
}

/// One server of an account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub host: String,
    pub port: u16,
    pub security: Security,
    /// A certificate of the server's own that the owner trusted, though this system does not:
    /// taken for this server only, and only while it shows exactly that one. Set only by
    /// `account.trustCertificate` and the settings file, and dropped when the host changes.
    pub trusted: Option<Fingerprint>,
}

/// What was taken down from an account so far, so nothing is taken down twice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Received {
    /// IMAP: the Inbox's UIDVALIDITY when its ids were last read; another value means the
    /// server numbered its messages anew ([RFC 9051] 2.3.1.1).
    pub uid_validity: u32,
    /// IMAP: the highest id taken down.
    pub last_uid: u32,
    /// POP3: the unique ids of the messages taken down that the server still had last time.
    pub seen: Vec<String>,
    /// The data file what was taken down went into; empty for the one new mail is kept in.
    /// Taken into another file, nothing of it is in this one: all of it is taken down again.
    pub into: String,
    /// IMAP: how far each folder of the server other than the Inbox was taken down.
    pub folders: Vec<Mark>,
    /// IMAP: the Message-IDs of messages sent from here and put in the server's Sent folder,
    /// so they are not taken down again beside the copy Sent Items keeps already.
    pub sent_here: Vec<String>,
    /// IMAP: messages deleted here for good whose copy on the server is to go too, at the next
    /// Send/Receive.
    pub gone: Vec<Gone>,
    /// IMAP: the messages taken down before each was marked with its place on the server have
    /// been found there again by their Message-ID; until then nothing taken down earlier is put
    /// on the server as if it were new.
    pub matched: bool,
}

/// A message of an IMAP folder that is to be deleted from the server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Gone {
    pub raw: String,
    pub uid_validity: u32,
    pub uid: u32,
}

/// How far one IMAP folder was taken down.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mark {
    /// The folder's name as the server knows it.
    pub raw: String,
    pub uid_validity: u32,
    pub last_uid: u32,
}

/// POP3 ids remembered at most; the server's list is the bound in practice.
pub const MAX_SEEN: usize = 100_000;
/// IMAP folders followed at most, and Message-IDs of what was put in Sent remembered.
pub const MAX_FOLDERS: usize = 1000;
pub const MAX_SENT_HERE: usize = 1000;
/// Deletions waiting to go to a server.
pub const MAX_GONE: usize = 100_000;

/// A name or id as kept in the settings: not empty, not too long, nothing that is not text.
fn kept_text(v: &Value, max: usize) -> Option<&str> {
    v.as_str().filter(|t| !t.is_empty() && t.len() <= max && !t.chars().any(char::is_control))
}

impl Received {
    fn to_json(&self) -> Value {
        let folders: Vec<Value> =
            self.folders.iter().map(|m| json!({ "raw": m.raw, "uidValidity": m.uid_validity, "lastUid": m.last_uid })).collect();
        let gone: Vec<Value> = self.gone.iter().map(|g| json!({ "raw": g.raw, "uidValidity": g.uid_validity, "uid": g.uid })).collect();
        json!({ "uidValidity": self.uid_validity, "lastUid": self.last_uid, "seen": self.seen, "into": self.into,
                "folders": folders, "sentHere": self.sent_here, "gone": gone, "matched": self.matched })
    }

    /// As a settings file gives it: what makes no sense is left as nothing taken down yet.
    fn from_json(v: Option<&Value>) -> Self {
        let Some(v) = v else { return Self::default() };
        let number = |v: &Value, key: &str| v.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
        let n = |key: &str| number(v, key);
        let mut folders: Vec<Mark> = Vec::new();
        for f in v.get("folders").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().take(MAX_FOLDERS) {
            let Some(raw) = f.get("raw").and_then(|r| kept_text(r, 1000)) else { continue };
            if !folders.iter().any(|m| m.raw == raw) {
                folders.push(Mark { raw: raw.to_string(), uid_validity: number(f, "uidValidity"), last_uid: number(f, "lastUid") });
            }
        }
        let sent_here = v
            .get("sentHere")
            .and_then(Value::as_array)
            .map(|all| all.iter().take(MAX_SENT_HERE).filter_map(|m| kept_text(m, 998)).map(str::to_string).collect())
            .unwrap_or_default();
        let seen = v
            .get("seen")
            .and_then(Value::as_array)
            .map(|all| {
                all.iter()
                    .take(MAX_SEEN)
                    .filter_map(Value::as_str)
                    .filter(|u| !u.is_empty() && u.len() <= 70 && u.bytes().all(|b| (0x21..=0x7E).contains(&b)))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let mut gone: Vec<Gone> = Vec::new();
        for g in v.get("gone").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().take(MAX_GONE) {
            let Some(raw) = g.get("raw").and_then(|r| kept_text(r, 1000)) else { continue };
            let one = Gone { raw: raw.to_string(), uid_validity: number(g, "uidValidity"), uid: number(g, "uid") };
            if one.uid > 0 && one.uid_validity > 0 && !gone.contains(&one) {
                gone.push(one);
            }
        }
        let matched = v.get("matched").and_then(Value::as_bool).unwrap_or(false);
        let into = v.get("into").and_then(Value::as_str).filter(|p| p.len() <= 1024 && !p.chars().any(char::is_control)).unwrap_or_default();
        Self { uid_validity: n("uidValidity"), last_uid: n("lastUid"), seen, into: into.to_string(), folders, sent_here, gone, matched }
    }

    /// What was taken down into the data file `into`: nothing, when it went into another.
    pub fn kept_in(&self, into: &str) -> Self {
        if self.into == into { self.clone() } else { Self { into: into.to_string(), sent_here: self.sent_here.clone(), ..Self::default() } }
    }

    /// Remember that a message of the server is to be deleted there.
    pub fn delete_on_server(&mut self, g: Gone) {
        if g.uid == 0 || self.gone.contains(&g) {
            return;
        }
        self.gone.push(g);
        if self.gone.len() > MAX_GONE {
            let over = self.gone.len() - MAX_GONE;
            self.gone.drain(..over);
        }
    }

    /// Remember a message put in the server's Sent folder, the oldest forgotten past the bound.
    pub fn put_in_sent(&mut self, message_id: &str) {
        let id = message_id.trim();
        if id.is_empty() || self.sent_here.iter().any(|m| m == id) {
            return;
        }
        self.sent_here.push(id.to_string());
        if self.sent_here.len() > MAX_SENT_HERE {
            let over = self.sent_here.len() - MAX_SENT_HERE;
            self.sent_here.drain(..over);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    /// What its password is kept under; never changes once made.
    pub id: String,
    /// What the account is called in the folder pane and lists: its address unless named.
    pub name: String,
    pub email: String,
    /// The name mail from it is sent under.
    pub display_name: String,
    pub kind: Kind,
    /// The name the account signs in with: its address unless given.
    pub user: String,
    pub incoming: Server,
    pub outgoing: Server,
    /// Whether the outgoing server is signed in to, with the same name and password.
    pub sign_in_to_send: bool,
    /// POP3: a copy of each message is left on the server once taken down.
    pub leave_on_server: bool,
    /// IMAP: a copy of what is sent is put in the server's Sent folder, so the owner's other
    /// mail programs see it too.
    pub sent_on_server: bool,
    /// What was taken down so far; kept in the settings, not given by `account.list`.
    pub received: Received,
    /// The data file the account's mail is kept in, as Outlook keeps one for each account:
    /// made in the program's own folder the first time it is needed. Set only by the program
    /// and the settings file, never by a command.
    pub data_file: Option<std::path::PathBuf>,
}

fn security_name(s: Security) -> &'static str {
    match s {
        Security::Tls => "tls",
        Security::StartTls => "starttls",
        Security::Plain => "none",
    }
}

fn parse_security(s: &str) -> Option<Security> {
    match s.to_ascii_lowercase().as_str() {
        "tls" | "ssl" | "ssl/tls" => Some(Security::Tls),
        "starttls" => Some(Security::StartTls),
        "none" | "plain" => Some(Security::Plain),
        _ => None,
    }
}

fn bad(what: impl Into<String>) -> EngineError {
    EngineError::BadParams(what.into())
}

/// Text of a field: one line, no more than `max` characters.
fn text<'a>(p: &'a Value, key: &str, max: usize) -> Result<Option<&'a str>, EngineError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.chars().count() <= max && !s.chars().any(char::is_control) => Ok(Some(s.trim())),
        Some(_) => Err(bad(format!("\"{key}\" is one line of no more than {max} characters"))),
    }
}

/// An address mail can be sent from: one `@` with something either side, no spaces.
pub fn is_address(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else { return false };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && s.len() <= 320
        && !s.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | ',' | ';'))
}

/// A server's name, or an address in brackets.
fn is_host(s: &str) -> bool {
    !s.is_empty() && s.len() <= 253 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']'))
}

fn server(p: &Value, key: &str, protocol: Protocol, was: Option<&Server>) -> Result<Server, EngineError> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else {
        return was.cloned().ok_or_else(|| bad(format!("\"{key}\" ({{\"host\", \"port\", \"security\"}}) is required")));
    };
    if !v.is_object() {
        return Err(bad(format!("\"{key}\" is {{\"host\", \"port\", \"security\"}}")));
    }
    let host = match text(v, "host", 253)? {
        Some(h) => h.to_ascii_lowercase(),
        None => was.map(|w| w.host.clone()).ok_or_else(|| bad(format!("\"{key}\" needs a \"host\"")))?,
    };
    if !is_host(&host) {
        return Err(bad(format!("\"{key}\": {} is not a server name", host.chars().take(80).collect::<String>())));
    }
    let security = match text(v, "security", 16)? {
        Some(s) => parse_security(s).ok_or_else(|| bad(format!("\"{key}\": \"security\" is \"tls\", \"starttls\" or \"none\"")))?,
        None => was.map_or(Security::Tls, |w| w.security),
    };
    let port = match v.get("port").filter(|v| !v.is_null()) {
        Some(n) => {
            n.as_u64().and_then(|n| u16::try_from(n).ok()).filter(|n| *n > 0).ok_or_else(|| bad(format!("\"{key}\": \"port\" is from 1 to 65535")))?
        }
        // A port not given follows the security, unless one was set before and the security
        // stays as it was.
        None => match was {
            Some(w) if w.security == security => w.port,
            _ => protocol.port(security),
        },
    };
    // A certificate trusted for one server says nothing of another.
    let trusted = was.filter(|w| w.host == host).and_then(|w| w.trusted);
    Ok(Server { host, port, security, trusted })
}

fn flag(p: &Value, key: &str) -> Result<Option<bool>, EngineError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(format!("\"{key}\" is true or false"))),
    }
}

/// The sign-in name an edit leaves: the one given, else the one before.
fn p_user<'a>(p: &'a Value, was: &'a Account) -> &'a str {
    p.get("user").and_then(Value::as_str).map(str::trim).filter(|u| !u.is_empty()).unwrap_or(&was.user)
}

impl Account {
    /// An account made from what is given, or `was` changed by it.
    pub fn from_json(p: &Value, id: &str, was: Option<&Account>) -> Result<Self, EngineError> {
        if !p.is_object() {
            return Err(bad("an account is an object"));
        }
        let email = match text(p, "email", 320)? {
            Some(e) => e.to_string(),
            None => was.map(|w| w.email.clone()).ok_or_else(|| bad("\"email\" (the account's address) is required"))?,
        };
        if !is_address(&email) {
            return Err(bad(format!("{} is not an address mail can be sent from", email.chars().take(80).collect::<String>())));
        }
        let kind = match text(p, "kind", 8)? {
            Some(k) => Kind::parse(k).ok_or_else(|| bad("\"kind\" is \"imap\" or \"pop3\""))?,
            None => was.map_or(Kind::Imap, |w| w.kind),
        };
        // A changed kind leaves nothing of the incoming server it had.
        let was_in = was.filter(|w| w.kind == kind).map(|w| &w.incoming);
        let incoming = server(p, "incoming", kind.protocol(), was_in)?;
        // The outgoing server is the incoming one's host when not given.
        let fallback = Server { host: incoming.host.clone(), port: Protocol::Smtp.port(Security::Tls), security: Security::Tls, trusted: None };
        let outgoing = server(p, "outgoing", Protocol::Smtp, Some(was.map_or(&fallback, |w| &w.outgoing)))?;
        let pick = |key: &str, max: usize, before: Option<&str>, or: &str| -> Result<String, EngineError> {
            Ok(match text(p, key, max)? {
                Some(t) if !t.is_empty() => t.to_string(),
                Some(_) => or.to_string(),
                None => before.map_or_else(|| or.to_string(), str::to_string),
            })
        };
        Ok(Self {
            id: id.to_string(),
            name: pick("name", 100, was.map(|w| w.name.as_str()), &email)?,
            display_name: pick("displayName", 200, was.map(|w| w.display_name.as_str()), "")?,
            user: pick("user", 320, was.map(|w| w.user.as_str()), &email)?,
            sign_in_to_send: flag(p, "signInToSend")?.unwrap_or(was.is_none_or(|w| w.sign_in_to_send)),
            leave_on_server: flag(p, "leaveOnServer")?.unwrap_or(was.is_none_or(|w| w.leave_on_server)),
            sent_on_server: flag(p, "sentOnServer")?.unwrap_or(was.is_none_or(|w| w.sent_on_server)),
            // What was taken down stays with the account while it reads from the same place.
            received: was
                .filter(|w| w.kind == kind && w.incoming.host == incoming.host && w.user == p_user(p, w))
                .map(|w| w.received.clone())
                .unwrap_or_default(),
            data_file: was.and_then(|w| w.data_file.clone()),
            email,
            kind,
            incoming,
            outgoing,
        })
    }

    /// The account as the settings file keeps it and commands give it: no password.
    pub fn to_json(&self) -> Value {
        let server = |s: &Server| json!({ "host": s.host, "port": s.port, "security": security_name(s.security), "trusted": s.trusted.map(|f| f.to_string()) });
        json!({
            "id": self.id,
            "name": self.name,
            "email": self.email,
            "displayName": self.display_name,
            "kind": self.kind.name(),
            "user": self.user,
            "incoming": server(&self.incoming),
            "outgoing": server(&self.outgoing),
            "signInToSend": self.sign_in_to_send,
            "leaveOnServer": self.leave_on_server,
            "sentOnServer": self.sent_on_server,
            "dataFile": self.data_file.as_ref().map(|p| p.display().to_string()),
        })
    }

    /// The account as the settings file keeps it: with what was taken down so far.
    pub fn to_settings(&self) -> Value {
        let mut v = self.to_json();
        v["received"] = self.received.to_json();
        v
    }

    pub fn incoming_endpoint(&self) -> Endpoint {
        Endpoint {
            protocol: self.kind.protocol(),
            host: self.incoming.host.clone(),
            port: self.incoming.port,
            security: self.incoming.security,
            trusted: self.incoming.trusted,
        }
    }

    pub fn outgoing_endpoint(&self) -> Endpoint {
        Endpoint {
            protocol: Protocol::Smtp,
            host: self.outgoing.host.clone(),
            port: self.outgoing.port,
            security: self.outgoing.security,
            trusted: self.outgoing.trusted,
        }
    }
}

/// An id for a new account: the first `account-N` not taken.
pub fn new_id(accounts: &[Account]) -> String {
    (1..).map(|n: u32| format!("account-{n}")).find(|id| !accounts.iter().any(|a| &a.id == id)).unwrap_or_else(|| "account".to_string())
}

/// An id as the settings file gives it: short, plain and not yet taken.
pub fn kept_id(id: &str, accounts: &[Account]) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !accounts.iter().any(|a| a.id == id)
}

/// The accounts a settings file keeps; those that make no sense are counted, not taken.
pub fn from_settings(v: &Value) -> (Vec<Account>, u32) {
    let mut out: Vec<Account> = Vec::new();
    let mut ignored = 0;
    for a in v.as_array().map(Vec::as_slice).unwrap_or(&[]).iter().take(MAX_ACCOUNTS) {
        let id = a.get("id").and_then(Value::as_str).unwrap_or_default();
        match kept_id(id, &out).then(|| Account::from_json(a, id, None)) {
            Some(Ok(mut account)) => {
                account.received = Received::from_json(a.get("received"));
                account.data_file = a.get("dataFile").and_then(Value::as_str).and_then(data_file_of);
                let trusted = |key: &str| a.get(key).and_then(|s| s.get("trusted")).and_then(Value::as_str).and_then(Fingerprint::parse);
                account.incoming.trusted = trusted("incoming");
                account.outgoing.trusted = trusted("outgoing");
                out.push(account);
            }
            _ => ignored += 1,
        }
    }
    (out, ignored)
}

/// A data file as the settings file names it: a whole path to a `.pst`, nothing else.
fn data_file_of(s: &str) -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(s);
    let pst = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pst"));
    (s.len() <= 1024 && !s.chars().any(char::is_control) && path.is_absolute() && pst).then_some(path)
}

/// The name of an account's data file: its address, with what a file name cannot hold left out.
pub fn data_file_name(account: &Account) -> String {
    let plain: String = account.email.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '@' | '-' | '_' | '+')).take(120).collect();
    let plain = plain.trim_matches('.');
    if plain.is_empty() { format!("{}.pst", account.id) } else { format!("{plain}.pst") }
}

/// What a test of one server came to, in words fit to show; with the certificate it showed,
/// when that was not trusted.
pub(crate) fn outcome(r: Result<String, openmailclient_net::NetError>) -> Value {
    match r {
        Ok(said) => json!({ "ok": true, "said": said }),
        Err(openmailclient_net::NetError::Untrusted(seen)) => {
            json!({ "ok": false, "said": format!("the certificate of {} is not trusted: {}", seen.host, seen.reason), "certificate": shown(&seen) })
        }
        Err(e) => json!({ "ok": false, "said": e.to_string() }),
    }
}

/// A certificate a server showed, as commands give it.
pub fn shown(seen: &Seen) -> Value {
    json!({
        "host": seen.host,
        "fingerprint": seen.fingerprint.to_string(),
        "reason": seen.reason,
        "trustable": seen.trustable,
        "changed": seen.changed,
        "subject": seen.about.subject,
        "issuer": seen.about.issuer,
        "ownMaking": seen.about.own(),
        "names": seen.about.names,
        "notBefore": seen.about.not_before,
        "notAfter": seen.about.not_after,
    })
}

/// Certificates kept of those servers showed in this run that were not trusted.
pub const MAX_SHOWN: usize = 16;

/// The certificates servers showed that were not trusted, the newest last: only these may be
/// trusted, so what the owner trusts is what a server really showed. Shared with the jobs that
/// reach servers, which note them as they are refused.
#[derive(Clone, Default)]
pub struct Shown(std::sync::Arc<std::sync::Mutex<Vec<Seen>>>);

impl Shown {
    pub fn note(&self, seen: &Seen) {
        let Ok(mut all) = self.0.lock() else { return };
        all.retain(|s| !(s.host == seen.host && s.fingerprint == seen.fingerprint));
        all.push(seen.clone());
        if all.len() > MAX_SHOWN {
            let over = all.len() - MAX_SHOWN;
            all.drain(..over);
        }
    }

    /// Those shown by a server.
    pub fn of(&self, host: &str) -> Vec<Seen> {
        self.0.lock().map(|all| all.iter().filter(|s| s.host.eq_ignore_ascii_case(host)).cloned().collect()).unwrap_or_default()
    }

    pub fn forget(&self, host: &str, fingerprint: Fingerprint) {
        if let Ok(mut all) = self.0.lock() {
            all.retain(|s| !(s.host.eq_ignore_ascii_case(host) && s.fingerprint == fingerprint));
        }
    }
}

/// A connector that notes each certificate refused, then gives the refusal on.
pub(crate) struct Noting {
    pub inner: std::sync::Arc<dyn openmailclient_net::Connector>,
    pub shown: Shown,
}

impl Noting {
    fn noted<T>(&self, r: Result<T, openmailclient_net::NetError>) -> Result<T, openmailclient_net::NetError> {
        if let Err(openmailclient_net::NetError::Untrusted(seen)) = &r {
            self.shown.note(seen);
        }
        r
    }
}

impl openmailclient_net::Connector for Noting {
    fn connect(&self, to: &Endpoint) -> Result<Box<dyn openmailclient_net::Stream>, openmailclient_net::NetError> {
        self.noted(self.inner.connect(to))
    }

    fn secure(
        &self,
        plain: Box<dyn openmailclient_net::Stream>,
        to: &Endpoint,
    ) -> Result<Box<dyn openmailclient_net::Stream>, openmailclient_net::NetError> {
        self.noted(self.inner.secure(plain, to))
    }
}

/// Fields of an account given to `account.edit`, less those it does not take. A server's
/// trusted certificate is never taken from them: `account.trustCertificate` alone sets it.
pub(crate) fn changes(p: &Value) -> Value {
    let mut m = Map::new();
    if let Some(all) = p.as_object() {
        for (k, v) in all {
            if !matches!(k.as_str(), "id" | "password" | "received" | "dataFile") {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(m)
}
