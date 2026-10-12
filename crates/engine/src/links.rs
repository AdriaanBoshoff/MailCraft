//! The links of a message, as a reader or an agent may ask for them: what each says, where it
//! goes, and what is wrong with it. Nothing here follows a link.

use openmailclient_formats::links::{self, Concern};
use openmailclient_formats::rich::Block;
use openmailclient_model::Item;

/// Links told of one message, and characters of a link's text and of its address.
pub const MAX_LINKS: usize = 500;
const MAX_TEXT: usize = 200;
const MAX_ADDRESS: usize = 2048;
const MAX_DEPTH: usize = 32;

/// One link of a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// What the link says, on one line.
    pub text: String,
    /// Where it points, as the message writes it, without control characters.
    pub address: String,
    /// The place the address goes to, where it has one.
    pub goes: Option<String>,
    pub concern: Option<Concern>,
}

fn shown(text: &str, most: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(most).collect()
}

fn walk(blocks: &[Block], out: &mut Vec<Found>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    for block in blocks {
        match block {
            Block::Para(para) => {
                for (at, run) in para.runs.iter().enumerate() {
                    let Some(link) = run.style.link.as_ref() else { continue };
                    // Runs that follow one another to the same address are one link.
                    let goes_on = at.checked_sub(1).and_then(|before| para.runs.get(before)).is_some_and(|r| r.style.link.as_ref() == Some(link));
                    if goes_on || link.trim().is_empty() {
                        continue;
                    }
                    if out.len() >= MAX_LINKS {
                        return;
                    }
                    out.push(Found {
                        text: shown(links::text_around(&para.runs, at).trim(), MAX_TEXT),
                        address: shown(link, MAX_ADDRESS),
                        goes: links::place(link).map(|place| place.host),
                        concern: links::concern_at(&para.runs, at),
                    });
                }
            }
            Block::Table(table) => {
                for cell in table.rows.iter().flatten() {
                    walk(&cell.blocks, out, depth + 1);
                }
            }
            Block::Rule => {}
        }
    }
}

/// The links of `item`'s body, in the order they stand in it.
pub fn of(item: &Item) -> Vec<Found> {
    let (doc, _) = crate::print::body(item);
    let mut out = Vec::new();
    walk(&doc.blocks, &mut out, 0);
    out
}
