use crate::html::parse;
use crate::rich::{Align, Block, Doc, Para, ParaKind, Picture, Script};

fn paras(doc: &Doc) -> Vec<&Para> {
    doc.blocks
        .iter()
        .filter_map(|b| match b {
            Block::Para(p) => Some(p),
            _ => None,
        })
        .collect()
}

#[test]
fn paragraphs_runs_and_inline_styles() {
    let doc = parse(
        "<html><head><title>x</title><style>p{color:red}</style></head><body><p>Hi <b>bold</b> and <i>it<u>al</u>ic</i>.</p><div>Next&nbsp;&amp; last</div></body></html>",
    );
    let p = paras(&doc);
    assert_eq!(p.len(), 2);
    assert_eq!(p[0].text(), "Hi bold and italic.");
    assert!(p[0].gap);
    let styles: Vec<(&str, bool, bool, bool)> =
        p[0].runs.iter().map(|r| (r.text.as_str(), r.style.bold, r.style.italic, r.style.underline)).collect();
    assert_eq!(
        styles,
        [
            ("Hi ", false, false, false),
            ("bold", true, false, false),
            (" and ", false, false, false),
            ("it", false, true, false),
            ("al", false, true, true),
            ("ic", false, true, false),
            (".", false, false, false)
        ]
    );
    assert_eq!(p[1].text(), "Next\u{a0}& last");
    assert_eq!(doc.text(), "Hi bold and italic.\nNext\u{a0}& last");
}

#[test]
fn inline_css_and_font_tags() {
    let doc = parse(
        "<p style=\"text-align:center; margin:0\"><span style=\"color:#C00000;font-weight:700;font-size:14pt\">Red</span>\
         <font color=navy size=5 face=\"Courier New\">mono</font><span style='background-color: rgb(255, 255, 0); text-decoration: line-through'>hl</span>\
         <a href=\"https://example.org/a?b=1&amp;c=2\" style=\"text-decoration:none\">link</a><sup>2</sup></p>",
    );
    let p = paras(&doc);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].align, Align::Center);
    assert!(!p[0].gap, "a zero margin leaves no gap");
    let r = &p[0].runs;
    assert_eq!((r[0].style.color, r[0].style.bold, r[0].style.size), (Some([0xC0, 0, 0]), true, Some(14.0)));
    assert_eq!((r[1].style.color, r[1].style.size, r[1].style.mono), (Some([0, 0, 128]), Some(18.0), true));
    assert_eq!((r[2].style.background, r[2].style.strike), (Some([255, 255, 0]), true));
    assert_eq!(r[3].style.link.as_deref(), Some("https://example.org/a?b=1&c=2"));
    assert!(!r[3].style.underline);
    assert_eq!(r[4].style.script, Script::Super);
}

#[test]
fn lists_headings_quotes_and_rules() {
    let doc = parse(
        "<h2>Title</h2><ul><li>One<li>Two<ol start=3><li>Three</li><li>Four</li></ol></li></ul><blockquote>Quoted<blockquote>Deeper</blockquote></blockquote><hr><pre>a\n  b\tc</pre>",
    );
    let p = paras(&doc);
    let got: Vec<(String, ParaKind, u8, u8)> = p.iter().map(|p| (p.text(), p.kind.clone(), p.indent, p.quote)).collect();
    assert_eq!(
        got,
        [
            ("Title".to_string(), ParaKind::Heading(2), 0, 0),
            ("One".to_string(), ParaKind::Item("•".to_string()), 1, 0),
            ("Two".to_string(), ParaKind::Item("•".to_string()), 1, 0),
            ("Three".to_string(), ParaKind::Item("3.".to_string()), 2, 0),
            ("Four".to_string(), ParaKind::Item("4.".to_string()), 2, 0),
            ("Quoted".to_string(), ParaKind::Body, 0, 1),
            ("Deeper".to_string(), ParaKind::Body, 0, 2),
            ("a\n  b    c".to_string(), ParaKind::Pre, 0, 0),
        ]
    );
    assert!(p[0].runs[0].style.bold);
    assert!(doc.blocks.iter().any(|b| matches!(b, Block::Rule)));
    assert!(p[7].runs[0].style.mono);
}

#[test]
fn line_breaks_and_blank_lines() {
    let doc = parse(
        "<div>one<br>two<br></div><div><br></div><div>three</div><p class=MsoNormal>four<o:p></o:p></p><p class=MsoNormal><o:p>&nbsp;</o:p></p>",
    );
    let p = paras(&doc);
    let texts: Vec<String> = p.iter().map(|p| p.text()).collect();
    assert_eq!(texts, ["one\ntwo", "\u{a0}", "three", "four", "\u{a0}"]);
    assert!(p.iter().all(|p| !p.gap), "div and word-processor paragraphs have no margins");
}

#[test]
fn tables_keep_cells_and_layout_tables_flatten() {
    let doc = parse(
        "<table border=1><tr><th>Item</th><th>Qty</th></tr><tr><td>Flyers</td><td align=right>500</td></tr><tr><td colspan=2>Total <b>500</b></td></tr></table>",
    );
    let Some(Block::Table(t)) = doc.blocks.first() else { panic!("expected a table, got {:?}", doc.blocks) };
    assert!(t.border);
    assert_eq!(t.columns(), 2);
    assert_eq!(t.rows.len(), 3);
    assert!(t.rows[0][0].header);
    assert_eq!(t.rows[2][0].span, 2);
    assert_eq!(doc.text(), "Item\tQty\nFlyers\t500\nTotal 500");
    let Some(Block::Para(qty)) = t.rows[1][1].blocks.first() else { panic!("expected text in the cell") };
    assert_eq!(qty.align, Align::Right);

    // One column, nested: reads as ordinary paragraphs.
    let doc = parse("<table><tr><td><table><tr><td><p>Hello</p></td></tr><tr><td>World</td></tr></table></td></tr></table><p>After</p>");
    assert_eq!(doc.blocks.len(), 3);
    assert!(doc.blocks.iter().all(|b| matches!(b, Block::Para(_))));
    assert_eq!(doc.text(), "Hello\nWorld\nAfter");

    // Markup left open inside a cell does not leak out of it.
    let doc = parse("<table><tr><td><b>bold<td>plain</table>after");
    let Some(Block::Table(t)) = doc.blocks.first() else { panic!("expected a table") };
    let Some(Block::Para(plain)) = t.rows[0][1].blocks.first() else { panic!("expected text") };
    assert!(!plain.runs[0].style.bold);
    assert_eq!(paras(&doc).last().map(|p| (p.text(), p.runs[0].style.bold)), Some(("after".to_string(), false)));
}

#[test]
fn nothing_active_survives_and_remote_content_is_only_counted() {
    let doc = parse(
        "<p onclick=\"evil()\">Text<script>alert('x')</script><style>p{}</style><iframe src=\"https://x.example/\">frame</iframe>\
         <object data=x>obj</object><img src=\"https://t.example/pixel.gif\" width=1 height=1><img src=\"https://x.example/a.png\" alt=\"Logo\">\
         <img src=\"cid:part1@example\" alt=chart><img src=\"data:image/png;base64,AAAA\"><span style=\"display:none\">hidden preheader</span>\
         <span style=\"font-size:0\">also hidden</span><!-- comment <b>not text</b> --><![if !mso]>shown<![endif]></p>\
         <div style=\"background:url(https://x.example/bg.png)\">bg</div>",
    );
    assert_eq!(doc.text(), "TextLogochartshown\nbg");
    assert_eq!(doc.remote_blocked, 3, "tracker, logo and background");
    let p = paras(&doc);
    let pictures: Vec<&Picture> = p[0].runs.iter().filter_map(|r| r.picture.as_ref()).collect();
    assert_eq!(pictures, [&Picture::Remote, &Picture::Attached("part1@example".to_string()), &Picture::Inline]);
    let all = format!("{doc:?}");
    for gone in ["evil", "alert", "frame", "obj", "hidden", "comment", "AAAA"] {
        assert!(!all.contains(gone), "{gone} must not be in the document");
    }
}

#[test]
fn near_white_backgrounds_are_the_page() {
    let doc = parse(
        "<body bgcolor=\"#FFFFFF\"><table><tr><td bgcolor=\"#1F4E79\"><font color=white>Banner</font></td><td style=\"background:#fafafa\">Side</td></tr></table></body>",
    );
    let Some(Block::Table(t)) = doc.blocks.first() else { panic!("expected a table") };
    let run = |c: usize| match t.rows[0][c].blocks.first() {
        Some(Block::Para(p)) => p.runs[0].style.clone(),
        _ => panic!("expected text"),
    };
    assert_eq!((run(0).background, run(0).color), (Some([0x1F, 0x4E, 0x79]), Some([255, 255, 255])));
    assert_eq!(run(1).background, None);
}

#[test]
fn hostile_markup_never_panics_and_is_capped() {
    for s in [
        "<",
        "<<<>>>",
        "</",
        "</>",
        "<a href=",
        "<a href='unterminated>text",
        "<p style=\"font-size:nanpt;color:#zzz;margin:;:;\">x",
        "<font size=99999999999 color=#12>x",
        "&#xFFFFFFFF;&;&#0;",
        "<script>never closed",
        "<!-- open",
        "<table><tr><td>",
        "</td></tr></table></table></p></b>",
        "<td>cell without table</td>",
        "<ol start=4294967295><li>a<li>b",
        "<img>",
        "<img src=cid:>",
        "<p\u{0} =\"\" ='' \"'>é<é>é",
        "a < b and c > d",
        "<table><table><table><tr><tr><td><td></table>x",
    ] {
        let _ = parse(s);
    }
    assert_eq!(parse("a < b and c > d").text(), "a < b and c > d");

    // Deep nesting: bounded element stack, bounded tables, text still read.
    let deep = format!("{}text{}", "<div><b><table><tr><td>".repeat(3000), "</td></tr></table></b></div>".repeat(3000));
    assert!(parse(&deep).text().contains("text"));
    let wide = "<table><tr>".to_string() + &"<td>c</td>".repeat(5000) + "</tr></table>";
    assert_eq!(parse(&wide).text().matches('c').count(), 5000);

    // Oversized input is cut, and says so.
    let big = "<p>word word word</p>".repeat(300_000);
    let doc = parse(&big);
    assert!(doc.truncated);
    assert!(doc.blocks.len() <= 50_000);
    let multibyte = "é".repeat(3 << 20);
    assert!(parse(&multibyte).truncated);
}

const RTF: &[u8] = b"{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fswiss Arial;}{\\f1\\fmodern Courier New;}}{\\colortbl;\\red0\\green102\\blue153;\\red255\\green242\\blue204;}\
    {\\*\\generator Ours;}\\pard\\qc\\b\\fs32\\cf1 Title\\b0\\fs20\\cf0\\par\
    \\pard Code {\\f1 RP-1}, {\\i it}{\\v hidden}{\\super 2}{\\strike old}\\line next\\par\\par\
    \\trowd\\clbrdrt\\brdrs\\cellx3000\\cellx6000\\intbl\\b Item\\cell Amount\\b0\\cell\\row\
    \\trowd\\cellx3000\\cellx6000\\intbl {\\highlight2 Total}\\cell 62.00\\cell\\row\
    \\pard\\li1440 Indented {\\pict\\wmetafile8 010203}\\par\
    \\pard See {\\field{\\*\\fldinst{HYPERLINK \"https://example.net/help\"}}{\\fldrslt{\\ul\\cf1 the help page}}} now\\par\\par\\par}";

#[test]
fn rtf_keeps_styles_paragraphs_tables_and_links() {
    let doc = crate::rtf::parse(RTF);
    assert!(!doc.truncated);
    assert_eq!(doc.text(), "Title\nCode RP-1, it2old\nnext\n\u{a0}\nItem\tAmount\nTotal\t62.00\nIndented \nSee the help page now");
    let p = paras(&doc);
    assert_eq!(p[0].align, Align::Center);
    let title = &p[0].runs[0].style;
    assert_eq!((title.bold, title.size, title.color), (true, Some(16.0), Some([0, 102, 153])));
    assert_eq!(p[1].align, Align::Left);
    let by_text = |para: &Para, text: &str| para.runs.iter().find(|r| r.text == text).map(|r| r.style.clone()).unwrap();
    assert!(by_text(p[1], "RP-1").mono);
    assert!(by_text(p[1], "it").italic);
    assert_eq!(by_text(p[1], "2").script, Script::Super);
    assert!(by_text(p[1], "old").strike);
    assert!(!by_text(p[1], "Code ").bold, "the title's bold ended with \\b0");
    let table = doc.blocks.iter().find_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).unwrap();
    assert!(table.border);
    assert_eq!((table.rows.len(), table.columns()), (2, 2));
    let Block::Para(total) = &table.rows[1][0].blocks[0] else { panic!("a paragraph") };
    assert_eq!(total.runs[0].style.background, Some([255, 242, 204]));
    assert_eq!(p[3].indent, 2);
    assert_eq!(p[3].runs[1].picture, Some(Picture::Inline));
    let link = by_text(p[4], "the help page");
    assert_eq!(link.link.as_deref(), Some("https://example.net/help"));
    assert!(link.underline);
    assert_eq!(by_text(p[4], " now").link, None);
}

#[test]
fn rtf_that_wraps_html_or_text_is_read_as_what_it_wraps() {
    let html = b"{\\rtf1\\ansi\\fromhtml1 {\\*\\htmltag64 <p>}\\htmlrtf {\\htmlrtf0 Hello {\\*\\htmltag84 <b>}you{\\*\\htmltag92 </b>}\\htmlrtf }\\htmlrtf0{\\*\\htmltag72 </p>}\
        {\\*\\htmltag4 <script>}alert(1){\\*\\htmltag12 </script>}{\\*\\htmltag4 <img src=\"http://example.com/p.gif\" width=40 height=40>}}";
    let doc = crate::rtf::parse(html);
    assert_eq!(doc.text(), "Hello you");
    assert!(paras(&doc)[0].runs[1].style.bold);
    assert_eq!(doc.remote_blocked, 1, "what the wrapped HTML points at is still not fetched");
    let text = crate::rtf::parse(b"{\\rtf1\\ansi\\fromtext \\pard One\\par\\par Three\\par}");
    assert_eq!(paras(&text).iter().map(|p| p.text()).collect::<Vec<_>>(), ["One", "\u{a0}", "Three"]);
}

#[test]
fn hostile_rtf_gives_a_bounded_document() {
    let junk: [&[u8]; 7] = [
        b"",
        b"{",
        b"}",
        b"\\",
        b"{\\rtf1\\cell\\row\\cell\\row}",
        b"{\\rtf1\\cf99999\\highlight-4 x\\fs-9 y\\fs999999 z\\li-5 q\\li99999999 r}",
        b"{\\field{\\*\\fldinst HYPERLINK}{\\fldrslt x}}",
    ];
    for rtf in junk {
        let _ = crate::rtf::parse(rtf);
    }
    let deep = [b"{\\rtf1 ".to_vec(), b"{\\b ".repeat(5000), b"deep".to_vec(), vec![b'}'; 5001]].concat();
    assert_eq!(crate::rtf::parse(&deep).text(), "deep");
    let many = [b"{\\rtf1 ".to_vec(), b"x\\par ".repeat(80_000), b"}".to_vec()].concat();
    let doc = crate::rtf::parse(&many);
    assert!(doc.truncated && doc.blocks.len() <= 50_000);
    let cells = [b"{\\rtf1 ".to_vec(), b"\\intbl c\\cell ".repeat(20_000), b"\\row}".to_vec()].concat();
    assert!(matches!(crate::rtf::parse(&cells).blocks.first(), Some(Block::Table(t)) if t.columns() == 20_000));
    let colors = [b"{\\rtf1{\\colortbl".to_vec(), b";".repeat(100_000), b"}\\cf5000 x}".to_vec()].concat();
    assert_eq!(crate::rtf::parse(&colors).text(), "x");
}
