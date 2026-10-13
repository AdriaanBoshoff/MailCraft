//! An enhanced metafile that is a drawing of lines, curves, filled shapes, text and bitmaps,
//! drawn onto a page of pixels ([MS-EMF] 2.3.5 drawing records, 2.3.1 bitmap records, 2.3.7 and
//! 2.3.8 objects, 2.3.10 the path bracket, 2.3.11 state, 2.3.12 transforms, 2.3.2 clipping).
//! Bitmaps are copied whole or in part, mirrored and turned, and mixed with the brush and with
//! what is there ([MS-WMF] 2.1.1.31); brushes are a colour, a hatching or a pattern; regions of
//! boxes are filled, framed, inverted and cut to, and so is any path. A metafile that asks for
//! anything this does not draw (a pen of a pattern or one that does not cover, measures in
//! parts of an inch, text in a font nobody here knows) is not drawn at all: half a picture is
//! worse than its place.

use std::f32::consts::TAU;
use std::sync::Arc;

use openmailclient_fonts::Lettering;

use crate::canvas::{Brush, Canvas, Paint, Point, Rgb, Tile, arc, page_size, rounded, steps};
use crate::metafile::{Blit, Drawn, MAX_DIB_PIXELS, blit, dib_pixels, emf_parts, size_at, u32_at};
use crate::words::{self, Font, Line};

/// Records one drawing is read through.
const MAX_RECORDS: usize = 50_000;
/// Points all of a drawing's shapes may have between them.
const MAX_POINTS: usize = 500_000;
/// Pens, brushes and the like one drawing may make.
const MAX_OBJECTS: usize = 4096;
/// States that may be saved one on another.
const MAX_SAVED: usize = 64;
/// Straight pieces one curve is drawn as.
const CURVE: usize = 16;

/// Characters of text in one line.
const MAX_LINE: usize = 4096;
/// Boxes one region is of, and the pixels of a brush's pattern.
const MAX_BOXES: usize = 4096;
const MAX_TILE: usize = 65_536;

/// Records that change nothing that is drawn here: ways of bitmaps, the palette, colour
/// matching, comments ([MS-EMF] 2.3.11, 2.3.3).
const IGNORED: [u32; 13] = [13, 16, 21, 23, 28, 48, 50, 51, 52, 58, 70, 98, 100];

/// A whole number of the record, as a measure.
fn num(data: &[u8], at: usize) -> Option<f32> {
    Some(u32_at(data, at)?.cast_signed() as f32)
}

fn short(data: &[u8], at: usize) -> Option<f32> {
    let bytes = data.get(at..at.checked_add(2)?)?;
    Some(f32::from(i16::from_le_bytes(<[u8; 2]>::try_from(bytes).ok()?)))
}

/// A fraction of the record; one that is no number is not read.
fn real(data: &[u8], at: usize) -> Option<f32> {
    Some(f32::from_bits(u32_at(data, at)?)).filter(|v| v.is_finite())
}

fn point(data: &[u8], at: usize) -> Option<Point> {
    Some((num(data, at)?, num(data, at.checked_add(4)?)?))
}

/// [MS-WMF] 2.2.2.8, ColorRef; a number in a palette is not read.
fn colour_at(data: &[u8], at: usize) -> Option<Rgb> {
    match *data.get(at..at.checked_add(4)?)? {
        [r, g, b, flag] if flag != 1 => Some([r, g, b]),
        _ => None,
    }
}

/// `count` points from `at`: of two whole numbers each, long ones when `long`.
fn points(data: &[u8], at: usize, count: usize, long: bool) -> Option<Vec<Point>> {
    let each = if long { 8 } else { 4 };
    let all = data.get(at..at.checked_add(count.checked_mul(each)?)?)?;
    all.chunks_exact(each).map(|p| if long { point(p, 0) } else { Some((short(p, 0)?, short(p, 2)?)) }).collect()
}

/// [MS-EMF] 2.2.28, XForm: where a point goes is `(x·m0 + y·m2 + m4, x·m1 + y·m3 + m5)`.
#[derive(Clone, Copy, PartialEq)]
struct Turn([f32; 6]);

const SAME: Turn = Turn([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

impl Turn {
    fn at(&self, (x, y): Point) -> Point {
        let m = self.0;
        (x * m[0] + y * m[2] + m[4], x * m[1] + y * m[3] + m[5])
    }

    /// This, and then `next`.
    fn then(&self, next: &Turn) -> Turn {
        let (a, b) = (self.0, next.0);
        Turn([
            a[0] * b[0] + a[1] * b[2],
            a[0] * b[1] + a[1] * b[3],
            a[2] * b[0] + a[3] * b[2],
            a[2] * b[1] + a[3] * b[3],
            a[4] * b[0] + a[5] * b[2] + b[4],
            a[4] * b[1] + a[5] * b[3] + b[5],
        ])
    }

    fn area(&self) -> f32 {
        self.0[0] * self.0[3] - self.0[1] * self.0[2]
    }

    /// How much longer a length is after it.
    fn grow(&self) -> f32 {
        self.area().abs().sqrt()
    }

    /// Whether a box is still a box that stands straight after it.
    fn straight(&self) -> bool {
        self.0[1] == 0.0 && self.0[2] == 0.0
    }
}

#[derive(Clone, Copy)]
struct Pen {
    width: f32,
    /// The width is in the drawing's own units, not the device's.
    own: bool,
    colour: Rgb,
}

#[derive(Clone)]
enum Object {
    Pen(Option<Pen>),
    Brush(Brush),
    Font(Arc<Font>),
    Other,
    /// A pen that draws a pattern: a drawing that selects it is not drawn.
    Unknown,
}

type Clip = (f32, f32, f32, f32);

/// What a drawing is drawn with and where ([MS-EMF] 3.1.1.2, the playback device context).
#[derive(Clone)]
struct State {
    world: Turn,
    /// The window and the viewport, each its origin and its extent; the extents count only
    /// when the mapping mode says so ([MS-EMF] 2.1.21, MM_ISOTROPIC and MM_ANISOTROPIC).
    window: (Point, Point),
    viewport: (Point, Point),
    scaled: bool,
    pen: Option<Pen>,
    brush: Brush,
    winding: bool,
    clockwise: bool,
    at: Point,
    /// On the page; `None` is all of it.
    clip: Option<Clip>,
    /// How much of each of the page's pixels can be drawn on; `None` is all of each.
    mask: Option<Arc<Vec<u8>>>,
    /// What text is written in; `None` is one of the device's own fonts, which nobody here
    /// knows.
    font: Option<Arc<Font>>,
    /// The colour of letters, the colour behind them, and whether that is filled in.
    ink: Rgb,
    back: Rgb,
    opaque: bool,
    /// [MS-WMF] 2.1.2.3, TextAlignmentMode.
    align: u32,
}

/// Points on the page, and whether the line through them goes back to the first.
type Figure = (Vec<Point>, bool);

struct Page<'a> {
    /// Who gives the shapes of letters, when anybody does.
    letters: Option<&'a dyn Lettering>,
    /// What the drawing's text says.
    words: String,
    canvas: Canvas,
    /// From the device's units to the page's pixels.
    frame: Turn,
    state: State,
    saved: Vec<State>,
    objects: Vec<Option<Object>>,
    path: Vec<Figure>,
    /// Shapes go into the path, not onto the page.
    building: bool,
    points: usize,
    /// Pixels of the bitmaps drawn so far.
    pixels: usize,
    drew: bool,
}

/// The straight pieces of the curves through `through`: a start, then two points that pull
/// and one that is reached, for each curve.
fn curves(through: &[Point]) -> Vec<Point> {
    let Some((&start, rest)) = through.split_first() else { return Vec::new() };
    let mut line = vec![start];
    let mut from = start;
    for curve in rest.chunks_exact(3) {
        let (Some(&a), Some(&b), Some(&to)) = (curve.first(), curve.get(1), curve.get(2)) else { break };
        for i in 1..=CURVE {
            let t = i as f32 / CURVE as f32;
            let s = 1.0 - t;
            let part = |p: f32, a: f32, b: f32, q: f32| s * s * s * p + 3.0 * s * s * t * a + 3.0 * s * t * t * b + t * t * t * q;
            line.push((part(from.0, a.0, b.0, to.0), part(from.1, a.1, b.1, to.1)));
        }
        from = to;
    }
    line
}

/// A box given by two corners, as left, top, right, bottom.
fn boxed(a: Point, b: Point) -> Clip {
    (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))
}

/// The boxes of the region whose header is at `head` of a record, each moved `by`
/// ([MS-EMF] 2.2.24, RegionData: a header of 32 bytes that says they are boxes and how many,
/// then two corners for each).
fn region(record: &[u8], head: usize, by: &Turn) -> Option<Vec<Vec<Point>>> {
    let count = size_at(record, head.checked_add(8)?)?;
    if u32_at(record, head.checked_add(4)?)? != 1 || count > MAX_BOXES {
        return None;
    }
    (0..count)
        .map(|i| {
            let at = head.checked_add(32 + 16 * i)?;
            let (l, t, r, b) = boxed(point(record, at)?, point(record, at.checked_add(8)?)?);
            Some(vec![by.at((l, t)), by.at((r, t)), by.at((r, b)), by.at((l, b))])
        })
        .collect()
}

impl Page<'_> {
    /// From the drawing's units to the device's: the world's turn, the window onto the
    /// viewport.
    fn device(&self) -> Turn {
        let State { window, viewport, scaled, .. } = self.state;
        let by = if scaled { (viewport.1.0 / window.1.0, viewport.1.1 / window.1.1) } else { (1.0, 1.0) };
        let page = Turn([by.0, 0.0, 0.0, by.1, viewport.0.0 - window.0.0 * by.0, viewport.0.1 - window.0.1 * by.1]);
        self.state.world.then(&page)
    }

    /// From the drawing's units to the page's pixels: to the device, and the device onto the
    /// page.
    fn total(&self) -> Turn {
        self.device().then(&self.frame)
    }

    /// Draw inside what the drawing is cut to.
    fn aim(&mut self) {
        self.canvas.clip = self.state.clip.unwrap_or(self.all());
        self.canvas.mask = self.state.mask.clone();
    }

    /// The colour between the lines of a hatching (none when what is behind shows through),
    /// and how many of the page's pixels one of a pattern's is.
    fn with(&self) -> (Option<Rgb>, f32) {
        (self.state.opaque.then_some(self.state.back), self.frame.grow().clamp(1.0, 8.0))
    }

    /// The brush that was made as object `which`, or one that every device has.
    fn brush(&self, which: u32) -> Option<Brush> {
        if which & 0x8000_0000 != 0 {
            return Some(match which & 0x7FFF_FFFF {
                0 => Brush::Solid([255; 3]),
                1 => Brush::Solid([192; 3]),
                2 => Brush::Solid([128; 3]),
                3 => Brush::Solid([64; 3]),
                4 => Brush::Solid([0; 3]),
                5 => Brush::Null,
                _ => return None,
            });
        }
        match self.objects.get(usize::try_from(which).ok()?)?.as_ref()? {
            Object::Brush(brush) => Some(brush.clone()),
            _ => None,
        }
    }

    /// Copy onto the box `to` of the drawing (left, top, wide, high) the part `from` of a
    /// bitmap (the same four, in its pixels), the brush, or what is there, mixed the way `how`
    /// says ([MS-WMF] 2.1.1.31). The box is turned as the drawing is.
    fn copy(&mut self, how: u32, dib: Option<&[u8]>, from: Clip, to: Clip, upwards: bool) -> Option<()> {
        let (brush, with, forward) = (self.state.brush.clone(), self.with(), self.total().0);
        self.aim();
        let drew = blit(&mut self.canvas, &Blit { how, dib, from, to, upwards, forward, brush: &brush, with }, &mut self.pixels)?;
        self.drew |= drew;
        Some(())
    }

    /// Join `rings` to what the drawing is cut to ([MS-EMF] 2.1.29, RegionMode).
    fn cut_to(&mut self, rings: &[Vec<Point>], winding: bool, mode: u32) -> Option<()> {
        let was = (self.state.clip, self.state.mask.clone());
        (self.state.clip, self.state.mask) = self.canvas.cut(was, rings, winding, mode)?;
        Some(())
    }

    fn count(&mut self, points: usize) -> Option<()> {
        self.points = self.points.checked_add(points)?;
        (self.points <= MAX_POINTS).then_some(())
    }

    fn all(&self) -> Clip {
        (0.0, 0.0, self.canvas.w as f32, self.canvas.h as f32)
    }

    /// Fill `figures` with the brush when `fill`, and draw the pen along them when `line`.
    fn paint(&mut self, figures: &[Figure], fill: bool, line: bool) -> Option<()> {
        let (pen, winding, brush, with) = (self.state.pen, self.state.winding, self.state.brush.clone(), self.with());
        let grow = (self.total().grow(), self.frame.grow());
        self.aim();
        if fill && !matches!(brush, Brush::Null) {
            let rings: Vec<Vec<Point>> = figures.iter().map(|figure| figure.0.clone()).collect();
            self.canvas.brushed(&rings, winding, &brush, with, None)?;
        }
        if line && let Some(pen) = pen {
            let width = (pen.width * if pen.own { grow.0 } else { grow.1 }).max(1.0);
            for (ring, closed) in figures {
                self.canvas.stroke(ring, *closed, width, pen.colour)?;
            }
        }
        self.drew = true;
        Some(())
    }

    /// Whole shapes, in the drawing's units: into the path, or onto the page.
    fn shapes(&mut self, rings: Vec<Vec<Point>>, closed: bool) -> Option<()> {
        let total = self.total();
        let mut figures = Vec::with_capacity(rings.len());
        for ring in rings {
            self.count(ring.len().max(1))?;
            figures.push((ring.into_iter().map(|p| total.at(p)).collect(), closed));
        }
        if self.building {
            self.path.extend(figures);
            Some(())
        } else {
            self.paint(&figures, closed, true)
        }
    }

    /// More of the line that runs on from where the last one ended, in the drawing's units.
    fn run_on(&mut self, more: &[Point]) -> Option<()> {
        let Some(&end) = more.last() else { return Some(()) };
        self.count(more.len())?;
        let total = self.total();
        let from = total.at(self.state.at);
        let more = more.iter().map(|&p| total.at(p));
        if self.building {
            match self.path.last_mut() {
                Some((figure, false)) => figure.extend(more),
                _ => self.path.push((std::iter::once(from).chain(more).collect(), false)),
            }
        } else {
            self.paint(&[(std::iter::once(from).chain(more).collect(), false)], false, true)?;
        }
        self.state.at = end;
        Some(())
    }

    fn keep(&mut self, record: &[u8], object: Object) -> Option<()> {
        let at = size_at(record, 8)?;
        if at >= MAX_OBJECTS {
            return None;
        }
        if self.objects.len() <= at {
            self.objects.resize(at + 1, None);
        }
        *self.objects.get_mut(at)? = Some(object);
        Some(())
    }

    /// [MS-EMF] 2.1.31, StockObject: the pens and brushes every device has.
    fn stock(&mut self, which: u32) {
        let grey = |v: u8| Brush::Solid([v; 3]);
        let pen = |v: u8| Some(Pen { width: 1.0, own: false, colour: [v; 3] });
        match which {
            0 | 18 => self.state.brush = grey(255),
            1 => self.state.brush = grey(192),
            2 => self.state.brush = grey(128),
            3 => self.state.brush = grey(64),
            4 => self.state.brush = grey(0),
            5 => self.state.brush = Brush::Null,
            6 => self.state.pen = pen(255),
            7 | 19 => self.state.pen = pen(0),
            8 => self.state.pen = None,
            10..=14 | 16 | 17 => self.state.font = None,
            _ => {}
        }
    }

    /// Write the line of text of an EMR_EXTTEXTOUTA or EMR_EXTTEXTOUTW record, whose
    /// characters are `wide` or bytes ([MS-EMF] 2.2.5, EmrText: the point the line is set by,
    /// how many characters, where they are, what to do with the box, the box, and where the
    /// spaces from each character to the next are).
    fn say(&mut self, record: &[u8], wide: bool) -> Option<()> {
        let (at, count, start, options) = (point(record, 36)?, size_at(record, 44)?, size_at(record, 48)?, u32_at(record, 52)?);
        // 0x0200: the characters are bytes although the record says otherwise.
        if options & (words::UNDRAWN | 0x0200) != 0 || self.state.align & words::RIGHT_TO_LEFT != 0 || self.building || count > MAX_LINE {
            return None;
        }
        // 0x0100: there is no box.
        let (frame, spaces) =
            if options & 0x0100 != 0 { (None, size_at(record, 56)?) } else { (Some((point(record, 56)?, point(record, 64)?)), size_at(record, 72)?) };
        let total = self.total();
        if !total.straight() {
            return None;
        }
        let State { ink, back, opaque, align, clip, .. } = self.state;
        let all = clip.unwrap_or(self.all());
        self.aim();
        let frame = frame.map(|(a, b)| boxed(total.at(a), total.at(b)));
        if options & words::OPAQUE != 0
            && let Some((l, t, r, b)) = frame
        {
            self.canvas.fill(&[vec![(l, t), (r, t), (r, b), (l, b)]], true, back)?;
            self.drew = true;
        }
        if count == 0 {
            return Some(());
        }
        let (lettering, font) = (self.letters?, self.state.font.clone()?);
        let bytes = record.get(start..start.checked_add(count.checked_mul(if wide { 2 } else { 1 })?)?)?;
        let text = if wide {
            let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
            String::from_utf16(&units).ok()?
        } else {
            font.read(bytes)?
        };
        self.count(count)?;
        let dx = if spaces == 0 { None } else { (0..count).map(|i| num(record, spaces.checked_add(4 * i)?)).collect::<Option<Vec<f32>>>() };
        let from = if align & words::UPDATE_PLACE != 0 { self.state.at } else { at };
        if options & words::CLIPPED != 0
            && let Some((l, t, r, b)) = frame
        {
            self.canvas.clip = (l.max(all.0), t.max(all.1), r.min(all.2), b.min(all.3));
        }
        let scale = (total.0[0].abs(), total.0[3].abs());
        let line = Line { at: total.at(from), scale, text: &text, dx: dx.as_deref(), align, colour: ink, back: opaque.then_some(back) };
        let long = words::write(&mut self.canvas, lettering, &font, &line);
        self.canvas.clip = all;
        // The next line starts where this one ended, when the drawing keeps its place.
        if align & words::UPDATE_PLACE != 0 && align & 6 == 0 && scale.0 != 0.0 {
            self.state.at.0 += long? / scale.0;
        }
        long?;
        words::note(&mut self.words, &text);
        self.drew = true;
        Some(())
    }

    /// Cut what is drawn from here on down to `to`, or to that and what it was cut to before.
    fn cut(&mut self, to: Clip, mode: u32) -> Option<()> {
        if ![to.0, to.1, to.2, to.3].iter().all(|v| v.is_finite()) {
            return None;
        }
        let was = self.state.clip.unwrap_or(self.all());
        match mode {
            // [MS-EMF] 2.1.29, RegionMode: RGN_AND and RGN_COPY; then the ways that leave what
            // is no box.
            1 => self.state.clip = Some((to.0.max(was.0), to.1.max(was.1), to.2.min(was.2), to.3.min(was.3))),
            5 => (self.state.clip, self.state.mask) = (Some(to), None),
            _ => self.cut_to(&[vec![(to.0, to.1), (to.2, to.1), (to.2, to.3), (to.0, to.3)]], true, mode)?,
        }
        Some(())
    }

    /// The part of an ellipse in the box at byte 8 of `record`, from where the line from its
    /// middle to the point at byte 24 crosses it round to the one for the point at byte 32,
    /// in the drawing's units; and the ellipse's middle.
    fn part(&self, record: &[u8]) -> Option<(Vec<Point>, Point)> {
        let (l, t, r, b) = boxed(point(record, 8)?, point(record, 16)?);
        let (centre, radii) = (((l + r) / 2.0, (t + b) / 2.0), ((r - l) / 2.0, (b - t) / 2.0));
        if radii.0 <= 0.0 || radii.1 <= 0.0 {
            return Some((Vec::new(), centre));
        }
        let angle = |p: Point| (-(p.1 - centre.1) / radii.1).atan2((p.0 - centre.0) / radii.0);
        let (from, to) = (angle(point(record, 24)?), angle(point(record, 32)?));
        // Against the clock as it is seen on the device, unless the drawing says otherwise; a
        // window that turns the drawing over turns that round with it.
        let total = self.total();
        let against = !self.state.clockwise ^ (self.state.world == SAME && total.area() < 0.0);
        let (from, to) = if against { (from, to) } else { (to, from) };
        let turn = if to > from { to - from } else { to - from + TAU };
        let mut ring = arc(centre, radii, from, from + turn, steps((radii.0 * total.grow(), radii.1 * total.grow())));
        if !against {
            ring.reverse();
        }
        Some((ring, centre))
    }

    /// Do what one record says. `None` when it is cut short or asks for what is not drawn.
    fn record(&mut self, kind: u32, record: &[u8]) -> Option<()> {
        // Records of points come with long numbers or with short ones ([MS-EMF] 2.3.5.24 on).
        let long = kind < 85;
        match kind {
            // EMR_POLYBEZIER, EMR_POLYGON, EMR_POLYLINE and the ones of short numbers: the box
            // they fill, how many points, and the points.
            2..=4 | 85..=87 => {
                let ring = points(record, 28, size_at(record, 24)?, long)?;
                let ring = if matches!(kind, 2 | 85) { curves(&ring) } else { ring };
                self.shapes(vec![ring], matches!(kind, 3 | 86))?;
            }
            // EMR_POLYBEZIERTO, EMR_POLYLINETO: the same, on from where the last line ended.
            5 | 6 | 88 | 89 => {
                let mut more = points(record, 28, size_at(record, 24)?, long)?;
                if matches!(kind, 5 | 88) {
                    more.insert(0, self.state.at);
                    more = curves(&more).split_off(1);
                }
                self.run_on(&more)?;
            }
            // EMR_POLYPOLYLINE, EMR_POLYPOLYGON: how many there are, how many points in all, how
            // many each has, and the points.
            7 | 8 | 90 | 91 => {
                let (many, all) = (size_at(record, 24)?, size_at(record, 28)?);
                let mut at = 32usize.checked_add(many.checked_mul(4)?)?;
                self.count(many)?;
                let mut rings = Vec::new();
                let mut left = all;
                for i in 0..many {
                    let count = size_at(record, 32 + 4 * i)?;
                    left = left.checked_sub(count)?;
                    rings.push(points(record, at, count, long)?);
                    at = at.checked_add(count.checked_mul(if long { 8 } else { 4 })?)?;
                }
                if matches!(kind, 8 | 91) {
                    self.shapes(rings, true)?;
                } else {
                    for ring in rings {
                        self.shapes(vec![ring], false)?;
                    }
                }
            }
            // EMR_SETWINDOWEXTEX, EMR_SETWINDOWORGEX, EMR_SETVIEWPORTEXTEX, EMR_SETVIEWPORTORGEX.
            9 | 11 => {
                let extent = point(record, 8)?;
                if extent.0 == 0.0 || extent.1 == 0.0 {
                    return None;
                }
                if kind == 9 { self.state.window.1 = extent } else { self.state.viewport.1 = extent }
            }
            10 => self.state.window.0 = point(record, 8)?,
            12 => self.state.viewport.0 = point(record, 8)?,
            // EMR_SETPIXELV: a point and the colour it takes.
            15 => {
                let (at, colour) = (self.total().at(point(record, 8)?), colour_at(record, 16)?);
                if !self.building {
                    self.count(1)?;
                    let side = self.frame.grow().max(1.0);
                    self.aim();
                    self.canvas.fill(&[vec![at, (at.0 + side, at.1), (at.0 + side, at.1 + side), (at.0, at.1 + side)]], true, colour)?;
                    self.drew = true;
                }
            }
            // EMR_SETMAPMODE: the device's own units, or the window onto the viewport; the
            // modes that count in parts of an inch or of a metre are not read.
            17 => {
                self.state.scaled = match u32_at(record, 8)? {
                    1 => false,
                    7 | 8 => true,
                    _ => return None,
                }
            }
            // EMR_SETPOLYFILLMODE; EMR_SETROP2, where only a pen that covers is drawn;
            // EMR_SETARCDIRECTION; EMR_SETLAYOUT, where only left to right is drawn.
            19 => self.state.winding = u32_at(record, 8)? == 2,
            20 => {
                if u32_at(record, 8)? != 13 {
                    return None;
                }
            }
            57 => self.state.clockwise = u32_at(record, 8)? == 2,
            115 => {
                if u32_at(record, 8)? != 0 {
                    return None;
                }
            }
            // EMR_MOVETOEX, EMR_LINETO.
            27 => {
                self.state.at = point(record, 8)?;
                if self.building {
                    self.count(1)?;
                    self.path.push((vec![self.total().at(self.state.at)], false));
                }
            }
            54 => self.run_on(&[point(record, 8)?])?,
            // EMR_EXCLUDECLIPRECT, EMR_INTERSECTCLIPRECT: a box in the drawing's units, which
            // what is drawn from here on stays out of, or inside.
            29 | 30 => {
                let total = self.total();
                let (l, t, r, b) = boxed(point(record, 8)?, point(record, 16)?);
                if kind == 30 && total.straight() {
                    self.cut(boxed(total.at((l, t)), total.at((r, b))), 1)?;
                } else {
                    self.count(4)?;
                    let ring = vec![total.at((l, t)), total.at((r, t)), total.at((r, b)), total.at((l, b))];
                    self.cut_to(&[ring], true, if kind == 30 { 1 } else { 4 })?;
                }
            }
            // EMR_SAVEDC, EMR_RESTOREDC: back so many states, or to the one of that number.
            33 => {
                if self.saved.len() >= MAX_SAVED {
                    return None;
                }
                self.saved.push(self.state.clone());
            }
            34 => {
                let n = u32_at(record, 8)?.cast_signed();
                let back = usize::try_from(n.unsigned_abs()).ok()?;
                let keep = if n < 0 { self.saved.len().checked_sub(back)? } else { back.checked_sub(1)? };
                self.state = self.saved.get(keep)?.clone();
                self.saved.truncate(keep);
            }
            // EMR_SETWORLDTRANSFORM, EMR_MODIFYWORLDTRANSFORM ([MS-EMF] 2.1.24: to none, before
            // what there is, after it, or in its place).
            35 | 36 => {
                let mut turn = SAME;
                for (i, part) in turn.0.iter_mut().enumerate() {
                    *part = real(record, 8 + 4 * i)?;
                }
                self.state.world = match if kind == 35 { 4 } else { u32_at(record, 32)? } {
                    1 => SAME,
                    2 => turn.then(&self.state.world),
                    3 => self.state.world.then(&turn),
                    4 => turn,
                    _ => return None,
                };
            }
            // EMR_SELECTOBJECT: one that was made, or one every device has.
            37 => {
                let which = u32_at(record, 8)?;
                if which & 0x8000_0000 != 0 {
                    self.stock(which & 0x7FFF_FFFF);
                } else {
                    match self.objects.get(usize::try_from(which).ok()?)?.clone()? {
                        Object::Pen(pen) => self.state.pen = pen,
                        Object::Brush(brush) => self.state.brush = brush,
                        Object::Font(font) => self.state.font = Some(font),
                        Object::Other => {}
                        Object::Unknown => return None,
                    }
                }
            }
            // EMR_CREATEPEN ([MS-EMF] 2.2.19, LogPen): its style, its width, its colour.
            38 => {
                let pen = if u32_at(record, 12)? & 0x0F == 5 {
                    None
                } else {
                    Some(Pen { width: num(record, 16)?.abs(), own: true, colour: colour_at(record, 24)? })
                };
                self.keep(record, Object::Pen(pen))?;
            }
            // EMR_CREATEBRUSHINDIRECT ([MS-EMF] 2.2.12, LogBrushEx).
            39 => {
                let brush = match (u32_at(record, 12)?, u32_at(record, 20)) {
                    (0, _) => Brush::Solid(colour_at(record, 16)?),
                    (1, _) => Brush::Null,
                    // [MS-WMF] 2.1.1.12, HatchStyle.
                    (2, Some(kind @ 0..=5)) => Brush::Hatch(kind, colour_at(record, 16)?),
                    _ => Brush::Unknown,
                };
                self.keep(record, Object::Brush(brush))?;
            }
            // EMR_EXTCREATEPEN ([MS-EMF] 2.2.20, LogPenEx): a pen as wide as the device's unit,
            // or one measured in the drawing's; one that draws with a pattern is not read.
            95 => {
                let (style, solid) = (u32_at(record, 28)?, u32_at(record, 36)? == 0);
                let own = style & 0x0001_0000 != 0;
                let object = if style & 0x0F == 5 {
                    Object::Pen(None)
                } else if solid {
                    Object::Pen(Some(Pen { width: if own { u32_at(record, 32)? as f32 } else { 1.0 }, own, colour: colour_at(record, 40)? }))
                } else {
                    Object::Unknown
                };
                self.keep(record, object)?;
            }
            // [MS-EMF] 2.3.7.5, EMR_CREATEMONOBRUSH, and 2.3.7.4, EMR_CREATEDIBPATTERNBRUSHPT:
            // what the colours are numbers in, and the two parts of a bitmap that is laid side
            // by side. Then a palette and a colour space, which only take a place.
            93 | 94 => {
                let most = MAX_TILE.min(MAX_DIB_PIXELS.saturating_sub(self.pixels));
                let tile = if u32_at(record, 12)? == 0 { emf_parts(record, 16).and_then(|dib| dib_pixels(&dib, most)) } else { None };
                let brush = match tile {
                    Some(((w, h), rgba)) => {
                        self.pixels = self.pixels.saturating_add(w.saturating_mul(h));
                        Brush::Tile(Arc::new(Tile { w, h, rgba }))
                    }
                    None => Brush::Unknown,
                };
                self.keep(record, Object::Brush(brush))?;
            }
            49 | 99 | 122 => self.keep(record, Object::Other)?,
            // EMR_EXTCREATEFONTINDIRECTW ([MS-EMF] 2.2.13, LogFont): its height, how it is
            // turned, how heavy it is, whether it leans, is underlined or struck out, its
            // character set, and its name in up to 32 characters.
            82 => {
                let byte = |at: usize| record.get(at).copied();
                let name: Vec<u16> = record
                    .get(40..)?
                    .chunks_exact(2)
                    .take(32)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .take_while(|&unit| unit != 0)
                    .collect();
                let font = Font {
                    height: num(record, 12)?,
                    turn: num(record, 20)?,
                    bold: u32_at(record, 28)?.cast_signed() >= 600,
                    italic: byte(32)? != 0,
                    underline: byte(33)? != 0,
                    strike: byte(34)? != 0,
                    charset: byte(35)?,
                    family: String::from_utf16_lossy(&name),
                };
                self.keep(record, Object::Font(Arc::new(font)))?;
            }
            // EMR_SETBKMODE, EMR_SETTEXTALIGN, EMR_SETTEXTCOLOR, EMR_SETBKCOLOR.
            18 => self.state.opaque = u32_at(record, 8)? == 2,
            22 => self.state.align = u32_at(record, 8)?,
            24 => self.state.ink = colour_at(record, 8)?,
            25 => self.state.back = colour_at(record, 8)?,
            // EMR_EXTTEXTOUTA, EMR_EXTTEXTOUTW.
            83 | 84 => self.say(record, kind == 84)?,
            // EMR_DELETEOBJECT.
            40 => {
                if let Some(place) = self.objects.get_mut(size_at(record, 8)?) {
                    *place = None;
                }
            }
            // EMR_ELLIPSE, EMR_RECTANGLE, EMR_ROUNDRECT: the box, and for the last the width and
            // height of the ellipse its corners are cut from.
            42..=44 => {
                let (l, t, r, b) = boxed(point(record, 8)?, point(record, 16)?);
                let grow = self.total().grow();
                let ring = match kind {
                    42 => {
                        let radii = ((r - l) / 2.0, (b - t) / 2.0);
                        arc(((l + r) / 2.0, (t + b) / 2.0), radii, 0.0, TAU, steps((radii.0 * grow, radii.1 * grow)))
                    }
                    43 => vec![(l, t), (r, t), (r, b), (l, b)],
                    _ => {
                        let corner = point(record, 24)?;
                        let radii = (corner.0.abs().min(r - l) / 2.0, corner.1.abs().min(b - t) / 2.0);
                        rounded((l, t, r, b), radii, steps((radii.0 * grow, radii.1 * grow)))
                    }
                };
                self.shapes(vec![ring], true)?;
            }
            // EMR_ARC, EMR_CHORD, EMR_PIE; EMR_ARCTO, which runs on from the last line.
            45..=47 => {
                let (mut ring, centre) = self.part(record)?;
                if ring.is_empty() {
                    return Some(());
                }
                if kind == 47 {
                    ring.push(centre);
                }
                self.shapes(vec![ring], kind != 45)?;
            }
            55 => {
                let (ring, _) = self.part(record)?;
                self.run_on(&ring)?;
            }
            // [MS-EMF] 2.3.10, the path bracket: EMR_BEGINPATH, EMR_ENDPATH, EMR_CLOSEFIGURE,
            // EMR_ABORTPATH.
            59 => {
                self.path.clear();
                self.building = true;
            }
            60 => self.building = false,
            61 => {
                if let Some(figure) = self.path.last_mut() {
                    figure.1 = true;
                }
            }
            66 => {
                self.path.clear();
                self.building = false;
            }
            // EMR_FILLPATH, EMR_STROKEANDFILLPATH, EMR_STROKEPATH: the path is used up.
            62..=64 => {
                let path = std::mem::take(&mut self.path);
                self.building = false;
                if !path.is_empty() {
                    self.paint(&path, kind != 64, kind != 62)?;
                }
            }
            // EMR_SELECTCLIPPATH: the path is cut to. One that is a box standing straight stays
            // a box; any other is what it would fill.
            67 => {
                let path = std::mem::take(&mut self.path);
                self.building = false;
                let mode = u32_at(record, 8)?;
                if path.is_empty() {
                    return None;
                }
                let stands = || {
                    let [(ring, _)] = &path[..] else { return None };
                    let corners = match &ring[..] {
                        [a, b, c, d] => [a, b, c, d],
                        [a, b, c, d, again] if again == a => [a, b, c, d],
                        _ => return None,
                    };
                    let to = corners.iter().fold((f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY), |to, p| {
                        (to.0.min(p.0), to.1.min(p.1), to.2.max(p.0), to.3.max(p.1))
                    });
                    corners.iter().all(|p| (p.0 == to.0 || p.0 == to.2) && (p.1 == to.1 || p.1 == to.3)).then_some(to)
                };
                match stands() {
                    Some(to) if matches!(mode, 1 | 5) => self.cut(to, mode)?,
                    _ => {
                        let rings: Vec<Vec<Point>> = path.into_iter().map(|figure| figure.0).collect();
                        self.cut_to(&rings, self.state.winding, mode)?;
                    }
                }
            }
            // EMR_EXTSELECTCLIPRGN: no region, or one of boxes in the device's units
            // ([MS-EMF] 2.2.24, RegionData), joined to what the drawing is cut to.
            75 => {
                let (size, mode) = (size_at(record, 8)?, u32_at(record, 12)?);
                if size == 0 {
                    if mode != 5 {
                        return None;
                    }
                    (self.state.clip, self.state.mask) = (None, None);
                } else {
                    let rings = region(record, 16, &self.frame)?;
                    self.count(rings.len().checked_mul(4)?)?;
                    match (&rings[..], mode) {
                        ([], 1 | 5) => self.cut((0.0, 0.0, 0.0, 0.0), mode)?,
                        ([ring], 1 | 5) => {
                            let (Some(&a), Some(&c)) = (ring.first(), ring.get(2)) else { return None };
                            self.cut(boxed(a, c), mode)?;
                        }
                        _ => self.cut_to(&rings, true, mode)?,
                    }
                }
            }
            // [MS-EMF] 2.3.1.7, EMR_STRETCHDIBITS: where it goes, the part of the bitmap that is
            // taken (its rows counted from the bottom of a bitmap that is kept that way), the
            // bitmap's two parts, what its colours are numbers in, how it is copied, and how
            // large it is made. A width or a height below nothing mirrors it.
            81 => {
                if u32_at(record, 64)? != 0 {
                    return None;
                }
                let dib = if size_at(record, 52)? == 0 { None } else { Some(emf_parts(record, 48)?) };
                let from = (num(record, 32)?, num(record, 36)?, num(record, 40)?, num(record, 44)?);
                let to = (num(record, 24)?, num(record, 28)?, num(record, 72)?, num(record, 76)?);
                self.copy(u32_at(record, 68)?, dib.as_deref(), from, to, true)?;
            }
            // [MS-EMF] 2.3.1.2, EMR_BITBLT, and 2.3.1.6, EMR_STRETCHBLT: the box it goes to, how
            // it is copied, where the part taken starts, how the bitmap's own units are turned
            // (which must leave a box standing straight), what its colours are numbers in, the
            // bitmap's two parts, and for the second how large the part is. The first takes as
            // many pixels as the box has on the device. One with no bitmap copies the brush
            // or what is there.
            76 | 77 => {
                let to = (num(record, 24)?, num(record, 28)?, num(record, 32)?, num(record, 36)?);
                let how = u32_at(record, 40)?;
                if size_at(record, 88)? == 0 {
                    self.copy(how, None, (0.0, 0.0, 0.0, 0.0), to, false)?;
                } else {
                    if u32_at(record, 80)? != 0 {
                        return None;
                    }
                    let dib = emf_parts(record, 84)?;
                    let mut turn = SAME;
                    for (i, part) in turn.0.iter_mut().enumerate() {
                        *part = real(record, 52 + 4 * i)?;
                    }
                    let device = self.device();
                    let size = if kind == 77 {
                        (num(record, 100)?, num(record, 104)?)
                    } else if device.straight() {
                        (to.2 * device.0[0].abs(), to.3 * device.0[3].abs())
                    } else {
                        (to.2, to.3)
                    };
                    if !turn.straight() {
                        return None;
                    }
                    let start = (num(record, 44)?, num(record, 48)?);
                    let (a, b) = (turn.at(start), turn.at((start.0 + size.0, start.1 + size.1)));
                    self.copy(how, Some(&dib), (a.0, a.1, b.0 - a.0, b.1 - a.1), to, false)?;
                }
            }
            // [MS-EMF] 2.3.5.9, EMR_FILLRGN (a brush), 2.3.5.11, EMR_FRAMERGN (a brush, and how
            // wide and high its strokes are), 2.3.5.14, EMR_INVERTRGN, and 2.3.5.26,
            // EMR_PAINTRGN (the brush there is): a region of boxes, in the drawing's units.
            71..=74 => {
                let head = match kind {
                    71 => 32,
                    72 => 40,
                    _ => 28,
                };
                let total = self.total();
                let rings = region(record, head, &total)?;
                self.count(rings.len().checked_mul(4)?)?;
                let brush = if kind < 73 { self.brush(u32_at(record, 28)?)? } else { self.state.brush.clone() };
                let with = self.with();
                self.aim();
                match kind {
                    73 => self.canvas.fill_with(&rings, true, Paint::Solid([0; 3]), Some(0x55))?,
                    72 => self.canvas.framed(&rings, (num(record, 32)? * total.grow(), num(record, 36)? * total.grow()), &brush, with)?,
                    _ => self.canvas.brushed(&rings, true, &brush, with, None)?,
                }
                self.drew = true;
            }
            _ if IGNORED.contains(&kind) => {}
            _ => return None,
        }
        Some(())
    }
}

/// An enhanced metafile that draws lines, curves, filled shapes, text and bitmaps and nothing else → a
/// BMP file of it, for showing at `want` pixels when that is known. Text is drawn in the fonts
/// `letters` knows; with nobody to ask, a metafile with text in it is not drawn.
pub(crate) fn emf_drawing(data: &[u8], want: Option<(u32, u32)>, letters: Option<&dyn Lettering>) -> Option<Drawn> {
    // [MS-EMF] 2.2.9, Header: the box that was drawn in, in the device's units; the picture's
    // frame, in hundredths of a millimetre; and the device's size both ways.
    if u32_at(data, 0)? != 1 || u32_at(data, 40)? != 0x464D_4520 {
        return None;
    }
    let mut at = size_at(data, 4)?;
    if at < 88 || !at.is_multiple_of(4) {
        return None;
    }
    let (bounds, frame) = ((point(data, 8)?, point(data, 16)?), (point(data, 24)?, point(data, 32)?));
    let (device, millimetres) = (point(data, 72)?, point(data, 80)?);
    let per = (device.0 / (millimetres.0 * 100.0), device.1 / (millimetres.1 * 100.0));
    let framed = frame.1.0 > frame.0.0 && frame.1.1 > frame.0.1;
    // The part of the device the picture is: its frame, or else the box that was drawn in.
    let (area, natural) = if framed && per.0.is_finite() && per.1.is_finite() && per.0 > 0.0 && per.1 > 0.0 {
        let area = ((frame.0.0 * per.0, frame.0.1 * per.1), (frame.1.0 * per.0, frame.1.1 * per.1));
        (area, Some(((frame.1.0 - frame.0.0) / 2540.0 * 96.0, (frame.1.1 - frame.0.1) / 2540.0 * 96.0)))
    } else {
        ((bounds.0, (bounds.1.0 + 1.0, bounds.1.1 + 1.0)), None)
    };
    let (w, h) = (area.1.0 - area.0.0, area.1.1 - area.0.1);
    if w.is_nan() || h.is_nan() || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let (cw, ch) = page_size(want, natural, (w, h))?;
    let canvas = Canvas::new(cw, ch)?;
    let by = (cw as f32 / w, ch as f32 / h);
    let frame = Turn([by.0, 0.0, 0.0, by.1, -area.0.0 * by.0, -area.0.1 * by.1]);
    let state = State {
        world: SAME,
        window: ((0.0, 0.0), (1.0, 1.0)),
        viewport: ((0.0, 0.0), (1.0, 1.0)),
        scaled: false,
        pen: Some(Pen { width: 1.0, own: false, colour: [0; 3] }),
        brush: Brush::Solid([255; 3]),
        winding: false,
        clockwise: false,
        at: (0.0, 0.0),
        clip: None,
        mask: None,
        font: None,
        ink: [0; 3],
        back: [255; 3],
        opaque: true,
        align: 0,
    };
    let mut page = Page {
        letters,
        words: String::new(),
        canvas,
        frame,
        state,
        saved: Vec::new(),
        objects: Vec::new(),
        path: Vec::new(),
        building: false,
        points: 0,
        pixels: 0,
        drew: false,
    };
    for _ in 0..MAX_RECORDS {
        // [MS-EMF] 2.3: a record's type and its size in bytes.
        let (kind, size) = (u32_at(data, at)?, size_at(data, at.checked_add(4)?)?);
        if size < 8 || !size.is_multiple_of(4) {
            return None;
        }
        let record = data.get(at..at.checked_add(size)?)?;
        // EMR_EOF: a drawing of nothing is no picture.
        if kind == 14 {
            let file = if page.drew { page.canvas.bmp()? } else { return None };
            return Some(Drawn { file, words: page.words });
        }
        page.record(kind, record)?;
        at = at.checked_add(size)?;
    }
    None
}
