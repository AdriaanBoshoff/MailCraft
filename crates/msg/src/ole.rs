//! An attached object of another program (attach method 6, [MS-OXCMSG] 2.2.2.9): a compound file
//! whose streams that program lays out ([MS-OXMSG] 2.2.2.2). Nothing in it is run or handed to
//! any program. What is read is what [MS-OLEDS] gives every such object: the pictures it keeps of
//! itself (2.3.4, OLEPresentationStream), its data when that came from the older OLE and is a
//! picture file as it stands (2.3.6, OLENativeStream), and what it calls itself (2.3.8,
//! CompObjStream).

use crate::MsgError;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};

/// Storages and streams one object is copied with, how deep, and how many bytes in all.
const MAX_ENTRIES: usize = 4_096;
const MAX_DEPTH: usize = 16;
const MAX_BYTES: u64 = 512 * 1024 * 1024;
/// No picture stream is read past this size, and no more pictures than this are offered.
const MAX_PICTURE: u64 = 64 * 1024 * 1024;
const MAX_PICTURES: usize = 8;
/// Presentation streams are numbered 000 to 999; this many of them are looked for.
const MAX_PRESENTATIONS: usize = 16;
/// Characters kept of what an object calls itself.
const MAX_KIND: usize = 64;

const COMP_OBJ: &str = "\u{1}CompObj";
const NATIVE: &str = "\u{1}Ole10Native";
const PRESENTATION: &str = "\u{2}OlePres";

/// Standard clipboard formats ([MS-OLEDS] 1.3.5.1).
const CF_METAFILEPICT: u32 = 3;
const CF_DIB: u32 = 8;
const CF_ENHMETAFILE: u32 = 14;

/// What a picture's bytes are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A picture file as it stands: PNG, JPEG, GIF or BMP.
    File,
    /// A bitmap without a file's header ([MS-WMF] 2.2.2.9).
    Bitmap,
    /// A Windows metafile ([MS-WMF]) and an enhanced one ([MS-EMF]).
    Metafile,
    Enhanced,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub shape: Shape,
    pub data: Vec<u8>,
}

/// What can be told of an attached object without the program that made it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Object {
    /// What the object calls itself, such as the name of a kind of drawing.
    pub kind: Option<String>,
    /// Pictures of the object, the best first: its own data when that is a picture file, then
    /// the pictures it keeps for showing.
    pub pictures: Vec<Picture>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn size_at(b: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32_at(b, at)?).ok()
}

/// Read an attached object. `None` when the bytes are not a compound file.
pub fn object(storage: &[u8]) -> Option<Object> {
    // The container library is not ours; a panic inside it on a damaged file must not escape.
    std::panic::catch_unwind(|| object_inner(storage)).ok().flatten()
}

fn object_inner(storage: &[u8]) -> Option<Object> {
    let mut cf = cfb::CompoundFile::open(Cursor::new(storage)).ok()?;
    let root = PathBuf::from("/");
    let mut stream = |name: &str| {
        let mut s = cf.open_stream(root.join(name)).ok()?;
        if s.len() > MAX_PICTURE {
            return None;
        }
        let mut out = Vec::new();
        s.read_to_end(&mut out).ok()?;
        Some(out)
    };
    let kind = stream(COMP_OBJ).and_then(|b| kind(&b));
    let mut pictures = Vec::new();
    if let Some(native) = stream(NATIVE).and_then(|b| native(&b)) {
        pictures.push(native);
    }
    for n in 0..MAX_PRESENTATIONS {
        // The streams are numbered without a gap, so the first one missing ends them.
        let Some(bytes) = stream(&format!("{PRESENTATION}{n:03}")) else { break };
        if pictures.len() < MAX_PICTURES
            && let Some(picture) = presentation(&bytes)
        {
            pictures.push(picture);
        }
    }
    Some(Object { kind, pictures })
}

/// [MS-OLEDS] 2.3.8, CompObjStream: a header of 28 bytes, then the name the object goes by, a
/// string with its length before it (2.1.4).
fn kind(b: &[u8]) -> Option<String> {
    let len = size_at(b, 28)?;
    let text = b.get(32..32usize.checked_add(len)?)?;
    let name: String = text.iter().take_while(|c| **c != 0).map(|c| char::from(*c)).filter(|c| !c.is_control()).take(MAX_KIND).collect();
    let name = name.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// [MS-OLEDS] 2.3.6, OLENativeStream: a size, then the object's data. Kept only when the data is
/// a picture file by its first bytes; whether it is a sound one is for the decoder to say.
fn native(b: &[u8]) -> Option<Picture> {
    let data = b.get(4..4usize.checked_add(size_at(b, 0)?)?)?;
    let file =
        data.starts_with(b"\x89PNG\r\n\x1a\n") || data.starts_with(&[0xFF, 0xD8, 0xFF]) || data.starts_with(b"GIF8") || data.starts_with(b"BM");
    file.then(|| Picture { shape: Shape::File, data: data.to_vec() })
}

/// [MS-OLEDS] 2.3.4, OLEPresentationStream.
fn presentation(b: &[u8]) -> Option<Picture> {
    // 2.3.1, ClipboardFormatOrAnsiString: a standard format behind one of two markers. A format
    // with a name is some program's own and is not read.
    let shape = match u32_at(b, 0)? {
        0xFFFF_FFFF | 0xFFFF_FFFE => match u32_at(b, 4)? {
            CF_DIB => Shape::Bitmap,
            CF_METAFILEPICT => Shape::Metafile,
            CF_ENHMETAFILE => Shape::Enhanced,
            _ => return None,
        },
        _ => return None,
    };
    // The size of the target device, which counts its own four bytes; then the device.
    let device = size_at(b, 8)?;
    if device < 4 {
        return None;
    }
    // Aspect, Lindex, Advf, Reserved1, Width and Height: six fields of four bytes.
    let at = 8usize.checked_add(device)?.checked_add(24)?;
    let start = at.checked_add(4)?;
    let data = b.get(start..start.checked_add(size_at(b, at)?)?)?;
    (!data.is_empty()).then(|| Picture { shape, data: data.to_vec() })
}

/// An object that is one picture, as our own samples and tests have them: what it calls itself
/// and a bitmap (without a file's header) it keeps for showing.
pub fn picture_object(kind: &str, bitmap: &[u8]) -> Result<Vec<u8>, MsgError> {
    let mut cf = cfb::CompoundFile::create_with_version(cfb::Version::V3, Cursor::new(Vec::new()))?;
    let len = |n: usize| u32::try_from(n).map(u32::to_le_bytes).map_err(|_| MsgError::Damaged("the object is too large".into()));
    // CompObjStream: the header (its bytes are free), the name, no format and no reserved string.
    let name: Vec<u8> = kind.chars().filter(char::is_ascii).take(MAX_KIND).map(|c| c as u8).chain([0]).collect();
    let mut comp = vec![0u8; 28];
    comp.extend_from_slice(&len(name.len())?);
    comp.extend_from_slice(&name);
    comp.extend_from_slice(&[0u8; 8]);
    put(&mut cf, &Path::new("/").join(COMP_OBJ), &comp)?;
    // OLEPresentationStream: CF_DIB, no target device, the content aspect, no size hints.
    let mut pres = Vec::new();
    for field in [0xFFFF_FFFF, CF_DIB, 4, 1, 0xFFFF_FFFF, 0, 0, 0, 0] {
        pres.extend_from_slice(&u32::to_le_bytes(field));
    }
    pres.extend_from_slice(&len(bitmap.len())?);
    pres.extend_from_slice(bitmap);
    put(&mut cf, &Path::new("/").join(format!("{PRESENTATION}000")), &pres)?;
    cf.flush()?;
    Ok(cf.into_inner().into_inner())
}

fn put<F: Read + Write + Seek>(cf: &mut cfb::CompoundFile<F>, path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut s = cf.create_stream(path)?;
    s.write_all(data)?;
    s.flush()
}

/// What is left of the limits while one object is copied.
pub(crate) struct Budget {
    entries: usize,
    bytes: u64,
}

impl Budget {
    pub(crate) fn new() -> Self {
        Self { entries: MAX_ENTRIES, bytes: MAX_BYTES }
    }
}

fn too_large() -> std::io::Error {
    std::io::Error::other("an attached object is too large to copy")
}

/// Copy all that is under the storage `at` of one compound file into the storage `into` of
/// another, which must exist: every stream and storage, with each storage's class and state.
pub(crate) fn copy<R: Read + Seek, W: Read + Write + Seek>(
    from: &mut cfb::CompoundFile<R>,
    at: &Path,
    to: &mut cfb::CompoundFile<W>,
    into: &Path,
    budget: &mut Budget,
    depth: usize,
) -> std::io::Result<()> {
    if depth > MAX_DEPTH {
        return Err(too_large());
    }
    let here = from.entry(at)?;
    to.set_storage_clsid(into, *here.clsid())?;
    to.set_state_bits(into, here.state_bits())?;
    let children: Vec<(String, bool, u64)> =
        from.read_storage(at)?.take(MAX_ENTRIES + 1).map(|e| (e.name().to_string(), e.is_storage(), e.len())).collect();
    for (name, storage, len) in children {
        budget.entries = budget.entries.checked_sub(1).ok_or_else(too_large)?;
        let (source, target) = (at.join(&name), into.join(&name));
        if storage {
            to.create_storage(&target)?;
            copy(from, &source, to, &target, budget, depth + 1)?;
        } else {
            budget.bytes = budget.bytes.checked_sub(len).ok_or_else(too_large)?;
            let mut data = Vec::new();
            from.open_stream(&source)?.take(len).read_to_end(&mut data)?;
            put(to, &target, &data)?;
        }
    }
    Ok(())
}

/// The storage `at` as a compound file of its own.
pub(crate) fn pack<R: Read + Seek>(from: &mut cfb::CompoundFile<R>, at: &Path) -> Option<Vec<u8>> {
    let mut cf = cfb::CompoundFile::create_with_version(cfb::Version::V3, Cursor::new(Vec::new())).ok()?;
    copy(from, at, &mut cf, Path::new("/"), &mut Budget::new(), 0).ok()?;
    cf.flush().ok()?;
    Some(cf.into_inner().into_inner())
}
