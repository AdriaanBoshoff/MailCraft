//! Memo Style: the owner's name over a thick line, the message's fields as a list of names and
//! values, then the body, and each page's number at its foot.

use std::sync::Arc;

use mailcraft_formats::rich::{Block, Cell, Doc, Edge, Frame, Line, Para, Run, Style, Table, TableWidth};

use crate::flow::{Base, FOOT, Flow};
use crate::fonts::Fonts;
use crate::{LayoutError, Paper, Printed, Text};

/// Fields a header shows, and characters of one field.
const MAX_FIELDS: usize = 32;
const MAX_FIELD_CHARS: usize = 20_000;
/// Width of the column of field names, in pixels.
const NAMES: u16 = 110;

const HEAD_FONT: &str = "Segoe UI, Arial, Liberation Sans, sans-serif";
const BODY_FONT: &str = "Times New Roman, Liberation Serif, serif";
const PLAIN_FONT: &str = "Calibri, Carlito, Arial, sans-serif";

/// A message to print.
pub struct Memo<'a> {
    /// Whose mailbox it is printed from; heads the first page.
    pub owner: &'a str,
    /// Names and values, in the order they are listed. A value that is empty is left out.
    pub fields: &'a [(&'a str, String)],
    pub body: &'a Doc,
    /// The body is plain text, not a formatted one.
    pub plain: bool,
}

fn text(text: &str, bold: bool, size: Option<f32>) -> Para {
    let text: String = text.chars().filter(|c| !c.is_control()).take(MAX_FIELD_CHARS).collect();
    Para { runs: vec![Run { text, style: Style { bold, size, ..Style::default() }, ..Run::default() }], ..Para::default() }
}

fn head(memo: &Memo) -> Vec<Block> {
    let line = Edge { width: 3, color: Some([0, 0, 0]), line: Line::Solid };
    let frame = Arc::new(Frame { edges: [Edge::default(), Edge::default(), line, Edge::default()], padding: (3, 0) });
    let mut owner = text(memo.owner, true, Some(12.0));
    owner.frame = Some(frame);
    let cell = |para: Para, width: u16| Cell { blocks: vec![Block::Para(para)], span: 1, rows: 1, width, padding: Some((1, 0)), ..Cell::default() };
    let rows: Vec<Vec<Cell>> = memo
        .fields
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .take(MAX_FIELDS)
        .map(|(name, value)| vec![cell(text(&format!("{name}:"), true, None), NAMES), cell(text(value, false, None), 0)])
        .collect();
    let mut blocks = vec![Block::Para(owner), Block::Para(Para::default())];
    if !rows.is_empty() {
        blocks.push(Block::Table(Table { rows, width: TableWidth::Fill, snug: true, ..Table::default() }));
        blocks.push(Block::Para(Para::default()));
    }
    blocks
}

/// The page's number, in the middle of its foot.
fn number(flow: &mut Flow, page: usize) {
    const SIZE: f32 = 8.0;
    let mut chain = flow.fonts.chain(HEAD_FONT, false, false, false);
    let mut set = Vec::new();
    for ch in (page + 1).to_string().chars() {
        if let Some((pick, glyph)) = flow.fonts.resolve(&mut chain, ch) {
            let advance = flow.fonts.face(pick.face).map_or(0.0, |f| f.advance(glyph, SIZE));
            set.push((pick, glyph, ch, advance));
        }
    }
    let wide: f32 = set.iter().map(|s| s.3).sum();
    let mut x = flow.left() + (flow.width() - wide) / 2.0;
    let y = flow.paper.height - flow.paper.margin - (FOOT - SIZE) / 2.0;
    let Some(page) = flow.pages.get_mut(page) else { return };
    for (pick, glyph, ch, advance) in set {
        page.texts.push(Text {
            x,
            y,
            face: pick.face,
            size: SIZE,
            color: [0, 0, 0],
            glyphs: vec![(glyph, ch)],
            width: advance,
            bold: pick.bold,
            slant: pick.slant,
        });
        x += advance;
    }
}

/// Lay a message out in Memo Style.
pub fn memo(memo: &Memo, paper: Paper, fonts: &mut Fonts) -> Result<Printed, LayoutError> {
    let mut flow = Flow::new(fonts, paper, Base { family: HEAD_FONT.into(), size: 8.0 });
    let (x, width) = (flow.left(), flow.width());
    let start = flow.start();
    let at = flow.blocks(&head(memo), x, width, start, 0);
    flow.base = if memo.plain { Base { family: PLAIN_FONT.into(), size: 11.0 } } else { Base { family: BODY_FONT.into(), size: 12.0 } };
    flow.blocks(&memo.body.blocks, x, width, at, 0);
    if flow.no_font {
        return Err(LayoutError::NoFont);
    }
    for page in 0..flow.pages.len() {
        number(&mut flow, page);
    }
    let truncated = flow.full || memo.body.truncated;
    let mut pages = std::mem::take(&mut flow.pages);
    for page in &mut pages {
        // A fill deeper in tables lies over a shallower one.
        page.fills.sort_by_key(|fill| fill.depth);
    }
    Ok(Printed { pages, faces: fonts.faces().to_vec(), truncated })
}
