//! What can be told about a link without following it: where it goes ([RFC 3986] 3.2, the
//! authority of an address), and whether that is where its text says it goes. Mail that is after
//! a password writes one site's name and links to another; a reader should be told so before
//! deciding anything. Nothing here opens, fetches or resolves an address.

use crate::rich::Run;

/// Characters of an address or of a link's text that are looked at.
const MAX_LOOKED_AT: usize = 2048;
/// Characters of a name that are said back.
const MAX_SAID: usize = 80;

/// Endings that make the last part of a name a file's kind and not a place: `report.pdf` is not
/// a site.
const FILE_ENDINGS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "csv", "zip", "png", "jpg", "jpeg", "gif", "bmp", "svg", "htm", "html", "msg",
    "eml", "ics", "vcf", "pst", "exe", "dll", "js", "json", "xml", "md", "log", "mp3", "mp4", "mov", "wav",
];

/// Something about a link its reader should know.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Concern {
    /// The text names one place and the link goes to another.
    Elsewhere { says: String, goes: String },
    /// The address has a name before an `@`, which is not where it goes.
    Disguised { goes: String },
    /// The place is written in letters outside the plain Latin ones, which can look like others.
    OtherLetters { goes: String },
    /// The place is a number, not a name.
    Number { goes: String },
    /// Not an address of a page, a file or a mailbox: it would start something else.
    NotAnAddress { scheme: String },
}

impl Concern {
    /// The concern in a sentence.
    pub fn words(&self) -> String {
        match self {
            Self::Elsewhere { says, goes } => format!("The text names {says}, but the link goes to {goes}."),
            Self::Disguised { goes } => format!("The address is written to look like another place: it goes to {goes}."),
            Self::OtherLetters { goes } => format!("The place is written in letters that can pass for others: {goes}."),
            Self::Number { goes } => format!("The link goes to a number, not a name: {goes}."),
            Self::NotAnAddress { scheme } => format!("This is not a web address: it starts with \"{scheme}:\"."),
        }
    }
}

/// Where an address goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub scheme: String,
    /// In lower case, without a port, a leading `www.` or a closing dot.
    pub host: String,
    /// A name stood before an `@` in the address.
    pub disguised: bool,
}

fn said(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(MAX_SAID).collect()
}

fn scheme_of(link: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = link.split_once(':')?;
    let mut chars = scheme.chars();
    (chars.next()?.is_ascii_alphabetic() && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))).then_some((scheme, rest))
}

fn tidy(host: &str) -> String {
    let host = host.trim_end_matches('.').to_lowercase();
    host.strip_prefix("www.").map(str::to_string).unwrap_or(host)
}

/// Where `link` goes. `None` for an address with no place in it: one inside the message, one
/// without a scheme, a telephone number.
pub fn place(link: &str) -> Option<Place> {
    // Programs that follow links drop the white space and control characters inside one, so an
    // address is read here the way it would be followed.
    let link: String = link.chars().filter(|c| !c.is_whitespace() && !c.is_control()).take(MAX_LOOKED_AT).collect();
    let (scheme, rest) = scheme_of(&link)?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme == "mailto" {
        let address = rest.split('?').next()?;
        let (_, host) = address.rsplit_once('@')?;
        return Some(Place { scheme, host: tidy(host), disguised: false });
    }
    // The authority: after two slashes (a backslash is taken for a slash by programs that follow
    // links), up to the path, the query or the fragment.
    let rest =
        rest.strip_prefix("//").or_else(|| rest.strip_prefix("\\\\")).or_else(|| rest.strip_prefix("/\\")).or_else(|| rest.strip_prefix("\\/"))?;
    let authority = rest.split(['/', '\\', '?', '#']).next()?;
    let (before, host) = match authority.rsplit_once('@') {
        Some((before, host)) => (Some(before), host),
        None => (None, authority),
    };
    // Without the port; an address in brackets ([RFC 3986] 3.2.2) keeps its colons.
    let host = match host.strip_prefix('[') {
        Some(inner) => inner.split(']').next()?,
        None => host.split(':').next()?,
    };
    if host.is_empty() {
        return None;
    }
    Some(Place { scheme, host: tidy(host), disguised: before.is_some_and(|b| !b.is_empty()) })
}

/// A name of a site: parts of letters, digits and hyphens with dots between, the last one of
/// letters only and not a file's ending.
fn site(word: &str) -> Option<String> {
    let host = word.split(['/', '?', '#', ':']).next()?;
    let host = tidy(host);
    let parts: Vec<&str> = host.split('.').collect();
    let last = parts.last()?;
    let shaped = parts.len() >= 2
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '-'))
        && (2..=24).contains(&last.chars().count())
        && last.chars().all(char::is_alphabetic)
        && !FILE_ENDINGS.contains(last);
    shaped.then_some(host)
}

/// The place a link's text names, if it names one: an address written out anywhere in it, or the
/// whole text being the name of a site or of a mailbox.
fn named(text: &str, mailbox: bool) -> Option<String> {
    let trim = |word: &str| {
        word.trim_matches(|c: char| matches!(c, '<' | '>' | '(' | ')' | '[' | ']' | '"' | '\'' | ',' | ';' | '.' | '!' | '?')).to_string()
    };
    let words: Vec<String> = text.split_whitespace().map(trim).filter(|w| !w.is_empty()).take(64).collect();
    for word in &words {
        let lower = word.to_lowercase();
        if (lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("ftp://"))
            && let Some(place) = place(word)
        {
            return Some(place.host);
        }
        if lower.starts_with("www.")
            && let Some(host) = site(word)
        {
            return Some(host);
        }
    }
    let [only] = words.as_slice() else { return None };
    match only.rsplit_once('@') {
        Some((_, host)) if mailbox => site(host),
        Some(_) => None,
        None if mailbox => None,
        None => site(only),
    }
}

/// One place is the other, or a part of it: `example.org` and `login.example.org`.
fn same(a: &str, b: &str) -> bool {
    a == b || a.strip_suffix(b).is_some_and(|front| front.ends_with('.')) || b.strip_suffix(a).is_some_and(|front| front.ends_with('.'))
}

/// What a reader should know about a link with this text, or `None` when it is what it seems.
pub fn concern(text: &str, link: &str) -> Option<Concern> {
    let text: String = text.chars().take(MAX_LOOKED_AT).collect();
    let trimmed: String = link.chars().filter(|c| !c.is_whitespace() && !c.is_control()).take(MAX_LOOKED_AT).collect();
    if let Some((scheme, _)) = scheme_of(&trimmed) {
        let scheme = scheme.to_ascii_lowercase();
        if !matches!(scheme.as_str(), "http" | "https" | "ftp" | "ftps" | "mailto" | "tel" | "sms" | "cid" | "news" | "nntp") {
            return Some(Concern::NotAnAddress { scheme: said(&scheme) });
        }
    }
    let place = place(link)?;
    let goes = said(&place.host);
    if place.disguised {
        return Some(Concern::Disguised { goes });
    }
    if let Some(says) = named(&text, place.scheme == "mailto")
        && !same(&says, &place.host)
    {
        return Some(Concern::Elsewhere { says: said(&says), goes });
    }
    if !place.host.is_ascii() || place.host.split('.').any(|part| part.starts_with("xn--")) {
        return Some(Concern::OtherLetters { goes });
    }
    // An address in brackets, or parts that are all numbers, written in tens or in sixteens.
    let digits = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
    let sixteens = |part: &str| part.strip_prefix("0x").is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit()));
    let number = place.host.contains(':') || place.host.split('.').all(|part| digits(part) || sixteens(part));
    if number && place.scheme != "mailto" {
        return Some(Concern::Number { goes });
    }
    None
}

/// The whole text of the link the run at `at` is part of: the runs before and after it that go
/// to the same address, pictures by their alternative text.
pub fn text_around(runs: &[Run], at: usize) -> String {
    let Some(link) = runs.get(at).and_then(|run| run.style.link.as_ref()) else { return String::new() };
    let part = |run: &&Run| run.style.link.as_ref() == Some(link);
    let first = runs.get(..at).map_or(at, |before| at - before.iter().rev().take_while(part).count());
    let mut text = String::new();
    for run in runs.iter().skip(first).take_while(part) {
        if text.chars().count() >= MAX_LOOKED_AT {
            break;
        }
        text.push_str(&run.text);
    }
    text
}

/// What a reader should know about the link the run at `at` is part of.
pub fn concern_at(runs: &[Run], at: usize) -> Option<Concern> {
    let link = runs.get(at)?.style.link.as_deref()?;
    concern(&text_around(runs, at), link)
}

/// What following a link would start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A web page: the address as a browser may be given it.
    Page(String),
    /// A mailbox to write to.
    Mailbox(String),
}

/// Characters a web address may be written in ([RFC 3986] 2: unreserved, reserved and `%`).
fn in_address(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c)
}

/// Characters a mailbox may be written in here ([RFC 5322] 3.2.3, without the ones that would
/// mean more than one mailbox, a display name or something to decode).
fn in_mailbox(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._+@".contains(c)
}

/// What following `link` would start, or why it is not followed. Only a web page (`http`,
/// `https`) and a mailbox (`mailto`) are: any other kind of address would start a program or
/// open a file. The address is given back as it may be handed on: without white space, in the
/// plain characters an address is made of, and no longer than is looked at.
pub fn target(link: &str) -> Result<Target, &'static str> {
    let clean: String = link.chars().filter(|c| !c.is_whitespace() && !c.is_control()).take(MAX_LOOKED_AT + 1).collect();
    if clean.chars().count() > MAX_LOOKED_AT {
        return Err("The address is too long to open.");
    }
    let Some((scheme, rest)) = scheme_of(&clean) else {
        return Err("The link has no address of a web page or a mailbox.");
    };
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => {
            if !clean.chars().all(in_address) {
                return Err("The address is written in characters a web address does not have. It is not opened; it can be copied.");
            }
            match place(&clean) {
                Some(place) if !place.host.is_empty() => Ok(Target::Page(clean)),
                _ => Err("The address names no place to go to."),
            }
        }
        "mailto" => {
            let mailbox = rest.split('?').next().unwrap_or_default();
            let shaped = mailbox.split_once('@').is_some_and(|(name, host)| !name.is_empty() && host.contains('.') && !host.contains('@'));
            if shaped && mailbox.chars().count() <= 320 && mailbox.chars().all(in_mailbox) {
                Ok(Target::Mailbox(mailbox.to_string()))
            } else {
                Err("The link does not name one plain mailbox.")
            }
        }
        _ => Err("Only web pages and mailboxes are opened from a message."),
    }
}
