//! Fonts a message asks for by name, drawn with the fonts installed on this machine.
//!
//! Nothing is bundled and nothing is fetched: a name is looked up among the installed fonts
//! (`mailcraft-fonts`), with open fonts of the same measures standing in for proprietary ones
//! that are not installed. A name the message gives is hostile input and is only ever compared
//! with installed names. How many fonts and how many bytes of them are loaded is capped; past
//! the caps, and for a name nothing answers to, text is drawn in the reader's own font.

use std::collections::HashMap;
use std::sync::Arc;
use std::thread::JoinHandle;

use egui::{Context, FontData, FontDefinitions, FontFamily};
use mailcraft_fonts::{Face, Library};
use mailcraft_formats::rich::{Block, Doc};

/// Font files and bytes of them loaded while the window is open.
const MAX_FACES: usize = 24;
const MAX_BYTES: usize = 64 << 20;
/// Lists of names, and family names, that are remembered.
const MAX_LISTS: usize = 512;
const MAX_FAMILIES: usize = 512;
/// Lists looked up for one message.
const MAX_LISTS_PER_MESSAGE: usize = 64;
const MAX_DEPTH: usize = 64;

/// The faces one installed family is drawn with.
#[derive(Clone, Default)]
struct Family {
    regular: Option<FontFamily>,
    bold: Option<FontFamily>,
}

#[derive(Default)]
pub(crate) struct Faces {
    /// The installed fonts are listed off the interface thread, once.
    listing: Option<JoinHandle<Library>>,
    library: Option<Library>,
    /// What egui has been given: the interface fonts and every face loaded since.
    defs: FontDefinitions,
    /// The message whose lists were last looked up.
    key: Option<u64>,
    by_list: HashMap<Arc<str>, Family>,
    by_family: HashMap<String, Family>,
    /// The families egui can draw with in this frame. One given to it is ready a frame later.
    ready: Vec<FontFamily>,
    faces: usize,
    bytes: usize,
}

impl Faces {
    /// `defs` is what egui was given for the interface. The installed fonts start being listed.
    pub(crate) fn new(defs: FontDefinitions) -> Self {
        let listing = std::thread::Builder::new().name("fonts".to_string()).spawn(Library::system).ok();
        Self { listing, defs, ..Self::default() }
    }

    /// The family to draw text with that asks for the fonts in `list`. `None` is the reader's own.
    pub(crate) fn pick(&self, list: &str, bold: bool) -> Option<FontFamily> {
        let family = self.by_list.get(list)?;
        let face = if bold { family.bold.as_ref() } else { family.regular.as_ref() }?;
        self.ready.contains(face).then(|| face.clone())
    }

    /// Look up the fonts `doc` names, once per message. `key` names the message's body. Call
    /// every frame the document is drawn in.
    pub(crate) fn load(&mut self, ctx: &Context, key: u64, doc: &Doc) {
        self.ready = ctx.fonts(|fonts| fonts.families());
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        let mut lists: Vec<&Arc<str>> = Vec::new();
        named(&doc.blocks, &mut lists, 0);
        let mut added = false;
        for list in lists {
            if self.by_list.len() >= MAX_LISTS || self.by_list.contains_key(list) {
                continue;
            }
            let mut found = Family::default();
            for name in mailcraft_fonts::wanted(list) {
                found = self.family(&name, &mut added);
                if found.regular.is_some() {
                    break;
                }
            }
            self.by_list.insert(list.clone(), found);
        }
        if added {
            ctx.set_fonts(self.defs.clone());
            ctx.request_repaint();
        }
    }

    /// The faces of one installed family, loaded the first time it is asked for.
    fn family(&mut self, name: &str, added: &mut bool) -> Family {
        let key = name.to_lowercase();
        if let Some(known) = self.by_family.get(&key) {
            return known.clone();
        }
        if self.by_family.len() >= MAX_FAMILIES {
            return Family::default();
        }
        let library = self.library.get_or_insert_with(|| match self.listing.take() {
            // A listing that failed leaves no fonts to find; the reader's own are used.
            Some(listing) => listing.join().unwrap_or_default(),
            None => Library::system(),
        });
        let (regular, bold) = (library.face(name, false), library.face(name, true));
        let family = Family { regular: regular.and_then(|face| self.give(face, added)), bold: bold.and_then(|face| self.give(face, added)) };
        self.by_family.insert(key, family.clone());
        family
    }

    /// Hand one face to egui, under a name of our own, with the interface fonts behind it for
    /// the characters it lacks.
    fn give(&mut self, face: Face, added: &mut bool) -> Option<FontFamily> {
        if self.faces >= MAX_FACES || self.bytes.saturating_add(face.data.len()) > MAX_BYTES {
            return None;
        }
        self.faces += 1;
        self.bytes += face.data.len();
        let name = format!("message-{}", self.faces);
        let mut data = FontData::from_owned(face.data);
        data.index = face.index;
        self.defs.font_data.insert(name.clone(), Arc::new(data));
        let mut fonts = vec![name.clone()];
        fonts.extend(self.defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
        let family = FontFamily::Name(name.into());
        self.defs.families.insert(family.clone(), fonts);
        *added = true;
        Some(family)
    }
}

/// The lists of font names in `blocks`, each once.
fn named<'a>(blocks: &'a [Block], out: &mut Vec<&'a Arc<str>>, depth: usize) {
    for block in blocks {
        match block {
            Block::Para(p) => {
                for list in p.runs.iter().filter_map(|r| r.style.family.as_ref()) {
                    if out.len() < MAX_LISTS_PER_MESSAGE && !out.iter().any(|known| Arc::ptr_eq(known, list) || known == &list) {
                        out.push(list);
                    }
                }
            }
            Block::Table(t) if depth < MAX_DEPTH => {
                for cell in t.rows.iter().flatten() {
                    named(&cell.blocks, out, depth + 1);
                }
            }
            _ => {}
        }
    }
}
