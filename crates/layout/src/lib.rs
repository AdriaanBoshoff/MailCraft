//! Lays a message out on pages, for printing.
//!
//! What comes out is pages of placed glyphs, fills and lines, in points from each page's top
//! left corner ([`Printed`]); writing them to a file or a printer is someone else's work. The
//! body is the formatted document the reading pane draws ([`mailcraft_formats::rich::Doc`]),
//! with the same rules for paragraphs, borders and tables, so a message prints as it reads.
//!
//! Fonts are the installed ones, found by name ([`mailcraft_fonts`]); a face that may not be
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
mod memo;

pub use fonts::{Fonts, Loaded};
pub use mailcraft_formats::rich::{Line, Picture, Rgb};
pub use memo::{Memo, memo};

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

    /// A sheet by its name, in any letter case.
    pub fn named(name: &str) -> Option<Paper> {
        match name.to_ascii_lowercase().as_str() {
            "a4" => Some(Self::A4),
            "letter" => Some(Self::LETTER),
            _ => None,
        }
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
            out.extend(piece.glyphs.iter().map(|(_, ch)| *ch));
            last = Some(piece.y);
        }
        out
    }
}

#[cfg(test)]
mod tests;
