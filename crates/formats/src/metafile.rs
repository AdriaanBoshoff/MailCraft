//! Pictures in the older Windows forms an RTF body and an attachment's rendering are kept in: a
//! bitmap without a file's header ([MS-WMF] 2.2.2.9, DeviceIndependentBitmap), and a metafile
//! whose only drawing is one such bitmap ([MS-WMF], [MS-EMF]). Each becomes the bytes of a BMP
//! file, undecoded. A metafile that is a drawing of lines, filled shapes, text and bitmaps is
//! drawn (`wmf`, `emf`), its bitmaps read here into pixels and copied onto the page the way
//! the drawing says; one that draws anything else is not read: half a picture is worse than
//! its place.

use openmailclient_fonts::Lettering;

use crate::canvas::{Brush, Canvas, Clip, Lay, Paint, Rgb, hatch, mix_needs, placed};

/// Records one metafile is searched through.
const MAX_RECORDS: usize = 4096;
/// The raster operation that copies the bitmap as it is ([MS-WMF] 2.1.1.31, SRCCOPY).
pub(crate) const COPY: u32 = 0x00CC_0020;

pub(crate) fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes(<[u8; 2]>::try_from(bytes).ok()?))
}

pub(crate) fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(<[u8; 4]>::try_from(bytes).ok()?))
}

pub(crate) fn size_at(data: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32_at(data, at)?).ok()
}

/// A bitmap without a file's header → a BMP file: the header such a file starts with, which says
/// where the pixels begin, and then the bitmap as it is.
pub fn dib_file(dib: &[u8]) -> Option<Vec<u8>> {
    let header = size_at(dib, 0)?;
    // The colours a bitmap of few bits looks its pixels up in, and what one of them takes.
    let (bits, used, entry, masks) = match header {
        // [MS-WMF] 2.2.2.2, BitmapCoreHeader.
        12 => (u16_at(dib, 10)?, 0, 3, 0),
        // [MS-WMF] 2.2.2.3 to 2.2.2.5, BitmapInfoHeader and the two longer ones.
        40..=124 => {
            let compression = u32_at(dib, 16)?;
            // Three masks follow the shortest header when the pixels are bit fields (BI_BITFIELDS).
            let masks = if header == 40 && compression == 3 { 12 } else { 0 };
            (u16_at(dib, 14)?, size_at(dib, 32)?, 4, masks)
        }
        _ => return None,
    };
    let colors = match bits {
        1 | 4 | 8 => {
            let all = 1usize << bits;
            if used == 0 { all } else { used.min(all) }
        }
        16 | 24 | 32 => used.min(256),
        _ => return None,
    };
    let pixels = header.checked_add(masks)?.checked_add(colors.checked_mul(entry)?)?;
    if pixels >= dib.len() {
        return None;
    }
    let (offset, total) = (pixels.checked_add(14)?, dib.len().checked_add(14)?);
    let mut file = Vec::with_capacity(total);
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&u32::try_from(total).ok()?.to_le_bytes());
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(&u32::try_from(offset).ok()?.to_le_bytes());
    file.extend_from_slice(dib);
    Some(file)
}

/// Pixels a bitmap drawn into a drawing may have along a side, and the bitmaps of one drawing
/// between them.
const MAX_DIB_SIDE: usize = 4096;
pub(crate) const MAX_DIB_PIXELS: usize = 8_000_000;

/// A bitmap's pixels: how many across and down, and red, green, blue and cover for each, row
/// after row from the top.
pub(crate) type Pixels = ((usize, usize), Vec<u8>);

/// The part of a pixel's number that `mask` keeps, as a byte.
fn masked(value: u32, mask: u32) -> u8 {
    let bits = mask.count_ones();
    if bits == 0 {
        return 0;
    }
    let part = (value & mask) >> mask.trailing_zeros();
    // The most the part can be; a mask with holes in it keeps less, and is held to that.
    let most = if bits >= 32 { u32::MAX } else { (1u32 << bits) - 1 };
    let scaled = u64::from(part.min(most)) * 255 / u64::from(most);
    u8::try_from(scaled).unwrap_or(u8::MAX)
}

/// A bitmap without a file's header → its pixels ([MS-WMF] 2.2.2.9, DeviceIndependentBitmap).
/// Pixels that are numbers in a table of colours (as they are, or packed in runs: BI_RLE8,
/// BI_RLE4), bit fields, or blue, green and red bytes, kept from the bottom row up or from the
/// top down; none that are a picture file of another kind (BI_JPEG, BI_PNG). `None` for those,
/// for a bitmap that is cut short, and for one of more than `most` pixels.
pub(crate) fn dib_pixels(dib: &[u8], most: usize) -> Option<Pixels> {
    let header = size_at(dib, 0)?;
    let long = |at: usize| u32_at(dib, at).map(|n| n as i32);
    // How wide, how high (below nothing: the top row first), bits to the pixel, how they are
    // packed, colours in the table, the bytes of one of them, and masks after the header.
    let (w, h, bits, packed, used, entry, after) = match header {
        // [MS-WMF] 2.2.2.2, BitmapCoreHeader.
        12 => (i32::from(u16_at(dib, 4)?), i32::from(u16_at(dib, 6)?), u16_at(dib, 10)?, 0, 0, 3, 0),
        // [MS-WMF] 2.2.2.3 to 2.2.2.5, BitmapInfoHeader and the two longer ones.
        40..=124 => {
            let packed = u32_at(dib, 16)?;
            let after = if header == 40 && packed == 3 { 12 } else { 0 };
            (long(4)?, long(8)?, u16_at(dib, 14)?, packed, size_at(dib, 32)?, 4, after)
        }
        _ => return None,
    };
    let top_first = h < 0;
    let (w, h) = (usize::try_from(w).ok()?, usize::try_from(h.checked_abs()?).ok()?);
    if w == 0 || h == 0 || w > MAX_DIB_SIDE || h > MAX_DIB_SIDE || w.checked_mul(h)? > most {
        return None;
    }
    // [MS-WMF] 2.1.1.7, Compression: BI_RGB, or BI_BITFIELDS for 16 and 32 bits to the pixel.
    let masks = match (packed, bits) {
        (0, 1 | 4 | 8 | 24) | (1, 8) | (2, 4) => None,
        (0, 16) => Some([0x7C00, 0x03E0, 0x001F]),
        (0, 32) => Some([0x00FF_0000, 0x0000_FF00, 0x0000_00FF]),
        // The masks stand after the shortest header, and inside the longer ones.
        (3, 16 | 32) => Some([u32_at(dib, 40)?, u32_at(dib, 44)?, u32_at(dib, 48)?]),
        _ => return None,
    };
    let colors = match bits {
        1 | 4 | 8 => {
            let all = 1usize << bits;
            if used == 0 { all } else { used.min(all) }
        }
        _ => used.min(256),
    };
    let table = header.checked_add(after)?;
    let start = table.checked_add(colors.checked_mul(entry)?)?;
    // A row is a whole number of four bytes ([MS-WMF] 2.2.2.9).
    let bits = usize::from(bits);
    let colour = |n: usize| -> [u8; 3] {
        let at = table.saturating_add(n.saturating_mul(entry));
        match dib.get(at..at.saturating_add(3)) {
            Some(&[blue, green, red]) if n < colors => [red, green, blue],
            _ => [0; 3],
        }
    };
    // Packed in runs: always from the bottom row up.
    if packed != 0 && bits <= 8 {
        if top_first {
            return None;
        }
        let numbers = unpacked(dib.get(start..)?, (w, h), bits == 4)?;
        let mut rgba = Vec::with_capacity(w.checked_mul(h)?.checked_mul(4)?);
        for y in (0..h).rev() {
            for &n in numbers.get(y * w..(y + 1) * w)? {
                let [red, green, blue] = colour(usize::from(n));
                rgba.extend_from_slice(&[red, green, blue, 255]);
            }
        }
        return Some(((w, h), rgba));
    }
    let row = w.checked_mul(bits)?.checked_add(31)? / 32 * 4;
    let data = dib.get(start..start.checked_add(row.checked_mul(h)?)?)?;
    let mut rgba = Vec::with_capacity(w.checked_mul(h)?.checked_mul(4)?);
    for y in 0..h {
        let line = data.get((if top_first { y } else { h - 1 - y }) * row..)?;
        for x in 0..w {
            let [red, green, blue] = match bits {
                1 | 4 => {
                    let byte = usize::from(*line.get(x * bits / 8)?);
                    // The first pixel is in the highest bits of its byte.
                    let shift = 8 - bits - (x * bits) % 8;
                    colour((byte >> shift) & ((1 << bits) - 1))
                }
                8 => colour(usize::from(*line.get(x)?)),
                24 => match line.get(x * 3..x * 3 + 3)? {
                    &[blue, green, red] => [red, green, blue],
                    _ => return None,
                },
                _ => {
                    let value = if bits == 16 { u32::from(u16_at(line, x * 2)?) } else { u32_at(line, x * 4)? };
                    masks?.map(|mask| masked(value, mask))
                }
            };
            rgba.extend_from_slice(&[red, green, blue, 255]);
        }
    }
    Some(((w, h), rgba))
}

/// Pixels packed in runs → the number of each, row after row from the bottom ([MS-WMF] 2.1.1.7,
/// Compression, BI_RLE8 and BI_RLE4, whose numbers are `halves` of a byte). A pair of bytes is
/// how many pixels and their number (for halves, the two numbers by turns); or nothing and then
/// the end of the row (0), the end of the bitmap (1), a step right and up (2, and two bytes),
/// or how many numbers follow as they are, made up to a whole word. Pixels that no run reaches
/// have the first colour; runs that go past the side of the bitmap are cut off there.
fn unpacked(runs: &[u8], (w, h): (usize, usize), halves: bool) -> Option<Vec<u8>> {
    let mut out = vec![0u8; w.checked_mul(h)?];
    let (mut x, mut y, mut at) = (0usize, 0usize, 0usize);
    let half = |byte: u8, i: usize| {
        if !halves {
            byte
        } else if i.is_multiple_of(2) {
            byte >> 4
        } else {
            byte & 15
        }
    };
    while y < h
        && let (Some(&count), Some(&value)) = (runs.get(at), runs.get(at.checked_add(1)?))
    {
        at = at.checked_add(2)?;
        match (count, value) {
            (0, 0) => (x, y) = (0, y + 1),
            (0, 1) => break,
            (0, 2) => {
                let (right, up) = (*runs.get(at)?, *runs.get(at.checked_add(1)?)?);
                at = at.checked_add(2)?;
                (x, y) = (x.saturating_add(usize::from(right)), y.saturating_add(usize::from(up)));
            }
            (0, count) => {
                let count = usize::from(count);
                let bytes = if halves { count.div_ceil(2) } else { count };
                let run = runs.get(at..at.checked_add(bytes)?)?;
                at = at.checked_add(bytes + bytes % 2)?;
                for i in 0..count.min(w.saturating_sub(x)) {
                    let byte = run.get(if halves { i / 2 } else { i }).copied()?;
                    *out.get_mut(y.checked_mul(w)?.checked_add(x + i)?)? = half(byte, i);
                }
                x = x.saturating_add(count);
            }
            (count, value) => {
                let count = usize::from(count);
                for i in 0..count.min(w.saturating_sub(x)) {
                    *out.get_mut(y.checked_mul(w)?.checked_add(x + i)?)? = half(value, i);
                }
                x = x.saturating_add(count);
            }
        }
    }
    Some(out)
}

/// Whether a bitmap keeps its top row first (a height below nothing).
fn from_top(dib: &[u8]) -> bool {
    size_at(dib, 0).is_some_and(|header| header >= 40) && u32_at(dib, 8).is_some_and(|h| h.cast_signed() < 0)
}

/// A copy onto a box of a drawing: of a part of a bitmap, of the brush, or of what is there,
/// mixed between them ([MS-WMF] 2.3.1 bitmap records, [MS-EMF] 2.3.1).
pub(crate) struct Blit<'a> {
    /// [MS-WMF] 2.1.1.31, TernaryRasterOperation: its third byte says how the three are mixed.
    pub how: u32,
    /// The bitmap, when the record has one.
    pub dib: Option<&'a [u8]>,
    /// The part taken, in the bitmap's pixels: left, top, wide, high. Below nothing mirrors.
    pub from: Clip,
    /// The box it goes to, in the drawing's units: left, top, wide, high.
    pub to: Clip,
    /// The part's rows are counted from the bottom when the bitmap keeps its bottom row first.
    pub upwards: bool,
    /// From the drawing's units to the page: `(x·m0 + y·m2 + m4, x·m1 + y·m3 + m5)`.
    pub forward: [f32; 6],
    pub brush: &'a Brush,
    /// The colour between a hatching's lines, and the page's pixels to one of a pattern's.
    pub with: (Option<Rgb>, f32),
}

/// Do a [`Blit`]; `pixels` counts the bitmaps' pixels of one drawing. Whether anything was
/// drawn; `None` when it cannot be done as it is asked.
pub(crate) fn blit(canvas: &mut Canvas, blit: &Blit, pixels: &mut usize) -> Option<bool> {
    let mix = blit.how.to_le_bytes()[2];
    let (pictured, patterned) = mix_needs(mix);
    let (f, (l, t, wide, high)) = (blit.forward, blit.to);
    if wide == 0.0 || high == 0.0 {
        return Some(false);
    }
    if !pictured {
        let at = |(x, y): (f32, f32)| (x * f[0] + y * f[2] + f[4], x * f[1] + y * f[3] + f[5]);
        let ring = vec![at((l, t)), at((l + wide, t)), at((l + wide, t + high)), at((l, t + high))];
        if patterned {
            canvas.brushed(&[ring], true, blit.brush, blit.with, Some(mix))?;
        } else {
            canvas.fill_with(&[ring], true, Paint::Solid([0; 3]), Some(mix))?;
        }
        return Some(true);
    }
    let dib = blit.dib?;
    let ((w, h), rgba) = dib_pixels(dib, MAX_DIB_PIXELS.checked_sub(*pixels)?)?;
    *pixels = pixels.checked_add(w.checked_mul(h)?)?;
    let mut from = blit.from;
    if from.2 == 0.0 || from.3 == 0.0 {
        return Some(false);
    }
    if blit.upwards && !from_top(dib) {
        from.1 = h as f32 - from.1 - from.3;
    }
    let (back, part, reach) = placed(f, blit.to, from)?;
    let hatched;
    let paint = match blit.brush {
        _ if !patterned => Paint::Solid([0; 3]),
        Brush::Solid(colour) => Paint::Solid(*colour),
        Brush::Tile(tile) => Paint::Tile(tile, blit.with.1),
        Brush::Hatch(kind, colour) => {
            hatched = hatch(*kind, *colour, Some(blit.with.0.unwrap_or([255; 3])))?;
            Paint::Tile(&hatched, blit.with.1)
        }
        Brush::Null | Brush::Unknown => return None,
    };
    canvas.lay(&Lay { rgba: &rgba, size: (w, h), back, part, reach, mix, paint })?;
    Some(true)
}

/// The bitmap of an EMF record that carries one: its header and its pixels are two parts of the
/// record, each at an offset from its start that is written at `at` and at `at + 8`
/// ([MS-EMF] 2.3.1.7).
pub(crate) fn emf_parts(record: &[u8], at: usize) -> Option<Vec<u8>> {
    let part = |at: usize| {
        let (start, len) = (size_at(record, at)?, size_at(record, at.checked_add(4)?)?);
        record.get(start..start.checked_add(len)?)
    };
    let (header, pixels) = (part(at)?, part(at.checked_add(8)?)?);
    let mut dib = Vec::with_capacity(header.len().checked_add(pixels.len())?);
    dib.extend_from_slice(header);
    dib.extend_from_slice(pixels);
    Some(dib)
}

/// WMF records that set things up and draw nothing ([MS-WMF] 2.3.5 state records, 2.3.4 object
/// records, 2.3.6 escape).
const WMF_STATE: [u16; 37] = [
    0x001E, 0x0035, 0x00F7, 0x0102, 0x0103, 0x0104, 0x0105, 0x0106, 0x0107, 0x0108, 0x0127, 0x012C, 0x012D, 0x012E, 0x0142, 0x0149, 0x01F0, 0x0201,
    0x0209, 0x020A, 0x020B, 0x020C, 0x020D, 0x020E, 0x020F, 0x0211, 0x0231, 0x0234, 0x02FA, 0x02FB, 0x02FC, 0x0410, 0x0412, 0x0415, 0x0416, 0x0626,
    0x06FF,
];

/// Whether a record takes all of its bitmap and lays it the way it is kept: the part `from`
/// (left, top, wide, high) is the whole of it, or is not said, and the box it goes to is not
/// below nothing either way. A part, or a bitmap that is mirrored, is for the drawing.
fn whole(dib: &[u8], from: [i64; 4], to: (i64, i64)) -> bool {
    let all = match size_at(dib, 0) {
        Some(12) => u16_at(dib, 4).zip(u16_at(dib, 6)).map(|(w, h)| (i64::from(w), i64::from(h))),
        _ => u32_at(dib, 4).zip(u32_at(dib, 8)).map(|(w, h)| (i64::from(w.cast_signed()), i64::from(h.cast_signed()).abs())),
    };
    let unsaid = from[2] == 0 && from[3] == 0;
    to.0 >= 0 && to.1 >= 0 && (unsaid || all.is_none_or(|(w, h)| from == [0, 0, w, h]))
}

/// A Windows metafile that draws one bitmap and nothing else → that bitmap as a BMP file.
pub fn wmf_bitmap(data: &[u8]) -> Option<Vec<u8>> {
    // A placeable metafile has 22 bytes before its header ([MS-WMF] 2.3.2.3).
    let mut at = if u32_at(data, 0)? == 0x9AC6_CDD7 { 22 } else { 0 };
    // [MS-WMF] 2.3.2.2, META_HEADER: in memory or on disk, and nine words long.
    if !matches!(u16_at(data, at)?, 1 | 2) || u16_at(data, at + 2)? != 9 {
        return None;
    }
    at += 18;
    let mut found: Option<&[u8]> = None;
    for _ in 0..MAX_RECORDS {
        // [MS-WMF] 2.3: a record's size in words, then what it does.
        let size = size_at(data, at)?.checked_mul(2)?;
        let function = u16_at(data, at.checked_add(4)?)?;
        if size < 6 {
            return None;
        }
        let record = data.get(at..at.checked_add(size)?)?;
        let word = |at: usize| u16_at(record, at).map(|w| i64::from(w.cast_signed()));
        let taken = |start: usize, from: [usize; 4], to: (usize, usize)| {
            let from = [word(from[0])?, word(from[1])?, word(from[2])?, word(from[3])?];
            Some(whole(record.get(start..)?, from, (word(to.0)?, word(to.1)?)))
        };
        // Where the bitmap starts in each record that carries one, and where the record says how
        // it is copied or what its colours are numbers in (both must be the plain way), and
        // which part of it goes where (all of it, not mirrored).
        let bitmap = match function {
            // META_EOF.
            0x0000 => return found.and_then(dib_file),
            // [MS-WMF] 2.3.1.6, META_STRETCHDIB.
            0x0F43 => Some((28, u32_at(record, 6)? == COPY && u16_at(record, 10)? == 0 && taken(28, [18, 16, 14, 12], (22, 20))?)),
            // [MS-WMF] 2.3.1.3, META_DIBSTRETCHBLT, and 2.3.1.2, META_DIBBITBLT.
            0x0B41 => Some((26, u32_at(record, 6)? == COPY && taken(26, [16, 14, 12, 10], (20, 18))?)),
            0x0940 => Some((22, u32_at(record, 6)? == COPY && taken(22, [12, 10, 16, 14], (16, 14))?)),
            // [MS-WMF] 2.3.1.5, META_SETDIBTODEV.
            0x0D33 => Some((24, u16_at(record, 6)? == 0)),
            _ if WMF_STATE.contains(&function) => None,
            _ => return None,
        };
        if let Some((start, plain)) = bitmap {
            if !plain || found.is_some() {
                return None;
            }
            found = Some(record.get(start..)?);
        }
        at = at.checked_add(size)?;
    }
    None
}

/// A metafile as a picture file, and the words that are written in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Drawn {
    pub file: Vec<u8>,
    /// The text the drawing shows, in the order it is drawn; it comes from the message.
    pub words: String,
}

/// A Windows metafile → a picture file: the one bitmap it holds, as it is; else its drawing,
/// made for showing at `want` pixels when that is known. A drawing with text in it is not made.
pub fn wmf_picture(data: &[u8], want: Option<(u32, u32)>) -> Option<Vec<u8>> {
    wmf_drawn(data, want, None).map(|drawn| drawn.file)
}

/// As [`wmf_picture`], with the text of a drawing drawn in the fonts `letters` knows.
pub fn wmf_drawn(data: &[u8], want: Option<(u32, u32)>, letters: Option<&dyn Lettering>) -> Option<Drawn> {
    wmf_bitmap(data).map(|file| Drawn { file, words: String::new() }).or_else(|| crate::wmf::wmf_drawing(data, want, letters))
}

/// EMF records that set things up and draw nothing ([MS-EMF] 2.3.11 state records, 2.3.7 object
/// creation, 2.3.8 object manipulation, 2.3.2 clipping, 2.3.12 transforms, 2.3.3 comments).
const EMF_STATE: [u32; 43] = [
    9, 10, 11, 12, 13, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 48, 49, 52, 58, 70, 75,
    82, 93, 95, 98, 99, 100, 115,
];

/// An enhanced metafile that draws one bitmap and nothing else → that bitmap as a BMP file.
pub fn emf_bitmap(data: &[u8]) -> Option<Vec<u8>> {
    // [MS-EMF] 2.3.4.2, EMR_HEADER: the first record, with " EMF" 40 bytes in.
    if u32_at(data, 0)? != 1 || u32_at(data, 40)? != 0x464D_4520 {
        return None;
    }
    let mut at = size_at(data, 4)?;
    if at < 88 || !at.is_multiple_of(4) {
        return None;
    }
    let mut found: Option<Vec<u8>> = None;
    for _ in 0..MAX_RECORDS {
        // [MS-EMF] 2.3: a record's type and its size in bytes.
        let (kind, size) = (u32_at(data, at)?, size_at(data, at.checked_add(4)?)?);
        if size < 8 || !size.is_multiple_of(4) {
            return None;
        }
        let record = data.get(at..at.checked_add(size)?)?;
        match kind {
            // EMR_EOF.
            14 => return found.as_deref().and_then(dib_file),
            // [MS-EMF] 2.3.1.7, EMR_STRETCHDIBITS: the bitmap's header and its pixels are two
            // parts of the record, each at an offset from its start.
            81 => {
                if found.is_some() || u32_at(record, 64)? != 0 || u32_at(record, 68)? != COPY {
                    return None;
                }
                let long = |at: usize| u32_at(record, at).map(|n| i64::from(n.cast_signed()));
                let dib = emf_parts(record, 48)?;
                if !whole(&dib, [long(32)?, long(36)?, long(40)?, long(44)?], (long(72)?, long(76)?)) {
                    return None;
                }
                found = Some(dib);
            }
            _ if EMF_STATE.contains(&kind) => {}
            _ => return None,
        }
        at = at.checked_add(size)?;
    }
    None
}

/// An enhanced metafile → a picture file: the one bitmap it holds, as it is; else its drawing,
/// made for showing at `want` pixels when that is known. A drawing with text in it is not made.
pub fn emf_picture(data: &[u8], want: Option<(u32, u32)>) -> Option<Vec<u8>> {
    emf_drawn(data, want, None).map(|drawn| drawn.file)
}

/// As [`emf_picture`], with the text of a drawing drawn in the fonts `letters` knows.
pub fn emf_drawn(data: &[u8], want: Option<(u32, u32)>, letters: Option<&dyn Lettering>) -> Option<Drawn> {
    emf_bitmap(data).map(|file| Drawn { file, words: String::new() }).or_else(|| crate::emf::emf_drawing(data, want, letters))
}
