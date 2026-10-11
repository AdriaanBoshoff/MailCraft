//! An enhanced metafile that is a drawing of lines, curves and filled shapes, drawn onto a page
//! of pixels ([MS-EMF] 2.3.5 drawing records, 2.3.7 and 2.3.8 objects, 2.3.10 the path bracket,
//! 2.3.11 state, 2.3.12 transforms, 2.3.2 clipping). A metafile that asks for anything this
//! does not draw (text, bitmaps, patterned brushes, regions of many boxes) is not drawn at all:
//! half a picture is worse than its place.

use std::f32::consts::TAU;

use crate::canvas::{Brush, Canvas, Point, Rgb, arc, page_size, rounded, steps};
use crate::metafile::{size_at, u32_at};

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

/// Records that change nothing that is drawn here: colours and ways of text and of bitmaps,
/// the palette, colour matching, comments ([MS-EMF] 2.3.11, 2.3.3).
const IGNORED: [u32; 17] = [13, 16, 18, 21, 22, 23, 24, 25, 28, 48, 50, 51, 52, 58, 70, 98, 100];

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

#[derive(Clone, Copy)]
enum Object {
    Pen(Option<Pen>),
    Brush(Brush),
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
}

/// Points on the page, and whether the line through them goes back to the first.
type Figure = (Vec<Point>, bool);

struct Page {
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

impl Page {
    /// From the drawing's units to the page's pixels: the world's turn, the window onto the
    /// viewport, the device onto the page.
    fn total(&self) -> Turn {
        let State { window, viewport, scaled, .. } = self.state;
        let by = if scaled { (viewport.1.0 / window.1.0, viewport.1.1 / window.1.1) } else { (1.0, 1.0) };
        let page = Turn([by.0, 0.0, 0.0, by.1, viewport.0.0 - window.0.0 * by.0, viewport.0.1 - window.0.1 * by.1]);
        self.state.world.then(&page).then(&self.frame)
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
        let State { pen, brush, winding, clip, .. } = self.state;
        let grow = (self.total().grow(), self.frame.grow());
        self.canvas.clip = clip.unwrap_or(self.all());
        if fill {
            match brush {
                Brush::Null => {}
                Brush::Solid(colour) => {
                    let rings: Vec<Vec<Point>> = figures.iter().map(|figure| figure.0.clone()).collect();
                    self.canvas.fill(&rings, winding, colour)?;
                }
                Brush::Unknown => return None,
            }
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
            _ => {}
        }
    }

    /// Cut what is drawn from here on down to `to`, or to that and what it was cut to before.
    fn cut(&mut self, to: Clip, mode: u32) -> Option<()> {
        if ![to.0, to.1, to.2, to.3].iter().all(|v| v.is_finite()) {
            return None;
        }
        let was = self.state.clip.unwrap_or(self.all());
        self.state.clip = Some(match mode {
            // [MS-EMF] 2.1.29, RegionMode: RGN_AND and RGN_COPY.
            1 => (to.0.max(was.0), to.1.max(was.1), to.2.min(was.2), to.3.min(was.3)),
            5 => to,
            _ => return None,
        });
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
                    self.canvas.clip = self.state.clip.unwrap_or(self.all());
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
            // EMR_INTERSECTCLIPRECT: a box in the drawing's units, which must stand straight.
            30 => {
                let total = self.total();
                if !total.straight() {
                    return None;
                }
                self.cut(boxed(total.at(point(record, 8)?), total.at(point(record, 16)?)), 1)?;
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
                    match (*self.objects.get(usize::try_from(which).ok()?)?)? {
                        Object::Pen(pen) => self.state.pen = pen,
                        Object::Brush(brush) => self.state.brush = brush,
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
                let brush = match u32_at(record, 12)? {
                    0 => Brush::Solid(colour_at(record, 16)?),
                    1 => Brush::Null,
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
            // Brushes of a pattern; a font, a palette and a colour space, which only take a place.
            93 | 94 => self.keep(record, Object::Brush(Brush::Unknown))?,
            49 | 82 | 99 | 122 => self.keep(record, Object::Other)?,
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
            // EMR_SELECTCLIPPATH: a path that is one box standing straight is cut to.
            67 => {
                let path = std::mem::take(&mut self.path);
                self.building = false;
                let [(ring, _)] = &path[..] else { return None };
                let corners = match &ring[..] {
                    [a, b, c, d] => [a, b, c, d],
                    [a, b, c, d, again] if again == a => [a, b, c, d],
                    _ => return None,
                };
                let to = corners.iter().fold((f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY), |to, p| {
                    (to.0.min(p.0), to.1.min(p.1), to.2.max(p.0), to.3.max(p.1))
                });
                if !corners.iter().all(|p| (p.0 == to.0 || p.0 == to.2) && (p.1 == to.1 || p.1 == to.3)) {
                    return None;
                }
                self.cut(to, u32_at(record, 8)?)?;
            }
            // EMR_EXTSELECTCLIPRGN: no region, an empty one, or one of a single box, in the
            // device's units ([MS-EMF] 2.2.24, RegionData).
            75 => {
                let (size, mode) = (size_at(record, 8)?, u32_at(record, 12)?);
                if size == 0 {
                    if mode != 5 {
                        return None;
                    }
                    self.state.clip = None;
                } else {
                    let to = match u32_at(record, 24)? {
                        0 => (0.0, 0.0, 0.0, 0.0),
                        1 => boxed(self.frame.at(point(record, 48)?), self.frame.at(point(record, 56)?)),
                        _ => return None,
                    };
                    self.cut(to, mode)?;
                }
            }
            _ if IGNORED.contains(&kind) => {}
            _ => return None,
        }
        Some(())
    }
}

/// An enhanced metafile that draws lines, curves and filled shapes and nothing else → a BMP
/// file of it, for showing at `want` pixels when that is known.
pub(crate) fn emf_drawing(data: &[u8], want: Option<(u32, u32)>) -> Option<Vec<u8>> {
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
    };
    let mut page = Page { canvas, frame, state, saved: Vec::new(), objects: Vec::new(), path: Vec::new(), building: false, points: 0, drew: false };
    for _ in 0..MAX_RECORDS {
        // [MS-EMF] 2.3: a record's type and its size in bytes.
        let (kind, size) = (u32_at(data, at)?, size_at(data, at.checked_add(4)?)?);
        if size < 8 || !size.is_multiple_of(4) {
            return None;
        }
        let record = data.get(at..at.checked_add(size)?)?;
        // EMR_EOF: a drawing of nothing is no picture.
        if kind == 14 {
            return if page.drew { page.canvas.bmp() } else { None };
        }
        page.record(kind, record)?;
        at = at.checked_add(size)?;
    }
    None
}
