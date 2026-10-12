//! Fonts a message asks for by name, drawn with the fonts installed on this machine.
//!
//! Nothing is bundled and nothing is fetched: a name is looked up among the installed fonts
//! (`openmailclient-fonts`), with open fonts of the same measures standing in for proprietary ones
//! that are not installed. A name the message gives is hostile input and is only ever compared
//! with installed names. Only the faces a message uses are loaded (bold and italic text have
//! faces of their own), and how many fonts and how many bytes of them are loaded is capped; past
//! the caps, and for a name nothing answers to, text is drawn in the reader's own font.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Context, FontData, FontDefinitions, FontFamily};
use openmailclient_fonts::Library;
use openmailclient_formats::rich::{Block, Doc};

/// Font files and bytes of them loaded while the window is open.
const MAX_FACES: usize = 32;
const MAX_BYTES: usize = 64 << 20;
/// Lists of names, and faces, that are remembered.
const MAX_LISTS: usize = 512;
const MAX_KNOWN: usize = 1024;
/// Lists looked up for one message, each as regular, bold, italic or both.
const MAX_ASKED_PER_MESSAGE: usize = 128;
const MAX_DEPTH: usize = 64;

/// One face of a family: its name in lower case, bold, italic.
type Which = (String, bool, bool);

/// What a run is drawn with.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Picked {
    pub(crate) family: FontFamily,
    /// The family has no italic face of this weight: the upright one is drawn slanted.
    pub(crate) slant: bool,
}

#[derive(Default)]
pub(crate) struct Faces {
    /// What egui has been given: the interface fonts and every face loaded since.
    defs: FontDefinitions,
    /// The message whose lists were last looked up.
    key: Option<u64>,
    /// The installed family a list of names comes to, in lower case. `None` is the reader's own.
    by_list: HashMap<Arc<str>, Option<String>>,
    /// Faces asked for so far, and what egui knows each as. `None` is a face the family lacks.
    known: HashMap<Which, Option<FontFamily>>,
    /// The families egui can draw with in this frame. One given to it is ready a frame later.
    ready: Vec<FontFamily>,
    faces: usize,
    bytes: usize,
}

impl Faces {
    /// `defs` is what egui was given for the interface. The installed fonts start being listed.
    pub(crate) fn new(defs: FontDefinitions) -> Self {
        // The installed fonts are listed off the interface thread, once for the whole program;
        // where no thread can be had they are listed when first asked for.
        let _ = std::thread::Builder::new().name("fonts".to_string()).spawn(|| {
            openmailclient_engine::pictures::lettering().library();
        });
        Self { defs, ..Self::default() }
    }

    /// What to draw text with that asks for the fonts in `list`. `None` is the reader's own.
    pub(crate) fn pick(&self, list: &str, bold: bool, italic: bool) -> Option<Picked> {
        let family = self.by_list.get(list)?.as_ref()?;
        let ready = |italic: bool| {
            let face = self.known.get(&(family.clone(), bold, italic))?.as_ref()?;
            self.ready.contains(face).then(|| face.clone())
        };
        if italic && let Some(family) = ready(true) {
            return Some(Picked { family, slant: false });
        }
        // Bold text is never drawn in a face that is not bold: the reader's own bold is used.
        ready(false).map(|family| Picked { family, slant: italic })
    }

    /// Look up the fonts `doc` names, once per message. `key` names the message's body. Call
    /// every frame the document is drawn in.
    pub(crate) fn load(&mut self, ctx: &Context, key: u64, doc: &Doc) {
        self.ready = ctx.fonts(|fonts| fonts.families());
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        let mut asked: Vec<(&Arc<str>, bool, bool)> = Vec::new();
        named(&doc.blocks, &mut asked, 0);
        let mut added = false;
        for (list, bold, italic) in asked {
            let Some(family) = self.family(list) else { continue };
            // The upright face too: it is what italic text is slanted from where there is no
            // italic one.
            if italic && self.face(&family, bold, true, &mut added).is_some() {
                continue;
            }
            self.face(&family, bold, false, &mut added);
        }
        if added {
            ctx.set_fonts(self.defs.clone());
            ctx.request_repaint();
        }
    }

    /// The installed fonts, listed by now or listed here.
    fn library(&self) -> &'static Library {
        openmailclient_engine::pictures::lettering().library()
    }

    /// The installed family a list of names comes to.
    fn family(&mut self, list: &Arc<str>) -> Option<String> {
        if let Some(known) = self.by_list.get(list) {
            return known.clone();
        }
        if self.by_list.len() >= MAX_LISTS {
            return None;
        }
        let library = self.library();
        let found = openmailclient_fonts::wanted(list).into_iter().find(|name| library.has(name)).map(|name| name.to_lowercase());
        self.by_list.insert(list.clone(), found.clone());
        found
    }

    /// One face of an installed family, handed to egui the first time it is asked for under a
    /// name of our own, with the interface fonts behind it for the characters it lacks.
    fn face(&mut self, family: &str, bold: bool, italic: bool, added: &mut bool) -> Option<FontFamily> {
        let which = (family.to_string(), bold, italic);
        if let Some(known) = self.known.get(&which) {
            return known.clone();
        }
        if self.known.len() >= MAX_KNOWN || self.faces >= MAX_FACES {
            return None;
        }
        let face = self.library().face(family, bold, italic).filter(|face| self.bytes.saturating_add(face.data.len()) <= MAX_BYTES);
        let given = face.map(|face| {
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
            family
        });
        self.known.insert(which, given.clone());
        given
    }
}

/// The lists of font names in `blocks` with the faces asked of each, each once.
fn named<'a>(blocks: &'a [Block], out: &mut Vec<(&'a Arc<str>, bool, bool)>, depth: usize) {
    for block in blocks {
        match block {
            Block::Para(p) => {
                for run in &p.runs {
                    let Some(list) = run.style.family.as_ref() else { continue };
                    let asked = (list, run.style.bold, run.style.italic);
                    if out.len() < MAX_ASKED_PER_MESSAGE
                        && !out.iter().any(|known| (Arc::ptr_eq(known.0, list) || known.0 == list) && (known.1, known.2) == (asked.1, asked.2))
                    {
                        out.push(asked);
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
