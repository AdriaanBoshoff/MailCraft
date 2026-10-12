//! Pictures a message body shows from the message itself: its own attachments (`cid:`
//! references) and pictures written into the body (`data:` addresses), read into pixels.
//!
//! Only what a body refers to is decoded, only as pictures, and only in this process: nothing
//! is fetched, written to disk or handed to another program. A picture is hostile input like the
//! rest of the message, so its file size, its dimensions and the memory it may take are capped
//! before anything is decoded.

use std::sync::Arc;

use mailcraft_formats::rich::{Block, Doc, Filed, Picture};
use mailcraft_model::{Attachment, Item, pid};

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

/// Pictures of attachments one body is given, and how deep its tables are followed.
const MAX_RENDERINGS: usize = 64;
const MAX_DEPTH: usize = 64;

/// The attachments that stand in an RTF body, in the order of their places ([MS-OXRTFEX]
/// 2.2.3.4): those that are not hidden and whose rendering position is not 0xFFFFFFFF, by that
/// position ([MS-OXCMSG] 2.2.2.16).
pub fn standing(item: &Item) -> Vec<&Attachment> {
    numbered(item).into_iter().map(|(_, a)| a).collect()
}

/// [`standing`], each with its number among the message's attachments.
fn numbered(item: &Item) -> Vec<(usize, &Attachment)> {
    let mut list: Vec<(u32, usize, &Attachment)> = item
        .attachments
        .iter()
        .enumerate()
        .filter(|(_, a)| a.props.bool(pid::ATTACHMENT_HIDDEN) != Some(true))
        .filter_map(|(i, a)| Some((u32::from_ne_bytes(a.props.i32(pid::RENDERING_POSITION)?.to_ne_bytes()), i, a)))
        .filter(|(at, ..)| *at != u32::MAX)
        .collect();
    list.sort_by_key(|(at, ..)| *at);
    list.into_iter().map(|(_, i, a)| (i, a)).collect()
}

/// What the text shows of an attachment that stands in it as a file: the name it would be
/// saved under, its size, and what a reader should know before saving it. One that is not a
/// file of its own (an object of another program that has no picture) is named and no more.
fn filed(number: usize, attachment: &Attachment, kind: Option<String>) -> Filed {
    use crate::attachments;
    if attachments::is_file(attachment) {
        Filed {
            number: Some(number),
            name: attachments::file_name(attachment),
            kind,
            bytes: attachment.size(),
            caution: attachments::caution_of(attachment).map(|caution| caution.words()),
        }
    } else {
        Filed { number: None, name: attachments::safe_name(&attachment.name()), kind, bytes: attachment.size(), caution: None }
    }
}

/// Put `item`'s attachments into the places `doc` keeps for them. An attached object that has a
/// picture of itself ([MS-OXCMSG] 2.2.2.17, a metafile) is shown as that picture when it can be
/// read; any other attachment is named. Where the places and the attachments do not agree in
/// number the places are given up, as [MS-OXRTFEX] 2.2.3.4 has it: the attachments are still
/// listed with the message.
pub fn settle(doc: &mut Doc, item: &Item) {
    if doc.objects == 0 {
        return;
    }
    let list = numbered(item);
    let list = if list.len() == doc.objects { list } else { Vec::new() };
    let Doc { blocks, written, .. } = doc;
    settle_blocks(blocks, written, &list, 0);
}

/// The fonts installed on this machine, and the shapes of text in them for pictures that are
/// kept as drawings. One for the whole program: the fonts are listed when first asked for,
/// which takes a moment, and a face that was read is remembered.
pub fn lettering() -> &'static mailcraft_fonts::Scribe {
    static SCRIBE: mailcraft_fonts::Scribe = mailcraft_fonts::Scribe::system();
    &SCRIBE
}

/// A picture file for an attached object, from the best source that can be read: the object's
/// own data when that is a picture, the pictures it keeps of itself ([MS-OLEDS] 2.3.4), and last
/// the small picture the message keeps of it ([MS-OXCMSG] 2.2.2.17).
fn object_picture(attachment: &Attachment, object: Option<&mailcraft_msg::ole::Object>) -> Option<Vec<u8>> {
    use mailcraft_formats::metafile;
    use mailcraft_msg::ole::Shape;
    let own = object.into_iter().flat_map(|o| &o.pictures).find_map(|p| match p.shape {
        Shape::File => decode(&p.data).map(|_| p.data.clone()),
        Shape::Bitmap => metafile::dib_file(&p.data),
        Shape::Metafile => metafile::wmf_drawn(&p.data, None, Some(lettering())).map(|drawn| drawn.file),
        Shape::Enhanced => metafile::emf_drawn(&p.data, None, Some(lettering())).map(|drawn| drawn.file),
    });
    own.or_else(|| {
        attachment.props.bin(pid::ATTACH_RENDERING).and_then(|kept| metafile::wmf_drawn(kept, None, Some(lettering()))).map(|drawn| drawn.file)
    })
}

fn settle_blocks(blocks: &mut [Block], written: &mut Vec<Arc<[u8]>>, list: &[(usize, &Attachment)], depth: usize) {
    for block in blocks {
        match block {
            Block::Para(para) => {
                for run in &mut para.runs {
                    let Some(Picture::Object(place)) = run.picture else { continue };
                    run.picture = None;
                    let Some(&(number, attachment)) = list.get(place) else { continue };
                    // Attach method 6: an object of another program, with its own picture.
                    let object =
                        (attachment.props.i32(pid::ATTACH_METHOD) == Some(6)).then(|| mailcraft_msg::ole::object(&attachment.data)).flatten();
                    let rendering = (attachment.props.i32(pid::ATTACH_METHOD) == Some(6) && written.len() < MAX_RENDERINGS)
                        .then(|| object_picture(attachment, object.as_ref()))
                        .flatten();
                    match rendering {
                        Some(file) => {
                            written.push(Arc::from(file));
                            run.picture = Some(Picture::Inline(Some(written.len() - 1)));
                            run.text = attachment.name().chars().filter(|c| !c.is_control()).take(120).collect();
                        }
                        None => {
                            // An object that cannot be shown is at least said to be what it is.
                            let filed = filed(number, attachment, object.and_then(|o| o.kind));
                            run.text = match &filed.kind {
                                Some(kind) => format!("[Attachment: {} ({kind})]", filed.name),
                                None => format!("[Attachment: {}]", filed.name),
                            };
                            run.picture = Some(Picture::File(filed));
                        }
                    }
                }
            }
            Block::Table(table) if depth < MAX_DEPTH => {
                for cell in table.rows.iter_mut().flatten() {
                    settle_blocks(&mut cell.blocks, written, list, depth + 1);
                }
            }
            _ => {}
        }
    }
}
