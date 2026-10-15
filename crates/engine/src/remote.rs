//! Pictures a message names on a web server, kept once they were fetched on the reader's word
//! (`mail.downloadPictures`). Nothing here fetches anything: the command does, only when asked,
//! and what it brought is kept in memory only, never written to disk or into the data file.
//!
//! What is kept is capped: each picture by `openmailclient_net::http::MAX_PICTURE`, all of them
//! together by [`MAX_KEPT`], the oldest let go first.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use openmailclient_formats::rich::{Block, Doc, Picture};

/// Bytes of downloaded pictures kept in all.
pub const MAX_KEPT: usize = 64 << 20;
/// Pictures fetched for one message.
pub const MAX_PER_MESSAGE: usize = 64;
/// Tables within tables looked into for pictures.
const MAX_DEPTH: usize = 32;

/// Pictures downloaded on the reader's word, by the address the message gave.
#[derive(Debug, Default)]
pub struct Downloaded {
    by_address: HashMap<String, Arc<[u8]>>,
    /// Addresses in the order they were kept, the oldest first.
    order: VecDeque<String>,
    bytes: usize,
    /// Counts each change, so whoever shows the pictures knows to look again.
    pub generation: u64,
}

impl Downloaded {
    pub fn get(&self, address: &str) -> Option<&Arc<[u8]>> {
        self.by_address.get(address)
    }

    pub fn contains(&self, address: &str) -> bool {
        self.by_address.contains_key(address)
    }

    pub fn len(&self) -> usize {
        self.by_address.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_address.is_empty()
    }

    /// Keep a picture, letting the oldest go while there is no room for it.
    pub fn keep(&mut self, address: String, bytes: Vec<u8>) {
        if bytes.len() > MAX_KEPT {
            return;
        }
        if let Some(old) = self.by_address.remove(&address) {
            self.bytes = self.bytes.saturating_sub(old.len());
            self.order.retain(|a| a != &address);
        }
        while self.bytes.saturating_add(bytes.len()) > MAX_KEPT {
            let Some(oldest) = self.order.pop_front() else { break };
            if let Some(gone) = self.by_address.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(gone.len());
            }
        }
        self.bytes = self.bytes.saturating_add(bytes.len());
        self.order.push_back(address.clone());
        self.by_address.insert(address, bytes.into());
        self.generation = self.generation.wrapping_add(1);
    }
}

/// The addresses of the pictures `doc` names on a server, each once, in the body's order.
pub fn addresses(doc: &Doc) -> Vec<String> {
    let mut out = Vec::new();
    walk(&doc.blocks, &mut out, 0);
    out
}

fn walk(blocks: &[Block], out: &mut Vec<String>, depth: usize) {
    for block in blocks {
        if out.len() >= MAX_PER_MESSAGE {
            return;
        }
        match block {
            Block::Para(p) => {
                for run in &p.runs {
                    if let Some(Picture::Remote(address)) = &run.picture
                        && !address.is_empty()
                        && out.len() < MAX_PER_MESSAGE
                        && !out.contains(address)
                    {
                        out.push(address.clone());
                    }
                }
            }
            Block::Table(t) if depth < MAX_DEPTH => {
                for cell in t.rows.iter().flatten() {
                    walk(&cell.blocks, out, depth + 1);
                }
            }
            _ => {}
        }
    }
}
