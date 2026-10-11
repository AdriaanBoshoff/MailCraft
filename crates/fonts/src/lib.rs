//! Finds the fonts a message names among the fonts installed on this machine.
//!
//! No font is bundled here and none is fetched. A message names fonts as a list, first choice
//! first (CSS Fonts Level 3, section 3.1, `font-family`); [`wanted`] turns that list into the
//! family names to look for, adding open fonts drawn to the same measures as well-known
//! proprietary ones, so that a message asking for a font that is not installed keeps its line
//! lengths. [`Library`] then finds an installed face by family name and hands back its file.
//!
//! The names come from the message and are hostile: they are only ever compared with the names
//! of installed fonts. A font file is checked to be readable before it is handed on.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::HashMap;

use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use skrifa::MetadataProvider;

/// Names read from one list, and names looked for on its behalf.
const MAX_LISTED: usize = 8;
const MAX_NAMES: usize = 32;
const MAX_NAME_LEN: usize = 64;
/// Largest font file handed on.
const MAX_FACE_BYTES: usize = 16 << 20;

/// What a message shows unknown fonts in, and what `serif` means: as Outlook Classic was seen to.
const SERIF: &[&str] = &["Times New Roman", "Liberation Serif", "Tinos", "DejaVu Serif", "Noto Serif"];
const SANS: &[&str] = &["Arial", "Liberation Sans", "Arimo", "Helvetica", "DejaVu Sans", "Noto Sans"];

/// The fonts to look for, in order, for a list of names as a message gives it. An empty answer
/// means the reader's own font. A list that names only fonts, none of them a kind of font, ends
/// with the serif fonts.
pub fn wanted(list: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let add = |names: &mut Vec<String>, name: &str| {
        if names.len() < MAX_NAMES && !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            names.push(name.to_string());
        }
    };
    for listed in list.split(',').take(MAX_LISTED) {
        let name: String =
            listed.trim().trim_matches(|c| c == '"' || c == '\'').trim().chars().filter(|c| !c.is_control()).take(MAX_NAME_LEN).collect();
        match name.to_ascii_lowercase().as_str() {
            "" | "cursive" | "fantasy" | "emoji" | "math" | "fangsong" => {}
            "serif" | "ui-serif" => {
                SERIF.iter().for_each(|n| add(&mut names, n));
                return names;
            }
            "sans-serif" | "ui-sans-serif" => {
                SANS.iter().for_each(|n| add(&mut names, n));
                return names;
            }
            // The reader has its own fixed-width font and its own interface font.
            "monospace" | "ui-monospace" | "system-ui" | "ui-rounded" | "inherit" | "initial" => return names,
            lower => {
                add(&mut names, &name);
                stand_ins(lower).iter().for_each(|n| add(&mut names, n));
            }
        }
    }
    SERIF.iter().for_each(|n| add(&mut names, n));
    names
}

/// Open fonts that keep the measures of a proprietary one, or come close to it; `name` in lower
/// case. The pairs are the ones their makers publish: Liberation and the Croscore fonts (Arimo,
/// Tinos, Cousine) for Arial, Times New Roman and Courier New; Carlito for Calibri; Caladea for
/// Cambria; Gelasio for Georgia; Selawik for Segoe UI.
fn stand_ins(name: &str) -> &'static [&'static str] {
    match name {
        "arial" => &["Liberation Sans", "Arimo", "Helvetica"],
        "helvetica" | "helvetica neue" => &["Arial", "Liberation Sans", "Arimo"],
        "times new roman" | "times" => &["Times New Roman", "Liberation Serif", "Tinos"],
        "courier new" | "courier" => &["Courier New", "Liberation Mono", "Cousine"],
        "calibri" => &["Carlito"],
        "cambria" => &["Caladea"],
        "georgia" => &["Gelasio"],
        "segoe ui" => &["Selawik", "Open Sans", "Noto Sans"],
        "aptos" => &["Carlito", "Noto Sans"],
        "verdana" | "tahoma" => &["DejaVu Sans"],
        "consolas" => &["Inconsolata", "DejaVu Sans Mono"],
        _ => &[],
    }
}

/// One installed face: its file and which face of the file it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Face {
    pub data: Vec<u8>,
    pub index: u32,
}

/// The fonts installed on this machine, by family name.
#[derive(Default)]
pub struct Library {
    fonts: Database,
    /// Family names as the fonts spell them, by their lower-case form: a message may write a
    /// name in any case.
    spelled: HashMap<String, String>,
}

impl Library {
    /// No fonts at all.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The fonts the system has installed. Reads the name of every font, which takes a moment.
    pub fn system() -> Self {
        let mut fonts = Database::new();
        fonts.load_system_fonts();
        let mut spelled = HashMap::new();
        for (name, _) in fonts.faces().flat_map(|face| face.families.iter()) {
            spelled.entry(name.to_lowercase()).or_insert_with(|| name.clone());
        }
        Self { fonts, spelled }
    }

    /// How many faces are known.
    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }

    /// The upright face of `family`, or its bold one. `None` when the family is not installed,
    /// when bold is asked for and the family has none, or when the file is too large or cannot
    /// be read as a font.
    pub fn face(&self, family: &str, bold: bool) -> Option<Face> {
        if family.is_empty() || family.len() > MAX_NAME_LEN * 4 {
            return None;
        }
        let family = self.spelled.get(&family.to_lowercase())?;
        let query = Query {
            families: &[Family::Name(family)],
            weight: if bold { Weight::BOLD } else { Weight::NORMAL },
            stretch: Stretch::Normal,
            style: Style::Normal,
        };
        let id = self.fonts.query(&query)?;
        // The nearest face is found whatever its weight; a light one is not a bold one.
        if bold && self.fonts.face(id)?.weight.0 < 600 {
            return None;
        }
        self.fonts
            .with_face_data(id, |data, index| (data.len() <= MAX_FACE_BYTES && readable(data, index)).then(|| Face { data: data.to_vec(), index }))
            .flatten()
    }
}

/// The file holds a font at `index` that can be opened and that maps characters to glyphs.
pub fn readable(data: &[u8], index: u32) -> bool {
    skrifa::FontRef::from_index(data, index).is_ok_and(|font| font.charmap().has_map())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_become_names_to_look_for() {
        assert_eq!(wanted("Georgia"), ["Georgia", "Gelasio", "Times New Roman", "Liberation Serif", "Tinos", "DejaVu Serif", "Noto Serif"]);
        assert_eq!(wanted(" 'Nosuch Font' , \"Calibri\",sans-serif, Georgia")[..3], ["Nosuch Font", "Calibri", "Carlito"]);
        assert!(wanted("Calibri, sans-serif").iter().any(|n| n == "Arial"));
        assert!(!wanted("Calibri, sans-serif, Georgia").iter().any(|n| n == "Georgia"), "the list ends at a kind of font");
        assert_eq!(
            wanted("ARIAL, arial, Arial"),
            ["ARIAL", "Liberation Sans", "Arimo", "Helvetica", "Times New Roman", "Liberation Serif", "Tinos", "DejaVu Serif", "Noto Serif"]
        );
        assert_eq!(wanted("monospace"), [] as [&str; 0]);
        assert_eq!(wanted("system-ui, Arial"), [] as [&str; 0]);
        assert_eq!(wanted("cursive, , fantasy"), SERIF);
        assert_eq!(wanted(""), SERIF);
    }

    #[test]
    fn hostile_lists_are_bounded() {
        let long = "x".repeat(100_000);
        assert!(wanted(&long).iter().all(|n| n.chars().count() <= MAX_NAME_LEN));
        let many = (0..10_000).map(|i| format!("font{i}")).collect::<Vec<_>>().join(",");
        assert!(wanted(&many).len() <= MAX_NAMES);
        for list in [
            "\u{0}\u{1}\u{202e},\"\"\",'''",
            ",,,,,,,,,,,,,,,,",
            "\"unclosed, 'mixed\", é\u{301}\u{fe0f}",
            "arial\narial\r\n",
            "..\\..\\Windows\\Fonts\\arial.ttf, C:/x.ttf",
        ] {
            let names = wanted(list);
            assert!(names.len() <= MAX_NAMES && names.iter().all(|n| !n.chars().any(char::is_control)), "{names:?}");
        }
    }

    #[test]
    fn faces_come_only_from_the_library() {
        let none = Library::empty();
        assert!(none.is_empty());
        assert_eq!(none.face("Arial", false), None);

        // Whatever this machine has installed: a name is only ever looked up, never opened as a path.
        let system = Library::system();
        for name in ["", "No Such Font At All 7f3a", "..\\..\\Windows\\win.ini", "C:\\Windows\\Fonts\\arial.ttf", &"y".repeat(10_000)] {
            assert_eq!(system.face(name, false), None, "{name:?}");
            assert_eq!(system.face(name, true), None);
        }
        for name in SERIF.iter().chain(SANS) {
            for bold in [false, true] {
                if let Some(face) = system.face(name, bold) {
                    assert!(face.data.len() <= MAX_FACE_BYTES && readable(&face.data, face.index));
                }
            }
        }
    }

    #[test]
    fn damaged_font_files_are_not_readable() {
        assert!(!readable(&[], 0));
        assert!(!readable(b"not a font at all", 0));
        assert!(!readable(&[0, 1, 0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff], 0));
        assert!(!readable(b"ttcf\0\x01\0\0\xff\xff\xff\xff", 7));
        assert!(!readable(&vec![0xff; 4096], u32::MAX));
    }
}
