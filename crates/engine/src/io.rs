//! File I/O dispatch: which reader or writer a path goes to.

use crate::EngineError;
use mailcraft_model::{Item, Store};
use std::io::Read;
use std::path::Path;

/// Loose message files larger than this are refused rather than loaded into memory.
const MAX_ITEM_FILE: u64 = 512 * 1024 * 1024;

pub enum Opened {
    /// A data file with folders (`.pst`, `.ost`).
    Store(Box<dyn Store>),
    /// One item (`.msg`, `.oft`, `.eml`).
    Item(Box<Item>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pst,
    Msg,
    Template,
    Eml,
}

fn kind_by_extension(path: &Path) -> Option<Kind> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "pst" | "ost" => Some(Kind::Pst),
        "msg" => Some(Kind::Msg),
        "oft" => Some(Kind::Template),
        "eml" | "mht" | "mhtml" | "nws" => Some(Kind::Eml),
        _ => None,
    }
}

/// Open a file read-only. The extension decides; without a known one, the first bytes do.
pub fn open_path(path: &Path) -> Result<Opened, EngineError> {
    let mut file = std::fs::File::open(path)?;
    let mut magic = [0u8; 8];
    let got = file.read(&mut magic)?;
    let magic = magic.get(..got).unwrap_or(&[]);
    let kind = kind_by_extension(path).unwrap_or(if magic.starts_with(b"!BDN") {
        Kind::Pst
    } else if magic.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        Kind::Msg
    } else {
        Kind::Eml
    });
    drop(file);
    if kind == Kind::Pst {
        let store = mailcraft_pst::PstStore::open(path).map_err(|e| EngineError::Format(e.to_string()))?;
        return Ok(Opened::Store(Box::new(store)));
    }
    if std::fs::metadata(path)?.len() > MAX_ITEM_FILE {
        return Err(EngineError::Format("this file is too large to open as one message".into()));
    }
    let data = std::fs::read(path)?;
    let item = match kind {
        Kind::Eml => mailcraft_mime::parse(&data).map_err(|e| EngineError::Format(e.to_string()))?,
        _ => mailcraft_msg::read(&data).map_err(|e| EngineError::Format(e.to_string()))?,
    };
    Ok(Opened::Item(Box::new(item)))
}

/// The bytes of an item in the format the path's extension names.
pub fn encode_item(item: &Item, path: &Path) -> Result<Vec<u8>, EngineError> {
    match kind_by_extension(path) {
        Some(Kind::Msg) => mailcraft_msg::write(item).map_err(|e| EngineError::Format(e.to_string())),
        Some(Kind::Template) => mailcraft_msg::write_as(item, mailcraft_msg::Kind::Template).map_err(|e| EngineError::Format(e.to_string())),
        Some(Kind::Eml) => Ok(mailcraft_mime::write(item)),
        Some(Kind::Pst) => Err(EngineError::Format("writing .pst data files is not supported yet; save as .msg or .eml".into())),
        None => Err(EngineError::BadParams("the file name must end in .msg, .oft or .eml".into())),
    }
}

/// Write `data` to `path` so that a failure leaves whatever was there before intact: the bytes go
/// to a temporary file beside the target, which then replaces it in one step.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), EngineError> {
    let name = path.file_name().ok_or_else(|| EngineError::BadParams("the path has no file name".into()))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = path.with_file_name(tmp_name);
    let written = std::fs::write(&tmp, data).and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(written?)
}

pub fn save_item(item: &Item, path: &Path) -> Result<(), EngineError> {
    let data = encode_item(item, path)?;
    write_atomic(path, &data)
}
