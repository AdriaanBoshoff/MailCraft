//! A small page of pixels that shapes are filled on, for pictures that are kept as drawings.
//! Edges are smoothed by counting how much of each pixel a shape covers.

use std::sync::Arc;

/// The longest side of a page, in pixels.
pub(crate) const MAX_SIDE: usize = 800;
/// The longest side of any page.
pub const LARGEST: usize = 4096;
/// Rows a pixel is cut into when its cover is counted.
const SUB: usize = 4;
/// Steps one page may take (an edge met by a row, a pixel covered); a drawing that asks for
/// more is not drawn.
const MAX_WORK: u64 = 200_000_000;

pub type Point = (f32, f32);
pub type Rgb = [u8; 3];
/// A box: left, top, right, bottom.
pub type Clip = (f32, f32, f32, f32);
/// What can be drawn on: a box, and how much of each pixel (`None` is all of each).
pub(crate) type Cut = (Option<Clip>, Option<Arc<Vec<u8>>>);

/// The way of mixing that copies a picture as it is ([MS-WMF] 2.1.1.31, SRCCOPY).
pub const PLAIN: u8 = 0xCC;

/// [MS-WMF] 2.1.1.31, TernaryRasterOperation: what a pixel becomes from the brush's (`p`), the
/// picture's (`s`) and the one that is there (`d`). `table` has a bit for each of the eight
/// things three bits can be between them, the brush's bit counting four and the picture's two.
pub fn mixed(table: u8, p: u8, s: u8, d: u8) -> u8 {
    (0..8u8)
        .filter(|i| table >> i & 1 != 0)
        .fold(0, |out, i| out | (if i & 4 != 0 { p } else { !p }) & (if i & 2 != 0 { s } else { !s }) & (if i & 1 != 0 { d } else { !d }))
}

/// Whether a way of mixing looks at the picture, and at the brush.
pub fn mix_needs(table: u8) -> (bool, bool) {
    ((table >> 2) & 0x33 != table & 0x33, table >> 4 != table & 0x0F)
}

/// A small picture that is laid side by side over all of the page: red, green, blue and cover
/// for each pixel, row after row from the top.
pub struct Tile {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

/// What a shape is filled with: one colour, or a pattern of which each pixel is so many of
/// the page's (a whole number of them, so that its lines stay even).
#[derive(Clone, Copy)]
pub enum Paint<'a> {
    Solid(Rgb),
    Tile(&'a Tile, f32),
}

impl Paint<'_> {
    /// The colour at one pixel of the page, and how much of the pixel it covers.
    fn at(&self, x: usize, y: usize) -> [u8; 4] {
        match *self {
            Paint::Solid([r, g, b]) => [r, g, b, 255],
            Paint::Tile(tile, step) => {
                if tile.w == 0 || tile.h == 0 {
                    return [0; 4];
                }
                let step = if step.is_finite() && step >= 1.0 { step.round() } else { 1.0 };
                let (tx, ty) = ((x as f32 / step) as usize % tile.w, (y as f32 / step) as usize % tile.h);
                let at = ty.saturating_mul(tile.w).saturating_add(tx).saturating_mul(4);
                match tile.rgba.get(at..at.saturating_add(4)) {
                    Some(&[r, g, b, a]) => [r, g, b, a],
                    _ => [0; 4],
                }
            }
        }
    }
}

/// A part of a picture and where on the page it goes.
pub struct Lay<'a> {
    /// Red, green, blue and cover for each pixel, row after row from the top.
    pub rgba: &'a [u8],
    pub size: (usize, usize),
    /// From a point of the page to one of the picture:
    /// `(x·m0 + y·m2 + m4, x·m1 + y·m3 + m5)`.
    pub back: [f32; 6],
    /// The part of the picture that is taken, in its own pixels.
    pub part: Clip,
    /// The box of the page that the part reaches.
    pub reach: Clip,
    /// How it is mixed with what is there, and with `paint` ([`mixed`]).
    pub mix: u8,
    pub paint: Paint<'a>,
}

/// Where the part `from` of a picture (left, top, wide, high, in its pixels; a width or a
/// height below nothing mirrors it) goes when it is laid over the box `to` of a drawing (the
/// same four, in the drawing's units), and `forward` takes the drawing's units to the page:
/// the way back from the page to the picture, the part, and the box of the page it reaches.
pub(crate) fn placed(forward: [f32; 6], to: Clip, from: Clip) -> Option<([f32; 6], Clip, Clip)> {
    let f = forward;
    let area = f[0] * f[3] - f[1] * f[2];
    if !area.is_finite() || area == 0.0 || to.2 == 0.0 || to.3 == 0.0 {
        return None;
    }
    let inverse = [f[3] / area, -f[1] / area, -f[2] / area, f[0] / area, (f[2] * f[5] - f[3] * f[4]) / area, (f[1] * f[4] - f[0] * f[5]) / area];
    let (kx, ky) = (from.2 / to.2, from.3 / to.3);
    let back = [
        inverse[0] * kx,
        inverse[1] * ky,
        inverse[2] * kx,
        inverse[3] * ky,
        inverse[4] * kx + from.0 - to.0 * kx,
        inverse[5] * ky + from.1 - to.1 * ky,
    ];
    let part = (from.0.min(from.0 + from.2), from.1.min(from.1 + from.3), from.0.max(from.0 + from.2), from.1.max(from.1 + from.3));
    let corners = [(to.0, to.1), (to.0 + to.2, to.1), (to.0 + to.2, to.1 + to.3), (to.0, to.1 + to.3)];
    let reach = corners.iter().fold((f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY), |reach, &(x, y)| {
        let (px, py) = (x * f[0] + y * f[2] + f[4], x * f[1] + y * f[3] + f[5]);
        (reach.0.min(px), reach.1.min(py), reach.2.max(px), reach.3.max(py))
    });
    back.iter().all(|v| v.is_finite()).then_some((back, part, reach))
}

/// [MS-WMF] 2.1.1.12, HatchStyle: lines across, lines down, lines that fall to the right,
/// lines that rise to the right, a grid, and a grid that leans. Between the lines is `back`,
/// or what was there.
pub(crate) fn hatch(kind: u32, colour: Rgb, back: Option<Rgb>) -> Option<Tile> {
    const SIDE: usize = 8;
    if kind > 5 {
        return None;
    }
    let on = |x: usize, y: usize| match kind {
        0 => y == 3,
        1 => x == 3,
        2 => x == y,
        3 => x + y == SIDE - 1,
        4 => x == 3 || y == 3,
        _ => x == y || x + y == SIDE - 1,
    };
    let mut rgba = Vec::with_capacity(SIDE * SIDE * 4);
    for at in 0..SIDE * SIDE {
        let [r, g, b] = if on(at % SIDE, at / SIDE) { colour } else { back.unwrap_or([0; 3]) };
        rgba.extend([r, g, b, if on(at % SIDE, at / SIDE) || back.is_some() { 255 } else { 0 }]);
    }
    Some(Tile { w: SIDE, h: SIDE, rgba })
}

/// A white page. `clip` is the part of it that can be drawn on: left, top, right, bottom.
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    px: Vec<u8>,
    pub clip: (f32, f32, f32, f32),
    /// How much of each pixel can be drawn on, inside `clip`; `None` is all of each.
    pub mask: Option<Arc<Vec<u8>>>,
    work: u64,
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Option<Self> {
        if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
            return None;
        }
        Self::sized(w, h, MAX_SIDE)
    }

    /// A white page whose sides may be as long as `most`, and no longer than [`LARGEST`].
    pub fn sized(w: usize, h: usize, most: usize) -> Option<Self> {
        if w == 0 || h == 0 || w > most.min(LARGEST) || h > most.min(LARGEST) {
            return None;
        }
        Some(Self { w, h, px: vec![255; w.checked_mul(h)?.checked_mul(3)?], clip: (0.0, 0.0, w as f32, h as f32), mask: None, work: 0 })
    }

    /// The page: red, green and blue for each pixel, row after row from the top.
    pub fn into_rgb(self) -> Vec<u8> {
        self.px
    }

    /// Lay a picture of `w` by `h` pixels (red, green, blue and cover for each, row after row
    /// from the top) over the box `to` (left, top, right, bottom), made as large as the box.
    /// `None` when the page's work is used up.
    pub fn image(&mut self, rgba: &[u8], (w, h): (usize, usize), to: (f32, f32, f32, f32)) -> Option<()> {
        let (l, t, r, b) = to;
        if w == 0 || h == 0 || ![l, t, r, b].iter().all(|v| v.is_finite()) || r - l < 0.01 || b - t < 0.01 {
            return Some(());
        }
        let (step_x, step_y) = (w as f32 / (r - l), h as f32 / (b - t));
        let back = [step_x, 0.0, 0.0, step_y, -l * step_x, -t * step_y];
        self.lay(&Lay { rgba, size: (w, h), back, part: (0.0, 0.0, w as f32, h as f32), reach: to, mix: PLAIN, paint: Paint::Solid([0; 3]) })
    }

    /// Lay a part of a picture onto the page, mixed with what is there as `lay` says. `None`
    /// when the page's work is used up.
    pub fn lay(&mut self, lay: &Lay) -> Option<()> {
        let ((w, h), (l, t, r, b), m) = (lay.size, lay.reach, lay.back);
        if w == 0 || h == 0 || ![l, t, r, b].iter().chain(&m).all(|v| v.is_finite()) || r - l < 0.01 || b - t < 0.01 {
            return Some(());
        }
        if lay.rgba.len() < w.checked_mul(h)?.checked_mul(4)? {
            return Some(());
        }
        let (first_x, last_x) = (l.max(self.clip.0).max(0.0).floor() as usize, (r.min(self.clip.2).ceil().max(0.0) as usize).min(self.w));
        let (first_y, last_y) = (t.max(self.clip.1).max(0.0).floor() as usize, (b.min(self.clip.3).ceil().max(0.0) as usize).min(self.h));
        // Picture pixels to one of the page, each way; up to four of them are looked at.
        let (step_x, step_y) = ((m[0] * m[0] + m[1] * m[1]).sqrt(), (m[2] * m[2] + m[3] * m[3]).sqrt());
        let (nx, ny) = ((step_x.ceil() as usize).clamp(1, 4), (step_y.ceil() as usize).clamp(1, 4));
        let (part_l, part_t, part_r, part_b) = lay.part;
        let mask = self.mask.as_deref();
        for y in first_y..last_y {
            self.work = self.work.saturating_add((last_x.saturating_sub(first_x) * nx * ny) as u64 + 1);
            if self.work > MAX_WORK {
                return None;
            }
            for x in first_x..last_x {
                let (mut sum, mut cover) = ([0f32; 3], 0f32);
                for j in 0..ny {
                    for i in 0..nx {
                        let (px, py) = (x as f32 + (i as f32 + 0.5) / nx as f32, y as f32 + (j as f32 + 0.5) / ny as f32);
                        let (sx, sy) = (px * m[0] + py * m[2] + m[4], px * m[1] + py * m[3] + m[5]);
                        // Only inside the part that is taken, and inside the picture.
                        let inside = sx >= part_l.max(0.0) && sy >= part_t.max(0.0) && sx < part_r.min(w as f32) && sy < part_b.min(h as f32);
                        if !inside {
                            continue;
                        }
                        let at = (sy as usize * w + sx as usize) * 4;
                        if let Some(&[red, green, blue, a]) = lay.rgba.get(at..at + 4) {
                            let a = f32::from(a) / 255.0;
                            sum = [sum[0] + f32::from(red) * a, sum[1] + f32::from(green) * a, sum[2] + f32::from(blue) * a];
                            cover += a;
                        }
                    }
                }
                if cover <= 0.0 {
                    continue;
                }
                let mut part = cover / (nx * ny) as f32;
                if let Some(mask) = mask {
                    part *= f32::from(y.checked_mul(self.w).and_then(|row| mask.get(row + x)).copied().unwrap_or(0)) / 255.0;
                }
                let Some(pixel) = y.checked_mul(self.w).and_then(|row| self.px.get_mut((row + x) * 3..(row + x) * 3 + 3)) else { continue };
                let with = lay.paint.at(x, y);
                for ((was, sum), with) in pixel.iter_mut().zip(sum).zip(with) {
                    let source = sum / cover;
                    let now = if lay.mix == PLAIN { source } else { f32::from(mixed(lay.mix, with, source.round().clamp(0.0, 255.0) as u8, *was)) };
                    *was = (f32::from(*was) + (now - f32::from(*was)) * part).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        Some(())
    }

    fn charge(&mut self, work: u64) -> Option<()> {
        self.work = self.work.saturating_add(work);
        (self.work <= MAX_WORK).then_some(())
    }

    /// How much of each pixel `rings` cover (255 is all of it), row after row from the top.
    pub fn cover(&mut self, rings: &[Vec<Point>], winding: bool) -> Option<Vec<u8>> {
        self.charge((self.w * self.h) as u64)?;
        let mut page = Canvas::sized(self.w, self.h, LARGEST)?;
        page.work = self.work;
        let filled = page.fill(rings, winding, [0; 3]);
        self.work = page.work;
        filled?;
        Some(page.px.chunks_exact(3).map(|pixel| 255 - pixel.first().copied().unwrap_or(255)).collect())
    }

    /// What can be drawn on after `rings` are joined to what could be drawn on before (`was`),
    /// the way `mode` says ([MS-EMF] 2.1.29, RegionMode: both, either, one of the two, the
    /// old without the new, or the new alone).
    pub(crate) fn cut(&mut self, was: Cut, rings: &[Vec<Point>], winding: bool, mode: u32) -> Option<Cut> {
        let new = self.cover(rings, winding)?;
        self.charge((self.w * self.h) as u64)?;
        let (clip, old) = was;
        let old_at = |at: usize| old.as_ref().map_or(255, |mask| mask.get(at).copied().unwrap_or(0));
        Some(match mode {
            1 => (clip, Some(Arc::new(new.iter().enumerate().map(|(at, new)| old_at(at).min(*new)).collect()))),
            5 => (None, Some(Arc::new(new))),
            2..=4 => {
                // What could be drawn on before, with its box made part of it.
                let (l, t, r, b) = clip.unwrap_or((0.0, 0.0, self.w as f32, self.h as f32));
                let w = self.w.max(1);
                let before = |at: usize| {
                    let (x, y) = ((at % w) as f32 + 0.5, (at / w) as f32 + 0.5);
                    if x >= l && x < r && y >= t && y < b { old_at(at) } else { 0 }
                };
                let join = |(at, new): (usize, &u8)| match mode {
                    2 => before(at).max(*new),
                    3 => before(at).abs_diff(*new),
                    _ => before(at).min(255 - *new),
                };
                (None, Some(Arc::new(new.iter().enumerate().map(join).collect())))
            }
            _ => return None,
        })
    }

    /// Fill `rings` with a brush, mixed with what is there when `mix` says how. `back` is the
    /// colour between the lines of a hatching, when there is one; `step` is how many of the
    /// page's pixels one of a pattern's is. `None` for a brush nobody here knows.
    pub(crate) fn brushed(&mut self, rings: &[Vec<Point>], winding: bool, brush: &Brush, with: (Option<Rgb>, f32), mix: Option<u8>) -> Option<()> {
        match brush {
            Brush::Null => Some(()),
            Brush::Solid(colour) => self.fill_with(rings, winding, Paint::Solid(*colour), mix),
            Brush::Hatch(kind, colour) => {
                let tile = hatch(*kind, *colour, with.0)?;
                self.fill_with(rings, winding, Paint::Tile(&tile, with.1), mix)
            }
            Brush::Tile(tile) => self.fill_with(rings, winding, Paint::Tile(tile, with.1), mix),
            Brush::Unknown => None,
        }
    }

    /// Fill the rim of what `rings` cover, `wide` pixels across and down, with a brush.
    pub(crate) fn framed(&mut self, rings: &[Vec<Point>], wide: (f32, f32), brush: &Brush, with: (Option<Rgb>, f32)) -> Option<()> {
        let all = self.cover(rings, true)?;
        self.charge((self.w * self.h) as u64)?;
        let side = |v: f32| if v.is_finite() { (v.abs().round() as usize).clamp(1, MAX_SIDE) } else { 1 };
        let (dx, dy, w) = (side(wide.0), side(wide.1), self.w);
        let at = |x: Option<usize>, y: Option<usize>| match (x, y) {
            (Some(x), Some(y)) if x < w => all.get(y * w + x).copied().unwrap_or(0),
            _ => 0,
        };
        let held = self.mask.take();
        let rim: Vec<u8> = (0..all.len())
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let here = at(Some(x), Some(y));
                let inner = here
                    .min(at(x.checked_sub(dx), Some(y)))
                    .min(at(x.checked_add(dx), Some(y)))
                    .min(at(Some(x), y.checked_sub(dy)))
                    .min(at(Some(x), y.checked_add(dy)));
                (here - inner).min(held.as_ref().map_or(255, |mask| mask.get(i).copied().unwrap_or(0)))
            })
            .collect();
        self.mask = Some(Arc::new(rim));
        let (r, b) = (self.w as f32, self.h as f32);
        let done = self.brushed(&[vec![(0.0, 0.0), (r, 0.0), (r, b), (0.0, b)]], true, brush, with, None);
        self.mask = held;
        done
    }

    /// Fill what `rings` enclose, each ring closed back to its first point. `winding` fills
    /// all that is wound round; otherwise what an odd number of edges enclose
    /// ([MS-WMF] 2.1.1.25, PolyFillMode). `None` when the page's work is used up.
    pub fn fill(&mut self, rings: &[Vec<Point>], winding: bool, colour: Rgb) -> Option<()> {
        self.fill_with(rings, winding, Paint::Solid(colour), None)
    }

    /// As [`Canvas::fill`], with a colour or a pattern, mixed with what is there the way `mix`
    /// says when it says one ([`mixed`]; there is no picture, so its bits are none).
    pub fn fill_with(&mut self, rings: &[Vec<Point>], winding: bool, paint: Paint, mix: Option<u8>) -> Option<()> {
        // Edges that cross rows, each running downwards, with the way it ran.
        let mut edges: Vec<(f32, f32, f32, f32, i32)> = Vec::new();
        for ring in rings {
            let mut last = ring.last().copied();
            for &(x, y) in ring {
                if let Some((lx, ly)) = last
                    && x.is_finite()
                    && y.is_finite()
                    && lx.is_finite()
                    && ly.is_finite()
                    && ly != y
                {
                    edges.push(if ly < y { (lx, ly, x, y, 1) } else { (x, y, lx, ly, -1) });
                }
                last = Some((x, y));
            }
        }
        let top = edges.iter().map(|e| e.1).fold(f32::INFINITY, f32::min).max(self.clip.1).max(0.0);
        let bottom = edges.iter().map(|e| e.3).fold(f32::NEG_INFINITY, f32::max).min(self.clip.3).min(self.h as f32);
        // No further left or right than the shape reaches.
        let reach = edges.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(l, r), e| (l.min(e.0).min(e.2), r.max(e.0).max(e.2)));
        let (left, right) = (self.clip.0.max(0.0).max(reach.0), self.clip.2.min(self.w as f32).min(reach.1));
        if edges.is_empty() || top >= bottom || left >= right {
            return Some(());
        }
        let (first, last) = (left.floor() as usize, (right.ceil() as usize).min(self.w));
        let mut cover = vec![0f32; last.saturating_sub(first)];
        let mut crossings: Vec<(f32, i32)> = Vec::new();
        let mask = self.mask.as_deref();
        for row in (top.floor() as usize)..(bottom.ceil() as usize).min(self.h) {
            cover.fill(0.0);
            for sub in 0..SUB {
                let y = row as f32 + (sub as f32 + 0.5) / SUB as f32;
                if y < top || y >= bottom {
                    continue;
                }
                self.work = self.work.saturating_add(edges.len() as u64 + cover.len() as u64 / SUB as u64 + 1);
                if self.work > MAX_WORK {
                    return None;
                }
                crossings.clear();
                crossings.extend(edges.iter().filter(|e| e.1 <= y && y < e.3).map(|e| (e.0 + (y - e.1) / (e.3 - e.1) * (e.2 - e.0), e.4)));
                crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut wound = 0;
                let mut from: Option<f32> = None;
                for &(x, way) in &crossings {
                    let was_in = if winding { wound != 0 } else { wound % 2 != 0 };
                    wound += if winding { way } else { 1 };
                    let is_in = if winding { wound != 0 } else { wound % 2 != 0 };
                    match (was_in, is_in) {
                        (false, true) => from = Some(x),
                        (true, false) => {
                            if let Some(start) = from.take() {
                                span(&mut cover, start.max(left) - first as f32, x.min(right) - first as f32);
                            }
                        }
                        _ => {}
                    }
                }
            }
            let Some(line) = row.checked_mul(self.w).and_then(|at| self.px.get_mut((at + first) * 3..(at + last) * 3)) else { continue };
            for (x, (pixel, part)) in line.chunks_exact_mut(3).zip(&cover).enumerate() {
                let mut part = (part / SUB as f32).clamp(0.0, 1.0);
                if part <= 0.0 {
                    continue;
                }
                if let Some(mask) = mask {
                    part *= f32::from(row.checked_mul(self.w).and_then(|at| mask.get(at + first + x)).copied().unwrap_or(0)) / 255.0;
                }
                let [r, g, b, a] = paint.at(first + x, row);
                part *= f32::from(a) / 255.0;
                for (was, now) in pixel.iter_mut().zip([r, g, b]) {
                    let now = mix.map_or(now, |table| mixed(table, now, 0, *was));
                    *was = (f32::from(*was) + (f32::from(now) - f32::from(*was)) * part).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        Some(())
    }

    /// Draw a line through `points`, `width` pixels wide, back to the first when `closed`.
    pub fn stroke(&mut self, points: &[Point], closed: bool, width: f32, colour: Rgb) -> Option<()> {
        let half = width.clamp(1.0, MAX_SIDE as f32) / 2.0;
        let count = points.len();
        let segments = if closed { count } else { count.saturating_sub(1) };
        for i in 0..segments {
            let (Some(&(ax, ay)), Some(&(bx, by))) = (points.get(i), points.get((i + 1) % count.max(1))) else { continue };
            let (dx, dy) = (bx - ax, by - ay);
            let long = (dx * dx + dy * dy).sqrt();
            if !long.is_finite() {
                continue;
            }
            // A point with no length is a dot as wide as the line.
            let (ux, uy) = if long > 0.0 { (dx / long, dy / long) } else { (1.0, 0.0) };
            // Thin lines reach half their width past each end, so that corners close.
            let cap = if half < 1.5 || long == 0.0 { half } else { 0.0 };
            let (ax, ay, bx, by) = (ax - ux * cap, ay - uy * cap, bx + ux * cap, by + uy * cap);
            let (nx, ny) = (-uy * half, ux * half);
            self.fill(&[vec![(ax + nx, ay + ny), (bx + nx, by + ny), (bx - nx, by - ny), (ax - nx, ay - ny)]], true, colour)?;
        }
        // Wide lines are rounded where they turn and where they end.
        if half >= 1.5 {
            for &(x, y) in points {
                self.fill(&[arc((x, y), (half, half), 0.0, std::f32::consts::TAU, 12)], true, colour)?;
            }
        }
        Some(())
    }

    /// The page as a BMP file of 24 bits to the pixel ([MS-WMF] 2.2.2.3, BitmapInfoHeader).
    pub fn bmp(&self) -> Option<Vec<u8>> {
        let stride = (self.w.checked_mul(3)?.checked_add(3)?) & !3;
        let size = stride.checked_mul(self.h)?;
        let mut file = Vec::with_capacity(54usize.checked_add(size)?);
        file.extend_from_slice(b"BM");
        file.extend_from_slice(&u32::try_from(54 + size).ok()?.to_le_bytes());
        file.extend_from_slice(&[0; 4]);
        file.extend_from_slice(&54u32.to_le_bytes());
        file.extend_from_slice(&40u32.to_le_bytes());
        file.extend_from_slice(&i32::try_from(self.w).ok()?.to_le_bytes());
        file.extend_from_slice(&i32::try_from(self.h).ok()?.to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&24u16.to_le_bytes());
        file.extend_from_slice(&[0; 4]);
        file.extend_from_slice(&u32::try_from(size).ok()?.to_le_bytes());
        file.extend_from_slice(&[0; 16]);
        // Rows from the bottom up, each pixel blue first.
        for row in self.px.chunks_exact(self.w * 3).rev() {
            for pixel in row.chunks_exact(3) {
                file.extend(pixel.iter().rev());
            }
            file.resize(file.len() + stride - self.w * 3, 0);
        }
        Some(file)
    }
}

/// Add to `cover` what one cut row covers between `from` and `to`.
fn span(cover: &mut [f32], from: f32, to: f32) {
    if from.is_nan() || to.is_nan() || from >= to {
        return;
    }
    let (first, last) = (from.floor().max(0.0) as usize, to.ceil().max(0.0) as usize);
    for (at, part) in cover.iter_mut().enumerate().take(last).skip(first) {
        let (l, r) = (at as f32, at as f32 + 1.0);
        *part += (to.min(r) - from.max(l)).clamp(0.0, 1.0);
    }
}

/// Points round an ellipse from angle `from` to `to` (radians, anticlockwise as seen, the
/// page's rows running downwards), `steps` to the full turn.
pub(crate) fn arc(centre: Point, radii: Point, from: f32, to: f32, steps: usize) -> Vec<Point> {
    let turn = (to - from).clamp(0.0, std::f32::consts::TAU);
    let n = ((steps as f32 * turn / std::f32::consts::TAU).ceil() as usize).clamp(1, 256);
    (0..=n)
        .map(|i| {
            let a = from + turn * i as f32 / n as f32;
            (centre.0 + radii.0 * a.cos(), centre.1 - radii.1 * a.sin())
        })
        .collect()
}

/// What a shape is filled with.
#[derive(Clone)]
pub(crate) enum Brush {
    Null,
    Solid(Rgb),
    /// Lines of a colour, the way [`hatch`] reads.
    Hatch(u32, Rgb),
    /// A small picture, side by side.
    Tile(Arc<Tile>),
    /// A pattern nobody here knows: a shape filled with it is not drawn.
    Unknown,
}

/// The longer side of a drawing that says nothing of how large it is.
const UNSIZED: f32 = 600.0;

/// How many pixels a drawing's page is: twice what it is shown at (`want`), so that it stays
/// sharp on a dense screen and on paper; else the size it says it has (`natural`); else its
/// `shape` at a size that can be looked at.
pub(crate) fn page_size(want: Option<(u32, u32)>, natural: Option<Point>, shape: Point) -> Option<(usize, usize)> {
    let (w, h) = match (want, natural) {
        (Some((w, h)), _) if w > 0 && h > 0 => (w.min(4096) as f32 * 2.0, h.min(4096) as f32 * 2.0),
        (_, Some(natural)) => natural,
        _ => {
            let (w, h) = (shape.0.abs(), shape.1.abs());
            (w * UNSIZED / w.max(h), h * UNSIZED / w.max(h))
        }
    };
    if w.is_nan() || h.is_nan() || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let fit = (MAX_SIDE as f32 / w.max(h)).min(1.0);
    let side = |v: f32| ((v * fit).round() as usize).clamp(1, MAX_SIDE);
    Some((side(w), side(h)))
}

/// Steps round an ellipse that is this large on the page.
pub(crate) fn steps(radii: Point) -> usize {
    (radii.0.abs().max(radii.1.abs()) as usize).clamp(16, 200)
}

/// A box (left, top, right, bottom) whose corners are cut from an ellipse of these radii.
pub(crate) fn rounded((l, t, r, b): (f32, f32, f32, f32), radii: Point, steps: usize) -> Vec<Point> {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    let mut ring = arc((r - radii.0, t + radii.1), radii, 0.0, FRAC_PI_2, steps);
    ring.extend(arc((l + radii.0, t + radii.1), radii, FRAC_PI_2, PI, steps));
    ring.extend(arc((l + radii.0, b - radii.1), radii, PI, PI + FRAC_PI_2, steps));
    ring.extend(arc((r - radii.0, b - radii.1), radii, PI + FRAC_PI_2, TAU, steps));
    ring
}
