//! Draws a laid-out page as pixels, so that it can be looked at before it is printed.
//!
//! The page is drawn from the same [`Printed`] the PDF file is written from, in the same order
//! (fills, lines, pictures, text) and with the glyphs' own outlines, so what is seen is what
//! the file will hold: nothing is laid out or measured again here.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::HashMap;

use mailcraft_fonts::{Ring, shapes as glyph_shapes};
use mailcraft_formats::canvas::Canvas;
use mailcraft_layout::{Line, Page, Picture, Printed, Text};

/// The longest side of a page drawn, in pixels.
pub const MAX_SIDE: usize = 4096;
/// Pixels to the point: the least and the most a page is drawn at.
const MIN_SCALE: f32 = 0.05;
const MAX_SCALE: f32 = 8.0;
/// Dashes one line is cut into.
const MAX_DASHES: usize = 2000;
/// How far a glyph thickened for want of a bold face is smeared to each side, in ems; the PDF
/// file strokes its outline 0.03 em wide.
const BOLD: f32 = 0.015;

/// A page as pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// Red, green and blue for each pixel, row after row from the top.
    pub rgb: Vec<u8>,
}

/// A picture a page shows: red, green, blue and cover for each pixel, row after row from the
/// top.
#[derive(Clone, Copy)]
pub struct Bitmap<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

/// Draw page `page` of `printed` at `scale` pixels to the point; less when the page would be
/// longer than [`MAX_SIDE`]. `pictures` gives the pixels of a picture the page shows; one it
/// has no pixels for is left out. `None` when there is no such page.
pub fn page<'a>(printed: &Printed, page: usize, scale: f32, pictures: &dyn Fn(&Picture) -> Option<Bitmap<'a>>) -> Option<Image> {
    let page = printed.pages.get(page)?;
    let (wide, tall) = (page.width, page.height);
    if !(wide.is_finite() && tall.is_finite() && wide >= 1.0 && tall >= 1.0) {
        return None;
    }
    let scale = if scale.is_finite() { scale.clamp(MIN_SCALE, MAX_SCALE) } else { 1.0 };
    let scale = scale.min(MAX_SIDE as f32 / wide.max(tall));
    let side = |v: f32| ((v * scale).round() as usize).clamp(1, MAX_SIDE);
    let mut canvas = Canvas::sized(side(wide), side(tall), MAX_SIDE)?;
    // A page that asks for more work than one may take is shown as far as it was drawn.
    let _ = draw(&mut canvas, printed, page, scale, pictures);
    Some(Image { width: canvas.w, height: canvas.h, rgb: canvas.into_rgb() })
}

fn draw<'a>(canvas: &mut Canvas, printed: &Printed, page: &Page, s: f32, pictures: &dyn Fn(&Picture) -> Option<Bitmap<'a>>) -> Option<()> {
    for fill in &page.fills {
        let (l, t, r, b) = (fill.x * s, fill.y * s, (fill.x + fill.width) * s, (fill.y + fill.height) * s);
        canvas.fill(&[vec![(l, t), (r, t), (r, b), (l, b)]], true, fill.color)?;
    }
    for rule in &page.rules {
        let width = rule.width * s;
        let (from, to) = ((rule.from.0 * s, rule.from.1 * s), (rule.to.0 * s, rule.to.1 * s));
        // As the PDF file dashes them: three widths on and off, or one.
        let dash = match rule.line {
            Line::Dashed => Some(width.max(1.0) * 3.0),
            Line::Dotted => Some(width.max(1.0)),
            _ => None,
        };
        let long = ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt();
        match dash.filter(|dash| long.is_finite() && long > *dash) {
            Some(dash) => {
                let (ux, uy) = ((to.0 - from.0) / long, (to.1 - from.1) / long);
                let mut at = 0.0f32;
                for _ in 0..MAX_DASHES {
                    if at >= long {
                        break;
                    }
                    let end = (at + dash).min(long);
                    canvas.stroke(&[(from.0 + ux * at, from.1 + uy * at), (from.0 + ux * end, from.1 + uy * end)], false, width, rule.color)?;
                    at += dash * 2.0;
                }
            }
            None => canvas.stroke(&[from, to], false, width, rule.color)?,
        }
    }
    for placed in &page.pictures {
        if let Some(bitmap) = pictures(&placed.picture) {
            let to = (placed.x * s, placed.y * s, (placed.x + placed.width) * s, (placed.y + placed.height) * s);
            canvas.image(bitmap.rgba, (bitmap.width as usize, bitmap.height as usize), to)?;
        }
    }
    // The outline of each glyph, in ems, read from its face once.
    let mut shapes: HashMap<(usize, u16, bool), Vec<Ring>> = HashMap::new();
    for text in &page.texts {
        glyphs(canvas, printed, text, s, &mut shapes)?;
    }
    Some(())
}

fn glyphs(canvas: &mut Canvas, printed: &Printed, text: &Text, s: f32, shapes: &mut HashMap<(usize, u16, bool), Vec<Ring>>) -> Option<()> {
    let Some(face) = printed.faces.get(text.face) else { return Some(()) };
    if !(text.size.is_finite() && text.size > 0.0 && text.x.is_finite() && text.y.is_finite()) {
        return Some(());
    }
    let new: Vec<u16> = text.glyphs.iter().map(|g| g.0).filter(|g| !shapes.contains_key(&(text.face, *g, text.slant))).collect();
    if !new.is_empty() {
        let read = glyph_shapes(&face.data, face.index, text.slant, &new).unwrap_or_default();
        for (n, glyph) in new.iter().enumerate() {
            shapes.insert((text.face, *glyph, text.slant), read.get(n).cloned().unwrap_or_default());
        }
    }
    let em = text.size * s;
    let mut pen = text.x;
    for (n, (glyph, _)) in text.glyphs.iter().enumerate() {
        if let Some(rings) = shapes.get(&(text.face, *glyph, text.slant)).filter(|rings| !rings.is_empty()) {
            let (x, y) = (pen * s, text.y * s);
            let smears: &[f32] = if text.bold { &[-BOLD, 0.0, BOLD] } else { &[0.0] };
            for smear in smears {
                let placed: Vec<Ring> = rings.iter().map(|ring| ring.iter().map(|p| (x + (p.0 + smear) * em, y + p.1 * em)).collect()).collect();
                canvas.fill(&placed, true, text.color)?;
            }
        }
        let kern = text.kerns.get(n).copied().filter(|k| k.is_finite()).unwrap_or(0.0);
        pen += face.advance(*glyph, text.size) + kern;
        if !pen.is_finite() {
            break;
        }
    }
    Some(())
}

#[cfg(test)]
mod tests;
