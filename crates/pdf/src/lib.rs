//! Writes pages of text, fills, lines and pictures to a PDF file.
//!
//! A [`Writer`] is told the fonts, then page after page what stands on it, in points from the
//! page's top left corner; [`Writer::finish`] gives the file. Text is given as glyphs of a font
//! that was laid out elsewhere: nothing is measured or wrapped here. Each font is carried in the
//! file with only the glyphs that were used, when its maker allows that
//! ([`openmailclient_fonts::embedding`]).
//!
//! The file layout is from ISO 32000-1 (PDF 1.7): 7.5 (file structure), 7.7 (document
//! structure), 8 (graphics), 8.9 (images), 9.4 (text objects), 9.7 (composite fonts), 9.8 (font descriptors),
//! 9.9 (embedded font programs), 9.10 (extraction of text content).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod font;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;

use openmailclient_fonts::Embedding;

/// Red, green, blue.
pub type Rgb = [u8; 3];

/// Pages in one file.
pub const MAX_PAGES: usize = 5000;
/// Fonts in one file.
pub const MAX_FONTS: usize = 256;
/// Pictures in one file, the longest side of one, and the bytes of all of them unpacked.
pub const MAX_IMAGES: usize = 256;
pub const MAX_IMAGE_SIDE: u32 = 8192;
const MAX_IMAGE_BYTES: usize = 512 << 20;
/// Bytes of drawing instructions on one page.
const MAX_PAGE_BYTES: usize = 16 << 20;
/// Glyphs in one piece of text.
const MAX_PIECE: usize = 4096;
/// The most stretches of one piece that say what they stand for; the glyphs after them are
/// taken for the characters the file says they are.
const MAX_SAID: usize = 256;
/// The largest measure written, in points; a page is far smaller.
const MAX_MEASURE: f32 = 14_400.0;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PdfError {
    #[error("the font cannot be put into a PDF file")]
    Font,
    #[error("too many fonts for one PDF file")]
    TooManyFonts,
    #[error("the PDF file would be too large")]
    TooLarge,
    #[error("the picture cannot be put into a PDF file")]
    Image,
    #[error("too many pictures for one PDF file")]
    TooManyImages,
}

/// A font of this file, from [`Writer::font`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontId(usize);

/// A picture of this file, from [`Writer::image`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageId(usize);

/// How a line is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stroke {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

/// Glyphs of a piece that are not one character each, and what they stand for together:
/// letters drawn as one glyph, a letter drawn as several.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Said<'a> {
    /// The first of the glyphs, counted in the piece.
    pub from: usize,
    /// How many glyphs.
    pub count: usize,
    /// What they stand for, in the order it is read; nothing for a mark read with its letter.
    pub text: &'a str,
}

/// One piece of text: glyphs of one font, in one size and colour, on one baseline.
#[derive(Clone, Copy, Debug)]
pub struct Piece<'a> {
    pub font: FontId,
    /// Font size in points.
    pub size: f32,
    /// Where the first glyph starts.
    pub x: f32,
    /// The baseline, from the top of the page.
    pub baseline: f32,
    pub color: Rgb,
    /// Each glyph with the character it stands for, which is what a reader finds and copies.
    pub glyphs: &'a [(u16, char)],
    /// Kerning: how much further than its own width each glyph moves on before the next, in
    /// points. Empty, or shorter than the glyphs, where none is moved.
    pub kerns: &'a [f32],
    /// The glyphs that are not one character each, from the first on, none of them twice.
    /// Text written from the right is not among them: its glyphs are given in the order they
    /// are seen, as every reader expects them, and the reader turns them round itself (one
    /// that was tried turned a line round twice when the line said how it is read).
    pub actual: &'a [Said<'a>],
    /// Thicken the glyphs: the face has no bold of its own.
    pub bold: bool,
    /// Lean the glyphs: the face has no italic of its own.
    pub slant: bool,
}

struct Font {
    data: Arc<[u8]>,
    index: u32,
    how: Embedding,
    used: BTreeMap<u16, char>,
}

/// A picture's samples: three bytes of colour for each pixel, and where any pixel lets what
/// is under it show, a byte for each of how much it covers.
struct Image {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
    alpha: Option<Vec<u8>>,
}

struct Page {
    width: f32,
    height: f32,
    ops: String,
}

#[derive(Default)]
pub struct Writer {
    fonts: Vec<Font>,
    images: Vec<Image>,
    image_bytes: usize,
    pages: Vec<Page>,
}

/// A number as a PDF file writes it: plain decimals, never an exponent.
fn num(v: f32) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let text = format!("{:.2}", v.clamp(-MAX_MEASURE, MAX_MEASURE));
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-" || text == "-0" { "0".to_string() } else { text.to_string() }
}

fn rgb(color: Rgb) -> String {
    let part = |c: u8| num(f32::from(c) / 255.0);
    format!("{} {} {}", part(color[0]), part(color[1]), part(color[2]))
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pages(&self) -> usize {
        self.pages.len()
    }

    /// Take a face into the file. An error when its file cannot be read, when it holds no
    /// TrueType outlines, or when its maker does not allow it to be embedded.
    pub fn font(&mut self, data: Arc<[u8]>, index: u32) -> Result<FontId, PdfError> {
        if self.fonts.len() >= MAX_FONTS {
            return Err(PdfError::TooManyFonts);
        }
        let how = openmailclient_fonts::embedding(&data, index).ok_or(PdfError::Font)?;
        font::measures(&data, index).ok_or(PdfError::Font)?;
        self.fonts.push(Font { data, index, how, used: BTreeMap::new() });
        Ok(FontId(self.fonts.len() - 1))
    }

    /// Start a page; what is drawn next stands on it. False when the file has all the pages it
    /// may hold, and what is drawn next is dropped.
    pub fn page(&mut self, width: f32, height: f32) -> bool {
        if self.pages.len() >= MAX_PAGES {
            return false;
        }
        let side = |v: f32| if v.is_finite() { v.clamp(1.0, MAX_MEASURE) } else { 1.0 };
        self.pages.push(Page { width: side(width), height: side(height), ops: String::new() });
        true
    }

    fn open(&mut self) -> Option<&mut Page> {
        self.pages.last_mut().filter(|page| page.ops.len() < MAX_PAGE_BYTES)
    }

    /// Fill a rectangle given by its top left corner and size.
    pub fn fill(&mut self, x: f32, y: f32, width: f32, height: f32, color: Rgb) {
        let Some(page) = self.open() else { return };
        let bottom = page.height - y - height;
        let _ = writeln!(page.ops, "{} rg {} {} {} {} re f", rgb(color), num(x), num(bottom), num(width), num(height));
    }

    /// Draw a straight line `width` thick.
    pub fn line(&mut self, from: (f32, f32), to: (f32, f32), width: f32, color: Rgb, stroke: Stroke) {
        let Some(page) = self.open() else { return };
        let w = if width.is_finite() { width.clamp(0.1, 72.0) } else { 0.1 };
        let dash = match stroke {
            Stroke::Solid => "[] 0 d".to_string(),
            Stroke::Dashed => format!("[{} {}] 0 d", num(w * 3.0), num(w * 3.0)),
            Stroke::Dotted => format!("[{} {}] 0 d", num(w), num(w)),
        };
        let (h, ops) = (page.height, &mut page.ops);
        let _ = writeln!(ops, "{} RG {} w {dash} {} {} m {} {} l S", rgb(color), num(w), num(from.0), num(h - from.1), num(to.0), num(h - to.1));
    }

    /// Take a picture into the file: `rgba` holds red, green, blue and cover for each pixel, row
    /// after row from the top. An error when the bytes are not `width` by `height` pixels, or
    /// the picture or all of them together are too large.
    pub fn image(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<ImageId, PdfError> {
        if self.images.len() >= MAX_IMAGES {
            return Err(PdfError::TooManyImages);
        }
        if width == 0 || height == 0 || width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
            return Err(PdfError::Image);
        }
        let pixels = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| PdfError::Image)?;
        if pixels.checked_mul(4) != Some(rgba.len()) {
            return Err(PdfError::Image);
        }
        let bytes = pixels.saturating_mul(4);
        if self.image_bytes.saturating_add(bytes) > MAX_IMAGE_BYTES {
            return Err(PdfError::TooLarge);
        }
        let mut rgb = Vec::with_capacity(pixels.saturating_mul(3));
        let mut alpha = Vec::with_capacity(pixels);
        for pixel in rgba.chunks_exact(4) {
            rgb.extend(pixel.iter().take(3));
            alpha.push(pixel.get(3).copied().unwrap_or(255));
        }
        let alpha = alpha.iter().any(|a| *a != 255).then_some(alpha);
        self.image_bytes += bytes;
        self.images.push(Image { width, height, rgb, alpha });
        Ok(ImageId(self.images.len() - 1))
    }

    /// Draw a picture into the rectangle given by its top left corner and size. Dropped when
    /// the picture is not of this file.
    pub fn picture(&mut self, image: ImageId, x: f32, y: f32, width: f32, height: f32) {
        if image.0 >= self.images.len() {
            return;
        }
        let Some(page) = self.open() else { return };
        let side = |v: f32| if v.is_finite() { v.clamp(0.01, MAX_MEASURE) } else { 0.01 };
        let (width, height) = (side(width), side(height));
        let bottom = page.height - y - height;
        // An image fills the unit square; the matrix puts that square on the page (8.9.5).
        let _ = writeln!(page.ops, "q {} 0 0 {} {} {} cm /Im{} Do Q", num(width), num(height), num(x), num(bottom), image.0);
    }

    /// Draw a piece of text. Dropped when the font is not of this file.
    pub fn text(&mut self, piece: Piece) {
        let Some(font) = self.fonts.get_mut(piece.font.0) else { return };
        let Some(page) = self.pages.last_mut().filter(|page| page.ops.len() < MAX_PAGE_BYTES) else { return };
        let size = if piece.size.is_finite() { piece.size.clamp(1.0, 1000.0) } else { 10.0 };
        if piece.glyphs.is_empty() {
            return;
        }
        let count = piece.glyphs.len().min(MAX_PIECE);
        // The glyphs in stretches: those that are one character each, and between them those
        // that say together what they stand for. What is said of glyphs that are not there,
        // or of glyphs already said, is not heard.
        let mut stretches: Vec<(usize, usize, Option<String>)> = Vec::new();
        let mut said = piece.actual.iter().take(MAX_SAID).peekable();
        let mut own = 0usize;
        let mut at = 0usize;
        let mut units = 0usize;
        while at < count {
            while said.next_if(|s| s.from < at || s.count == 0 || s.from.checked_add(s.count).is_none_or(|end| end > count)).is_some() {}
            let (to, text) = match said.peek() {
                Some(s) if s.from == at => (at + s.count, Some(s.text.to_string())),
                // A glyph the file already says is another character says here what it is.
                _ => match piece.glyphs.get(at) {
                    Some((glyph, ch)) if own < MAX_SAID && font.used.get(glyph).is_some_and(|was| was != ch) => {
                        own += 1;
                        (at + 1, Some(ch.to_string()))
                    }
                    _ => (at + 1, None),
                },
            };
            match (stretches.last_mut(), &text) {
                (Some((_, end, None)), None) => *end = to,
                _ => stretches.push((at, to, text)),
            }
            at = to;
        }
        let color = rgb(piece.color);
        let ops = &mut page.ops;
        let _ = write!(ops, "BT /F{} {} Tf {color} rg ", piece.font.0, num(size));
        if piece.bold {
            let _ = write!(ops, "{color} RG {} w 2 Tr ", num(size * 0.03));
        } else {
            ops.push_str("0 Tr ");
        }
        let lean = if piece.slant { "0.2" } else { "0" };
        let _ = write!(ops, "1 0 {lean} 1 {} {} Tm", num(piece.x), num(page.height - piece.baseline));
        for (from, to, text) in stretches {
            // What a stretch stands for, around it: in UTF-16 with its mark of order before it
            // (14.9.4 replacement text, 7.9.2.2 text strings; 14.6: marked content may stand
            // inside a text object when it opens and closes there).
            if let Some(text) = &text {
                ops.push_str(" /Span << /ActualText <FEFF");
                // Never half of a character that takes two units, and no more than a piece may say.
                for ch in text.chars().take_while(|ch| {
                    units += ch.len_utf16();
                    units <= MAX_PIECE
                }) {
                    for unit in ch.encode_utf16(&mut [0u16; 2]) {
                        let _ = write!(ops, "{unit:04X}");
                    }
                }
                ops.push_str("> >> BDC");
            }
            // Glyphs are shown as one string; where one is kerned the string is cut, with the
            // amount between the parts in thousandths of the size, taken off the way on (9.4.3,
            // the `TJ` operator).
            let mut hex = String::with_capacity((to - from) * 4 + 2);
            let (mut kerned, mut open) = (false, true);
            hex.push('<');
            for (i, (glyph, ch)) in piece.glyphs.iter().enumerate().take(to).skip(from) {
                font.used.entry(*glyph).or_insert(*ch);
                let _ = write!(hex, "{glyph:04X}");
                let kern = piece.kerns.get(i).copied().filter(|k| k.is_finite()).unwrap_or(0.0);
                let off = (-kern / size * 1000.0).clamp(-2000.0, 2000.0).round();
                if off != 0.0 && i + 1 < count {
                    // The last glyph of a stretch moves the next stretch.
                    let _ = if i + 1 < to { write!(hex, "> {off} <") } else { write!(hex, "> {off}") };
                    (kerned, open) = (true, i + 1 < to);
                }
            }
            if open {
                hex.push('>');
            }
            let _ = if kerned { write!(ops, " [{hex}] TJ") } else { write!(ops, " {hex} Tj") };
            if text.is_some() {
                ops.push_str(" EMC");
            }
        }
        ops.push_str(" ET\n");
    }

    /// The finished file. `title` is what a reader's window shows for it.
    pub fn finish(self, title: &str) -> Result<Vec<u8>, PdfError> {
        let mut file = File::new();
        let (catalog, tree, info) = (file.id(), file.id(), file.id());

        let mut font_ids: Vec<usize> = Vec::with_capacity(self.fonts.len());
        for (n, font) in self.fonts.iter().enumerate() {
            font_ids.push(write_font(&mut file, n, font)?);
        }
        let mut resources = String::from("<< /Font <<");
        for (n, id) in font_ids.iter().enumerate() {
            let _ = write!(resources, " /F{n} {id} 0 R");
        }
        resources.push_str(" >>");
        if !self.images.is_empty() {
            resources.push_str(" /XObject <<");
            for (n, image) in self.images.iter().enumerate() {
                let id = file.id();
                let size = format!("/Type /XObject /Subtype /Image /Width {} /Height {} /BitsPerComponent 8 ", image.width, image.height);
                // How much each pixel covers is an image of its own, in grey (11.6.5.3 soft masks).
                let mask = match &image.alpha {
                    Some(alpha) => {
                        let mask = file.id();
                        file.stream(mask, &format!("{size}/ColorSpace /DeviceGray "), alpha)?;
                        format!("/SMask {mask} 0 R ")
                    }
                    None => String::new(),
                };
                file.stream(id, &format!("{size}/ColorSpace /DeviceRGB {mask}"), &image.rgb)?;
                let _ = write!(resources, " /Im{n} {id} 0 R");
            }
            resources.push_str(" >>");
        }
        resources.push_str(" >>");

        let mut kids = String::new();
        for page in &self.pages {
            let (id, contents) = (file.id(), file.id());
            file.stream(contents, "", page.ops.as_bytes())?;
            let (w, h) = (num(page.width), num(page.height));
            file.object(
                id,
                &format!("<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 {w} {h}] /Resources {resources} /Contents {contents} 0 R >>"),
            )?;
            let _ = write!(kids, "{id} 0 R ");
        }
        file.object(tree, &format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.trim_end(), self.pages.len()))?;
        file.object(catalog, &format!("<< /Type /Catalog /Pages {tree} 0 R >>"))?;
        // Text strings as UTF-16 with a byte order mark, in hexadecimal (7.9.2.2).
        let mut named = String::from("FEFF");
        for unit in title.chars().filter(|c| !c.is_control()).take(200).collect::<String>().encode_utf16() {
            let _ = write!(named, "{unit:04X}");
        }
        file.object(info, &format!("<< /Title <{named}> /Producer (OpenMailClient) >>"))?;
        file.end(catalog, info)
    }
}

/// The objects of one font (9.7.4 CIDFonts, 9.7.6 Type 0 font dictionaries); gives the number
/// of the font's own object.
fn write_font(file: &mut File, n: usize, font: &Font) -> Result<usize, PdfError> {
    let measures = font::measures(&font.data, font.index).ok_or(PdfError::Font)?;
    let used: BTreeSet<u16> = font.used.keys().copied().collect();
    let program = font::file(&font.data, font.index, &used, font.how).ok_or(PdfError::Font)?;
    let (top, descendant, descriptor, embedded, unicode) = (file.id(), file.id(), file.id(), file.id(), file.id());

    // A subset is named with six letters and a plus sign before its name (9.6.4).
    let mut tag = String::new();
    let mut left = n;
    for _ in 0..6 {
        tag.push(char::from(b'A' + (left % 26) as u8));
        left /= 26;
    }
    let name = format!("{tag}+{}", measures.name);
    let scale = 1000.0 / measures.units;
    let in_thousands = |v: f32| num(v * scale);

    file.stream(embedded, &format!("/Length1 {} ", program.len()), &program)?;
    let b = measures.bounds;
    let flags = 4 | if measures.fixed_pitch { 1 } else { 0 } | if measures.italic_angle != 0.0 { 64 } else { 0 };
    file.object(
        descriptor,
        &format!(
            "<< /Type /FontDescriptor /FontName /{name} /Flags {flags} /FontBBox [{} {} {} {}] /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {embedded} 0 R >>",
            in_thousands(f32::from(b[0])),
            in_thousands(f32::from(b[1])),
            in_thousands(f32::from(b[2])),
            in_thousands(f32::from(b[3])),
            num(measures.italic_angle),
            in_thousands(f32::from(measures.ascent)),
            in_thousands(f32::from(measures.descent)),
            in_thousands(f32::from(measures.ascent) * 0.9),
        ),
    )?;
    let mut widths = String::new();
    for glyph in &used {
        let advance = measures.advances.get(usize::from(*glyph)).copied().unwrap_or(0);
        let _ = write!(widths, "{glyph} [{}] ", in_thousands(f32::from(advance)));
    }
    file.object(
        descendant,
        &format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {descriptor} 0 R /DW 1000 /W [{}] /CIDToGIDMap /Identity >>",
            widths.trim_end()
        ),
    )?;

    // Which character each glyph stands for (9.10.3 ToUnicode CMaps).
    let mut map = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let pairs: Vec<(&u16, &char)> = font.used.iter().collect();
    for chunk in pairs.chunks(100) {
        let _ = writeln!(map, "{} beginbfchar", chunk.len());
        for (glyph, ch) in chunk {
            let _ = write!(map, "<{glyph:04X}> <");
            let mut units = [0u16; 2];
            for unit in ch.encode_utf16(&mut units) {
                let _ = write!(map, "{unit:04X}");
            }
            map.push_str(">\n");
        }
        map.push_str("endbfchar\n");
    }
    map.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    file.stream(unicode, "", map.as_bytes())?;

    file.object(
        top,
        &format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H /DescendantFonts [{descendant} 0 R] /ToUnicode {unicode} 0 R >>"
        ),
    )?;
    Ok(top)
}

/// The bytes of the file as they grow, and where each object starts (7.5.4 cross-reference table).
struct File {
    out: Vec<u8>,
    starts: Vec<Option<usize>>,
}

/// The largest file written.
const MAX_FILE: usize = 1 << 30;

impl File {
    fn new() -> Self {
        // The second line holds bytes above 127, which tells a reader the file is not text.
        Self { out: b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec(), starts: vec![None] }
    }

    /// The number of an object that is written later.
    fn id(&mut self) -> usize {
        self.starts.push(None);
        self.starts.len() - 1
    }

    fn begin(&mut self, id: usize, more: usize) -> Result<(), PdfError> {
        if self.out.len().saturating_add(more) > MAX_FILE {
            return Err(PdfError::TooLarge);
        }
        let at = self.out.len();
        *self.starts.get_mut(id).ok_or(PdfError::TooLarge)? = Some(at);
        self.out.extend(format!("{id} 0 obj\n").as_bytes());
        Ok(())
    }

    fn object(&mut self, id: usize, body: &str) -> Result<(), PdfError> {
        self.begin(id, body.len())?;
        self.out.extend(body.as_bytes());
        self.out.extend(b"\nendobj\n");
        Ok(())
    }

    /// A stream, compressed (7.3.8, 7.4.4 FlateDecode). `entries` are more entries of its dictionary.
    fn stream(&mut self, id: usize, entries: &str, data: &[u8]) -> Result<(), PdfError> {
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(data, 6);
        self.begin(id, packed.len())?;
        self.out.extend(format!("<< {entries}/Filter /FlateDecode /Length {} >>\nstream\n", packed.len()).as_bytes());
        self.out.extend(&packed);
        self.out.extend(b"\nendstream\nendobj\n");
        Ok(())
    }

    fn end(mut self, catalog: usize, info: usize) -> Result<Vec<u8>, PdfError> {
        let at = self.out.len();
        let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", self.starts.len());
        for start in self.starts.iter().skip(1) {
            // Every object asked for was written; one that was not is marked free.
            match start {
                Some(start) => {
                    let _ = writeln!(table, "{start:010} 00000 n ");
                }
                None => table.push_str("0000000000 65535 f \n"),
            }
        }
        let _ = write!(table, "trailer\n<< /Size {} /Root {catalog} 0 R /Info {info} 0 R >>\nstartxref\n{at}\n%%EOF\n", self.starts.len());
        self.out.extend(table.as_bytes());
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests;
