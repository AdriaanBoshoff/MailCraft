//! Reading a `.msg`.

use crate::{
    ATTACH_PREFIX, EMBEDDED_STORAGE, HEADER_CHILD, HEADER_EMBEDDED, HEADER_TOP, MsgError, NAMEID_STORAGE, PROPS_STREAM, RECIP_PREFIX, value_stream,
};
use openmailclient_model::{Attachment, Item, NamedKey, PropBag, PropValue, codepage, pid, ptype};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

/// No single stream is read past this size.
const MAX_STREAM: u64 = 512 * 1024 * 1024;
/// Recipients, attachments, properties and multi-value entries beyond these counts are ignored.
const MAX_CHILDREN: usize = 20_000;
const MAX_PROPS: usize = 8_192;
const MAX_VALUES: usize = 4_096;
/// Messages attached to messages are followed this deep.
const MAX_DEPTH: usize = 8;

type Cf<'a> = cfb::CompoundFile<Cursor<&'a [u8]>>;

/// Read a `.msg` or `.oft` file.
pub fn read(data: &[u8]) -> Result<Item, MsgError> {
    // The container library is not ours; a panic inside it on a damaged file must not escape.
    let result = std::panic::catch_unwind(|| read_inner(data));
    result.unwrap_or_else(|_| Err(MsgError::Damaged("the compound file structure is not readable".into())))
}

fn read_inner(data: &[u8]) -> Result<Item, MsgError> {
    let mut cf = cfb::CompoundFile::open(Cursor::new(data)).map_err(|_| MsgError::NotMsg)?;
    let root = PathBuf::from("/");
    if !cf.is_stream(root.join(PROPS_STREAM)) {
        return Err(MsgError::NotMsg);
    }
    let names = read_names(&mut cf);
    Ok(read_message(&mut cf, &root, HEADER_TOP, &names, 0))
}

fn read_stream(cf: &mut Cf, path: &Path) -> Option<Vec<u8>> {
    let mut s = cf.open_stream(path).ok()?;
    if s.len() > MAX_STREAM {
        return None;
    }
    let mut out = Vec::new();
    s.read_to_end(&mut out).ok()?;
    Some(out)
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// The named property mapping ([MS-OXMSG] 2.2.3): index n is property id 0x8000 + n.
fn read_names(cf: &mut Cf) -> Vec<Option<NamedKey>> {
    let base = PathBuf::from("/").join(NAMEID_STORAGE);
    let guids = read_stream(cf, &base.join(value_stream(0x0002, ptype::BINARY))).unwrap_or_default();
    let entries = read_stream(cf, &base.join(value_stream(0x0003, ptype::BINARY))).unwrap_or_default();
    let strings = read_stream(cf, &base.join(value_stream(0x0004, ptype::BINARY))).unwrap_or_default();
    openmailclient_model::props::parse_name_map(&guids, &entries, &strings)
}

struct Object {
    props: PropBag,
    named: Vec<(NamedKey, PropValue)>,
}

/// One object's properties: the property stream plus a value stream per variable-size property
/// ([MS-OXMSG] 2.4). `cp` is the code page for 8-bit strings; 0 takes it from the object itself.
fn read_object(cf: &mut Cf, base: &Path, header: usize, names: &[Option<NamedKey>], cp: u32) -> (Object, u32) {
    let stream = read_stream(cf, &base.join(PROPS_STREAM)).unwrap_or_default();
    let entries: Vec<(u16, u16, [u8; 8])> = stream
        .get(header..)
        .unwrap_or(&[])
        .chunks_exact(16)
        .take(MAX_PROPS)
        .filter_map(|e| {
            let tag = u32_at(e, 0)?;
            Some(((tag >> 16) as u16, (tag & 0xFFFF) as u16, <[u8; 8]>::try_from(e.get(8..16)?).ok()?))
        })
        .collect();
    let own_cp =
        |want: u16| entries.iter().find(|(id, ty, _)| *id == want && *ty == ptype::INT32).and_then(|(_, _, v)| u32_at(v, 0)).filter(|c| *c != 0);
    let cp = if cp != 0 { cp } else { own_cp(pid::MESSAGE_CODEPAGE).or_else(|| own_cp(pid::INTERNET_CODEPAGE)).unwrap_or(1252) };
    let mut obj = Object { props: PropBag::default(), named: Vec::new() };
    for (id, ty, value) in entries {
        let fixed = ptype::fixed_size(ty).filter(|n| *n <= 8);
        let v = if fixed.is_some() {
            PropValue::from_fixed(ty, &value)
        } else if ty == ptype::OBJECT {
            None
        } else {
            read_variable(cf, base, id, ty, cp)
        };
        let Some(v) = v else { continue };
        if id >= 0x8000 {
            if let Some(Some(key)) = names.get(usize::from(id - 0x8000)) {
                obj.named.push((key.clone(), v));
            }
        } else {
            obj.props.set(id, v);
        }
    }
    (obj, cp)
}

fn read_variable(cf: &mut Cf, base: &Path, id: u16, ty: u16, cp: u32) -> Option<PropValue> {
    let name = value_stream(id, ty);
    let data = read_stream(cf, &base.join(&name))?;
    let strip = |s: String| s.trim_end_matches('\0').to_string();
    Some(match ty {
        ptype::STRING => PropValue::String(strip(codepage::utf16le(&data))),
        ptype::STRING8 => PropValue::String(strip(codepage::decode(cp, &data))),
        ptype::BINARY => PropValue::Binary(data),
        ptype::GUID => PropValue::Guid(data.get(..16)?.try_into().ok()?),
        ptype::MULTI_INT32 => {
            PropValue::MultiInt32(data.chunks_exact(4).take(MAX_VALUES).filter_map(|c| Some(i32::from_le_bytes(c.try_into().ok()?))).collect())
        }
        ptype::MULTI_STRING | ptype::MULTI_STRING8 | ptype::MULTI_BINARY => {
            // The stream itself holds one length entry per value; values are in "-NNNNNNNN" streams.
            let entry = if ty == ptype::MULTI_BINARY { 8 } else { 4 };
            let count = (data.len() / entry).min(MAX_VALUES);
            let values: Vec<Vec<u8>> = (0..count).map(|i| read_stream(cf, &base.join(format!("{name}-{i:08X}"))).unwrap_or_default()).collect();
            match ty {
                ptype::MULTI_BINARY => PropValue::MultiBinary(values),
                ptype::MULTI_STRING => PropValue::MultiString(values.iter().map(|v| strip(codepage::utf16le(v))).collect()),
                _ => PropValue::MultiString(values.iter().map(|v| strip(codepage::decode(cp, v))).collect()),
            }
        }
        _ => PropValue::Other { ty, raw: data },
    })
}

/// Child storages of `base` whose names start with `prefix`, ordered by their hexadecimal number.
fn numbered_children(cf: &Cf, base: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(entries) = cf.read_storage(base) else { return Vec::new() };
    let mut found: Vec<(u32, PathBuf)> = entries
        .filter(|e| e.is_storage())
        .filter_map(|e| Some((u32::from_str_radix(e.name().strip_prefix(prefix)?, 16).ok()?, base.join(e.name()))))
        .take(MAX_CHILDREN)
        .collect();
    found.sort();
    found.into_iter().map(|(_, p)| p).collect()
}

fn read_message(cf: &mut Cf, base: &Path, header: usize, names: &[Option<NamedKey>], depth: usize) -> Item {
    let (obj, cp) = read_object(cf, base, header, names, 0);
    let mut item = Item { props: obj.props, ..Item::default() };
    for (key, v) in obj.named {
        match v {
            PropValue::MultiString(list) if key == NamedKey::keywords() => item.categories = list,
            v => item.named.push((key, v)),
        }
    }
    for path in numbered_children(cf, base, RECIP_PREFIX) {
        item.recipients.push(read_object(cf, &path, HEADER_CHILD, names, cp).0.props);
    }
    for path in numbered_children(cf, base, ATTACH_PREFIX) {
        let mut props = read_object(cf, &path, HEADER_CHILD, names, cp).0.props;
        let data = match props.0.remove(&pid::ATTACH_DATA) {
            Some(PropValue::Binary(b)) => b,
            _ => Vec::new(),
        };
        let inner = path.join(EMBEDDED_STORAGE);
        // The storage under an attachment is a message, or, with attach method 6, an object some
        // other program laid out ([MS-OXMSG] 2.2.2.2). That one is kept whole, as a compound
        // file of its own, which is also how a store hands it over.
        let custom = props.i32(pid::ATTACH_METHOD) == Some(6);
        let (data, embedded) = if !cf.is_storage(&inner) {
            (data, None)
        } else if custom {
            (crate::ole::pack(cf, &inner).unwrap_or_default(), None)
        } else if depth < MAX_DEPTH {
            (data, Some(Box::new(read_message(cf, &inner, HEADER_EMBEDDED, names, depth + 1))))
        } else {
            (data, None)
        };
        item.attachments.push(Attachment { props, data, embedded });
    }
    item.normalise_html();
    item
}
