//! Table Style: a folder's messages as a table, one to a row, under the owner's name and the
//! folder's. The row of column names stands again at the top of each page the table goes on to.

use openmailclient_formats::rich::{Align, Block, Cell, Edge, Line, Para, Table, TableWidth};

use crate::flow::{Base, Flow};
use crate::fonts::Fonts;
use crate::memo::{HEAD_FONT, finish, owner, text};
use crate::{LayoutError, Printed, Setup};

/// Rows of one list that are printed, columns of it, and characters of one cell.
pub const MAX_ROWS: usize = 5_000;
const MAX_COLUMNS: usize = 12;
const MAX_CELL_CHARS: usize = 300;
/// The narrowest a column is, in points: room for a short word and the cell's own.
const NARROWEST: f32 = 32.0;

/// A column of the list.
pub struct Column<'a> {
    pub name: &'a str,
    /// Its part of the table's width, in hundredths.
    pub share: u8,
    /// What it holds stands at its right edge, as numbers do.
    pub right: bool,
}

/// One message of the list.
pub struct Row {
    /// A text for each column; one too many is left out, one too few leaves its cell empty.
    pub cells: Vec<String>,
    /// Printed in bold, as a message not yet read is listed.
    pub bold: bool,
}

/// A folder's list to print.
pub struct Listed<'a> {
    /// Whose mailbox it is printed from; heads the first page.
    pub owner: &'a str,
    /// The folder's name.
    pub title: &'a str,
    /// A line under the name: how many there are, how they are chosen and ordered.
    pub note: &'a str,
    pub columns: &'a [Column<'a>],
    pub rows: &'a [Row],
}

fn cell(words: &str, bold: bool, column: &Column, share: u8, under: Edge) -> Cell {
    let short: String = words.chars().take(MAX_CELL_CHARS).collect();
    let mut para = text(&short, bold, None);
    if column.right {
        para.align = Align::Right;
    }
    Cell {
        blocks: vec![Block::Para(para)],
        span: 1,
        rows: 1,
        share,
        padding: Some((2, 3)),
        edges: Some([Edge::default(), Edge::default(), under, Edge::default()]),
        ..Cell::default()
    }
}

/// Lay a folder's list out in Table Style, on pages set up as `setup` says.
pub fn list(list: &Listed, setup: impl Into<Setup>, fonts: &mut Fonts) -> Result<Printed, LayoutError> {
    let setup: Setup = setup.into();
    let none = |_: &crate::Picture| None;
    let mut flow = Flow::new(fonts, &setup, Base { family: HEAD_FONT.into(), size: 8.5 }, &none);
    let (x, width) = (flow.left(), flow.width());
    // No more columns than have room for a word each, and none narrower than that.
    let fit = ((width / NARROWEST) as usize).clamp(1, MAX_COLUMNS);
    let columns = list.columns.get(..list.columns.len().min(fit)).unwrap_or_default();
    let asked: f32 = columns.iter().map(|c| f32::from(c.share.clamp(1, 100))).sum::<f32>().max(1.0);
    let parts: Vec<f32> = columns.iter().map(|c| (f32::from(c.share.clamp(1, 100)) / asked).max(NARROWEST / width.max(1.0))).collect();
    let whole: f32 = parts.iter().sum::<f32>().max(0.01);
    let shares: Vec<u8> = parts.iter().map(|part| (part / whole * 100.0).round().clamp(1.0, 100.0) as u8).collect();
    let share = |n: usize| shares.get(n).copied().unwrap_or(1);
    let mut blocks = vec![Block::Para(owner(list.owner)), Block::Para(Para::default()), Block::Para(text(list.title, true, Some(11.0)))];
    if !list.note.trim().is_empty() {
        blocks.push(Block::Para(text(list.note, false, None)));
    }
    blocks.push(Block::Para(Para::default()));
    if !columns.is_empty() {
        let dark = Edge { width: 1, color: Some([0, 0, 0]), line: Line::Solid };
        let light = Edge { width: 1, color: Some([200, 200, 200]), line: Line::Solid };
        let mut rows: Vec<Vec<Cell>> = Vec::with_capacity(list.rows.len().min(MAX_ROWS).saturating_add(1));
        rows.push(columns.iter().enumerate().map(|(n, column)| cell(column.name, true, column, share(n), dark)).collect());
        for row in list.rows.iter().take(MAX_ROWS) {
            rows.push(
                columns
                    .iter()
                    .enumerate()
                    .map(|(n, column)| cell(row.cells.get(n).map_or("", String::as_str), row.bold, column, share(n), light))
                    .collect(),
            );
        }
        blocks.push(Block::Table(Table { rows, width: TableWidth::Fill, snug: true, repeat: true, ..Table::default() }));
    }
    let start = flow.start();
    flow.blocks(&blocks, x, width, start, 0);
    let (pages, full) = finish(flow, &setup)?;
    Ok(Printed { pages, faces: fonts.faces().to_vec(), truncated: full || list.rows.len() > MAX_ROWS })
}
