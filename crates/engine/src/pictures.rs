//! Pictures a message body shows from the message itself: its own attachments (`cid:`
//! references) and pictures written into the body (`data:` addresses), read into pixels.
//!
//! Only what a body refers to is decoded, only as pictures, and only in this process: nothing
//! is fetched, written to disk or handed to another program. A picture is hostile input like the
//! rest of the message, so its file size, its dimensions and the memory it may take are capped
//! before anything is decoded.

use mailcraft_model::{Attachment, Item};

/// Largest picture file that is decoded.
const MAX_FILE: usize = 16 << 20;
/// Largest width or height that is decoded.
const MAX_SIDE: u32 = 8192;
/// Memory one decode may take.
const MAX_ALLOC: u64 = 96 << 20;
/// A longer side than this is scaled down before it is kept.
const MAX_KEPT_SIDE: u32 = 2048;

/// A decoded picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixels {
    /// Size of what is kept, in pixels.
    pub width: u32,
    pub height: u32,
    /// Red, green, blue and cover for each pixel, row after row from the top.
    pub rgba: Vec<u8>,
    /// Size as the file has it, before any scaling to fit what is kept.
    pub natural: (u16, u16),
}

/// A picture file → pixels. `None` for anything that is not a picture this build reads (PNG,
/// JPEG, GIF, BMP), is over the limits, or is damaged.
pub fn decode(data: &[u8]) -> Option<Pixels> {
    if data.len() > MAX_FILE {
        return None;
    }
    // The decoders are not ours. Should one panic on a damaged file, the picture is dropped and
    // the program goes on.
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
        let natural = (u16::try_from(w).ok()?, u16::try_from(h).ok()?);
        let image = if w.max(h) > MAX_KEPT_SIDE { image.thumbnail(MAX_KEPT_SIDE, MAX_KEPT_SIDE) } else { image };
        let rgba = image.to_rgba8();
        Some(Pixels { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw(), natural })
    })
    .ok()
    .flatten()
}

/// A content id as the attachment has it against one as the body writes it.
pub fn same_id(attachment: &str, body: &str) -> bool {
    let bare = |s: &str| s.trim().trim_start_matches('<').trim_end_matches('>').to_string();
    bare(attachment).eq_ignore_ascii_case(&bare(body))
}

/// The attachment of `item` a body means by a content id.
pub fn attached<'a>(item: &'a Item, content_id: &str) -> Option<&'a Attachment> {
    item.attachments.iter().find(|a| a.content_id().is_some_and(|c| same_id(c, content_id)))
}
