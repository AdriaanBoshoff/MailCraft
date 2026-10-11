//! RTF message bodies → [`Doc`]. An RTF body that encapsulates HTML or plain text ([MS-OXRTFEX])
//! is read as what it wraps; ordinary RTF keeps its character styles, colours, alignment,
//! indents, links and tables. Pictures and embedded objects are never decoded here.

use std::collections::HashSet;
use std::sync::Arc;

use mailcraft_model::rtf::{Event, Reader, Scope, Wrapped, encapsulated, is_skipped_destination};

use crate::html;
use crate::rich::{Align, Block, Cell, Doc, Para, ParaKind, Picture, Rgb, Run, Script, Style, Table};

const MAX_BLOCKS: usize = 50_000;
const MAX_RUNS: usize = 200_000;
const MAX_COLORS: usize = 1024;
const MAX_FONTS: usize = 4096;
const MAX_LINK: usize = 2048;

pub fn parse(rtf: &[u8]) -> Doc {
    match encapsulated(rtf) {
        Some(Wrapped::Html(h)) => html::parse(&h),
        Some(Wrapped::Text(t)) => plain(&t),
        None => formatted(rtf),
    }
}

/// Plain text as a document: one paragraph per line.
fn plain(text: &str) -> Doc {
    let mut doc = Doc::default();
    for line in text.trim_end().lines() {
        if doc.blocks.len() >= MAX_BLOCKS {
            doc.truncated = true;
            break;
        }
        doc.blocks
            .push(Block::Para(Para { runs: vec![text_run(if line.is_empty() { "\u{a0}" } else { line }, Style::default())], ..Para::default() }));
    }
    doc
}

fn text_run(text: &str, style: Style) -> Run {
    Run { text: text.to_string(), style, ..Run::default() }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Dest {
    #[default]
    Body,
    Skip,
    Fonts,
    Colors,
    /// A field's instruction, e.g. `HYPERLINK "address"`.
    Instruction,
}

#[derive(Clone, Default)]
struct Chars {
    style: Style,
    hidden: bool,
    dest: Dest,
    star: bool,
    // Paragraph settings are saved and restored by groups like character ones.
    align: Align,
    indent: u8,
    in_table: bool,
}

#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    /// The table being read: finished rows, the open row's cells, the open cell's blocks.
    rows: Vec<Vec<Cell>>,
    row: Vec<Cell>,
    cell: Vec<Block>,
    border: bool,
    runs: Vec<Run>,
    colors: Vec<Option<Rgb>>,
    rgb: Option<Rgb>,
    mono_fonts: HashSet<i32>,
    declaring: i32,
    instruction: String,
    count: usize,
    truncated: bool,
}

fn formatted(rtf: &[u8]) -> Doc {
    let mut b = Builder::default();
    let mut scope = Scope::new(Chars::default());
    let mut reader = Reader::new(rtf);
    for event in reader.by_ref() {
        if b.truncated {
            break;
        }
        match event {
            Event::Open => {
                scope.open();
                scope.cur.star = false;
            }
            Event::Close => {
                let was = scope.cur.dest;
                scope.close();
                if was == Dest::Instruction && scope.cur.dest != Dest::Instruction {
                    scope.cur.style.link = link(&b.instruction);
                    b.instruction.clear();
                }
            }
            Event::Word("*", _) => scope.cur.star = true,
            Event::Word(word, param) => {
                let star = std::mem::take(&mut scope.cur.star);
                match scope.cur.dest {
                    Dest::Body => b.word(&mut scope.cur, word, param, star),
                    Dest::Fonts => match word {
                        "f" => b.declaring = param.unwrap_or(0),
                        "fmodern" if b.mono_fonts.len() < MAX_FONTS => {
                            b.mono_fonts.insert(b.declaring);
                        }
                        _ => {}
                    },
                    Dest::Colors => {
                        let part = match word {
                            "red" => 0,
                            "green" => 1,
                            "blue" => 2,
                            _ => continue,
                        };
                        let rgb = b.rgb.get_or_insert([0; 3]);
                        if let Some(slot) = rgb.get_mut(part) {
                            *slot = u8::try_from(param.unwrap_or(0).clamp(0, 255)).unwrap_or(0);
                        }
                    }
                    Dest::Skip | Dest::Instruction => {}
                }
            }
            Event::Text(text) => {
                scope.cur.star = false;
                match scope.cur.dest {
                    Dest::Body if !scope.cur.hidden => b.text(&text, &scope.cur.style),
                    Dest::Colors => {
                        // Each ";" ends one colour; one with no numbers is "automatic".
                        for _ in text.matches(';') {
                            let rgb = b.rgb.take();
                            if b.colors.len() < MAX_COLORS {
                                b.colors.push(rgb);
                            }
                        }
                    }
                    Dest::Instruction if b.instruction.len() < MAX_LINK * 2 => b.instruction.push_str(&text),
                    _ => {}
                }
            }
        }
    }
    b.end_para(&scope.cur, false, false);
    b.end_table();
    // Empty paragraphs at the end are the writer's, not the reader's.
    while matches!(b.blocks.last(), Some(Block::Para(p)) if p.text().trim().is_empty() && p.runs.iter().all(|r| r.picture.is_none())) {
        b.blocks.pop();
    }
    Doc { blocks: b.blocks, truncated: b.truncated || reader.truncated(), remote_blocked: 0 }
}

impl Builder {
    fn word(&mut self, cur: &mut Chars, word: &str, param: Option<i32>, star: bool) {
        let on = param != Some(0);
        let color = |colors: &[Option<Rgb>]| usize::try_from(param.unwrap_or(0)).ok().and_then(|i| colors.get(i).copied()).flatten();
        match word {
            "fonttbl" => cur.dest = Dest::Fonts,
            "colortbl" => cur.dest = Dest::Colors,
            "fldinst" => {
                cur.dest = Dest::Instruction;
                self.instruction.clear();
            }
            // The picture a reader of this RTF version shows; its bytes are skipped.
            "shppict" => {}
            "pict" => {
                cur.dest = Dest::Skip;
                if !cur.hidden {
                    self.push(Run { style: cur.style.clone(), picture: Some(Picture::Inline), ..Run::default() });
                }
            }
            "b" => cur.style.bold = on,
            "i" => cur.style.italic = on,
            "ul" | "uld" | "uldb" | "ulw" | "uldash" | "ulwave" => cur.style.underline = on,
            "ulnone" => cur.style.underline = false,
            "strike" | "striked" => cur.style.strike = on,
            "v" => cur.hidden = on,
            // Half points.
            "fs" => cur.style.size = param.map(|p| f32::from(u16::try_from(p.clamp(2, 800)).unwrap_or(24)) / 2.0),
            "cf" => cur.style.color = color(&self.colors),
            "highlight" | "cb" | "chcbpat" => cur.style.background = color(&self.colors),
            "super" => cur.style.script = Script::Super,
            "sub" => cur.style.script = Script::Sub,
            "nosupersub" => cur.style.script = Script::None,
            "f" => cur.style.mono = self.mono_fonts.contains(&param.unwrap_or(0)),
            "plain" => {
                cur.style = Style { link: cur.style.link.take(), ..Style::default() };
                cur.hidden = false;
            }
            "par" | "sect" | "page" => self.end_para(cur, cur.in_table, true),
            "line" => self.text("\n", &cur.style),
            "pard" => {
                cur.align = Align::Left;
                cur.indent = 0;
                cur.in_table = false;
            }
            "ql" | "qj" => cur.align = Align::Left,
            "qc" => cur.align = Align::Center,
            "qr" => cur.align = Align::Right,
            // Twips; half an inch is one level.
            "li" => cur.indent = u8::try_from((param.unwrap_or(0).max(0) / 720).min(8)).unwrap_or(0),
            "intbl" => cur.in_table = true,
            "cell" => {
                self.end_para(cur, true, false);
                let blocks = std::mem::take(&mut self.cell);
                self.row.push(Cell { blocks, header: false, span: 1 });
            }
            "row" => self.end_row(),
            "clbrdrt" | "clbrdrl" | "clbrdrb" | "clbrdrr" => self.border = true,
            _ if star || is_skipped_destination(word) => cur.dest = Dest::Skip,
            _ => {}
        }
    }

    fn text(&mut self, text: &str, style: &Style) {
        match self.runs.last_mut() {
            Some(last) if last.picture.is_none() && last.style == *style => last.text.push_str(text),
            _ => self.push(text_run(text, style.clone())),
        }
    }

    fn push(&mut self, run: Run) {
        self.count = self.count.saturating_add(1);
        if self.count > MAX_RUNS {
            self.truncated = true;
        } else {
            self.runs.push(run);
        }
    }

    /// Close the open paragraph. `blank` keeps one with nothing in it, as the empty line it is.
    fn end_para(&mut self, cur: &Chars, in_table: bool, blank: bool) {
        let mut runs = std::mem::take(&mut self.runs);
        if runs.is_empty() {
            if !blank {
                return;
            }
            runs.push(text_run("\u{a0}", Style::default()));
        }
        let para = Block::Para(Para { runs, kind: ParaKind::Body, align: cur.align, indent: cur.indent, quote: 0, gap: false });
        if in_table {
            self.cell.push(para);
        } else {
            self.end_table();
            self.blocks.push(para);
        }
        self.count = self.count.saturating_add(1);
        if self.blocks.len() >= MAX_BLOCKS || self.count > MAX_RUNS {
            self.truncated = true;
        }
    }

    fn end_row(&mut self) {
        if !self.cell.is_empty() {
            let blocks = std::mem::take(&mut self.cell);
            self.row.push(Cell { blocks, header: false, span: 1 });
        }
        if !self.row.is_empty() {
            self.rows.push(std::mem::take(&mut self.row));
        }
    }

    fn end_table(&mut self) {
        self.end_row();
        if self.rows.is_empty() {
            return;
        }
        let table = Table { rows: std::mem::take(&mut self.rows), border: std::mem::take(&mut self.border) };
        if table.columns() > 1 {
            self.blocks.push(Block::Table(table));
        } else {
            // One column is only layout: its paragraphs read as ordinary ones.
            self.blocks.extend(table.rows.into_iter().flatten().flat_map(|cell| cell.blocks));
        }
    }
}

/// The address in a `HYPERLINK` field instruction. It is shown, never followed.
fn link(instruction: &str) -> Option<Arc<str>> {
    let rest = instruction.trim_start();
    let rest = rest.get(..9).filter(|w| w.eq_ignore_ascii_case("HYPERLINK")).and_then(|_| rest.get(9..))?.trim_start();
    let address = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next()?,
        // A switch such as \l comes before a place inside the document, not an address.
        None if rest.starts_with('\\') => return None,
        None => rest.split_whitespace().next()?,
    };
    let address = address.trim();
    (!address.is_empty() && address.len() <= MAX_LINK).then(|| Arc::from(address))
}
