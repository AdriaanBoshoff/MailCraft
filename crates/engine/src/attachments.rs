//! Attachments on their way to a disk: a file name that is safe to write whatever the message
//! called the attachment, a place for it that takes no other file's, and what a reader should
//! know about it before saving. Nothing here opens, runs or looks inside an attachment beyond
//! its first bytes.

use openmailclient_model::{Attachment, Item, pid};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Characters of a name that are looked at.
const MAX_LOOKED_AT: usize = 1024;
/// Characters of a file name that is written, its ending included.
const MAX_NAME: usize = 120;
/// Characters of an ending that are kept when a name is cut short.
const MAX_ENDING: usize = 16;
/// Numbered names tried before a save gives up on a folder.
const MAX_NUMBERED: usize = 10_000;

/// Endings Windows starts as a program, a script, a shortcut or a settings change.
const RUNS: &[&str] = &[
    "exe",
    "com",
    "scr",
    "pif",
    "bat",
    "cmd",
    "msi",
    "msp",
    "msix",
    "appx",
    "ps1",
    "psm1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "ws",
    "sct",
    "hta",
    "lnk",
    "url",
    "scf",
    "jar",
    "cpl",
    "reg",
    "dll",
    "sys",
    "inf",
    "chm",
    "msc",
    "application",
    "gadget",
];
/// Endings of documents that carry macros.
const MACROS: &[&str] = &["docm", "dotm", "xlsm", "xltm", "xlam", "xlsb", "pptm", "potm", "ppam", "ppsm", "sldm"];
/// Endings a browser opens as a page, with whatever scripts and forms it holds.
const PAGES: &[&str] = &["htm", "html", "xhtml", "xht", "mht", "mhtml", "shtml", "svg"];
/// Endings a name is given to pass for a harmless document.
const DOCUMENTS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "csv", "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "mp3", "mp4", "wav",
    "zip", "odt", "ods",
];
/// Names Windows keeps for devices, with or without an ending.
const DEVICES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5",
    "lpt6", "lpt7", "lpt8", "lpt9", "conin$", "conout$",
];

/// Characters that change the direction text is shown in, or take no room at all: a name with
/// one can read as another name.
fn unseen(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{061c}' | '\u{feff}' | '\u{00ad}')
}

/// The ending of a name in lower case, without its dot: letters and digits after the last dot.
/// Dots and spaces at the end are not part of it, as Windows drops them.
pub fn ending(name: &str) -> Option<String> {
    let name = name.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    let (stem, end) = name.rsplit_once('.')?;
    let fits = !stem.is_empty() && (1..=MAX_ENDING).contains(&end.chars().count()) && end.chars().all(|c| c.is_alphanumeric() || c == '$');
    fits.then(|| end.to_lowercase())
}

/// A file name for untrusted text: one name and not a path, nothing Windows refuses or reads as
/// a device, no characters that hide or reverse it, runs of spaces closed up, of a length every
/// file system takes, its ending kept.
pub fn safe_name(name: &str) -> String {
    let mut cleaned = String::new();
    let mut spaced = false;
    for c in name.chars().take(MAX_LOOKED_AT) {
        if c.is_control() || unseen(c) {
            continue;
        }
        if c.is_whitespace() {
            spaced = !cleaned.is_empty();
            continue;
        }
        if std::mem::take(&mut spaced) {
            cleaned.push(' ');
        }
        cleaned.push(if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c });
    }
    // A space before the ending is how a long name pushes it out of sight.
    while let Some(at) = cleaned.find(" .") {
        cleaned.remove(at);
    }
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c == ' ');
    if trimmed.is_empty() {
        return "attachment".to_string();
    }
    // The ending keeps the case it was written in.
    let (stem, end) = match trimmed.rsplit_once('.') {
        Some(parts) if ending(trimmed).is_some() => parts,
        _ => (trimmed, ""),
    };
    let room = MAX_NAME.saturating_sub(end.chars().count().saturating_add(1));
    let mut stem: String = stem.chars().take(room).collect();
    stem = stem.trim_end_matches(['.', ' ']).to_string();
    let first = stem.split('.').next().unwrap_or("").trim().to_lowercase();
    if stem.is_empty() || DEVICES.contains(&first.as_str()) {
        stem.insert(0, '_');
    }
    if end.is_empty() { stem } else { format!("{stem}.{end}") }
}

/// Something about an attachment its reader should know before saving it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caution {
    /// The name reads as one kind of file and is another: `invoice.pdf.exe`.
    TwoEndings { reads: String, is: String },
    /// The name has characters that reverse or hide part of it.
    Reversed { is: Option<String> },
    /// The content is a Windows program under a name that does not say so.
    Program { named: Option<String> },
    /// Opening it runs it.
    Runs { ending: String },
    /// A document of a kind that carries macros.
    Macros { ending: String },
    /// A page a browser opens.
    Page { ending: String },
}

impl Caution {
    /// The caution in a sentence.
    pub fn words(&self) -> String {
        match self {
            Self::TwoEndings { reads, is } => format!("The name reads as a .{reads} file, but it is a .{is} file."),
            Self::Reversed { is: Some(is) } => format!("The name has hidden characters that change how it reads; it is a .{is} file."),
            Self::Reversed { is: None } => "The name has hidden characters that change how it reads.".to_string(),
            Self::Program { named: Some(named) } => format!("The content is a program, though the name ends in .{named}."),
            Self::Program { named: None } => "The content is a program, though the name does not say so.".to_string(),
            Self::Runs { ending } => format!("A .{ending} file is a program or a script: opening it runs it."),
            Self::Macros { ending } => format!("A .{ending} document can carry macros that run when it is opened."),
            Self::Page { ending } => format!("A .{ending} file opens in a browser, where it can show a sign-in form of its own."),
        }
    }
}

/// What a reader should know about an attachment with this name and content, or `None`.
pub fn caution(name: &str, data: &[u8]) -> Option<Caution> {
    let looked: String = name.chars().take(MAX_LOOKED_AT).collect();
    let safe = safe_name(&looked);
    let end = ending(&safe);
    let risky = |end: &str| RUNS.contains(&end) || MACROS.contains(&end) || PAGES.contains(&end);
    if looked.chars().any(|c| matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{061c}')) {
        return Some(Caution::Reversed { is: end });
    }
    // [MS-PE] the first two bytes of a Windows program are "MZ".
    if data.starts_with(b"MZ") && data.len() >= 64 && !end.as_deref().is_some_and(|end| RUNS.contains(&end)) {
        return Some(Caution::Program { named: end });
    }
    let end = end?;
    if risky(&end)
        && let Some((stem, _)) = safe.rsplit_once('.')
        && let Some(reads) = ending(stem)
        && DOCUMENTS.contains(&reads.as_str())
    {
        return Some(Caution::TwoEndings { reads, is: end });
    }
    if RUNS.contains(&end.as_str()) {
        return Some(Caution::Runs { ending: end });
    }
    if MACROS.contains(&end.as_str()) {
        return Some(Caution::Macros { ending: end });
    }
    PAGES.contains(&end.as_str()).then_some(Caution::Page { ending: end })
}

/// A size in words: bytes, then thousands and millions of them by 1024.
pub fn size_words(bytes: usize) -> String {
    match bytes {
        0..=1023 => format!("{bytes} bytes"),
        1024..=1_048_575 => format!("{} KB", bytes / 1024),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

/// What a reader should know about `attachment`. An attached message is a message, whatever it
/// is called.
pub fn caution_of(attachment: &Attachment) -> Option<Caution> {
    if attachment.embedded.is_some() {
        return None;
    }
    caution(&attachment.name(), &attachment.data)
}

/// An attachment that is a file of its own: not a picture placed in the body, and not an object
/// of another program, which is shown where it stands.
pub fn is_file(attachment: &Attachment) -> bool {
    attachment.embedded.is_some() || (attachment.content_id().is_none() && attachment.props.i32(pid::ATTACH_METHOD) != Some(6))
}

/// The attachments of `item` that are files of their own, with their numbers.
pub fn files(item: &Item) -> impl Iterator<Item = (usize, &Attachment)> {
    item.attachments.iter().enumerate().filter(|(_, a)| is_file(a))
}

/// The name `attachment` is saved under: an attached message as a `.msg` file.
pub fn file_name(attachment: &Attachment) -> String {
    let name = safe_name(&attachment.name());
    if attachment.embedded.is_some() && ending(&name).as_deref() != Some("msg") {
        // Room for the ending is made by cutting the name, not by going over the length.
        let stem: String = name.chars().take(MAX_NAME.saturating_sub(4)).collect();
        return format!("{}.msg", stem.trim_end_matches(['.', ' ']));
    }
    name
}

/// A path in `folder` for a file called `name` that no file has and none of `taken` is: the name
/// itself, or the name with a number, `report (2).pdf`. File names are compared without case, as
/// Windows and macOS do. `None` when the folder is full of them.
pub fn free_path(folder: &Path, name: &str, taken: &mut HashSet<String>) -> Option<PathBuf> {
    let (stem, end) = match ending(name) {
        Some(_) => name.rsplit_once('.').map_or((name, String::new()), |(stem, end)| (stem, format!(".{end}"))),
        None => (name, String::new()),
    };
    for number in 1..=MAX_NUMBERED {
        let candidate = if number == 1 { name.to_string() } else { format!("{stem} ({number}){end}") };
        let path = folder.join(&candidate);
        // `symlink_metadata` also sees a link that points nowhere, which writing would follow.
        if !taken.contains(&candidate.to_lowercase()) && path.symlink_metadata().is_err() {
            taken.insert(candidate.to_lowercase());
            return Some(path);
        }
    }
    None
}
