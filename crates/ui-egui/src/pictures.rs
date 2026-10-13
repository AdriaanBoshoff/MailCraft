//! Pictures a message body shows from the message itself: its own attachments (`cid:`
//! references) and pictures written into the body (`data:` addresses).
//!
//! Only what the body refers to is decoded, only as pictures, and only in this process:
//! nothing is fetched, written to disk or handed to another program. A picture is hostile input
//! like the rest of the message, so its file size, its dimensions and the memory it may take are
//! capped before anything is decoded (`openmailclient_engine::pictures`); one that cannot be read
//! stays a labelled placeholder.

use std::collections::HashMap;

use egui::{ColorImage, Context, TextureHandle, TextureOptions, Vec2, vec2};
use openmailclient_formats::rich::{Block, Doc, Picture};
use openmailclient_model::Item;

/// Pictures and decoded bytes kept for one message.
const MAX_PICTURES: usize = 64;
const MAX_KEPT_BYTES: usize = 128 << 20;
const MAX_DEPTH: usize = 64;

pub(crate) struct Shown {
    /// The picture, or the first frame of one that moves.
    pub texture: TextureHandle,
    /// Size in pixels as decoded, before any scaling to fit what is kept.
    pub size: Vec2,
    /// The frames of a picture that moves.
    pub film: Option<Reel>,
}

/// The frames of a picture that moves, the first among them.
pub(crate) struct Reel {
    /// Each frame, with the time from the start of one playing at which it ends, in
    /// thousandths of a second.
    frames: Vec<(TextureHandle, u64)>,
    /// How many times it is played; `None` is without end.
    loops: Option<u32>,
    /// When it was first shown, by the window's clock.
    since: f64,
}

impl Reel {
    #[cfg(test)]
    pub(crate) fn frames(&self) -> usize {
        self.frames.len()
    }

    /// Which frame is shown `passed` seconds after the first was, and for how long yet; no
    /// time when it has played as often as it asks and stands on its last frame.
    pub(crate) fn at(&self, passed: f64) -> (usize, Option<f64>) {
        let last = self.frames.len().saturating_sub(1);
        let whole = self.frames.last().map_or(0, |(_, ends)| *ends);
        if whole == 0 || !passed.is_finite() {
            return (if passed == f64::INFINITY && self.loops.is_some() { last } else { 0 }, None);
        }
        // Whole thousandths: the times of the frames are, and a sum of them is then exact.
        let passed = (passed.max(0.0) * 1000.0).round().min(1e18) as u64;
        if self.loops.is_some_and(|times| passed / whole >= u64::from(times)) {
            return (last, None);
        }
        let within = passed % whole;
        let frame = self.frames.iter().position(|(_, ends)| *ends > within).unwrap_or(last);
        (frame, self.frames.get(frame).map(|(_, ends)| (ends.saturating_sub(within).max(1)) as f64 / 1000.0))
    }
}

impl Shown {
    /// What is drawn now: the picture, or the frame of the moment when it moves and `moving`
    /// says such pictures are played. The window is asked to draw again when the next is due.
    pub(crate) fn frame(&self, ctx: &Context, moving: bool) -> egui::TextureId {
        let Some(reel) = self.film.as_ref().filter(|_| moving) else { return self.texture.id() };
        let (frame, left) = reel.at(ctx.input(|i| i.time) - reel.since);
        if let Some(left) = left {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left.min(60.0)));
        }
        reel.frames.get(frame).map_or(self.texture.id(), |(texture, _)| texture.id())
    }
}

/// The decoded pictures of the message being read, by content id.
#[derive(Default)]
pub(crate) struct Pictures {
    key: u64,
    by_id: HashMap<String, Option<Shown>>,
    /// Pictures written into the body, in the document's order.
    written: Vec<Option<Shown>>,
    kept_bytes: usize,
    /// Whether pictures that move are played; else each stands on its first frame.
    pub moving: bool,
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
            let attachment = openmailclient_engine::pictures::attached(item, id);
            let data = attachment.filter(|_| self.kept_bytes < MAX_KEPT_BYTES).map(|a| a.data.as_slice());
            let shown = data.and_then(|data| self.show(ctx, format!("picture-{}", self.by_id.len()), data));
            self.by_id.insert(id.to_string(), shown);
        }
        for data in doc.written.iter().skip(self.written.len()) {
            let count = self.by_id.len() + self.written.len();
            let within = count < MAX_PICTURES && self.kept_bytes < MAX_KEPT_BYTES;
            let shown = if within { self.show(ctx, format!("written-{count}"), data) } else { None };
            self.written.push(shown);
        }
    }

    /// A picture file as textures: every frame of one that moves, while there is room to keep
    /// them, else the picture standing still.
    fn show(&mut self, ctx: &Context, name: String, data: &[u8]) -> Option<Shown> {
        let room = MAX_KEPT_BYTES.saturating_sub(self.kept_bytes);
        let film = openmailclient_engine::pictures::film(data)
            .filter(|film| film.frames.len().saturating_mul(film.frames.first().map_or(0, |f| f.rgba.len())) <= room);
        if let Some(film) = film {
            let kept = [usize::try_from(film.width).ok()?, usize::try_from(film.height).ok()?];
            let mut ends = 0u64;
            let mut frames = Vec::with_capacity(film.frames.len());
            for (n, frame) in film.frames.iter().enumerate() {
                if frame.rgba.len() != kept[0].checked_mul(kept[1])?.checked_mul(4)? {
                    return None;
                }
                self.kept_bytes = self.kept_bytes.saturating_add(frame.rgba.len());
                ends = ends.saturating_add(u64::from(frame.millis));
                let image = ColorImage::from_rgba_unmultiplied(kept, &frame.rgba);
                frames.push((ctx.load_texture(format!("{name}-{n}"), image, TextureOptions::LINEAR), ends));
            }
            let texture = frames.first()?.0.clone();
            let size = vec2(f32::from(film.natural.0), f32::from(film.natural.1));
            let since = ctx.input(|i| i.time);
            return Some(Shown { texture, size, film: Some(Reel { frames, loops: film.loops, since }) });
        }
        let (image, size) = decode(data)?;
        self.kept_bytes = self.kept_bytes.saturating_add(image.pixels.len().saturating_mul(4));
        Some(Shown { texture: ctx.load_texture(name, image, TextureOptions::LINEAR), size, film: None })
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
/// this build reads (PNG, JPEG, GIF, BMP, WebP, TIFF, SVG), is over the limits, or is damaged.
pub(crate) fn decode(data: &[u8]) -> Option<(ColorImage, Vec2)> {
    let pixels = openmailclient_engine::pictures::decode(data)?;
    let kept = [usize::try_from(pixels.width).ok()?, usize::try_from(pixels.height).ok()?];
    let size = vec2(f32::from(pixels.natural.0), f32::from(pixels.natural.1));
    Some((ColorImage::from_rgba_unmultiplied(kept, &pixels.rgba), size))
}
