//! Pictures a message body shows from the message's own attachments (`cid:` references).
//!
//! Only attachments the body refers to are decoded, only as pictures, and only in this process:
//! nothing is fetched, written to disk or handed to another program. A picture is hostile input
//! like the rest of the message, so its file size, its dimensions and the memory it may take are
//! capped before anything is decoded; one that cannot be read stays a labelled placeholder.

use std::collections::HashMap;

use egui::{ColorImage, Context, TextureHandle, TextureOptions, Vec2, vec2};
use mailcraft_formats::rich::{Block, Doc, Picture};
use mailcraft_model::Item;

/// Largest picture file that is decoded.
const MAX_FILE: usize = 16 << 20;
/// Largest width or height that is decoded.
const MAX_SIDE: u32 = 8192;
/// Memory one decode may take.
const MAX_ALLOC: u64 = 96 << 20;
/// A longer side than this is scaled down before it is kept.
const MAX_KEPT_SIDE: u32 = 2048;
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
    kept_bytes: usize,
}

impl Pictures {
    pub(crate) fn get(&self, content_id: &str) -> Option<&Shown> {
        self.by_id.get(content_id)?.as_ref()
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
            let attachment = item.attachments.iter().find(|a| a.content_id().is_some_and(|c| same_id(c, id)));
            let image = attachment.filter(|_| self.kept_bytes < MAX_KEPT_BYTES).and_then(|a| decode(&a.data));
            let shown = image.map(|(image, size)| {
                self.kept_bytes = self.kept_bytes.saturating_add(image.pixels.len().saturating_mul(4));
                Shown { texture: ctx.load_texture(format!("picture-{}", self.by_id.len()), image, TextureOptions::LINEAR), size }
            });
            self.by_id.insert(id.to_string(), shown);
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

/// A content id as the attachment has it against one as the body writes it.
fn same_id(attachment: &str, body: &str) -> bool {
    let bare = |s: &str| s.trim().trim_start_matches('<').trim_end_matches('>').to_string();
    bare(attachment).eq_ignore_ascii_case(&bare(body))
}

/// A picture file → pixels and its size as written. `None` for anything that is not a picture
/// this build reads (PNG, JPEG, GIF, BMP), is over the limits, or is damaged.
pub(crate) fn decode(data: &[u8]) -> Option<(ColorImage, Vec2)> {
    if data.len() > MAX_FILE {
        return None;
    }
    // The decoders are not ours. Should one panic on a damaged file, the picture is dropped and
    // the window stays up.
    std::panic::catch_unwind(|| {
        let mut reader = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().ok()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_SIDE);
        limits.max_image_height = Some(MAX_SIDE);
        limits.max_alloc = Some(MAX_ALLOC);
        reader.limits(limits);
        let image = reader.decode().ok()?;
        let (w, h) = (image.width(), image.height());
        if w == 0 || h == 0 {
            return None;
        }
        let image = if w.max(h) > MAX_KEPT_SIDE { image.thumbnail(MAX_KEPT_SIDE, MAX_KEPT_SIDE) } else { image };
        let rgba = image.to_rgba8();
        let kept = [usize::try_from(rgba.width()).ok()?, usize::try_from(rgba.height()).ok()?];
        let size = vec2(f32::from(u16::try_from(w).ok()?), f32::from(u16::try_from(h).ok()?));
        Some((ColorImage::from_rgba_unmultiplied(kept, rgba.as_raw()), size))
    })
    .ok()
    .flatten()
}
