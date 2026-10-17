//! DNS answers read byte by byte, hostile ones among them, and a question asked of a name
//! server on this machine's loopback address: nothing leaves the process.

use crate::dns::{Kind, Record, answer, listed, question, resolv_conf};
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

/// A name as a message carries it.
fn name(n: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for label in n.split('.').filter(|l| !l.is_empty()) {
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out
}

/// An answer to `id` for `asked` of `kind`, with `records` (type, data) each owned by a
/// pointer to the name asked, which is at 12.
fn reply(id: u16, flags: u16, asked: &str, kind: u16, records: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut m = id.to_be_bytes().to_vec();
    m.extend_from_slice(&flags.to_be_bytes());
    m.extend_from_slice(&[0, 1]);
    m.extend_from_slice(&(records.len() as u16).to_be_bytes());
    m.extend_from_slice(&[0, 0, 0, 0]);
    m.extend_from_slice(&name(asked));
    m.extend_from_slice(&kind.to_be_bytes());
    m.extend_from_slice(&[0, 1]);
    for (rtype, data) in records {
        m.extend_from_slice(&[0xC0, 12]);
        m.extend_from_slice(&rtype.to_be_bytes());
        m.extend_from_slice(&[0, 1, 0, 0, 0x0E, 0x10]);
        m.extend_from_slice(&(data.len() as u16).to_be_bytes());
        m.extend_from_slice(data);
    }
    m
}

fn mx(preference: u16, exchange: &[u8]) -> (u16, Vec<u8>) {
    let mut d = preference.to_be_bytes().to_vec();
    d.extend_from_slice(exchange);
    (15, d)
}

fn srv(priority: u16, weight: u16, port: u16, target: &[u8]) -> (u16, Vec<u8>) {
    let mut d = [priority, weight, port].iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<u8>>();
    d.extend_from_slice(target);
    (33, d)
}

const OK: u16 = 0x8180;

#[test]
fn a_question_is_as_rfc_1035_lays_it_out() {
    let q = question(0xBEEF, "_imaps._tcp.Bluebird.Example.", Kind::Srv).unwrap();
    let mut want = vec![0xBE, 0xEF, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    want.extend_from_slice(&name("_imaps._tcp.Bluebird.Example"));
    want.extend_from_slice(&[0, 33, 0, 1]);
    assert_eq!(q, want);
    for bad in ["", ".", "a..b", "a b.example", "x\r\n.example", &format!("{}.example", "a".repeat(64)), &"a.".repeat(130)] {
        assert!(question(1, bad, Kind::Mx).is_err(), "{bad:?}");
    }
}

#[test]
fn mail_exchangers_are_read_through_compressed_names() {
    // The second exchanger's name ends with a pointer to the domain asked, at 12.
    let mut tail = vec![3];
    tail.extend_from_slice(b"ALT");
    tail.extend_from_slice(&[0xC0, 12]);
    let m = reply(7, OK, "bluebird.example", 15, &[mx(10, &name("mx.bluebird.example")), mx(20, &tail), (5, name("other.example"))]);
    let a = answer(&m, 7, "Bluebird.Example.", Kind::Mx).unwrap();
    assert!(!a.truncated);
    assert_eq!(
        a.records,
        vec![
            Record::Mx { preference: 10, exchange: "mx.bluebird.example".into() },
            Record::Mx { preference: 20, exchange: "alt.bluebird.example".into() }
        ]
    );
}

#[test]
fn services_are_read_and_a_name_that_does_not_exist_has_none() {
    let m = reply(9, OK, "_imaps._tcp.bluebird.example", 33, &[srv(0, 5, 993, &name("imap.bluebird.example")), srv(0, 0, 0, &[0])]);
    let a = answer(&m, 9, "_imaps._tcp.bluebird.example", Kind::Srv).unwrap();
    assert_eq!(
        a.records,
        vec![
            Record::Srv { priority: 0, weight: 5, port: 993, target: "imap.bluebird.example".into() },
            Record::Srv { priority: 0, weight: 0, port: 0, target: String::new() }
        ]
    );
    let none = reply(9, 0x8183, "_imaps._tcp.bluebird.example", 33, &[]);
    assert!(answer(&none, 9, "_imaps._tcp.bluebird.example", Kind::Srv).unwrap().records.is_empty());
    let cut = reply(9, OK | 0x0200, "_imaps._tcp.bluebird.example", 33, &[]);
    assert!(answer(&cut, 9, "_imaps._tcp.bluebird.example", Kind::Srv).unwrap().truncated);
}

#[test]
fn hostile_answers_are_refused_and_never_panic() {
    let asked = "bluebird.example";
    let good = reply(3, OK, asked, 15, &[mx(10, &name("mx.bluebird.example"))]);
    let read = |m: &[u8]| answer(m, 3, asked, Kind::Mx);
    assert!(read(&good).is_ok());
    // Cut short at every length.
    for n in 0..good.len() {
        assert!(read(&good[..n]).is_err(), "cut at {n}");
    }
    let with = |at: usize, b: &[u8]| {
        let mut m = good.clone();
        m.splice(at..at + b.len(), b.iter().copied());
        m
    };
    let swapped = |rtype: (u16, Vec<u8>)| reply(3, OK, asked, 15, &[rtype]);
    let bad = [
        // Another id, not an answer, a server failure, another question.
        with(0, &[0, 4]),
        with(2, &[0x01, 0x80]),
        with(2, &[0x81, 0x82]),
        reply(3, OK, "other.example", 15, &[]),
        reply(3, OK, asked, 33, &[]),
        // More records said than there are.
        with(6, &[0xFF, 0xFF]),
        // A pointer to itself, one forward, one of a kind not known.
        swapped(mx(1, &[0xC0, 48])),
        swapped(mx(1, &[0xC0, 0xFF])),
        swapped(mx(1, &[0x80, 12])),
        // A record whose length is not its name's.
        swapped((15, vec![0, 1, 0, 0])),
        swapped((15, vec![0, 1])),
        // A name with what no server's name holds.
        swapped(mx(1, &[3, b'a', b' ', b'b', 0])),
        swapped(mx(1, &[3, b'a', 0xFF, b'b', 0])),
    ];
    for (i, m) in bad.iter().enumerate() {
        assert!(read(m).is_err(), "{i}");
    }
    // A name too long, and pointers going back and back.
    let mut long = Vec::new();
    for _ in 0..5 {
        long.push(63);
        long.extend_from_slice(&[b'a'; 63]);
    }
    long.push(0);
    assert!(read(&swapped(mx(1, &long))).is_err());
    // An answer of many records keeps only so many.
    let many: Vec<_> = (0..200).map(|i| mx(i, &name("mx.bluebird.example"))).collect();
    assert_eq!(read(&reply(3, OK, asked, 15, &many)).unwrap().records.len(), 32);
    // Bounded fuzz: bytes of a good answer changed at random never panic.
    let mut seed = 0x2545_F491u32;
    for _ in 0..20_000 {
        let mut m = good.clone();
        for _ in 0..3 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let at = (seed >> 8) as usize % m.len();
            m[at] = (seed >> 24) as u8;
        }
        let _ = read(&m);
        let _ = answer(&m, 3, asked, Kind::Srv);
    }
}

#[test]
fn name_servers_are_read_as_systems_keep_them() {
    let text = "# a comment\nsearch example\nnameserver 192.0.2.53\nnameserver fe80::1%eth0\nnameserver 192.0.2.53\nnameserver junk\n";
    assert_eq!(resolv_conf(text), vec!["192.0.2.53".parse::<std::net::IpAddr>().unwrap(), "fe80::1".parse().unwrap()]);
    assert_eq!(listed("192.0.2.1 192.0.2.2,198.51.100.1;x").len(), 3);
    assert!(listed("").is_empty());
}

#[test]
fn a_name_server_is_asked_and_its_answer_taken_from_it_alone() {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    server.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let at: SocketAddr = server.local_addr().unwrap();
    let serve = std::thread::spawn(move || {
        let mut buf = [0u8; 512];
        let (n, from) = server.recv_from(&mut buf).unwrap();
        let id = u16::from_be_bytes([buf[0], buf[1]]);
        assert_eq!(&buf[n - 4..n], &[0, 15, 0, 1]);
        // An answer to another question comes first, then the right one.
        server.send_to(&reply(id.wrapping_add(1), OK, "bluebird.example", 15, &[]), from).unwrap();
        server.send_to(&reply(id, OK, "bluebird.example", 15, &[mx(5, &name("mx.bluebird.example"))]), from).unwrap();
    });
    let records = crate::tcp::look_up(&[at], "bluebird.example", Kind::Mx, Duration::from_secs(5)).unwrap();
    serve.join().unwrap();
    assert_eq!(records, vec![Record::Mx { preference: 5, exchange: "mx.bluebird.example".into() }]);
    // No one there: an error, soon.
    let quiet = UdpSocket::bind("127.0.0.1:0").unwrap();
    let to = quiet.local_addr().unwrap();
    assert!(crate::tcp::look_up(&[to], "bluebird.example", Kind::Mx, Duration::from_millis(200)).is_err());
}
