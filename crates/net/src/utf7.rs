//! Mailbox names in modified UTF-7 ([RFC 3501] 5.1.3): printable ASCII stands for itself, `&`
//! is `&-`, and anything else is UTF-16 in a base64 with `,` for `/`, between `&` and `-`.

use base64::Engine as _;
use base64::alphabet::Alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

fn engine() -> Option<GeneralPurpose> {
    let alphabet = Alphabet::new("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,").ok()?;
    let config = GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::RequireNone)
        .with_decode_allow_trailing_bits(true);
    Some(GeneralPurpose::new(&alphabet, config))
}

/// A name as the server stores it, read. A run that does not decode is kept as it was sent.
pub fn decode(name: &str) -> String {
    let Some(b64) = engine() else { return name.to_string() };
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at).unwrap_or_default());
        let after = rest.get(at + 1..).unwrap_or_default();
        let Some(end) = after.find('-') else {
            out.push_str(rest.get(at..).unwrap_or_default());
            return out;
        };
        let run = after.get(..end).unwrap_or_default();
        if run.is_empty() {
            out.push('&');
        } else {
            let units: Option<Vec<u16>> =
                b64.decode(run).ok().filter(|b| b.len() % 2 == 0).map(|b| b.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect());
            match units.and_then(|u| String::from_utf16(&u).ok()) {
                Some(text) => out.push_str(&text),
                None => {
                    out.push('&');
                    out.push_str(run);
                    out.push('-');
                }
            }
        }
        rest = after.get(end + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// A name as it is sent to the server.
pub fn encode(name: &str) -> String {
    let Some(b64) = engine() else { return name.to_string() };
    let mut out = String::with_capacity(name.len());
    let mut wide: Vec<u16> = Vec::new();
    let flush = |wide: &mut Vec<u16>, out: &mut String| {
        if !wide.is_empty() {
            let bytes: Vec<u8> = wide.iter().flat_map(|u| u.to_be_bytes()).collect();
            out.push('&');
            out.push_str(&b64.encode(bytes));
            out.push('-');
            wide.clear();
        }
    };
    for c in name.chars() {
        if (' '..='~').contains(&c) {
            flush(&mut wide, &mut out);
            if c == '&' {
                out.push_str("&-");
            } else {
                out.push(c);
            }
        } else {
            let mut units = [0u16; 2];
            wide.extend_from_slice(c.encode_utf16(&mut units));
        }
    }
    flush(&mut wide, &mut out);
    out
}
