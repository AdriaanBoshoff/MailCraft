//! The clients against the in-process fake server, and against servers that break the rules.

use crate::fake::{Fake, State};
use crate::imap::{self, Imap, Resp, Tok};
use crate::pop3::Pop3;
use crate::smtp::{Smtp, stuffed};
use crate::{Endpoint, Login, NetError, Protocol, Secret, Security, decode_mailbox_name, encode_mailbox_name};

fn login() -> Login {
    let s = State::default();
    Login { user: s.user, password: Secret::new(s.password) }
}

fn tls(protocol: Protocol) -> Endpoint {
    Endpoint { security: Security::Tls, port: protocol.port(Security::Tls), ..Fake::endpoint(protocol) }
}

const MESSAGE: &[u8] =
    b"From: Pat <pat@example.test>\r\nTo: Sam <sam@example.test>\r\nSubject: Hello\r\n\r\nFirst line\r\n.starts with a dot\r\n.\r\nbare\nends\n";

#[test]
fn a_message_is_submitted_after_starttls_and_sign_in() {
    let fake = Fake::default();
    let mut s = Smtp::open(&fake, &Fake::endpoint(Protocol::Smtp), Some(&login())).unwrap();
    assert!(s.is_secure());
    let sent = s.send("pat@example.test", &["sam@example.test".into(), "kim@refused.test".into()], MESSAGE).unwrap();
    s.quit().unwrap();
    assert_eq!(sent.accepted, vec!["sam@example.test".to_string()]);
    assert_eq!(sent.refused.len(), 1);
    let state = fake.state();
    assert_eq!((state.secured, state.logins), (1, 1));
    let got = &state.submitted[0];
    assert_eq!(got.from, "pat@example.test");
    assert_eq!(got.to, vec!["sam@example.test".to_string()]);
    // Dots are put back as they were and every line ends in CRLF.
    let want = String::from_utf8_lossy(MESSAGE).replace("bare\nends\n", "bare\r\nends\r\n");
    assert_eq!(String::from_utf8_lossy(&got.data), want);
    // The size was said, and the password never went before TLS: STARTTLS came first.
    let heard = &state.heard;
    let at = |what: &str| heard.iter().position(|l| l.starts_with(what)).unwrap();
    assert!(at("STARTTLS") < at("AUTH PLAIN"));
    assert!(heard.iter().any(|l| l.starts_with("MAIL FROM:<pat@example.test> SIZE=")));
}

#[test]
fn no_password_goes_to_a_server_that_cannot_be_made_secure() {
    let mut state = State::default();
    state.smtp_ext.retain(|e| e != "STARTTLS");
    state.pop_caps.retain(|c| c != "STLS");
    state.imap_without.push("STARTTLS".into());
    let fake = Fake::new(state);
    assert!(matches!(Smtp::open(&fake, &Fake::endpoint(Protocol::Smtp), Some(&login())), Err(NetError::NotSecure)));
    assert!(matches!(Pop3::open(&fake, &Fake::endpoint(Protocol::Pop3), &login()), Err(NetError::NotSecure)));
    let plain = |p| Endpoint { security: Security::Plain, ..Fake::endpoint(p) };
    assert!(matches!(Pop3::open(&fake, &plain(Protocol::Pop3), &login()), Err(NetError::NotSecure)));
    assert!(matches!(Imap::open(&fake, &plain(Protocol::Imap), &login()), Err(NetError::NotSecure)));
    assert!(matches!(Smtp::open(&fake, &plain(Protocol::Smtp), Some(&login())), Err(NetError::NotSecure)));
    let state = fake.state();
    assert_eq!(state.logins, 0);
    assert!(!state.heard.iter().any(|l| l.contains(&state.password) || l.starts_with("AUTH") || l.contains("LOGIN")));
}

#[test]
fn smtp_signs_in_with_login_when_plain_is_not_offered_and_says_a_wrong_password() {
    let mut state = State::default();
    for e in &mut state.smtp_ext {
        if e.starts_with("AUTH") {
            *e = "AUTH LOGIN".into();
        }
    }
    let fake = Fake::new(state);
    // The fake takes only PLAIN, so LOGIN being tried shows in what it heard.
    let r = Smtp::open(&fake, &tls(Protocol::Smtp), Some(&login()));
    assert!(matches!(r, Err(NetError::Refused(_) | NetError::Auth(_))));
    assert!(fake.state().heard.iter().any(|l| l == "AUTH LOGIN"));
    let wrong = Login { password: Secret::new("wrong"), ..login() };
    assert!(matches!(Smtp::open(&Fake::default(), &tls(Protocol::Smtp), Some(&wrong)), Err(NetError::Auth(_))));
}

#[test]
fn an_address_cannot_carry_a_command_in_with_it() {
    let fake = Fake::default();
    let mut s = Smtp::open(&fake, &tls(Protocol::Smtp), Some(&login())).unwrap();
    for bad in ["sam@example.test>\r\nRCPT TO:<eve@example.test", "sam@example.test> NOTIFY=NEVER", "", "no-at-sign", "a<b@example.test"] {
        assert!(matches!(s.send("pat@example.test", &[bad.to_string()], MESSAGE), Err(NetError::Unsupported(_))), "{bad:?}");
        assert!(matches!(s.send(bad, &["sam@example.test".into()], MESSAGE), Err(NetError::Unsupported(_))), "{bad:?}");
    }
    let refused = s.send("pat@example.test", &["kim@refused.test".into()], MESSAGE);
    assert!(matches!(refused, Err(NetError::Refused(_))));
    assert!(fake.state().submitted.is_empty());
    assert!(!fake.state().heard.iter().any(|l| l == "DATA"));
}

#[test]
fn a_message_too_large_for_the_server_is_not_sent() {
    let fake = Fake::default();
    let mut s = Smtp::open(&fake, &tls(Protocol::Smtp), Some(&login())).unwrap();
    let big = vec![b'a'; 10_485_761];
    assert!(matches!(s.send("pat@example.test", &["sam@example.test".into()], &big), Err(NetError::Refused(_))));
    assert!(fake.state().submitted.is_empty());
}

#[test]
fn dot_stuffing_follows_rfc_5321() {
    assert_eq!(stuffed(b""), b".\r\n");
    assert_eq!(stuffed(b"a"), b"a\r\n.\r\n");
    assert_eq!(stuffed(b".\r\n..x\r\n"), b"..\r\n...x\r\n.\r\n");
    assert_eq!(stuffed(b"a\rb\n.c"), b"a\r\nb\r\n..c\r\n.\r\n");
}

fn with_inbox(messages: &[&[u8]]) -> Fake {
    let mut state = State::default();
    if let Some(inbox) = state.mailbox("INBOX") {
        for m in messages {
            inbox.add(m, &[]);
        }
    }
    Fake::new(state)
}

#[test]
fn pop3_lists_retrieves_and_deletes() {
    let fake = with_inbox(&[MESSAGE, b"Subject: two\r\n\r\nx\r\n"]);
    let mut p = Pop3::open(&fake, &Fake::endpoint(Protocol::Pop3), &login()).unwrap();
    let listed = p.list().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].uid, "uid-1");
    assert_eq!(listed[0].size, MESSAGE.len() as u64);
    let got = p.retrieve(1).unwrap();
    assert_eq!(String::from_utf8_lossy(&got), String::from_utf8_lossy(MESSAGE).replace("bare\nends\n", "bare\r\nends\r\n"));
    p.delete(1).unwrap();
    assert!(p.retrieve(1).is_err());
    p.quit().unwrap();
    let mut state = fake.state();
    assert_eq!(state.mailbox("INBOX").unwrap().messages.len(), 1);
    assert_eq!(state.secured, 1);
    // Signed in with SASL PLAIN, which the server offered: no password on a line of its own.
    assert!(state.heard.iter().any(|l| l.starts_with("AUTH PLAIN ")));
    assert!(!state.heard.iter().any(|l| l.starts_with("PASS")));
}

#[test]
fn pop3_signs_in_with_user_and_pass_without_sasl() {
    let mut state = State::default();
    state.pop_caps.retain(|c| !c.starts_with("SASL"));
    let fake = Fake::new(state);
    let mut p = Pop3::open(&fake, &tls(Protocol::Pop3), &login()).unwrap();
    assert!(p.list().unwrap().is_empty());
    p.quit().unwrap();
    assert!(fake.state().heard.iter().any(|l| l.starts_with("PASS ")));
    let wrong = Login { password: Secret::new("wrong"), ..login() };
    assert!(matches!(Pop3::open(&fake, &tls(Protocol::Pop3), &wrong), Err(NetError::Auth(_))));
}

#[test]
fn imap_lists_folders_reads_flags_and_appends() {
    let mut state = State::default();
    let mut drafts = crate::fake::Mailbox::new("Entw&APw-rfe", &["\\HasNoChildren"]);
    drafts.uid_validity = 9;
    state.mailboxes.push(drafts);
    if let Some(inbox) = state.mailbox("INBOX") {
        inbox.add(MESSAGE, &["\\Seen"]);
        inbox.add(b"Subject: (two) {5}\r\n\r\n\"quoted\" )(\r\n", &[]);
    }
    let fake = Fake::new(state);
    let mut i = Imap::open(&fake, &Fake::endpoint(Protocol::Imap), &login()).unwrap();
    assert!(i.has("UIDPLUS"));
    // Signing in was offered only before it: capabilities are asked again afterwards.
    assert!(!i.has("AUTH=PLAIN"));
    let folders = i.folders().unwrap();
    let names: Vec<&str> = folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Inbox", "Sent", "Drafts", "Trash", "Entwürfe"]);
    assert!(folders[1].has("\\Sent"));
    assert_eq!(folders[4].raw, "Entw&APw-rfe");
    let sel = i.select("INBOX").unwrap();
    assert_eq!((sel.exists, sel.uid_validity, sel.uid_next, sel.read_only), (2, Some(7), Some(3), false));
    let list = i.list(1).unwrap();
    assert_eq!(list.iter().map(|s| (s.uid, s.size)).collect::<Vec<_>>(), [(1, MESSAGE.len() as u64), (2, 35)]);
    assert_eq!(list[0].flags, ["\\Seen"]);
    // Nothing new after the last: the last is not taken for new.
    assert!(i.list(3).unwrap().is_empty());
    assert_eq!(i.fetch(2).unwrap().unwrap(), b"Subject: (two) {5}\r\n\r\n\"quoted\" )(\r\n");
    assert_eq!(i.fetch(1).unwrap().unwrap(), MESSAGE);
    assert_eq!(i.fetch(99).unwrap(), None);
    i.flag(2, true, &["\\Seen", "\\Flagged"]).unwrap();
    i.flag(1, false, &["\\Seen"]).unwrap();
    assert!(i.flag(1, true, &["\\Seen) (x"]).is_err());
    let list = i.list(1).unwrap();
    assert_eq!((list[0].flags.len(), list[1].flags.len()), (0, 2));
    // An append waits to be told to go on, then sends the message as a literal.
    assert_eq!(i.append("Sent", &["\\Seen"], MESSAGE).unwrap(), Some(1));
    assert!(matches!(i.select("Nowhere"), Err(NetError::Refused(_))));
    i.logout().unwrap();
    let mut state = fake.state();
    assert_eq!(state.mailbox("Sent").unwrap().messages[0].data, MESSAGE);
    assert_eq!(state.mailbox("Sent").unwrap().messages[0].flags, ["\\Seen"]);
    assert_eq!(state.secured, 1);
    assert!(state.heard.iter().any(|l| l.contains("BODY.PEEK[]")));
}

#[test]
fn imap_uses_literal_plus_and_login_when_offered() {
    let mut state = State::default();
    state.imap_caps.push("LITERAL+".into());
    state.imap_without.push("AUTH=PLAIN".into());
    state.password = "pässword \"with\" quotes\\".into();
    let user = Login { user: state.user.clone(), password: Secret::new(state.password.clone()) };
    let fake = Fake::new(state);
    let mut i = Imap::open(&fake, &tls(Protocol::Imap), &user).unwrap();
    assert_eq!(i.append("INBOX", &[], b"Subject: x\r\n\r\ny\r\n").unwrap(), Some(1));
    i.logout().unwrap();
    let state = fake.state();
    assert!(state.heard.iter().any(|l| l.contains(" LOGIN ")));
    assert!(state.heard.iter().any(|l| l.contains("{17+}")));
    assert_eq!(state.secured, 0);
}

#[test]
fn imap_refuses_a_session_signed_in_before_it_was_made_secure() {
    let fake = Fake::script(b"* PREAUTH welcome\r\n");
    assert!(matches!(Imap::open(&fake, &Fake::endpoint(Protocol::Imap), &login()), Err(NetError::NotSecure)));
}

#[test]
fn what_was_said_in_the_clear_after_starttls_is_refused() {
    // Someone between us and the server adds an answer behind the server's go-ahead.
    let fake = Fake::script(b"220 hi\r\n250-x\r\n250 STARTTLS\r\n220 go\r\n250 injected\r\n");
    let r = Smtp::open(&fake, &Fake::endpoint(Protocol::Smtp), Some(&login()));
    assert!(matches!(r, Err(NetError::Protocol(ref w)) if w.contains("before the connection was made secure")), "{:?}", r.err());
    let fake = Fake::script(b"* OK [CAPABILITY IMAP4rev1 STARTTLS] hi\r\nA0001 OK go\r\n* OK injected\r\n");
    assert!(matches!(Imap::open(&fake, &Fake::endpoint(Protocol::Imap), &login()), Err(NetError::Protocol(_))));
}

#[test]
fn mailbox_names_in_modified_utf7() {
    for (raw, name) in [
        ("INBOX", "INBOX"),
        ("Entw&APw-rfe", "Entwürfe"),
        ("&-", "&"),
        ("~peter/mail/&U,BTFw-/&ZeVnLIqe-", "~peter/mail/台北/日本語"),
        ("a&2D3eAA-b", "a😀b"),
    ] {
        assert_eq!(decode_mailbox_name(raw), name);
        assert_eq!(encode_mailbox_name(name), raw);
    }
    // What does not decode is kept as it came.
    for raw in ["&", "&%%-", "a&AAA-", "&2D3-"] {
        let _ = decode_mailbox_name(raw);
    }
    assert_eq!(decode_mailbox_name("&"), "&");
}

#[test]
fn a_password_never_prints() {
    let l = login();
    assert!(!format!("{l:?}").contains(l.password.expose()));
    assert_eq!(format!("{:?}", Secret::new("x")), "Secret(..)");
}

#[test]
fn imap_responses_parse() {
    let r = imap::parse(b"* 12 FETCH (UID 4 FLAGS (\\Seen $Junk) BODY[HEADER.FIELDS (SUBJECT)] {5}\r\nhello RFC822.SIZE 99)").unwrap();
    let Resp::Data { number, name, rest } = r else { panic!() };
    assert_eq!((number, name.as_str()), (Some(12), "FETCH"));
    let Tok::List(items) = &rest[0] else { panic!() };
    assert_eq!(items[4], Tok::Atom("BODY[HEADER.FIELDS (SUBJECT)]".into()));
    assert_eq!(items[5], Tok::Str(b"hello".to_vec()));
    let r = imap::parse(b"A0001 OK [APPENDUID 3 9] done [really]").unwrap();
    assert_eq!(r, Resp::Status { tag: Some("A0001".into()), kind: imap::Kind::Ok, code: Some("APPENDUID 3 9".into()), text: "done [really]".into() });
    let r = imap::parse(b"* LIST (\\HasNoChildren) NIL \"a \\\"b\\\"\"").unwrap();
    let Resp::Data { rest, .. } = r else { panic!() };
    assert_eq!(rest[1], Tok::Nil);
    assert_eq!(rest[2], Tok::Str(b"a \"b\"".to_vec()));
}

/// Bytes that look like a server's, made from a seed.
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let parts: [&[u8]; 24] = [
        b"* ",
        b"OK ",
        b"A0001 ",
        b"+ ",
        b"(",
        b")",
        b"{",
        b"}",
        b"\r\n",
        b"\"",
        b"\\",
        b"[",
        b"]",
        b"250-",
        b"250 ",
        b"220 ",
        b"+OK",
        b"-ERR",
        b".\r\n",
        b"{999999999999}\r\n",
        b"FETCH ",
        b"UID ",
        b"BODY[] ",
        b"\xff\xfe",
    ];
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut out = Vec::new();
    while out.len() < len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.extend_from_slice(parts[(x % 24) as usize]);
        if x.is_multiple_of(5) {
            out.push((x >> 8) as u8);
        }
    }
    out
}

#[test]
fn hostile_servers_are_errors_never_panics() {
    let long_line = vec![b'a'; crate::conn::MAX_LINE + 10];
    let deep = [b"* 1 FETCH ".as_slice(), &[b'('; 200], b"\r\n"].concat();
    let many_lines: Vec<u8> = b"220 hi\r\n".iter().copied().chain(std::iter::repeat_n(b"250-x\r\n".iter().copied(), 1_100).flatten()).collect();
    let scripts: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"garbage\r\n".to_vec(),
        long_line,
        b"* OK hi\r\n* 1 FETCH (BODY[] {99999999999999999}\r\n".to_vec(),
        b"* OK hi\r\n* 1 FETCH (BODY[] {18446744073709551615}\r\n".to_vec(),
        [b"* OK hi\r\n".as_slice(), &deep].concat(),
        b"* OK hi\r\nA0001 OK x\r\n* LIST (\"unclosed\r\n".to_vec(),
        many_lines,
        b"+OK hi\r\n+OK\r\nSASL PLAIN\r\nSTLS\r\n.\r\n+OK\r\n".to_vec(),
        b"+OK\r\n+OK\r\n.\r\n+OK\r\n+OK\r\nnot a number uid\r\n.\r\n".to_vec(),
        b"999 nope\r\n".to_vec(),
    ];
    let ends = [Protocol::Imap, Protocol::Pop3, Protocol::Smtp].map(tls);
    let run = |bytes: &[u8]| {
        for to in &ends {
            let fake = Fake::script(bytes);
            match to.protocol {
                Protocol::Imap => {
                    if let Ok(mut i) = Imap::open(&fake, to, &login()) {
                        let _ = i.folders();
                        let _ = i.select("INBOX");
                        let _ = i.list(1);
                        let _ = i.fetch(1);
                    }
                }
                Protocol::Pop3 => {
                    if let Ok(mut p) = Pop3::open(&fake, to, &login()) {
                        let _ = p.list();
                        let _ = p.retrieve(1);
                    }
                }
                Protocol::Smtp => {
                    if let Ok(mut s) = Smtp::open(&fake, to, Some(&login())) {
                        let _ = s.send("a@b.test", &["c@d.test".into()], b"x");
                    }
                }
            }
        }
    };
    for s in &scripts {
        run(s);
    }
    for seed in 0..150 {
        run(&noise(seed, 64 + (seed as usize % 7) * 300));
    }
    for seed in 0..300 {
        let _ = imap::parse(&noise(seed, 200));
    }
}
