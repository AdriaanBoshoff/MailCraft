//! A server's certificate as the owner is shown it before trusting it: whom it was made for,
//! who signed it, from when to when it holds and the server names it covers, read from its DER
//! ([RFC 5280] 4.1, [X.690] 8 and 10). Only what is shown is read; the certificate is checked
//! by rustls, never here. A certificate comes from the server and is hostile: every length is
//! checked against what is left, and what cannot be read is shown as unknown.

/// The SHA-256 of a certificate's DER: what the owner trusts, and what a server's certificate
/// is compared with.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    /// As people compare them: pairs of capital hex digits split by colons. Spaces, colons and
    /// small letters are taken as well.
    pub fn parse(s: &str) -> Option<Self> {
        let digits: Vec<u8> = s.bytes().filter(|b| !matches!(b, b':' | b' ' | b'-')).collect();
        if digits.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (slot, pair) in out.iter_mut().zip(digits.chunks(2)) {
            let hex = |b: u8| char::from(b).to_digit(16);
            let (Some(hi), Some(lo)) = (pair.first().and_then(|b| hex(*b)), pair.get(1).and_then(|b| hex(*b))) else { return None };
            *slot = u8::try_from(hi * 16 + lo).ok()?;
        }
        Some(Self(out))
    }
}

impl std::fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(":")?;
            }
            write!(f, "{b:02X}")?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

/// A certificate a server showed that was not trusted, as the owner is asked about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    /// The server name it was shown for.
    pub host: String,
    pub fingerprint: Fingerprint,
    /// Why it was not trusted, in words.
    pub reason: String,
    /// Whether the owner may trust it: not when it has been revoked by whoever signed it.
    pub trustable: bool,
    /// The owner had trusted another certificate for this server: this one is not that one.
    pub changed: bool,
    pub about: About,
}

/// What a certificate says of itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct About {
    /// Whom it was made for: its subject's common name and organisation.
    pub subject: String,
    /// Who signed it; the same as the subject for one a server made of its own.
    pub issuer: String,
    /// The server names it covers (its subject alternative names).
    pub names: Vec<String>,
    /// From and until when it holds, as YYYY-MM-DD.
    pub not_before: String,
    pub not_after: String,
}

impl About {
    /// Whether it was signed by what it was made for: a certificate of the server's own.
    pub fn own(&self) -> bool {
        !self.subject.is_empty() && self.subject == self.issuer
    }
}

/// Names and fields read at most.
const MAX_NAMES: usize = 32;
const MAX_TEXT: usize = 200;

/// One DER element: its tag, its contents and what follows it.
fn element(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    // Tags of more than one byte are not found where this reads.
    if tag & 0x1F == 0x1F {
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7F);
        // No indefinite lengths in DER, and nothing in a certificate is 4 GB.
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let (bytes, rest) = rest.split_at(count);
        (bytes.iter().fold(0usize, |n, b| (n << 8) | usize::from(*b)), rest)
    };
    if len > rest.len() {
        return None;
    }
    let (contents, rest) = rest.split_at(len);
    Some((tag, contents, rest))
}

/// The elements one after another inside a constructed element.
fn elements(mut input: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    std::iter::from_fn(move || {
        let (tag, contents, rest) = element(input)?;
        input = rest;
        Some((tag, contents))
    })
}

const SEQUENCE: u8 = 0x30;
const SET: u8 = 0x31;
const OID: u8 = 0x06;
/// commonName, organizationName ([RFC 5280] A.1) and subjectAltName (4.2.1.6).
const CN: &[u8] = &[0x55, 0x04, 0x03];
const O: &[u8] = &[0x55, 0x04, 0x0A];
const SAN: &[u8] = &[0x55, 0x1D, 0x11];

/// A string of a name, of the kinds a certificate uses; control characters left out.
fn string(tag: u8, bytes: &[u8]) -> Option<String> {
    let text = match tag {
        // UTF8String, PrintableString, TeletexString (taken as Latin-1), IA5String.
        0x0C | 0x13 | 0x16 => String::from_utf8_lossy(bytes).into_owned(),
        0x14 => bytes.iter().map(|b| char::from(*b)).collect(),
        // BMPString: UTF-16, high byte first.
        0x1E => {
            let units: Vec<u16> = bytes.chunks_exact(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect();
            String::from_utf16_lossy(&units)
        }
        _ => return None,
    };
    Some(text.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect())
}

/// A Name ([RFC 5280] 4.1.2.4) as its common name and organisation, "CN, O".
fn name(der: &[u8]) -> String {
    let (mut cn, mut o) = (None, None);
    for (tag, set) in elements(der) {
        if tag != SET {
            continue;
        }
        for (tag, pair) in elements(set) {
            if tag != SEQUENCE {
                continue;
            }
            let mut parts = elements(pair);
            let (Some((OID, oid)), Some((kind, value))) = (parts.next(), parts.next()) else { continue };
            let slot = if oid == CN {
                &mut cn
            } else if oid == O {
                &mut o
            } else {
                continue;
            };
            if slot.is_none() {
                *slot = string(kind, value);
            }
        }
    }
    match (cn, o) {
        (Some(cn), Some(o)) if cn != o => format!("{cn}, {o}"),
        (Some(one), _) | (None, Some(one)) => one,
        (None, None) => String::new(),
    }
}

/// UTCTime or GeneralizedTime ([RFC 5280] 4.1.2.5) as YYYY-MM-DD.
fn date(tag: u8, bytes: &[u8]) -> String {
    let digits = |b: &[u8]| -> Option<u32> { std::str::from_utf8(b).ok().filter(|s| s.bytes().all(|c| c.is_ascii_digit()))?.parse().ok() };
    let (year, rest) = match tag {
        0x17 => {
            let yy = bytes.get(..2).and_then(digits);
            (yy.map(|y| if y >= 50 { 1900 + y } else { 2000 + y }), bytes.get(2..))
        }
        0x18 => (bytes.get(..4).and_then(digits), bytes.get(4..)),
        _ => (None, None),
    };
    let month = rest.and_then(|r| r.get(..2)).and_then(digits).filter(|m| (1..=12).contains(m));
    let day = rest.and_then(|r| r.get(2..4)).and_then(digits).filter(|d| (1..=31).contains(d));
    match (year, month, day) {
        (Some(y), Some(m), Some(d)) => format!("{y:04}-{m:02}-{d:02}"),
        _ => String::new(),
    }
}

/// The server names of a subjectAltName extension's value: dNSName and iPAddress.
fn alt_names(value: &[u8], out: &mut Vec<String>) {
    let Some((SEQUENCE, names, _)) = element(value) else { return };
    for (tag, bytes) in elements(names) {
        if out.len() >= MAX_NAMES {
            return;
        }
        match tag {
            0x82 => out.extend(string(0x16, bytes)),
            0x87 if bytes.len() == 4 => out.push(format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])),
            0x87 if bytes.len() == 16 => {
                let groups: Vec<String> = bytes.chunks_exact(2).map(|p| format!("{:x}", u16::from_be_bytes([p[0], p[1]]))).collect();
                out.push(groups.join(":"));
            }
            _ => {}
        }
    }
}

/// What a certificate says of itself; fields that cannot be read are left empty.
pub fn about(der: &[u8]) -> About {
    let mut out = About::default();
    let Some((SEQUENCE, cert, _)) = element(der) else { return out };
    let Some((SEQUENCE, tbs, _)) = element(cert) else { return out };
    let mut fields = elements(tbs).peekable();
    // version [0], when there, then serialNumber and signature.
    if fields.peek().is_some_and(|(tag, _)| *tag == 0xA0) {
        fields.next();
    }
    let _serial = fields.next();
    let _signature = fields.next();
    if let Some((SEQUENCE, issuer)) = fields.next() {
        out.issuer = name(issuer);
    }
    if let Some((SEQUENCE, validity)) = fields.next() {
        let mut times = elements(validity);
        if let Some((tag, t)) = times.next() {
            out.not_before = date(tag, t);
        }
        if let Some((tag, t)) = times.next() {
            out.not_after = date(tag, t);
        }
    }
    if let Some((SEQUENCE, subject)) = fields.next() {
        out.subject = name(subject);
    }
    // subjectPublicKeyInfo, the unique ids [1] and [2], then the extensions [3].
    for (tag, contents) in fields {
        if tag != 0xA3 {
            continue;
        }
        let Some((SEQUENCE, all, _)) = element(contents) else { break };
        for (tag, ext) in elements(all) {
            if tag != SEQUENCE {
                continue;
            }
            let mut parts = elements(ext);
            let Some((OID, oid)) = parts.next() else { continue };
            if oid != SAN {
                continue;
            }
            // critical, when there, then the value.
            if let Some((_, value)) = parts.find(|(tag, _)| *tag == 0x04) {
                alt_names(value, &mut out.names);
            }
        }
    }
    out
}
