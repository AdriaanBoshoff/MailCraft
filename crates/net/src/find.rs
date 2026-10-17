//! An account's servers found from its address, as Auto Account Setup finds them, in this
//! order:
//!
//! 1. what is known of large providers, kept here (`KNOWN`);
//! 2. the settings the address's domain publishes for mail programs, an XML file at
//!    `https://autoconfig.<domain>/mail/config-v1.1.xml` or under
//!    `https://<domain>/.well-known/autoconfig/`, fetched over HTTPS only;
//! 3. the usual names of a domain's mail servers (`imap.`, `mail.`, `smtp.` and the domain
//!    itself), each greeted over TLS, or made secure with STARTTLS, and signed in to by none.
//!
//! The address itself is sent to no one: only its domain is named, in the server names asked
//! for. What a web server sends is hostile: its size is capped, it is read as XML without
//! anything fetched for it, and a name or port in it that is not one is passed over. A server
//! that offers no secure connection is never chosen. Finding servers by DNS SRV records
//! ([RFC 6186]) is a do-later item: it asks for a resolver this program does not have yet.
//!
//! [RFC 6186]: https://www.rfc-editor.org/rfc/rfc6186

use crate::{Connector, Endpoint, NetError, Protocol, Security, http};

/// The settings file is no larger than this.
const MAX_SETTINGS: usize = 64 * 1024;
/// Nodes read of it.
const MAX_NODES: u32 = 4096;
/// Elements inside one another: the reader goes down each with a call of its own, so a file
/// nested deeper than this is not read at all. A settings file needs four.
const MAX_DEPTH: usize = 32;

/// Where the settings were found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// What is known here of a large provider.
    Known,
    /// The settings the domain publishes.
    Published,
    /// The usual names, tried: each server chosen greeted us.
    Tried,
}

impl Source {
    pub fn word(self) -> &'static str {
        match self {
            Self::Known => "known",
            Self::Published => "published",
            Self::Tried => "tried",
        }
    }
}

/// A server, as an account names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub host: String,
    pub port: u16,
    pub security: Security,
}

/// The settings found for an address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// IMAP or POP3.
    pub protocol: Protocol,
    pub incoming: Server,
    pub outgoing: Server,
    /// The name to sign in with.
    pub user: String,
    pub from: Source,
    /// What the owner should know: that a provider asks for an app password, say.
    pub note: Option<&'static str>,
}

/// Large providers: their domains, IMAP and SMTP servers, and a note. From what each provider
/// publishes for mail programs; our own list.
const KNOWN: &[(&[&str], (&str, u16, Security), (&str, u16, Security), Option<&str>)] = &[
    (
        &["gmail.com", "googlemail.com"],
        ("imap.gmail.com", 993, Security::Tls),
        ("smtp.gmail.com", 465, Security::Tls),
        Some("Google signs in mail programs with an app password made in the Google account, or with Google's own sign-in (later)"),
    ),
    (
        &["outlook.com", "hotmail.com", "live.com", "msn.com", "hotmail.co.uk", "outlook.co.uk", "live.co.uk"],
        ("outlook.office365.com", 993, Security::Tls),
        ("smtp-mail.outlook.com", 587, Security::StartTls),
        Some("Microsoft signs in mail programs with its own sign-in (later); a password alone may be refused"),
    ),
    (
        &["yahoo.com", "ymail.com", "rocketmail.com", "yahoo.co.uk"],
        ("imap.mail.yahoo.com", 993, Security::Tls),
        ("smtp.mail.yahoo.com", 465, Security::Tls),
        Some("Yahoo signs in mail programs with an app password made in the Yahoo account"),
    ),
    (
        &["aol.com"],
        ("imap.aol.com", 993, Security::Tls),
        ("smtp.aol.com", 465, Security::Tls),
        Some("AOL signs in mail programs with an app password made in the AOL account"),
    ),
    (
        &["icloud.com", "me.com", "mac.com"],
        ("imap.mail.me.com", 993, Security::Tls),
        ("smtp.mail.me.com", 587, Security::StartTls),
        Some("Apple signs in mail programs with an app-specific password made in the Apple account"),
    ),
    (
        &["fastmail.com", "fastmail.fm"],
        ("imap.fastmail.com", 993, Security::Tls),
        ("smtp.fastmail.com", 465, Security::Tls),
        Some("Fastmail signs in mail programs with an app password"),
    ),
    (&["gmx.com", "gmx.net", "gmx.de"], ("imap.gmx.com", 993, Security::Tls), ("mail.gmx.com", 587, Security::StartTls), None),
    (&["zoho.com", "zohomail.com"], ("imap.zoho.com", 993, Security::Tls), ("smtp.zoho.com", 465, Security::Tls), None),
    (
        &["yandex.com", "yandex.ru"],
        ("imap.yandex.com", 993, Security::Tls),
        ("smtp.yandex.com", 465, Security::Tls),
        Some("Yandex signs in mail programs with an app password"),
    ),
];

/// An address's local part and domain, the domain in lower case; `None` for what is not one.
pub fn parts(email: &str) -> Option<(&str, String)> {
    let email = email.trim();
    let (local, domain) = email.rsplit_once('@')?;
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    (!local.is_empty() && local.len() <= 64 && is_host(&domain) && domain.contains('.')).then_some((local, domain))
}

/// A host name as DNS has them ([RFC 1123] 2.1): labels of letters, digits and hyphens.
pub fn is_host(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// What is known here of the address's provider.
pub fn known(email: &str) -> Option<Found> {
    let (_, domain) = parts(email)?;
    let (_, inc, out, note) = KNOWN.iter().find(|(domains, ..)| domains.contains(&domain.as_str()))?;
    let server = |(host, port, security): (&str, u16, Security)| Server { host: host.to_string(), port, security };
    Some(Found {
        protocol: Protocol::Imap,
        incoming: server(*inc),
        outgoing: server(*out),
        user: email.trim().to_string(),
        from: Source::Known,
        note: *note,
    })
}

/// The settings for `email` in a published settings file, IMAP before POP3, secure servers
/// only. `None` when it holds none that can be used.
pub fn published(xml: &[u8], email: &str) -> Option<Found> {
    let (local, domain) = parts(email)?;
    if xml.len() > MAX_SETTINGS {
        return None;
    }
    let text = std::str::from_utf8(xml).ok()?;
    if !shallow(text.as_bytes(), MAX_DEPTH) {
        return None;
    }
    let options = roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: MAX_NODES, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(text, options).ok()?;
    let fill = |s: &str| s.trim().replace("%EMAILADDRESS%", email.trim()).replace("%EMAILLOCALPART%", local).replace("%EMAILDOMAIN%", &domain);
    let read = |node: roxmltree::Node<'_, '_>| -> Option<(Server, String)> {
        let child = |name: &str| node.children().find(|c| c.has_tag_name(name)).and_then(|c| c.text()).map(fill);
        let host = child("hostname")?.to_ascii_lowercase();
        let port = child("port")?.parse::<u16>().ok().filter(|p| *p != 0)?;
        let security = match child("socketType")?.to_ascii_uppercase().as_str() {
            "SSL" | "TLS" => Security::Tls,
            "STARTTLS" => Security::StartTls,
            _ => return None,
        };
        let user = child("username")
            .filter(|u| !u.is_empty() && u.len() <= 256 && !u.chars().any(char::is_control))
            .unwrap_or_else(|| email.trim().to_string());
        is_host(&host).then_some((Server { host, port, security }, user))
    };
    let servers = |tag: &str, kind: &str| {
        doc.descendants()
            .filter(move |n| n.has_tag_name(tag) && n.attribute("type").is_some_and(|t| t.eq_ignore_ascii_case(kind)))
            .filter_map(read)
            .next()
    };
    let (protocol, (incoming, user)) = match servers("incomingServer", "imap") {
        Some(found) => (Protocol::Imap, found),
        None => (Protocol::Pop3, servers("incomingServer", "pop3")?),
    };
    let (outgoing, _) = servers("outgoingServer", "smtp")?;
    Some(Found { protocol, incoming, outgoing, user, from: Source::Published, note: None })
}

/// Whether XML nests elements no deeper than `most`. Comments, CDATA, processing
/// instructions and declarations are passed over whole, and quoted attribute values are read as
/// such, so nothing in them can hide an element; where it cannot tell, it counts one more.
fn shallow(xml: &[u8], most: usize) -> bool {
    let after = |at: usize, end: &[u8]| xml.get(at..).and_then(|rest| rest.windows(end.len()).position(|w| w == end)).map(|n| at + n + end.len());
    let (mut at, mut depth) = (0usize, 0usize);
    while let Some(n) = xml.get(at..).and_then(|rest| rest.iter().position(|b| *b == b'<')) {
        let open = at + n;
        let rest = xml.get(open..).unwrap_or_default();
        let skip = if rest.starts_with(b"<!--") {
            after(open + 4, b"-->")
        } else if rest.starts_with(b"<![CDATA[") {
            after(open + 9, b"]]>")
        } else if rest.starts_with(b"<?") {
            after(open + 2, b"?>")
        } else {
            None
        };
        if let Some(next) = skip {
            at = next;
            continue;
        }
        // A tag: to its `>`, quotes read as quotes.
        let (mut quote, mut close) = (None::<u8>, None::<usize>);
        for (i, b) in rest.iter().enumerate().skip(1) {
            match (quote, *b) {
                (Some(q), b) if b == q => quote = None,
                (Some(_), _) => {}
                (None, b'"' | b'\'') => quote = Some(*b),
                (None, b'>') => {
                    close = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(close) = close else { return depth < most };
        let tag = rest.get(..=close).unwrap_or_default();
        if tag.starts_with(b"</") {
            depth = depth.saturating_sub(1);
        } else if !tag.starts_with(b"<!") && !tag.ends_with(b"/>") {
            depth += 1;
            if depth > most {
                return false;
            }
        }
        at = open + close + 1;
    }
    true
}

/// Where a domain's settings may be published, in the order they are asked for.
pub fn settings_addresses(domain: &str) -> [String; 2] {
    [format!("https://autoconfig.{domain}/mail/config-v1.1.xml"), format!("https://{domain}/.well-known/autoconfig/mail/config-v1.1.xml")]
}

/// The usual servers of a domain, in the order they are tried: incoming (IMAP), then
/// outgoing.
pub fn usual(domain: &str) -> (Vec<Server>, Vec<Server>) {
    let at = |prefix: &str, port: u16, security: Security| Server { host: format!("{prefix}{domain}"), port, security };
    let incoming = vec![
        at("imap.", 993, Security::Tls),
        at("mail.", 993, Security::Tls),
        at("", 993, Security::Tls),
        at("imap.", 143, Security::StartTls),
        at("mail.", 143, Security::StartTls),
    ];
    let outgoing = vec![
        at("smtp.", 465, Security::Tls),
        at("smtp.", 587, Security::StartTls),
        at("mail.", 465, Security::Tls),
        at("mail.", 587, Security::StartTls),
        at("", 465, Security::Tls),
        at("", 587, Security::StartTls),
    ];
    (incoming, outgoing)
}

/// Whether a server greets us as its protocol does, made secure; nothing is signed in to.
pub fn greets(with: &dyn Connector, protocol: Protocol, server: &Server) -> Result<(), NetError> {
    let to = Endpoint { protocol, host: server.host.clone(), port: server.port, security: server.security, trusted: None };
    match protocol {
        Protocol::Imap => crate::imap::Imap::greets(with, &to),
        Protocol::Pop3 => crate::pop3::Pop3::greets(with, &to),
        Protocol::Smtp => crate::smtp::Smtp::open(with, &to, None)?.quit(),
        Protocol::Https => Err(NetError::Unsupported("not a mail server".into())),
    }
}

/// Find the settings for `email`. `stopped` is asked between one server and the next.
pub fn find(with: &dyn Connector, email: &str, stopped: &dyn Fn() -> bool) -> Result<Found, NetError> {
    let Some((_, domain)) = parts(email) else { return Err(NetError::Unsupported("that is not an email address".into())) };
    if let Some(found) = known(email) {
        return Ok(found);
    }
    for address in settings_addresses(&domain) {
        if stopped() {
            return Err(NetError::Unsupported("stopped".into()));
        }
        if let Ok(got) = http::get_settings(with, &address, MAX_SETTINGS)
            && let Some(found) = published(&got.bytes, email)
        {
            return Ok(found);
        }
    }
    let (incoming, outgoing) = usual(&domain);
    let first = |protocol: Protocol, servers: Vec<Server>| -> Result<Option<Server>, NetError> {
        for server in servers {
            if stopped() {
                return Err(NetError::Unsupported("stopped".into()));
            }
            if greets(with, protocol, &server).is_ok() {
                return Ok(Some(server));
            }
        }
        Ok(None)
    };
    let Some(incoming) = first(Protocol::Imap, incoming)? else {
        return Err(NetError::Unsupported(format!(
            "no settings were found for {domain}: it publishes none, and none of the usual servers answered securely"
        )));
    };
    let Some(outgoing) = first(Protocol::Smtp, outgoing)? else {
        return Err(NetError::Unsupported(format!("{} answered for incoming mail, but none of the usual servers for sending did", incoming.host)));
    };
    Ok(Found { protocol: Protocol::Imap, incoming, outgoing, user: email.trim().to_string(), from: Source::Tried, note: None })
}
