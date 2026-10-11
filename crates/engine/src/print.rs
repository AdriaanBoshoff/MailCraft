//! Printing: the open message laid out in Memo Style and written as a PDF file.

use std::sync::OnceLock;

use mailcraft_fonts::Library;
use mailcraft_formats::rich::{Block, Doc, Picture};
use mailcraft_layout::{Fonts, LayoutError, Line, Memo, Paper, Printed};
use mailcraft_model::{Item, RECIP_BCC, RECIP_CC, RECIP_TO, time};
use mailcraft_pdf::{Piece, Stroke, Writer};

use crate::EngineError;
use crate::pictures::{self, Pixels};

/// Pictures printed with one message, and the bytes of them all, decoded.
const MAX_PICTURES: usize = 64;
const MAX_PICTURE_BYTES: usize = 128 << 20;
const MAX_DEPTH: usize = 64;

/// The decoded pictures of a message's body, each under the name the body has for it.
#[derive(Default)]
pub struct Pictures {
    found: Vec<(Picture, Pixels)>,
    bytes: usize,
}

impl Pictures {
    /// Decode the pictures `doc` shows from `item`'s attachments and from the body itself.
    /// Nothing is fetched: a picture on a server is not among them.
    pub fn of(doc: &Doc, item: &Item) -> Self {
        let mut all = Pictures::default();
        all.walk(&doc.blocks, doc, item, 0);
        all
    }

    fn walk(&mut self, blocks: &[Block], doc: &Doc, item: &Item, depth: usize) {
        for block in blocks {
            match block {
                Block::Para(para) => {
                    for picture in para.runs.iter().filter_map(|run| run.picture.as_ref()) {
                        self.take(picture, doc, item);
                    }
                }
                Block::Table(table) if depth < MAX_DEPTH => {
                    for cell in table.rows.iter().flatten() {
                        self.walk(&cell.blocks, doc, item, depth + 1);
                    }
                }
                _ => {}
            }
        }
    }

    fn take(&mut self, picture: &Picture, doc: &Doc, item: &Item) {
        if self.found.len() >= MAX_PICTURES || self.bytes >= MAX_PICTURE_BYTES || self.get(picture).is_some() {
            return;
        }
        let data: Option<&[u8]> = match picture {
            Picture::Attached(id) => pictures::attached(item, id).map(|a| a.data.as_slice()),
            Picture::Inline(Some(index)) => doc.written.get(*index).map(|data| &data[..]),
            Picture::Inline(None) | Picture::Remote => None,
        };
        if let Some(pixels) = data.and_then(pictures::decode) {
            self.bytes = self.bytes.saturating_add(pixels.rgba.len());
            self.found.push((picture.clone(), pixels));
        }
    }

    fn place(&self, picture: &Picture) -> Option<usize> {
        self.found.iter().position(|(known, _)| known == picture)
    }

    pub fn get(&self, picture: &Picture) -> Option<&Pixels> {
        self.found.get(self.place(picture)?).map(|(_, pixels)| pixels)
    }
}

/// A message laid out, with the pictures its pages show.
pub struct Laid {
    pub printed: Printed,
    pub pictures: Pictures,
}

/// What printing a message gave.
pub struct Print {
    pub file: Vec<u8>,
    pub pages: usize,
    /// The message was longer than may be printed; the end of it is not in the file.
    pub truncated: bool,
}

/// The installed fonts, looked up once: reading them all takes a moment.
fn library() -> &'static Library {
    static LIBRARY: OnceLock<Library> = OnceLock::new();
    LIBRARY.get_or_init(Library::system)
}

fn mailbox(name: &str, email: &str) -> String {
    match (name.trim().is_empty(), email.trim().is_empty() || name.eq_ignore_ascii_case(email)) {
        (true, _) => email.to_string(),
        (false, true) => name.to_string(),
        (false, false) => format!("{name} <{email}>"),
    }
}

/// The message's formatted body, or its plain one as a document. True for a plain one.
fn body(item: &Item) -> (Doc, bool) {
    if let Some(html) = item.body_html() {
        let doc = mailcraft_formats::html::parse(&html);
        if !doc.is_empty() {
            return (doc, false);
        }
    }
    if let Some(rtf) = item.body_rtf() {
        let doc = mailcraft_formats::rtf::parse(&rtf);
        if !doc.is_empty() {
            return (doc, false);
        }
    }
    (Doc::plain(&item.body_text()), true)
}

/// Lay `item` out on `paper` in the fonts of `library`.
pub fn layout(item: &Item, owner: &str, tz_offset_min: i32, paper: Paper, library: &Library) -> Result<Laid, EngineError> {
    let attachments: Vec<String> = item.visible_attachments().map(|a| a.name()).collect();
    let sent = if item.time() > 0 { time::civil(item.time(), tz_offset_min).full_stamp() } else { String::new() };
    let fields = [
        ("From", mailbox(&item.sender_name(), &item.sender_email())),
        ("Sent", sent),
        ("To", item.display_recipients(RECIP_TO)),
        ("Cc", item.display_recipients(RECIP_CC)),
        ("Bcc", item.display_recipients(RECIP_BCC)),
        ("Subject", item.subject()),
        ("Attachments", attachments.join("; ")),
    ];
    let (doc, plain) = body(item);
    let pictures = Pictures::of(&doc, item);
    let sizes = |picture: &Picture| pictures.get(picture).map(|pixels| pixels.natural);
    let mut fonts = Fonts::new(library);
    let printed =
        mailcraft_layout::memo(&Memo { owner, fields: &fields, body: &doc, plain, sizes: &sizes }, paper, &mut fonts).map_err(|e| match e {
            LayoutError::NoFont => EngineError::Format(e.to_string()),
        })?;
    Ok(Laid { printed, pictures })
}

/// Write laid-out pages as a PDF file.
pub fn write(printed: &Printed, pictures: &Pictures, title: &str) -> Result<Vec<u8>, EngineError> {
    let mut writer = Writer::new();
    // Each picture goes into the file once, when a page first shows it; one the file cannot
    // carry is left out.
    let mut images: Vec<Option<Option<mailcraft_pdf::ImageId>>> = vec![None; pictures.found.len()];
    // A face the file cannot carry leaves its text out; the layout picks none such.
    let fonts: Vec<_> = printed.faces.iter().map(|face| writer.font(face.data.clone(), face.index).ok()).collect();
    for page in &printed.pages {
        if !writer.page(page.width, page.height) {
            break;
        }
        for fill in &page.fills {
            writer.fill(fill.x, fill.y, fill.width, fill.height, fill.color);
        }
        for rule in &page.rules {
            let stroke = match rule.line {
                Line::Dashed => Stroke::Dashed,
                Line::Dotted => Stroke::Dotted,
                _ => Stroke::Solid,
            };
            writer.line(rule.from, rule.to, rule.width, rule.color, stroke);
        }
        for placed in &page.pictures {
            let Some(at) = pictures.place(&placed.picture) else { continue };
            let known = images.get(at).copied().flatten();
            let image = match known {
                Some(image) => image,
                None => {
                    let image = pictures.found.get(at).and_then(|(_, p)| writer.image(p.width, p.height, &p.rgba).ok());
                    if let Some(slot) = images.get_mut(at) {
                        *slot = Some(image);
                    }
                    image
                }
            };
            if let Some(image) = image {
                writer.picture(image, placed.x, placed.y, placed.width, placed.height);
            }
        }
        for text in &page.texts {
            if let Some(Some(font)) = fonts.get(text.face) {
                writer.text(Piece {
                    font: *font,
                    size: text.size,
                    x: text.x,
                    baseline: text.y,
                    color: text.color,
                    glyphs: &text.glyphs,
                    bold: text.bold,
                    slant: text.slant,
                });
            }
        }
    }
    writer.finish(title).map_err(|e| EngineError::Format(e.to_string()))
}

/// Print `item` to a PDF file's bytes, in the installed fonts.
pub fn pdf(item: &Item, owner: &str, tz_offset_min: i32, paper: Paper) -> Result<Print, EngineError> {
    let Laid { printed, pictures } = layout(item, owner, tz_offset_min, paper, library())?;
    Ok(Print { file: write(&printed, &pictures, &item.subject())?, pages: printed.pages.len(), truncated: printed.truncated })
}
