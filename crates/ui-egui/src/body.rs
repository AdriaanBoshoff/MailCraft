//! Draws a message body that was read into a formatted document.
//!
//! A link shows where it points when the pointer rests on it and is never followed. A picture is
//! drawn only when the message carries it, as an attachment or written into the body, and it
//! could be decoded ([`Pictures`]); any other picture is an empty frame of the size it asks for,
//! or a labelled placeholder when it asks for none. A picture stands in the line of text it is
//! written in, or at one side when it asks to, with what follows beside it until it ends. An
//! attachment that stands in the text is a small card there with our own mark, its name and its
//! size; a click on it asks where to save it, and nothing is opened or run. A
//! border a style asks for is drawn round its paragraphs, its table or its cell. Text is drawn in the font it
//! names when that font, or an open one of the same measures, is installed ([`Faces`]). Colours
//! a message asks for are kept only where they stay readable on what is behind them. Sizes follow what Outlook Classic
//! was seen to do with the same message (`docs/ui-parity.md`, "Message bodies").

use crate::faces::Faces;
use crate::icons::{self, Icon};
use crate::pictures::{Pictures, Shown};
use crate::theme::Tokens;
use crate::{bold, clip_text, font, text_width};
use egui::text::{LayoutJob, TextFormat};
use egui::text_selection::LabelSelectionState;
use egui::{Align, Color32, FontFamily, FontId, Rect, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use mailcraft_formats::rich::{
    self, Block, Doc, Edge, Edges, Filed, Float, Frame, Line, Para, ParaKind, Picture, Run, Script, Table, TableWidth, VAlign, any_edge,
};
use std::cell::Cell;
use std::sync::Arc;

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
/// Widest border line drawn, and most dashes drawn along one side of a border.
const MAX_EDGE: u8 = 12;
const MAX_DASHES: usize = 2048;
/// Tabs of one paragraph whose room is filled with marks.
const MAX_LEADERS: usize = 64;
/// A border colour this close to what is behind it is drawn in the reader's own.
const EDGE_CONTRAST: f32 = 1.3;
/// The card of an attachment that stands in the text: how tall it is, the room its mark and its
/// edges take beside the words, and the narrowest it is drawn.
const CARD_TALL: f32 = 22.0;
const CARD_ROOM: f32 = 32.0;
const MIN_CARD: f32 = 56.0;

/// What drawing needs besides the document: the theme, the decoded pictures, and the colour
/// behind the text being drawn (the pane, or a filled table cell).
#[derive(Clone, Copy)]
struct Look<'a> {
    t: &'a Tokens,
    pictures: &'a Pictures,
    faces: &'a Faces,
    on: Color32,
    /// The attachment a click in the text asked for, by its number in the message.
    picked: &'a Cell<Option<usize>>,
}

impl Look<'_> {
    /// The colour of text that asks for none.
    fn text(&self) -> Color32 {
        if self.on == self.t.surface { self.t.text } else { plain_on(self.on) }
    }
}

/// Draw the body. Returns the number of an attachment standing in the text that was clicked:
/// the reader asks for it to be saved. Nothing is opened from here.
pub(crate) fn draw(ui: &mut Ui, doc: &Doc, t: &Tokens, pictures: &Pictures, faces: &Faces) -> Option<usize> {
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let picked = Cell::new(None);
    blocks(ui, &doc.blocks, Look { t, pictures, faces, on: t.surface, picked: &picked }, 0);
    picked.get()
}

/// A picture standing at one side, with room beside it still to fill.
#[derive(Clone, Copy)]
struct Aside {
    left: bool,
    /// Room the picture takes at its side, with the gap to what stands beside it.
    width: f32,
    bottom: f32,
}

/// Paragraphs that follow one another inside one border: the border and how many they are.
type Group<'a> = Option<(&'a Arc<Frame>, usize)>;

fn blocks(ui: &mut Ui, list: &[Block], look: Look, depth: usize) {
    let mut aside: Option<Aside> = None;
    let mut i = 0usize;
    while let Some(block) = list.get(i) {
        let group: Group = match block {
            Block::Para(p) => p.frame.as_ref().map(|frame| {
                let same = |b: &Block| matches!(b, Block::Para(q) if q.frame.as_ref().is_some_and(|f| Arc::ptr_eq(f, frame)));
                (frame, 1 + list.get(i + 1..).unwrap_or_default().iter().take_while(|b| same(b)).count())
            }),
            _ => None,
        };
        // A picture at the side ends where it ends, or where a paragraph asks to start below it.
        if let Some(a) = aside
            && (ui.cursor().top() >= a.bottom || matches!(block, Block::Para(p) if p.clear))
        {
            ui.add_space((a.bottom - ui.cursor().top()).max(0.0));
            aside = None;
        }
        match aside {
            None => aside = one(ui, list, i, group, look, depth, None).0,
            Some(a) => {
                // What follows the picture's paragraph stands beside the picture too.
                let top = ui.cursor().top();
                let full = ui.available_rect_before_wrap();
                let (left, right) = if a.left { (full.left() + a.width, full.right()) } else { (full.left(), full.right() - a.width) };
                let room = Rect::from_min_max(pos2(left, top), pos2(right.max(left + 8.0), top));
                let mut child = ui.new_child(UiBuilder::new().id_salt(("beside", i)).max_rect(room));
                child.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let (_, under) = one(&mut child, list, i, group, look, depth, Some(a.bottom));
                ui.allocate_rect(Rect::from_min_max(pos2(full.left(), top), pos2(full.right(), child.min_rect().bottom().max(top))), Sense::hover());
                // The lines of a paragraph that start below the picture go under it.
                if let (Some(under), Block::Para(p)) = (under, block) {
                    let (_, width) = self::room(ui, p);
                    piece(ui, p, &under, look, Spot { left: 0.0, width, min_height: 0.0, marker: false });
                    aside = None;
                }
            }
        }
        i += group.map_or(1, |(_, count)| count.max(1));
    }
    if let Some(a) = aside {
        ui.add_space((a.bottom - ui.cursor().top()).max(0.0));
    }
}

/// Draw the block at `i`, or the paragraphs that share its border. `until` is where a picture
/// standing at the side ends. Returns a picture this block leaves standing, and the runs of a
/// paragraph that go under the picture it stood beside.
fn one(ui: &mut Ui, list: &[Block], i: usize, group: Group, look: Look, depth: usize, until: Option<f32>) -> (Option<Aside>, Option<Vec<Run>>) {
    match (list.get(i), group) {
        (Some(Block::Para(_)), Some((frame, count))) => {
            framed(ui, list.get(i..i.saturating_add(count)).unwrap_or_default(), frame, look, i);
            (None, None)
        }
        (Some(Block::Para(p)), None) => para(ui, p, look, i > 0, until),
        (Some(Block::Rule), _) => {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 13.0), Sense::hover());
            ui.painter().hline(rect.x_range(), rect.center().y, (1.0, look.t.text_faint));
            (None, None)
        }
        (Some(Block::Table(table)), _) => {
            if i > 0 && !table.snug {
                ui.add_space(6.0);
            }
            self::table(ui, table, look, depth);
            (None, None)
        }
        (None, _) => (None, None),
    }
}

/// Room a border takes inside its box: left and top, then right and bottom.
fn frame_room(frame: &Frame) -> (Vec2, Vec2) {
    let side = |i: usize| frame.edges.get(i).map_or(0.0, |e| f32::from(e.width.min(MAX_EDGE)));
    let [above, right, below, left] = frame.padding.map(|v| f32::from(v.min(MAX_CELL_ROOM)));
    // Text does not touch a line beside it.
    let clear = |w: f32, pad: f32| if w > 0.0 { w + pad.max(2.0) } else { pad };
    (vec2(clear(side(3), left), side(0) + above), vec2(clear(side(1), right), side(2) + below))
}

/// Paragraphs inside one border.
fn framed(ui: &mut Ui, paras: &[Block], frame: &Frame, look: Look, at: usize) {
    if let (true, Some(Block::Para(p))) = (at > 0, paras.first()) {
        ui.add_space(f32::from(p.lead()));
    }
    let top = ui.cursor().top();
    let mut full = ui.available_rect_before_wrap();
    full.min.x += (f32::from(frame.indent.min(12)) * INDENT).min(full.width() * 0.6);
    let (near, far) = frame_room(frame);
    let left = (full.left() + near.x).min(full.right());
    let inside = Rect::from_min_max(pos2(left, top + near.y), pos2((full.right() - far.x).max(left + 8.0), top + near.y));
    // The fill goes behind what the box holds, and what it holds is read against the fill.
    let behind = ui.painter().add(egui::Shape::Noop);
    let fill = frame.fill.map(|c| Color32::from_rgb(c[0], c[1], c[2]));
    let look = Look { on: fill.unwrap_or(look.on), ..look };
    let mut child = ui.new_child(UiBuilder::new().id_salt(("framed", at)).max_rect(inside));
    child.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let between = f32::from(frame.between.width.min(MAX_EDGE));
    for (k, block) in paras.iter().enumerate() {
        if let Block::Para(p) = block {
            // The line between one paragraph and the next, with the room the border keeps.
            if k > 0 && between > 0.0 {
                let [above, _, below, _] = frame.padding.map(|v| f32::from(v.min(MAX_CELL_ROOM)));
                child.add_space(below);
                let y = child.cursor().top();
                let line = Rect::from_min_max(pos2(full.left(), y), pos2(full.right(), y + between));
                child.add_space(between + above);
                let only = [frame.between, Edge::default(), Edge::default(), Edge::default()];
                edges(ui, line, &only, look, look.text());
            }
            // A picture at the side stays inside the border.
            if let (Some(aside), _) = para(&mut child, p, look, k > 0 && between == 0.0, None) {
                child.add_space((aside.bottom - child.cursor().top()).max(0.0));
            }
        }
    }
    let rect = Rect::from_min_max(pos2(full.left(), top), pos2(full.right(), child.min_rect().bottom().max(inside.top()) + far.y));
    ui.allocate_rect(rect, Sense::hover());
    if let Some(fill) = fill {
        ui.painter().set(behind, egui::Shape::rect_filled(rect, 0.0, fill));
    }
    edges(ui, rect, &frame.edges, look, look.text());
}

/// The lines of a border, drawn just inside `rect`. A side that names no colour, or one that
/// would not be seen on what is behind it, is drawn in `unset`.
fn edges(ui: &Ui, rect: Rect, edges: &Edges, look: Look, unset: Color32) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let seen = ui.clip_rect();
    let painter = ui.painter();
    for (side, edge) in edges.iter().enumerate() {
        let w = f32::from(edge.width.min(MAX_EDGE)).min(rect.width()).min(rect.height());
        if w <= 0.0 {
            continue;
        }
        let color = edge.color.map(|c| Color32::from_rgb(c[0], c[1], c[2])).filter(|c| contrast(*c, look.on) >= EDGE_CONTRAST).unwrap_or(unset);
        // The strip the line fills, and whether it runs across.
        let (strip, across) = match side {
            0 => (Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + w)), true),
            1 => (Rect::from_min_max(pos2(rect.right() - w, rect.top()), rect.max), false),
            2 => (Rect::from_min_max(pos2(rect.left(), rect.bottom() - w), rect.max), true),
            _ => (Rect::from_min_max(rect.min, pos2(rect.left() + w, rect.bottom())), false),
        };
        match edge.line {
            Line::Double if w >= 3.0 => {
                // Two lines with as much room between them.
                let thin = (w / 3.0).floor().max(1.0);
                let (first, second) = if across {
                    (
                        Rect::from_min_max(strip.min, pos2(strip.right(), strip.top() + thin)),
                        Rect::from_min_max(pos2(strip.left(), strip.bottom() - thin), strip.max),
                    )
                } else {
                    (
                        Rect::from_min_max(strip.min, pos2(strip.left() + thin, strip.bottom())),
                        Rect::from_min_max(pos2(strip.right() - thin, strip.top()), strip.max),
                    )
                };
                painter.rect_filled(first, 0.0, color);
                painter.rect_filled(second, 0.0, color);
            }
            Line::Dashed | Line::Dotted => {
                // A mark one pixel square is too small to be drawn: dots are two long at the least.
                let mark = if edge.line == Line::Dashed { w * 3.0 } else { w.max(2.0) };
                let step = mark * 2.0;
                let (start, end) = if across { (strip.left(), strip.right()) } else { (strip.top(), strip.bottom()) };
                let (from, to) = if across { (seen.left(), seen.right()) } else { (seen.top(), seen.bottom()) };
                // Only the marks that can be seen, keeping their places along the line.
                let skipped = ((from - start) / step).floor().max(0.0);
                // Marks start on a whole pixel, or the smallest of them fade away.
                let mut at = (start + skipped * step).round();
                let mut drawn = 0usize;
                while at < end.min(to) && drawn < MAX_DASHES && step > 0.0 {
                    let upto = (at + mark).min(end);
                    let dash = if across {
                        Rect::from_min_max(pos2(at, strip.top()), pos2(upto, strip.bottom()))
                    } else {
                        Rect::from_min_max(pos2(strip.left(), at), pos2(strip.right(), upto))
                    };
                    painter.rect_filled(dash, 0.0, color);
                    at += step;
                    drawn += 1;
                }
            }
            Line::Solid | Line::Double => {
                painter.rect_filled(strip, 0.0, color);
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
    /// An attachment that stands in the text as a file.
    File(&'a Filed),
}

impl Stand<'_> {
    /// The size the picture has before the body's wishes and the room are taken into account.
    fn natural(&self, run: &Run) -> Vec2 {
        match self {
            Stand::Drawn(picture) => picture.size,
            Stand::Frame => vec2(f32::from(run.extent.0), f32::from(run.extent.1)),
            Stand::File(_) => vec2(MIN_CARD, CARD_TALL),
        }
    }
}

/// The picture a run stands for, or the frame that takes its place; `None` for text, and for a
/// picture that is shown as a line of text.
fn shown<'a>(run: &'a Run, pictures: &'a Pictures) -> Option<Stand<'a>> {
    let drawn = match run.picture.as_ref()? {
        Picture::Attached(id) => pictures.get(id),
        Picture::Inline(Some(index)) => pictures.written(*index),
        Picture::Inline(None) | Picture::Remote | Picture::Object(_) => None,
        Picture::File(filed) => return Some(Stand::File(filed)),
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
        Picture::Object(_) | Picture::File(_) => "Attachment",
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

/// What the card of an attachment says: its name, what kind of object it is, and its size.
fn card_words(filed: &Filed) -> String {
    let mut words: String = filed.name.chars().filter(|c| !c.is_control()).take(120).collect();
    if let Some(kind) = &filed.kind {
        let kind: String = kind.chars().filter(|c| !c.is_control()).take(60).collect();
        words = format!("{words} ({kind})");
    }
    if filed.bytes > 0 {
        words = format!("{words}  ({})", mailcraft_engine::attachments::size_words(filed.bytes));
    }
    words
}

/// The size of an attachment's card in a line no wider than `wrap`.
fn card_size(ui: &Ui, filed: &Filed, wrap: f32) -> Vec2 {
    let wide = text_width(ui.painter(), &card_words(filed), font(12.0)) + CARD_ROOM;
    let most = if wrap.is_finite() { wrap.max(MIN_CARD) } else { MAX_PICTURE_WIDTH };
    vec2(wide.clamp(MIN_CARD, most).ceil(), CARD_TALL)
}

/// What is said about an attachment in the text when the pointer rests on it.
fn card_tip(filed: &Filed) -> String {
    let mut lines = vec![card_words(filed)];
    lines.extend(filed.caution.iter().map(|words| words.chars().take(300).collect::<String>()));
    lines.push(match filed.number {
        Some(_) => "Click to save this attachment to a file. Nothing is opened or run from here.".to_string(),
        None => "An object of another program that cannot be shown here.".to_string(),
    });
    lines.join("\n")
}

/// An attachment where it stands in the text: our own mark for a file, or for one to think
/// twice about, then its name and size. A click asks for it to be saved.
fn card(ui: &Ui, filed: &Filed, at: Rect, look: Look) {
    let plain = look.on == look.t.surface;
    let response = filed.number.map(|number| ui.interact(at, ui.id().with(("file in text", number)), Sense::click()));
    let hovered = response.as_ref().is_some_and(|r| r.hovered());
    let painter = ui.painter().with_clip_rect(at.intersect(ui.clip_rect()));
    let ink = look.text();
    let fill = match (plain, hovered) {
        (true, true) => look.t.hover,
        (true, false) => look.t.pane,
        (false, true) => ink.gamma_multiply(0.15),
        (false, false) => Color32::TRANSPARENT,
    };
    painter.rect_filled(at, 3.0, fill);
    // One a reader should think twice about says so before it is pointed at.
    let (line, icon, tint) = match filed.caution {
        Some(_) => ((1.5, look.t.danger), Icon::Importance, look.t.danger),
        None => ((1.0, if plain { look.t.line } else { faint(look) }), Icon::Page, faint(look)),
    };
    painter.rect_stroke(at, 3.0, line, egui::StrokeKind::Inside);
    icons::paint(&painter, Rect::from_center_size(pos2(at.left() + 12.0, at.center().y), vec2(13.0, 13.0)), icon, tint);
    // A long name is cut in its middle: the ending is what tells the kind of file.
    let room = at.width() - CARD_ROOM + 4.0;
    let words = card_words(filed);
    let words = if text_width(&painter, &words, font(12.0)) > room { crate::middle_cut(&painter, &words, room, font(12.0)) } else { words };
    clip_text(&painter, pos2(at.left() + 23.0, at.center().y - 8.0), room, &words, font(12.0), ink);
    if let Some(response) = response {
        if response.clicked() {
            look.picked.set(filed.number);
        }
        response.on_hover_text(card_tip(filed));
    }
}

/// Draw a paragraph. `until` is where a picture already standing at the side ends: lines that
/// start below it are not drawn but handed back, to go under the picture. A picture of the
/// paragraph's own that is left standing past its text is handed back too.
fn para(ui: &mut Ui, para: &Para, look: Look, after_another: bool, until: Option<f32>) -> (Option<Aside>, Option<Vec<Run>>) {
    if after_another {
        ui.add_space(f32::from(para.lead()));
    }
    let (_, width) = room(ui, para);
    let whole = Spot { left: 0.0, width, min_height: 0.0, marker: true };
    if let Some(until) = until {
        let (job, ends, _) = job(ui, para, &para.runs, look, width);
        let galley = ui.painter().layout_job(job);
        return match lines_beside(&galley, until - ui.cursor().top()) {
            Some(chars) => {
                let (head, tail) = split(&para.runs, &ends, chars);
                piece(ui, para, &head, look, whole);
                (None, Some(tail))
            }
            None => {
                piece(ui, para, &para.runs, look, whole);
                (None, None)
            }
        };
    }
    // The first picture that asks to stand at one side does, when there is room beside it.
    let aside = para.runs.iter().take(MAX_FLOAT_SEARCH).enumerate().find_map(|(i, run)| {
        let stand = (run.float != Float::None).then(|| shown(run, look.pictures)).flatten()?;
        let size = picture_size(run.extent, stand.natural(run), width);
        (width - size.x - FLOAT_GAP >= MIN_BESIDE).then_some((i, stand, size))
    });
    match aside {
        Some((at, stand, size)) => (beside(ui, para, at, stand, size, look), None),
        None => {
            piece(ui, para, &para.runs, look, whole);
            (None, None)
        }
    }
}

/// How many characters of a laid-out text stand in lines that start above `tall`; `None` when
/// all of them do. The first line always does.
fn lines_beside(galley: &egui::Galley, tall: f32) -> Option<usize> {
    let mut chars = 0usize;
    for (i, row) in galley.rows.iter().enumerate() {
        if i > 0 && row.pos.y >= tall {
            return Some(chars);
        }
        chars = chars.saturating_add(row.glyphs.len()).saturating_add(usize::from(row.ends_with_newline));
    }
    None
}

/// A paragraph with a picture at one side: the text stands beside the picture for as many lines
/// as the picture is tall, and under it from there on. A picture taller than the paragraph's
/// text is handed back as still standing: what follows the paragraph stands beside it too.
fn beside(ui: &mut Ui, para: &Para, at: usize, stand: Stand, size: Vec2, look: Look) -> Option<Aside> {
    let (inset, width) = room(ui, para);
    let run = para.runs.get(at)?;
    let rest: Vec<Run> = para.runs.iter().enumerate().filter(|(i, _)| *i != at).map(|(_, r)| r.clone()).collect();
    let narrow = width - size.x - FLOAT_GAP;
    let left = if run.float == Float::Left { size.x + FLOAT_GAP } else { 0.0 };

    // Lay the text out beside the picture to learn where the picture ends.
    let (job, ends, _) = job(ui, para, &rest, look, narrow);
    let galley = ui.painter().layout_job(job);
    let rect = match lines_beside(&galley, size.y) {
        Some(chars) => {
            let (head, tail) = split(&rest, &ends, chars);
            let rect = piece(ui, para, &head, look, Spot { left, width: narrow, min_height: size.y, marker: true });
            piece(ui, para, &tail, look, Spot { left: 0.0, width, min_height: 0.0, marker: false });
            rect
        }
        None => piece(ui, para, &rest, look, Spot { left, width: narrow, min_height: 0.0, marker: true }),
    };
    let bottom = rect.top() + size.y + FLOAT_GAP / 2.0;
    let standing = (bottom > ui.cursor().top()).then_some(Aside { left: run.float == Float::Left, width: inset + size.x + FLOAT_GAP, bottom });

    let x = rect.left() + inset + if run.float == Float::Right { width - size.x } else { 0.0 };
    let spot = Rect::from_min_size(pos2(x.round(), rect.top().round()), size);
    if ui.is_rect_visible(spot) {
        picture(ui, run, stand, spot, look);
        let id = ui.id().with(("aside", at, spot.min.x.to_bits(), spot.min.y.to_bits()));
        let tip = tip(run, stand);
        let response = ui.interact(spot, id, Sense::hover());
        if !tip.is_empty() {
            response.on_hover_text_at_pointer(tip);
        }
    }
    standing
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
        // A card is set as low as the tails of the letters beside it, so that its words stand on
        // their line.
        Stand::File(filed) => card(ui, filed, at.translate(vec2(0.0, BASELINE)), look),
    }
}

/// What is said about a picture when the pointer rests on it.
fn tip(run: &Run, stand: Stand) -> String {
    if let Stand::File(filed) = stand {
        return card_tip(filed);
    }
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
fn job<'a>(ui: &Ui, para: &Para, runs: &'a [Run], look: Look<'a>, wrap: f32) -> (LayoutJob, Vec<usize>, Vec<Placed<'a>>) {
    let (job, ends, placed, _) = job_and_leaders(ui, para, runs, look, wrap);
    (job, ends, placed)
}

/// A tab whose room is filled with marks.
struct Lead {
    /// Which of the text's tabs it is, counted from the first.
    nth: usize,
    with: rich::Leader,
    /// The room before the tab, and the size of the letters beside it.
    room: f32,
    size: f32,
}

/// As [`job`], and the tabs whose room is filled.
fn job_and_leaders<'a>(ui: &Ui, para: &Para, runs: &'a [Run], look: Look<'a>, wrap: f32) -> (LayoutJob, Vec<usize>, Vec<Placed<'a>>, Vec<Lead>) {
    let mut leaders: Vec<Lead> = Vec::new();
    let mut tabs = 0usize;
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    // Where the text stands in its line, kept only where a tab will ask.
    let tabbed = runs.iter().any(|r| r.picture.is_none() && r.text.contains('\t'));
    let mut x = 0.0f32;
    job.halign = match para.align {
        rich::Align::Left => Align::LEFT,
        rich::Align::Center => Align::Center,
        rich::Align::Right => Align::RIGHT,
    };
    let mut ends: Vec<usize> = Vec::with_capacity(runs.len());
    let mut placed: Vec<Placed> = Vec::new();
    let mut chars = 0usize;
    // A line told how tall to be: each piece of text asks for that height, counted from
    // its own.
    let told = |format: &mut TextFormat| {
        if para.leading != rich::Leading::Own {
            let own = ui.ctx().fonts_mut(|fonts| fonts.row_height(&format.font_id));
            format.line_height = Some(para.leading.tall(own, 1.0).max(1.0));
        }
    };
    for (at, run) in runs.iter().enumerate() {
        let held = shown(run, look.pictures).and_then(|stand| {
            let size = match stand {
                Stand::File(filed) => card_size(ui, filed, wrap),
                _ => picture_size(run.extent, stand.natural(run), wrap),
            };
            holder(ui, size).map(|(space, format)| (stand, size, space, format))
        });
        let wide = held.as_ref().map(|(_, size, ..)| size.x);
        let (text, format) = match held {
            Some((stand, size, space, format)) => {
                placed.push(Placed { run, stand, size, holder: (space, size.y + BASELINE) });
                (space.to_string(), format)
            }
            None => {
                let (text, mut format) = section(run, look);
                // A link that is not what its text says is marked before anyone points at it.
                if run.picture.is_none() && mailcraft_formats::links::concern_at(runs, at).is_some() {
                    format.underline = Stroke::new(2.0, look.t.danger);
                }
                told(&mut format);
                (text, format)
            }
        };
        chars = chars.saturating_add(text.chars().count());
        ends.push(chars);
        match wide {
            _ if !tabbed => job.append(&text, 0.0, format),
            Some(wide) => {
                x += wide;
                job.append(&text, 0.0, format);
            }
            None => {
                let mut rest = text.as_str();
                loop {
                    let (before, after) = match rest.split_once('\t') {
                        Some((before, after)) => (before, Some(after)),
                        None => (rest, None),
                    };
                    if !before.is_empty() {
                        let measure = |text: &str| text_width(ui.painter(), text, format.font_id.clone());
                        x = match before.rsplit_once('\n') {
                            Some((_, last)) => measure(last),
                            None => x + measure(before),
                        };
                        job.append(before, 0.0, format.clone());
                    }
                    let Some(after) = after else { break };
                    let (tab, own) = tab_format(ui, &format);
                    // Preformatted text has a stop every eight characters.
                    let step = if matches!(para.kind, ParaKind::Pre) {
                        8.0 * text_width(ui.painter(), " ", format.font_id.clone())
                    } else {
                        f32::from(rich::TAB_STEP)
                    };
                    // A tab past the end of the first line stands where nothing here can say.
                    let room = if x < wrap {
                        let (ahead, point) = ahead(ui, runs, look, at, after, &format.font_id);
                        let room = rich::tab_room(&para.tabs, 1.0, step, x, ahead, point);
                        if let Some((_, stop)) = rich::tab_stop(&para.tabs, 1.0, x)
                            && stop.lead != rich::Leader::None
                            && leaders.len() < MAX_LEADERS
                        {
                            leaders.push(Lead { nth: tabs, with: stop.lead, room: (room - own).max(0.0), size: format.font_id.size });
                        }
                        room
                    } else {
                        step
                    };
                    tabs = tabs.saturating_add(1);
                    job.append("\t", (room - own).max(0.0), tab);
                    x += room.max(own);
                    rest = after;
                }
            }
        }
    }
    (job, ends, placed, leaders)
}

/// What fills the room of the tabs of a laid out text: marks from the end of what is before
/// each tab to the tab, on the line of the letters. `pos` is the text's own corner.
fn draw_leaders(ui: &Ui, galley: &egui::Galley, leaders: &[Lead], pos: egui::Pos2, color: Color32) {
    if leaders.is_empty() {
        return;
    }
    let painter = ui.painter();
    let mut nth = 0usize;
    for row in &galley.rows {
        for glyph in &row.glyphs {
            if glyph.chr != '\t' {
                continue;
            }
            let this = nth;
            nth = nth.saturating_add(1);
            let Some(lead) = leaders.iter().find(|l| l.nth == this) else { continue };
            let (room, size) = (lead.room, lead.size.clamp(8.0, 64.0));
            let end = pos.x + row.pos.x + glyph.pos.x - size * 0.2;
            let start = end - room + size * 0.4;
            if !(start.is_finite() && end.is_finite()) || end - start < size * 0.5 {
                continue;
            }
            let base = (pos.y + row.pos.y + row.size.y - size * 0.3).round() - 1.0;
            let line = |y: f32, thick: f32| {
                painter.rect_filled(Rect::from_min_max(pos2(start.round(), y), pos2(end.round(), y + thick)), 0.0, color);
            };
            let marks = |y: f32, mark: f32, step: f32| {
                let mut at = start.round();
                let mut drawn = 0usize;
                while at + mark <= end && drawn < MAX_DASHES {
                    painter.rect_filled(Rect::from_min_size(pos2(at, y), vec2(mark, (size / 12.0).round().max(1.0))), 0.0, color);
                    at += step;
                    drawn += 1;
                }
            };
            let thin = (size / 14.0).round().max(1.0);
            match lead.with {
                rich::Leader::None => {}
                rich::Leader::Dots => marks(base, thin.max(2.0), (size * 0.3).round().max(4.0)),
                rich::Leader::MiddleDots => marks(base - (size * 0.25).round(), thin.max(2.0), (size * 0.3).round().max(4.0)),
                rich::Leader::Hyphens => marks(base - (size * 0.22).round(), (size * 0.25).round().max(2.0), (size * 0.45).round().max(4.0)),
                rich::Leader::Line => line(base + 1.0, thin),
                rich::Leader::ThickLine => line(base + 1.0, thin * 2.0),
                rich::Leader::Equals => {
                    line(base - (size * 0.18).round(), thin);
                    line(base - (size * 0.18).round() - thin * 2.0 - 1.0, thin);
                }
            }
        }
    }
}

/// How a tab is set: the tab itself in letters too small to take room, so that the room
/// before it is all the room it takes. Gives its format and the room it takes of itself.
fn tab_format(ui: &Ui, format: &TextFormat) -> (TextFormat, f32) {
    let font_id = FontId::new(1.0, format.font_id.family.clone());
    let own = ui.ctx().fonts_mut(|fonts| fonts.glyph_width(&font_id, '\t'));
    let tab = TextFormat { font_id, underline: Stroke::NONE, strikethrough: Stroke::NONE, background: Color32::TRANSPARENT, ..format.clone() };
    (tab, if own.is_finite() { own.max(0.0) } else { 0.0 })
}

/// The width of the text that follows a tab, up to the next tab or the end of the line, and
/// its width up to its decimal point (all of it where it has none). `rest` is what follows
/// the tab in run `at`, set in `font`.
fn ahead(ui: &Ui, runs: &[Run], look: Look, at: usize, rest: &str, font: &FontId) -> (f32, f32) {
    const RUNS_AHEAD: usize = 64;
    let (mut wide, mut point) = (0.0f32, None);
    // Gives whether the line or the room up to the next tab ended in `text`.
    let mut take = |text: &str, font: &FontId| {
        let end = text.find(['\t', '\n']);
        let part = text.get(..end.unwrap_or(text.len())).unwrap_or_default();
        if point.is_none()
            && let Some(dot) = part.find('.')
        {
            point = Some(wide + text_width(ui.painter(), part.get(..dot).unwrap_or_default(), font.clone()));
        }
        wide += text_width(ui.painter(), part, font.clone());
        end.is_some()
    };
    if !take(rest, font) {
        for run in runs.iter().skip(at.saturating_add(1)).take(RUNS_AHEAD) {
            if run.picture.is_some() {
                continue;
            }
            let (text, format) = section(run, look);
            if take(&text, &format.font_id) {
                break;
            }
        }
    }
    (wide, point.unwrap_or(wide))
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
    let (job, ends, placed, leaders) = job_and_leaders(ui, para, runs, look, spot.width);
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
        // In the letters of the text it stands before, so that it sits on the same line.
        let like = runs.iter().find(|r| r.picture.is_none()).map_or_else(|| font(BODY_PX), |r| section(r, look).1.font_id);
        ui.painter().text(pos2(rect.left() + inset - 7.0, rect.top()), egui::Align2::RIGHT_TOP, marker, like, look.text());
    }

    let spots: Vec<Rect> = spots(&galley, &placed).into_iter().map(|at| at.translate(pos.to_vec2())).collect();
    // Where the pointer is: on a picture, or in the text, where a link shows its address and
    // nothing else happens.
    if let Some(pointer) = response.hover_pos() {
        if let Some((p, _)) = placed.iter().zip(&spots).find(|(_, at)| at.contains(pointer)) {
            let mut tip = tip(p.run, p.stand);
            if let Some(concern) = runs.iter().position(|r| std::ptr::eq(r, p.run)).and_then(|at| mailcraft_formats::links::concern_at(runs, at)) {
                tip = format!("{}\n{tip}", concern.words());
            }
            if !tip.is_empty() {
                response.clone().on_hover_text_at_pointer(tip);
            }
        } else {
            let cursor = galley.cursor_from_pos(pointer - pos);
            let at = galley.pos_from_cursor(cursor).translate(pos.to_vec2());
            if (pointer.x - at.center().x).abs() < 16.0 && at.y_range().contains(pointer.y) {
                let index = cursor.index.0;
                let at = ends.iter().position(|end| index < *end);
                if let Some(link) = at.and_then(|i| runs.get(i)).and_then(|r| r.style.link.as_deref()) {
                    let shown: String = link.chars().filter(|c| !c.is_control()).take(300).collect();
                    let concern =
                        at.and_then(|i| mailcraft_formats::links::concern_at(runs, i)).map(|c| format!("{}\n", c.words())).unwrap_or_default();
                    response.clone().on_hover_text_at_pointer(format!("{concern}{shown}\nLinks are not opened from here."));
                }
            }
        }
    }
    draw_leaders(ui, &galley, &leaders, pos, look.text());
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
    let named = s.family.as_deref().and_then(|list| look.faces.pick(list, s.bold, s.italic));
    // A font's own italic face is drawn as it is; without one, the upright face is slanted.
    format.italics = named.as_ref().map_or(s.italic, |picked| picked.slant);
    format.font_id = match (named, s.mono, s.bold) {
        (Some(picked), ..) => FontId::new(px, picked.family),
        (None, true, _) => FontId::new(px, FontFamily::Monospace),
        (None, false, true) => bold(px),
        (None, false, false) => font(px),
    };
    let (color, background) = colors(s.color, s.background, look);
    format.color = color;
    format.background = background;
    if let Some(Picture::File(_)) = &run.picture {
        // Where there is no room for its card, an attachment is named in words.
        format.color = faint(look);
        format.background = Color32::TRANSPARENT;
        return (alt(run), format);
    }
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
                let (mut job, ..) = job(ui, p, &p.runs, look, f32::INFINITY);
                job.halign = Align::LEFT;
                let border = p.frame.as_ref().map_or(0.0, |frame| {
                    let (near, far) = frame_room(frame);
                    near.x + far.x + f32::from(frame.indent.min(12)) * INDENT
                });
                inset(p) + ui.painter().layout_job(job).size().x + 2.0 + border
            }
            Block::Rule => 0.0,
            Block::Table(t) => match t.width {
                TableWidth::Pixels(w) => f32::from(w),
                TableWidth::Percent(_) | TableWidth::Fill => UNMEASURED,
                _ if depth < MAX_TABLE_DEPTH && t.columns() <= MAX_COLUMNS => {
                    let columns = natural_columns(ui, t, look, depth);
                    columns.iter().sum::<f32>() + spacing(t) * (columns.len() as f32 + 1.0) + rim(t) * 2.0
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
    // A cell's own lines are drawn inside it: what they take past a hair line is not the text's.
    let line = |a: usize, b: usize| {
        let wide = |i: usize| cell.edges.and_then(|e| e.get(i).map(|e| e.width.min(MAX_EDGE))).unwrap_or(0);
        f32::from(wide(a).max(wide(b)).saturating_sub(1))
    };
    vec2(f32::from(horizontal.min(MAX_CELL_ROOM)) + line(1, 3), f32::from(vertical.min(MAX_CELL_ROOM)) + line(0, 2))
}

/// Room a table's own border takes on each side when it is thicker than a hair line.
fn rim(table: &Table) -> f32 {
    (table.frame.iter().map(|e| f32::from(e.width.min(MAX_EDGE))).fold(0.0, f32::max) - 1.0).max(0.0)
}

/// Room between the cells of a table.
fn spacing(table: &Table) -> f32 {
    f32::from(table.spacing.min(MAX_CELL_ROOM))
}

/// Give the columns that ask for a share of the table's width that share of `room`, and the
/// room that is left to the others.
fn shares(table: &Table, widths: &mut [f32], room: f32) {
    let mut asked = vec![0.0f32; widths.len()];
    let places = table.places();
    for (row, starts) in table.rows.iter().zip(&places).take(MEASURED_ROWS) {
        for (cell, col) in row.iter().zip(starts) {
            if cell.span <= 1
                && let Some(a) = asked.get_mut(*col)
            {
                *a = a.max(f32::from(cell.share.min(100)) / 100.0);
            }
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
    let places = table.places();
    for (row, starts) in table.rows.iter().zip(&places).take(MEASURED_ROWS) {
        for (cell, col) in row.iter().zip(starts) {
            let col = *col;
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
    // Its cells stand apart by the table's spacing, which also runs round the outside, inside
    // the table's own border.
    let mut widths = natural_columns(ui, table, look, depth);
    let total: f32 = widths.iter().sum();
    let gaps = columns as f32 + 1.0;
    let rim = rim(table);
    let target = match table.width {
        TableWidth::Pixels(w) => f32::from(w).min(available),
        TableWidth::Percent(share) => available * f32::from(share.clamp(1, 100)) / 100.0,
        TableWidth::Fill => available,
        TableWidth::Content => (total + spacing(table) * gaps + rim * 2.0).min(available),
    };
    let rim = rim.min(target / 4.0);
    // Spacing never takes more than half the table.
    let gap = spacing(table).min((target - rim * 2.0) / gaps / 2.0).max(0.0);
    let cells = (target - rim * 2.0 - gap * gaps).max(0.0);
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
    let room_left = ui.cursor().left();
    let left = room_left + shift;
    let table_top = ui.cursor().top();
    let mut lefts: Vec<f32> = Vec::with_capacity(columns);
    let mut x = left + rim + gap;
    for w in &widths {
        lefts.push(x);
        x += w + gap;
    }

    // Where each cell stands: a cell may cover several columns, and several rows.
    struct Slot<'a> {
        row: usize,
        /// The last row the cell covers.
        upto: usize,
        number: usize,
        cell: &'a rich::Cell,
        x: f32,
        w: f32,
        pad: Vec2,
        /// How tall the cell is with what it holds.
        tall: f32,
    }
    let count = table.rows.len();
    let places = table.places();
    let mut slots: Vec<Slot> = Vec::new();
    for (r, (row, starts)) in table.rows.iter().zip(&places).enumerate() {
        for (c, (cell, col)) in row.iter().zip(starts).enumerate() {
            let Some(x) = lefts.get(*col).copied() else { continue };
            let covered = widths.iter().skip(*col).take(usize::from(cell.span.max(1)));
            let w: f32 = covered.clone().sum::<f32>() + gap * (covered.count().max(1) - 1) as f32;
            if w <= 0.0 {
                continue;
            }
            let upto = r.saturating_add(usize::from(cell.rows.max(1)) - 1).min(count.saturating_sub(1));
            slots.push(Slot { row: r, upto, number: c, cell, x, w, pad: padding(table, cell), tall: 0.0 });
        }
    }
    let on = |cell: &rich::Cell| cell.background.map_or(look.on, |c| Color32::from_rgb(c[0], c[1], c[2]));
    let inside =
        |slot: &Slot, top: f32| Rect::from_min_size(pos2(slot.x + slot.pad.x, top + slot.pad.y), vec2((slot.w - slot.pad.x * 2.0).max(8.0), 0.0));

    // Every cell is laid out once unseen, to learn how tall it is: a row is as tall as its
    // tallest cell, and a cell over several rows makes the last of them tall enough for it.
    for slot in &mut slots {
        let mut child = ui.new_child(UiBuilder::new().id_salt(("cell size", slot.row, slot.number)).max_rect(inside(slot, table_top)).invisible());
        child.spacing_mut().item_spacing = vec2(0.0, 0.0);
        blocks(&mut child, &slot.cell.blocks, Look { on: on(slot.cell), ..look }, depth + 1);
        slot.tall = child.min_rect().height() + slot.pad.y * 2.0;
    }
    let mut heights = vec![0.0f32; count];
    for slot in slots.iter().filter(|s| s.upto == s.row) {
        if let Some(h) = heights.get_mut(slot.row) {
            *h = h.max(slot.tall);
        }
    }
    let mut reaching: Vec<&Slot> = slots.iter().filter(|s| s.upto > s.row).collect();
    reaching.sort_by_key(|s| s.upto);
    let mut reaching = reaching.into_iter().peekable();
    // Where each row starts, below the first one's top.
    let mut tops = vec![0.0f32; count];
    let mut y = 0.0f32;
    for r in 0..count {
        if let Some(top) = tops.get_mut(r) {
            *top = y;
        }
        while let Some(slot) = reaching.next_if(|s| s.upto <= r) {
            let start = tops.get(slot.row).copied().unwrap_or(y);
            if let Some(h) = heights.get_mut(r) {
                *h = h.max(slot.tall - (y - start));
            }
        }
        y += heights.get(r).copied().unwrap_or(0.0) + gap;
    }
    let first = table_top + rim + gap;
    let bottom = first + y + rim;

    let whole = Rect::from_min_max(pos2(room_left, table_top), pos2(room_left + available, bottom));
    if ui.is_visible() && ui.is_rect_visible(whole) {
        let lines =
            table.line.map(|c| Color32::from_rgb(c[0], c[1], c[2])).filter(|c| contrast(*c, look.on) >= EDGE_CONTRAST).unwrap_or(look.t.text_faint);
        for slot in &slots {
            let top = first + tops.get(slot.row).copied().unwrap_or(0.0);
            let end = first + tops.get(slot.upto).copied().unwrap_or(0.0) + heights.get(slot.upto).copied().unwrap_or(0.0);
            let rect = Rect::from_min_max(pos2(slot.x, top), pos2(slot.x + slot.w, end.max(top)));
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let within = Look { on: on(slot.cell), ..look };
            if slot.cell.background.is_some() {
                ui.painter().rect_filled(rect, 0.0, within.on);
            }
            let spare = (rect.height() - slot.tall).max(0.0);
            let down = match slot.cell.valign {
                VAlign::Top => 0.0,
                VAlign::Middle => (spare / 2.0).floor(),
                VAlign::Bottom => spare,
            };
            let mut child = ui.new_child(UiBuilder::new().id_salt(("cell", slot.row, slot.number)).max_rect(inside(slot, top + down)));
            child.spacing_mut().item_spacing = vec2(0.0, 0.0);
            blocks(&mut child, &slot.cell.blocks, within, depth + 1);
            match &slot.cell.edges {
                Some(own) => edges(ui, rect, own, within, within.text()),
                None if table.border => {
                    ui.painter().rect_stroke(rect, 0.0, (1.0, lines), egui::StrokeKind::Middle);
                }
                None => {}
            }
        }
        // The table's own border: seen round cells that stand apart, or when it is a border of
        // its own making.
        if any_edge(&table.frame) && (gap > 0.0 || rim > 0.0 || !table.border) {
            let outer = Rect::from_min_max(pos2(left, table_top), pos2(left + target, bottom));
            edges(ui, outer, &table.frame, look, if table.border { lines } else { look.text() });
        }
    }
    ui.allocate_rect(whole, Sense::hover());
}
