//! Windows code page numbers → text. Outlook stores non-Unicode strings in the code page named by
//! PidTagMessageCodepage / PidTagInternetCodepage.

/// The encoding label for a Windows code page identifier, when there is one.
fn label(cp: u32) -> Option<&'static str> {
    Some(match cp {
        437 | 850 | 20127 | 1252 | 28591 => "windows-1252",
        866 => "ibm866",
        874 => "windows-874",
        932 => "shift_jis",
        936 | 54936 => "gb18030",
        949 | 51949 => "euc-kr",
        950 => "big5",
        1200 => "utf-16le",
        1201 => "utf-16be",
        1250 => "windows-1250",
        1251 => "windows-1251",
        1253 => "windows-1253",
        1254 => "windows-1254",
        1255 => "windows-1255",
        1256 => "windows-1256",
        1257 => "windows-1257",
        1258 => "windows-1258",
        20866 => "koi8-r",
        21866 => "koi8-u",
        28592 => "iso-8859-2",
        28593 => "iso-8859-3",
        28594 => "iso-8859-4",
        28595 => "iso-8859-5",
        28596 => "iso-8859-6",
        28597 => "iso-8859-7",
        28598 | 38598 => "iso-8859-8",
        28603 => "iso-8859-13",
        28605 => "iso-8859-15",
        50220..=50222 => "iso-2022-jp",
        51932 => "euc-jp",
        65001 => "utf-8",
        _ => return None,
    })
}

/// Decode bytes in a code page. An unknown code page falls back to UTF-8 when the bytes are
/// valid UTF-8 and to Windows-1252 otherwise; undecodable bytes become U+FFFD, never an error.
pub fn decode(cp: u32, bytes: &[u8]) -> String {
    let enc = label(cp).and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes()));
    match enc {
        Some(e) => e.decode_without_bom_handling(bytes).0.into_owned(),
        None => match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes).0.into_owned(),
        },
    }
}

/// UTF-16LE bytes → text; an odd trailing byte is ignored and lone surrogates become U+FFFD.
pub fn utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
}

pub fn to_utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Text as bytes in a code page, when `decode` gives the same text back from them: `None` for
/// text the code page does not have, and for a code page that is not one of bytes (UTF-16).
/// Nothing is ever replaced by a look-alike or a question mark.
pub fn encode(cp: u32, s: &str) -> Option<Vec<u8>> {
    if matches!(cp, 1200 | 1201) {
        return None;
    }
    let bytes = match label(cp).and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes())) {
        Some(e) => {
            let (bytes, _, lost) = e.encode(s);
            if lost {
                return None;
            }
            bytes.into_owned()
        }
        // What `decode` takes bytes of an unknown code page for.
        None => s.as_bytes().to_vec(),
    };
    (decode(cp, &bytes) == s).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_by_code_page() {
        assert_eq!(decode(1252, &[0x93, 0xE9, 0x94]), "\u{201c}é\u{201d}");
        assert_eq!(decode(1251, &[0xCF, 0xF0, 0xE8]), "При");
        assert_eq!(decode(0, "ünï".as_bytes()), "ünï");
        assert_eq!(decode(424_242, &[0xE9]), "é");
        assert_eq!(utf16le(&[0x41, 0, 0x42]), "A");
        assert_eq!(utf16le(&to_utf16le("Zöe ✓")), "Zöe ✓");
    }

    #[test]
    fn encodes_only_what_comes_back_the_same() {
        assert_eq!(encode(1252, "\u{201c}é\u{201d}"), Some(vec![0x93, 0xE9, 0x94]));
        assert_eq!(encode(1251, "При"), Some(vec![0xCF, 0xF0, 0xE8]));
        assert_eq!(encode(1252, "При"), None);
        assert_eq!(encode(1252, "Zöe ✓"), None);
        assert_eq!(encode(1200, "A"), None);
        assert_eq!(encode(932, "日本語").map(|b| decode(932, &b)), Some("日本語".to_string()));
        assert_eq!(encode(424_242, "é").map(|b| decode(424_242, &b)), Some("é".to_string()));
        // A code page with states: whatever comes of it has to read back as it was.
        for text in ["日本語 abc", "abc", "\u{1b}(B"] {
            if let Some(bytes) = encode(50220, text) {
                assert_eq!(decode(50220, &bytes), text);
            }
        }
    }
}
