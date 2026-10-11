//! Draws a message body that was read into a formatted document.
//!
//! A link shows where it points when the pointer rests on it and is never followed. A picture is
//! drawn only when the message carries it, as an attachment or written into the body, and it
//! could be decoded ([`Pictures`]); any other picture is an empty frame of the size it asks for,
//! or a labelled placeholder when it asks for none. A picture stands in the line of text it is
//! written in, or at one side of its paragraph when it asks to. Text is drawn in the font it
//! names when that font, or an open one of the same measures, is installed ([`Faces`]). Colours
//! a message asks for are kept only where they stay readable on what is behind them. Sizes follow what Outlook Classic
//! was seen to do with the same message (`docs/ui-parity.md`, "Message bodies").

use crate::faces::Faces;
use crate::pictures::{Pictures, Shown};
use crate::theme::Tokens;
use crate::{bold, font};
use egui::text::{LayoutJob, TextFormat};
use egui::text_selection::LabelSelectionState;
use egui::{Align, Color32, FontFamily, FontId, Rect, Sense, Shape, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use mailcraft_formats::rich::{self, Block, Doc, Float, Para, ParaKind, Picture, Run, Script, Table, TableWidth, VAlign};

/// Size of text the message gives no size for: 12 points.
const BODY_PX: f32 = 16.0;
/// One level of indent: a list level, a block set in, half an inch of RTF indent.
const INDENT: f32 = 40.0;
const QUOTE: f32 = 14.0;
/// More columns than this are drawn one cell under another.
const MAX_COLUMNS: usize = 12;
/// Tables nested deeper than this are drawn one cell under another.
const MAX_TABLE_DEPTH: usize = 6;
/// Largest cell padding and spacing drawn, whatever the message asks for.
const MAX_CELL_ROOM: u16 = 64;
/// A picture that is not drawn gets a frame when it asks for at least this size each way.
const MIN_FRAME: u16 = 24;
const MIN_COLUMN: f32 = 24.0;
/// Rows of a table, and blocks of a cell, that are measured to size its columns.
const MEASURED_ROWS: usize = 100;
const MEASURED_BLOCKS: usize = 200;
/// Width a column is taken to want when what it holds is not measured.
const UNMEASURED: f32 = 240.0;
/// Widest and tallest a picture is drawn, whatever it asks for.
const MAX_PICTURE_WIDTH: f32 = 4096.0;
const MAX_PICTURE_HEIGHT: f32 = 4096.0;
/// Room kept under a picture in a line of text, so that it stands on the line the letters stand
/// on and not on the line their tails reach down to.
const BASELINE: f32 = 4.0;
/// Largest font size given to the space that holds a picture's place.
const MAX_HOLDER_PX: f32 = 20_000.0;
/// Room between a picture at one side of a paragraph and the text beside it.
const FLOAT_GAP: f32 = 8.0;
/// A picture stands at one side only when this much width is left for the text beside it.
const MIN_BESIDE: f32 = 120.0;
/// Runs of a paragraph looked through for a picture that stands at one side.
const MAX_FLOAT_SEARCH: usize = 256;

/// What drawing needs besides the document: the theme, the decoded pictures, and the colour
/// behind the text being drawn (the pane, or a filled table cell).
#[derive(Clone, Copy)]
struct Look<'a> {
    t: &'a Tokens,
    pictures: &'a Pictures,
    faces: &'a Faces,
    on: Color32,
}

impl Look<'_> {
    /// The colour of text that asks for none.
    fn text(&self) -> Color32 {
        if self.on == self.t.surface { self.t.text } else { plain_on(self.on) }
    }
}

pub(crate) fn draw(ui: &mut Ui, doc: &Doc, t: &Tokens, pictures: &Pictures, faces: &Faces) {
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    blocks(ui, &doc.blocks, Look { t, pictures, faces, on: t.surface }, 0);
}

fn blocks(ui: &mut Ui, list: &[Block], look: Look, depth: usize) {
    for (i, block) in list.iter().enumerate() {
        match block {
            Block::Para(p) => para(ui, p, look, i > 0),
            Block::Rule => {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 13.0), Sense::hover());
                ui.painter().hline(rect.x_range(), rect.center().y, (1.0, look.t.text_faint));
            }
            Block::Table(table) => {
                if i > 0 {
                    ui.add_space(6.0);
                }
                self::table(ui, table, look, depth);
            }
        }
    }
}

/// What stands on the page where a run has a picture.
#[derive(Clone, Copy)]
enum Stand<'a> {
    Drawn(&'a Shown),
    /// The picture is not here to draw, but the body says how large it is.
    Frame,
}

impl Stand<'_> {
    /// The size the picture has before the body's wishes and the room are taken into account.
    fn natural(&self, run: &Run) -> Vec2 {
        match self {
            Stand::Drawn(picture) => picture.size,
            Stand::Frame => vec2(f32::from(run.extent.0), f32::from(run.extent.1)),
        }
    }
}

/// The picture a run stands for, or the frame that takes its place; `None` for text, and for a
/// picture that is shown as a line of text.
fn shown<'a>(run: &Run, pictures: &'a Pictures) -> Option<Stand<'a>> {
    let drawn = match run.picture.as_ref()? {
        Picture::Attached(id) => pictures.get(id),
        Picture::Inline(Some(index)) => pictures.written(*index),
        Picture::Inline(None) | Picture::Remote => None,
    };
    if let Some(drawn) = drawn {
        return Some(Stand::Drawn(drawn));
    }
    (run.extent.0 >= MIN_FRAME && run.extent.1 >= MIN_FRAME).then_some(Stand::Frame)
}

/// What a picture that is not drawn is called.
fn missing(picture: &Picture) -> &'static str {
    match picture {
        Picture::Remote => "Picture not downloaded",
        Picture::Attached(_) | Picture::Inline(_) => "Picture",
    }
}

/// A picture's alternative text, as it is shown.
fn alt(run: &Run) -> String {
    run.text.chars().filter(|c| !c.is_control()).take(120).collect()
}

/// The colour of notes that stand in for content.
fn faint(look: Look) -> Color32 {
    if look.on == look.t.surface { look.t.text_faint } else { look.text().gamma_multiply(0.75) }
}

/// An empty frame where a picture would be: an outline, our own mark for "no picture", and what
/// the message says the picture shows.
fn frame(ui: &Ui, run: &Run, at: Rect, look: Look) {
    let color = faint(look);
    let painter = ui.painter().with_clip_rect(at.intersect(ui.clip_rect()));
    painter.rect_stroke(at, 0.0, (1.0, color.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    // The mark: a small picture frame with a line struck through it.
    let mark = Rect::from_min_size(at.min + vec2(7.0, 7.0), vec2(14.0, 11.0));
    painter.rect_stroke(mark, 1.5, (1.0, color), egui::StrokeKind::Inside);
    painter.line_segment([mark.left_bottom() + vec2(-2.0, 2.0), mark.right_top() + vec2(2.0, -2.0)], (1.2, color));
    let what = run.picture.as_ref().map_or("Picture", missing);
    let alt = alt(run);
    let label = if alt.is_empty() { what.to_string() } else { format!("{what}\n{alt}") };
    let galley = painter.layout(label, font(11.5), color, (at.width() - 36.0).max(8.0));
    painter.galley(at.min + vec2(28.0, 5.0), galley, color);
}

fn para(ui: &mut Ui, para: &Para, look: Look, after_another: bool) {
    if para.gap && after_another {
        ui.add_space(if matches!(para.kind, ParaKind::Heading(_)) { 12.0 } else { 9.0 });
    }
    let (_, width) = room(ui, para);
    // The first picture that asks to stand at one side does, when there is room beside it.
    let aside = para.runs.iter().take(MAX_FLOAT_SEARCH).enumerate().find_map(|(i, run)| {
        let stand = (run.float != Float::None).then(|| shown(run, look.pictures)).flatten()?;
        let size = picture_size(run.extent, stand.natural(run), width);
        (width - size.x - FLOAT_GAP >= MIN_BESIDE).then_some((i, stand, size))
    });
    match aside {
        Some((at, stand, size)) => beside(ui, para, at, stand, size, look),
        None => {
            piece(ui, para, &para.runs, look, Spot { left: 0.0, width, min_height: 0.0, marker: true });
        }
    }
}

/// A paragraph with a picture at one side: the text stands beside the picture for as many lines
/// as the picture is tall, and under it from there on.
fn beside(ui: &mut Ui, para: &Para, at: usize, stand: Stand, size: Vec2, look: Look) {
    let (inset, width) = room(ui, para);
    let Some(run) = para.runs.get(at) else { return };
    let rest: Vec<Run> = para.runs.iter().enumerate().filter(|(i, _)| *i != at).map(|(_, r)| r.clone()).collect();
    let narrow = width - size.x - FLOAT_GAP;
    let left = if run.float == Float::Left { size.x + FLOAT_GAP } else { 0.0 };

    // Lay the text out beside the picture to learn where the picture ends.
    let (job, ends, _) = job(ui, &rest, look, narrow, para.align);
    let galley = ui.painter().layout_job(job);
    let mut chars = 0usize;
    let mut under = false;
    for (i, row) in galley.rows.iter().enumerate() {
        if i > 0 && row.pos.y >= size.y {
            under = true;
            break;
        }
        chars = chars.saturating_add(row.glyphs.len()).saturating_add(usize::from(row.ends_with_newline));
    }
    let rect = if under {
        let (head, tail) = split(&rest, &ends, chars);
        let rect = piece(ui, para, &head, look, Spot { left, width: narrow, min_height: size.y, marker: true });
        piece(ui, para, &tail, look, Spot { left: 0.0, width, min_height: 0.0, marker: false });
        rect
    } else {
        piece(ui, para, &rest, look, Spot { left, width: narrow, min_height: size.y + FLOAT_GAP / 2.0, marker: true })
    };

    let x = rect.left() + inset + if run.float == Float::Right { width - size.x } else { 0.0 };
    let spot = Rect::from_min_size(pos2(x.round(), rect.top().round()), size);
    if !ui.is_rect_visible(spot) {
        return;
    }
    picture(ui, run, stand, spot, look);
    let id = ui.id().with(("aside", at, spot.min.x.to_bits(), spot.min.y.to_bits()));
    let tip = tip(run, stand);
    let response = ui.interact(spot, id, Sense::hover());
    if !tip.is_empty() {
        response.on_hover_text_at_pointer(tip);
    }
}

/// Runs cut in two after `chars` characters of the text they are drawn as; `ends` is where each
/// run ends in that text. Only plain text is cut; a picture or its label stays whole.
pub(crate) fn split(runs: &[Run], ends: &[usize], chars: usize) -> (Vec<Run>, Vec<Run>) {
    let (mut head, mut tail): (Vec<Run>, Vec<Run>) = (Vec::new(), Vec::new());
    let mut start = 0usize;
    for (run, end) in runs.iter().zip(ends) {
        if *end <= chars {
            head.push(run.clone());
        } else if start >= chars {
            tail.push(run.clone());
        } else if run.picture.is_some() {
            head.push(run.clone());
        } else {
            let cut = run.text.char_indices().nth(chars - start).map_or(run.text.len(), |(i, _)| i);
            let (a, b) = run.text.split_at_checked(cut).unwrap_or((run.text.as_str(), ""));
            head.push(Run { text: a.to_string(), ..run.clone() });
            tail.push(Run { text: b.to_string(), ..run.clone() });
        }
        start = *end;
    }
    // The line break that ended the last line beside the picture is not drawn twice.
    if let Some(last) = head.last_mut()
        && last.picture.is_none()
        && last.text.ends_with('\n')
    {
        last.text.pop();
    }
    (head, tail)
}

/// Room a paragraph leaves on its left.
fn inset(para: &Para) -> f32 {
    f32::from(para.indent.min(12)) * INDENT + f32::from(para.quote.min(8)) * QUOTE
}

/// Room a paragraph leaves on its left here, and the width left for what it holds.
fn room(ui: &Ui, para: &Para) -> (f32, f32) {
    let inset = inset(para).min(ui.available_width() * 0.6);
    (inset, (ui.available_width() - inset).max(40.0))
}

fn quote_bars(ui: &Ui, para: &Para, rect: Rect, t: &Tokens) {
    for level in 0..para.quote.min(8) {
        let x = rect.left() + f32::from(level) * QUOTE + 4.0;
        ui.painter().vline(x, rect.y_range(), (2.0, t.line.lerp_to_gamma(t.text_faint, 0.6)));
    }
}

/// The size a picture is drawn at: what the body asks for, else its own, never wider than `room`.
pub(crate) fn picture_size(asked: (u16, u16), natural: Vec2, room: f32) -> Vec2 {
    let natural = vec2(natural.x.max(1.0), natural.y.max(1.0));
    let (w, h) = (f32::from(asked.0), f32::from(asked.1));
    let mut size = match (w > 0.0, h > 0.0) {
        (true, true) => vec2(w, h),
        (true, false) => vec2(w, natural.y * w / natural.x),
        (false, true) => vec2(natural.x * h / natural.y, h),
        (false, false) => natural,
    };
    if size.x > room {
        size *= room.max(1.0) / size.x;
    }
    vec2(size.x.clamp(1.0, MAX_PICTURE_WIDTH), size.y.clamp(1.0, MAX_PICTURE_HEIGHT))
}

/// Draw a picture, or the frame that takes its place.
fn picture(ui: &Ui, run: &Run, stand: Stand, at: Rect, look: Look) {
    match stand {
        Stand::Drawn(drawn) => {
            ui.painter().image(drawn.texture.id(), at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        Stand::Frame => frame(ui, run, at, look),
    }
}

/// What is said about a picture when the pointer rests on it.
fn tip(run: &Run, stand: Stand) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let (Stand::Frame, Some(picture)) = (stand, &run.picture) {
        lines.push(missing(picture).to_string());
    }
    lines.push(alt(run));
    if let Some(link) = run.style.link.as_deref() {
        lines.push(link.chars().take(300).collect());
        lines.push("Links are not opened from here.".to_string());
    }
    lines.retain(|line| !line.is_empty());
    lines.join("\n")
}

/// A picture that stands in a line of text, and the size it is drawn at.
struct Placed<'a> {
    run: &'a Run,
    stand: Stand<'a>,
    size: Vec2,
    /// The character that holds its place in the text, and the line height given to it.
    holder: (char, f32),
}

/// What holds a picture's place in a line of text: a space set so large that it is as wide as
/// the picture, on a line as tall as the picture. `None` when the fonts have no such space.
fn holder(ui: &Ui, size: Vec2) -> Option<(char, TextFormat)> {
    const PROBE: f32 = 64.0;
    let probe = FontId::new(PROBE, FontFamily::Proportional);
    let (space, px) = ui.ctx().fonts_mut(|fonts| {
        // An em space is as wide as its font is tall; a plain space does where there is none.
        [('\u{2003}', true), (' ', false)].into_iter().find_map(|(c, check)| {
            if check && !fonts.has_glyph(&probe, c) {
                return None;
            }
            let advance = fonts.glyph_width(&probe, c);
            (advance.is_finite() && advance > 1.0).then(|| (c, size.x * PROBE / advance))
        })
    })?;
    let format = TextFormat {
        font_id: FontId::new(px.clamp(1.0, MAX_HOLDER_PX), FontFamily::Proportional),
        line_height: Some(size.y + BASELINE),
        color: Color32::TRANSPARENT,
        valign: Align::BOTTOM,
        ..TextFormat::default()
    };
    Some((space, format))
}

/// Runs laid out as one piece of text no wider than `wrap`; the character offset where each run
/// ends; and the pictures that stand in the text, in order.
fn job<'a>(ui: &Ui, runs: &'a [Run], look: Look<'a>, wrap: f32, align: rich::Align) -> (LayoutJob, Vec<usize>, Vec<Placed<'a>>) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    job.halign = match align {
        rich::Align::Left => Align::LEFT,
        rich::Align::Center => Align::Center,
        rich::Align::Right => Align::RIGHT,
    };
    let mut ends: Vec<usize> = Vec::with_capacity(runs.len());
    let mut placed: Vec<Placed> = Vec::new();
    let mut chars = 0usize;
    for run in runs {
        let held = shown(run, look.pictures).and_then(|stand| {
            let size = picture_size(run.extent, stand.natural(run), wrap);
            holder(ui, size).map(|(space, format)| (stand, size, space, format))
        });
        let (text, format) = match held {
            Some((stand, size, space, format)) => {
                placed.push(Placed { run, stand, size, holder: (space, size.y + BASELINE) });
                (space.to_string(), format)
            }
            None => section(run, look),
        };
        chars = chars.saturating_add(text.chars().count());
        ends.push(chars);
        job.append(&text, 0.0, format);
    }
    (job, ends, placed)
}

/// Where each picture of `placed` landed in the laid out text, from the text's own corner.
fn spots(galley: &egui::Galley, placed: &[Placed]) -> Vec<Rect> {
    let mut out = Vec::with_capacity(placed.len());
    let mut next = placed.iter();
    let mut want = next.next();
    'rows: for row in &galley.rows {
        for glyph in &row.glyphs {
            let Some(p) = want else { break 'rows };
            // Pictures come in the order of the text, each held by its space on its own tall line.
            if glyph.chr == p.holder.0 && (glyph.line_height - p.holder.1).abs() < 0.75 {
                let bottom = row.pos.y + row.size.y - BASELINE;
                out.push(Rect::from_min_size(pos2((row.pos.x + glyph.pos.x).round(), (bottom - p.size.y).round()), p.size));
                want = next.next();
            }
        }
    }
    out
}

/// Where a piece of a paragraph's text goes in the room the paragraph has.
struct Spot {
    /// Room left free before the text, for a picture at the paragraph's left.
    left: f32,
    width: f32,
    min_height: f32,
    /// Draw the list marker beside it.
    marker: bool,
}

/// Runs of one paragraph drawn as text, with the pictures that stand in it. Returns the room
/// it took.
fn piece(ui: &mut Ui, para: &Para, runs: &[Run], look: Look, spot: Spot) -> Rect {
    let (inset, _) = room(ui, para);
    let (job, ends, placed) = job(ui, runs, look, spot.width, para.align);
    let galley = ui.painter().layout_job(job);

    let height = (galley.size().y + 2.0).max(spot.min_height);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag() - Sense::FOCUSABLE);
    if !ui.is_rect_visible(rect) {
        return rect;
    }
    let text_left = rect.left() + inset + spot.left;
    let pos = match para.align {
        rich::Align::Left => pos2(text_left, rect.top()),
        rich::Align::Center => pos2(text_left + spot.width / 2.0, rect.top()),
        rich::Align::Right => pos2(text_left + spot.width, rect.top()),
    };
    quote_bars(ui, para, rect, look.t);
    if spot.marker
        && let ParaKind::Item(marker) = &para.kind
    {
        let marker: String = marker.chars().take(12).collect();
        ui.painter().text(pos2(rect.left() + inset - 7.0, rect.top()), egui::Align2::RIGHT_TOP, marker, font(BODY_PX), look.text());
    }

    let spots: Vec<Rect> = spots(&galley, &placed).into_iter().map(|at| at.translate(pos.to_vec2())).collect();
    // Where the pointer is: on a picture, or in the text, where a link shows its address and
    // nothing else happens.
    if let Some(pointer) = response.hover_pos() {
        if let Some((p, _)) = placed.iter().zip(&spots).find(|(_, at)| at.contains(pointer)) {
            let tip = tip(p.run, p.stand);
            if !tip.is_empty() {
                response.clone().on_hover_text_at_pointer(tip);
            }
        } else {
            let cursor = galley.cursor_from_pos(pointer - pos);
            let at = galley.pos_from_cursor(cursor).translate(pos.to_vec2());
            if (pointer.x - at.center().x).abs() < 16.0 && at.y_range().contains(pointer.y) {
                let index = cursor.index.0;
                let run = ends.iter().position(|end| index < *end).and_then(|i| runs.get(i));
                if let Some(link) = run.and_then(|r| r.style.link.as_deref()) {
                    let shown: String = link.chars().take(300).collect();
                    response.clone().on_hover_text_at_pointer(format!("{shown}\nLinks are not opened from here."));
                }
            }
        }
    }
    LabelSelectionState::label_text_selection(ui, &response, pos, galley, look.text(), Stroke::NONE);
    for (p, at) in placed.iter().zip(&spots) {
        if ui.is_rect_visible(*at) {
            picture(ui, p.run, p.stand, *at, look);
        }
    }
    rect
}

/// The text and format of one run as it is drawn.
fn section(run: &Run, look: Look) -> (String, TextFormat) {
    let s = &run.style;
    let mut px = s.size.map_or(BODY_PX, |pt| pt * 4.0 / 3.0).clamp(8.0, 64.0);
    let mut format = TextFormat::default();
    match s.script {
        Script::None => {}
        Script::Super => {
            px *= 0.75;
            format.valign = Align::TOP;
        }
        Script::Sub => {
            px *= 0.75;
            format.valign = Align::BOTTOM;
        }
    }
    // The font the run names, when it is installed; otherwise the reader's own.
    let named = s.family.as_deref().and_then(|list| look.faces.pick(list, s.bold));
    format.font_id = match (named, s.mono, s.bold) {
        (Some(family), ..) => FontId::new(px, family),
        (None, true, _) => FontId::new(px, FontFamily::Monospace),
        (None, false, true) => bold(px),
        (None, false, false) => font(px),
    };
    format.italics = s.italic;
    let (color, background) = colors(s.color, s.background, look);
    format.color = color;
    format.background = background;
    if let Some(picture) = &run.picture {
        let what = missing(picture);
        let alt = alt(run);
        format.color = faint(look);
        format.background = Color32::TRANSPARENT;
        format.font_id = font(px.min(13.5));
        return (if alt.is_empty() { format!("[{what}]") } else { format!("[{what}: {alt}]") }, format);
    }
    if s.link.is_some() {
        if s.color.is_none() && s.background.is_none() && contrast(look.t.accent_dark, look.on) >= 3.0 {
            format.color = look.t.accent_dark;
        }
        format.underline = Stroke::new(1.0, format.color);
    } else if s.underline {
        format.underline = Stroke::new(1.0, format.color);
    }
    if s.strike {
        format.strikethrough = Stroke::new(1.0, format.color);
    }
    (run.text.clone(), format)
}

/// Text that asks for no colour, on a colour the message chose.
fn plain_on(background: Color32) -> Color32 {
    if luminance(background) > 0.4 { Color32::from_rgb(0x1A, 0x1A, 0x1A) } else { Color32::WHITE }
}

/// Text and highlight colours for a run. A message is written for a white page; here a colour is
/// kept only if it can be read on what is behind it, otherwise it is moved toward the plain text
/// colour there.
fn colors(color: Option<rich::Rgb>, background: Option<rich::Rgb>, look: Look) -> (Color32, Color32) {
    let rgb = |c: rich::Rgb| Color32::from_rgb(c[0], c[1], c[2]);
    match background.map(rgb) {
        Some(bg) => {
            let fg = color.map(rgb).filter(|c| contrast(*c, bg) >= 2.0).unwrap_or(plain_on(bg));
            (fg, bg)
        }
        None => {
            let plain = look.text();
            let Some(c) = color.map(rgb) else { return (plain, Color32::TRANSPARENT) };
            let readable =
                [0.0, 0.4, 0.7, 1.0].into_iter().map(|k| c.lerp_to_gamma(plain, k)).find(|c| contrast(*c, look.on) >= 3.0).unwrap_or(plain);
            (readable, Color32::TRANSPARENT)
        }
    }
}

/// Relative luminance and contrast ratio (WCAG 2.1, "relative luminance" and "contrast ratio").
fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The width blocks would take with all the room they could want.
fn natural_width(ui: &Ui, list: &[Block], look: Look, depth: usize) -> f32 {
    let mut widest = 0.0f32;
    for block in list.iter().take(MEASURED_BLOCKS) {
        let width = match block {
            Block::Para(p) => {
                let (job, ..) = job(ui, &p.runs, look, f32::INFINITY, rich::Align::Left);
                inset(p) + ui.painter().layout_job(job).size().x + 2.0
            }
            Block::Rule => 0.0,
            Block::Table(t) => match t.width {
                TableWidth::Pixels(w) => f32::from(w),
                TableWidth::Percent(_) | TableWidth::Fill => UNMEASURED,
                _ if depth < MAX_TABLE_DEPTH && t.columns() <= MAX_COLUMNS => {
                    let columns = natural_columns(ui, t, look, depth);
                    columns.iter().sum::<f32>() + spacing(t) * (columns.len() as f32 + 1.0)
                }
                _ => UNMEASURED,
            },
        };
        if width.is_finite() {
            widest = widest.max(width);
        }
    }
    widest
}

/// Room between a cell's edges and what it holds: left and right, then above and below.
fn padding(table: &Table, cell: &rich::Cell) -> Vec2 {
    let (vertical, horizontal) = cell.padding.unwrap_or((table.padding, table.padding));
    vec2(f32::from(horizontal.min(MAX_CELL_ROOM)), f32::from(vertical.min(MAX_CELL_ROOM)))
}

/// Room between the cells of a table.
fn spacing(table: &Table) -> f32 {
    f32::from(table.spacing.min(MAX_CELL_ROOM))
}

/// Give the columns that ask for a share of the table's width that share of `room`, and the
/// room that is left to the others.
fn shares(table: &Table, widths: &mut [f32], room: f32) {
    let mut asked = vec![0.0f32; widths.len()];
    for row in table.rows.iter().take(MEASURED_ROWS) {
        let mut col = 0usize;
        for cell in row {
            if cell.span <= 1
                && let Some(a) = asked.get_mut(col)
            {
                *a = a.max(f32::from(cell.share.min(100)) / 100.0);
            }
            col = col.saturating_add(usize::from(cell.span.max(1)));
        }
    }
    let claimed: f32 = asked.iter().sum();
    if claimed <= 0.0 {
        return;
    }
    // Shares that add up to more than everything are cut down together.
    let scale = if claimed > 1.0 { 1.0 / claimed } else { 1.0 };
    let others: f32 = widths.iter().zip(&asked).filter(|(_, a)| **a <= 0.0).map(|(w, _)| *w).sum();
    let left = (room - room * claimed * scale).max(0.0);
    for (w, a) in widths.iter_mut().zip(&asked) {
        if *a > 0.0 {
            *w = room * a * scale;
        } else if others > 0.0 {
            *w *= left / others;
        }
    }
    // With no other column to take what is left, the sharing columns take it.
    let total: f32 = widths.iter().sum();
    if total > 0.0 && (others <= 0.0 || left <= 0.0) {
        widths.iter_mut().for_each(|w| *w *= room / total);
    }
}

/// The width each column asks for, or else wants for what it holds.
fn natural_columns(ui: &Ui, table: &Table, look: Look, depth: usize) -> Vec<f32> {
    let columns = table.columns();
    let mut asked = vec![0.0f32; columns];
    let mut wanted = vec![MIN_COLUMN; columns];
    let mut wide: Vec<(usize, usize, f32)> = Vec::new();
    for row in table.rows.iter().take(MEASURED_ROWS) {
        let mut col = 0usize;
        for cell in row {
            let span = usize::from(cell.span.max(1));
            let on = cell.background.map_or(look.on, |c| Color32::from_rgb(c[0], c[1], c[2]));
            let want = natural_width(ui, &cell.blocks, Look { on, ..look }, depth + 1) + padding(table, cell).x * 2.0;
            if span == 1 {
                if let Some(a) = asked.get_mut(col) {
                    *a = a.max(f32::from(cell.width));
                }
                if let Some(w) = wanted.get_mut(col) {
                    *w = w.max(want);
                }
            } else {
                wide.push((col, span, want.max(f32::from(cell.width))));
            }
            col = col.saturating_add(span);
        }
    }
    if table.rows.len() > MEASURED_ROWS {
        wanted.iter_mut().for_each(|w| *w = w.max(UNMEASURED));
    }
    let mut widths: Vec<f32> = asked.iter().zip(&wanted).map(|(a, w)| if *a > 0.0 { a.max(MIN_COLUMN) } else { *w }).collect();
    // A cell over several columns that wants more than they have widens the last of them.
    for (col, span, want) in wide {
        let have: f32 = widths.iter().skip(col).take(span).sum();
        if want > have
            && let Some(last) = widths.get_mut(col.saturating_add(span).saturating_sub(1))
        {
            *last += want - have;
        }
    }
    widths
}

fn table(ui: &mut Ui, table: &Table, look: Look, depth: usize) {
    let columns = table.columns();
    let available = ui.available_width();
    if columns == 0 || columns > MAX_COLUMNS || depth >= MAX_TABLE_DEPTH || available / (columns as f32) < 48.0 {
        // Too many columns or too little room: one cell under another, in reading order.
        for cell in table.rows.iter().flatten() {
            blocks(ui, &cell.blocks, look, depth + 1);
        }
        return;
    }
    // A table is as wide as it asks to be, or as what it holds, and never wider than the room.
    // Its cells stand apart by the table's spacing, which also runs round the outside.
    let mut widths = natural_columns(ui, table, look, depth);
    let total: f32 = widths.iter().sum();
    let gaps = columns as f32 + 1.0;
    let target = match table.width {
        TableWidth::Pixels(w) => f32::from(w).min(available),
        TableWidth::Percent(share) => available * f32::from(share.clamp(1, 100)) / 100.0,
        TableWidth::Fill => available,
        TableWidth::Content => (total + spacing(table) * gaps).min(available),
    };
    // Spacing never takes more than half the table.
    let gap = spacing(table).min(target / gaps / 2.0).max(0.0);
    let cells = (target - gap * gaps).max(0.0);
    if total > 0.0 && total.is_finite() {
        widths.iter_mut().for_each(|w| *w *= cells / total);
    } else {
        widths = vec![cells / columns as f32; columns];
    }
    if table.width != TableWidth::Content {
        shares(table, &mut widths, cells);
    }
    let shift = match table.align {
        rich::Align::Left => 0.0,
        rich::Align::Center => ((available - target) / 2.0).max(0.0),
        rich::Align::Right => (available - target).max(0.0),
    };

    let left = ui.cursor().left();
    let table_top = ui.cursor().top();
    ui.add_space(gap);
    for (r, row) in table.rows.iter().enumerate() {
        let top = ui.cursor().top();
        // Where each cell stands: its number, left edge, width and padding.
        let mut placed: Vec<(usize, &rich::Cell, f32, f32, Vec2)> = Vec::with_capacity(row.len());
        let mut x = left + shift + gap;
        let mut col = 0usize;
        for (c, cell) in row.iter().enumerate() {
            let span = usize::from(cell.span.max(1));
            let covered = widths.iter().skip(col).take(span);
            let w: f32 = covered.clone().sum::<f32>() + gap * (covered.count().max(1) - 1) as f32;
            col = col.saturating_add(span);
            if w <= 0.0 {
                continue;
            }
            placed.push((c, cell, x, w, padding(table, cell)));
            x += w + gap;
        }
        let on = |cell: &rich::Cell| cell.background.map_or(look.on, |c| Color32::from_rgb(c[0], c[1], c[2]));
        let inside =
            |x: f32, w: f32, pad: Vec2, down: f32| Rect::from_min_size(pos2(x + pad.x, top + pad.y + down), vec2((w - pad.x * 2.0).max(8.0), 0.0));
        // Cells that do not start at the top are laid out once unseen, to learn how tall each is.
        // Inside such an unseen pass nothing is seen anyway, so nothing is measured again.
        let settle = ui.is_visible() && placed.len() > 1 && placed.iter().any(|(_, cell, ..)| cell.valign != VAlign::Top);
        let mut tall: Vec<f32> = Vec::new();
        if settle {
            for (c, cell, x, w, pad) in &placed {
                let mut child = ui.new_child(UiBuilder::new().id_salt(("cell size", r, *c)).max_rect(inside(*x, *w, *pad, 0.0)).invisible());
                child.spacing_mut().item_spacing = vec2(0.0, 0.0);
                blocks(&mut child, &cell.blocks, Look { on: on(cell), ..look }, depth + 1);
                tall.push(child.min_rect().height() + pad.y * 2.0);
            }
        }
        let tallest = tall.iter().copied().fold(0.0f32, f32::max);
        let mut bottom = top + tallest;
        // Each cell's left edge and width, and where its fill goes once the row's height is known.
        let mut edges: Vec<(f32, f32, Option<(egui::layers::ShapeIdx, Color32)>)> = Vec::with_capacity(row.len());
        for (i, (c, cell, x, w, pad)) in placed.iter().enumerate() {
            let spare = tall.get(i).map_or(0.0, |own| (tallest - own).max(0.0));
            let down = match cell.valign {
                VAlign::Top => 0.0,
                VAlign::Middle => (spare / 2.0).floor(),
                VAlign::Bottom => spare,
            };
            let fill = cell.background.map(|c| (ui.painter().add(Shape::Noop), Color32::from_rgb(c[0], c[1], c[2])));
            let mut child = ui.new_child(UiBuilder::new().id_salt(("cell", r, *c)).max_rect(inside(*x, *w, *pad, down)));
            child.spacing_mut().item_spacing = vec2(0.0, 0.0);
            blocks(&mut child, &cell.blocks, Look { on: on(cell), ..look }, depth + 1);
            bottom = bottom.max(child.min_rect().bottom() + pad.y);
            edges.push((*x, *w, fill));
        }
        ui.allocate_rect(Rect::from_min_max(pos2(left, top), pos2(left + available, bottom + gap)), Sense::hover());
        for (x, w, fill) in edges {
            let cell = Rect::from_min_max(pos2(x, top), pos2(x + w, bottom));
            if let Some((slot, color)) = fill {
                ui.painter().set(slot, Shape::rect_filled(cell, 0.0, color));
            }
            if table.border {
                ui.painter().rect_stroke(cell, 0.0, (1.0, look.t.text_faint), egui::StrokeKind::Middle);
            }
        }
    }
    // Cells that stand apart leave the table's own border to be seen round them.
    if table.border && gap > 0.0 {
        let outer = Rect::from_min_max(pos2(left + shift, table_top), pos2(left + shift + target, ui.cursor().top()));
        ui.painter().rect_stroke(outer, 0.0, (1.0, look.t.text_faint), egui::StrokeKind::Middle);
    }
}
