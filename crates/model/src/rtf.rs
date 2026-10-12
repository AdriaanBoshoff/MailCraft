//! RTF message bodies: the compressed form Outlook stores in PidTagRtfCompressed ([MS-OXRTFCP]),
//! a tolerant reader of RTF itself, and the HTML or plain text Outlook wraps in RTF ([MS-OXRTFEX]).
//!
//! Everything here is a reader of hostile bytes: sizes in the data are never trusted, nesting and
//! output are capped, and nothing in a body is run or fetched.

use std::collections::HashMap;

use crate::{codepage, html};

/// The most RTF read from one body.
pub const MAX_RTF: usize = 8 << 20;
/// The most text taken out of one body.
const MAX_TEXT: usize = 2 << 20;
const MAX_DEPTH: usize = 256;
const MAX_FONTS: usize = 4096;

/// COMPTYPE values ([MS-OXRTFCP] 2.1.3.1.1): "LZFu" and "MELA".
const COMPRESSED: u32 = 0x7546_5A4C;
const UNCOMPRESSED: u32 = 0x414C_454D;
const DICT_SIZE: usize = 4096;
/// The text the dictionary starts with ([MS-OXRTFCP] 2.1.2.1), 207 bytes.
const DICT_START: &[u8] = b"{\\rtf1\\ansi\\mac\\deff0\\deftab720{\\fonttbl;}{\\f0\\fnil \\froman \\fswiss \\fmodern \\fscript \\fdecor MS Sans SerifSymbolArialTimes New RomanCourier{\\colortbl\\red0\\green0\\blue0\r\n\\par \\pard\\plain\\f0\\fs20\\b\\i\\u\\tab\\tx";

/// PidTagRtfCompressed → RTF ([MS-OXRTFCP] 3.2.1 and 3.2.2). `None` when the header is not one.
/// The header's sizes only ever shorten what is read, and its checksum is not checked: a body with
/// a damaged tail is better shown than hidden.
pub fn decompress(data: &[u8]) -> Option<Vec<u8>> {
    let word = |at: usize| data.get(at..at.checked_add(4)?).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_le_bytes);
    let (comp_size, kind) = (word(0)?, word(8)?);
    // COMPSIZE counts everything after itself.
    let end = usize::try_from(comp_size).ok().and_then(|n| n.checked_add(4)).filter(|end| (16..=data.len()).contains(end)).unwrap_or(data.len());
    let body = data.get(16..end)?;
    match kind {
        UNCOMPRESSED => Some(body.get(..body.len().min(MAX_RTF))?.to_vec()),
        COMPRESSED => Some(inflate(body)),
        _ => None,
    }
}

/// The LZFu stream: runs of eight tokens after a control byte whose bits, lowest first, say
/// literal (0) or dictionary reference (1). A reference is 12 bits of offset and 4 of length - 2,
/// big-endian; one that points at the write position ends the stream.
fn inflate(body: &[u8]) -> Vec<u8> {
    let mut dict = vec![0u8; DICT_SIZE];
    for (slot, b) in dict.iter_mut().zip(DICT_START) {
        *slot = *b;
    }
    let mut pos = DICT_START.len() % DICT_SIZE;
    let mut out = Vec::new();
    let put = |b: u8, out: &mut Vec<u8>, dict: &mut Vec<u8>, pos: usize| {
        out.push(b);
        if let Some(slot) = dict.get_mut(pos) {
            *slot = b;
        }
        (pos + 1) % DICT_SIZE
    };
    let mut at = 0usize;
    'stream: while let Some(&control) = body.get(at) {
        at += 1;
        for bit in 0..8 {
            if out.len() >= MAX_RTF {
                break 'stream;
            }
            if control >> bit & 1 == 0 {
                let Some(&b) = body.get(at) else { break 'stream };
                at += 1;
                pos = put(b, &mut out, &mut dict, pos);
            } else {
                let (Some(&hi), Some(&lo)) = (body.get(at), body.get(at + 1)) else { break 'stream };
                at += 2;
                let reference = u16::from_be_bytes([hi, lo]);
                let offset = usize::from(reference >> 4);
                if offset == pos {
                    break 'stream;
                }
                for k in 0..usize::from(reference & 15) + 2 {
                    let b = dict.get((offset + k) % DICT_SIZE).copied().unwrap_or(0);
                    pos = put(b, &mut out, &mut dict, pos);
                }
            }
        }
    }
    out.truncate(MAX_RTF);
    out
}

/// RTF → the uncompressed form of PidTagRtfCompressed ([MS-OXRTFCP] 2.1.3.1: COMPTYPE "MELA",
/// checksum zero). Every reader of the property accepts it.
pub fn store(rtf: &[u8]) -> Vec<u8> {
    let raw = u32::try_from(rtf.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(rtf.len().saturating_add(16));
    out.extend_from_slice(&raw.saturating_add(12).to_le_bytes());
    out.extend_from_slice(&raw.to_le_bytes());
    out.extend_from_slice(&UNCOMPRESSED.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(rtf);
    out
}

enum Token<'a> {
    Open,
    Close,
    Word(&'a str, Option<i32>),
    Symbol(u8),
    Hex(u8),
    Bytes(&'a [u8]),
    /// The bytes after `\binN`, and whether all N of them were there.
    Bin(&'a [u8], bool),
}

struct Tokens<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Tokens<'a> {
    fn at(&self, pos: usize) -> Option<u8> {
        self.src.get(pos).copied()
    }

    fn next(&mut self) -> Option<Token<'a>> {
        loop {
            match self.at(self.pos)? {
                b'{' => {
                    self.pos += 1;
                    return Some(Token::Open);
                }
                b'}' => {
                    self.pos += 1;
                    return Some(Token::Close);
                }
                // Line ends in the file mean nothing.
                b'\r' | b'\n' | 0 => self.pos += 1,
                b'\\' => return Some(self.control()),
                _ => {
                    let start = self.pos;
                    while self.at(self.pos).is_some_and(|c| !matches!(c, b'{' | b'}' | b'\\' | b'\r' | b'\n' | 0)) {
                        self.pos += 1;
                    }
                    return Some(Token::Bytes(self.src.get(start..self.pos).unwrap_or_default()));
                }
            }
        }
    }

    /// A control word (letters, an optional number, one optional space), `\'hh`, or a control symbol.
    fn control(&mut self) -> Token<'a> {
        self.pos += 1;
        let Some(c) = self.at(self.pos) else { return Token::Bytes(&[]) };
        if c == b'\'' {
            let hex = self.src.get(self.pos + 1..self.pos + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
            self.pos = (self.pos + 3).min(self.src.len());
            return hex.map_or(Token::Bytes(&[]), Token::Hex);
        }
        if !c.is_ascii_alphabetic() {
            self.pos += 1;
            return Token::Symbol(c);
        }
        let start = self.pos;
        while self.pos - start < 32 && self.at(self.pos).is_some_and(|c| c.is_ascii_alphabetic()) {
            self.pos += 1;
        }
        let word = self.src.get(start..self.pos).and_then(|w| std::str::from_utf8(w).ok()).unwrap_or("");
        let number = self.pos;
        if self.at(self.pos) == Some(b'-') {
            self.pos += 1;
        }
        let digits = self.pos;
        while self.pos - digits < 10 && self.at(self.pos).is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        let param = if self.pos > digits {
            self.src.get(number..self.pos).and_then(|n| std::str::from_utf8(n).ok()).and_then(|n| n.parse::<i32>().ok())
        } else {
            self.pos = number;
            None
        };
        if self.at(self.pos) == Some(b' ') {
            self.pos += 1;
        }
        if word == "bin" {
            // Raw bytes follow, as many as the number says (RTF 1.9.1, "Pictures"); they are
            // never text, whatever they look like.
            let n = usize::try_from(param.unwrap_or(0)).unwrap_or(0);
            let start = self.pos;
            self.pos = self.pos.saturating_add(n).min(self.src.len());
            return Token::Bin(self.src.get(start..self.pos).unwrap_or_default(), self.pos - start == n);
        }
        Token::Word(word, param)
    }
}

/// State that an RTF group saves when it opens and restores when it closes. Nesting deeper than
/// the cap shares the innermost saved state instead of growing.
pub struct Scope<T> {
    pub cur: T,
    stack: Vec<T>,
    overflow: usize,
}

impl<T: Clone> Scope<T> {
    pub fn new(cur: T) -> Self {
        Scope { cur, stack: Vec::new(), overflow: 0 }
    }

    pub fn open(&mut self) {
        if self.stack.len() < MAX_DEPTH {
            self.stack.push(self.cur.clone());
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
    }

    pub fn close(&mut self) {
        if self.overflow > 0 {
            self.overflow -= 1;
        } else if let Some(saved) = self.stack.pop() {
            self.cur = saved;
        }
    }
}

/// What a [`Reader`] hands out. Text is already decoded: `\'hh` bytes through the code page of
/// the font in use, `\uN` with its fallback characters dropped, and the named characters
/// (`\tab`, `\emdash`, `\~` …) as themselves. `\*` arrives as the word `"*"`.
#[derive(Debug, PartialEq)]
pub enum Event<'a> {
    Open,
    Close,
    Word(&'a str, Option<i32>),
    Text(String),
    /// Bytes written as they are (`\binN`), and whether all that were promised are there. They
    /// are data for whatever group they stand in, never text.
    Bin(&'a [u8], bool),
}

#[derive(Clone, Copy)]
struct Chars {
    /// Fallback characters after each `\uN`.
    uc: usize,
    code_page: u32,
    in_fonts: bool,
}

pub struct Reader<'a> {
    tokens: Tokens<'a>,
    scope: Scope<Chars>,
    default_cp: u32,
    /// Font number → code page, from the font table's `\fcharset` and `\cpg`.
    fonts: HashMap<i32, u32>,
    declaring: i32,
    bytes: Vec<u8>,
    units: Vec<u16>,
    text: String,
    skip: usize,
    held: Option<Event<'a>>,
    emitted: usize,
    uncounted: bool,
    truncated: bool,
}

impl<'a> Reader<'a> {
    pub fn new(rtf: &'a [u8]) -> Self {
        let src = rtf.get(..rtf.len().min(MAX_RTF)).unwrap_or_default();
        Reader {
            tokens: Tokens { src, pos: 0 },
            scope: Scope::new(Chars { uc: 1, code_page: 1252, in_fonts: false }),
            default_cp: 1252,
            fonts: HashMap::new(),
            declaring: 0,
            bytes: Vec::new(),
            units: Vec::new(),
            text: String::new(),
            skip: 0,
            held: None,
            emitted: 0,
            uncounted: false,
            truncated: rtf.len() > MAX_RTF,
        }
    }

    /// The body was longer than the limits; the rest was not read.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Whether what is read from here on counts towards the cap on text. What a caller reads as
    /// data and not as text (the hex digits of a picture, a destination it skips) is bounded by
    /// the size of the body alone, so that a picture does not cut the text after it short.
    pub fn count_text(&mut self, on: bool) {
        self.uncounted = !on;
    }

    fn flush(&mut self) {
        if !self.bytes.is_empty() {
            self.text.push_str(&codepage::decode(self.scope.cur.code_page, &self.bytes));
            self.bytes.clear();
        }
        if !self.units.is_empty() {
            self.text.push_str(&String::from_utf16_lossy(&self.units));
            self.units.clear();
        }
    }

    fn literal(&mut self, s: &str) {
        self.flush();
        self.text.push_str(s);
    }

    fn take_text(&mut self) -> Option<Event<'a>> {
        if self.text.is_empty() {
            return None;
        }
        if !self.uncounted {
            self.emitted = self.emitted.saturating_add(self.text.len());
        }
        if self.emitted > MAX_TEXT {
            self.truncated = true;
            self.tokens.pos = self.tokens.src.len();
        }
        Some(Event::Text(std::mem::take(&mut self.text)))
    }

    /// Handles what the reader keeps to itself; returns the word when the caller should see it.
    fn word(&mut self, word: &'a str, param: Option<i32>) -> Option<Event<'a>> {
        if self.skip > 0 {
            // A control word after `\uN` counts as one fallback character.
            self.skip -= 1;
            return None;
        }
        let named = match word {
            "tab" => "\t",
            "emdash" => "\u{2014}",
            "endash" => "\u{2013}",
            "emspace" => "\u{2003}",
            "enspace" => "\u{2002}",
            "qmspace" => "\u{2005}",
            "bullet" => "\u{2022}",
            "lquote" => "\u{2018}",
            "rquote" => "\u{2019}",
            "ldblquote" => "\u{201c}",
            "rdblquote" => "\u{201d}",
            _ => "",
        };
        if !named.is_empty() {
            self.literal(named);
            return None;
        }
        if word == "u" {
            // Written as a signed 16-bit number; two in a row can be a surrogate pair.
            if let Some(n) = param {
                if !self.bytes.is_empty() {
                    self.flush();
                }
                self.units.push(u16::try_from(n & 0xFFFF).unwrap_or(0xFFFD));
                self.skip = self.scope.cur.uc;
            }
            return None;
        }
        self.flush();
        match word {
            "uc" => {
                self.scope.cur.uc = usize::try_from(param.unwrap_or(1).clamp(0, 8)).unwrap_or(1);
                return None;
            }
            "ansicpg" => {
                if let Some(cp) = param.and_then(|p| u32::try_from(p).ok()) {
                    self.default_cp = cp;
                    self.scope.cur.code_page = cp;
                }
            }
            "fonttbl" => self.scope.cur.in_fonts = true,
            "f" if self.scope.cur.in_fonts => self.declaring = param.unwrap_or(0),
            "f" => self.scope.cur.code_page = self.fonts.get(&param.unwrap_or(0)).copied().unwrap_or(self.default_cp),
            "fcharset" | "cpg" if self.scope.cur.in_fonts => {
                let cp = if word == "cpg" { param.and_then(|p| u32::try_from(p).ok()) } else { charset_code_page(param.unwrap_or(1)) };
                if let Some(cp) = cp
                    && (self.fonts.len() < MAX_FONTS || self.fonts.contains_key(&self.declaring))
                {
                    self.fonts.insert(self.declaring, cp);
                }
            }
            "plain" => self.scope.cur.code_page = self.default_cp,
            _ => {}
        }
        Some(Event::Word(word, param))
    }
}

impl<'a> Iterator for Reader<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Event<'a>> {
        if let Some(event) = self.held.take() {
            return Some(event);
        }
        loop {
            let Some(token) = self.tokens.next() else {
                self.flush();
                return self.take_text();
            };
            let event = match token {
                Token::Bytes(b) => {
                    let dropped = self.skip.min(b.len());
                    self.skip -= dropped;
                    let rest = b.get(dropped..).unwrap_or_default();
                    if !rest.is_empty() {
                        if !self.units.is_empty() {
                            self.flush();
                        }
                        self.bytes.extend_from_slice(rest);
                    }
                    continue;
                }
                Token::Hex(b) => {
                    if self.skip > 0 {
                        self.skip -= 1;
                    } else {
                        if !self.units.is_empty() {
                            self.flush();
                        }
                        self.bytes.push(b);
                    }
                    continue;
                }
                Token::Open => {
                    self.skip = 0;
                    self.flush();
                    self.scope.open();
                    Event::Open
                }
                Token::Close => {
                    self.skip = 0;
                    self.flush();
                    self.scope.close();
                    Event::Close
                }
                Token::Symbol(c) => {
                    if self.skip > 0 {
                        self.skip -= 1;
                        continue;
                    }
                    match c {
                        b'~' => self.literal("\u{a0}"),
                        b'_' => self.literal("\u{2011}"),
                        b'{' => self.literal("{"),
                        b'}' => self.literal("}"),
                        b'\\' => self.literal("\\"),
                        _ => {}
                    }
                    match c {
                        // A backslash before a line end is a paragraph end.
                        b'\r' | b'\n' => Event::Word("par", None),
                        b'*' => Event::Word("*", None),
                        _ => continue,
                    }
                }
                Token::Word(word, param) => match self.word(word, param) {
                    Some(event) => event,
                    None => continue,
                },
                Token::Bin(data, whole) => {
                    // Like any control word after `\uN`, it counts as one fallback character.
                    if self.skip > 0 {
                        self.skip -= 1;
                        continue;
                    }
                    Event::Bin(data, whole)
                }
            };
            self.flush();
            return match self.take_text() {
                Some(text) => {
                    self.held = Some(event);
                    Some(text)
                }
                None => Some(event),
            };
        }
    }
}

/// `\fcharsetN` → Windows code page. `None` for the default and symbol character sets.
fn charset_code_page(charset: i32) -> Option<u32> {
    Some(match charset {
        0 => 1252,
        128 => 932,
        129 => 949,
        134 => 936,
        136 => 950,
        161 => 1253,
        162 => 1254,
        163 => 1258,
        177 => 1255,
        178 => 1256,
        186 => 1257,
        204 => 1251,
        222 => 874,
        238 => 1250,
        _ => return None,
    })
}

/// Groups that start with one of these hold no body text. (Groups that start with `\*` are
/// skipped by every reader that does not know them, whatever the word.)
pub fn is_skipped_destination(word: &str) -> bool {
    matches!(
        word,
        "fonttbl"
            | "colortbl"
            | "stylesheet"
            | "info"
            | "pict"
            | "object"
            | "objdata"
            | "header"
            | "headerl"
            | "headerr"
            | "headerf"
            | "footer"
            | "footerl"
            | "footerr"
            | "footerf"
            | "footnote"
            | "annotation"
            | "filetbl"
            | "revtbl"
            | "listtable"
            | "listoverridetable"
            | "rsidtbl"
            | "generator"
            | "themedata"
            | "colorschememapping"
            | "datastore"
            | "latentstyles"
            | "xmlnstbl"
            | "nonshppict"
            | "nonesttables"
            | "bkmkstart"
            | "bkmkend"
            | "template"
            | "tc"
            | "xe"
    )
}

/// What an RTF body encapsulates ([MS-OXRTFEX] 2.1.3.1: `\fromhtml1` or `\fromtext` in the header).
#[derive(Debug, PartialEq, Eq)]
pub enum Wrapped {
    Html(String),
    Text(String),
}

/// Take the original HTML or plain text back out of an RTF body that encapsulates one
/// ([MS-OXRTFEX] 3.1.3): the text of `\*\htmltag` groups, plus the text outside them that is not
/// between `\htmlrtf` and `\htmlrtf0`. `None` when the body is ordinary RTF.
pub fn encapsulated(rtf: &[u8]) -> Option<Wrapped> {
    #[derive(Clone, Copy, Default)]
    struct State {
        skip: bool,
        tag: bool,
        rtf_only: bool,
        star: bool,
    }
    let shown = |s: &State| !s.skip && (s.tag || !s.rtf_only);
    let mut scope = Scope::new(State::default());
    let mut kind = None;
    let mut out = String::new();
    for event in Reader::new(rtf) {
        match event {
            Event::Open => {
                scope.open();
                scope.cur.star = false;
            }
            Event::Close => scope.close(),
            Event::Word("*", _) => scope.cur.star = true,
            Event::Word(word, param) => {
                let star = std::mem::take(&mut scope.cur.star);
                match word {
                    "fromhtml" if param != Some(0) && kind.is_none() => kind = Some(true),
                    "fromtext" if kind.is_none() => kind = Some(false),
                    "htmltag" if star => scope.cur.tag = true,
                    "htmlrtf" => scope.cur.rtf_only = param != Some(0),
                    "par" | "line" if shown(&scope.cur) => out.push_str("\r\n"),
                    _ if star || is_skipped_destination(word) => scope.cur.skip = true,
                    _ => {}
                }
            }
            Event::Bin(..) => {}
            Event::Text(text) => {
                scope.cur.star = false;
                if shown(&scope.cur) {
                    // Body text before the header said so: this is ordinary RTF.
                    kind?;
                    out.push_str(&text);
                }
            }
        }
    }
    Some(if kind? { Wrapped::Html(out) } else { Wrapped::Text(out) })
}

/// The text of an RTF body, one line per paragraph, table cells separated by tabs. An
/// encapsulated HTML or text body gives the text of what it wraps.
pub fn to_text(rtf: &[u8]) -> String {
    match encapsulated(rtf) {
        Some(Wrapped::Html(h)) => return html::to_text(&h),
        Some(Wrapped::Text(t)) => return t.replace("\r\n", "\n").trim_end().to_string(),
        None => {}
    }
    #[derive(Clone, Copy, Default)]
    struct State {
        skip: bool,
        hidden: bool,
        star: bool,
    }
    let mut scope = Scope::new(State::default());
    let mut out = String::new();
    for event in Reader::new(rtf) {
        match event {
            Event::Open => {
                scope.open();
                scope.cur.star = false;
            }
            Event::Close => scope.close(),
            Event::Word("*", _) => scope.cur.star = true,
            Event::Word(word, param) => {
                let star = std::mem::take(&mut scope.cur.star);
                match word {
                    _ if scope.cur.skip => {}
                    "par" | "line" | "row" | "sect" | "page" => out.push('\n'),
                    "cell" => out.push('\t'),
                    "v" => scope.cur.hidden = param != Some(0),
                    "plain" => scope.cur.hidden = false,
                    _ if star || is_skipped_destination(word) => scope.cur.skip = true,
                    _ => {}
                }
            }
            Event::Bin(..) => {}
            Event::Text(text) => {
                scope.cur.star = false;
                if !scope.cur.skip && !scope.cur.hidden {
                    out.push_str(&text);
                }
            }
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked example of [MS-OXRTFCP] 4.1.
    const SPEC_COMPRESSED: [u8; 49] = [
        0x2d, 0x00, 0x00, 0x00, 0x2b, 0x00, 0x00, 0x00, 0x4c, 0x5a, 0x46, 0x75, 0xf1, 0xc5, 0xc7, 0xa7, 0x03, 0x00, 0x0a, 0x00, 0x72, 0x63, 0x70,
        0x67, 0x31, 0x32, 0x35, 0x42, 0x32, 0x0a, 0xf3, 0x20, 0x68, 0x65, 0x6c, 0x09, 0x00, 0x20, 0x62, 0x77, 0x05, 0xb0, 0x6c, 0x64, 0x7d, 0x0a,
        0x80, 0x0f, 0xa0,
    ];
    const SPEC_RTF: &[u8] = b"{\\rtf1\\ansi\\ansicpg1252\\pard hello world}\r\n";

    #[test]
    fn decompresses_the_specification_example() {
        assert_eq!(DICT_START.len(), 207);
        assert_eq!(decompress(&SPEC_COMPRESSED).as_deref(), Some(SPEC_RTF));
        assert_eq!(to_text(SPEC_RTF), "hello world");
    }

    #[test]
    fn stored_form_reads_back() {
        let stored = store(SPEC_RTF);
        assert_eq!(stored.len(), SPEC_RTF.len() + 16);
        assert_eq!(decompress(&stored).as_deref(), Some(SPEC_RTF));
        assert_eq!(decompress(&store(b"")).as_deref(), Some(&b""[..]));
    }

    #[test]
    fn hostile_compressed_data_never_panics() {
        assert_eq!(decompress(&[]), None);
        assert_eq!(decompress(&[0; 15]), None);
        assert_eq!(decompress(&[0xFF; 64]), None, "an unknown COMPTYPE is not a body");
        for cut in 0..SPEC_COMPRESSED.len() {
            let _ = decompress(&SPEC_COMPRESSED[..cut]);
        }
        // Sizes that lie, and a stream of nothing but references into the dictionary.
        let mut lying = SPEC_COMPRESSED.to_vec();
        lying[..8].copy_from_slice(&[0xFF; 8]);
        assert_eq!(decompress(&lying).as_deref(), Some(SPEC_RTF));
        let mut loops = vec![0u8; 16];
        loops[8..12].copy_from_slice(&COMPRESSED.to_le_bytes());
        loops.extend(
            std::iter::repeat_n([0xFF, 0x00, 0x0F, 0x00, 0x0F, 0x00, 0x0F, 0x00, 0x0F, 0x00, 0x0F, 0x00, 0x0F, 0x00, 0x0F, 0x00, 0x0F], 4096)
                .flatten(),
        );
        assert!(decompress(&loops).is_some_and(|out| out.len() <= MAX_RTF));
    }

    #[test]
    fn reads_characters_by_code_page_and_unicode() {
        let rtf = b"{\\rtf1\\ansi\\ansicpg1252{\\fonttbl{\\f0\\fswiss Arial;}{\\f1\\fcharset204 Arial Cyr;}}\\f0 caf\\'e9 \\u8364?5 {\\uc2\\u8212\\'3f\\'3f}\\f1 \\'cf\\'f0\\'e8 \\{x\\}\\~y\\tab z\\emdash\\u-10179?\\u-8704?}";
        assert_eq!(to_text(rtf), "café €5 —При {x}\u{a0}y\tz\u{2014}\u{1f600}");
    }

    #[test]
    fn plain_rtf_gives_paragraphs_and_skips_what_is_not_body() {
        let rtf = b"{\\rtf1\\ansi{\\fonttbl{\\f0 Times;}}{\\colortbl;\\red255\\green0\\blue0;}{\\stylesheet{\\s0 Normal;}}{\\info{\\title Secret}}\
            {\\*\\generator Something;}\\pard One\\par Two{\\v hidden}\\line Three\\par{\\pict\\bin4 }{}}}\\par\
            \\trowd\\intbl A\\cell B\\cell\\row\\pard {\\field{\\*\\fldinst HYPERLINK \"http://example.org/\"}{\\fldrslt link}}\\\nEnd}";
        assert_eq!(to_text(rtf), "One\nTwo\nThree\n\nA\tB\t\nlink\nEnd");
    }

    #[test]
    fn takes_encapsulated_html_and_text_back_out() {
        let rtf = b"{\\rtf1\\ansi\\ansicpg1252\\fromhtml1 \\deff0{\\fonttbl{\\f0\\fswiss Arial;}}{\\colortbl;}\
            {\\*\\htmltag19 <html>}{\\*\\htmltag34 <body>}{\\*\\htmltag64 <p>}\\htmlrtf {\\b \\htmlrtf0 Caf\\'e9 & {\\*\\htmltag84 <b>}bold{\\*\\htmltag92 </b>}\
            \\htmlrtf }\\par\\htmlrtf0{\\*\\htmltag72 </p>}{\\*\\htmltag0 \\par }{\\*\\mhtmltag ignored}{\\*\\htmltag58 </body>}{\\*\\htmltag27 </html>}}";
        assert_eq!(encapsulated(rtf), Some(Wrapped::Html("<html><body><p>Café & <b>bold</b></p>\r\n</body></html>".to_string())));
        assert_eq!(to_text(rtf), "Café & bold");
        let text = b"{\\rtf1\\ansi\\fromtext \\deff0{\\fonttbl{\\f0 Courier;}}\\pard\\f0 Line one\\par Line two\\par}";
        assert_eq!(encapsulated(text), Some(Wrapped::Text("Line one\r\nLine two\r\n".to_string())));
        assert_eq!(to_text(text), "Line one\nLine two");
        assert_eq!(encapsulated(b"{\\rtf1\\ansi\\pard plain}"), None);
        assert_eq!(encapsulated(b"{\\rtf1\\ansi text first\\fromhtml1 }"), None);
    }

    #[test]
    fn hostile_rtf_never_panics_and_stays_bounded() {
        for junk in [
            &b""[..],
            b"\\",
            b"\\'",
            b"\\'z",
            b"{\\",
            b"}}}}",
            b"\\u",
            b"\\bin999999999 x",
            b"\\uc99\\u65 abc",
            b"\\-9",
            b"\\'e",
            b"{\\*",
            b"\\u99999999999 x",
        ] {
            let _ = to_text(junk);
            let _ = encapsulated(junk);
        }
        let deep = [vec![b'{'; 100_000], b"deep".to_vec(), vec![b'}'; 100_000]].concat();
        assert_eq!(to_text(&deep), "deep");
        let long = [b"{\\rtf1 ".to_vec(), vec![b'a'; 3 << 20], b"}".to_vec()].concat();
        let mut reader = Reader::new(&long);
        let read: usize = reader.by_ref().map(|e| if let Event::Text(t) = e { t.len() } else { 0 }).sum();
        assert!(read <= 3 << 20);
        assert!(reader.truncated());
        assert!(to_text(&[0xFFu8; 4096]).chars().count() <= 4096);
    }
}
