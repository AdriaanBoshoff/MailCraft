//! Draws a message body that was read into a formatted document.
//!
//! A link shows where it points when the pointer rests on it and is never followed. A picture is
//! drawn only when the message carries it as an attachment and it could be decoded
//! ([`Pictures`]); any other picture is a labelled placeholder. Colours a message asks for are
//! kept only where they stay readable on what is behind them. Sizes follow what Outlook Classic
//! was seen to do with the same message (`docs/ui-parity.md`, "Message bodies").

use crate::pictures::{Pictures, Shown};
use crate::theme::Tokens;
use crate::{bold, font};
use egui::text::{LayoutJob, TextFormat};
use egui::text_selection::LabelSelectionState;
use egui::{Align, Color32, FontFamily, FontId, Rect, Sense, Shape, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use mailcraft_formats::rich::{self, Block, Doc, Para, ParaKind, Picture, Run, Script, Table, TableWidth};

/// Size of text the message gives no size for: 12 points.
const BODY_PX: f32 = 16.0;
/// One level of indent: a list level, a block set in, half an inch of RTF indent.
const INDENT: f32 = 40.0;
const QUOTE: f32 = 14.0;
/// More columns than this are drawn one cell under another.
const MAX_COLUMNS: usize = 12;
/// Tables nested deeper than this are drawn one cell under another.
const MAX_TABLE_DEPTH: usize = 6;
const CELL_PAD: f32 = 5.0;
const MIN_COLUMN: f32 = 24.0;
/// Rows of a table, and blocks of a cell, that are measured to size its columns.
const MEASURED_ROWS: usize = 100;
const MEASURED_BLOCKS: usize = 200;
/// Width a column is taken to want when what it holds is not measured.
const UNMEASURED: f32 = 240.0;
/// Space between pictures that stand side by side, and between rows of them.
const PICTURE_GAP: f32 = 4.0;
/// Tallest a picture is drawn, whatever it asks for.
const MAX_PICTURE_HEIGHT: f32 = 4096.0;

/// What drawing needs besides the document: the theme, the decoded pictures, and the colour
/// behind the text being drawn (the pane, or a filled table cell).
#[derive(Clone, Copy)]
struct Look<'a> {
    t: &'a Tokens,
    pictures: &'a Pictures,
    on: Color32,
}

impl Look<'_> {
    /// The colour of text that asks for none.
    fn text(&self) -> Color32 {
        if self.on == self.t.surface { self.t.text } else { plain_on(self.on) }
    }
}

pub(crate) fn draw(ui: &mut Ui, doc: &Doc, t: &Tokens, pictures: &Pictures) {
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    blocks(ui, &doc.blocks, Look { t, pictures, on: t.surface }, 0);
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

/// The picture a run stands for, when there is one to draw.
fn shown<'a>(run: &Run, pictures: &'a Pictures) -> Option<&'a Shown> {
    match &run.picture {
        Some(Picture::Attached(id)) => pictures.get(id),
        _ => None,
    }
}

fn para(ui: &mut Ui, para: &Para, look: Look, after_another: bool) {
    if para.gap && after_another {
        ui.add_space(if matches!(para.kind, ParaKind::Heading(_)) { 12.0 } else { 9.0 });
    }
    if !para.runs.iter().any(|r| shown(r, look.pictures).is_some()) {
        return text(ui, para, &para.runs, look, true, (false, false));
    }
    // Text and pictures take turns down the page: pictures stand in rows of their own.
    let blank = |r: &Run| r.picture.is_none() && r.text.trim().is_empty();
    let mut rest = para.runs.as_slice();
    let mut first = true;
    while let Some(head) = rest.first() {
        let is_picture = shown(head, look.pictures).is_some();
        let count = if is_picture {
            // White space between two pictures does not end the row.
            rest.iter().take_while(|r| shown(r, look.pictures).is_some() || (blank(r) && !r.text.contains('\n'))).count()
        } else {
            rest.iter().take_while(|r| shown(r, look.pictures).is_none()).count()
        };
        let (piece, tail) = rest.split_at(count.clamp(1, rest.len()));
        if is_picture {
            picture_rows(ui, para, piece, look);
            first = false;
        } else if !piece.iter().all(blank) {
            text(ui, para, piece, look, first, (!first, !tail.is_empty()));
            first = false;
        }
        rest = tail;
    }
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
    vec2(size.x.max(1.0), size.y.clamp(1.0, MAX_PICTURE_HEIGHT))
}

/// Pictures of one paragraph that follow each other, side by side while they fit.
fn picture_rows(ui: &mut Ui, para: &Para, runs: &[Run], look: Look) {
    let (inset, width) = room(ui, para);
    struct Row<'a> {
        size: Vec2,
        items: Vec<(&'a Run, &'a Shown, Vec2)>,
    }
    let mut rows: Vec<Row> = Vec::new();
    for run in runs {
        let Some(picture) = shown(run, look.pictures) else { continue };
        let size = picture_size(run.extent, picture.size, width);
        match rows.last_mut() {
            Some(row) if row.size.x + PICTURE_GAP + size.x <= width => {
                row.size = vec2(row.size.x + PICTURE_GAP + size.x, row.size.y.max(size.y));
                row.items.push((run, picture, size));
            }
            _ => rows.push(Row { size, items: vec![(run, picture, size)] }),
        }
    }
    let height: f32 = rows.iter().map(|r| r.size.y + PICTURE_GAP).sum();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    quote_bars(ui, para, rect, look.t);
    let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    let mut y = rect.top() + PICTURE_GAP / 2.0;
    for row in &rows {
        let mut x = rect.left()
            + inset
            + match para.align {
                rich::Align::Left => 0.0,
                rich::Align::Center => ((width - row.size.x) / 2.0).max(0.0),
                rich::Align::Right => (width - row.size.x).max(0.0),
            };
        for (run, picture, size) in &row.items {
            // Pictures of one row stand on the same line.
            let at = Rect::from_min_size(pos2(x, y + row.size.y - size.y), *size);
            x += size.x + PICTURE_GAP;
            if !ui.is_rect_visible(at) {
                continue;
            }
            ui.painter().image(picture.texture.id(), at, whole, Color32::WHITE);
            if response.hover_pos().is_some_and(|p| at.contains(p)) {
                let alt: String = run.text.chars().filter(|c| !c.is_control()).take(120).collect();
                let link: Option<String> = run.style.link.as_deref().map(|l| l.chars().take(300).collect());
                let tip = match (alt.is_empty(), link) {
                    (false, Some(link)) => format!("{alt}\n{link}\nLinks are not opened from here."),
                    (true, Some(link)) => format!("{link}\nLinks are not opened from here."),
                    (false, None) => alt,
                    (true, None) => continue,
                };
                response.clone().on_hover_text_at_pointer(tip);
            }
        }
        y += row.size.y + PICTURE_GAP;
    }
}

/// Runs laid out as one piece of text no wider than `wrap`, and the character offset where each
/// run ends. `trim` drops a line break at the start or the end.
fn job(runs: &[Run], look: Look, wrap: f32, align: rich::Align, trim: (bool, bool)) -> (LayoutJob, Vec<usize>) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    job.halign = match align {
        rich::Align::Left => Align::LEFT,
        rich::Align::Center => Align::Center,
        rich::Align::Right => Align::RIGHT,
    };
    let mut ends: Vec<usize> = Vec::with_capacity(runs.len());
    let mut chars = 0usize;
    for (i, run) in runs.iter().enumerate() {
        let (mut text, format) = section(run, look);
        if trim.0 && i == 0 && text.starts_with('\n') {
            text.remove(0);
        }
        if trim.1 && i + 1 == runs.len() && text.ends_with('\n') {
            text.pop();
        }
        chars = chars.saturating_add(text.chars().count());
        ends.push(chars);
        job.append(&text, 0.0, format);
    }
    (job, ends)
}

/// Runs of one paragraph drawn as text. `marker` draws the list marker beside them; `trim` drops
/// a line break at the start or the end, where a row of pictures already breaks the line.
fn text(ui: &mut Ui, para: &Para, runs: &[Run], look: Look, marker: bool, trim: (bool, bool)) {
    let (inset, width) = room(ui, para);
    let (job, ends) = job(runs, look, width, para.align, trim);
    let galley = ui.painter().layout_job(job);

    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), galley.size().y + 2.0), Sense::click_and_drag() - Sense::FOCUSABLE);
    if !ui.is_rect_visible(rect) {
        return;
    }
    let text_left = rect.left() + inset;
    let pos = match para.align {
        rich::Align::Left => pos2(text_left, rect.top()),
        rich::Align::Center => pos2(text_left + width / 2.0, rect.top()),
        rich::Align::Right => pos2(text_left + width, rect.top()),
    };
    quote_bars(ui, para, rect, look.t);
    if marker && let ParaKind::Item(marker) = &para.kind {
        let marker: String = marker.chars().take(12).collect();
        ui.painter().text(pos2(text_left - 7.0, rect.top()), egui::Align2::RIGHT_TOP, marker, font(BODY_PX), look.text());
    }

    // Where the pointer is, in the text: a link shows its address and nothing else happens.
    if let Some(pointer) = response.hover_pos() {
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
    LabelSelectionState::label_text_selection(ui, &response, pos, galley, look.text(), Stroke::NONE);
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
    format.font_id = match (s.mono, s.bold) {
        (true, _) => FontId::new(px, FontFamily::Monospace),
        (false, true) => bold(px),
        (false, false) => font(px),
    };
    format.italics = s.italic;
    let (color, background) = colors(s.color, s.background, look);
    format.color = color;
    format.background = background;
    if let Some(picture) = &run.picture {
        let what = match picture {
            Picture::Remote => "Picture not downloaded",
            Picture::Attached(_) | Picture::Inline => "Picture",
        };
        let alt: String = run.text.chars().filter(|c| !c.is_control()).take(120).collect();
        format.color = if look.on == look.t.surface { look.t.text_faint } else { look.text().gamma_multiply(0.75) };
        format.background = Color32::TRANSPARENT;
        format.font_id = font(px.min(13.5));
        return (if alt.is_empty() { format!("[{what}]") } else { format!("[{what}: {alt}]") }, format);
    }
    if s.link.is_some() {
        if s.background.is_none() && contrast(look.t.accent_dark, look.on) >= 3.0 {
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
                let words: Vec<Run> = p.runs.iter().filter(|r| shown(r, look.pictures).is_none()).cloned().collect();
                let (job, _) = job(&words, look, f32::INFINITY, rich::Align::Left, (false, false));
                let pictures: f32 = p
                    .runs
                    .iter()
                    .filter_map(|r| shown(r, look.pictures).map(|s| picture_size(r.extent, s.size, f32::INFINITY).x + PICTURE_GAP))
                    .sum();
                inset(p) + ui.painter().layout_job(job).size().x.max(pictures) + 2.0
            }
            Block::Rule => 0.0,
            Block::Table(t) => match t.width {
                TableWidth::Pixels(w) => f32::from(w),
                _ if depth < MAX_TABLE_DEPTH && t.columns() <= MAX_COLUMNS => natural_columns(ui, t, look, depth).iter().sum(),
                _ => UNMEASURED,
            },
        };
        if width.is_finite() {
            widest = widest.max(width);
        }
    }
    widest
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
            let want = natural_width(ui, &cell.blocks, Look { on, ..look }, depth + 1) + CELL_PAD * 2.0;
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
    let mut widths = natural_columns(ui, table, look, depth);
    let total: f32 = widths.iter().sum();
    let target = match table.width {
        TableWidth::Pixels(w) => f32::from(w).min(available),
        TableWidth::Fill => available,
        TableWidth::Content => total.min(available),
    };
    if total > 0.0 && total.is_finite() {
        widths.iter_mut().for_each(|w| *w *= target / total);
    } else {
        widths = vec![target / columns as f32; columns];
    }
    let shift = match table.align {
        rich::Align::Left => 0.0,
        rich::Align::Center => ((available - target) / 2.0).max(0.0),
        rich::Align::Right => (available - target).max(0.0),
    };

    for (r, row) in table.rows.iter().enumerate() {
        let top = ui.cursor().top();
        let left = ui.cursor().left();
        let mut x = left + shift;
        let mut bottom = top;
        let mut col = 0usize;
        // Each cell's left edge and width, and where its fill goes once the row's height is known.
        let mut edges: Vec<(f32, f32, Option<(egui::layers::ShapeIdx, Color32)>)> = Vec::with_capacity(row.len());
        for (c, cell) in row.iter().enumerate() {
            let span = usize::from(cell.span.max(1));
            let w: f32 = widths.iter().skip(col).take(span).sum();
            col = col.saturating_add(span);
            if w <= 0.0 {
                continue;
            }
            let fill = cell.background.map(|c| (ui.painter().add(Shape::Noop), Color32::from_rgb(c[0], c[1], c[2])));
            let on = fill.map_or(look.on, |(_, color)| color);
            let inner = Rect::from_min_size(pos2(x + CELL_PAD, top + CELL_PAD - 2.0), vec2((w - CELL_PAD * 2.0).max(8.0), 0.0));
            let mut child = ui.new_child(UiBuilder::new().id_salt(("cell", r, c)).max_rect(inner));
            child.spacing_mut().item_spacing = vec2(0.0, 0.0);
            blocks(&mut child, &cell.blocks, Look { on, ..look }, depth + 1);
            bottom = bottom.max(child.min_rect().bottom());
            edges.push((x, w, fill));
            x += w;
        }
        bottom += CELL_PAD - 2.0;
        ui.allocate_rect(Rect::from_min_max(pos2(left, top), pos2(left + available, bottom)), Sense::hover());
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
}
