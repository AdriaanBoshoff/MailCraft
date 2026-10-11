//! Printing: the open message laid out in Memo Style and written as a PDF file.

use std::sync::OnceLock;

use mailcraft_fonts::Library;
use mailcraft_formats::rich::Doc;
use mailcraft_layout::{Fonts, LayoutError, Line, Memo, Paper, Printed};
use mailcraft_model::{Item, RECIP_BCC, RECIP_CC, RECIP_TO, time};
use mailcraft_pdf::{Piece, Stroke, Writer};

use crate::EngineError;

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
pub fn layout(item: &Item, owner: &str, tz_offset_min: i32, paper: Paper, library: &Library) -> Result<Printed, EngineError> {
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
    let mut fonts = Fonts::new(library);
    mailcraft_layout::memo(&Memo { owner, fields: &fields, body: &doc, plain }, paper, &mut fonts).map_err(|e| match e {
        LayoutError::NoFont => EngineError::Format(e.to_string()),
    })
}

/// Write laid-out pages as a PDF file.
pub fn write(printed: &Printed, title: &str) -> Result<Vec<u8>, EngineError> {
    let mut writer = Writer::new();
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
    let printed = layout(item, owner, tz_offset_min, paper, library())?;
    Ok(Print { file: write(&printed, &item.subject())?, pages: printed.pages.len(), truncated: printed.truncated })
}
