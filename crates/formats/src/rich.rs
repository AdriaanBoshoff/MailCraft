//! The formatted document a message body is read into: blocks of styled runs, with tables.
//! It is what the reading pane draws; it holds no markup, no scripts and no fetched content.
//! A picture the body itself spells out is kept as the bytes the body gave; they are not decoded
//! here.

use std::sync::Arc;

/// Red, green, blue.
pub type Rgb = [u8; 3];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Doc {
    pub blocks: Vec<Block>,
    /// The body was longer than the reader's limits; the rest is not in the document.
    pub truncated: bool,
    /// Pictures and backgrounds that point at a server. None of them was fetched.
    pub remote_blocked: usize,
    /// Pictures written into the body itself, each as the bytes of a picture file.
    pub written: Vec<Arc<[u8]>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Para(Para),
    /// A horizontal line.
    Rule,
    Table(Table),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Para {
    pub runs: Vec<Run>,
    pub kind: ParaKind,
    pub align: Align,
    /// List and definition nesting.
    pub indent: u8,
    /// Quotation nesting; each level is drawn with a bar.
    pub quote: u8,
    /// The paragraph has a margin above it.
    pub gap: bool,
    /// The border the paragraph stands inside. Paragraphs that follow one another and share one
    /// frame stand inside one border.
    pub frame: Option<Arc<Frame>>,
    /// The paragraph starts below any picture standing at a side.
    pub clear: bool,
}

/// One side of a border.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edge {
    /// Pixels; 0 is no line.
    pub width: u8,
    /// `None` is the colour of the text.
    pub color: Option<Rgb>,
    pub line: Line,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Line {
    #[default]
    Solid,
    Dashed,
    Dotted,
    Double,
}

/// The four sides of a border: top, right, bottom, left.
pub type Edges = [Edge; 4];

/// Whether any side has a line.
pub fn any_edge(edges: &Edges) -> bool {
    edges.iter().any(|e| e.width > 0)
}

/// A border round one or more paragraphs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    pub edges: Edges,
    /// Room between the border and what it holds: above and below, then left and right.
    pub padding: (u16, u16),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ParaKind {
    #[default]
    Body,
    /// Heading level 1 to 6.
    Heading(u8),
    /// Preformatted: line breaks and spaces are kept.
    Pre,
    /// A list item with its marker ("•" or "3.").
    Item(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    /// For a picture this is its alternative text.
    pub text: String,
    pub style: Style,
    pub picture: Option<Picture>,
    /// Width and height the body asks a picture to be shown at, in pixels; 0 where it gives none.
    pub extent: (u16, u16),
    /// A picture that stands at one side with the paragraph's text beside it.
    pub float: Float,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Float {
    #[default]
    None,
    Left,
    Right,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub mono: bool,
    /// The fonts asked for, by name, as the body lists them (first choice first, separated by
    /// commas). They are names only: whether one is installed is the reader's business.
    pub family: Option<Arc<str>>,
    pub script: Script,
    /// Font size in points; `None` is the reader's default.
    pub size: Option<f32>,
    pub color: Option<Rgb>,
    pub background: Option<Rgb>,
    /// Where a link points. It is shown, never followed by the reader.
    pub link: Option<Arc<str>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Script {
    #[default]
    None,
    Super,
    Sub,
}

/// Where a picture's data is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Picture {
    /// An attachment of the same message, by content id.
    Attached(String),
    /// Data written into the body itself: its place in [`Doc::written`], or `None` where the
    /// data was not kept (not readable, too large, or of a kind that is not read).
    Inline(Option<usize>),
    /// On a server. Not fetched.
    Remote,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub rows: Vec<Vec<Cell>>,
    pub border: bool,
    pub width: TableWidth,
    /// Where the table stands when it is narrower than the room it has.
    pub align: Align,
    /// Room between a cell's edge and what it holds, in pixels, for cells that set none.
    pub padding: u16,
    /// Room between cells, and between the cells and the table's edge, in pixels.
    pub spacing: u16,
    /// The table's own border, round all its cells.
    pub frame: Edges,
    /// The colour of the lines `border` asks for; `None` is the reader's.
    pub line: Option<Rgb>,
}

/// How wide a table asks to be.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableWidth {
    /// As wide as what it holds.
    #[default]
    Content,
    Pixels(u16),
    /// This many hundredths of the room there is, 1 to 99.
    Percent(u8),
    /// All the room there is.
    Fill,
}

/// Where what a cell holds stands when the cell is taller than it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    pub blocks: Vec<Block>,
    pub header: bool,
    /// Columns the cell covers, at least 1.
    pub span: u16,
    /// Fills the whole cell, behind its text.
    pub background: Option<Rgb>,
    /// Width the cell asks for, in pixels; 0 where it gives none.
    pub width: u16,
    /// The cell's own padding, above and below, then left and right; `None` is the table's.
    pub padding: Option<(u16, u16)>,
    /// Width the cell asks for as hundredths of the table's, 1 to 100; 0 where it gives none.
    pub share: u8,
    pub valign: VAlign,
    /// Rows the cell covers, at least 1.
    pub rows: u16,
    /// The cell's own border; `None` is what the table gives its cells.
    pub edges: Option<Edges>,
}

impl Doc {
    /// The document's text, one line per paragraph; cells of a row are separated by tabs.
    pub fn text(&self) -> String {
        let mut out = String::new();
        blocks_text(&self.blocks, &mut out, 0);
        out.trim_end().to_string()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

impl Para {
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

impl Table {
    /// Columns in the widest row, counting spans and the cells that reach down from rows above.
    pub fn columns(&self) -> usize {
        if !self.reaches_down() {
            return self.rows.iter().map(|r| r.iter().map(|c| usize::from(c.span.max(1))).sum::<usize>()).max().unwrap_or(0);
        }
        self.walk(|_, _, _| {})
    }

    /// The column each cell starts in, row by row. A cell that covers several rows takes its
    /// columns in the rows below it too, and the cells of those rows stand after it
    /// (HTML 4.01 section 11.2.6.1).
    pub fn places(&self) -> Vec<Vec<usize>> {
        let mut places: Vec<Vec<usize>> = self.rows.iter().map(|row| Vec::with_capacity(row.len())).collect();
        self.walk(|row, _, column| {
            if let Some(known) = places.get_mut(row) {
                known.push(column);
            }
        });
        places
    }

    fn reaches_down(&self) -> bool {
        self.rows.iter().flatten().any(|c| c.rows > 1)
    }

    /// Go through the cells in order, telling `place` each one's row, number in the row and
    /// column. Returns the number of columns.
    fn walk(&self, mut place: impl FnMut(usize, usize, usize)) -> usize {
        // Rows each column is still covered for from above. Columns past the limit are not
        // tracked: a cell there covers its own row only.
        const TRACKED: usize = 1024;
        let mut down: Vec<u16> = Vec::new();
        let mut live = 0usize;
        let mut columns = 0usize;
        for (r, row) in self.rows.iter().enumerate() {
            let mut col = 0usize;
            for (i, cell) in row.iter().enumerate() {
                while down.get(col).is_some_and(|rows| *rows > 0) {
                    col += 1;
                }
                place(r, i, col);
                let span = usize::from(cell.span.max(1));
                if cell.rows > 1 {
                    for k in col..col.saturating_add(span).min(TRACKED) {
                        if down.len() <= k {
                            down.resize(k + 1, 0);
                        }
                        if let Some(rows) = down.get_mut(k) {
                            live += usize::from(*rows == 0);
                            *rows = cell.rows;
                        }
                    }
                }
                col = col.saturating_add(span);
            }
            columns = columns.max(col);
            if live > 0 {
                for rows in down.iter_mut().filter(|rows| **rows > 0) {
                    *rows -= 1;
                    live -= usize::from(*rows == 0);
                }
            }
        }
        columns.max(down.len())
    }
}

/// Characters of text in `blocks`, counted up to `cap`. Used to share out column widths.
pub fn text_len(blocks: &[Block], cap: usize) -> usize {
    let mut n = 0usize;
    for block in blocks {
        if n >= cap {
            break;
        }
        match block {
            Block::Para(p) => n = n.saturating_add(p.runs.iter().map(|r| r.text.len()).sum::<usize>()),
            Block::Rule => {}
            Block::Table(t) => {
                for cell in t.rows.iter().flatten() {
                    n = n.saturating_add(text_len(&cell.blocks, cap.saturating_sub(n)));
                }
            }
        }
    }
    n.min(cap)
}

fn blocks_text(blocks: &[Block], out: &mut String, depth: usize) {
    for block in blocks {
        match block {
            Block::Para(p) => {
                if let ParaKind::Item(marker) = &p.kind {
                    out.push_str(marker);
                    out.push(' ');
                }
                out.push_str(&p.text());
                out.push('\n');
            }
            Block::Rule => out.push_str("----\n"),
            Block::Table(t) => {
                for row in &t.rows {
                    let mut cells = Vec::new();
                    for cell in row {
                        let mut s = String::new();
                        if depth < 64 {
                            blocks_text(&cell.blocks, &mut s, depth + 1);
                        }
                        cells.push(s.trim_end().replace('\n', " "));
                    }
                    out.push_str(&cells.join("\t"));
                    out.push('\n');
                }
            }
        }
    }
}
