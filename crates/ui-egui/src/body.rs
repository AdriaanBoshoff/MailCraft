//! Draws a message body that was read into a formatted document.
//!
//! Only text is painted. A link shows where it points when the pointer rests on it and is never
//! followed; a picture is a labelled placeholder. Colours a message asks for are kept only where
//! they stay readable on the pane.

use crate::theme::Tokens;
use crate::{bold, font};
use egui::text::{LayoutJob, TextFormat};
use egui::text_selection::LabelSelectionState;
use egui::{Align, Color32, FontFamily, FontId, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use mailcraft_formats::rich::{self, Block, Doc, Para, ParaKind, Picture, Run, Script, Table};

/// Size of text the message gives no size for.
const BODY_PX: f32 = 13.5;
const INDENT: f32 = 24.0;
const QUOTE: f32 = 14.0;
/// More columns than this are drawn one cell under another.
const MAX_COLUMNS: usize = 12;
/// Tables nested deeper than this are drawn one cell under another.
const MAX_TABLE_DEPTH: usize = 6;
const CELL_PAD: f32 = 5.0;

pub(crate) fn draw(ui: &mut Ui, doc: &Doc, t: &Tokens) {
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    blocks(ui, &doc.blocks, t, 0);
}

fn blocks(ui: &mut Ui, list: &[Block], t: &Tokens, depth: usize) {
    for (i, block) in list.iter().enumerate() {
        match block {
            Block::Para(p) => para(ui, p, t, i > 0),
            Block::Rule => {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 13.0), Sense::hover());
                ui.painter().hline(rect.x_range(), rect.center().y, (1.0, t.text_faint));
            }
            Block::Table(table) => {
                if i > 0 {
                    ui.add_space(6.0);
                }
                self::table(ui, table, t, depth);
            }
        }
    }
}

fn para(ui: &mut Ui, para: &Para, t: &Tokens, after_another: bool) {
    if para.gap && after_another {
        ui.add_space(if matches!(para.kind, ParaKind::Heading(_)) { 12.0 } else { 9.0 });
    }
    let quote = f32::from(para.quote.min(8)) * QUOTE;
    let inset = (f32::from(para.indent.min(12)) * INDENT + quote).min(ui.available_width() * 0.6);
    let width = (ui.available_width() - inset).max(40.0);

    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    job.halign = match para.align {
        rich::Align::Left => Align::LEFT,
        rich::Align::Center => Align::Center,
        rich::Align::Right => Align::RIGHT,
    };
    // Character offset where each run ends, to find the run under the pointer.
    let mut ends: Vec<usize> = Vec::with_capacity(para.runs.len());
    let mut chars = 0usize;
    for run in &para.runs {
        let (text, format) = section(run, t);
        chars = chars.saturating_add(text.chars().count());
        ends.push(chars);
        job.append(&text, 0.0, format);
    }
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
    for level in 0..para.quote.min(8) {
        let x = rect.left() + f32::from(level) * QUOTE + 4.0;
        ui.painter().vline(x, rect.y_range(), (2.0, t.line.lerp_to_gamma(t.text_faint, 0.6)));
    }
    if let ParaKind::Item(marker) = &para.kind {
        let marker: String = marker.chars().take(12).collect();
        ui.painter().text(pos2(text_left - 7.0, rect.top()), egui::Align2::RIGHT_TOP, marker, font(BODY_PX), t.text);
    }

    // Where the pointer is, in the text: a link shows its address and nothing else happens.
    if let Some(pointer) = response.hover_pos() {
        let cursor = galley.cursor_from_pos(pointer - pos);
        let at = galley.pos_from_cursor(cursor).translate(pos.to_vec2());
        if (pointer.x - at.center().x).abs() < 16.0 && at.y_range().contains(pointer.y) {
            let index = cursor.index.0;
            let run = ends.iter().position(|end| index < *end).and_then(|i| para.runs.get(i));
            if let Some(link) = run.and_then(|r| r.style.link.as_deref()) {
                let shown: String = link.chars().take(300).collect();
                response.clone().on_hover_text_at_pointer(format!("{shown}\nLinks are not opened from here."));
            }
        }
    }
    LabelSelectionState::label_text_selection(ui, &response, pos, galley, t.text, Stroke::NONE);
}

/// The text and format of one run as it is drawn.
fn section(run: &Run, t: &Tokens) -> (String, TextFormat) {
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
    let (color, background) = colors(s.color, s.background, t);
    format.color = color;
    format.background = background;
    if let Some(picture) = &run.picture {
        let what = match picture {
            Picture::Remote => "Picture not downloaded",
            Picture::Attached(_) | Picture::Inline => "Picture",
        };
        let alt: String = run.text.chars().filter(|c| !c.is_control()).take(120).collect();
        format.color = t.text_faint;
        format.background = Color32::TRANSPARENT;
        format.font_id = font(px.min(BODY_PX));
        return (if alt.is_empty() { format!("[{what}]") } else { format!("[{what}: {alt}]") }, format);
    }
    if s.link.is_some() {
        if s.background.is_none() {
            format.color = t.accent_dark;
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

/// Text and highlight colours for a run. A message is written for a white page; on our pane a
/// colour is kept only if it can be read there, otherwise it is moved toward the pane's text colour.
fn colors(color: Option<rich::Rgb>, background: Option<rich::Rgb>, t: &Tokens) -> (Color32, Color32) {
    let rgb = |c: rich::Rgb| Color32::from_rgb(c[0], c[1], c[2]);
    match background.map(rgb) {
        Some(bg) => {
            let fallback = if luminance(bg) > 0.4 { Color32::from_rgb(0x1A, 0x1A, 0x1A) } else { Color32::WHITE };
            let fg = color.map(rgb).filter(|c| contrast(*c, bg) >= 2.0).unwrap_or(fallback);
            (fg, bg)
        }
        None => {
            let Some(c) = color.map(rgb) else { return (t.text, Color32::TRANSPARENT) };
            let readable =
                [0.0, 0.4, 0.7, 1.0].into_iter().map(|k| c.lerp_to_gamma(t.text, k)).find(|c| contrast(*c, t.surface) >= 3.0).unwrap_or(t.text);
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

fn table(ui: &mut Ui, table: &Table, t: &Tokens, depth: usize) {
    let columns = table.columns();
    let available = ui.available_width();
    if columns == 0 || columns > MAX_COLUMNS || depth >= MAX_TABLE_DEPTH || available / (columns as f32) < 48.0 {
        // Too many columns or too little room: one cell under another, in reading order.
        for cell in table.rows.iter().flatten() {
            blocks(ui, &cell.blocks, t, depth + 1);
        }
        return;
    }
    // Share the width by how much text each column holds.
    let mut weights = vec![8.0f32; columns];
    for row in &table.rows {
        let mut col = 0usize;
        for cell in row {
            let span = usize::from(cell.span.max(1));
            let len = rich::text_len(&cell.blocks, 600) as f32 / span as f32;
            for w in weights.iter_mut().skip(col).take(span) {
                *w = w.max(8.0 + len.sqrt() * 6.0);
            }
            col = col.saturating_add(span);
        }
    }
    let total: f32 = weights.iter().sum();
    let widths: Vec<f32> = weights.iter().map(|w| available * w / total.max(1.0)).collect();

    for (r, row) in table.rows.iter().enumerate() {
        let top = ui.cursor().top();
        let left = ui.cursor().left();
        let mut x = left;
        let mut bottom = top;
        let mut col = 0usize;
        let mut edges: Vec<(f32, f32)> = Vec::with_capacity(row.len());
        for (c, cell) in row.iter().enumerate() {
            let span = usize::from(cell.span.max(1));
            let w: f32 = widths.iter().skip(col).take(span).sum();
            col = col.saturating_add(span);
            if w <= 0.0 {
                continue;
            }
            let inner = Rect::from_min_size(pos2(x + CELL_PAD, top + CELL_PAD - 2.0), vec2((w - CELL_PAD * 2.0).max(8.0), 0.0));
            let mut child = ui.new_child(UiBuilder::new().id_salt(("cell", r, c)).max_rect(inner));
            child.spacing_mut().item_spacing = vec2(0.0, 0.0);
            blocks(&mut child, &cell.blocks, t, depth + 1);
            bottom = bottom.max(child.min_rect().bottom());
            edges.push((x, w));
            x += w;
        }
        bottom += CELL_PAD - 2.0;
        ui.allocate_rect(Rect::from_min_max(pos2(left, top), pos2(left + available, bottom)), Sense::hover());
        if table.border {
            for (x, w) in edges {
                let cell = Rect::from_min_max(pos2(x, top), pos2(x + w, bottom));
                ui.painter().rect_stroke(cell, 0.0, (1.0, t.text_faint), egui::StrokeKind::Middle);
            }
        }
    }
}
