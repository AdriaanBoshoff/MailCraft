//! The formatted document a message body is read into: blocks of styled runs, with tables.
//! It is what the reading pane draws; it holds no markup, no scripts and no fetched content.

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
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub mono: bool,
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

/// Where a picture's data is. The document never holds the data itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Picture {
    /// An attachment of the same message, by content id.
    Attached(String),
    /// Data written into the body itself.
    Inline,
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
    /// Columns in the widest row, counting spans.
    pub fn columns(&self) -> usize {
        self.rows.iter().map(|r| r.iter().map(|c| usize::from(c.span.max(1))).sum::<usize>()).max().unwrap_or(0)
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
