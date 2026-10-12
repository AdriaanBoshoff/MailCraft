//! A Windows metafile that is a drawing of lines, filled shapes and text, drawn onto a page of
//! pixels ([MS-WMF] 2.3.3 drawing records, 2.3.4 object records, 2.3.5 state records). A
//! metafile that asks for anything this does not draw (bitmaps, patterned brushes, regions,
//! text in a font nobody here knows) is not drawn at all: half a picture is worse than its
//! place.

use std::f32::consts::TAU;
use std::sync::Arc;

use mailcraft_fonts::Lettering;

use crate::canvas::{Brush, Canvas, Point, Rgb, arc, page_size, rounded, steps};
use crate::metafile::{Drawn, size_at, u16_at, u32_at};
use crate::words::{self, Font, Line};

/// Records one drawing is read through.
const MAX_RECORDS: usize = 50_000;
/// Points all of a drawing's shapes may have between them.
const MAX_POINTS: usize = 500_000;
/// Pens, brushes and the like one drawing may make.
const MAX_OBJECTS: usize = 4096;
/// States that may be saved one on another.
const MAX_SAVED: usize = 64;

/// Bytes of text in one line.
const MAX_LINE: usize = 4096;

/// Records that change nothing that is drawn here: ways of bitmaps and of spacing text out,
/// the palette, and what is said to a printer ([MS-WMF] 2.3.5, 2.3.6).
const IGNORED: [u16; 13] = [0x0035, 0x0037, 0x0103, 0x0105, 0x0107, 0x0108, 0x0139, 0x0149, 0x020A, 0x0231, 0x0234, 0x0436, 0x0626];

fn i16_at(data: &[u8], at: usize) -> Option<i16> {
    let bytes = data.get(at..at.checked_add(2)?)?;
    Some(i16::from_le_bytes(<[u8; 2]>::try_from(bytes).ok()?))
}

/// [MS-WMF] 2.2.2.8, ColorRef: red, green, blue, and a byte that says whether they are a
/// colour or a number in a palette (which is not read).
fn colour_at(data: &[u8], at: usize) -> Option<Rgb> {
    match *data.get(at..at.checked_add(4)?)? {
        [r, g, b, flag] if flag != 1 => Some([r, g, b]),
        _ => None,
    }
}

/// A pen is its width in the drawing's own units and its colour; none draws no line.
type Pen = Option<(f32, Rgb)>;

#[derive(Clone)]
enum Object {
    Pen(Pen),
    Brush(Brush),
    Font(Arc<Font>),
    Other,
}

/// What a drawing is drawn with and where ([MS-WMF] 3.1.5, the playback device context).
#[derive(Clone)]
struct State {
    /// The window: the part of the drawing's own space that fills the page.
    origin: Point,
    extent: Option<Point>,
    pen: Pen,
    brush: Brush,
    winding: bool,
    at: Point,
    /// On the page; `None` is all of it.
    clip: Option<(f32, f32, f32, f32)>,
    /// What text is written in; `None` is the device's own font, which nobody here knows.
    font: Option<Arc<Font>>,
    /// The colour of letters, the colour behind them, and whether that is filled in.
    ink: Rgb,
    back: Rgb,
    opaque: bool,
    /// [MS-WMF] 2.1.2.3, TextAlignmentMode.
    align: u32,
}

/// From the drawing's units to the page's pixels.
#[derive(Clone, Copy)]
struct Map {
    origin: Point,
    scale: Point,
}

impl Map {
    fn at(&self, (x, y): Point) -> Point {
        ((x - self.origin.0) * self.scale.0, (y - self.origin.1) * self.scale.1)
    }
}

struct Page<'a> {
    /// Who gives the shapes of letters, when anybody does.
    letters: Option<&'a dyn Lettering>,
    /// What the drawing's text says.
    words: String,
    /// The size it will be shown at, in pixels, when that is known.
    want: Option<(u32, u32)>,
    /// The size the metafile says it has, in pixels.
    natural: Option<Point>,
    canvas: Option<Canvas>,
    state: State,
    saved: Vec<State>,
    objects: Vec<Option<Object>>,
    points: usize,
    drew: bool,
}

impl Page<'_> {
    /// The page's pixels, made when the first thing is drawn: by then the window is known.
    fn ready(&mut self) -> Option<Map> {
        let (ex, ey) = self.state.extent?;
        if ex == 0.0 || ey == 0.0 {
            return None;
        }
        if self.canvas.is_none() {
            let (w, h) = page_size(self.want, self.natural, (ex, ey))?;
            self.canvas = Canvas::new(w, h);
        }
        let canvas = self.canvas.as_mut()?;
        let all = (0.0, 0.0, canvas.w as f32, canvas.h as f32);
        canvas.clip = self.state.clip.unwrap_or(all);
        Some(Map { origin: self.state.origin, scale: (canvas.w as f32 / ex, canvas.h as f32 / ey) })
    }

    fn count(&mut self, points: usize) -> Option<()> {
        self.points = self.points.checked_add(points)?;
        (self.points <= MAX_POINTS).then_some(())
    }

    /// Fill `rings` with the brush when `filled`, and draw the pen along them.
    fn shape(&mut self, rings: &[Vec<Point>], filled: bool, closed: bool, map: Map) -> Option<()> {
        let State { pen, brush, winding, .. } = self.state;
        let canvas = self.canvas.as_mut()?;
        if filled {
            match brush {
                Brush::Null => {}
                Brush::Solid(colour) => canvas.fill(rings, winding, colour)?,
                Brush::Unknown => return None,
            }
        }
        if let Some((width, colour)) = pen {
            for ring in rings {
                canvas.stroke(ring, closed, (width * map.scale.0.abs()).max(1.0), colour)?;
            }
        }
        self.drew = true;
        Some(())
    }

    /// Write a line of text, set by `at` in the drawing's units. `bytes` are in the font's
    /// character set; `dx` is how far each is from the next; `frame` is the box that
    /// `options` say is filled or cut to ([MS-WMF] 2.3.3.5, META_EXTTEXTOUT).
    fn say(&mut self, at: Point, bytes: &[u8], dx: Option<Vec<f32>>, frame: Option<(f32, f32, f32, f32)>, options: u32) -> Option<()> {
        if options & words::UNDRAWN != 0 || self.state.align & words::RIGHT_TO_LEFT != 0 {
            return None;
        }
        let map = self.ready()?;
        let State { ink, back, opaque, align, .. } = self.state;
        let frame = frame.map(|(l, t, r, b)| {
            let (a, b) = (map.at((l, t)), map.at((r, b)));
            (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))
        });
        if options & words::OPAQUE != 0
            && let Some((l, t, r, b)) = frame
        {
            self.canvas.as_mut()?.fill(&[vec![(l, t), (r, t), (r, b), (l, b)]], true, back)?;
            self.drew = true;
        }
        if bytes.is_empty() {
            return Some(());
        }
        let (lettering, font) = (self.letters?, self.state.font.clone()?);
        let text = font.read(bytes)?;
        self.count(bytes.len())?;
        let from = if align & words::UPDATE_PLACE != 0 { self.state.at } else { at };
        let canvas = self.canvas.as_mut()?;
        let all = canvas.clip;
        if options & words::CLIPPED != 0
            && let Some((l, t, r, b)) = frame
        {
            canvas.clip = (l.max(all.0), t.max(all.1), r.min(all.2), b.min(all.3));
        }
        let line = Line {
            at: map.at(from),
            scale: (map.scale.0.abs(), map.scale.1.abs()),
            text: &text,
            dx: dx.as_deref(),
            align,
            colour: ink,
            back: opaque.then_some(back),
        };
        let long = words::write(canvas, lettering, &font, &line);
        canvas.clip = all;
        // The next line starts where this one ended, when the drawing keeps its place.
        if align & words::UPDATE_PLACE != 0 && align & 6 == 0 && map.scale.0 != 0.0 {
            self.state.at.0 += long? / map.scale.0.abs();
        }
        long?;
        words::note(&mut self.words, &text);
        self.drew = true;
        Some(())
    }

    /// Keep a new object in the first free place ([MS-WMF] 3.1.4.1).
    fn make(&mut self, object: Object) -> Option<()> {
        if let Some(place) = self.objects.iter_mut().find(|place| place.is_none()) {
            *place = Some(object);
        } else if self.objects.len() < MAX_OBJECTS {
            self.objects.push(Some(object));
        } else {
            return None;
        }
        Some(())
    }

    /// The corners of a box given as bottom, right, top, left from word `first` of a record,
    /// on the page: left, top, right, bottom.
    fn bounds(record: &[u8], first: usize, map: Map) -> Option<(f32, f32, f32, f32)> {
        let word = |i: usize| i16_at(record, 6 + 2 * (first + i)).map(f32::from);
        let (a, b) = (map.at((word(3)?, word(2)?)), map.at((word(1)?, word(0)?)));
        Some((a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)))
    }

    /// Do what one record says. `None` when it is cut short or asks for what is not drawn.
    fn record(&mut self, function: u16, record: &[u8]) -> Option<()> {
        // A record's parameters are words, the last one written first.
        let word = |i: usize| i16_at(record, 6 + 2 * i).map(f32::from);
        match function {
            // META_SAVEDC and META_RESTOREDC: back so many states, or to the one of that number.
            0x001E => {
                if self.saved.len() >= MAX_SAVED {
                    return None;
                }
                self.saved.push(self.state.clone());
            }
            0x0127 => {
                let n = i16_at(record, 6)?;
                let keep =
                    if n < 0 { self.saved.len().checked_sub(usize::from(n.unsigned_abs()))? } else { usize::try_from(n).ok()?.checked_sub(1)? };
                self.state = self.saved.get(keep)?.clone();
                self.saved.truncate(keep);
            }
            // META_SETWINDOWORG, META_SETWINDOWEXT, META_OFFSETWINDOWORG, META_SCALEWINDOWEXT.
            0x020B => self.state.origin = (word(1)?, word(0)?),
            0x020C => self.state.extent = Some((word(1)?, word(0)?)),
            0x020F => self.state.origin = (self.state.origin.0 + word(1)?, self.state.origin.1 + word(0)?),
            0x0410 => {
                let (ex, ey) = self.state.extent?;
                let (y_by, y, x_by, x) = (word(0)?, word(1)?, word(2)?, word(3)?);
                if x_by == 0.0 || y_by == 0.0 {
                    return None;
                }
                self.state.extent = Some((ex * x / x_by, ey * y / y_by));
            }
            // META_SETPOLYFILLMODE; META_SETROP2, where only a pen that covers is drawn.
            0x0106 => self.state.winding = u16_at(record, 6)? == 2,
            0x0104 => {
                if u16_at(record, 6)? != 13 {
                    return None;
                }
            }
            // [MS-WMF] 2.2.1.4, Pen: its style, its width and its colour.
            0x02FA => {
                let pen = if u16_at(record, 6)? & 0x0F == 5 { None } else { Some((word(1)?.abs(), colour_at(record, 12)?)) };
                self.make(Object::Pen(pen))?;
            }
            // [MS-WMF] 2.2.1.1, Brush: its style, and its colour when that is all it is.
            0x02FC => {
                let brush = match u16_at(record, 6)? {
                    0 => Brush::Solid(colour_at(record, 8)?),
                    1 => Brush::Null,
                    _ => Brush::Unknown,
                };
                self.make(Object::Brush(brush))?;
            }
            // Brushes of a pattern; a palette and a region, which only take a place.
            0x0142 | 0x01F9 => self.make(Object::Brush(Brush::Unknown))?,
            0x00F7 | 0x06FF => self.make(Object::Other)?,
            // [MS-WMF] 2.2.1.2, Font: its height, how it is turned, how heavy it is, whether it
            // leans, is underlined or struck out, its character set, and its name.
            0x02FB => {
                let byte = |at: usize| record.get(at).copied();
                let name = record.get(24..)?;
                let name = name.get(..name.iter().position(|&b| b == 0).unwrap_or(name.len()).min(32))?;
                let font = Font {
                    height: word(0)?,
                    turn: word(2)?,
                    bold: i16_at(record, 14)? >= 600,
                    italic: byte(16)? != 0,
                    underline: byte(17)? != 0,
                    strike: byte(18)? != 0,
                    charset: byte(19)?,
                    family: mailcraft_model::codepage::decode(1252, name),
                };
                self.make(Object::Font(Arc::new(font)))?;
            }
            // META_SELECTOBJECT, META_DELETEOBJECT.
            0x012D => match self.objects.get(usize::from(u16_at(record, 6)?))?.clone()? {
                Object::Pen(pen) => self.state.pen = pen,
                Object::Brush(brush) => self.state.brush = brush,
                Object::Font(font) => self.state.font = Some(font),
                Object::Other => {}
            },
            // META_SETTEXTCOLOR, META_SETBKCOLOR, META_SETBKMODE, META_SETTEXTALIGN.
            0x0209 => self.state.ink = colour_at(record, 6)?,
            0x0201 => self.state.back = colour_at(record, 6)?,
            0x0102 => self.state.opaque = u16_at(record, 6)? == 2,
            0x012E => self.state.align = u32::from(u16_at(record, 6)?),
            // META_TEXTOUT: how many bytes, the bytes made up to a whole word, and the point.
            0x0521 => {
                let count = usize::from(u16_at(record, 6)?);
                if count > MAX_LINE {
                    return None;
                }
                let after = 8 + count + count % 2;
                let at = (f32::from(i16_at(record, after + 2)?), f32::from(i16_at(record, after)?));
                self.say(at, record.get(8..8 + count)?, None, None, 0)?;
            }
            // META_EXTTEXTOUT: the point, how many bytes, what to do with the box, the box when
            // there is one, the bytes, and how far each is from the next when that is given.
            0x0A32 => {
                let (at, count, options) = ((word(1)?, word(0)?), usize::from(u16_at(record, 10)?), u32::from(u16_at(record, 12)?));
                if count > MAX_LINE {
                    return None;
                }
                let mut next = 14;
                let mut frame = None;
                if options & (words::OPAQUE | words::CLIPPED) != 0 {
                    let side = |i: usize| i16_at(record, 14 + 2 * i).map(f32::from);
                    frame = Some((side(0)?, side(1)?, side(2)?, side(3)?));
                    next = 22;
                }
                let bytes = record.get(next..next + count)?;
                next += count + count % 2;
                let dx = (0..count).map(|i| i16_at(record, next + 2 * i).map(f32::from)).collect::<Option<Vec<f32>>>();
                self.say(at, bytes, dx.filter(|dx| !dx.is_empty()), frame, options)?;
            }
            0x01F0 => *self.objects.get_mut(usize::from(u16_at(record, 6)?))? = None,
            // META_MOVETO, META_LINETO.
            0x0214 => self.state.at = (word(1)?, word(0)?),
            0x0213 => {
                let to = (word(1)?, word(0)?);
                let map = self.ready()?;
                self.count(2)?;
                self.shape(&[vec![map.at(self.state.at), map.at(to)]], false, false, map)?;
                self.state.at = to;
            }
            // META_POLYLINE, META_POLYGON: how many points, and the points.
            0x0325 | 0x0324 => {
                let count = usize::from(u16_at(record, 6)?);
                let map = self.ready()?;
                self.count(count)?;
                let ring = (0..count).map(|i| Some(map.at((word(1 + 2 * i)?, word(2 + 2 * i)?)))).collect::<Option<Vec<_>>>()?;
                let closed = function == 0x0324;
                self.shape(&[ring], closed, closed, map)?;
            }
            // META_POLYPOLYGON: how many polygons, how many points each has, and the points.
            0x0538 => {
                let polygons = usize::from(u16_at(record, 6)?);
                let map = self.ready()?;
                let mut next = 1 + polygons;
                let mut rings = Vec::new();
                for polygon in 0..polygons {
                    let count = usize::from(u16_at(record, 8 + 2 * polygon)?);
                    self.count(count.max(1))?;
                    rings.push((0..count).map(|i| Some(map.at((word(next + 2 * i)?, word(next + 2 * i + 1)?)))).collect::<Option<Vec<_>>>()?);
                    next += 2 * count;
                }
                self.shape(&rings, true, true, map)?;
            }
            // META_RECTANGLE.
            0x041B => {
                let map = self.ready()?;
                let (l, t, r, b) = Self::bounds(record, 0, map)?;
                self.count(4)?;
                self.shape(&[vec![(l, t), (r, t), (r, b), (l, b)]], true, true, map)?;
            }
            // META_ELLIPSE.
            0x0418 => {
                let map = self.ready()?;
                let (l, t, r, b) = Self::bounds(record, 0, map)?;
                let radii = ((r - l) / 2.0, (b - t) / 2.0);
                let ring = arc(((l + r) / 2.0, (t + b) / 2.0), radii, 0.0, TAU, steps(radii));
                self.count(ring.len())?;
                self.shape(&[ring], true, true, map)?;
            }
            // META_ROUNDRECT: the height and the width of the ellipse its corners are cut from.
            0x061C => {
                let map = self.ready()?;
                let (l, t, r, b) = Self::bounds(record, 2, map)?;
                let radii = ((word(1)? * map.scale.0).abs().min(r - l) / 2.0, (word(0)? * map.scale.1).abs().min(b - t) / 2.0);
                let ring = rounded((l, t, r, b), radii, steps(radii));
                self.count(ring.len())?;
                self.shape(&[ring], true, true, map)?;
            }
            // META_ARC, META_PIE, META_CHORD: part of an ellipse, from where the line from its
            // middle to one point crosses it round to the other, against the clock.
            0x0817 | 0x081A | 0x0830 => {
                let map = self.ready()?;
                let (l, t, r, b) = Self::bounds(record, 4, map)?;
                let (centre, radii) = (((l + r) / 2.0, (t + b) / 2.0), ((r - l) / 2.0, (b - t) / 2.0));
                if radii.0 <= 0.0 || radii.1 <= 0.0 {
                    return Some(());
                }
                let angle = |p: Point| (-(p.1 - centre.1) / radii.1).atan2((p.0 - centre.0) / radii.0);
                let (from, to) = (angle(map.at((word(3)?, word(2)?))), angle(map.at((word(1)?, word(0)?))));
                let turn = if to > from { to - from } else { to - from + TAU };
                let mut ring = arc(centre, radii, from, from + turn, steps(radii));
                if function == 0x081A {
                    ring.push(centre);
                }
                self.count(ring.len())?;
                let closed = function != 0x0817;
                self.shape(&[ring], closed, closed, map)?;
            }
            // META_SETPIXEL: a colour, and the one point that takes it.
            0x041F => {
                let colour = colour_at(record, 6)?;
                let map = self.ready()?;
                let (x, y) = map.at((word(3)?, word(2)?));
                self.count(1)?;
                self.canvas.as_mut()?.fill(&[vec![(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)]], true, colour)?;
                self.drew = true;
            }
            // META_INTERSECTCLIPRECT: what is drawn from here on stays inside this box too.
            0x0416 => {
                let map = self.ready()?;
                let (l, t, r, b) = Self::bounds(record, 0, map)?;
                let was = self.canvas.as_ref()?.clip;
                self.state.clip = Some((l.max(was.0), t.max(was.1), r.min(was.2), b.min(was.3)));
            }
            _ if IGNORED.contains(&function) => {}
            _ => return None,
        }
        Some(())
    }
}

/// A Windows metafile that draws lines, filled shapes and text and nothing else → a BMP file of
/// it, for showing at `want` pixels when that is known. Text is drawn in the fonts `letters`
/// knows; with nobody to ask, a metafile with text in it is not drawn.
pub(crate) fn wmf_drawing(data: &[u8], want: Option<(u32, u32)>, letters: Option<&dyn Lettering>) -> Option<Drawn> {
    let mut state = State {
        origin: (0.0, 0.0),
        extent: None,
        pen: Some((0.0, [0; 3])),
        brush: Brush::Solid([255; 3]),
        winding: false,
        at: (0.0, 0.0),
        clip: None,
        font: None,
        ink: [0; 3],
        back: [255; 3],
        opaque: true,
        align: 0,
    };
    let mut natural = None;
    let mut at = 0usize;
    // [MS-WMF] 2.3.2.3, META_PLACEABLE: the box the drawing fills, and its units to the inch.
    if u32_at(data, 0)? == 0x9AC6_CDD7 {
        let side = |i: usize| i16_at(data, 6 + 2 * i).map(f32::from);
        let (l, t, r, b, inch) = (side(0)?, side(1)?, side(2)?, side(3)?, f32::from(u16_at(data, 14)?));
        if r != l && b != t {
            state.origin = (l, t);
            state.extent = Some((r - l, b - t));
            if inch > 0.0 {
                natural = Some(((r - l).abs() / inch * 96.0, (b - t).abs() / inch * 96.0));
            }
        }
        at = 22;
    }
    // [MS-WMF] 2.3.2.2, META_HEADER.
    if !matches!(u16_at(data, at)?, 1 | 2) || u16_at(data, at + 2)? != 9 {
        return None;
    }
    at += 18;
    let mut page =
        Page { letters, words: String::new(), want, natural, canvas: None, state, saved: Vec::new(), objects: Vec::new(), points: 0, drew: false };
    for _ in 0..MAX_RECORDS {
        let size = size_at(data, at)?.checked_mul(2)?;
        let function = u16_at(data, at.checked_add(4)?)?;
        if size < 6 {
            return None;
        }
        let record = data.get(at..at.checked_add(size)?)?;
        // META_EOF: a drawing of nothing is no picture.
        if function == 0 {
            let file = if page.drew { page.canvas?.bmp()? } else { return None };
            return Some(Drawn { file, words: page.words });
        }
        page.record(function, record)?;
        at = at.checked_add(size)?;
    }
    None
}
