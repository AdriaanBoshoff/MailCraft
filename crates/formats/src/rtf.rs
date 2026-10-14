//! RTF message bodies → [`Doc`]. An RTF body that encapsulates HTML or plain text ([MS-OXRTFEX])
//! is read as what it wraps; ordinary RTF keeps its character styles, colours, alignment,
//! indents, links and tables, with the borders, fills and merged cells of each row. A picture
//! written into the body as PNG or JPEG data is kept as its bytes, undecoded; a bitmap, or a
//! metafile that holds only one, is kept as a BMP file; other pictures leave only a place. The
//! place of an attachment (`\objattph`, [MS-OXRTFEX] 2.2.3.4) is kept by its number. Control words are those of the RTF specification,
//! version 1.9.1 ("Pictures", "Table Definitions", "Paragraph Borders").

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use openmailclient_fonts::Lettering;
use openmailclient_model::rtf::{Event, Reader, Scope, Wrapped, encapsulated, is_skipped_destination};

use crate::html;
use crate::rich::{
    Align, Block, Cell, Doc, Edge, Edges, Frame, LEADING_PX, LEADING_TIMES, Leader, Leading, Line, Para, ParaKind, Picture, Rgb, Run, Script, Style,
    Tab, TabKind, Table, VAlign, any_edge,
};

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
/// The most columns one cell merged across covers, and how deep tables stand in tables.
const MAX_SPAN: u16 = 64;
const MAX_NEST: u8 = 8;
/// Tab stops of one paragraph, and the furthest one may be, in pixels.
const MAX_TABS: usize = 32;
const MAX_TAB_AT: i32 = 4096;
/// Characters of a list paragraph's number or bullet.
const MAX_MARKER: usize = 12;
/// Pixels of room a paragraph may ask for above it, and between its border and its text.
const MAX_ABOVE: i32 = 96;
const MAX_FRAME_ROOM: i32 = 64;
/// Which sides of a paragraph's border the words that follow describe: all four.
const ALL_SIDES: u8 = 4;
/// The line between paragraphs inside one border, as a side border words apply to.
const BETWEEN: u8 = 5;
/// Places for attachments one body may keep.
const MAX_OBJECTS: usize = 256;
/// Longest side a picture may ask to be shown at, in pixels.
const MAX_PICTURE_SIDE: i64 = 4096;

pub fn parse(rtf: &[u8]) -> Doc {
    parse_with(rtf, None)
}

/// As [`parse`], with the text in pictures that are kept as drawings drawn in the fonts
/// `letters` knows. Without it such a picture keeps its place and is not shown.
pub fn parse_with(rtf: &[u8], letters: Option<&dyn Lettering>) -> Doc {
    match encapsulated(rtf) {
        Some(Wrapped::Html(h)) => html::parse(&h),
        Some(Wrapped::Text(t)) => plain(&t),
        None => formatted(rtf, letters),
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
        doc.blocks.push(Block::Para(
            Para { runs: vec![text_run(if line.is_empty() { "\u{a0}" } else { line }, Style::default())], ..Para::default() }.by_its_text(),
        ));
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
    /// The number or bullet a list paragraph starts with, as the writer worked it out
    /// (`\listtext`, `\pntext`).
    Marker,
    /// The lists the body has (`\listtable`), and the words a level's number is made of
    /// (`\leveltext`).
    Lists,
    LevelText,
    /// Which list each list paragraph is of (`\listoverridetable`), and the words of a level
    /// that one of them writes anew.
    Overrides,
    OverrideText,
}

/// Lists a body may describe, levels a list has, lists paragraphs may name, and characters
/// in the words a level's number is made of.
const MAX_LISTS: usize = 256;
const MAX_LEVELS: usize = 9;
const MAX_OVERRIDES: usize = 1024;
const MAX_LEVEL_TEXT: usize = 32;

/// One level of a list (RTF 1.9.1, "List Table").
#[derive(Clone, Default)]
struct ListLevel {
    /// How its number is written (`\levelnfc`): 0 is 1, 2, 3; 23 is a bullet.
    nfc: i32,
    start: i32,
    /// The words its number is made of: how many characters follow, then those characters,
    /// where one below 9 stands for the number of the level it counts.
    text: Vec<char>,
}

#[derive(Default)]
struct List {
    id: i32,
    levels: Vec<ListLevel>,
}

/// Where a list has counted to: the number of each level, where it has one, and which of
/// the numbers that paragraphs name (`\ls`) counted last.
#[derive(Clone, Copy, Default)]
struct Count {
    at: [Option<i32>; MAX_LEVELS],
    ls: Option<i32>,
}

/// What paragraphs that name one number (`\ls`) have otherwise than their list says, for one
/// level (`\lfolevel`; RTF 1.9.1, "List Override Table").
#[derive(Clone, Default)]
struct Changed {
    /// The level starts at a number of its own (`\listoverridestartat`).
    starts: bool,
    /// The level is written in a way of its own (`\listoverrideformat`).
    formats: bool,
    start: Option<i32>,
    level: Option<ListLevel>,
}

/// The sixteen colours a highlight is kept as (ECMA-376 Part 1, 17.18.40, names them). RTF
/// gives a highlight as a place in the colour table (RTF 1.9.1, "Highlighting"); what stands
/// there is shown as the one of these it is nearest to.
const MARKERS: [Rgb; 16] = [
    [0, 0, 0],
    [0, 0, 255],
    [0, 255, 255],
    [0, 255, 0],
    [255, 0, 255],
    [255, 0, 0],
    [255, 255, 0],
    [255, 255, 255],
    [0, 0, 128],
    [0, 128, 128],
    [0, 128, 0],
    [128, 0, 128],
    [128, 0, 0],
    [128, 128, 0],
    [128, 128, 128],
    [192, 192, 192],
];

/// The highlight colour `color` is shown as: itself when it is one of the sixteen; a grey by
/// how light it is when it has nearly no colour; else the pure colour of its hue, bright or
/// dark. (Seen in Outlook for one case: a pale yellow is shown as bright yellow.)
fn highlighter(color: Rgb) -> Rgb {
    if MARKERS.contains(&color) {
        return color;
    }
    let (most, least) = (color.iter().copied().max().unwrap_or(0), color.iter().copied().min().unwrap_or(0));
    let spread = most - least;
    if spread < 24 {
        let grey = match most {
            0..=63 => 0,
            64..=159 => 128,
            160..=223 => 192,
            _ => 255,
        };
        return [grey; 3];
    }
    // A part is in the hue when it is nearer the most of the three than the least.
    let full = if most >= 192 { 255 } else { 128 };
    color.map(|part| if u16::from(part - least) * 2 >= u16::from(spread) { full } else { 0 })
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
    across: Across,
}

/// A cell's part in a cell merged across several columns (`\clmgf`, `\clmrg`).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Across {
    #[default]
    None,
    First,
    Next,
}

/// The row being defined (`\trowd` and what follows it): its cells, the one whose settings are
/// being read, and the side of it border words apply to.
#[derive(Default)]
struct RowDef {
    defs: Vec<CellDef>,
    def: CellDef,
    side: Option<usize>,
    /// Half the room between the text of two cells, in twips.
    half_gap: i32,
    /// `\rtlrow`: the row's first cell stands at the right.
    rtl: bool,
}

/// A table being read: finished rows, the open row's cells, the open cell's blocks.
#[derive(Default)]
struct Level {
    rows: Vec<Vec<Cell>>,
    /// The first row is written from the right.
    rtl: bool,
    row: Vec<Cell>,
    cell: Vec<Block>,
    /// For each finished row, each cell's right edge and its part in a cell merged down.
    merges: Vec<Vec<(i32, Merge)>>,
    merging: Vec<(i32, Merge)>,
    /// Cells of the open row that have ended, those merged into the one before them too.
    seen: usize,
    /// Cells of the open row of a table inside a cell: such a row is defined after its cells
    /// (`\nesttableprops`), so they wait for it.
    waiting: Vec<Vec<Block>>,
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
    /// `\rtlpar`, and whether the paragraph was told a side to stand at.
    rtl: bool,
    sided: bool,
    indent: u8,
    in_table: bool,
    /// How many tables deep the paragraph is (`\itap`); 0 where it does not say.
    itap: u8,
    /// Inside `\nesttableprops`: the row defined here is one of a table inside a cell.
    nest_props: bool,
    /// The paragraph's border (top, right, bottom, left), which of its sides the words being
    /// read describe, and the room between it and the text, in pixels.
    edges: Edges,
    side: Option<u8>,
    pad: u16,
    /// Pixels of room asked for above and below the paragraph (`\sb`, `\sa`).
    before: u8,
    after: u8,
    /// Twips from the margin to the paragraph's text (`\li`), and its tab stops, in twips
    /// from the margin, with how the stop being described holds its text.
    left: i32,
    /// The list the paragraph is of (`\ls`), and its level there (`\ilvl`).
    ls: Option<i32>,
    ilvl: u8,
    tabs: Vec<(i32, TabKind, Leader)>,
    tab_kind: TabKind,
    tab_lead: Leader,
    /// The line between this paragraph and the next inside one border (`\brdrbtw`).
    between: Edge,
    /// The colour behind the whole paragraph (`\cbpat`).
    shade: Option<Rgb>,
    /// The height of the paragraph's lines (`\sl`), and whether it counts in 240ths of
    /// their own height (`\slmult1`) and not in twips.
    sl: i32,
    sl_times: bool,
}

#[derive(Default)]
struct Builder<'a> {
    /// Who gives the shapes of the letters in a drawing, when anybody does.
    letters: Option<&'a dyn Lettering>,
    blocks: Vec<Block>,
    /// The table being read, and the tables inside its open cell, each inside the one before.
    table: Level,
    nested: Vec<Level>,
    /// The row being defined, of the table and of a table inside a cell, and which of them
    /// the words being read define.
    outer: RowDef,
    inner: RowDef,
    inner_now: bool,
    /// The border of the last paragraph that had one: paragraphs that follow one another with
    /// the same border stand inside one.
    frame: Option<Arc<Frame>>,
    /// The room the paragraph before asked for below it.
    below: u8,
    pict: Option<Pict>,
    written: Vec<Arc<[u8]>>,
    written_bytes: usize,
    objects: usize,
    runs: Vec<Run>,
    /// The number or bullet of the paragraph being read, when it is one of a list.
    marker: String,
    /// The lists the body describes, the one being read, which list each number that
    /// paragraphs name stands for, and where each has counted to.
    lists: Vec<List>,
    list: Option<List>,
    override_id: i32,
    overrides: HashMap<i32, i32>,
    /// The levels the override being read changes, whether the last `\lfolevel` is one of
    /// them, and the changes of each number that paragraphs name.
    changed: Vec<Changed>,
    changing: bool,
    changes: HashMap<i32, Vec<Changed>>,
    counts: HashMap<i32, Count>,
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

fn formatted(rtf: &[u8], letters: Option<&dyn Lettering>) -> Doc {
    let mut b = Builder { letters, ..Builder::default() };
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
                    Dest::Lists => b.list_word(&mut scope.cur, word, param, star),
                    Dest::Overrides => b.override_word(&mut scope.cur, word, param),
                    Dest::Skip | Dest::Instruction | Dest::Marker | Dest::LevelText | Dest::OverrideText => {}
                }
            }
            Event::Bin(data, whole) => {
                if scope.cur.dest == Dest::Picture {
                    b.picture_bytes(data, whole);
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
                    Dest::Marker => {
                        let room = (MAX_MARKER * 4).saturating_sub(b.marker.chars().count());
                        b.marker.extend(text.chars().take(room));
                    }
                    Dest::LevelText => {
                        if let Some(level) = b.list.as_mut().and_then(|l| l.levels.last_mut()) {
                            let room = MAX_LEVEL_TEXT.saturating_sub(level.text.len());
                            level.text.extend(text.chars().take(room));
                        }
                    }
                    Dest::OverrideText => {
                        if let Some(level) = b.changed.last_mut().filter(|_| b.changing).and_then(|c| c.level.as_mut()) {
                            let room = MAX_LEVEL_TEXT.saturating_sub(level.text.len());
                            level.text.extend(text.chars().take(room));
                        }
                    }
                    _ => {}
                }
            }
        }
        // A picture's digits and what is skipped are not text: they do not use up the room for it.
        reader.count_text(!matches!(scope.cur.dest, Dest::Skip | Dest::Picture));
    }
    b.end_picture();
    b.end_list();
    b.end_para(&scope.cur, 0, false);
    b.end_table();
    // Empty paragraphs at the end are the writer's, not the reader's.
    while matches!(b.blocks.last(), Some(Block::Para(p)) if p.text().trim().is_empty() && p.runs.iter().all(|r| r.picture.is_none())) {
        b.blocks.pop();
    }
    Doc { blocks: b.blocks, truncated: b.truncated || reader.truncated(), written: b.written, objects: b.objects, ..Doc::default() }
}

impl Builder<'_> {
    fn word(&mut self, cur: &mut Chars, word: &str, param: Option<i32>, star: bool) {
        let on = param != Some(0);
        self.inner_now = cur.nest_props;
        let color = |colors: &[Option<Rgb>]| usize::try_from(param.unwrap_or(0)).ok().and_then(|i| colors.get(i).copied()).flatten();
        match word {
            "fonttbl" => cur.dest = Dest::Fonts,
            // The lists, and which of them each list paragraph is of (RTF 1.9.1, "List Table"
            // and "List Override Table").
            "listtable" => cur.dest = Dest::Lists,
            "listoverridetable" => cur.dest = Dest::Overrides,
            "ls" => cur.ls = param,
            "ilvl" => cur.ilvl = u8::try_from(param.unwrap_or(0).clamp(0, 8)).unwrap_or(0),
            "colortbl" => cur.dest = Dest::Colors,
            "fldinst" => {
                cur.dest = Dest::Instruction;
                self.instruction.clear();
            }
            // The picture a reader of this RTF version shows (the other one, `\nonshppict`, is skipped).
            "shppict" => {}
            // A row of a table inside a cell is defined here, after its cells.
            "nesttableprops" => cur.nest_props = true,
            // What a reader that knows no tables inside cells is given in their place.
            "nonesttables" => cur.dest = Dest::Skip,
            // The number or bullet of a list paragraph, worked out by the writer (RTF 1.9.1,
            // "List Text" and "Bullets and Numbering").
            "listtext" | "pntext" => {
                cur.dest = Dest::Marker;
                self.marker.clear();
            }
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
            // A highlight is one of sixteen colours, whatever the colour table holds.
            "highlight" => cur.style.background = color(&self.colors).map(highlighter),
            "cb" | "chcbpat" => cur.style.background = color(&self.colors),
            // The colour behind a whole paragraph (RTF 1.9.1, "Paragraph Shading").
            "cbpat" => cur.shade = color(&self.colors),
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
            "par" | "sect" | "page" => self.end_para(cur, depth(cur), true),
            "line" => self.text("\n", &cur.style),
            "pard" => {
                cur.align = Align::Left;
                (cur.rtl, cur.sided) = (false, false);
                cur.indent = 0;
                cur.in_table = false;
                cur.itap = 0;
                cur.edges = Edges::default();
                (cur.side, cur.pad, cur.before, cur.after) = (None, 0, 0, 0);
                (cur.left, cur.tab_kind, cur.tab_lead) = (0, TabKind::Left, Leader::None);
                cur.tabs.clear();
                cur.between = Edge::default();
                cur.shade = None;
                (cur.ls, cur.ilvl) = (None, 0);
                (cur.sl, cur.sl_times) = (0, false);
            }
            // Line spacing (RTF 1.9.1, "Spacing").
            "sl" => cur.sl = param.unwrap_or(0),
            "slmult" => cur.sl_times = param == Some(1),
            // Twips, 15 to the pixel.
            "sb" => cur.before = pixels(param, MAX_ABOVE),
            "sa" => cur.after = pixels(param, MAX_ABOVE),
            "brsp" => cur.pad = u16::from(pixels(param, MAX_FRAME_ROOM)),
            "ql" | "qj" => (cur.align, cur.sided) = (Align::Left, true),
            "qc" => (cur.align, cur.sided) = (Align::Center, true),
            "qr" => (cur.align, cur.sided) = (Align::Right, true),
            // The paragraph's direction (RTF 1.9.1, "Bidirectional Writing Order").
            "rtlpar" => cur.rtl = true,
            "ltrpar" => cur.rtl = false,
            // Twips; half an inch is one level.
            "li" => {
                cur.left = param.unwrap_or(0).clamp(0, 31_680);
                cur.indent = u8::try_from((cur.left / 720).min(8)).unwrap_or(0);
            }
            // Tab stops (RTF 1.9.1, "Tabs"): how the next one holds its text, then where it is.
            "tqc" => cur.tab_kind = TabKind::Center,
            "tqr" => cur.tab_kind = TabKind::Right,
            "tqdec" => cur.tab_kind = TabKind::Decimal,
            // What fills the room up to the next stop.
            "tldot" => cur.tab_lead = Leader::Dots,
            "tlmdot" => cur.tab_lead = Leader::MiddleDots,
            "tlhyph" => cur.tab_lead = Leader::Hyphens,
            "tlul" => cur.tab_lead = Leader::Line,
            "tlth" => cur.tab_lead = Leader::ThickLine,
            "tleq" => cur.tab_lead = Leader::Equals,
            "tx" => {
                let (kind, lead) = (std::mem::take(&mut cur.tab_kind), std::mem::take(&mut cur.tab_lead));
                if cur.tabs.len() < MAX_TABS {
                    cur.tabs.push((param.unwrap_or(0), kind, lead));
                }
            }
            // A bar tab draws a line and moves nothing.
            "tb" => (cur.tab_kind, cur.tab_lead) = (TabKind::Left, Leader::None),
            "intbl" => cur.in_table = true,
            "itap" => cur.itap = u8::try_from(param.unwrap_or(1).clamp(0, i32::from(MAX_NEST))).unwrap_or(1),
            "trowd" => *self.def() = RowDef::default(),
            "trgaph" => self.def().half_gap = param.unwrap_or(0),
            "rtlrow" => self.def().rtl = true,
            "ltrrow" => self.def().rtl = false,
            // A cell's settings come before the `\cellx` that gives its right edge.
            "cellx" => {
                let row = self.def();
                let mut def = CellDef { right: param.unwrap_or(0), ..std::mem::take(&mut row.def) };
                // A double line is two lines of the width given with as much room between them.
                for edge in def.edges.iter_mut().filter(|e| e.line == Line::Double) {
                    edge.width = edge.width.saturating_mul(3).min(MAX_EDGE);
                }
                if row.defs.len() < MAX_EDGES {
                    row.defs.push(def);
                }
                row.side = None;
            }
            "cell" => {
                self.end_para(cur, 1, false);
                let blocks = std::mem::take(&mut self.table.cell);
                self.table.end_cell(blocks, &self.outer);
            }
            // The end of a cell of a table inside a cell (RTF 1.9.1, "Nested Tables").
            "nestcell" => {
                self.end_para(cur, depth(cur).max(2), false);
                if let Some(level) = self.nested.last_mut() {
                    let blocks = std::mem::take(&mut level.cell);
                    let full = level.waiting.len() >= MAX_EDGES;
                    match level.waiting.last_mut() {
                        Some(last) if full => last.extend(blocks),
                        _ => level.waiting.push(blocks),
                    }
                }
            }
            "nestrow" => {
                if let Some(level) = self.nested.last_mut() {
                    level.end_row(&self.inner);
                }
                self.inner = RowDef::default();
            }
            "row" => {
                self.settle(1);
                let rtl = self.outer.rtl;
                self.table.end_row(&RowDef { rtl, ..RowDef::default() });
            }
            // The sides of a cell, in the order the model keeps them: top, right, bottom, left.
            "clbrdrt" => self.sides(cur, Some(0), None),
            "clbrdrr" => self.sides(cur, Some(1), None),
            "clbrdrb" => self.sides(cur, Some(2), None),
            "clbrdrl" => self.sides(cur, Some(3), None),
            // The sides of a paragraph's border (RTF 1.9.1, "Paragraph Borders"), in the same order.
            "brdrbtw" => self.sides(cur, None, Some(BETWEEN)),
            "brdrt" => self.sides(cur, None, Some(0)),
            "brdrr" => self.sides(cur, None, Some(1)),
            "brdrb" => self.sides(cur, None, Some(2)),
            "brdrl" => self.sides(cur, None, Some(3)),
            "box" => self.sides(cur, None, Some(ALL_SIDES)),
            // Borders of rows and of characters are neither a cell's nor a paragraph's.
            "trbrdrt" | "trbrdrl" | "trbrdrb" | "trbrdrr" | "trbrdrh" | "trbrdrv" | "brdrbar" | "chbrdr" | "cldglu" | "cldgll" => {
                self.sides(cur, None, None);
            }
            "brdrs" | "brdrth" | "brdrsh" | "brdrhair" | "brdrinset" | "brdroutset" => self.edge(cur, |e| e.line = Line::Solid),
            "brdrdb" => self.edge(cur, |e| e.line = Line::Double),
            "brdrdot" => self.edge(cur, |e| e.line = Line::Dotted),
            "brdrdash" | "brdrdashsm" | "brdrdashd" | "brdrdashdd" => self.edge(cur, |e| e.line = Line::Dashed),
            "brdrnone" | "brdrnil" | "brdrtbl" => {
                self.each_edge(cur, |e| *e = Edge::default());
                self.sides(cur, None, None);
            }
            // Twips, 15 to the pixel; a line is never thinner than one pixel.
            "brdrw" => {
                let width = u8::try_from((param.unwrap_or(0).max(0).saturating_add(7) / 15).clamp(1, i32::from(MAX_EDGE))).unwrap_or(1);
                self.each_edge(cur, |e| e.width = width);
            }
            "brdrcf" => {
                let rgb = color(&self.colors);
                self.each_edge(cur, |e| e.color = rgb);
            }
            "clcbpat" => {
                let rgb = color(&self.colors);
                self.def().def.fill = rgb;
            }
            "clvertalt" => self.def().def.valign = VAlign::Top,
            "clvertalc" => self.def().def.valign = VAlign::Middle,
            "clvertalb" => self.def().def.valign = VAlign::Bottom,
            "clvmgf" => self.def().def.merge = Merge::First,
            "clvmrg" => self.def().def.merge = Merge::Under,
            "clmgf" => self.def().def.across = Across::First,
            "clmrg" => self.def().def.across = Across::Next,
            _ if star || is_skipped_destination(word) => cur.dest = Dest::Skip,
            _ => {}
        }
    }

    /// The row the words being read define.
    fn def(&mut self) -> &mut RowDef {
        if self.inner_now { &mut self.inner } else { &mut self.outer }
    }

    /// The cell that is open in the innermost open table.
    fn open_cell(&mut self) -> &mut Vec<Block> {
        match self.nested.last_mut() {
            Some(level) => &mut level.cell,
            None => &mut self.table.cell,
        }
    }

    /// Make the open tables agree with a paragraph that is `depth` tables deep: tables deeper
    /// than that have ended and stand in the cell they were in; tables not yet begun begin.
    fn settle(&mut self, depth: usize) {
        while self.nested.len() + 1 > depth.max(1) {
            let Some(done) = self.nested.pop() else { break };
            let blocks = done.finish();
            self.open_cell().extend(blocks);
        }
        while self.nested.len() + 1 < depth.min(usize::from(MAX_NEST)) {
            self.nested.push(Level::default());
        }
    }

    /// Set the kind of line of the side of the cell that border words apply to now.
    fn edge(&mut self, cur: &mut Chars, set: impl Fn(&mut Edge)) {
        self.each_edge(cur, |edge| {
            set(edge);
            edge.width = edge.width.max(1);
        });
    }

    /// A control word inside `\listtable` (RTF 1.9.1, "List Table").
    fn list_word(&mut self, cur: &mut Chars, word: &str, param: Option<i32>, star: bool) {
        match word {
            "list" => {
                self.end_list();
                self.list = Some(List::default());
            }
            "listlevel" => {
                if let Some(list) = &mut self.list
                    && list.levels.len() < MAX_LEVELS
                {
                    list.levels.push(ListLevel { start: 1, ..ListLevel::default() });
                }
            }
            // The newer word says the same as the older one, and more kinds.
            "levelnfc" | "levelnfcn" => self.level(|l| l.nfc = param.unwrap_or(0)),
            "levelstartat" => self.level(|l| l.start = param.unwrap_or(1)),
            "leveltext" => {
                cur.dest = Dest::LevelText;
                self.level(|l| l.text.clear());
            }
            "listid" => {
                if let Some(list) = &mut self.list {
                    list.id = param.unwrap_or(0);
                }
            }
            // Names, and the places of the numbers in a level's words: not read.
            "levelnumbers" | "listname" | "liststylename" | "listpicture" => cur.dest = Dest::Skip,
            _ if star => cur.dest = Dest::Skip,
            _ => {}
        }
    }

    /// A control word inside `\listoverridetable` (RTF 1.9.1, "List Override Table"): which
    /// list a number that paragraphs name stands for, and what its levels have otherwise
    /// than the list says. The first `\lfolevel` is for the first level, and so on.
    fn override_word(&mut self, cur: &mut Chars, word: &str, param: Option<i32>) {
        match word {
            "listoverride" => {
                self.changed.clear();
                self.changing = false;
            }
            "listid" => self.override_id = param.unwrap_or(0),
            "lfolevel" => {
                self.changing = self.changed.len() < MAX_LEVELS;
                if self.changing {
                    self.changed.push(Changed::default());
                }
            }
            "listoverridestartat" => self.change(|c| c.starts = true),
            "listoverrideformat" => self.change(|c| c.formats = true),
            "listlevel" => self.change(|c| c.level = Some(ListLevel { start: 1, ..ListLevel::default() })),
            "levelstartat" => self.change(|c| c.start = Some(param.unwrap_or(1))),
            "levelnfc" | "levelnfcn" => self.change(|c| {
                if let Some(level) = &mut c.level {
                    level.nfc = param.unwrap_or(0);
                }
            }),
            "leveltext" => {
                cur.dest = Dest::OverrideText;
                self.change(|c| {
                    if let Some(level) = &mut c.level {
                        level.text.clear();
                    }
                });
            }
            "levelnumbers" => cur.dest = Dest::Skip,
            "ls" if self.overrides.len() < MAX_OVERRIDES => {
                let ls = param.unwrap_or(0);
                self.overrides.insert(ls, self.override_id);
                let changed = std::mem::take(&mut self.changed);
                self.changing = false;
                if changed.iter().any(|c| c.starts || c.formats) {
                    self.changes.insert(ls, changed);
                } else {
                    self.changes.remove(&ls);
                }
            }
            _ => {}
        }
    }

    fn change(&mut self, set: impl FnOnce(&mut Changed)) {
        if let Some(changed) = self.changed.last_mut().filter(|_| self.changing) {
            set(changed);
        }
    }

    fn level(&mut self, set: impl FnOnce(&mut ListLevel)) {
        if let Some(level) = self.list.as_mut().and_then(|l| l.levels.last_mut()) {
            set(level);
        }
    }

    /// Keep the list that was being read.
    fn end_list(&mut self) {
        if let Some(list) = self.list.take()
            && self.lists.len() < MAX_LISTS
        {
            self.lists.push(list);
        }
    }

    /// Count a paragraph of a list, and give the number or bullet it has: one more than the
    /// paragraph before it at its level, and the levels below it start again.
    fn counted(&mut self, cur: &Chars) -> Option<String> {
        let ls = cur.ls?;
        self.end_list();
        let id = *self.overrides.get(&ls)?;
        let list = self.lists.iter().find(|l| l.id == id)?;
        let at = usize::from(cur.ilvl).min(MAX_LEVELS - 1);
        // A level as these paragraphs have it: as their own number changes it, or as the list
        // says.
        let changes = self.changes.get(&ls);
        let changed = |n: usize| changes.and_then(|c| c.get(n));
        let level_of = |n: usize| changed(n).filter(|c| c.formats).and_then(|c| c.level.as_ref()).or_else(|| list.levels.get(n));
        let start_of = |n: usize| changed(n).filter(|c| c.starts).and_then(|c| c.start).or_else(|| list.levels.get(n).map(|l| l.start)).unwrap_or(1);
        let level = level_of(at)?;
        // One list counts on through every number that names it; a number that says where
        // its level starts begins there when it takes over.
        if self.counts.len() >= MAX_OVERRIDES && !self.counts.contains_key(&id) {
            return None;
        }
        let count = self.counts.entry(id).or_default();
        let anew = count.ls != Some(ls) && changed(at).is_some_and(|c| c.starts);
        count.ls = Some(ls);
        let slot = count.at.get_mut(at)?;
        *slot = Some(slot.filter(|_| !anew).map_or(start_of(at), |n| n.saturating_add(1)));
        for deeper in count.at.iter_mut().skip(at + 1) {
            *deeper = None;
        }
        let count = *count;
        // The words say how many characters they are, then the characters.
        let many = level.text.first().map_or(0, |c| *c as usize);
        let words = level.text.get(1..).unwrap_or_default();
        let words = words.get(..many.min(words.len())).unwrap_or_default();
        if level.nfc == 23 {
            return Some(words.iter().filter(|c| **c as u32 >= 9).collect());
        }
        let mut out = String::new();
        for c in words {
            match usize::try_from(*c as u32).ok().filter(|n| *n < MAX_LEVELS) {
                Some(of) => {
                    let nfc = level_of(of).map_or(0, |l| l.nfc);
                    let n = count.at.get(of).copied().flatten().unwrap_or_else(|| start_of(of));
                    out.push_str(&numbered(n, nfc));
                }
                None => out.push(*c),
            }
        }
        if level.text.is_empty() {
            out = format!("{}.", numbered(count.at.get(at).copied().flatten().unwrap_or(1), level.nfc));
        }
        Some(out)
    }

    /// Say which border the words that follow describe: a side of a cell of the row being
    /// defined, a side (or all sides) of the paragraph, or neither.
    fn sides(&mut self, cur: &mut Chars, cell: Option<usize>, para: Option<u8>) {
        self.def().side = cell;
        cur.side = para;
    }

    /// Change the sides the words being read describe.
    fn each_edge(&mut self, cur: &mut Chars, set: impl Fn(&mut Edge)) {
        let row = self.def();
        if let Some(edge) = row.side.and_then(|side| row.def.edges.get_mut(side)) {
            set(edge);
            return;
        }
        match cur.side {
            Some(ALL_SIDES) => cur.edges.iter_mut().for_each(set),
            Some(BETWEEN) => set(&mut cur.between),
            Some(side) => {
                if let Some(edge) = cur.edges.get_mut(usize::from(side)) {
                    set(edge);
                }
            }
            None => {}
        }
    }

    /// The border a paragraph stands inside: the one the paragraph before stands inside, when
    /// it is the same.
    fn frame_of(&mut self, cur: &Chars) -> Option<Arc<Frame>> {
        if !any_edge(&cur.edges) && cur.shade.is_none() {
            return None;
        }
        // Where the body says nothing, a little room so that the text does not touch the lines.
        let (above, beside) = (cur.pad.max(1), cur.pad.max(4));
        let frame = Frame {
            edges: cur.edges,
            padding: [above, beside, above, beside],
            indent: cur.indent,
            between: cur.between,
            fill: cur.shade,
            within: None,
            radius: 0,
        };
        match &self.frame {
            Some(last) if **last == frame => Some(Arc::clone(last)),
            _ => {
                let frame = Arc::new(frame);
                self.frame = Some(Arc::clone(&frame));
                Some(frame)
            }
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

    /// The data of a picture written as bytes (`\binN`, RTF 1.9.1, "Pictures"). A picture may
    /// be written partly so and partly as hex digits, but a byte is never split between them.
    fn picture_bytes(&mut self, data: &[u8], whole: bool) {
        let Some(pict) = &mut self.pict else { return };
        if pict.kind == Kind::Unread || pict.bad {
            return;
        }
        let room = crate::html::MAX_WRITTEN_BYTES.saturating_sub(self.written_bytes).saturating_sub(pict.bytes.len());
        if !whole || pict.half.is_some() || data.len() > room {
            pict.bad = true;
            return;
        }
        pict.bytes.extend_from_slice(data);
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
        let want = Some((u32::from(extent.0), u32::from(extent.1)));
        // What a drawing's text says is what the picture is called.
        let mut words = String::new();
        let mut drawing = |drawn: Option<crate::metafile::Drawn>| {
            drawn.map(|drawn| {
                words = drawn.words;
                drawn.file
            })
        };
        let file = match pict.kind {
            _ if !whole => None,
            Kind::Unread => None,
            Kind::File => Some(pict.bytes),
            Kind::Bitmap => crate::metafile::dib_file(&pict.bytes),
            Kind::Wmf => drawing(crate::metafile::wmf_drawn(&pict.bytes, want, self.letters)),
            Kind::Emf => drawing(crate::metafile::emf_drawn(&pict.bytes, want, self.letters)),
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
        let text = if place.is_some() { words } else { String::new() };
        self.push(Run { text, style: pict.style, picture: Some(Picture::Inline(place)), extent, ..Run::default() });
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

    /// Close the open paragraph, which is `depth` tables deep. `blank` keeps one with nothing
    /// in it, as the empty line it is.
    fn end_para(&mut self, cur: &Chars, depth: usize, blank: bool) {
        self.settle(depth);
        let mut runs = std::mem::take(&mut self.runs);
        if runs.is_empty() {
            // A list paragraph with nothing after its number is still one.
            if !blank && marker(&self.marker).is_none() {
                self.marker.clear();
                return;
            }
            runs.push(text_run("\u{a0}", Style::default()));
        }
        let frame = self.frame_of(cur);
        let above = cur.before.saturating_add(std::mem::replace(&mut self.below, cur.after)).min(u8::try_from(MAX_ABOVE).unwrap_or(u8::MAX));
        // A border stands at the paragraph's indent, and the text just inside it.
        let mut indent = if frame.is_some() { 0 } else { cur.indent };
        // The number the writer left; where it left none, the one the list's own table gives.
        let counted = self.counted(cur);
        let written = marker(&std::mem::take(&mut self.marker)).or_else(|| counted.as_deref().and_then(marker));
        let kind = match written {
            // The number or bullet stands in the room the indent leaves.
            Some(marker) => {
                indent = indent.max(1);
                ParaKind::Item(marker)
            }
            None => ParaKind::Body,
        };
        let tabs = if runs.iter().any(|r| r.picture.is_none() && r.text.contains('\t')) { tabs(cur) } else { Vec::new() };
        // A right-to-left paragraph that is told no side stands at the right.
        let align = if cur.rtl && !cur.sided { Align::Right } else { cur.align };
        let para = Block::Para(Para { runs, kind, align, indent, frame, above, tabs, leading: leading(cur), rtl: cur.rtl, ..Para::default() });
        if depth > 0 {
            self.open_cell().push(para);
        } else {
            self.end_table();
            self.blocks.push(para);
        }
        self.count = self.count.saturating_add(1);
        if self.blocks.len() >= MAX_BLOCKS || self.count > MAX_RUNS {
            self.truncated = true;
        }
    }

    fn end_table(&mut self) {
        self.settle(0);
        let table = std::mem::take(&mut self.table);
        self.blocks.extend(table.finish());
    }
}

/// The number or bullet of a list paragraph, from the text the writer left for it. A bullet
/// written as a character of a symbol font is the bullet it stands for there.
fn marker(written: &str) -> Option<String> {
    let text: String = written.chars().filter(|c| !c.is_control()).collect();
    let text = text.trim();
    let marker = match text {
        "" => return None,
        // The round and the square bullet of the symbol fonts, by their codes in those fonts
        // and as those codes read in the Windows code page; "o" is the hollow one.
        "\u{b7}" | "\u{f0b7}" | "\u{a7}" | "\u{f0a7}" | "\u{f0a8}" => "\u{2022}".to_string(),
        "o" => "\u{25e6}".to_string(),
        _ => text.chars().take(MAX_MARKER).collect(),
    };
    Some(marker)
}

/// The height a paragraph asks of its lines (RTF 1.9.1, "Spacing"): `\sl` is in 240ths of
/// a line's own height where `\slmult1` says so; otherwise twips, the least a line is where
/// positive and all it is where negative. None, or one line, is the lines' own height.
fn leading(cur: &Chars) -> Leading {
    let most = i32::from(LEADING_PX);
    let px = |twips: i32| u16::try_from((twips / 15).clamp(1, most)).unwrap_or(1);
    match (cur.sl, cur.sl_times) {
        (0, _) | (240, true) => Leading::Own,
        (n, true) => {
            let times = (i64::from(n.unsigned_abs()) * 100 / 240).clamp(i64::from(LEADING_TIMES.0), i64::from(LEADING_TIMES.1));
            Leading::Times(u16::try_from(times).unwrap_or(100))
        }
        (n, false) if n > 0 => Leading::AtLeast(px(n)),
        (n, false) => Leading::Exactly(px(i32::try_from(n.unsigned_abs()).unwrap_or(i32::MAX))),
    }
}

/// A number as a level of a list writes it (RTF 1.9.1, `\levelnfc`): 0 is 1, 2, 3; 1 and 2
/// are roman numerals, 3 and 4 letters, 22 has a nought before one figure. Kinds that are
/// not known, and numbers a kind cannot write, are written in figures.
fn numbered(n: i32, nfc: i32) -> String {
    let letters = |from: u8| {
        // a to z, then aa to zz, as lists do.
        let n = u32::try_from(n).ok().filter(|n| (1..=780).contains(n))?;
        let (times, which) = ((n - 1) / 26 + 1, (n - 1) % 26);
        let c = char::from(from.saturating_add(u8::try_from(which).unwrap_or(0)));
        Some(std::iter::repeat_n(c, usize::try_from(times).unwrap_or(1)).collect::<String>())
    };
    let roman = || {
        let mut n = u32::try_from(n).ok().filter(|n| (1..=3999).contains(n))?;
        let mut out = String::new();
        for (value, mark) in [
            (1000, "M"),
            (900, "CM"),
            (500, "D"),
            (400, "CD"),
            (100, "C"),
            (90, "XC"),
            (50, "L"),
            (40, "XL"),
            (10, "X"),
            (9, "IX"),
            (5, "V"),
            (4, "IV"),
            (1, "I"),
        ] {
            while n >= value {
                out.push_str(mark);
                n -= value;
            }
        }
        Some(out)
    };
    let written = match nfc {
        1 => roman(),
        2 => roman().map(|r| r.to_lowercase()),
        3 => letters(b'A'),
        4 => letters(b'a'),
        22 if (0..10).contains(&n) => Some(format!("0{n}")),
        255 => Some(String::new()),
        _ => None,
    };
    written.unwrap_or_else(|| n.to_string())
}

/// A paragraph's tab stops as the model keeps them: in pixels from where its text starts, the
/// nearest first. A stop at or before the start of the text is none.
fn tabs(cur: &Chars) -> Vec<Tab> {
    let mut out: Vec<Tab> = cur
        .tabs
        .iter()
        .filter_map(|(twips, kind, lead)| {
            let at = twips.saturating_sub(cur.left) / 15;
            (1..=MAX_TAB_AT).contains(&at).then(|| Tab { at: u16::try_from(at).unwrap_or(u16::MAX), kind: *kind, lead: *lead })
        })
        .collect();
    out.sort_by_key(|t| t.at);
    out.dedup_by_key(|t| t.at);
    out
}

/// Twips as pixels, 15 to the pixel, and never more than `most`.
fn pixels(twips: Option<i32>, most: i32) -> u8 {
    u8::try_from((twips.unwrap_or(0).max(0) / 15).min(most)).unwrap_or(0)
}

/// How many tables deep a paragraph is: none, or one where it says no more than that it is in
/// a table.
fn depth(cur: &Chars) -> usize {
    if cur.in_table { usize::from(cur.itap.clamp(1, MAX_NEST)) } else { 0 }
}

impl Level {
    /// End a cell of the open row: `blocks` is what it holds, `props` the row as it is defined.
    fn end_cell(&mut self, blocks: Vec<Block>, props: &RowDef) {
        let at = self.seen;
        self.seen = self.seen.saturating_add(1);
        let def = props.defs.get(at).copied().unwrap_or_default();
        // 15 twips to the pixel.
        let left = at.checked_sub(1).and_then(|i| props.defs.get(i)).map_or(0, |d| d.right);
        let width = u16::try_from((def.right.saturating_sub(left) / 15).clamp(0, 4096)).unwrap_or(0);
        // A cell merged into the one before it (`\clmrg`) makes that one a column wider: what it
        // holds, its width and the line at its right go to the whole.
        if def.across == Across::Next
            && let Some(first) = self.row.last_mut()
            && first.span < MAX_SPAN
        {
            first.span += 1;
            first.width = first.width.saturating_add(width).min(4096);
            first.blocks.extend(blocks);
            if let Some(right) = def.edges.get(1).filter(|e| e.width > 0) {
                let whole = first.edges.get_or_insert_with(Edges::default);
                if let Some(side) = whole.get_mut(1) {
                    *side = *right;
                }
            }
            if let Some(last) = self.merging.last_mut() {
                last.0 = def.right;
            }
            return;
        }
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
        let beside = u16::try_from((props.half_gap / 15).clamp(i32::from(CELL_PADDING), MAX_CELL_PADDING)).unwrap_or(CELL_PADDING);
        self.row.push(Cell {
            blocks,
            span: 1,
            width,
            padding: Some([1, beside, 1, beside]),
            background: def.fill,
            valign: def.valign,
            edges: any_edge(&edges).then_some(edges),
            ..Cell::default()
        });
    }

    /// End the open row. Cells that waited for the row to be defined are ended as `props` has
    /// them.
    fn end_row(&mut self, props: &RowDef) {
        for blocks in std::mem::take(&mut self.waiting) {
            self.end_cell(blocks, props);
        }
        if !self.cell.is_empty() {
            let blocks = std::mem::take(&mut self.cell);
            self.row.push(Cell { blocks, span: 1, ..Cell::default() });
        }
        self.seen = 0;
        let merging = std::mem::take(&mut self.merging);
        if !self.row.is_empty() {
            // A table is written the way its first row is.
            if self.rows.is_empty() {
                self.rtl = props.rtl;
            }
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

    /// The table as it stands where it was written: a table, or the paragraphs of its cells
    /// when it has only one column, which is only layout.
    fn finish(mut self) -> Vec<Block> {
        self.end_row(&RowDef::default());
        self.merge_down();
        if self.rows.is_empty() {
            return Vec::new();
        }
        let table = Table { rows: std::mem::take(&mut self.rows), padding: CELL_PADDING, snug: true, rtl: self.rtl, ..Table::default() };
        if table.columns() > 1 { vec![Block::Table(table)] } else { table.rows.into_iter().flatten().flat_map(|cell| cell.blocks).collect() }
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
