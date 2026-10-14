//! Mail accounts: where mail comes from (IMAP or POP3) and where it goes out (SMTP), as the
//! owner set them up. An account is kept in the settings file without its password, which is
//! kept in the system's credential store under the account's id (`openmailclient_net::Vault`).
//!
//! What an account says comes from a file or a command and is checked as anything else from
//! outside is: a value that makes no sense is refused, never kept.

use crate::EngineError;
use openmailclient_net::{Endpoint, Protocol, Security};
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
    Ok(Server { host, port, security })
}

fn flag(p: &Value, key: &str) -> Result<Option<bool>, EngineError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(format!("\"{key}\" is true or false"))),
    }
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
        let fallback = Server { host: incoming.host.clone(), port: Protocol::Smtp.port(Security::Tls), security: Security::Tls };
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
            email,
            kind,
            incoming,
            outgoing,
        })
    }

    /// The account as the settings file keeps it and commands give it: no password.
    pub fn to_json(&self) -> Value {
        let server = |s: &Server| json!({ "host": s.host, "port": s.port, "security": security_name(s.security) });
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
        })
    }

    pub fn incoming_endpoint(&self) -> Endpoint {
        Endpoint { protocol: self.kind.protocol(), host: self.incoming.host.clone(), port: self.incoming.port, security: self.incoming.security }
    }

    pub fn outgoing_endpoint(&self) -> Endpoint {
        Endpoint { protocol: Protocol::Smtp, host: self.outgoing.host.clone(), port: self.outgoing.port, security: self.outgoing.security }
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
            Some(Ok(account)) => out.push(account),
            _ => ignored += 1,
        }
    }
    (out, ignored)
}

/// What a test of one server came to, in words fit to show.
pub(crate) fn outcome(r: Result<String, openmailclient_net::NetError>) -> Value {
    match r {
        Ok(said) => json!({ "ok": true, "said": said }),
        Err(e) => json!({ "ok": false, "said": e.to_string() }),
    }
}

/// Fields of an account given to `account.edit`, less those it does not take.
pub(crate) fn changes(p: &Value) -> Value {
    let mut m = Map::new();
    if let Some(all) = p.as_object() {
        for (k, v) in all {
            if !matches!(k.as_str(), "id" | "password") {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(m)
}
