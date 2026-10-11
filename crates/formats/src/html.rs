//! HTML message bodies read into a [`Doc`].
//!
//! The reader is tolerant, because mail HTML breaks every rule, and it is inert: scripts, style
//! sheets, frames, objects and forms are dropped, nothing is fetched, and a link is kept only as
//! the text of its address. Every size, depth and count that comes from the message is capped.
//!
//! Element and attribute meanings follow the HTML Living Standard (sections 4 and 15, "Rendering");
//! only the presentational subset mail uses is understood, and only inline `style` declarations.

use crate::rich::{Align, Block, Cell, Doc, Para, ParaKind, Picture, Rgb, Run, Script, Style, Table, TableWidth};
use mailcraft_model::html::entity;
use std::sync::Arc;

/// HTML beyond this many bytes is not read.
const MAX_INPUT: usize = 4 << 20;
/// Text beyond this many bytes is not kept.
const MAX_TEXT: usize = 2 << 20;
const MAX_BLOCKS: usize = 50_000;
const MAX_RUNS: usize = 200_000;
/// Open elements beyond this depth are not tracked.
const MAX_DEPTH: usize = 256;
/// Tables nested deeper than this are read into the innermost table kept.
const MAX_TABLE_DEPTH: usize = 24;
const MAX_ATTRS: usize = 32;
const MAX_ATTR_LEN: usize = 2048;
const MAX_ALT_LEN: usize = 200;
/// Largest width or height, in pixels, a picture may ask to be shown at.
const MAX_PICTURE_SIDE: f32 = 4096.0;
/// Relative sizes (`em`, `%`, `<small>`, headings) start from this many points when no size is
/// set: the size Outlook Classic shows unstyled HTML text at.
const BASE_PT: f32 = 12.0;

/// Elements whose content is dropped without being read.
const RAW: &[&str] = &["script", "style", "head", "title", "iframe", "object", "embed", "svg", "template", "select", "textarea", "noframes"];
const VOID: &[&str] = &["br", "hr", "img", "meta", "link", "input", "col", "wbr", "area", "base", "param", "source", "track"];
const BLOCK: &[&str] = &[
    "p",
    "div",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "menu",
    "dir",
    "li",
    "dl",
    "dt",
    "dd",
    "blockquote",
    "pre",
    "center",
    "section",
    "article",
    "header",
    "footer",
    "address",
    "figure",
    "figcaption",
    "form",
    "fieldset",
    "main",
    "nav",
    "aside",
    "body",
    "html",
];
/// Blocks that have a margin above and below.
const GAP: &[&str] = &["p", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "pre", "dl", "figure"];

/// Read an HTML body. Never fails: what cannot be understood is skipped.
pub fn parse(html: &str) -> Doc {
    let mut b = Builder { sinks: vec![Vec::new()], ..Builder::default() };
    let mut end = html.len().min(MAX_INPUT);
    while end > 0 && !html.is_char_boundary(end) {
        end -= 1;
    }
    b.doc.truncated = end < html.len();
    let mut rest = html.get(..end).unwrap_or("");
    while !rest.is_empty() && !b.full {
        let Some(lt) = rest.find('<') else {
            b.text(rest);
            break;
        };
        b.text(rest.get(..lt).unwrap_or(""));
        let after = rest.get(lt + 1..).unwrap_or("");
        if let Some(comment) = after.strip_prefix("!--") {
            rest = comment.find("-->").and_then(|i| comment.get(i + 3..)).unwrap_or("");
            continue;
        }
        match after.chars().next() {
            Some('!' | '?') => {
                rest = after.find('>').and_then(|i| after.get(i + 1..)).unwrap_or("");
                continue;
            }
            Some(c) if c.is_ascii_alphabetic() || c == '/' => {}
            _ => {
                b.text("<");
                rest = after;
                continue;
            }
        }
        let Some(gt) = tag_end(after) else { break };
        let tag = after.get(..gt).unwrap_or("");
        rest = after.get(gt + 1..).unwrap_or("");
        let closing = tag.starts_with('/');
        let body = tag.trim_start_matches('/');
        let name_len = body.find(|c: char| !(c.is_ascii_alphanumeric() || c == ':' || c == '-')).unwrap_or(body.len());
        let name = body.get(..name_len).unwrap_or("").to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        if closing {
            b.close_tag(&name);
            continue;
        }
        if RAW.contains(&name.as_str()) {
            let end_tag = format!("</{name}");
            rest = find_ci(rest, &end_tag).and_then(|i| rest.get(i..)).and_then(|from| from.find('>').and_then(|i| from.get(i + 1..))).unwrap_or("");
            continue;
        }
        let attrs = attributes(body.get(name_len..).unwrap_or(""));
        b.open_tag(&name, &attrs, body.trim_end().ends_with('/'));
    }
    b.finish()
}

/// What the current position inherits: the inline style and the block settings.
#[derive(Clone, Default)]
struct Fmt {
    style: Style,
    align: Align,
    indent: u8,
    quote: u8,
    heading: u8,
    pre: bool,
    hidden: bool,
}

struct Open {
    name: String,
    saved: Fmt,
    /// The element pushed a list level.
    list: bool,
    /// The element leaves a margin after it.
    gap: bool,
}

struct TableBuild {
    rows: Vec<Vec<Cell>>,
    border: bool,
    width: TableWidth,
    align: Align,
    /// Length of the element stack just after the table's own entry.
    open_len: usize,
    cell: Option<CellBuild>,
}

struct CellBuild {
    saved: Fmt,
    open_len: usize,
    header: bool,
    span: u16,
    background: Option<Rgb>,
    width: u16,
}

struct ParaHead {
    kind: ParaKind,
    align: Align,
    indent: u8,
    quote: u8,
    gap: bool,
}

#[derive(Default)]
struct Builder {
    doc: Doc,
    /// Block lists being filled: the document, then one per open table cell.
    sinks: Vec<Vec<Block>>,
    open: Vec<Open>,
    tables: Vec<TableBuild>,
    /// Open lists: ordered, and the last number given out.
    lists: Vec<(bool, u32)>,
    fmt: Fmt,
    head: Option<ParaHead>,
    runs: Vec<Run>,
    last_space: bool,
    marker: Option<String>,
    gap: bool,
    blocks: usize,
    run_count: usize,
    bytes: usize,
    full: bool,
}

impl Builder {
    fn finish(mut self) -> Doc {
        while !self.tables.is_empty() {
            self.close_table();
        }
        self.end_para();
        self.doc.blocks = self.sinks.drain(..).next().unwrap_or_default();
        self.doc
    }

    fn push_block(&mut self, block: Block) {
        self.blocks += 1;
        if self.blocks > MAX_BLOCKS {
            self.full = true;
            self.doc.truncated = true;
            return;
        }
        match self.sinks.last_mut() {
            Some(sink) => sink.push(block),
            None => self.sinks.push(vec![block]),
        }
    }

    /// Elements below this index were opened outside the current cell or table and cannot be
    /// closed from inside it.
    fn floor(&self) -> usize {
        match self.tables.last() {
            Some(t) => t.cell.as_ref().map_or(t.open_len, |c| c.open_len),
            None => 0,
        }
    }

    /// Text and pictures directly inside a table, outside any cell, get a cell of their own.
    fn ensure_cell(&mut self) {
        if self.tables.last().is_some_and(|t| t.cell.is_none()) {
            self.open_cell(false, 1);
        }
    }

    fn start_para(&mut self) {
        if self.head.is_some() {
            return;
        }
        let kind = match self.marker.take() {
            Some(marker) => ParaKind::Item(marker),
            None if self.fmt.heading > 0 => ParaKind::Heading(self.fmt.heading),
            None if self.fmt.pre => ParaKind::Pre,
            None => ParaKind::Body,
        };
        self.head =
            Some(ParaHead { kind, align: self.fmt.align, indent: self.fmt.indent, quote: self.fmt.quote, gap: std::mem::take(&mut self.gap) });
        self.last_space = true;
    }

    fn end_para(&mut self) {
        let Some(head) = self.head.take() else { return };
        let mut runs = std::mem::take(&mut self.runs);
        self.last_space = true;
        // A line break just before the end of a block does not make a line of its own.
        let mut newline_left = true;
        while let Some(last) = runs.last_mut() {
            if last.picture.is_some() {
                break;
            }
            let mut t = last.text.trim_end_matches(' ');
            if newline_left && let Some(s) = t.strip_suffix('\n') {
                t = s.trim_end_matches(' ');
                newline_left = false;
            }
            let n = t.len();
            last.text.truncate(n);
            if last.text.is_empty() {
                runs.pop();
            } else {
                break;
            }
        }
        if runs.is_empty() {
            if newline_left {
                return;
            }
            // Only a line break: an empty line.
            runs.push(Run { text: "\u{a0}".to_string(), ..Run::default() });
        }
        self.push_block(Block::Para(Para { runs, kind: head.kind, align: head.align, indent: head.indent, quote: head.quote, gap: head.gap }));
    }

    fn text(&mut self, raw: &str) {
        if self.fmt.hidden || raw.is_empty() {
            return;
        }
        let mut buf = String::new();
        let mut last_space = self.head.is_none() || self.last_space;
        let mut rest = raw;
        while let Some(c) = rest.chars().next() {
            if c == '&'
                && let Some((decoded, len)) = entity(rest)
            {
                for d in decoded.chars().filter(|d| !d.is_control()) {
                    buf.push(d);
                    last_space = false;
                }
                rest = rest.get(len..).unwrap_or("");
                continue;
            }
            rest = rest.get(c.len_utf8()..).unwrap_or("");
            if self.fmt.pre {
                match c {
                    '\n' => buf.push('\n'),
                    '\t' => buf.push_str("    "),
                    c if c.is_control() => {}
                    c => buf.push(c),
                }
                last_space = false;
            } else if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c') {
                if !last_space {
                    buf.push(' ');
                    last_space = true;
                }
            } else if !c.is_control() {
                buf.push(c);
                last_space = false;
            }
        }
        if buf.is_empty() {
            return;
        }
        if self.tables.last().is_some_and(|t| t.cell.is_none()) {
            if buf.trim().is_empty() {
                return;
            }
            self.ensure_cell();
        }
        self.append(buf, None);
        self.last_space = last_space;
    }

    fn append(&mut self, text: String, picture: Option<Picture>) {
        self.append_sized(text, picture, (0, 0));
    }

    fn append_sized(&mut self, text: String, picture: Option<Picture>, extent: (u16, u16)) {
        self.bytes = self.bytes.saturating_add(text.len());
        if self.bytes > MAX_TEXT || self.run_count >= MAX_RUNS {
            self.full = true;
            self.doc.truncated = true;
            return;
        }
        self.start_para();
        if picture.is_none()
            && let Some(last) = self.runs.last_mut()
            && last.picture.is_none()
            && last.style == self.fmt.style
        {
            last.text.push_str(&text);
            return;
        }
        self.run_count += 1;
        self.runs.push(Run { text, style: self.fmt.style.clone(), picture, extent });
    }

    fn line_break(&mut self) {
        if self.fmt.hidden || self.tables.last().is_some_and(|t| t.cell.is_none()) {
            return;
        }
        self.append("\n".to_string(), None);
        self.last_space = true;
    }

    fn picture(&mut self, attrs: &[(String, String)]) {
        if self.fmt.hidden {
            return;
        }
        let src = attr(attrs, "src").unwrap_or("").trim();
        let lower: String = src.chars().take(8).collect::<String>().to_ascii_lowercase();
        let picture = if let Some(cid) = lower.strip_prefix("cid:").and(src.get(4..)) {
            Picture::Attached(cid.to_string())
        } else if lower.starts_with("data:") {
            Picture::Inline
        } else {
            if !src.is_empty() {
                self.doc.remote_blocked += 1;
            }
            Picture::Remote
        };
        // One-pixel pictures are trackers or spacers; there is nothing to show for them.
        let asked = |name: &str| attr(attrs, name).and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok()).filter(|v| v.is_finite());
        let tiny = |name: &str| asked(name).is_some_and(|v| v <= 2.0);
        if src.is_empty() || tiny("width") || tiny("height") {
            return;
        }
        // A percentage is not a pixel size: it is left to the reader.
        let pixels = |name: &str| asked(name).map_or(0, |v| v.round().clamp(0.0, MAX_PICTURE_SIDE) as u16);
        let extent = (pixels("width"), pixels("height"));
        let alt: String = attr(attrs, "alt").unwrap_or("").chars().filter(|c| !c.is_control()).take(MAX_ALT_LEN).collect();
        self.ensure_cell();
        self.append_sized(alt.trim().to_string(), Some(picture), extent);
        self.last_space = false;
    }

    fn open_tag(&mut self, name: &str, attrs: &[(String, String)], self_closing: bool) {
        match name {
            "br" => return self.line_break(),
            "hr" => {
                self.end_para();
                if !self.fmt.hidden && !self.tables.last().is_some_and(|t| t.cell.is_none()) {
                    self.push_block(Block::Rule);
                }
                self.gap = true;
                return;
            }
            "img" => return self.picture(attrs),
            "table" => return self.open_table(attrs),
            "tr" => {
                if let Some(t) = self.tables.last()
                    && (t.cell.is_some() || t.rows.last().is_none_or(|r| !r.is_empty()))
                {
                    self.close_cell();
                    if let Some(t) = self.tables.last_mut() {
                        t.rows.push(Vec::new());
                    }
                }
                return;
            }
            "td" | "th" => {
                if self.tables.is_empty() {
                    self.end_para();
                    return;
                }
                self.close_cell();
                let span = attr(attrs, "colspan").and_then(|v| v.trim().parse::<u16>().ok()).unwrap_or(1).clamp(1, 64);
                self.open_cell(name == "th", span);
                if name == "th" {
                    self.fmt.style.bold = true;
                }
                self.common(name, attrs);
                // A background set on the cell, or inherited by it, is the cell's and not its text's.
                let background = self.fmt.style.background.take();
                let width = attr(attrs, "width").and_then(pixels).unwrap_or(0);
                if let Some(cell) = self.tables.last_mut().and_then(|t| t.cell.as_mut()) {
                    cell.background = background;
                    cell.width = width;
                }
                return;
            }
            "tbody" | "thead" | "tfoot" | "caption" | "colgroup" => return,
            _ => {}
        }
        if VOID.contains(&name) || self_closing {
            return;
        }
        let block = BLOCK.contains(&name);
        if block {
            // A block ends an open paragraph element (HTML 13.1.2.4, optional end tags).
            let floor = self.floor();
            if let Some(i) = self.open.iter().rposition(|o| o.name == "p").filter(|i| *i >= floor)
                && self.open.get(i + 1..).is_some_and(|above| above.iter().all(|o| !BLOCK.contains(&o.name.as_str())))
            {
                self.end_para();
                self.pop_to(i);
            }
            if name == "li" {
                let list = self.open.iter().rposition(|o| o.list).map_or(floor, |i| (i + 1).max(floor));
                if let Some(i) = self.open.iter().rposition(|o| o.name == "li").filter(|i| *i >= list) {
                    self.end_para();
                    self.pop_to(i);
                }
            }
            self.end_para();
        }
        if self.open.len() >= MAX_DEPTH {
            return;
        }
        self.open.push(Open { name: name.to_string(), saved: self.fmt.clone(), list: false, gap: false });
        let mut gap = GAP.contains(&name);
        let style = &mut self.fmt.style;
        match name {
            "b" | "strong" => style.bold = true,
            "i" | "em" | "cite" | "var" | "dfn" => style.italic = true,
            "u" | "ins" => style.underline = true,
            "s" | "strike" | "del" => style.strike = true,
            "code" | "tt" | "kbd" | "samp" => style.mono = true,
            "sup" => style.script = Script::Super,
            "sub" => style.script = Script::Sub,
            "small" => style.size = Some((style.size.unwrap_or(BASE_PT) * 0.83).clamp(6.0, 72.0)),
            "big" => style.size = Some((style.size.unwrap_or(BASE_PT) * 1.2).clamp(6.0, 72.0)),
            "mark" => style.background = Some([255, 235, 120]),
            "a" => {
                if let Some(href) = attr(attrs, "href") {
                    let href: String = href.trim().chars().filter(|c| !c.is_control()).collect();
                    if !href.is_empty() && !href.starts_with('#') {
                        style.link = Some(Arc::from(href));
                    }
                }
            }
            "font" => {
                if let Some(c) = attr(attrs, "color").and_then(color) {
                    style.color = Some(c);
                }
                if let Some(size) = attr(attrs, "size").and_then(font_size_attr) {
                    style.size = Some(size);
                }
                if attr(attrs, "face").is_some_and(is_mono_family) {
                    style.mono = true;
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name.as_bytes().get(1).map_or(1, |b| b.saturating_sub(b'0')).clamp(1, 6);
                self.fmt.heading = level;
                style.bold = true;
                style.size = Some(BASE_PT * [2.0, 1.5, 1.17, 1.0, 0.83, 0.67].get(usize::from(level) - 1).copied().unwrap_or(1.0));
            }
            "center" => self.fmt.align = Align::Center,
            // A quotation of earlier mail is marked as one (a bar); any other is only set in.
            "blockquote" => {
                let has = |name: &str, what: &str| attr(attrs, name).is_some_and(|v| v.to_ascii_lowercase().contains(what));
                if has("type", "cite") || has("style", "border-left") || has("class", "quote") {
                    self.fmt.quote = self.fmt.quote.saturating_add(1).min(8);
                } else {
                    self.fmt.indent = self.fmt.indent.saturating_add(1).min(12);
                }
            }
            "dd" => self.fmt.indent = self.fmt.indent.saturating_add(1).min(12),
            "pre" | "xmp" | "listing" | "plaintext" => {
                self.fmt.pre = true;
                style.mono = true;
            }
            "ul" | "ol" | "menu" | "dir" => {
                let start = attr(attrs, "start").and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(1);
                gap = self.lists.is_empty();
                self.lists.push((name == "ol", start.saturating_sub(1)));
                self.fmt.indent = self.fmt.indent.saturating_add(1).min(12);
                if let Some(o) = self.open.last_mut() {
                    o.list = true;
                }
            }
            "li" => {
                self.marker = Some(match self.lists.last_mut() {
                    Some((true, n)) => {
                        *n = n.saturating_add(1);
                        format!("{n}.")
                    }
                    _ => "•".to_string(),
                });
            }
            "body" => {
                if let Some(c) = attr(attrs, "text").and_then(color) {
                    style.color = Some(c);
                }
            }
            _ => {}
        }
        let margin = self.common(name, attrs);
        // Word-processor paragraphs ("MsoNormal" and friends) are written with no margins.
        if margin == Some(false) || (name == "p" && attr(attrs, "class").is_some_and(|c| c.trim_start().to_ascii_lowercase().starts_with("mso"))) {
            gap = false;
        }
        if gap {
            self.gap = true;
        }
        if let Some(o) = self.open.last_mut() {
            o.gap = gap;
        }
    }

    /// Attributes any element may carry. Returns whether the style asks for a margin.
    fn common(&mut self, name: &str, attrs: &[(String, String)]) -> Option<bool> {
        if let Some(a) = attr(attrs, "align").and_then(align)
            && name != "table"
            && name != "img"
        {
            self.fmt.align = a;
        }
        if let Some(c) = attr(attrs, "bgcolor").and_then(color) {
            self.fmt.style.background = page_or(c);
        }
        if attr(attrs, "background").is_some_and(is_remote) {
            self.doc.remote_blocked += 1;
        }
        if attr(attrs, "hidden").is_some() {
            self.fmt.hidden = true;
        }
        attr(attrs, "style").and_then(|css| self.css(css))
    }

    /// Inline style declarations (CSS 2.1 and CSS Text Decoration property names).
    fn css(&mut self, css: &str) -> Option<bool> {
        let mut zero_top = None;
        let mut zero_bottom = None;
        for decl in css.split(';').take(64) {
            let Some((prop, value)) = decl.split_once(':') else { continue };
            let prop = prop.trim().to_ascii_lowercase();
            let value = value.trim().to_ascii_lowercase();
            let v = value.trim_end_matches("!important").trim();
            let style = &mut self.fmt.style;
            match prop.as_str() {
                "color" => {
                    if let Some(c) = color(v) {
                        style.color = Some(c);
                    }
                }
                "background-color" | "background" => {
                    if v.contains("url(") && is_remote(v.split("url(").nth(1).unwrap_or("").trim_start_matches(['"', '\'', ' '])) {
                        self.doc.remote_blocked += 1;
                    }
                    if let Some(c) = color(v).or_else(|| v.split_whitespace().find_map(color)) {
                        style.background = page_or(c);
                    }
                }
                "font-weight" => match v {
                    "bold" | "bolder" => style.bold = true,
                    "normal" | "lighter" => style.bold = false,
                    _ => {
                        if let Ok(n) = v.parse::<u32>() {
                            style.bold = n >= 600;
                        }
                    }
                },
                "font-style" => style.italic = v.starts_with("italic") || v.starts_with("oblique"),
                "text-decoration" | "text-decoration-line" => {
                    if v.contains("none") {
                        style.underline = false;
                        style.strike = false;
                    }
                    if v.contains("underline") {
                        style.underline = true;
                    }
                    if v.contains("line-through") {
                        style.strike = true;
                    }
                }
                "font-size" => match font_size(v, style.size.unwrap_or(BASE_PT)) {
                    Some(pt) if pt < 1.0 => self.fmt.hidden = true,
                    Some(pt) => style.size = Some(pt.clamp(6.0, 72.0)),
                    None => {}
                },
                "font-family" => {
                    if is_mono_family(v) {
                        style.mono = true;
                    }
                }
                "text-align" => {
                    if let Some(a) = align(v) {
                        self.fmt.align = a;
                    }
                }
                "display" if v.starts_with("none") => self.fmt.hidden = true,
                "visibility" if v.starts_with("hidden") => self.fmt.hidden = true,
                "mso-hide" if v.starts_with("all") => self.fmt.hidden = true,
                "white-space" => self.fmt.pre = v.starts_with("pre"),
                "margin" => {
                    let parts: Vec<&str> = v.split_whitespace().take(4).collect();
                    zero_top = parts.first().map(|p| is_zero(p));
                    zero_bottom = parts.get(2).or(parts.first()).map(|p| is_zero(p));
                }
                "margin-top" => zero_top = Some(is_zero(v)),
                "margin-bottom" => zero_bottom = Some(is_zero(v)),
                _ => {}
            }
        }
        match (zero_top, zero_bottom) {
            (Some(true), Some(true)) => Some(false),
            (None, None) => None,
            _ => Some(true),
        }
    }

    fn close_tag(&mut self, name: &str) {
        match name {
            "table" => {
                if self.tables.is_empty() {
                    self.end_para();
                } else {
                    self.close_table();
                }
                return;
            }
            "tr" | "td" | "th" => {
                if self.tables.is_empty() {
                    self.end_para();
                } else {
                    self.close_cell();
                }
                return;
            }
            "tbody" | "thead" | "tfoot" | "caption" | "colgroup" => return,
            // A stray </br> stands for a line break (HTML 13.2.6.4.7).
            "br" => return self.line_break(),
            _ => {}
        }
        let floor = self.floor();
        let Some(i) = self.open.iter().rposition(|o| o.name == name).filter(|i| *i >= floor) else {
            if BLOCK.contains(&name) {
                self.end_para();
            }
            return;
        };
        let block = BLOCK.contains(&name);
        if block {
            self.end_para();
        }
        let gap = self.open.get(i).is_some_and(|o| o.gap);
        self.pop_to(i);
        if gap {
            self.gap = true;
        }
    }

    /// Close every element from index `len` up, restoring what each one changed.
    fn pop_to(&mut self, len: usize) {
        while self.open.len() > len {
            let Some(o) = self.open.pop() else { break };
            if o.list {
                self.lists.pop();
            }
            self.fmt = o.saved;
        }
    }

    fn open_table(&mut self, attrs: &[(String, String)]) {
        self.end_para();
        if self.tables.len() >= MAX_TABLE_DEPTH || self.open.len() >= MAX_DEPTH {
            return;
        }
        self.ensure_cell();
        self.open.push(Open { name: "table".to_string(), saved: self.fmt.clone(), list: false, gap: false });
        // Alignment of the table itself is not inherited by its text.
        self.fmt.align = Align::Left;
        let border = attr(attrs, "border").is_some_and(|v| v.trim().trim_end_matches("px").parse::<f32>().is_ok_and(|n| n > 0.0));
        self.common("table", attrs);
        let css = attr(attrs, "style").unwrap_or("");
        let auto = |prop: &str| css_value(css, prop).is_some_and(|v| v.split_whitespace().any(|part| part == "auto"));
        let table_align = match attr(attrs, "align").and_then(align) {
            Some(a) => a,
            None if auto("margin") || (auto("margin-left") && auto("margin-right")) => Align::Center,
            None => Align::Left,
        };
        self.tables.push(TableBuild {
            rows: Vec::new(),
            border,
            width: table_width(attrs),
            align: table_align,
            open_len: self.open.len(),
            cell: None,
        });
    }

    fn open_cell(&mut self, header: bool, span: u16) {
        self.end_para();
        let saved = self.fmt.clone();
        let open_len = self.open.len();
        let Some(t) = self.tables.last_mut() else { return };
        if t.rows.is_empty() {
            t.rows.push(Vec::new());
        }
        t.cell = Some(CellBuild { saved, open_len, header, span, background: None, width: 0 });
        self.sinks.push(Vec::new());
        self.gap = false;
    }

    fn close_cell(&mut self) {
        let Some(cell) = self.tables.last_mut().and_then(|t| t.cell.take()) else { return };
        self.end_para();
        self.pop_to(cell.open_len);
        self.fmt = cell.saved;
        self.marker = None;
        self.gap = false;
        // The document's own list is never popped: a cell always has a sink above it.
        let blocks = if self.sinks.len() > 1 { self.sinks.pop().unwrap_or_default() } else { Vec::new() };
        if let Some(row) = self.tables.last_mut().and_then(|t| t.rows.last_mut()) {
            row.push(Cell { blocks, header: cell.header, span: cell.span, background: cell.background, width: cell.width });
        }
    }

    fn close_table(&mut self) {
        self.close_cell();
        let Some(t) = self.tables.pop() else { return };
        self.pop_to(t.open_len.saturating_sub(1));
        let mut rows: Vec<Vec<Cell>> = t.rows.into_iter().filter(|r| r.iter().any(|c| !c.blocks.is_empty())).collect();
        if rows.is_empty() {
            return;
        }
        // A width, a place on the page or a filled cell is something to draw; without any of them
        // a table with one column is only layout, and its content reads as ordinary blocks.
        let shaped = matches!(t.width, TableWidth::Pixels(_)) || t.align != Align::Left || rows.iter().flatten().any(|c| c.background.is_some());
        if !shaped && rows.iter().all(|r| r.iter().filter(|c| !c.blocks.is_empty()).count() <= 1) {
            for cell in rows.drain(..).flatten() {
                for block in cell.blocks {
                    self.push_block(block);
                }
            }
        } else {
            self.push_block(Block::Table(Table { rows, border: t.border, width: t.width, align: t.align }));
        }
        self.gap = true;
    }
}

/// Index of the `>` that ends a tag, skipping quoted attribute values.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = 0u8;
    let mut after_eq = false;
    for (i, b) in s.bytes().enumerate() {
        if quote != 0 {
            if b == quote {
                quote = 0;
            }
            continue;
        }
        match b {
            b'>' => return Some(i),
            b'"' | b'\'' if after_eq => quote = b,
            b'=' => {
                after_eq = true;
                continue;
            }
            b' ' | b'\t' | b'\n' | b'\r' => continue,
            _ => {}
        }
        after_eq = false;
    }
    s.find('>')
}

/// Attributes of a tag: lower-case names, entity-decoded values.
fn attributes(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    while out.len() < MAX_ATTRS {
        rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
        if rest.is_empty() {
            break;
        }
        let name_len = rest.find(|c: char| c.is_ascii_whitespace() || c == '=' || c == '/').unwrap_or(rest.len());
        let name = rest.get(..name_len).unwrap_or("").to_ascii_lowercase();
        rest = rest.get(name_len..).unwrap_or("").trim_start();
        let mut value = "";
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            match after.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let inner = after.get(1..).unwrap_or("");
                    let end = inner.find(q).unwrap_or(inner.len());
                    value = inner.get(..end).unwrap_or("");
                    rest = inner.get(end + 1..).unwrap_or("");
                }
                _ => {
                    let end = after.find(|c: char| c.is_ascii_whitespace()).unwrap_or(after.len());
                    value = after.get(..end).unwrap_or("");
                    rest = after.get(end..).unwrap_or("");
                }
            }
        }
        if name.is_empty() {
            // A stray "=" or quote: step over one character so the loop always moves on.
            let step = rest.chars().next().map_or(0, char::len_utf8);
            rest = rest.get(step..).unwrap_or("");
            continue;
        }
        out.push((name, decode(value)));
    }
    out
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// Entity-decode an attribute value, keeping at most `MAX_ATTR_LEN` characters.
fn decode(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    let mut count = 0;
    while let Some(c) = rest.chars().next() {
        if count >= MAX_ATTR_LEN {
            break;
        }
        count += 1;
        if c == '&'
            && let Some((decoded, len)) = entity(rest)
        {
            out.push_str(&decoded);
            rest = rest.get(len..).unwrap_or("");
            continue;
        }
        out.push(c);
        rest = rest.get(c.len_utf8()..).unwrap_or("");
    }
    out
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h.get(i..i + n.len()).is_some_and(|w| w.eq_ignore_ascii_case(n)))
}

fn is_remote(url: &str) -> bool {
    let u: String = url.trim().chars().take(8).collect::<String>().to_ascii_lowercase();
    u.starts_with("http:") || u.starts_with("https:") || u.starts_with("//") || u.starts_with("ftp:")
}

fn is_mono_family(v: &str) -> bool {
    let v = v.to_ascii_lowercase();
    ["monospace", "courier", "consolas", "menlo", "monaco", "lucida console"].iter().any(|m| v.contains(m))
}

fn is_zero(v: &str) -> bool {
    v.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%').parse::<f32>().is_ok_and(|n| n == 0.0)
}

fn align(v: &str) -> Option<Align> {
    match v.trim().to_ascii_lowercase().as_str() {
        "left" | "justify" | "start" => Some(Align::Left),
        "center" | "middle" => Some(Align::Center),
        "right" | "end" => Some(Align::Right),
        _ => None,
    }
}

/// A length in pixels, as an attribute or a CSS value gives it. A percentage is not one.
fn pixels(v: &str) -> Option<u16> {
    let n = v.trim().trim_end_matches("px").trim_end().parse::<f32>().ok().filter(|n| n.is_finite() && *n >= 1.0)?;
    Some(n.round().min(MAX_PICTURE_SIDE) as u16)
}

/// The value of one property in an inline style, lower case.
fn css_value(css: &str, prop: &str) -> Option<String> {
    let (_, value) = css.split(';').take(64).filter_map(|d| d.split_once(':')).find(|(p, _)| p.trim().eq_ignore_ascii_case(prop))?;
    Some(value.to_ascii_lowercase().trim().trim_end_matches("!important").trim().to_string())
}

/// The width a table asks for: a limit in pixels first, then its width.
fn table_width(attrs: &[(String, String)]) -> TableWidth {
    let css = attr(attrs, "style").unwrap_or("");
    let asked = [css_value(css, "max-width"), css_value(css, "width"), attr(attrs, "width").map(str::to_string)];
    for v in asked.into_iter().flatten() {
        match v.trim().strip_suffix('%') {
            Some(share) if share.trim().parse::<f32>().is_ok_and(|p| p >= 100.0) => return TableWidth::Fill,
            Some(_) => {}
            None => {
                if let Some(px) = pixels(&v) {
                    return TableWidth::Pixels(px);
                }
            }
        }
    }
    TableWidth::Content
}

/// A near-white background is the page itself, not a highlight.
fn page_or(c: Rgb) -> Option<Rgb> {
    let [r, g, b] = c;
    let luma = (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000;
    (luma < 236).then_some(c)
}

/// A CSS font size in points. Zero means the text is hidden.
fn font_size(v: &str, current: f32) -> Option<f32> {
    let v = v.trim();
    let keyword = match v {
        "xx-small" => Some(6.75),
        "x-small" => Some(7.5),
        "small" => Some(9.75),
        "medium" => Some(12.0),
        "large" => Some(13.5),
        "x-large" => Some(18.0),
        "xx-large" => Some(24.0),
        "smaller" => Some(current * 0.83),
        "larger" => Some(current * 1.2),
        _ => None,
    };
    if keyword.is_some() {
        return keyword;
    }
    let split = v.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+')).unwrap_or(v.len());
    let n = v.get(..split)?.parse::<f32>().ok()?;
    if !n.is_finite() || n < 0.0 {
        return None;
    }
    let pt = match v.get(split..).unwrap_or("").trim() {
        "pt" => n,
        "px" | "" => n * 0.75,
        "em" | "rem" => n * current,
        "%" => n / 100.0 * current,
        "pc" => n * 12.0,
        "in" => n * 72.0,
        "cm" => n * 28.35,
        "mm" => n * 2.835,
        _ => return None,
    };
    pt.is_finite().then_some(pt)
}

/// `<font size>`: 1 to 7, or relative to 3.
fn font_size_attr(v: &str) -> Option<f32> {
    let v = v.trim();
    let n = v.parse::<i32>().ok()?;
    let step = if v.starts_with(['+', '-']) { n.saturating_add(3) } else { n }.clamp(1, 7);
    [7.5, 10.0, 12.0, 13.5, 18.0, 24.0, 36.0].get(usize::try_from(step - 1).ok()?).copied()
}

/// A CSS colour: `#rgb`, `#rrggbb`, `rgb()`/`rgba()` or a common name.
fn color(v: &str) -> Option<Rgb> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(hex) = v.strip_prefix('#') {
        if !hex.is_ascii() {
            return None;
        }
        let nibble = |i: usize| hex.get(i..i + 1).and_then(|d| u8::from_str_radix(d, 16).ok());
        let byte = |i: usize| hex.get(i..i + 2).and_then(|d| u8::from_str_radix(d, 16).ok());
        return match hex.len() {
            3 | 4 => Some([nibble(0)? * 17, nibble(1)? * 17, nibble(2)? * 17]),
            6 | 8 => Some([byte(0)?, byte(2)?, byte(4)?]),
            _ => None,
        };
    }
    if let Some(args) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")) {
        let mut parts = args.trim_end_matches(')').split(|c: char| c == ',' || c == '/' || c.is_whitespace()).filter(|p| !p.is_empty());
        let mut channel = || {
            let p = parts.next()?;
            let n = match p.strip_suffix('%') {
                Some(pc) => pc.parse::<f32>().ok()? * 2.55,
                None => p.parse::<f32>().ok()?,
            };
            // `as` saturates, and NaN becomes 0.
            n.is_finite().then_some(n.clamp(0.0, 255.0) as u8)
        };
        return Some([channel()?, channel()?, channel()?]);
    }
    Some(match v.as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "blue" => [0, 0, 255],
        "gray" | "grey" => [128, 128, 128],
        "darkgray" | "darkgrey" => [169, 169, 169],
        "lightgray" | "lightgrey" => [211, 211, 211],
        "dimgray" | "dimgrey" => [105, 105, 105],
        "silver" => [192, 192, 192],
        "maroon" => [128, 0, 0],
        "navy" => [0, 0, 128],
        "purple" => [128, 0, 128],
        "teal" => [0, 128, 128],
        "olive" => [128, 128, 0],
        "lime" => [0, 255, 0],
        "aqua" | "cyan" => [0, 255, 255],
        "fuchsia" | "magenta" => [255, 0, 255],
        "yellow" => [255, 255, 0],
        "orange" => [255, 165, 0],
        "pink" => [255, 192, 203],
        "brown" => [165, 42, 42],
        "gold" => [255, 215, 0],
        "darkred" => [139, 0, 0],
        "darkgreen" => [0, 100, 0],
        "darkblue" => [0, 0, 139],
        "steelblue" => [70, 130, 180],
        "royalblue" => [65, 105, 225],
        "crimson" => [220, 20, 60],
        "tomato" => [255, 99, 71],
        "indigo" => [75, 0, 130],
        "violet" => [238, 130, 238],
        _ => return None,
    })
}
