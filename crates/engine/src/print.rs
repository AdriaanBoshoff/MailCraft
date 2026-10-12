//! Printing: the open message laid out in Memo Style, or the folder's list in Table Style,
//! and written as a PDF file.

use openmailclient_fonts::Library;
use openmailclient_formats::links::{self, Concern};
use openmailclient_formats::rich::{Block, Doc, Para, Picture, Run, Script, Style as Look};
use openmailclient_layout::{Column, Fonts, LayoutError, Line, Listed, MM, Memo, Paper, Printed, Row, Setup};
use openmailclient_model::{Flag, FolderKind, Item, RECIP_BCC, RECIP_CC, RECIP_TO, StoreError, time};
use openmailclient_pdf::{Piece, Stroke, Writer};
use serde_json::{Value, json};

use crate::pictures::{self, Pixels};
use crate::{EngineError, attachments};

/// Pictures printed with one message, and the bytes of them all, decoded.
const MAX_PICTURES: usize = 64;
const MAX_PICTURE_BYTES: usize = 128 << 20;
const MAX_DEPTH: usize = 64;
/// Links listed after one message, and characters of a link's text and of its address there.
const MAX_LINKS: usize = 200;
const MAX_LINK_TEXT: usize = 80;
const MAX_ADDRESS: usize = 300;

/// Which of a message's links are listed after it. On paper no pointer can rest on a link to
/// ask where it goes, so the page has to say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Links {
    /// None: the body alone.
    None,
    /// Those that are not what their text says.
    #[default]
    Concerns,
    /// Every link whose text is not its address.
    All,
}

impl Links {
    /// By name, in any letter case.
    pub fn named(name: &str) -> Option<Links> {
        match name.to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "concerns" => Some(Self::Concerns),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// How a print is laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    /// The open message: its fields, then its body.
    #[default]
    Memo,
    /// The folder's list as it is shown, a message to a row.
    Table,
}

impl Style {
    pub const ALL: [Style; 2] = [Style::Memo, Style::Table];

    /// By name, in any letter case, with or without "style" after it.
    pub fn named(name: &str) -> Option<Style> {
        let name = name.trim().to_ascii_lowercase();
        match name.strip_suffix("style").map_or(name.as_str(), str::trim_end) {
            "memo" => Some(Self::Memo),
            "table" => Some(Self::Table),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Memo => "memo",
            Self::Table => "table",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Memo => "Memo Style",
            Self::Table => "Table Style",
        }
    }
}

/// The style `p` asks for, or `now` when it asks for none.
pub fn style_with(now: Style, p: &Value) -> Result<Style, EngineError> {
    match p.get("style") {
        None | Some(Value::Null) => Ok(now),
        Some(v) => v.as_str().and_then(Style::named).ok_or_else(|| EngineError::BadParams("unknown style: memo or table".into())),
    }
}

/// A link that is listed: what it says, where it goes, and what is wrong with it.
struct Noted {
    text: String,
    address: String,
    concern: Option<Concern>,
}

fn shown(text: &str, most: usize) -> String {
    let mut out: String = text.chars().filter(|c| !c.is_control()).take(most).collect();
    if text.chars().filter(|c| !c.is_control()).nth(most).is_some() {
        out.push('\u{2026}');
    }
    out
}

/// Put a number after each link of `blocks` that is to be listed, and note the link under it.
fn number_links(blocks: &mut [Block], which: Links, noted: &mut Vec<Noted>, depth: usize) {
    for block in blocks {
        match block {
            Block::Para(para) => {
                let mut runs = Vec::with_capacity(para.runs.len());
                for (at, run) in para.runs.iter().enumerate() {
                    runs.push(run.clone());
                    let Some(link) = run.style.link.as_ref() else { continue };
                    // The number goes after the last run of a link.
                    if para.runs.get(at + 1).is_some_and(|next| next.style.link.as_ref() == Some(link)) {
                        continue;
                    }
                    let address = shown(link, MAX_ADDRESS);
                    let text = shown(links::text_around(&para.runs, at).trim(), MAX_LINK_TEXT);
                    let concern = links::concern_at(&para.runs, at);
                    // A place inside the message, a picture of it, or a link that is its own
                    // address written out, needs no line.
                    let plain = link.starts_with('#') || link.starts_with("cid:") || link.trim().is_empty() || text == address;
                    let wanted = match which {
                        Links::None => false,
                        Links::Concerns => concern.is_some(),
                        Links::All => concern.is_some() || !plain,
                    };
                    if !wanted {
                        continue;
                    }
                    let number = match noted.iter().position(|n| n.address == address && n.text == text) {
                        Some(known) => known + 1,
                        None if noted.len() < MAX_LINKS => {
                            noted.push(Noted { text, address, concern });
                            noted.len()
                        }
                        None => continue,
                    };
                    runs.push(Run {
                        text: format!("[{number}]"),
                        style: Look { script: Script::Super, size: run.style.size, ..Look::default() },
                        ..Run::default()
                    });
                }
                para.runs = runs;
            }
            Block::Table(table) if depth < MAX_DEPTH => {
                for cell in table.rows.iter_mut().flatten() {
                    number_links(&mut cell.blocks, which, noted, depth + 1);
                }
            }
            _ => {}
        }
    }
}

/// The list of the numbered links, to stand after the body.
fn listed(noted: &[Noted], which: Links) -> Vec<Block> {
    if noted.is_empty() {
        return Vec::new();
    }
    let run = |text: String, bold: bool| Run { text, style: Look { bold, size: Some(9.0), ..Look::default() }, ..Run::default() };
    let title = if which == Links::All { "Links in this message" } else { "Links that are not what they say" };
    let mut blocks = vec![Block::Rule, Block::Para(Para { runs: vec![run(title.to_string(), true)], ..Para::default() })];
    for (n, link) in noted.iter().enumerate() {
        let says = if link.text.is_empty() { "A link".to_string() } else { format!("\u{201c}{}\u{201d}", link.text) };
        let mut runs = vec![run(format!("[{}] {says} goes to {}", n + 1, link.address), false)];
        if let Some(concern) = &link.concern {
            runs.push(run(format!("  {}", concern.words()), true));
        }
        blocks.push(Block::Para(Para { runs, ..Para::default() }));
    }
    blocks
}

/// Number the links of `doc` that `which` asks for and list them after it. Says how many.
pub fn note_links(doc: &mut Doc, which: Links) -> usize {
    let mut noted = Vec::new();
    number_links(&mut doc.blocks, which, &mut noted, 0);
    doc.blocks.extend(listed(&noted, which));
    noted.len()
}

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
            Picture::Inline(None) | Picture::Remote | Picture::Object(_) | Picture::File(_) => None,
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

/// What a print is made for: whose mailbox, when, and on what pages.
pub struct Job<'a> {
    /// Whose mailbox the message is printed from.
    pub owner: &'a str,
    /// Minutes east of UTC for the dates on the pages.
    pub tz_offset_min: i32,
    /// "Now" as UTC FILETIME ticks, for a header or footer that asks for the date or time.
    pub now: i64,
    pub setup: &'a Setup,
    pub links: Links,
}

/// A print of the session's open message: from its owner's mailbox, now.
pub fn job<'a>(s: &'a crate::Session, setup: &'a Setup, links: Links) -> Job<'a> {
    let owner = if s.user_name.trim().is_empty() { &s.user_email } else { &s.user_name };
    Job { owner, tz_offset_min: s.tz_offset_min, now: s.now, setup, links }
}

/// The widest margin that may be asked for, and the most characters of one part of a header
/// or footer.
const MAX_MARGIN_MM: f64 = 150.0;
const MAX_EDGE_CHARS: usize = 400;

/// The three parts of a header or footer as `value` gives them: a text is the middle part,
/// and `{"left", "center", "right"}` names each; a part not named stays as it is in `now`.
fn edge(now: &[String; 3], value: &Value, key: &str) -> Result<[String; 3], EngineError> {
    let clean = |text: &str| text.chars().filter(|c| !c.is_control()).take(MAX_EDGE_CHARS).collect::<String>();
    let bad = || EngineError::BadParams(format!("\"{key}\" is a text, or {{\"left\", \"center\", \"right\"}} with a text each"));
    match value {
        Value::String(text) => Ok([String::new(), clean(text), String::new()]),
        Value::Object(parts) => {
            let mut out = now.clone();
            for (name, part) in parts {
                let at = match name.as_str() {
                    "left" => 0,
                    "center" | "centre" | "middle" => 1,
                    "right" => 2,
                    _ => return Err(bad()),
                };
                if let Some(slot) = out.get_mut(at) {
                    *slot = clean(part.as_str().ok_or_else(bad)?);
                }
            }
            Ok(out)
        }
        _ => Err(bad()),
    }
}

/// `setup` with what the parameters ask for changed: `"paper"`, `"orientation"`,
/// `"margins"` (millimetres: one number for every edge, or `{"top", "right", "bottom",
/// "left"}`), `"header"` and `"footer"`.
pub fn setup_with(setup: &Setup, p: &Value) -> Result<Setup, EngineError> {
    let mut out = setup.clone();
    if let Some(name) = p.get("paper").and_then(Value::as_str) {
        out.paper =
            Paper::named(name).ok_or_else(|| EngineError::BadParams(format!("unknown paper {name:?}: one of {}", Paper::NAMES.join(", "))))?;
    }
    if let Some(way) = p.get("orientation").and_then(Value::as_str) {
        out.landscape = match way.to_ascii_lowercase().as_str() {
            "portrait" => false,
            "landscape" => true,
            _ => return Err(EngineError::BadParams(format!("unknown orientation {way:?}: portrait or landscape"))),
        };
    }
    if let Some(value) = p.get("margins") {
        let bad = || {
            EngineError::BadParams(format!(
                "\"margins\" is millimetres from 0 to {MAX_MARGIN_MM}: one number, or {{\"top\", \"right\", \"bottom\", \"left\"}} with a number each"
            ))
        };
        let points = |v: &Value| v.as_f64().filter(|mm| (0.0..=MAX_MARGIN_MM).contains(mm)).map(|mm| mm as f32 * MM).ok_or_else(bad);
        match value {
            Value::Object(edges) => {
                for (name, v) in edges {
                    let at = match name.as_str() {
                        "top" => 0,
                        "right" => 1,
                        "bottom" => 2,
                        "left" => 3,
                        _ => return Err(bad()),
                    };
                    if let Some(slot) = out.margins.get_mut(at) {
                        *slot = points(v)?;
                    }
                }
            }
            v => out.margins = [points(v)?; 4],
        }
    }
    if let Some(value) = p.get("header") {
        out.header = edge(&out.header, value, "header")?;
    }
    if let Some(value) = p.get("footer") {
        out.footer = edge(&out.footer, value, "footer")?;
    }
    Ok(out)
}

/// A page setup as commands give it back: sizes in millimetres.
pub fn setup_json(setup: &Setup) -> Value {
    let mm = |points: f32| (f64::from(points / MM) * 10.0).round() / 10.0;
    let parts = |parts: &[String; 3]| json!({"left": parts[0], "center": parts[1], "right": parts[2]});
    let [top, right, bottom, left] = setup.margins;
    json!({
        "paper": setup.paper.name(),
        "width_mm": mm(setup.paper.width),
        "height_mm": mm(setup.paper.height),
        "orientation": if setup.landscape { "landscape" } else { "portrait" },
        "margins": {"top": mm(top), "right": mm(right), "bottom": mm(bottom), "left": mm(left)},
        "header": parts(&setup.header),
        "footer": parts(&setup.footer),
    })
}

/// A message laid out, with the pictures its pages show.
pub struct Laid {
    /// Links listed after the body.
    pub links: usize,
    pub printed: Printed,
    pub pictures: Pictures,
}

/// What printing a message gave.
pub struct Print {
    pub file: Vec<u8>,
    pub pages: usize,
    /// The message was longer than may be printed; the end of it is not in the file.
    pub truncated: bool,
    /// Links listed after the body.
    pub links: usize,
}

/// The installed fonts, looked up once: reading them all takes a moment.
fn library() -> &'static Library {
    pictures::lettering().library()
}

fn mailbox(name: &str, email: &str) -> String {
    match (name.trim().is_empty(), email.trim().is_empty() || name.eq_ignore_ascii_case(email)) {
        (true, _) => email.to_string(),
        (false, true) => name.to_string(),
        (false, false) => format!("{name} <{email}>"),
    }
}

/// The message's formatted body, or its plain one as a document. True for a plain one.
pub(crate) fn body(item: &Item) -> (Doc, bool) {
    if let Some(html) = item.body_html() {
        let doc = openmailclient_formats::html::parse(&html);
        if !doc.is_empty() {
            return (doc, false);
        }
    }
    if let Some(rtf) = item.body_rtf() {
        let mut doc = openmailclient_formats::rtf::parse_with(&rtf, Some(pictures::lettering()));
        pictures::settle(&mut doc, item);
        if !doc.is_empty() {
            return (doc, false);
        }
    }
    (Doc::plain(&item.body_text()), true)
}

/// What a header or footer may ask for by name. The subject is the message's own text: the
/// layout does not look through it for names.
pub(crate) fn words(item: &Item, job: &Job) -> Vec<(String, String)> {
    let now = time::civil(job.now, job.tz_offset_min);
    vec![
        ("Date".to_string(), format!("{} {}, {}", now.month_name(), now.day, now.year)),
        ("Time".to_string(), now.time_short()),
        ("User".to_string(), job.owner.to_string()),
        ("Subject".to_string(), item.subject()),
    ]
}

/// Lay `item` out for `job` in the fonts of `library`.
pub fn layout(item: &Item, job: &Job, library: &Library) -> Result<Laid, EngineError> {
    let Job { owner, tz_offset_min, links: which, .. } = *job;
    let mut setup = job.setup.clone();
    setup.words = words(item, job);
    // Each attachment under the name it would be saved as, with its size.
    let attachments: Vec<String> =
        item.visible_attachments().map(|a| format!("{} ({})", attachments::file_name(a), attachments::size_words(a.size()))).collect();
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
    let (mut doc, plain) = body(item);
    let links = note_links(&mut doc, which);
    let pictures = Pictures::of(&doc, item);
    let sizes = |picture: &Picture| pictures.get(picture).map(|pixels| pixels.natural);
    let mut fonts = Fonts::new(library);
    let printed =
        openmailclient_layout::memo(&Memo { owner, fields: &fields, body: &doc, plain, sizes: &sizes }, setup, &mut fonts).map_err(|e| match e {
            LayoutError::NoFont => EngineError::Format(e.to_string()),
        })?;
    Ok(Laid { links, printed, pictures })
}

/// Characters of a search that the head of a printed list repeats.
const MAX_SEARCH_SHOWN: usize = 80;

/// Lay the folder's list out as it is shown (searched, filtered and ordered), in the fonts
/// of `library`.
pub fn list_layout(s: &crate::Session, job: &Job, library: &Library) -> Result<Laid, EngineError> {
    let folder = s.current_folder().ok_or(StoreError::NoFolder)?;
    let outgoing = matches!(folder.kind, FolderKind::Sent | FolderKind::Drafts | FolderKind::Outbox);
    let columns = [
        Column { name: if outgoing { "To" } else { "From" }, share: 22, right: false },
        Column { name: "Subject", share: 38, right: false },
        Column { name: if outgoing { "Sent" } else { "Received" }, share: 18, right: false },
        Column { name: "Size", share: 9, right: true },
        Column { name: "Notes", share: 13, right: false },
    ];
    let rows: Vec<Row> = s
        .rows
        .iter()
        .take(openmailclient_layout::MAX_ROWS.saturating_add(1))
        .map(|row| {
            let who = if outgoing || row.from.is_empty() { &row.to } else { &row.from };
            let when = if row.time > 0 {
                let at = time::civil(row.time, job.tz_offset_min);
                format!("{}/{}/{} {}", at.month, at.day, at.year, at.time_short())
            } else {
                String::new()
            };
            let mut notes: Vec<&str> = Vec::new();
            if row.importance > 1 {
                notes.push("High");
            }
            if row.has_attachments {
                notes.push("Attachment");
            }
            match row.flag {
                Flag::None => {}
                Flag::Complete => notes.push("Done"),
                _ => notes.push("Flagged"),
            }
            Row { cells: vec![who.clone(), row.subject.clone(), when, attachments::size_words(row.size as usize), notes.join(", ")], bold: !row.read }
        })
        .collect();
    let unread = s.rows.iter().filter(|row| !row.read).count();
    let mut note = format!("{} item{}, {unread} unread", s.rows.len(), if s.rows.len() == 1 { "" } else { "s" });
    if s.unread_only {
        note.push_str("; unread only");
    }
    if !s.search.trim().is_empty() {
        note.push_str(&format!("; search: {}", shown(s.search.trim(), MAX_SEARCH_SHOWN)));
    }
    note.push_str(&format!("; by {}", s.sort.name()));
    let now = time::civil(job.now, job.tz_offset_min);
    let mut setup = job.setup.clone();
    setup.words = vec![
        ("Date".to_string(), format!("{} {}, {}", now.month_name(), now.day, now.year)),
        ("Time".to_string(), now.time_short()),
        ("User".to_string(), job.owner.to_string()),
        ("Folder".to_string(), folder.name.clone()),
        ("Subject".to_string(), folder.name.clone()),
    ];
    let mut fonts = Fonts::new(library);
    let listed = Listed { owner: job.owner, title: &folder.name, note: &note, columns: &columns, rows: &rows };
    let printed = openmailclient_layout::list(&listed, setup, &mut fonts).map_err(|e| match e {
        LayoutError::NoFont => EngineError::Format(e.to_string()),
    })?;
    Ok(Laid { links: 0, printed, pictures: Pictures::default() })
}

/// What a print of the session in `style` is called: the message's subject, or the folder's name.
pub fn title(s: &crate::Session, style: Style) -> String {
    match style {
        Style::Memo => s.open.as_ref().map(Item::subject).unwrap_or_default(),
        Style::Table => s.current_folder().map(|f| f.name.clone()).unwrap_or_default(),
    }
}

/// Lay out what the session shows in `style`, in the installed fonts: the open message, or
/// the folder's list.
pub fn laid_as(s: &crate::Session, style: Style, setup: &Setup, links: Links) -> Result<Laid, EngineError> {
    let job = job(s, setup, links);
    match style {
        Style::Memo => layout(s.open.as_ref().ok_or(StoreError::NoItem)?, &job, library()),
        Style::Table => list_layout(s, &job, library()),
    }
}

/// Print what the session shows in `style` to a PDF file's bytes, in the installed fonts.
pub fn pdf_as(s: &crate::Session, style: Style, setup: &Setup, links: Links) -> Result<Print, EngineError> {
    let Laid { links, printed, pictures } = laid_as(s, style, setup, links)?;
    Ok(Print { file: write(&printed, &pictures, &title(s, style))?, pages: printed.pages.len(), truncated: printed.truncated, links })
}

/// Write laid-out pages as a PDF file.
pub fn write(printed: &Printed, pictures: &Pictures, title: &str) -> Result<Vec<u8>, EngineError> {
    let mut writer = Writer::new();
    // Each picture goes into the file once, when a page first shows it; one the file cannot
    // carry is left out.
    let mut images: Vec<Option<Option<openmailclient_pdf::ImageId>>> = vec![None; pictures.found.len()];
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
                    kerns: &text.kerns,
                    bold: text.bold,
                    slant: text.slant,
                });
            }
        }
    }
    writer.finish(title).map_err(|e| EngineError::Format(e.to_string()))
}

/// Lay `item` out for `job` in the installed fonts, to look at or to write.
pub fn laid(item: &Item, job: &Job) -> Result<Laid, EngineError> {
    layout(item, job, library())
}

/// Page `page` (the first is 0) of what was laid out, as pixels, `scale` of them to the point.
/// It is drawn from what the PDF file is written from. `None` when there is no such page.
pub fn raster(laid: &Laid, page: usize, scale: f32) -> Option<openmailclient_render::Image> {
    let pictures =
        |picture: &Picture| laid.pictures.get(picture).map(|p| openmailclient_render::Bitmap { width: p.width, height: p.height, rgba: &p.rgba });
    openmailclient_render::page(&laid.printed, page, scale, &pictures)
}

/// A drawn page as a PNG file.
pub fn png(image: &openmailclient_render::Image) -> Result<Vec<u8>, EngineError> {
    use image::ImageEncoder;
    let (width, height) = (u32::try_from(image.width).unwrap_or(0), u32::try_from(image.height).unwrap_or(0));
    let mut file = Vec::new();
    image::codecs::png::PngEncoder::new(&mut file)
        .write_image(&image.rgb, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| EngineError::Format(e.to_string()))?;
    Ok(file)
}

/// Print `item` to a PDF file's bytes, in the installed fonts.
pub fn pdf(item: &Item, job: &Job) -> Result<Print, EngineError> {
    let Laid { links, printed, pictures } = layout(item, job, library())?;
    Ok(Print { file: write(&printed, &pictures, &item.subject())?, pages: printed.pages.len(), truncated: printed.truncated, links })
}
