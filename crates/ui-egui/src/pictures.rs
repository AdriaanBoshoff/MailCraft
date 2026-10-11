//! Pictures a message body shows from the message itself: its own attachments (`cid:`
//! references) and pictures written into the body (`data:` addresses).
//!
//! Only what the body refers to is decoded, only as pictures, and only in this process:
//! nothing is fetched, written to disk or handed to another program. A picture is hostile input
//! like the rest of the message, so its file size, its dimensions and the memory it may take are
//! capped before anything is decoded (`mailcraft_engine::pictures`); one that cannot be read
//! stays a labelled placeholder.

use std::collections::HashMap;

use egui::{ColorImage, Context, TextureHandle, TextureOptions, Vec2, vec2};
use mailcraft_formats::rich::{Block, Doc, Picture};
use mailcraft_model::Item;

/// Pictures and decoded bytes kept for one message.
const MAX_PICTURES: usize = 64;
const MAX_KEPT_BYTES: usize = 128 << 20;
const MAX_DEPTH: usize = 64;

pub(crate) struct Shown {
    pub texture: TextureHandle,
    /// Size in pixels as decoded, before any scaling to fit what is kept.
    pub size: Vec2,
}

/// The decoded pictures of the message being read, by content id.
#[derive(Default)]
pub(crate) struct Pictures {
    key: u64,
    by_id: HashMap<String, Option<Shown>>,
    /// Pictures written into the body, in the document's order.
    written: Vec<Option<Shown>>,
    kept_bytes: usize,
}

impl Pictures {
    pub(crate) fn get(&self, content_id: &str) -> Option<&Shown> {
        self.by_id.get(content_id)?.as_ref()
    }

    /// A picture written into the body, by its place in the document's list.
    pub(crate) fn written(&self, index: usize) -> Option<&Shown> {
        self.written.get(index)?.as_ref()
    }

    /// Decode the pictures `doc` refers to, once per message. `key` names the message's body.
    pub(crate) fn load(&mut self, ctx: &Context, key: u64, doc: &Doc, item: &Item) {
        if self.key != key {
            *self = Pictures { key, ..Pictures::default() };
        }
        let mut wanted = Vec::new();
        content_ids(&doc.blocks, &mut wanted, 0);
        for id in wanted {
            if self.by_id.len() >= MAX_PICTURES || self.by_id.contains_key(id) {
                continue;
            }
            let attachment = mailcraft_engine::pictures::attached(item, id);
            let image = attachment.filter(|_| self.kept_bytes < MAX_KEPT_BYTES).and_then(|a| decode(&a.data));
            let shown = image.map(|(image, size)| {
                self.kept_bytes = self.kept_bytes.saturating_add(image.pixels.len().saturating_mul(4));
                Shown { texture: ctx.load_texture(format!("picture-{}", self.by_id.len()), image, TextureOptions::LINEAR), size }
            });
            self.by_id.insert(id.to_string(), shown);
        }
        for data in doc.written.iter().skip(self.written.len()) {
            let count = self.by_id.len() + self.written.len();
            let image = if count < MAX_PICTURES && self.kept_bytes < MAX_KEPT_BYTES { decode(data) } else { None };
            let shown = image.map(|(image, size)| {
                self.kept_bytes = self.kept_bytes.saturating_add(image.pixels.len().saturating_mul(4));
                Shown { texture: ctx.load_texture(format!("written-{count}"), image, TextureOptions::LINEAR), size }
            });
            self.written.push(shown);
        }
    }
}

fn content_ids<'a>(blocks: &'a [Block], out: &mut Vec<&'a str>, depth: usize) {
    for block in blocks {
        match block {
            Block::Para(p) => out.extend(p.runs.iter().filter_map(|r| match &r.picture {
                Some(Picture::Attached(id)) => Some(id.as_str()),
                _ => None,
            })),
            Block::Table(t) if depth < MAX_DEPTH => {
                for cell in t.rows.iter().flatten() {
                    content_ids(&cell.blocks, out, depth + 1);
                }
            }
            _ => {}
        }
    }
}

/// A picture file → pixels and its size as written. `None` for anything that is not a picture
/// this build reads (PNG, JPEG, GIF, BMP), is over the limits, or is damaged.
pub(crate) fn decode(data: &[u8]) -> Option<(ColorImage, Vec2)> {
    let pixels = mailcraft_engine::pictures::decode(data)?;
    let kept = [usize::try_from(pixels.width).ok()?, usize::try_from(pixels.height).ok()?];
    let size = vec2(f32::from(pixels.natural.0), f32::from(pixels.natural.1));
    Some((ColorImage::from_rgba_unmultiplied(kept, &pixels.rgba), size))
}
