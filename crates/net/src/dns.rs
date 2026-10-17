//! Names looked up in the DNS ([RFC 1035]): the mail exchangers of a domain (MX) and the
//! services it names for mail programs (SRV, [RFC 2782] and [RFC 6186]). Only the asking and
//! the reading of an answer are here; the asking goes through a [`Connector`](crate::Connector),
//! so the fake server can answer it in tests.
//!
//! An answer is hostile: every length and count in it is checked against the bytes there are,
//! names are followed through at most a few compression pointers and never past 255 bytes,
//! and no more records are taken than a domain needs.
//!
//! [RFC 1035]: https://www.rfc-editor.org/rfc/rfc1035
//! [RFC 2782]: https://www.rfc-editor.org/rfc/rfc2782
//! [RFC 6186]: https://www.rfc-editor.org/rfc/rfc6186

use crate::NetError;

/// A DNS message over UDP is no longer than this unless the server says it was cut short
/// ([RFC 1035] 4.2.1); over TCP, no longer than a two-byte length.
pub const MAX_UDP: usize = 512;
/// Records taken from one answer.
const MAX_RECORDS: usize = 32;
/// Compression pointers followed in one name.
const MAX_JUMPS: usize = 16;
/// A name's length, its labels and the dots between them ([RFC 1035] 2.3.4).
const MAX_NAME: usize = 255;

/// The kinds of record asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Mx,
    Srv,
}

impl Kind {
    /// Its number in a question ([RFC 1035] 3.2.2, [RFC 2782]).
    fn code(self) -> u16 {
        match self {
            Self::Mx => 15,
            Self::Srv => 33,
        }
    }
}

/// A record of an answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Record {
    /// A mail exchanger: the lower its preference, the sooner it is used.
    Mx { preference: u16, exchange: String },
    /// A service: its server and port; the lower the priority the sooner it is used, and among
    /// one priority the higher the weight the more ([RFC 2782]). A target of `.` says the
    /// service is not offered.
    Srv { priority: u16, weight: u16, port: u16, target: String },
}

/// A name as a question carries it ([RFC 1035] 3.1): each label after its length, then a zero.
fn encode_name(name: &str, out: &mut Vec<u8>) -> Result<(), NetError> {
    let name = name.trim_end_matches('.');
    if name.is_empty() || name.len() > 253 {
        return Err(NetError::Unsupported("not a name that can be looked up".into()));
    }
    for label in name.split('.') {
        let ok = !label.is_empty() && label.len() <= 63 && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !ok {
            return Err(NetError::Unsupported("not a name that can be looked up".into()));
        }
        out.push(u8::try_from(label.len()).map_err(|_| NetError::Unsupported("a label too long".into()))?);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    Ok(())
}

/// A question for `name`'s records of `kind`, with `id` to know its answer by, recursion asked
/// for ([RFC 1035] 4.1.1).
pub fn question(id: u16, name: &str, kind: Kind) -> Result<Vec<u8>, NetError> {
    let mut out = Vec::with_capacity(32 + name.len());
    out.extend_from_slice(&id.to_be_bytes());
    // RD set; one question, no answers.
    out.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    encode_name(name, &mut out)?;
    out.extend_from_slice(&kind.code().to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    Ok(out)
}

/// What an answer said.
#[derive(Debug, PartialEq, Eq)]
pub struct Answer {
    pub records: Vec<Record>,
    /// It was cut short, and is asked again over TCP.
    pub truncated: bool,
}

fn bad(what: &str) -> NetError {
    NetError::protocol(format!("a DNS answer {what}"))
}

fn u16_at(msg: &[u8], at: usize) -> Result<u16, NetError> {
    let b = msg.get(at..at.checked_add(2).ok_or_else(|| bad("too short"))?).ok_or_else(|| bad("too short"))?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}

/// The name at `at`, in lower case, and where what follows it begins.
fn read_name(msg: &[u8], at: usize) -> Result<(String, usize), NetError> {
    let (mut name, mut pos, mut after, mut jumps) = (String::new(), at, None::<usize>, 0usize);
    loop {
        let len = *msg.get(pos).ok_or_else(|| bad("ends inside a name"))?;
        match len & 0xC0 {
            0x00 if len == 0 => {
                return Ok((name, after.unwrap_or(pos + 1)));
            }
            0x00 => {
                let start = pos + 1;
                let label = msg.get(start..start + usize::from(len)).ok_or_else(|| bad("ends inside a name"))?;
                if !name.is_empty() {
                    name.push('.');
                }
                for b in label {
                    // A name of ours holds letters, digits, hyphens and underscores; anything
                    // else in a name it gives makes it no name of a server.
                    if !(b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_') {
                        return Err(bad("holds a name that is not one"));
                    }
                    name.push(char::from(b.to_ascii_lowercase()));
                }
                if name.len() > MAX_NAME {
                    return Err(bad("holds a name too long"));
                }
                pos = start + usize::from(len);
            }
            0xC0 => {
                let low = *msg.get(pos + 1).ok_or_else(|| bad("ends inside a name"))?;
                jumps += 1;
                if jumps > MAX_JUMPS {
                    return Err(bad("points round in a circle"));
                }
                after.get_or_insert(pos + 2);
                // A pointer goes back, to what was before it ([RFC 1035] 4.1.4: "a prior
                // occurrence"), so it can never lead on for ever.
                let to = usize::from(u16::from_be_bytes([len & 0x3F, low]));
                if to >= pos {
                    return Err(bad("points forward"));
                }
                pos = to;
            }
            _ => return Err(bad("holds a label of a kind not known")),
        }
    }
}

/// Read the answer `msg` to the question with `id` for `name`'s records of `kind`. A name that
/// does not exist answers no records.
pub fn answer(msg: &[u8], id: u16, name: &str, kind: Kind) -> Result<Answer, NetError> {
    if u16_at(msg, 0)? != id {
        return Err(bad("to another question"));
    }
    let flags = u16_at(msg, 2)?;
    if flags & 0x8000 == 0 || (flags >> 11) & 0xF != 0 {
        return Err(bad("that is not an answer"));
    }
    let truncated = flags & 0x0200 != 0;
    match flags & 0xF {
        0 => {}
        // NXDOMAIN: there is no such name.
        3 => return Ok(Answer { records: Vec::new(), truncated: false }),
        code => return Err(NetError::Refused(format!("the name server answered with error {code}"))),
    }
    let (questions, answers) = (u16_at(msg, 4)?, u16_at(msg, 6)?);
    let mut pos = 12usize;
    let wanted = name.trim_end_matches('.').to_ascii_lowercase();
    for _ in 0..questions {
        let (asked, next) = read_name(msg, pos)?;
        if asked != wanted || u16_at(msg, next)? != kind.code() {
            return Err(bad("to another question"));
        }
        pos = next + 4;
    }
    let mut records = Vec::new();
    for _ in 0..answers {
        if truncated && pos >= msg.len() {
            break;
        }
        let (_, next) = read_name(msg, pos)?;
        let (rtype, class) = (u16_at(msg, next)?, u16_at(msg, next + 2)?);
        let len = usize::from(u16_at(msg, next + 8)?);
        let data = next + 10;
        let end = data.checked_add(len).filter(|end| *end <= msg.len()).ok_or_else(|| bad("ends inside a record"))?;
        pos = end;
        // A record of another kind (the CNAME the name led through, say) is passed over: what
        // is asked of the server is what it has for the name it ends at.
        if class != 1 || rtype != kind.code() || records.len() >= MAX_RECORDS {
            continue;
        }
        let record = match kind {
            Kind::Mx => {
                let preference = u16_at(msg, data)?;
                let (exchange, after) = read_name(msg, data + 2)?;
                if after != end {
                    return Err(bad("holds a record of the wrong length"));
                }
                Record::Mx { preference, exchange }
            }
            Kind::Srv => {
                let (priority, weight, port) = (u16_at(msg, data)?, u16_at(msg, data + 2)?, u16_at(msg, data + 4)?);
                let (target, after) = read_name(msg, data + 6)?;
                if after != end {
                    return Err(bad("holds a record of the wrong length"));
                }
                Record::Srv { priority, weight, port, target }
            }
        };
        records.push(record);
    }
    Ok(Answer { records, truncated })
}

/// The name servers of `/etc/resolv.conf`, as Unix systems keep them, in their order.
pub fn resolv_conf(text: &str) -> Vec<std::net::IpAddr> {
    let mut out = Vec::new();
    for line in text.lines().take(1000) {
        let mut words = line.split_whitespace();
        if words.next() == Some("nameserver")
            && let Some(ip) = words.next().and_then(|w| w.split('%').next()).and_then(|w| w.parse().ok())
            && !out.contains(&ip)
        {
            out.push(ip);
        }
    }
    out
}

/// Name servers as Windows keeps them for an interface: addresses parted by spaces or commas.
pub fn listed(text: &str) -> Vec<std::net::IpAddr> {
    text.split([' ', ',', ';']).filter_map(|w| w.trim().parse().ok()).take(16).collect()
}
