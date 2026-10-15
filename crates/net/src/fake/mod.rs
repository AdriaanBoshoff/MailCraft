//! An in-process mail server for tests: IMAP, POP3 and SMTP over an in-memory [`pipe`], with
//! mailboxes held in memory. No socket is opened and nothing leaves the process; this is what
//! the tests of this crate and the engine talk to, never a real server.
//!
//! It speaks only what the clients here say, and checks it: a command it does not know is
//! answered as an error. [`Fake::script`] instead plays back fixed bytes, for hostile-server
//! tests.

mod http;
mod imap;
mod pop3;
mod smtp;

use crate::{Connector, End, Endpoint, NetError, Protocol, Security, Stream, pipe};
use std::sync::{Arc, Mutex, MutexGuard};

/// A message held by the fake server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub uid: u32,
    pub flags: Vec<String>,
    pub data: Vec<u8>,
}

/// A folder of the fake server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mailbox {
    /// The name as the server knows it (modified UTF-7).
    pub name: String,
    pub attributes: Vec<String>,
    pub uid_validity: u32,
    pub uid_next: u32,
    pub messages: Vec<Message>,
}

impl Mailbox {
    pub fn new(name: &str, attributes: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            attributes: attributes.iter().map(|a| a.to_string()).collect(),
            uid_validity: 1,
            uid_next: 1,
            messages: Vec::new(),
        }
    }

    pub fn add(&mut self, data: &[u8], flags: &[&str]) -> u32 {
        let uid = self.uid_next;
        self.uid_next += 1;
        self.messages.push(Message { uid, flags: flags.iter().map(|f| f.to_string()).collect(), data: data.to_vec() });
        uid
    }
}

/// A message handed to the fake SMTP server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submitted {
    pub from: String,
    pub to: Vec<String>,
    /// As it came, dots taken off, CRLF line ends.
    pub data: Vec<u8>,
}

/// What the fake server holds and how it behaves.
#[derive(Clone, Debug)]
pub struct State {
    pub user: String,
    pub password: String,
    pub mailboxes: Vec<Mailbox>,
    pub submitted: Vec<Submitted>,
    /// IMAP capabilities beyond IMAP4rev1, STARTTLS and AUTH=PLAIN, which the server
    /// always offers unless they are taken out in `imap_without`.
    pub imap_caps: Vec<String>,
    pub imap_without: Vec<String>,
    /// SMTP extensions offered after EHLO.
    pub smtp_ext: Vec<String>,
    /// POP3 capabilities offered by CAPA.
    pub pop_caps: Vec<String>,
    /// Times a connection was made secure, and sign-ins taken.
    pub secured: usize,
    pub logins: usize,
    /// Every command line the server was sent, passwords and all: for the tests to look at.
    pub heard: Vec<String>,
    /// What the fake web server answers, by host and path: the whole answer, head and body.
    pub pages: Vec<(String, Vec<u8>)>,
}

impl Default for State {
    fn default() -> Self {
        let mut inbox = Mailbox::new("INBOX", &[]);
        inbox.uid_validity = 7;
        Self {
            user: "pat@example.test".into(),
            password: "not a real password".into(),
            mailboxes: vec![inbox, Mailbox::new("Sent", &["\\Sent"]), Mailbox::new("Drafts", &["\\Drafts"]), Mailbox::new("Trash", &["\\Trash"])],
            submitted: Vec::new(),
            imap_caps: vec!["SPECIAL-USE".into(), "UIDPLUS".into()],
            imap_without: Vec::new(),
            smtp_ext: vec!["PIPELINING".into(), "SIZE 10485760".into(), "8BITMIME".into(), "STARTTLS".into(), "AUTH PLAIN LOGIN".into()],
            pop_caps: vec!["USER".into(), "UIDL".into(), "STLS".into(), "SASL PLAIN".into()],
            secured: 0,
            logins: 0,
            heard: Vec::new(),
            pages: Vec::new(),
        }
    }
}

impl State {
    pub fn mailbox(&mut self, name: &str) -> Option<&mut Mailbox> {
        self.mailboxes.iter_mut().find(|m| m.name.eq_ignore_ascii_case(name) || (name.eq_ignore_ascii_case("INBOX") && m.name == "INBOX"))
    }
}

#[derive(Clone)]
enum Mode {
    Serve,
    Script(Vec<u8>),
}

/// The fake server, as a connector the clients use.
#[derive(Clone)]
pub struct Fake {
    state: Arc<Mutex<State>>,
    mode: Mode,
}

impl Default for Fake {
    fn default() -> Self {
        Self::new(State::default())
    }
}

impl Fake {
    pub fn new(state: State) -> Self {
        Self { state: Arc::new(Mutex::new(state)), mode: Mode::Serve }
    }

    /// A server that sends these bytes whatever it is told, hears what comes, and hangs up
    /// once nothing has come for a moment.
    pub fn script(bytes: &[u8]) -> Self {
        Self { state: Arc::new(Mutex::new(State::default())), mode: Mode::Script(bytes.to_vec()) }
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// An endpoint of this server for a protocol, secured with STARTTLS.
    pub fn endpoint(protocol: Protocol) -> Endpoint {
        Endpoint { protocol, host: "mail.example.test".into(), port: protocol.port(Security::StartTls), security: Security::StartTls }
    }
}

impl Connector for Fake {
    fn connect(&self, to: &Endpoint) -> Result<Box<dyn Stream>, NetError> {
        let (client, server) = pipe();
        let state = self.state.clone();
        let mode = self.mode.clone();
        let protocol = to.protocol;
        let tls = to.security == Security::Tls;
        let host = to.host.clone();
        std::thread::spawn(move || {
            let _ = match mode {
                Mode::Script(bytes) => script(server, &bytes),
                Mode::Serve => match protocol {
                    Protocol::Imap => imap::serve(server, &state, tls),
                    Protocol::Pop3 => pop3::serve(server, &state, tls),
                    Protocol::Smtp => smtp::serve(server, &state, tls),
                    Protocol::Https => http::serve(server, &state, &host),
                },
            };
        });
        Ok(Box::new(client))
    }

    /// The fake has no TLS: the same pipe goes on, and the server counts that it was asked.
    fn secure(&self, plain: Box<dyn Stream>, _host: &str) -> Result<Box<dyn Stream>, NetError> {
        Ok(plain)
    }
}

fn script(mut end: End, bytes: &[u8]) -> Result<(), NetError> {
    use std::io::{Read, Write};
    end.write_all(bytes)?;
    end.set_wait(std::time::Duration::from_millis(150));
    let mut sink = [0u8; 4096];
    while end.read(&mut sink)? > 0 {}
    Ok(())
}

/// Locks the state for one step of a session.
fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
