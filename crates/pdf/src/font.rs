//! A TrueType font as a PDF file carries it: its measures, and a font file rebuilt from its
//! tables with only the glyphs that are used.
//!
//! Table layouts are from the OpenType specification (`head`, `hhea`, `hmtx`, `maxp`, `loca`,
//! `glyf`, `post`). Every offset and count in a font file is read as hostile.

use std::collections::BTreeSet;

use openmailclient_fonts::Embedding;
use skrifa::raw::TableProvider;
use skrifa::raw::types::Tag;
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};

/// Glyphs followed from one glyph that is made of others.
const MAX_PARTS: usize = 4096;
/// The largest rebuilt font file.
const MAX_FILE: usize = 32 << 20;

/// What the PDF's font dictionaries say about a face.
pub(crate) struct Measures {
    pub name: String,
    pub units: f32,
    pub bounds: [i16; 4],
    pub ascent: i16,
    pub descent: i16,
    pub italic_angle: f32,
    pub fixed_pitch: bool,
    /// How far each glyph moves on, in font units, by glyph number.
    pub advances: Vec<u16>,
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*data.get(at)?, *data.get(at.checked_add(1)?)?]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*data.get(at)?, *data.get(at.checked_add(1)?)?, *data.get(at.checked_add(2)?)?, *data.get(at.checked_add(3)?)?]))
}

fn table<'a>(font: &FontRef<'a>, tag: &[u8; 4]) -> Option<&'a [u8]> {
    font.data_for_tag(Tag::new(tag)).map(|data| data.as_bytes())
}

/// How far each glyph moves on (`hmtx`: the glyphs past the last full entry repeat its advance).
pub(crate) fn advances(hhea: &[u8], hmtx: &[u8], glyphs: usize) -> Vec<u16> {
    let full = usize::from(be16(hhea, 34).unwrap_or(0));
    let mut out = Vec::with_capacity(glyphs.min(usize::from(u16::MAX)));
    let mut last = 0u16;
    for glyph in 0..glyphs.min(usize::from(u16::MAX)) {
        if glyph < full
            && let Some(advance) = be16(hmtx, glyph * 4)
        {
            last = advance;
        }
        out.push(last);
    }
    out
}

pub(crate) fn measures(data: &[u8], index: u32) -> Option<Measures> {
    let font = FontRef::from_index(data, index).ok()?;
    let head = table(&font, b"head")?;
    let hhea = table(&font, b"hhea")?;
    let hmtx = table(&font, b"hmtx")?;
    let maxp = table(&font, b"maxp")?;
    let units = be16(head, 18).filter(|u| *u > 0)?;
    let side = |at: usize| be16(head, at).map(|v| v as i16);
    let post = table(&font, b"post");
    let name: String = font
        .localized_strings(StringId::POSTSCRIPT_NAME)
        .english_or_first()
        .map(|s| s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(60).collect())
        .unwrap_or_default();
    Some(Measures {
        name: if name.is_empty() { "Font".to_string() } else { name },
        units: f32::from(units),
        bounds: [side(36)?, side(38)?, side(40)?, side(42)?],
        ascent: be16(hhea, 4)? as i16,
        descent: be16(hhea, 6)? as i16,
        italic_angle: post.and_then(|p| be32(p, 4)).map_or(0.0, |fixed| fixed as i32 as f32 / 65536.0),
        fixed_pitch: post.and_then(|p| be32(p, 12)).is_some_and(|v| v != 0),
        advances: advances(hhea, hmtx, usize::from(be16(maxp, 4)?)),
    })
}

/// Where glyph `glyph` is in `glyf`, when it has an outline there.
fn place(loca: &[u8], long: bool, glyf_len: usize, glyph: usize) -> Option<(usize, usize)> {
    let at = |i: usize| -> Option<usize> {
        if long {
            be32(loca, i.checked_mul(4)?).and_then(|v| usize::try_from(v).ok())
        } else {
            be16(loca, i.checked_mul(2)?).map(|v| usize::from(v) * 2)
        }
    };
    let (from, to) = (at(glyph)?, at(glyph.checked_add(1)?)?);
    (from < to && to <= glyf_len).then_some((from, to))
}

/// The glyphs a glyph is made of ("glyf", composite glyph description), added to `found`.
fn parts(outline: &[u8], found: &mut Vec<u16>) {
    if be16(outline, 0).is_none_or(|contours| (contours as i16) >= 0) {
        return;
    }
    let mut at = 10usize;
    for _ in 0..MAX_PARTS {
        let (Some(flags), Some(glyph)) = (be16(outline, at), be16(outline, at.saturating_add(2))) else {
            return;
        };
        found.push(glyph);
        let mut step = 4 + if flags & 0x0001 != 0 { 4 } else { 2 };
        if flags & 0x0008 != 0 {
            step += 2;
        } else if flags & 0x0040 != 0 {
            step += 4;
        } else if flags & 0x0080 != 0 {
            step += 8;
        }
        if flags & 0x0020 == 0 {
            return;
        }
        at = at.saturating_add(step);
    }
}

/// A font file holding the face's outlines for `used` and the glyphs they are made of; with
/// [`Embedding::Whole`], all its outlines. Glyph numbers stay as they are.
pub(crate) fn file(data: &[u8], index: u32, used: &BTreeSet<u16>, how: Embedding) -> Option<Vec<u8>> {
    let font = FontRef::from_index(data, index).ok()?;
    let head = table(&font, b"head")?;
    let maxp = table(&font, b"maxp")?;
    let loca = table(&font, b"loca")?;
    let glyf = table(&font, b"glyf")?;
    let glyphs = usize::from(be16(maxp, 4)?);
    let long = be16(head, 50)? != 0;

    let mut kept: BTreeSet<u16> = BTreeSet::new();
    match how {
        Embedding::Whole => kept.extend((0..glyphs).filter_map(|g| u16::try_from(g).ok())),
        Embedding::Subset => {
            let mut todo: Vec<u16> = used.iter().copied().chain([0]).collect();
            while let Some(glyph) = todo.pop() {
                if usize::from(glyph) >= glyphs || !kept.insert(glyph) {
                    continue;
                }
                if let Some((from, to)) = place(loca, long, glyf.len(), usize::from(glyph)) {
                    parts(glyf.get(from..to)?, &mut todo);
                }
            }
        }
    }

    let mut new_glyf: Vec<u8> = Vec::new();
    let mut new_loca: Vec<u8> = Vec::with_capacity((glyphs + 1) * 4);
    for glyph in 0..glyphs {
        new_loca.extend(u32::try_from(new_glyf.len()).ok()?.to_be_bytes());
        let outline = u16::try_from(glyph).ok().filter(|g| kept.contains(g)).and_then(|_| place(loca, long, glyf.len(), glyph));
        if let Some((from, to)) = outline {
            new_glyf.extend(glyf.get(from..to)?);
            while !new_glyf.len().is_multiple_of(4) {
                new_glyf.push(0);
            }
            if new_glyf.len() > MAX_FILE {
                return None;
            }
        }
    }
    new_loca.extend(u32::try_from(new_glyf.len()).ok()?.to_be_bytes());

    // `head` with long offsets into `loca`, and its checksum left to be worked out.
    let mut new_head = head.get(..54)?.to_vec();
    new_head.get_mut(8..12)?.copy_from_slice(&[0; 4]);
    new_head.get_mut(50..52)?.copy_from_slice(&1u16.to_be_bytes());

    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    for tag in [b"cvt ", b"fpgm"] {
        if let Some(data) = table(&font, tag) {
            tables.push((*tag, data.to_vec()));
        }
    }
    tables.push((*b"glyf", new_glyf));
    tables.push((*b"head", new_head));
    tables.push((*b"hhea", table(&font, b"hhea")?.to_vec()));
    tables.push((*b"hmtx", table(&font, b"hmtx")?.to_vec()));
    tables.push((*b"loca", new_loca));
    tables.push((*b"maxp", maxp.to_vec()));
    if let Some(data) = table(&font, b"prep") {
        tables.push((*b"prep", data.to_vec()));
    }
    assemble(&tables)
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        for (to, from) in word.iter_mut().zip(chunk) {
            *to = *from;
        }
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// A font file of `tables`, given in the order of their tags ("Font file": table directory).
fn assemble(tables: &[([u8; 4], Vec<u8>)]) -> Option<Vec<u8>> {
    let count = u16::try_from(tables.len()).ok().filter(|n| *n > 0)?;
    let total: usize = tables.iter().map(|(_, data)| data.len().div_ceil(4) * 4).sum::<usize>().checked_add(12 + tables.len() * 16)?;
    if total > MAX_FILE {
        return None;
    }
    let mut out: Vec<u8> = Vec::with_capacity(total);
    out.extend(0x0001_0000u32.to_be_bytes());
    out.extend(count.to_be_bytes());
    let power = 15u16.saturating_sub(count.leading_zeros() as u16);
    let range = 16u16.checked_shl(u32::from(power))?;
    out.extend(range.to_be_bytes());
    out.extend(power.to_be_bytes());
    out.extend(count.saturating_mul(16).saturating_sub(range).to_be_bytes());
    let mut at = 12 + tables.len() * 16;
    let mut head_at = None;
    for (tag, data) in tables {
        if tag == b"head" {
            head_at = Some(at);
        }
        out.extend(tag);
        out.extend(checksum(data).to_be_bytes());
        out.extend(u32::try_from(at).ok()?.to_be_bytes());
        out.extend(u32::try_from(data.len()).ok()?.to_be_bytes());
        at += data.len().div_ceil(4) * 4;
    }
    for (_, data) in tables {
        out.extend(data);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    // The whole file sums to a fixed number once `head` holds what is missing from it.
    let adjust = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    if let Some(slot) = head_at.and_then(|at| out.get_mut(at + 8..at + 12)) {
        slot.copy_from_slice(&adjust.to_be_bytes());
    }
    Some(out)
}
