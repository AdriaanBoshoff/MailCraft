//! The undo file of a save that changes a data file where it is. Such a save writes only
//! into space the file calls free and past its end, then its maps and last of all its
//! header; nothing that holds mail is written over. Before the first of those writes, the
//! bytes that will be written over (the header, the last section with its allocation map, a
//! free map) are put in a file of our own beside the data file, with the length the data
//! file had, and that file is forced to disk. Putting them back and cutting the file to that
//! length gives the data file as it was, byte for byte.
//!
//! The undo file is there for as long as the save is not done: taking it away is what ends
//! the save. One that is found when a data file is opened is from a save that was cut short;
//! the data file is then read through it, as it was before that save, and is put back the
//! next time a save is asked for. Opening writes nothing.
//!
//! The file is our own: a mark, the old length, two pages the save leaves alone (to know
//! the data file by), the parts, and a checksum of all of it. What is read from it is
//! treated as hostile.

use crate::write::crc;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"OMCUNDO1";
/// An undo file is a header, one section and a page or two.
const MAX_UNDO: u64 = 16 << 20;
const MAX_PARTS: usize = 16;
/// The bytes a data file is known by ([`crate::journal::ident`] takes as many).
const KNOWN_BY: usize = 1024;
const PAGE: usize = 512;

/// Where the undo file of a data file is kept: beside it.
pub fn undo_beside(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(std::ffi::OsString::from).unwrap_or_default();
    name.push(".openmailclient-undo");
    path.with_file_name(name)
}

/// What a data file held where a save writes, before it wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Undo {
    /// How long the data file was.
    pub len: u64,
    /// Two pages the save does not write over, and the sum of each: the file is known by them.
    pub proof: [(u64, u32); 2],
    /// The start of the header the save is about to write, once it is known. A data file
    /// whose header is neither the old one nor this was changed by someone else since.
    pub next: Option<Vec<u8>>,
    /// Where, and what was there. The first part is the start of the file.
    pub parts: Vec<(u64, Vec<u8>)>,
}

impl Undo {
    fn encode(&self) -> Option<Vec<u8>> {
        let mut out = MAGIC.to_vec();
        out.extend(self.len.to_le_bytes());
        for (at, sum) in self.proof {
            out.extend(at.to_le_bytes());
            out.extend(sum.to_le_bytes());
        }
        let next = self.next.as_deref().unwrap_or_default();
        out.extend(u32::try_from(next.len()).ok()?.to_le_bytes());
        out.extend(next);
        out.extend(u32::try_from(self.parts.len()).ok()?.to_le_bytes());
        for (at, bytes) in &self.parts {
            out.extend(at.to_le_bytes());
            out.extend(u32::try_from(bytes.len()).ok()?.to_le_bytes());
            out.extend(bytes);
        }
        let sum = crc(&out);
        out.extend(sum.to_le_bytes());
        Some(out)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        let body = bytes.get(..bytes.len().checked_sub(4)?)?;
        if u32::from_le_bytes(bytes.get(body.len()..)?.try_into().ok()?) != crc(body) {
            return None;
        }
        let mut at = 0usize;
        let mut take = |n: usize| -> Option<&[u8]> {
            let part = body.get(at..at.checked_add(n)?)?;
            at += n;
            Some(part)
        };
        if take(8)? != MAGIC {
            return None;
        }
        let len = u64::from_le_bytes(take(8)?.try_into().ok()?);
        let mut proof = [(0u64, 0u32); 2];
        for p in &mut proof {
            *p = (u64::from_le_bytes(take(8)?.try_into().ok()?), u32::from_le_bytes(take(4)?.try_into().ok()?));
            if p.0.checked_add(PAGE as u64)? > len {
                return None;
            }
        }
        let next = match u32::from_le_bytes(take(4)?.try_into().ok()?) as usize {
            0 => None,
            KNOWN_BY => Some(take(KNOWN_BY)?.to_vec()),
            _ => return None,
        };
        let count = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
        if count == 0 || count > MAX_PARTS {
            return None;
        }
        let mut parts = Vec::with_capacity(count);
        for _ in 0..count {
            let to = u64::from_le_bytes(take(8)?.try_into().ok()?);
            let n = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
            // Nothing is put back past the length the file had.
            if to.checked_add(n as u64)? > len {
                return None;
            }
            parts.push((to, take(n)?.to_vec()));
        }
        if at != body.len() || parts.first().is_none_or(|(to, bytes)| *to != 0 || bytes.len() < KNOWN_BY) {
            return None;
        }
        Some(Self { len, proof, next, parts })
    }

    /// Put the undo file at `at`, whole or not at all, and force it to disk.
    pub(crate) fn write(&self, at: &Path) -> std::io::Result<()> {
        let bytes = self.encode().ok_or_else(|| std::io::Error::other("an undo file that is too large"))?;
        let mut tmp = at.as_os_str().to_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        let written = (|| {
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&tmp, at)
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        // Where a folder can be forced to disk, the new name is.
        if let Some(dir) = at.parent()
            && let Ok(dir) = std::fs::File::open(dir)
        {
            let _ = dir.sync_all();
        }
        written
    }

    /// The undo file at `at`; nothing when there is none or it is not sound.
    pub(crate) fn read(at: &Path) -> Option<Self> {
        let file = std::fs::File::open(at).ok()?;
        if file.metadata().ok()?.len() > MAX_UNDO {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(MAX_UNDO).read_to_end(&mut bytes).ok()?;
        Self::decode(&bytes)
    }

    /// Whether `file` is the data file this was taken of, in a state the save it belongs to
    /// can have left: at least as long, the two pages as they were, and each half of the
    /// header's start as it was or as the save was going to write it.
    pub(crate) fn fits<R: Read + Seek>(&self, file: &mut R) -> bool {
        if !file.seek(SeekFrom::End(0)).is_ok_and(|len| len >= self.len) {
            return false;
        }
        let mut read = |at: u64, n: usize| -> Option<Vec<u8>> {
            let mut buf = vec![0u8; n];
            file.seek(SeekFrom::Start(at)).ok()?;
            file.read_exact(&mut buf).ok()?;
            Some(buf)
        };
        let Some(start) = read(0, KNOWN_BY) else { return false };
        let Some(was) = self.parts.first().and_then(|p| p.1.get(..KNOWN_BY)) else { return false };
        let halves = |b: &[u8], i: usize| b.get(i * PAGE..(i + 1) * PAGE).map(<[u8]>::to_vec);
        let known = (0..KNOWN_BY / PAGE).all(|i| {
            let has = halves(&start, i);
            has == halves(was, i) || self.next.as_deref().is_some_and(|next| has == halves(next, i))
        });
        known && self.proof.iter().all(|(at, sum)| read(*at, PAGE).is_some_and(|page| crc(&page) == *sum))
    }

    /// Put the data file back as it was: every part where it came from, and the file cut to
    /// the length it had. Doing it twice, or again after it was cut short, does no harm.
    pub(crate) fn restore(&self, file: &mut std::fs::File) -> std::io::Result<()> {
        // The header goes back last: until then the file is still known as the save's.
        for (at, bytes) in self.parts.iter().rev() {
            file.seek(SeekFrom::Start(*at))?;
            file.write_all(bytes)?;
        }
        file.set_len(self.len)?;
        file.sync_all()
    }

    /// What the data file was known by before the save: its length and the sum of its start.
    pub(crate) fn ident(&self) -> Option<(u64, u32)> {
        Some((self.len, crc(self.parts.first()?.1.get(..KNOWN_BY)?)))
    }

    /// The data file as it was, read through this.
    pub(crate) fn over<R: Read + Seek>(&self, inner: R) -> Under<R> {
        Under { inner, undo: self.clone(), pos: 0 }
    }
}

/// A data file read as it was before a save that was cut short: the parts the undo file
/// holds come from there, the rest from the file, and nothing past the old length is seen.
pub(crate) struct Under<R> {
    inner: R,
    undo: Undo,
    pos: u64,
}

impl<R: Read + Seek> Read for Under<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.undo.len.saturating_sub(self.pos);
        let mut want = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        if want == 0 {
            return Ok(0);
        }
        for (at, bytes) in &self.undo.parts {
            let end = at.saturating_add(bytes.len() as u64);
            if (*at..end).contains(&self.pos) {
                let from = usize::try_from(self.pos - at).unwrap_or(usize::MAX);
                let part = bytes.get(from..).unwrap_or_default();
                let n = want.min(part.len());
                if let (Some(to), Some(part)) = (buf.get_mut(..n), part.get(..n)) {
                    to.copy_from_slice(part);
                }
                self.pos += n as u64;
                return Ok(n);
            }
            if *at > self.pos {
                want = want.min(usize::try_from(at - self.pos).unwrap_or(usize::MAX));
            }
        }
        self.inner.seek(SeekFrom::Start(self.pos))?;
        let n = self.inner.read(buf.get_mut(..want).unwrap_or_default())?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl<R: Read + Seek> Seek for Under<R> {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(by) => self.undo.len.checked_add_signed(by),
            SeekFrom::Current(by) => self.pos.checked_add_signed(by),
        };
        self.pos = pos.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "a place before the start of the file"))?;
        Ok(self.pos)
    }
}
