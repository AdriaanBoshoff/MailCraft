//! An account's servers found from its address, against the fake server: nothing leaves the
//! process, and the addresses are of domains reserved for examples.

use crate::fake::{Fake, State};
use crate::find::{Found, Server, Source, find, known, parts, published};
use crate::{Protocol, Security};

fn page(body: &str) -> Vec<u8> {
    format!("HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

fn fake(pages: &[(&str, Vec<u8>)], answers_only: Option<&[&str]>) -> Fake {
    Fake::new(State {
        pages: pages.iter().map(|(at, answer)| (at.to_string(), answer.clone())).collect(),
        answers_only: answers_only.map(|only| only.iter().map(|s| s.to_string()).collect()),
        ..State::default()
    })
}

fn server(host: &str, port: u16, security: Security) -> Server {
    Server { host: host.into(), port, security }
}

const SETTINGS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<clientConfig version="1.1">
  <emailProvider id="bluebird.example">
    <domain>bluebird.example</domain>
    <incomingServer type="pop3">
      <hostname>pop.bluebird.example</hostname><port>995</port><socketType>SSL</socketType>
      <username>%EMAILADDRESS%</username>
    </incomingServer>
    <incomingServer type="imap">
      <hostname>plain.bluebird.example</hostname><port>143</port><socketType>plain</socketType>
      <username>%EMAILADDRESS%</username>
    </incomingServer>
    <incomingServer type="imap">
      <hostname>IMAP.Bluebird.Example</hostname><port>993</port><socketType>SSL</socketType>
      <username>%EMAILLOCALPART%</username>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>send.%EMAILDOMAIN%</hostname><port>587</port><socketType>STARTTLS</socketType>
      <username>%EMAILADDRESS%</username>
    </outgoingServer>
  </emailProvider>
</clientConfig>"#;

#[test]
fn a_large_providers_servers_are_known_without_asking_anyone() {
    let f = fake(&[], Some(&[]));
    let found = find(&f, " Sam@GMail.com ", &|| false).unwrap();
    assert_eq!(found.from, Source::Known);
    assert_eq!(found.protocol, Protocol::Imap);
    assert_eq!(found.incoming, server("imap.gmail.com", 993, Security::Tls));
    assert_eq!(found.outgoing, server("smtp.gmail.com", 465, Security::Tls));
    assert_eq!(found.user, "Sam@GMail.com");
    assert!(found.note.is_some_and(|n| n.contains("app password")));
    assert!(known("sam@outlook.com").is_some_and(|f| f.outgoing.security == Security::StartTls));
    assert!(known("sam@bluebird.example").is_none());
    assert!(f.state().heard.is_empty(), "nothing is asked of anyone for a known provider");
}

#[test]
fn the_settings_a_domain_publishes_are_read_imap_first_and_secure_only() {
    let f = fake(&[("autoconfig.bluebird.example/mail/config-v1.1.xml", page(SETTINGS))], Some(&[]));
    let found = find(&f, "sam.lee@bluebird.example", &|| false).unwrap();
    assert_eq!(
        found,
        Found {
            protocol: Protocol::Imap,
            incoming: server("imap.bluebird.example", 993, Security::Tls),
            outgoing: server("send.bluebird.example", 587, Security::StartTls),
            user: "sam.lee".into(),
            from: Source::Published,
            note: None,
        }
    );
    let heard = f.state().heard.clone();
    assert_eq!(heard.len(), 1);
    assert!(!heard.iter().any(|h| h.contains("sam")), "the address is never sent: {heard:?}");
}

#[test]
fn settings_under_the_domains_well_known_place_are_read_when_there_are_none_at_autoconfig() {
    let only_pop = SETTINGS.replace("type=\"imap\"", "type=\"x\"");
    let f = fake(&[("bluebird.example/.well-known/autoconfig/mail/config-v1.1.xml", page(&only_pop))], Some(&[]));
    let found = find(&f, "sam@bluebird.example", &|| false).unwrap();
    assert_eq!(found.protocol, Protocol::Pop3);
    assert_eq!(found.incoming, server("pop.bluebird.example", 995, Security::Tls));
    assert_eq!(found.user, "sam@bluebird.example");
    assert_eq!(f.state().heard.len(), 2);
}

#[test]
fn the_usual_servers_are_greeted_and_none_is_signed_in_to() {
    let f = fake(&[], Some(&["mail.bluebird.example:993", "smtp.bluebird.example:587"]));
    let found = find(&f, "sam@bluebird.example", &|| false).unwrap();
    assert_eq!(found.from, Source::Tried);
    assert_eq!(found.incoming, server("mail.bluebird.example", 993, Security::Tls));
    assert_eq!(found.outgoing, server("smtp.bluebird.example", 587, Security::StartTls));
    let state = f.state();
    assert_eq!(state.logins, 0);
    assert!(!state.heard.iter().any(|h| h.contains("LOGIN") || h.contains("AUTH")), "{:?}", state.heard);
}

#[test]
fn a_domain_with_no_settings_and_no_servers_that_answer_says_so() {
    let f = fake(&[], Some(&["imap.bluebird.example:993"]));
    let e = find(&f, "sam@bluebird.example", &|| false).unwrap_err().to_string();
    assert!(e.contains("sending"), "{e}");
    let f = fake(&[], Some(&[]));
    let e = find(&f, "sam@bluebird.example", &|| false).unwrap_err().to_string();
    assert!(e.contains("no settings were found for bluebird.example"), "{e}");
}

#[test]
fn finding_stops_when_asked() {
    let f = fake(&[], None);
    let e = find(&f, "sam@bluebird.example", &|| true).unwrap_err().to_string();
    assert!(e.contains("stopped"), "{e}");
    assert!(f.state().heard.is_empty());
}

#[test]
fn what_is_not_an_address_finds_nothing() {
    let f = fake(&[], None);
    for bad in [
        "",
        "sam",
        "@bluebird.example",
        "sam@",
        "sam@localhost",
        "sam@blue bird.example",
        "sam@-x.example",
        "sam@x..example",
        "sam@bluebird.example/x",
    ] {
        assert!(parts(bad).is_none(), "{bad}");
        assert!(find(&f, bad, &|| false).is_err(), "{bad}");
    }
    assert!(parts(&format!("{}@bluebird.example", "a".repeat(65))).is_none());
    assert!(f.state().heard.is_empty());
}

#[test]
fn hostile_settings_are_passed_over() {
    let email = "sam@bluebird.example";
    let swap = |from: &str, to: &str| SETTINGS.replace(from, to);
    let bad = [
        String::new(),
        "<clientConfig".into(),
        "not xml at all \u{0} \u{ffff}".into(),
        SETTINGS[..SETTINGS.len() / 2].to_string(),
        // Entities, which could be made to grow without end, are not read.
        r#"<!DOCTYPE c [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;">]><clientConfig>&b;</clientConfig>"#.to_string(),
        swap("<outgoingServer type=\"smtp\">", "<outgoingServer type=\"x\">"),
        swap("<port>587</port>", "<port>99999</port>"),
        swap("<port>587</port>", "<port>0</port>"),
        swap("<port>587</port>", "<port>-1</port>"),
        swap("send.%EMAILDOMAIN%", "send.bluebird.example\r\nRCPT TO:x"),
        swap("send.%EMAILDOMAIN%", "evil host"),
        swap("send.%EMAILDOMAIN%", ""),
        swap("STARTTLS", "plain"),
        format!("{SETTINGS}{}", " ".repeat(70 * 1024)),
        format!("<a>{}</a>", "<b/>".repeat(10_000)),
        "<a>".repeat(5_000),
        // Nesting hidden from a count of tags, in comments and quoted values.
        "<a><!--</x>-->".repeat(5_000),
        "<a x='/>'>".repeat(5_000),
        "<a x=\"</b>\">".repeat(5_000),
    ];
    for xml in &bad {
        assert!(published(xml.as_bytes(), email).is_none(), "{:?}", xml.chars().take(80).collect::<String>());
    }
    assert!(published(&[0xff, 0xfe, 0x00], email).is_none());
    // A user name that is not one is passed over for the address.
    let odd = published(swap("%EMAILLOCALPART%", "sam\r\nA1 LOGIN x").as_bytes(), email).unwrap();
    assert_eq!(odd.user, email);
    // A settings page that is not one leaves finding to go on to the usual servers.
    let f = fake(
        &[("autoconfig.bluebird.example/mail/config-v1.1.xml", page(&bad[4]))],
        Some(&["imap.bluebird.example:993", "smtp.bluebird.example:465"]),
    );
    assert_eq!(find(&f, email, &|| false).unwrap().from, Source::Tried);
}

fn with_records(records: &[(&str, crate::dns::Record)], answers_only: &[&str]) -> Fake {
    Fake::new(State {
        records: records.iter().map(|(at, r)| (at.to_string(), r.clone())).collect(),
        answers_only: Some(answers_only.iter().map(|s| s.to_string()).collect()),
        ..State::default()
    })
}

fn srv(priority: u16, weight: u16, port: u16, target: &str) -> crate::dns::Record {
    crate::dns::Record::Srv { priority, weight, port, target: target.into() }
}

fn mx(preference: u16, exchange: &str) -> crate::dns::Record {
    crate::dns::Record::Mx { preference, exchange: exchange.into() }
}

#[test]
fn the_servers_a_domains_srv_records_name_are_found_first_by_priority_then_weight() {
    let f = with_records(
        &[
            ("_imaps._tcp.bluebird.example", srv(10, 0, 993, "slow.bluebird.example")),
            ("_imaps._tcp.bluebird.example", srv(0, 1, 993, "light.bluebird.example")),
            ("_imaps._tcp.bluebird.example", srv(0, 9, 9993, "heavy.bluebird.example")),
            ("_submission._tcp.bluebird.example", srv(0, 0, 587, "out.bluebird.example")),
            // An MX that would name Google is not asked for: the records come first.
            ("bluebird.example", mx(1, "aspmx.l.google.com")),
        ],
        &[],
    );
    let found = find(&f, "sam@bluebird.example", &|| false).unwrap();
    assert_eq!(found.from, Source::Records);
    assert_eq!(found.protocol, Protocol::Imap);
    assert_eq!(found.incoming, server("heavy.bluebird.example", 9993, Security::Tls));
    assert_eq!(found.outgoing, server("out.bluebird.example", 587, Security::StartTls));
    assert!(found.note.is_some_and(|n| n.contains("check")));
    let heard = f.state().heard.clone();
    assert!(heard.contains(&"DNS Srv _submissions._tcp.bluebird.example".to_string()), "{heard:?}");
    assert!(!heard.iter().any(|h| h.contains("DNS Mx") || h.contains("sam")), "{heard:?}");
}

#[test]
fn a_domain_whose_mail_goes_to_a_known_provider_is_given_that_providers_servers() {
    let f = with_records(
        &[("bluebird.example", mx(20, "alt1.aspmx.l.google.com")), ("bluebird.example", mx(5, "bluebird-example.mail.protection.outlook.com"))],
        &[],
    );
    let found = find(&f, "sam@bluebird.example", &|| false).unwrap();
    assert_eq!(found.from, Source::Exchanger);
    assert_eq!(found.incoming, server("outlook.office365.com", 993, Security::Tls));
    assert_eq!(found.outgoing, server("smtp.office365.com", 587, Security::StartTls));
    assert_eq!(found.user, "sam@bluebird.example");
    assert!(found.note.is_some_and(|n| n.contains("Microsoft 365")));
    // A name that only looks like a provider's is not one.
    for odd in ["evilgoogle.com", "google.com.evil.example", "outlook.com-x.example"] {
        assert!(crate::find::exchanger(&[mx(1, odd)], "sam@bluebird.example").is_none(), "{odd}");
    }
}

#[test]
fn hostile_srv_records_are_passed_over_for_the_usual_servers() {
    let f = with_records(
        &[
            // "." says the service is not offered; the others are no servers at all.
            ("_imaps._tcp.bluebird.example", srv(0, 0, 0, "")),
            ("_imap._tcp.bluebird.example", srv(0, 0, 0, "imap.bluebird.example")),
            ("_pop3s._tcp.bluebird.example", srv(0, 0, 995, "evil host")),
            ("_pop3._tcp.bluebird.example", srv(0, 0, 110, "-x.example")),
            ("_submissions._tcp.bluebird.example", srv(0, 0, 465, "smtp.bluebird.example")),
        ],
        &["imap.bluebird.example:993", "smtp.bluebird.example:465"],
    );
    assert_eq!(find(&f, "sam@bluebird.example", &|| false).unwrap().from, Source::Tried);
    assert!(crate::find::first_named(&[srv(0, 0, 0, "")]).is_none());
    assert!(crate::find::first_named(&[mx(0, "mx.bluebird.example")]).is_none());
}
