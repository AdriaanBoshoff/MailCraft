//! Letters as shapes: the outlines of a line of text, for drawing it where text cannot be set
//! as text (a picture that is kept as a drawing). Each character is one glyph, set left to
//! right by the font's own widths; text that needs more than that to be read (joined letters,
//! marks placed on others, right-to-left lines) is not made into shapes at all.
//!
//! The text and the font's name come from a message and are hostile: how many letters and how
//! many points a letter has are capped, and a name is only ever looked up in the [`Library`].

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::{Face, Library, wanted};

/// Letters in one line.
const MAX_LETTERS: usize = 1024;
/// Glyphs whose outlines are read at once.
const MAX_SHAPES: usize = 4096;
/// Points round one letter.
const MAX_POINTS: usize = 20_000;
/// Straight pieces a curve of a letter is cut into.
const CURVE: usize = 6;
/// Faces a [`Scribe`] remembers, and bytes of them.
const MAX_KEPT: usize = 64;
const MAX_KEPT_BYTES: usize = 64 << 20;
/// How far an upright face leans when it stands in for an italic one, for each unit of height.
const SLANT: f32 = 0.2;

/// A closed line round part of a letter: the em is 1, the letter starts at 0 on the line it
/// stands on, and rows run downwards.
pub type Ring = Vec<(f32, f32)>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Letter {
    /// What is inside these, wound either way, is ink.
    pub rings: Vec<Ring>,
    /// How far the next letter is from this one.
    pub advance: f32,
}

/// One line of text, a letter for each of its characters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Letters {
    pub each: Vec<Letter>,
    /// How far the font reaches above the line it stands on, and below it.
    pub ascent: f32,
    pub descent: f32,
}

/// Something that knows fonts and can give the shapes of text in them.
pub trait Lettering {
    /// `text` in the font a drawing names. `None` when no font answers, when the font lacks one
    /// of the characters, and for text that single letters set side by side do not spell.
    fn letters(&self, family: &str, bold: bool, italic: bool, text: &str) -> Option<Letters>;
}

/// Characters that are joined to their neighbours, stand on them or run right to left.
fn shaped(ch: char) -> bool {
    matches!(
        u32::from(ch),
        0x0300..=0x036F
            | 0x0590..=0x109F
            | 0x1780..=0x18AF
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x200C..=0x200F
            | 0x202A..=0x202E
            | 0x2066..=0x2069
            | 0x20D0..=0x20FF
            | 0xA800..=0xABFF
            | 0xFB1D..=0xFDFF
            | 0xFE00..=0xFE0F
            | 0xFE20..=0xFE2F
            | 0xFE70..=0xFEFC
            | 0x10800..=0x11FFF
            | 0x1E800..=0x1EFFF
            | 0xE0000..=0xE0FFF
    )
}

/// Takes down a glyph's outline as rings of points.
struct Tracer {
    rings: Vec<Ring>,
    /// Font units to the em, and the lean.
    by: f32,
    slant: f32,
    at: (f32, f32),
    points: usize,
}

impl Tracer {
    fn add(&mut self, x: f32, y: f32) {
        self.at = (x, y);
        self.points = self.points.saturating_add(1);
        if self.points <= MAX_POINTS
            && let Some(ring) = self.rings.last_mut()
        {
            ring.push(((x + y * self.slant) / self.by, -y / self.by));
        }
    }
}

impl OutlinePen for Tracer {
    fn move_to(&mut self, x: f32, y: f32) {
        self.rings.push(Vec::new());
        self.add(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.add(x, y);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let from = self.at;
        for i in 1..=CURVE {
            let t = i as f32 / CURVE as f32;
            let s = 1.0 - t;
            self.add(s * s * from.0 + 2.0 * s * t * cx + t * t * x, s * s * from.1 + 2.0 * s * t * cy + t * t * y);
        }
    }

    fn curve_to(&mut self, ax: f32, ay: f32, bx: f32, by: f32, x: f32, y: f32) {
        let from = self.at;
        for i in 1..=CURVE {
            let t = i as f32 / CURVE as f32;
            let s = 1.0 - t;
            let part = |p: f32, a: f32, b: f32, q: f32| s * s * s * p + 3.0 * s * s * t * a + 3.0 * s * t * t * b + t * t * t * q;
            self.add(part(from.0, ax, bx, x), part(from.1, ay, by, y));
        }
    }

    fn close(&mut self) {}
}

/// The shapes of `text` in the font at `index` of the file `data`; leaning when `slant`.
pub fn letters(data: &[u8], index: u32, slant: bool, text: &str) -> Option<Letters> {
    let font = FontRef::from_index(data, index).ok()?;
    let whole = font.metrics(Size::unscaled(), LocationRef::default());
    if whole.units_per_em == 0 {
        return None;
    }
    let by = f32::from(whole.units_per_em);
    let (map, outlines, widths) = (font.charmap(), font.outline_glyphs(), font.glyph_metrics(Size::unscaled(), LocationRef::default()));
    let mut each = Vec::new();
    for ch in text.chars() {
        if each.len() >= MAX_LETTERS || shaped(ch) {
            return None;
        }
        // A character that moves the line on and draws nothing where a file has one.
        if ch.is_control() {
            each.push(Letter::default());
            continue;
        }
        let glyph = match map.map(ch).filter(|glyph| glyph.to_u32() != 0) {
            Some(glyph) => glyph,
            // A kind of space the font has no glyph for is as wide as its space.
            None if ch.is_whitespace() => map.map(' ').filter(|glyph| glyph.to_u32() != 0)?,
            None => return None,
        };
        let advance = widths.advance_width(glyph)? / by;
        let mut tracer = Tracer { rings: Vec::new(), by, slant: if slant { SLANT } else { 0.0 }, at: (0.0, 0.0), points: 0 };
        if !ch.is_whitespace()
            && let Some(outline) = outlines.get(glyph)
        {
            outline.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut tracer).ok()?;
        }
        if tracer.points > MAX_POINTS || !advance.is_finite() {
            return None;
        }
        each.push(Letter { rings: tracer.rings, advance });
    }
    let (ascent, descent) = (whole.ascent / by, -whole.descent / by);
    (ascent.is_finite() && descent.is_finite() && ascent + descent > 0.0).then_some(Letters { each, ascent, descent })
}

/// The outlines of `glyphs` of the font at `index` of the file `data`, in ems from each
/// glyph's start on its baseline, rows running downwards; leaning when `slant`. A glyph that
/// draws nothing, or is not in the font, has no rings. For text that was laid out as glyphs.
pub fn shapes(data: &[u8], index: u32, slant: bool, glyphs: &[u16]) -> Option<Vec<Vec<Ring>>> {
    let font = FontRef::from_index(data, index).ok()?;
    let units = font.metrics(Size::unscaled(), LocationRef::default()).units_per_em;
    if units == 0 {
        return None;
    }
    let outlines = font.outline_glyphs();
    let mut each = Vec::new();
    for glyph in glyphs.iter().take(MAX_SHAPES) {
        let mut tracer = Tracer { rings: Vec::new(), by: f32::from(units), slant: if slant { SLANT } else { 0.0 }, at: (0.0, 0.0), points: 0 };
        let drawn = outlines
            .get(GlyphId::new(u32::from(*glyph)))
            .is_some_and(|outline| outline.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut tracer).is_ok());
        each.push(if drawn && tracer.points <= MAX_POINTS { tracer.rings } else { Vec::new() });
    }
    Some(each)
}

/// One face, whatever font is asked for: for where the font is known beforehand.
impl Lettering for Face {
    fn letters(&self, _family: &str, _bold: bool, _italic: bool, text: &str) -> Option<Letters> {
        letters(&self.data, self.index, false, text)
    }
}

/// A face that was found for a name, and whether it leans to stand in for an italic one.
type Found = Option<(Arc<Face>, bool)>;

/// Gives the shapes of text in the fonts of a [`Library`], with open fonts of the same measures
/// standing in for proprietary ones that are not installed ([`wanted`]). Faces that were looked
/// up are remembered.
pub struct Scribe {
    library: OnceLock<Library>,
    kept: Mutex<BTreeMap<(String, bool, bool), Found>>,
}

impl Scribe {
    /// The fonts installed on this machine. They are listed when first needed, which takes a
    /// moment.
    pub const fn system() -> Self {
        Self { library: OnceLock::new(), kept: Mutex::new(BTreeMap::new()) }
    }

    pub fn with(library: Library) -> Self {
        let scribe = Self::system();
        // A new scribe has no library yet, so this one is taken.
        let _ = scribe.library.set(library);
        scribe
    }

    pub fn library(&self) -> &Library {
        self.library.get_or_init(Library::system)
    }

    /// The face for a name: bold and italic where the family has them; else the upright face
    /// leaning for italic, and the regular one for bold, which is still the words.
    fn face(&self, family: &str, bold: bool, italic: bool) -> Found {
        let names = wanted(family);
        let key = (names.first()?.to_lowercase(), bold, italic);
        if let Some(known) = self.kept.lock().ok()?.get(&key) {
            return known.clone();
        }
        let library = self.library();
        let name = names.iter().find(|name| library.has(name))?;
        let found = [(bold, italic), (bold, false), (false, italic), (false, false)]
            .into_iter()
            .find_map(|(b, i)| library.face(name, b, i).map(|face| (Arc::new(face), italic && !i)));
        let mut kept = self.kept.lock().ok()?;
        let bytes: usize = kept.values().flatten().map(|(face, _)| face.data.len()).sum();
        if kept.len() >= MAX_KEPT || bytes > MAX_KEPT_BYTES {
            kept.clear();
        }
        kept.insert(key, found.clone());
        found
    }
}

impl Lettering for Scribe {
    fn letters(&self, family: &str, bold: bool, italic: bool, text: &str) -> Option<Letters> {
        let (face, slant) = self.face(family, bold, italic)?;
        letters(&face.data, face.index, slant, text)
    }
}
