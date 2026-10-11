use super::*;
use mailcraft_model::{Attachment, Item, NameId, NamedKey, PropValue, RECIP_CC, RECIP_TO, pid, pset, time};

fn sample() -> Item {
    let mut it = Item::new_mail();
    it.set_subject("Größe ✓ quarterly numbers");
    it.set_sender("Alex Morgan", "alex.morgan@example.org");
    it.add_recipient(RECIP_TO, "Ana Costa", "ana@example.org");
    it.add_recipient(RECIP_CC, "Zöe Ångström", "zoe@example.org");
    it.set_body_text("Line one\nLine two – done");
    it.props.set(pid::HTML, PropValue::Binary("<p>Hi ✓</p>".as_bytes().to_vec()));
    it.props.set(pid::INTERNET_CODEPAGE, PropValue::Int32(65001));
    it.props.set(pid::CLIENT_SUBMIT_TIME, PropValue::Time(time::from_civil(2026, 10, 5, 9, 14, 30, 0)));
    it.props.set(0x0E1F, PropValue::Bool(true));
    it.props.set(0x6001, PropValue::MultiBinary(vec![vec![1, 2, 3], vec![], vec![9; 5000]]));
    it.props.set(0x6002, PropValue::MultiInt32(vec![1, -2, 3]));
    it.props.set(0x6003, PropValue::Guid([7; 16]));
    it.props.set(0x6004, PropValue::Other { ty: 0x00FB, raw: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10] });
    it.props.set(0x6005, PropValue::Int64(-5));
    it.categories = vec!["Red Category".to_string(), "Projekt Ä".to_string()];
    it.named.push((NamedKey { guid: pset::PSETID_COMMON, name: NameId::Id(0x8503) }, PropValue::Bool(true)));
    it.named.push((NamedKey { guid: [0x42; 16], name: NameId::Name("x-custom".to_string()) }, PropValue::String("kept".to_string())));
    it.attachments.push(Attachment::file("Bericht für März.bin", "application/octet-stream", (0..=255u8).cycle().take(70_000).collect()));
    let mut inner = Item::new_mail();
    inner.set_subject("Inner");
    inner.set_body_text("inner body");
    inner.add_recipient(RECIP_TO, "Bo", "bo@example.org");
    inner.categories = vec!["Blue".to_string()];
    inner.attachments.push(Attachment::file("deep.txt", "text/plain", b"deep".to_vec()));
    let mut wrapper = Attachment::file("Inner.msg", "", Vec::new());
    wrapper.embedded = Some(Box::new(inner));
    it.attachments.push(wrapper);
    it
}

#[test]
fn write_then_read_round_trips() {
    let it = sample();
    let bytes = write(&it).unwrap();
    let back = read(&bytes).unwrap();
    assert_eq!(back.subject(), it.subject());
    assert_eq!(back.body_text(), it.body_text());
    assert_eq!(back.body_html(), it.body_html());
    assert_eq!(back.sender_email(), "alex.morgan@example.org");
    assert_eq!(back.display_recipients(RECIP_CC), "Zöe Ångström");
    assert_eq!(back.recipients.len(), 2);
    assert_eq!(back.time(), it.time());
    assert_eq!(back.categories, it.categories);
    assert_eq!(back.named, it.named);
    for id in [0x0E1F, 0x6001, 0x6002, 0x6003, 0x6004, 0x6005] {
        assert_eq!(back.props.get(id), it.props.get(id), "property 0x{id:04X}");
    }
    assert_eq!(back.attachments.len(), 2);
    assert_eq!(back.attachments[0].name(), "Bericht für März.bin");
    assert_eq!(back.attachments[0].data, it.attachments[0].data);
    let inner = back.attachments[1].embedded.as_ref().unwrap();
    assert_eq!(inner.subject(), "Inner");
    assert_eq!(inner.categories, vec!["Blue".to_string()]);
    assert_eq!(inner.attachments[0].data, b"deep");
    assert_eq!(back.attachments[1].props.i32(pid::ATTACH_METHOD), Some(5));
    // Writing what was read gives the same message again.
    let again = read(&write(&back).unwrap()).unwrap();
    assert_eq!(again.props, back.props);
    assert_eq!(again.named, back.named);
}

#[test]
fn layout_follows_the_specification() {
    let bytes = write(&sample()).unwrap();
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(&bytes)).unwrap();
    assert_eq!(cf.version(), cfb::Version::V3);
    assert_eq!(cf.root_entry().clsid().to_string(), "00020d0b-0000-0000-c000-000000000046");
    for stream in [
        "/__properties_version1.0",
        "/__substg1.0_0037001F",
        "/__nameid_version1.0/__substg1.0_00030102",
        "/__recip_version1.0_#00000001/__substg1.0_3001001F",
        "/__attach_version1.0_#00000000/__substg1.0_37010102",
    ] {
        assert!(cf.is_stream(stream), "{stream}");
    }
    assert!(cf.is_storage("/__attach_version1.0_#00000001/__substg1.0_3701000D"));
    // Top-level header: recipient and attachment counts at 16 and 20 ([MS-OXMSG] 2.4.1.1).
    let mut props = Vec::new();
    std::io::Read::read_to_end(&mut cf.open_stream("/__properties_version1.0").unwrap(), &mut props).unwrap();
    assert_eq!(&props[16..24], &[2, 0, 0, 0, 2, 0, 0, 0]);
    assert_eq!((props.len() - 32) % 16, 0);
    // The subject entry: tag 0x0037001F, size = UTF-16 bytes + 2.
    let entry = props[32..].chunks(16).find(|e| e[..4] == [0x1F, 0x00, 0x37, 0x00]).unwrap();
    let len = "Größe ✓ quarterly numbers".encode_utf16().count() * 2 + 2;
    assert_eq!(u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize, len);
    assert_eq!(write_as(&sample(), Kind::Template).unwrap().len(), bytes.len());
}

/// Outlook finds a property by name through the lookup stream its checksum selects. With the wrong
/// checksum the file opens but the categories are gone (seen with Outlook Classic).
#[test]
fn named_property_lookup_uses_the_zero_seeded_checksum() {
    let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
    assert_eq!(crc32(&utf16("Keywords")), 0x2EDA_4D3B);
    assert_eq!(crc32(&utf16("InTransitMessageCorrelator")), 0xFF0C_62AC);
    assert_eq!(crc32(&[]), 0);

    let bytes = write(&sample()).unwrap();
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(&bytes)).unwrap();
    // Keywords is PS_PUBLIC_STRINGS (index 2) with a string name: 0x1000 + (crc ^ 5) % 0x1F.
    let mut lookup = Vec::new();
    std::io::Read::read_to_end(&mut cf.open_stream("/__nameid_version1.0/__substg1.0_10150102").unwrap(), &mut lookup).unwrap();
    assert_eq!(&lookup[..8], &[0x3B, 0x4D, 0xDA, 0x2E, 0x05, 0x00, 0x00, 0x00]);
}

#[test]
fn hostile_input_never_panics() {
    assert!(matches!(read(b""), Err(MsgError::NotMsg)));
    assert!(matches!(read(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]), Err(MsgError::NotMsg)));
    // A compound file that is not a message.
    let mut cf = cfb::CompoundFile::create(std::io::Cursor::new(Vec::new())).unwrap();
    cf.create_stream("/WordDocument").unwrap();
    assert!(matches!(read(&cf.into_inner().into_inner()), Err(MsgError::NotMsg)));
    let mut small = Item::new_mail();
    small.set_subject("s");
    small.add_recipient(RECIP_TO, "A", "a@example.org");
    small.categories = vec!["c".into()];
    small.attachments.push(Attachment::file("a.txt", "text/plain", b"abc".to_vec()));
    let good = write(&small).unwrap();
    for n in (0..good.len()).step_by(7) {
        let _ = read(&good[..n]);
    }
    for n in 0..good.len() {
        let mut bad = good.clone();
        bad[n] = bad[n].wrapping_mul(31).wrapping_add(n as u8 | 1);
        let _ = read(&bad);
    }
}
