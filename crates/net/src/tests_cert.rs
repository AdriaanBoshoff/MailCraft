//! What is read of a certificate to show the owner, and fingerprints, against hostile bytes.

use crate::cert::{Fingerprint, about};

/// A certificate written by hand: a version, serial, signature algorithm, issuer and subject
/// names, validity and a subjectAltName with two server names and an address.
fn handmade() -> Vec<u8> {
    fn tlv(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        if body.len() < 0x80 {
            out.push(body.len() as u8);
        } else {
            out.push(0x82);
            out.extend_from_slice(&(body.len() as u16).to_be_bytes());
        }
        out.extend_from_slice(body);
        out
    }
    let attr = |oid: &[u8], tag: u8, text: &str| tlv(0x31, &tlv(0x30, &[tlv(0x06, oid), tlv(tag, text.as_bytes())].concat()));
    let issuer = tlv(0x30, &[attr(&[0x55, 0x04, 0x0A], 0x13, "Example Signer"), attr(&[0x55, 0x04, 0x03], 0x0C, "Example Root")].concat());
    let subject = tlv(0x30, &attr(&[0x55, 0x04, 0x03], 0x0C, "mail.example.test"));
    let validity = tlv(0x30, &[tlv(0x17, b"260101000000Z"), tlv(0x18, b"20360630000000Z")].concat());
    let names = tlv(0x30, &[tlv(0x82, b"mail.example.test"), tlv(0x82, b"imap.example.test"), tlv(0x87, &[192, 0, 2, 7])].concat());
    let san = tlv(0x30, &[tlv(0x06, &[0x55, 0x1D, 0x11]), tlv(0x01, &[0xFF]), tlv(0x04, &names)].concat());
    let extensions = tlv(0xA3, &tlv(0x30, &san));
    let alg = tlv(0x30, &tlv(0x06, &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02]));
    let spki = tlv(0x30, &[alg.clone(), tlv(0x03, &[0, 1, 2, 3])].concat());
    let tbs = tlv(0x30, &[tlv(0xA0, &tlv(0x02, &[2])), tlv(0x02, &[1, 2, 3]), alg.clone(), issuer, validity, subject, spki, extensions].concat());
    tlv(0x30, &[tbs, alg, tlv(0x03, &[0, 9, 9])].concat())
}

#[test]
fn a_certificate_is_read_for_what_the_owner_is_shown() {
    let a = about(&handmade());
    assert_eq!(a.subject, "mail.example.test");
    assert_eq!(a.issuer, "Example Root, Example Signer");
    assert_eq!(a.not_before, "2026-01-01");
    assert_eq!(a.not_after, "2036-06-30");
    assert_eq!(a.names, ["mail.example.test", "imap.example.test", "192.0.2.7"]);
    assert!(!a.own());
}

#[test]
fn hostile_certificates_are_read_as_far_as_they_go() {
    let good = handmade();
    // Every prefix, every byte changed, and lengths that run past the end.
    for n in 0..good.len() {
        let _ = about(&good[..n]);
        for v in [0x00, 0x7F, 0x80, 0x84, 0xFF] {
            let mut bad = good.clone();
            bad[n] = v;
            let _ = about(&bad);
        }
    }
    for junk in [&b""[..], b"\x30", b"\x30\x84\xFF\xFF\xFF\xFF", b"\x30\x85\x01\x01\x01\x01\x01", b"\x30\x80\x00\x00", b"\x1F\x01\x00"] {
        assert_eq!(about(junk), Default::default());
    }
    // Nesting deeper than a certificate has is not followed.
    let mut deep = Vec::new();
    for _ in 0..10_000 {
        deep = [vec![0x30, 0x82], (deep.len().min(0xFFFF) as u16).to_be_bytes().to_vec(), deep].concat();
        deep.truncate(60_000);
    }
    let _ = about(&deep);
    // Text of a name has no control characters.
    let mut odd = handmade();
    let at = odd.windows(17).position(|w| w == b"mail.example.test").unwrap();
    odd[at] = 0x07;
    let a = about(&odd);
    assert!(!a.subject.chars().any(char::is_control), "{:?}", a.subject);
}

#[test]
fn fingerprints_read_as_people_write_them() {
    let f = Fingerprint([0xAB; 32]);
    let shown = f.to_string();
    assert_eq!(shown.len(), 95);
    assert!(shown.starts_with("AB:AB:"));
    assert_eq!(Fingerprint::parse(&shown), Some(f));
    assert_eq!(Fingerprint::parse(&shown.to_lowercase().replace(':', " ")), Some(f));
    assert_eq!(Fingerprint::parse(&"ab".repeat(32)), Some(f));
    for bad in ["", "AB", &"AB".repeat(33), &"ZZ".repeat(32), &"é".repeat(64), &format!("{}A", "AB".repeat(31))] {
        assert_eq!(Fingerprint::parse(bad), None, "{bad:?}");
    }
}
