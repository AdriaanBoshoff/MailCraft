//! HTML message bodies as readable text. Nothing here fetches, runs or follows anything:
//! tags are dropped, scripts and styles are skipped, and remote content is never loaded.

/// Readable text for an HTML body: block tags become line breaks, entities are decoded.
pub fn to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    let mut skip_until: Option<&str> = None;
    while !rest.is_empty() {
        if let Some(end_tag) = skip_until {
            match find_ci(rest, end_tag) {
                Some(i) => {
                    rest = rest.get(i..).unwrap_or("");
                    skip_until = None;
                }
                None => break,
            }
            continue;
        }
        let Some(lt) = rest.find('<') else {
            push_text(&mut out, rest);
            break;
        };
        push_text(&mut out, rest.get(..lt).unwrap_or(""));
        let after = rest.get(lt + 1..).unwrap_or("");
        if let Some(comment) = after.strip_prefix("!--") {
            rest = comment.find("-->").and_then(|i| comment.get(i + 3..)).unwrap_or("");
            continue;
        }
        let Some(gt) = after.find('>') else {
            break;
        };
        let tag = after.get(..gt).unwrap_or("");
        rest = after.get(gt + 1..).unwrap_or("");
        let closing = tag.starts_with('/');
        let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        match name.as_str() {
            "script" | "style" | "head" | "title" if !closing => {
                skip_until = Some(match name.as_str() {
                    "script" => "</script",
                    "style" => "</style",
                    "head" => "</head",
                    _ => "</title",
                });
            }
            "br" => out.push('\n'),
            "p" | "div" | "tr" | "table" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "blockquote" | "pre" | "hr" => {
                break_line(&mut out)
            }
            "li" if !closing => {
                break_line(&mut out);
                out.push_str("• ");
            }
            "li" => break_line(&mut out),
            "td" | "th" if closing => out.push('\t'),
            _ => {}
        }
    }
    tidy(&out)
}

/// A minimal HTML body for plain text, as a mail client writes it.
pub fn from_text(text: &str) -> String {
    let mut out = String::from("<html><body>");
    for line in text.lines() {
        out.push_str("<div>");
        if line.is_empty() {
            out.push_str("&nbsp;");
        }
        for c in line.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                c => out.push(c),
            }
        }
        out.push_str("</div>");
    }
    out.push_str("</body></html>");
    out
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h.get(i..i + n.len()).is_some_and(|w| w.eq_ignore_ascii_case(n)))
}

fn break_line(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

/// Append text with whitespace runs collapsed and entities decoded.
fn push_text(out: &mut String, text: &str) {
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '&'
            && let Some((decoded, len)) = entity(rest)
        {
            out.push_str(&decoded);
            rest = rest.get(len..).unwrap_or("");
            continue;
        }
        rest = rest.get(c.len_utf8()..).unwrap_or("");
        if c.is_whitespace() {
            if !out.ends_with(' ') && !out.ends_with('\n') && !out.is_empty() {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
}

/// Decode the entity at the start of `s`: (text, bytes consumed).
pub fn entity(s: &str) -> Option<(String, usize)> {
    let end = s.char_indices().take(12).find(|(_, c)| *c == ';').map(|(i, _)| i)?;
    let name = s.get(1..end)?;
    let text = if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse::<u32>().ok()?,
        };
        char::from_u32(code)?.to_string()
    } else {
        match name {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            "nbsp" => "\u{a0}",
            "copy" => "©",
            "reg" => "®",
            "trade" => "™",
            "ndash" => "–",
            "mdash" => "—",
            "lsquo" => "‘",
            "rsquo" => "’",
            "ldquo" => "“",
            "rdquo" => "”",
            "hellip" => "…",
            "bull" => "•",
            "euro" => "€",
            "pound" => "£",
            "yen" => "¥",
            "cent" => "¢",
            "sect" => "§",
            "para" => "¶",
            "deg" => "°",
            "middot" => "·",
            "laquo" => "«",
            "raquo" => "»",
            "times" => "×",
            "divide" => "÷",
            "plusmn" => "±",
            "frac12" => "½",
            "larr" => "←",
            "rarr" => "→",
            "ensp" | "emsp" | "thinsp" => " ",
            "shy" | "zwnj" | "zwj" => "",
            _ => return None,
        }
        .to_string()
    };
    Some((text, end + 1))
}

/// Trim line ends and squeeze runs of blank lines to one.
fn tidy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank = 0;
    for line in s.lines() {
        let line = line.trim_end_matches([' ', '\t']).trim_start_matches(' ');
        if line.replace('\u{a0}', "").trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
            out.push('\n');
        } else {
            blank = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_entities_and_scripts() {
        let html = "<html><head><style>p{color:red}</style></head><body><p>Hi&nbsp;Ana &amp; Bo,</p><script>alert(1)</script><div>Line&#33; &#x41;<br>Next</div><ul><li>One</li><li>Two</li></ul></body></html>";
        assert_eq!(to_text(html), "Hi\u{a0}Ana & Bo,\nLine! A\nNext\n• One\n• Two");
    }

    #[test]
    fn hostile_markup_does_not_panic() {
        for s in ["<", "<<<>>>", "&#xFFFFFFFF;", "&;", "<script>never closed", "<!-- open", "a<b", "&#0;é<é>é", "<p\u{0}>"] {
            let _ = to_text(s);
        }
        assert_eq!(to_text("a &unknown; b"), "a &unknown; b");
    }

    #[test]
    fn text_to_html_escapes() {
        assert_eq!(from_text("a<b\n\nc"), "<html><body><div>a&lt;b</div><div>&nbsp;</div><div>c</div></body></html>");
    }
}
