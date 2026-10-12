//! Lays a message out on pages, for printing.
//!
//! What comes out is pages of placed glyphs, fills and lines, in points from each page's top
//! left corner ([`Printed`]); writing them to a file or a printer is someone else's work. The
//! body is the formatted document the reading pane draws ([`openmailclient_formats::rich::Doc`]),
//! with the same rules for paragraphs, borders and tables, so a message prints as it reads.
//!
//! Fonts are the installed ones, found by name ([`openmailclient_fonts`]); a face that may not be
//! put into a document is passed over for the next name. Text is set one character to one
//! glyph, left to right, with the pairs a face kerns standing as close as it says: there is
//! no shaping and no right-to-left setting yet.
//! Pictures stand in the line of text at the size the body asks for, or at one side with the
//! text beside them where the body asks for that; the caller says how large each is and draws
//! it.
//!
//! Everything in a body is hostile: sizes, widths, spans and nesting are capped, and so are the
//! pages and the glyphs one message may take.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod flow;
mod fonts;
mod list;
mod memo;

pub use fonts::{Fonts, Loaded};
pub use list::{Column, Listed, MAX_ROWS, Row, list};
pub use memo::{Memo, memo};
pub use openmailclient_formats::rich::{Line, Picture, Rgb};

/// Points in a millimetre.
pub const MM: f32 = 72.0 / 25.4;

/// Pages one message may take.
pub const MAX_PAGES: usize = 2000;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LayoutError {
    #[error("no font is installed that can be printed with")]
    NoFont,
}

/// A sheet: its size and the room left free at every edge, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paper {
    pub width: f32,
    pub height: f32,
    pub margin: f32,
}

impl Paper {
    /// ISO 216 A4, 210 by 297 mm.
    pub const A4: Paper = Paper { width: 595.28, height: 841.89, margin: 36.0 };
    /// US Letter, 8.5 by 11 inches.
    pub const LETTER: Paper = Paper { width: 612.0, height: 792.0, margin: 36.0 };

    /// US Legal, 8.5 by 14 inches.
    pub const LEGAL: Paper = Paper { width: 612.0, height: 1008.0, margin: 36.0 };
    /// ISO 216 A3, 297 by 420 mm.
    pub const A3: Paper = Paper { width: 841.89, height: 1190.55, margin: 36.0 };
    /// ISO 216 A5, 148 by 210 mm.
    pub const A5: Paper = Paper { width: 419.53, height: 595.28, margin: 36.0 };
    /// ISO 216 B5, 176 by 250 mm.
    pub const B5: Paper = Paper { width: 498.90, height: 708.66, margin: 36.0 };
    /// US Executive, 7.25 by 10.5 inches.
    pub const EXECUTIVE: Paper = Paper { width: 522.0, height: 756.0, margin: 36.0 };
    /// US Tabloid, 11 by 17 inches.
    pub const TABLOID: Paper = Paper { width: 792.0, height: 1224.0, margin: 36.0 };

    /// The names [`Paper::named`] knows.
    pub const NAMES: [&'static str; 8] = ["a4", "letter", "legal", "a3", "a5", "b5", "executive", "tabloid"];

    /// A sheet by its name, in any letter case.
    pub fn named(name: &str) -> Option<Paper> {
        match name.to_ascii_lowercase().as_str() {
            "a4" => Some(Self::A4),
            "letter" => Some(Self::LETTER),
            "legal" => Some(Self::LEGAL),
            "a3" => Some(Self::A3),
            "a5" => Some(Self::A5),
            "b5" => Some(Self::B5),
            "executive" => Some(Self::EXECUTIVE),
            "tabloid" => Some(Self::TABLOID),
            _ => None,
        }
    }

    /// The name of a sheet of this size, when it is one of the known ones.
    pub fn name(&self) -> Option<&'static str> {
        Self::NAMES
            .into_iter()
            .find(|name| Self::named(name).is_some_and(|p| (p.width - self.width).abs() < 0.5 && (p.height - self.height).abs() < 0.5))
    }
}

/// How pages are set up: the sheet and which way it lies, the room left free at each edge,
/// and what stands over and under every page.
///
/// A header or footer has three parts, at the left, in the middle and at the right, each one
/// line. In them `[Page]` is the page's number, `[Pages]` how many pages there are, and
/// `[Page of Pages]` the number, with "of" how many when there is more than one; any other
/// `[name]` is looked up in `words`. Names are matched in any letter case, and what a name
/// gives is not looked through for names again.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    /// The sheet, standing upright. Its `margin` is not looked at.
    pub paper: Paper,
    /// The sheet lies on its long side.
    pub landscape: bool,
    /// Room left free at the top, the right, the bottom and the left, in points.
    pub margins: [f32; 4],
    pub header: [String; 3],
    pub footer: [String; 3],
    /// Names a header or footer may ask for, each with what it gives.
    pub words: Vec<(String, String)>,
}

impl From<Paper> for Setup {
    /// The sheet upright with its margin at every edge, and each page's number at its foot.
    fn from(paper: Paper) -> Self {
        Setup {
            paper,
            landscape: false,
            margins: [paper.margin; 4],
            header: Default::default(),
            footer: [String::new(), "[Page of Pages]".into(), String::new()],
            words: Vec::new(),
        }
    }
}

impl From<&Setup> for Setup {
    fn from(setup: &Setup) -> Self {
        setup.clone()
    }
}

impl Default for Setup {
    fn default() -> Self {
        Paper::A4.into()
    }
}

/// A message laid out.
#[derive(Clone, Debug, Default)]
pub struct Printed {
    pub pages: Vec<Page>,
    /// The faces the pages' text is set in; [`Text::face`] counts into this.
    pub faces: Vec<Loaded>,
    /// The message was longer than the limits; the rest is not on the pages.
    pub truncated: bool,
}

/// One page. Fills lie under lines, lines under pictures, and pictures under text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    pub width: f32,
    pub height: f32,
    /// In the order they are to be painted.
    pub fills: Vec<Fill>,
    pub rules: Vec<Rule>,
    pub pictures: Vec<Placed>,
    pub texts: Vec<Text>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fill {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: Rgb,
    /// How deep in tables the fill is; a deeper one lies over a shallower one.
    pub depth: u16,
}

/// A picture and the rectangle it fills, from its top left corner.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub picture: Picture,
}

/// A straight line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub width: f32,
    pub color: Rgb,
    pub line: Line,
}

/// Glyphs of one face, in one size and colour, on one baseline.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub x: f32,
    /// The baseline.
    pub y: f32,
    pub face: usize,
    pub size: f32,
    pub color: Rgb,
    /// Each glyph with the character it stands for.
    pub glyphs: Vec<(u16, char)>,
    /// The width the glyphs take.
    pub width: f32,
    /// Kerning: how much further than its own width each glyph moves on before the next,
    /// less than nothing for a pair that stands closer. Empty when no glyph is moved.
    pub kerns: Vec<f32>,
    /// The face has no bold of its own: the glyphs are to be thickened.
    pub bold: bool,
    /// The face has no italic of its own: the glyphs are to be leant.
    pub slant: bool,
    /// How far above the baseline the glyphs are drawn: a mark the font places over a letter.
    pub lift: f32,
    /// What the glyphs say, where that is not their characters one after another: letters
    /// drawn as one glyph, or text written from the right, whose glyphs are in the order
    /// they are seen in. Nothing for a mark that is read with its letter.
    pub actual: Option<String>,
}

impl Page {
    /// The page's text, piece after piece in the order it was set, pieces on one baseline
    /// joined; for tests and for looking at a layout.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut last: Option<f32> = None;
        for piece in &self.texts {
            if last.is_some_and(|y| (y - piece.y).abs() > 0.01) {
                out.push('\n');
            }
            match &piece.actual {
                Some(actual) => out.push_str(actual),
                None => out.extend(piece.glyphs.iter().map(|(_, ch)| *ch)),
            }
            last = Some(piece.y);
        }
        out
    }
}

#[cfg(test)]
mod tests;
