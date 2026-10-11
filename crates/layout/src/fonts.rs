//! The faces a layout is set in: found by name among the installed fonts, read once, and asked
//! for glyphs and measures. Table layouts are from the OpenType specification (`head`, `hhea`,
//! `hmtx`, `maxp`); every number in a font file is read as hostile.

use std::collections::HashMap;
use std::sync::Arc;

use mailcraft_fonts::{Face, Library};
use skrifa::raw::TableProvider;
use skrifa::raw::types::Tag;
use skrifa::{FontRef, MetadataProvider};

/// Faces one layout may load.
const MAX_FACES: usize = 48;
/// Characters remembered per face.
const MAX_REMEMBERED: usize = 20_000;

/// Fixed-width fonts, for text that asks for one.
const MONO: &[&str] = &["Consolas", "Inconsolata", "Courier New", "Liberation Mono", "Cousine", "DejaVu Sans Mono", "Noto Sans Mono"];
/// Looked through, after the fonts a text names, for a character none of those has.
const LAST: &[&str] = &[
    "Segoe UI",
    "Arial",
    "Liberation Sans",
    "DejaVu Sans",
    "Noto Sans",
    "Segoe UI Symbol",
    "Noto Sans Symbols",
    "Segoe UI Emoji",
    "Microsoft YaHei",
    "Noto Sans CJK SC",
];

/// A face that is used: its file, and its measures in font units.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub data: Arc<[u8]>,
    pub index: u32,
    units: f32,
    ascent: f32,
    descent: f32,
    gap: f32,
    advances: Vec<u16>,
    chars: HashMap<char, u16>,
}

impl Loaded {
    /// Height above the baseline, depth below it and room between lines, at `size`.
    pub(crate) fn line(&self, size: f32) -> (f32, f32, f32) {
        let scale = size / self.units;
        (self.ascent * scale, self.descent * scale, self.gap * scale)
    }

    pub(crate) fn advance(&self, glyph: u16, size: f32) -> f32 {
        f32::from(self.advances.get(usize::from(glyph)).copied().unwrap_or(0)) * size / self.units
    }
}

/// A face picked for a text, and what the face lacks of the style asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pick {
    pub face: usize,
    pub bold: bool,
    pub slant: bool,
}

/// The font names one piece of text looks through, first choice first.
pub(crate) struct Chain {
    names: Vec<String>,
    bold: bool,
    italic: bool,
    picks: Vec<Option<Option<Pick>>>,
}

pub struct Fonts<'a> {
    library: &'a Library,
    given: Vec<(String, bool, bool, Face)>,
    faces: Vec<Loaded>,
    found: HashMap<(String, bool, bool), Option<usize>>,
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*data.get(at)?, *data.get(at.checked_add(1)?)?]))
}

fn load(face: Face) -> Option<Loaded> {
    mailcraft_fonts::embedding(&face.data, face.index)?;
    let font = FontRef::from_index(&face.data, face.index).ok()?;
    let table = |tag: &[u8; 4]| font.data_for_tag(Tag::new(tag)).map(|d| d.as_bytes());
    let (head, hhea, hmtx, maxp) = (table(b"head")?, table(b"hhea")?, table(b"hmtx")?, table(b"maxp")?);
    let units = be16(head, 18).filter(|u| *u >= 16)?;
    let (mut ascent, mut descent) = (f32::from(be16(hhea, 4)? as i16), -f32::from(be16(hhea, 6)? as i16));
    // A font that gives no heights, or absurd ones, is set as a common one is.
    let sane = |v: f32| (0.0..=f32::from(units) * 4.0).contains(&v);
    if !sane(ascent) || !sane(descent) || ascent + descent <= 0.0 {
        (ascent, descent) = (f32::from(units) * 0.8, f32::from(units) * 0.2);
    }
    let gap = f32::from(be16(hhea, 8)? as i16).clamp(0.0, f32::from(units));
    let glyphs = usize::from(be16(maxp, 4)?);
    let full = usize::from(be16(hhea, 34)?);
    let mut advances = Vec::with_capacity(glyphs);
    let mut last = 0u16;
    for glyph in 0..glyphs {
        if glyph < full
            && let Some(advance) = be16(hmtx, glyph * 4)
        {
            // No glyph moves on by more than a few ems.
            last = advance.min(units.saturating_mul(8));
        }
        advances.push(last);
    }
    Some(Loaded { data: face.data.into(), index: face.index, units: f32::from(units), ascent, descent, gap, advances, chars: HashMap::new() })
}

impl<'a> Fonts<'a> {
    pub fn new(library: &'a Library) -> Self {
        Self { library, given: Vec::new(), faces: Vec::new(), found: HashMap::new() }
    }

    /// Hand over a face under a family name, to be found before the installed ones.
    pub fn give(&mut self, family: &str, bold: bool, italic: bool, face: Face) {
        self.given.push((family.to_lowercase(), bold, italic, face));
    }

    pub fn faces(&self) -> &[Loaded] {
        &self.faces
    }

    pub(crate) fn face(&self, face: usize) -> Option<&Loaded> {
        self.faces.get(face)
    }

    /// The face of exactly this family and style, loaded once.
    fn exact(&mut self, family: &str, bold: bool, italic: bool) -> Option<usize> {
        let key = (family.to_lowercase(), bold, italic);
        if let Some(known) = self.found.get(&key) {
            return *known;
        }
        let face = match self.given.iter().find(|(name, b, i, _)| *name == key.0 && *b == bold && *i == italic) {
            Some((.., face)) => Some(face.clone()),
            None => self.library.face(family, bold, italic),
        };
        let loaded = if self.faces.len() < MAX_FACES { face.and_then(load) } else { None };
        let at = loaded.map(|loaded| {
            self.faces.push(loaded);
            self.faces.len() - 1
        });
        self.found.insert(key, at);
        at
    }

    /// A face of the family for the style: its own when it has one, else a plainer one that is
    /// then thickened or leant.
    fn pick(&mut self, family: &str, bold: bool, italic: bool) -> Option<Pick> {
        if let Some(face) = self.exact(family, bold, italic) {
            return Some(Pick { face, bold: false, slant: false });
        }
        if italic && let Some(face) = self.exact(family, bold, false) {
            return Some(Pick { face, bold: false, slant: true });
        }
        if bold && let Some(face) = self.exact(family, false, italic) {
            return Some(Pick { face, bold: true, slant: false });
        }
        if bold && italic {
            return self.exact(family, false, false).map(|face| Pick { face, bold: true, slant: true });
        }
        None
    }

    /// The names to look through for text that asks for `list` (as a message writes it; empty
    /// for none), fixed-width or not.
    pub(crate) fn chain(&self, list: &str, mono: bool, bold: bool, italic: bool) -> Chain {
        let mut names: Vec<String> = Vec::new();
        let listed = if list.trim().is_empty() { Vec::new() } else { mailcraft_fonts::wanted(list) };
        let own = listed.into_iter();
        let mono_names = MONO.iter().filter(|_| mono).map(|n| n.to_string());
        for name in own.chain(mono_names).chain(LAST.iter().map(|n| n.to_string())) {
            if !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                names.push(name);
            }
        }
        let picks = vec![None; names.len()];
        Chain { names, bold, italic, picks }
    }

    fn glyph(&mut self, face: usize, ch: char) -> u16 {
        let Some(loaded) = self.faces.get_mut(face) else { return 0 };
        if let Some(glyph) = loaded.chars.get(&ch) {
            return *glyph;
        }
        let glyph = FontRef::from_index(&loaded.data, loaded.index)
            .ok()
            .and_then(|font| font.charmap().map(ch))
            .and_then(|glyph| u16::try_from(glyph.to_u32()).ok())
            .filter(|glyph| usize::from(*glyph) < loaded.advances.len())
            .unwrap_or(0);
        if loaded.chars.len() < MAX_REMEMBERED {
            loaded.chars.insert(ch, glyph);
        }
        glyph
    }

    /// The glyph for `ch` in the first font of the chain that has one; in the first font there
    /// is, its "not defined" glyph, when none has. `None` when no font of the chain is there.
    pub(crate) fn resolve(&mut self, chain: &mut Chain, ch: char) -> Option<(Pick, u16)> {
        let mut first: Option<Pick> = None;
        for k in 0..chain.names.len() {
            let known = chain.picks.get(k).copied().flatten();
            let pick = match known {
                Some(pick) => pick,
                None => {
                    let pick = chain.names.get(k).and_then(|name| self.pick(name, chain.bold, chain.italic));
                    if let Some(slot) = chain.picks.get_mut(k) {
                        *slot = Some(pick);
                    }
                    pick
                }
            };
            let Some(pick) = pick else { continue };
            let glyph = self.glyph(pick.face, ch);
            if glyph != 0 {
                return Some((pick, glyph));
            }
            first = first.or(Some(pick));
        }
        first.map(|pick| (pick, 0))
    }
}
