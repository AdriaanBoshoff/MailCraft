//! SVG pictures, drawn to pixels.
//!
//! A drawing in a message is hostile input like the rest of it. Nothing in one is run: there
//! is no script engine here. Nothing is read from disk or fetched for one: a picture a drawing
//! refers to, by address or written into it, is left out. Its text is set in the fonts
//! installed on this machine. The reader and the painter are not ours (`resvg`), so a drawing
//! is drawn on a thread of its own, with room for the deepest drawing the reader takes, and is
//! given up when it takes too long; the picture then stays a placeholder.

use crate::pictures::Pixels;

/// Largest drawing that is read.
const MAX_FILE: usize = 4 << 20;
/// How far into a file the opening `<svg` is looked for.
const MAX_PROLOGUE: usize = 4096;

/// Whether `data` begins as an SVG file does: XML in UTF-8 with an `svg` element near its top.
pub(crate) fn looks_like(data: &[u8]) -> bool {
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    let from = data.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(data.len());
    let top = data.get(from..data.len().min(from.saturating_add(MAX_PROLOGUE))).unwrap_or(&[]);
    top.first() == Some(&b'<') && top.windows(4).any(|w| w == b"<svg")
}

/// An SVG file → pixels. `None` for one that is over the limits, cannot be read, draws nothing,
/// or takes too long.
pub(crate) fn decode(data: &[u8]) -> Option<Pixels> {
    if data.len() > MAX_FILE || !looks_like(data) {
        return None;
    }
    drawn(data)
}

/// The web build has one thread and a small stack: a drawing is not read there yet.
#[cfg(target_arch = "wasm32")]
fn drawn(_data: &[u8]) -> Option<Pixels> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn drawn(data: &[u8]) -> Option<Pixels> {
    native::drawn(data)
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    use resvg::{tiny_skia, usvg};

    use crate::pictures::Pixels;

    /// How long one drawing may take.
    const MAX_WAIT: Duration = Duration::from_secs(3);
    /// Room for the reader's deepest nesting (it stops at 1024 elements inside one another).
    const STACK: usize = 64 << 20;
    /// Drawings given up that may still be running. With this many, no other is begun.
    const MAX_LEFT: usize = 2;
    /// Largest width or height a drawing is said to have, and the longest side kept.
    const MAX_SIDE: f32 = 8192.0;
    const MAX_KEPT_SIDE: f32 = 2048.0;
    /// A drawing is kept this many times as large as it is, where that fits: it has no pixels
    /// of its own, and is then sharp on a dense screen and on paper.
    const SHARP: f32 = 2.0;

    static LEFT: AtomicUsize = AtomicUsize::new(0);

    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Running,
        Done,
        GivenUp,
    }

    pub(super) fn drawn(data: &[u8]) -> Option<Pixels> {
        if LEFT.load(Ordering::SeqCst) >= MAX_LEFT {
            return None;
        }
        let data = data.to_vec();
        let fonts = crate::pictures::lettering().library().shared();
        let state = Arc::new(Mutex::new(State::Running));
        let theirs = Arc::clone(&state);
        let (send, receive) = mpsc::channel();
        std::thread::Builder::new()
            .name("svg".into())
            .stack_size(STACK)
            .spawn(move || {
                let pixels = catch_unwind(AssertUnwindSafe(|| draw(&data, fonts))).ok().flatten();
                let _ = send.send(pixels);
                if let Ok(mut state) = theirs.lock() {
                    if *state == State::GivenUp {
                        LEFT.fetch_sub(1, Ordering::SeqCst);
                    }
                    *state = State::Done;
                }
            })
            .ok()?;
        match receive.recv_timeout(MAX_WAIT) {
            Ok(pixels) => pixels,
            Err(_) => {
                let mut state = state.lock().ok()?;
                if *state == State::Running {
                    *state = State::GivenUp;
                    LEFT.fetch_add(1, Ordering::SeqCst);
                }
                None
            }
        }
    }

    fn draw(data: &[u8], fonts: Arc<usvg::fontdb::Database>) -> Option<Pixels> {
        let options = usvg::Options {
            resources_dir: None,
            fontdb: fonts,
            // No picture is read for a drawing, from a file, an address or its own text.
            image_href_resolver: usvg::ImageHrefResolver { resolve_data: Box::new(|_, _, _| None), resolve_string: Box::new(|_, _| None) },
            ..usvg::Options::default()
        };
        let tree = usvg::Tree::from_data(data, &options).ok()?;
        if !tree.root().has_children() {
            return None;
        }
        let (w, h) = (tree.size().width(), tree.size().height());
        if !(w.is_finite() && h.is_finite()) || w < 0.5 || h < 0.5 {
            return None;
        }
        let longer = w.max(h);
        let said = (MAX_SIDE / longer).min(1.0);
        let natural = (((w * said).ceil() as u16).max(1), ((h * said).ceil() as u16).max(1));
        let by = (longer * SHARP).clamp(1.0, MAX_KEPT_SIDE) / longer;
        let side = |n: f32| ((n * by).ceil() as u32).clamp(1, MAX_KEPT_SIDE as u32);
        let mut page = tiny_skia::Pixmap::new(side(w), side(h))?;
        resvg::render(&tree, tiny_skia::Transform::from_scale(by, by), &mut page.as_mut());
        // The painter keeps colour already multiplied by cover; pictures here are kept plain.
        let mut rgba = Vec::with_capacity(page.pixels().len().saturating_mul(4));
        for pixel in page.pixels() {
            let plain = pixel.demultiply();
            rgba.extend_from_slice(&[plain.red(), plain.green(), plain.blue(), plain.alpha()]);
        }
        Some(Pixels { width: page.width(), height: page.height(), rgba, natural })
    }
}
