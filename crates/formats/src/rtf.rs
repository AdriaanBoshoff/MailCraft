//! RTF message bodies → [`Doc`]. An RTF body that encapsulates HTML or plain text ([MS-OXRTFEX])
//! is read as what it wraps; ordinary RTF keeps its character styles, colours, alignment,
//! indents, links and tables, with the borders, fills and merged cells of each row. A picture
//! written into the body as PNG or JPEG data is kept as its bytes, undecoded; a bitmap, or a
//! metafile that holds only one, is kept as a BMP file; other pictures leave only a place. The
//! place of an attachment (`\objattph`, [MS-OXRTFEX] 2.2.3.4) is kept by its number. Control words are those of the RTF specification,
//! version 1.9.1 ("Pictures", "Table Definitions", "Paragraph Borders").

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mailcraft_model::rtf::{Event, Reader, Scope, Wrapped, encapsulated, is_skipped_destination};

use crate::html;
use crate::rich::{Align, Block, Cell, Doc, Edge, Edges, Line, Para, ParaKind, Picture, Rgb, Run, Script, Style, Table, VAlign, any_edge};

const MAX_BLOCKS: usize = 50_000;
const MAX_RUNS: usize = 200_000;
const MAX_COLORS: usize = 1024;
const MAX_FONTS: usize = 4096;
const MAX_FONT_NAME: usize = 64;
const MAX_LINK: usize = 2048;
const MAX_EDGES: usize = 64;
/// Room at the sides of a cell's text, in pixels, for a row that asks for less or for none
/// (`\trgaph`), and the most a row may ask for. Above and below there is one pixel.
const CELL_PADDING: u16 = 2;
const MAX_CELL_PADDING: i32 = 64;
/// Widest border line kept, in pixels, and the most rows one merged cell covers.
const MAX_EDGE: u8 = 12;
const MAX_ROW_SPAN: u16 = 1000;
/// Places for attachments one body may keep.
const MAX_OBJECTS: usize = 256;
/// Longest side a picture may ask to be shown at, in pixels.
const MAX_PICTURE_SIDE: i64 = 4096;

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
    /// A picture's settings and data (`\pict`).
    Picture,
}

/// A picture being read.
#[derive(Default)]
struct Pict {
    style: Style,
    kind: Kind,
    /// The data could not be read as hex digits, or there is more of it than is kept.
    bad: bool,
    bytes: Vec<u8>,
    /// The first of two hex digits, when the second has not come yet.
    half: Option<u8>,
    /// The size to show the picture at, in twips, and how far to scale it, in hundredths.
    goal: (i32, i32),
    scale: (i32, i32),
}

/// What a picture's data is.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Kind {
    /// A kind that is not read.
    #[default]
    Unread,
    /// A PNG or JPEG file, kept as it is.
    File,
    /// A bitmap without a file's header (`\dibitmap`).
    Bitmap,
    /// A Windows metafile (`\wmetafile`) and an enhanced one (`\emfblip`).
    Wmf,
    Emf,
}

/// One cell of the row being defined: everything before its `\cellx`.
#[derive(Clone, Copy, Default)]
struct CellDef {
    /// The cell's right edge, in twips.
    right: i32,
    edges: Edges,
    fill: Option<Rgb>,
    valign: VAlign,
    merge: Merge,
}

/// A cell's part in a cell merged down several rows.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Merge {
    #[default]
    None,
    First,
    Under,
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
    /// For each finished row, each cell's right edge and its part in a cell merged down.
    merges: Vec<Vec<(i32, Merge)>>,
    merging: Vec<(i32, Merge)>,
    /// The cells of the row being defined, the one whose settings are being read, and the side
    /// of it border words apply to.
    defs: Vec<CellDef>,
    def: CellDef,
    side: Option<usize>,
    /// Half the room between the text of two cells of the row being defined, in twips.
    half_gap: i32,
    pict: Option<Pict>,
    written: Vec<Arc<[u8]>>,
    written_bytes: usize,
    objects: usize,
    runs: Vec<Run>,
    colors: Vec<Option<Rgb>>,
    rgb: Option<Rgb>,
    mono_fonts: HashSet<i32>,
    /// Names of the fonts in the font table, by number, and the number text has when it names none.
    fonts: HashMap<i32, String>,
    default_font: Option<i32>,
    declaring: i32,
    instruction: String,
    count: usize,
    truncated: bool,
}

fn formatted(rtf: &[u8]) -> Doc {
    let mut b = Builder::default();
    let mut scope = Scope::new(Chars::default());
    let mut reader = Reader::new(rtf);
    while let Some(event) = reader.next() {
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
                if was == Dest::Picture && scope.cur.dest != Dest::Picture {
                    b.end_picture();
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
                        // Other names of the same font, and its data: not the name it goes by.
                        "falt" | "panose" | "fname" | "fontemb" | "fontfile" => scope.cur.dest = Dest::Skip,
                        _ if star => scope.cur.dest = Dest::Skip,
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
                    Dest::Picture => b.picture_word(&mut scope.cur, word, param, star),
                    Dest::Skip | Dest::Instruction => {}
                }
            }
            Event::Text(text) => {
                scope.cur.star = false;
                match scope.cur.dest {
                    Dest::Body if !scope.cur.hidden => {
                        // Text that names no font has the document's default one (`\deff`).
                        if scope.cur.style.family.is_none() {
                            scope.cur.style.family = b.default_font.and_then(|font| b.family(font));
                        }
                        b.text(&text, &scope.cur.style);
                    }
                    // A font's name ends at the ";" (RTF 1.9.1, "Font Table").
                    Dest::Fonts => {
                        let name = text.split(';').next().unwrap_or("");
                        if b.fonts.len() < MAX_FONTS || b.fonts.contains_key(&b.declaring) {
                            let kept = b.fonts.entry(b.declaring).or_default();
                            let room = MAX_FONT_NAME.saturating_sub(kept.chars().count());
                            kept.extend(name.chars().filter(|c| !c.is_control()).take(room));
                        }
                    }
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
                    Dest::Picture => b.picture_data(&text),
                    _ => {}
                }
            }
        }
        // A picture's digits and what is skipped are not text: they do not use up the room for it.
        reader.count_text(!matches!(scope.cur.dest, Dest::Skip | Dest::Picture));
    }
    b.end_picture();
    b.end_para(&scope.cur, false, false);
    b.end_table();
    // Empty paragraphs at the end are the writer's, not the reader's.
    while matches!(b.blocks.last(), Some(Block::Para(p)) if p.text().trim().is_empty() && p.runs.iter().all(|r| r.picture.is_none())) {
        b.blocks.pop();
    }
    Doc { blocks: b.blocks, truncated: b.truncated || reader.truncated(), written: b.written, objects: b.objects, ..Doc::default() }
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
            // The picture a reader of this RTF version shows (the other one, `\nonshppict`, is skipped).
            "shppict" => {}
            "pict" if cur.hidden => cur.dest = Dest::Skip,
            "pict" => {
                self.end_picture();
                cur.dest = Dest::Picture;
                self.pict = Some(Pict { style: cur.style.clone(), scale: (100, 100), ..Pict::default() });
            }
            // An attachment stands here. Places past the cap are counted, so that the count
            // still tells whether the body and the attachments agree.
            "objattph" => {
                if self.objects < MAX_OBJECTS {
                    self.push(Run { style: cur.style.clone(), picture: Some(Picture::Object(self.objects)), ..Run::default() });
                }
                self.objects = self.objects.saturating_add(1);
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
            "f" => {
                cur.style.mono = self.mono_fonts.contains(&param.unwrap_or(0));
                cur.style.family = self.family(param.unwrap_or(0));
            }
            "deff" => self.default_font = param,
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
            "trowd" => {
                self.defs.clear();
                self.def = CellDef::default();
                self.side = None;
                self.half_gap = 0;
            }
            "trgaph" => self.half_gap = param.unwrap_or(0),
            // A cell's settings come before the `\cellx` that gives its right edge.
            "cellx" => {
                let mut def = CellDef { right: param.unwrap_or(0), ..std::mem::take(&mut self.def) };
                // A double line is two lines of the width given with as much room between them.
                for edge in def.edges.iter_mut().filter(|e| e.line == Line::Double) {
                    edge.width = edge.width.saturating_mul(3).min(MAX_EDGE);
                }
                if self.defs.len() < MAX_EDGES {
                    self.defs.push(def);
                }
                self.side = None;
            }
            "cell" => {
                self.end_para(cur, true, false);
                let blocks = std::mem::take(&mut self.cell);
                let def = self.defs.get(self.row.len()).copied().unwrap_or_default();
                // 15 twips to the pixel.
                let left = self.row.len().checked_sub(1).and_then(|i| self.defs.get(i)).map_or(0, |d| d.right);
                let width = u16::try_from((def.right.saturating_sub(left) / 15).clamp(0, 4096)).unwrap_or(0);
                let mut edges = def.edges;
                // Cells that touch share the line between them: it is drawn once.
                if let (Some(before), Some(own)) = (self.row.last().and_then(|c| c.edges), edges.get_mut(3))
                    && before.get(1).is_some_and(|e| e.width > 0)
                {
                    own.width = 0;
                }
                if let (Some(top), Some(above)) = (edges.first_mut(), self.rows.last())
                    && !above.is_empty()
                    && above.iter().all(|c| c.edges.is_some_and(|e| e.get(2).is_some_and(|e| e.width > 0)))
                {
                    top.width = 0;
                }
                self.merging.push((def.right, def.merge));
                let beside = u16::try_from((self.half_gap / 15).clamp(i32::from(CELL_PADDING), MAX_CELL_PADDING)).unwrap_or(CELL_PADDING);
                self.row.push(Cell {
                    blocks,
                    span: 1,
                    width,
                    padding: Some((1, beside)),
                    background: def.fill,
                    valign: def.valign,
                    edges: any_edge(&edges).then_some(edges),
                    ..Cell::default()
                });
            }
            "row" => self.end_row(),
            // The sides of a cell, in the order the model keeps them: top, right, bottom, left.
            "clbrdrt" => self.side = Some(0),
            "clbrdrr" => self.side = Some(1),
            "clbrdrb" => self.side = Some(2),
            "clbrdrl" => self.side = Some(3),
            // Borders of rows and of paragraphs are not a cell's.
            "trbrdrt" | "trbrdrl" | "trbrdrb" | "trbrdrr" | "trbrdrh" | "trbrdrv" | "brdrt" | "brdrb" | "brdrl" | "brdrr" | "brdrbtw" | "brdrbar"
            | "box" | "chbrdr" | "cldglu" | "cldgll" => self.side = None,
            "brdrs" | "brdrth" | "brdrsh" | "brdrhair" | "brdrinset" | "brdroutset" => self.edge(|e| e.line = Line::Solid),
            "brdrdb" => self.edge(|e| e.line = Line::Double),
            "brdrdot" => self.edge(|e| e.line = Line::Dotted),
            "brdrdash" | "brdrdashsm" | "brdrdashd" | "brdrdashdd" => self.edge(|e| e.line = Line::Dashed),
            "brdrnone" | "brdrnil" | "brdrtbl" => {
                if let Some(edge) = self.side.and_then(|side| self.def.edges.get_mut(side)) {
                    *edge = Edge::default();
                }
                self.side = None;
            }
            // Twips, 15 to the pixel; a line is never thinner than one pixel.
            "brdrw" => {
                let width = u8::try_from(((param.unwrap_or(0).max(0) + 7) / 15).clamp(1, i32::from(MAX_EDGE))).unwrap_or(1);
                if let Some(edge) = self.side.and_then(|side| self.def.edges.get_mut(side)) {
                    edge.width = width;
                }
            }
            "brdrcf" => {
                let rgb = color(&self.colors);
                if let Some(edge) = self.side.and_then(|side| self.def.edges.get_mut(side)) {
                    edge.color = rgb;
                }
            }
            "clcbpat" => self.def.fill = color(&self.colors),
            "clvertalt" => self.def.valign = VAlign::Top,
            "clvertalc" => self.def.valign = VAlign::Middle,
            "clvertalb" => self.def.valign = VAlign::Bottom,
            "clvmgf" => self.def.merge = Merge::First,
            "clvmrg" => self.def.merge = Merge::Under,
            _ if star || is_skipped_destination(word) => cur.dest = Dest::Skip,
            _ => {}
        }
    }

    /// Set the kind of line of the side of the cell that border words apply to now.
    fn edge(&mut self, set: impl Fn(&mut Edge)) {
        if let Some(edge) = self.side.and_then(|side| self.def.edges.get_mut(side)) {
            set(edge);
            edge.width = edge.width.max(1);
        }
    }

    /// A control word inside `\pict` (RTF 1.9.1, "Pictures").
    fn picture_word(&mut self, cur: &mut Chars, word: &str, param: Option<i32>, star: bool) {
        let Some(pict) = &mut self.pict else { return };
        let n = param.unwrap_or(0);
        match word {
            "pngblip" | "jpegblip" => pict.kind = Kind::File,
            "dibitmap" => pict.kind = Kind::Bitmap,
            "wmetafile" => pict.kind = Kind::Wmf,
            "emfblip" => pict.kind = Kind::Emf,
            // Kinds that are not read: a bitmap made for one device, QuickDraw, OS/2 metafiles.
            "wbitmap" | "macpict" | "pmmetafile" => pict.kind = Kind::Unread,
            "picwgoal" => pict.goal.0 = n,
            "pichgoal" => pict.goal.1 = n,
            "picscalex" => pict.scale.0 = n,
            "picscaley" => pict.scale.1 = n,
            // Bytes written as they are; the reader of words steps over them.
            "bin" => pict.bad = true,
            // Properties of the picture (`\*\picprop`) are not its data.
            _ if star => cur.dest = Dest::Skip,
            _ => {}
        }
    }

    /// The data of a picture: two hex digits to the byte, with line ends between them.
    fn picture_data(&mut self, text: &str) {
        let Some(pict) = &mut self.pict else { return };
        if pict.kind == Kind::Unread || pict.bad {
            return;
        }
        let room = crate::html::MAX_WRITTEN_BYTES.saturating_sub(self.written_bytes);
        for c in text.chars() {
            if c.is_ascii_whitespace() {
                continue;
            }
            let Some(digit) = c.to_digit(16).and_then(|d| u8::try_from(d).ok()) else {
                pict.bad = true;
                break;
            };
            match pict.half.take() {
                None => pict.half = Some(digit),
                Some(high) if pict.bytes.len() < room => pict.bytes.push(high << 4 | digit),
                Some(_) => {
                    pict.bad = true;
                    break;
                }
            }
        }
    }

    /// Close the picture being read: it stands in the line of text where it was written.
    fn end_picture(&mut self) {
        let Some(pict) = self.pict.take() else { return };
        let whole = !pict.bad && pict.half.is_none() && !pict.bytes.is_empty();
        // Twips to pixels, scaled as asked; a picture that gives no size is shown at its own.
        let side = |goal: i32, scale: i32| {
            let scale = if scale > 0 { i64::from(scale.min(10_000)) } else { 100 };
            u16::try_from((i64::from(goal.max(0)) * scale / 100 / 15).clamp(0, MAX_PICTURE_SIDE)).unwrap_or(0)
        };
        let extent = (side(pict.goal.0, pict.scale.0), side(pict.goal.1, pict.scale.1));
        let room = crate::html::MAX_WRITTEN_BYTES.saturating_sub(self.written_bytes);
        let file = match pict.kind {
            _ if !whole => None,
            Kind::Unread => None,
            Kind::File => Some(pict.bytes),
            Kind::Bitmap => crate::metafile::dib_file(&pict.bytes),
            Kind::Wmf => crate::metafile::wmf_picture(&pict.bytes, Some((u32::from(extent.0), u32::from(extent.1)))),
            Kind::Emf => crate::metafile::emf_bitmap(&pict.bytes),
        };
        let place = file.filter(|file| file.len() <= room).and_then(|file| match self.written.iter().position(|kept| **kept == *file) {
            Some(same) => Some(same),
            None if self.written.len() < crate::html::MAX_WRITTEN => {
                self.written_bytes = self.written_bytes.saturating_add(file.len());
                self.written.push(Arc::from(file));
                Some(self.written.len() - 1)
            }
            None => None,
        });
        self.push(Run { style: pict.style, picture: Some(Picture::Inline(place)), extent, ..Run::default() });
    }

    /// The name of a font of the font table, as a list of one.
    fn family(&self, font: i32) -> Option<Arc<str>> {
        let name = self.fonts.get(&font)?.trim();
        (!name.is_empty()).then(|| Arc::from(name))
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
        let para = Block::Para(Para { runs, kind: ParaKind::Body, align: cur.align, indent: cur.indent, ..Para::default() });
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
            self.row.push(Cell { blocks, span: 1, ..Cell::default() });
        }
        let merging = std::mem::take(&mut self.merging);
        if !self.row.is_empty() {
            self.rows.push(std::mem::take(&mut self.row));
            self.merges.push(merging);
        }
    }

    /// Cells merged down (`\clvmgf` in the first row, `\clvmrg` in those under it) become one
    /// cell that covers the rows; cells are matched by their right edge.
    fn merge_down(&mut self) {
        let merges = std::mem::take(&mut self.merges);
        if !merges.iter().flatten().any(|(_, merge)| *merge == Merge::Under) {
            return;
        }
        let rows = std::mem::take(&mut self.rows);
        // Right edge of a cell that reaches down: the row it is in and its place there.
        let mut open: HashMap<i32, (usize, usize)> = HashMap::new();
        for (r, (row, parts)) in rows.into_iter().zip(merges).enumerate() {
            let mut kept: Vec<Cell> = Vec::with_capacity(row.len());
            let mut still: HashMap<i32, (usize, usize)> = HashMap::new();
            for (i, cell) in row.into_iter().enumerate() {
                let (right, merge) = parts.get(i).copied().unwrap_or((0, Merge::None));
                let first = (merge == Merge::Under).then(|| open.get(&right).copied()).flatten();
                let above =
                    first.and_then(|(at, place)| self.rows.get_mut(at).and_then(|above| above.get_mut(place)).map(|cell| (cell, (at, place))));
                match above {
                    Some((above, place)) if above.rows.max(1) < MAX_ROW_SPAN => {
                        above.rows = above.rows.max(1) + 1;
                        // The line under the last part is the line under the whole cell.
                        if let (Some(whole), Some(part)) = (&mut above.edges, cell.edges)
                            && let (Some(under), Some(last)) = (whole.get_mut(2), part.get(2))
                        {
                            *under = *last;
                        }
                        still.insert(right, place);
                    }
                    _ => {
                        if merge == Merge::First {
                            still.insert(right, (r, kept.len()));
                        }
                        kept.push(cell);
                    }
                }
            }
            open = still;
            self.rows.push(kept);
        }
    }

    fn end_table(&mut self) {
        self.end_row();
        self.merge_down();
        if self.rows.is_empty() {
            return;
        }
        let table = Table { rows: std::mem::take(&mut self.rows), padding: CELL_PADDING, snug: true, ..Table::default() };
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
