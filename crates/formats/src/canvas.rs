//! A small page of pixels that shapes are filled on, for pictures that are kept as drawings.
//! Edges are smoothed by counting how much of each pixel a shape covers.

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

/// A white page. `clip` is the part of it that can be drawn on: left, top, right, bottom.
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    px: Vec<u8>,
    pub clip: (f32, f32, f32, f32),
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
        Some(Self { w, h, px: vec![255; w.checked_mul(h)?.checked_mul(3)?], clip: (0.0, 0.0, w as f32, h as f32), work: 0 })
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
        if rgba.len() < w.checked_mul(h)?.checked_mul(4)? {
            return Some(());
        }
        let (first_x, last_x) = (l.max(self.clip.0).max(0.0).floor() as usize, (r.min(self.clip.2).ceil().max(0.0) as usize).min(self.w));
        let (first_y, last_y) = (t.max(self.clip.1).max(0.0).floor() as usize, (b.min(self.clip.3).ceil().max(0.0) as usize).min(self.h));
        // Picture pixels to one of the page, each way; up to four of them are looked at.
        let (step_x, step_y) = (w as f32 / (r - l), h as f32 / (b - t));
        let (nx, ny) = ((step_x.ceil() as usize).clamp(1, 4), (step_y.ceil() as usize).clamp(1, 4));
        for y in first_y..last_y {
            self.work = self.work.saturating_add((last_x.saturating_sub(first_x) * nx * ny) as u64 + 1);
            if self.work > MAX_WORK {
                return None;
            }
            for x in first_x..last_x {
                let (mut sum, mut cover) = ([0f32; 3], 0f32);
                for j in 0..ny {
                    for i in 0..nx {
                        let sx = (x as f32 + (i as f32 + 0.5) / nx as f32 - l) * step_x;
                        let sy = (y as f32 + (j as f32 + 0.5) / ny as f32 - t) * step_y;
                        if sx < 0.0 || sy < 0.0 || sx >= w as f32 || sy >= h as f32 {
                            continue;
                        }
                        let at = (sy as usize * w + sx as usize) * 4;
                        if let Some(&[red, green, blue, a]) = rgba.get(at..at + 4) {
                            let a = f32::from(a) / 255.0;
                            sum = [sum[0] + f32::from(red) * a, sum[1] + f32::from(green) * a, sum[2] + f32::from(blue) * a];
                            cover += a;
                        }
                    }
                }
                let part = cover / (nx * ny) as f32;
                let Some(pixel) = y.checked_mul(self.w).and_then(|row| self.px.get_mut((row + x) * 3..(row + x) * 3 + 3)) else { continue };
                if part > 0.0 {
                    for (was, now) in pixel.iter_mut().zip(sum) {
                        *was = (f32::from(*was) * (1.0 - part) + now / (nx * ny) as f32).round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
        Some(())
    }

    /// Fill what `rings` enclose, each ring closed back to its first point. `winding` fills
    /// all that is wound round; otherwise what an odd number of edges enclose
    /// ([MS-WMF] 2.1.1.25, PolyFillMode). `None` when the page's work is used up.
    pub fn fill(&mut self, rings: &[Vec<Point>], winding: bool, colour: Rgb) -> Option<()> {
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
            for (pixel, part) in line.chunks_exact_mut(3).zip(&cover) {
                let part = (part / SUB as f32).clamp(0.0, 1.0);
                if part > 0.0 {
                    for (was, now) in pixel.iter_mut().zip(colour) {
                        *was = (f32::from(*was) + (f32::from(now) - f32::from(*was)) * part).round().clamp(0.0, 255.0) as u8;
                    }
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
#[derive(Clone, Copy)]
pub(crate) enum Brush {
    Null,
    Solid(Rgb),
    /// A pattern or a hatching: a shape filled with it is not drawn.
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
