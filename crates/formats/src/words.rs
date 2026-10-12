//! Text in a drawing: a line of it filled onto the page as the shapes of its letters, in the
//! font the drawing names ([MS-WMF] 2.2.1.2, Font; [MS-EMF] 2.2.13, LogFont). The shapes come
//! from whoever knows the installed fonts ([`Lettering`]); a line they cannot give is not drawn.

use openmailclient_fonts::Lettering;

use crate::canvas::{Canvas, MAX_SIDE, Point, Rgb};

/// Characters of a drawing's words that are kept as what it says.
const MAX_WORDS: usize = 2000;
/// The tallest letters, in pixels of the page.
const MAX_EM: f32 = 4.0 * MAX_SIDE as f32;

/// [MS-WMF] 2.1.2.3, TextAlignmentMode: the point a line is given is on its left, on its right
/// or in its middle; on its top, on its bottom or on the line the letters stand on.
pub(crate) const UPDATE_PLACE: u32 = 1;
const RIGHT: u32 = 2;
const CENTRE: u32 = 6;
const BOTTOM: u32 = 8;
const BASELINE: u32 = 24;
pub(crate) const RIGHT_TO_LEFT: u32 = 256;

/// [MS-WMF] 2.1.2.2, ExtTextOutOptions: the box is filled, the text is cut to it, the string is
/// numbers of glyphs, it reads right to left, each letter is moved both ways.
pub(crate) const OPAQUE: u32 = 0x0002;
pub(crate) const CLIPPED: u32 = 0x0004;
pub(crate) const UNDRAWN: u32 = 0x0010 | 0x0080 | 0x2000;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Font {
    /// In the drawing's units: above zero the height of a line, below it the height of the em.
    pub height: f32,
    /// Tenths of a degree the line is turned, against the clock.
    pub turn: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// [MS-WMF] 2.1.1.5, CharacterSet.
    pub charset: u8,
    pub family: String,
}

impl Font {
    /// Text written in bytes, in the font's character set. `None` for a set that is not known
    /// (a font of symbols has its own) and for bytes that are not text in it.
    pub fn read(&self, bytes: &[u8]) -> Option<String> {
        if bytes.is_ascii() && self.charset != 2 {
            return String::from_utf8(bytes.to_vec()).ok();
        }
        let page = match self.charset {
            0 | 1 => 1252,
            128 => 932,
            129 => 949,
            134 => 936,
            136 => 950,
            161 => 1253,
            162 => 1254,
            163 => 1258,
            177 => 1255,
            178 => 1256,
            186 => 1257,
            204 => 1251,
            222 => 874,
            238 => 1250,
            _ => return None,
        };
        Some(openmailclient_model::codepage::decode(page, bytes)).filter(|text| !text.contains('\u{FFFD}'))
    }
}

/// One line of text and where it goes on the page.
pub(crate) struct Line<'a> {
    /// The point the line is set by, on the page.
    pub at: Point,
    /// Pixels to the drawing's unit, across and down.
    pub scale: Point,
    pub text: &'a str,
    /// How far each letter is from the next, in the drawing's units, when the drawing says.
    pub dx: Option<&'a [f32]>,
    pub align: u32,
    pub colour: Rgb,
    /// What the line's box is filled with before the letters.
    pub back: Option<Rgb>,
}

/// Fill a line of text onto the page. How long it is, in pixels; `None` when it cannot be drawn.
pub(crate) fn write(canvas: &mut Canvas, lettering: &dyn Lettering, font: &Font, line: &Line) -> Option<f32> {
    let letters = lettering.letters(&font.family, font.bold, font.italic, line.text)?;
    let tall = letters.ascent + letters.descent;
    let em = if font.height < 0.0 { -font.height } else { font.height / tall } * line.scale.1;
    if !em.is_finite() || em <= 0.0 || em > MAX_EM {
        return None;
    }
    // The drawing's own spacing keeps every letter where it was meant, whatever font stands in.
    let dx = line.dx.filter(|dx| dx.len() == letters.each.len());
    let mut starts = Vec::with_capacity(letters.each.len());
    let mut long = 0.0f32;
    for (i, letter) in letters.each.iter().enumerate() {
        starts.push(long);
        long += match dx {
            Some(dx) => *dx.get(i)? * line.scale.0,
            None => letter.advance * em,
        };
    }
    if !long.is_finite() {
        return None;
    }
    let along = match line.align & CENTRE {
        CENTRE => -long / 2.0,
        RIGHT => -long,
        _ => 0.0,
    };
    let (up, down) = (letters.ascent * em, letters.descent * em);
    let base = match line.align & BASELINE {
        BASELINE => 0.0,
        BOTTOM => -down,
        _ => up,
    };
    let (sin, cos) = (font.turn / 10.0).to_radians().sin_cos();
    let place = |x: f32, y: f32| (line.at.0 + x * cos + y * sin, line.at.1 - x * sin + y * cos);
    let bar = |from: f32, to: f32| {
        vec![place(along, base + from), place(along + long, base + from), place(along + long, base + to), place(along, base + to)]
    };
    if let Some(back) = line.back {
        canvas.fill(&[bar(-up, down)], true, back)?;
    }
    for (letter, start) in letters.each.iter().zip(&starts) {
        if letter.rings.is_empty() {
            continue;
        }
        let rings: Vec<Vec<Point>> =
            letter.rings.iter().map(|ring| ring.iter().map(|&(x, y)| place(along + start + x * em, base + y * em)).collect()).collect();
        canvas.fill(&rings, true, line.colour)?;
    }
    let thick = (em * 0.06).max(1.0);
    if font.underline {
        canvas.fill(&[bar(em * 0.1, em * 0.1 + thick)], true, line.colour)?;
    }
    if font.strike {
        canvas.fill(&[bar(-em * 0.3, -em * 0.3 + thick)], true, line.colour)?;
    }
    Some(long)
}

/// Keep what a line says among the drawing's words.
pub(crate) fn note(words: &mut String, text: &str) {
    let text = text.trim();
    let mut room = MAX_WORDS.saturating_sub(words.chars().count());
    if text.is_empty() || room < 2 {
        return;
    }
    if !words.is_empty() {
        words.push(' ');
        room -= 1;
    }
    words.extend(text.chars().filter(|c| !c.is_control()).take(room));
}
