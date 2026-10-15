//! The TCP and TLS connector against a server on this machine's loopback with a certificate
//! made for the test, and the password store kept in memory. Nothing here leaves this machine.

use crate::smtp::Smtp;
use crate::{Endpoint, Login, MemoryVault, NetError, Protocol, Secret, Security, TcpConnector, Vault};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

fn cert(name: &str) -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
    let made = rcgen::generate_simple_self_signed(vec![name.to_string()]).unwrap();
    (made.cert.der().clone(), PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(made.key_pair.serialize_der())))
}

/// One line, read a byte at a time so nothing after it (a TLS hello) is taken with it.
fn line(r: &mut impl Read) -> Option<String> {
    let mut out = Vec::new();
    let mut b = [0u8; 1];
    loop {
        if r.read(&mut b).ok()? == 0 {
            return None;
        }
        if b[0] == b'\n' {
            return Some(String::from_utf8_lossy(&out).trim_end_matches('\r').to_string());
        }
        out.push(b[0]);
    }
}

/// A server that takes one connection and speaks a little SMTP: TLS from the start, or after
/// STARTTLS. It gives back each line it heard, marked with whether it came over TLS.
fn serve(cert: CertificateDer<'static>, key: PrivateKeyDer<'static>, starttls: bool) -> (u16, JoinHandle<Vec<(bool, String)>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    let handle = thread::spawn(move || {
        let mut heard = Vec::new();
        let Ok((mut tcp, _)) = listener.accept() else { return heard };
        let _ = tcp.set_read_timeout(Some(Duration::from_secs(10)));
        if starttls {
            let _ = tcp.write_all(b"220 hi\r\n");
            for answer in [&b"250-localhost\r\n250 STARTTLS\r\n"[..], b"220 go ahead\r\n"] {
                let Some(l) = line(&mut tcp) else { return heard };
                heard.push((false, l));
                let _ = tcp.write_all(answer);
            }
        }
        let Ok(conn) = ServerConnection::new(Arc::new(config)) else { return heard };
        let mut tls = StreamOwned::new(conn, tcp);
        if !starttls && tls.write_all(b"220 hi\r\n").is_err() {
            return heard;
        }
        while let Some(l) = line(&mut tls) {
            let answer: &[u8] = match l.split(' ').next().unwrap_or_default() {
                "EHLO" => b"250-localhost\r\n250 AUTH PLAIN\r\n",
                "AUTH" => b"235 welcome\r\n",
                "QUIT" => b"221 bye\r\n",
                _ => b"500 what\r\n",
            };
            heard.push((true, l));
            if tls.write_all(answer).is_err() || answer.starts_with(b"221") {
                break;
            }
        }
        heard
    });
    (port, handle)
}

fn local(port: u16, security: Security) -> Endpoint {
    Endpoint { protocol: Protocol::Smtp, host: "localhost".into(), port, security, trusted: None }
}

fn login() -> Login {
    Login { user: "pat@example.test".into(), password: Secret::new("not a real password") }
}

fn trusting(cert: &CertificateDer<'static>) -> TcpConnector {
    TcpConnector::trusting(std::slice::from_ref(cert)).unwrap().waiting(Duration::from_secs(10))
}

#[test]
fn smtp_signs_in_over_tls_from_the_first_byte() {
    let (c, k) = cert("localhost");
    let (port, heard) = serve(c.clone(), k, false);
    let s = Smtp::open(&trusting(&c), &local(port, Security::Tls), Some(&login())).unwrap();
    assert!(s.is_secure());
    s.quit().unwrap();
    let heard = heard.join().unwrap();
    let said: Vec<&str> = heard.iter().map(|(_, l)| l.split(' ').next().unwrap_or_default()).collect();
    assert_eq!(said, ["EHLO", "AUTH", "QUIT"]);
    assert!(heard.iter().all(|(tls, _)| *tls));
}

#[test]
fn smtp_signs_in_only_after_starttls_took_effect() {
    let (c, k) = cert("localhost");
    let (port, heard) = serve(c.clone(), k, true);
    let s = Smtp::open(&trusting(&c), &local(port, Security::StartTls), Some(&login())).unwrap();
    s.quit().unwrap();
    let heard = heard.join().unwrap();
    let said: Vec<(bool, &str)> = heard.iter().map(|(tls, l)| (*tls, l.split(' ').next().unwrap_or_default())).collect();
    assert_eq!(said, [(false, "EHLO"), (false, "STARTTLS"), (true, "EHLO"), (true, "AUTH"), (true, "QUIT")]);
}

#[test]
fn a_certificate_not_trusted_or_for_another_name_is_refused_before_anything_is_said() {
    // Signed by nobody we trust.
    let (c, k) = cert("localhost");
    let (other, _) = cert("localhost");
    let (port, heard) = serve(c, k, false);
    let got = Smtp::open(&trusting(&other), &local(port, Security::Tls), Some(&login()));
    assert!(matches!(got, Err(NetError::Untrusted(_))), "{:?}", got.err());
    assert!(heard.join().unwrap().is_empty());
    // Trusted, but made for another server.
    let (c, k) = cert("mail.example.test");
    let (port, heard) = serve(c.clone(), k, true);
    let got = Smtp::open(&trusting(&c), &local(port, Security::StartTls), Some(&login()));
    let Err(NetError::Untrusted(seen)) = got else { panic!("{:?}", got.err()) };
    assert!(seen.reason.contains("another server name"), "{}", seen.reason);
    assert!(heard.join().unwrap().iter().all(|(tls, l)| !tls && !l.starts_with("AUTH")));
}

/// A server's own certificate, refused and shown, then trusted on the owner's word: for that
/// server only, and only that certificate.
#[test]
fn a_certificate_the_owner_trusted_is_taken_for_its_server_only() {
    let (c, k) = cert("localhost");
    let nobody = trusting(&cert("someone.else.test").0);
    let (port, heard) = serve(c.clone(), k.clone_key(), false);
    let got = Smtp::open(&nobody, &local(port, Security::Tls), Some(&login()));
    let Err(NetError::Untrusted(seen)) = got else { panic!("{:?}", got.err()) };
    assert!(heard.join().unwrap().is_empty());
    assert_eq!(seen.fingerprint, crate::tcp::fingerprint(c.as_ref()));
    assert_eq!(seen.host, "localhost");
    assert!(seen.trustable && !seen.changed);
    // rcgen names every issuer alike, so the stranger trusted here is taken for its signer.
    assert!(seen.reason.contains("not signed by anyone") || seen.reason.contains("signature"), "{}", seen.reason);
    assert_eq!(seen.about.names, ["localhost"]);
    assert!(seen.about.own(), "{:?}", seen.about);
    assert!(seen.about.not_after.len() == 10 && seen.about.not_before.len() == 10, "{:?}", seen.about);
    // Trusted: it signs in, from the first byte and after STARTTLS.
    for starttls in [false, true] {
        let (port, heard) = serve(c.clone(), k.clone_key(), starttls);
        let security = if starttls { Security::StartTls } else { Security::Tls };
        let to = Endpoint { trusted: Some(seen.fingerprint), ..local(port, security) };
        let s = Smtp::open(&nobody, &to, Some(&login())).unwrap();
        s.quit().unwrap();
        assert!(heard.join().unwrap().iter().any(|(tls, l)| *tls && l.starts_with("AUTH")));
    }
    // Another certificate for the same server is not the one trusted, and is said to have changed.
    let (other, other_key) = cert("localhost");
    let (port, heard) = serve(other.clone(), other_key, false);
    let to = Endpoint { trusted: Some(seen.fingerprint), ..local(port, Security::Tls) };
    let got = Smtp::open(&nobody, &to, Some(&login()));
    let Err(NetError::Untrusted(again)) = got else { panic!("{:?}", got.err()) };
    assert!(again.changed);
    assert_eq!(again.fingerprint, crate::tcp::fingerprint(other.as_ref()));
    assert!(heard.join().unwrap().is_empty());
    // Trusted though made for another name: the owner trusted this certificate for this server.
    let (named, named_key) = cert("mail.example.test");
    let (port, heard) = serve(named.clone(), named_key, false);
    let to = Endpoint { trusted: Some(crate::tcp::fingerprint(named.as_ref())), ..local(port, Security::Tls) };
    Smtp::open(&nobody, &to, Some(&login())).unwrap().quit().unwrap();
    assert!(!heard.join().unwrap().is_empty());
}

#[test]
fn servers_that_are_not_there_are_errors() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let c = TcpConnector::trusting(&[cert("someone.else.test").0]).unwrap().waiting(Duration::from_secs(2));
    assert!(Smtp::open(&c, &Endpoint { host: "127.0.0.1".into(), ..local(port, Security::Tls) }, None).is_err());
    for host in ["", "bad\r\nname", "no such host.invalid"] {
        assert!(Smtp::open(&c, &Endpoint { host: host.into(), ..local(25, Security::Tls) }, None).is_err(), "{host:?}");
    }
    assert!(Smtp::open(&c, &local(0, Security::Tls), None).is_err());
}

#[test]
fn passwords_are_kept_by_account_and_forgotten() {
    let v = MemoryVault::default();
    assert!(v.get("work").unwrap().is_none());
    v.set("work", &Secret::new("one")).unwrap();
    v.set("work", &Secret::new("two")).unwrap();
    v.set("home", &Secret::new("three")).unwrap();
    assert_eq!(v.get("work").unwrap(), Some(Secret::new("two")));
    v.forget("work").unwrap();
    v.forget("work").unwrap();
    assert!(v.get("work").unwrap().is_none());
    assert_eq!(v.get("home").unwrap(), Some(Secret::new("three")));
    let long = "x".repeat(201);
    for bad in ["", "a\nb", long.as_str()] {
        assert!(v.set(bad, &Secret::new("p")).is_err());
        assert!(v.get(bad).is_err());
    }
}
