use crate::html::parse;
use crate::rich::{Align, Block, Doc, Float, Para, ParaKind, Picture, Script};

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
        "<h2>Title</h2><ul><li>One<li>Two<ol start=3><li>Three</li><li>Four</li></ol></li></ul><blockquote type=cite>Quoted<blockquote style=\"border-left:2px solid #ccc\">Deeper</blockquote></blockquote><blockquote>Set in</blockquote><hr><pre>a\n  b\tc</pre>",
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
            ("Set in".to_string(), ParaKind::Body, 1, 0),
            ("a\n  b    c".to_string(), ParaKind::Pre, 0, 0),
        ]
    );
    assert!(p[0].runs[0].style.bold);
    assert!(doc.blocks.iter().any(|b| matches!(b, Block::Rule)));
    assert!(p[8].runs[0].style.mono);
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
    assert_eq!(pictures, [&Picture::Remote, &Picture::Attached("part1@example".to_string()), &Picture::Inline(Some(0))]);
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
    assert_eq!((run(0).background, run(0).color), (None, Some([255, 255, 255])), "the colour fills the cell, not the text");
    assert_eq!((t.rows[0][0].background, t.rows[0][1].background), (Some([0x1F, 0x4E, 0x79]), None));
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
    assert_eq!(p[3].runs[1].picture, Some(Picture::Inline(None)));
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

#[test]
fn a_picture_keeps_the_size_the_body_asks_for() {
    let doc = parse(
        "<p><img src=cid:a width=240 height=\"96px\"><img src=cid:b width=\"50%\"><img src=cid:c height=99999999>\
         <img src=cid:d width=-7 height=nan><img src=cid:e width=1e39></p>",
    );
    let extents: Vec<(u16, u16)> = paras(&doc)[0].runs.iter().filter(|r| r.picture.is_some()).map(|r| r.extent).collect();
    assert_eq!(extents, [(240, 96), (0, 0), (0, 4096), (0, 0)], "a size below nothing drops the picture; one beyond any number is no size");
}

#[test]
fn tables_keep_their_width_their_place_and_their_cell_sizes() {
    use crate::rich::TableWidth;
    let table = |html: &str| match parse(html).blocks.into_iter().next() {
        Some(Block::Table(t)) => t,
        other => panic!("expected a table, got {other:?}"),
    };
    let t = table("<table width=600 align=center><tr><td bgcolor=#1F4E79>Banner<tr><td>Body</table>");
    assert_eq!((t.width, t.align, t.columns(), t.rows.len()), (TableWidth::Pixels(600), Align::Center, 1, 2));
    let t = table("<table style=\"width:100%;MAX-WIDTH: 480px !important; margin:0 auto\"><tr><td width=120>a<td width=\"30%\">b</table>");
    assert_eq!((t.width, t.align), (TableWidth::Pixels(480), Align::Center));
    assert_eq!((t.rows[0][0].width, t.rows[0][1].width), (120, 0));
    let t = table("<table width=\"100%\"><tr><td>a<td>b</table>");
    assert_eq!((t.width, t.align), (TableWidth::Fill, Align::Left));
    let t = table("<table width=\"50%\" style=\"margin-left:auto\"><tr><td width=99999999>a<td width=-3>b</table>");
    assert_eq!((t.width, t.align, t.rows[0][0].width, t.rows[0][1].width), (TableWidth::Percent(50), Align::Left, 4096, 0));
    // One column and nothing to draw: still only layout.
    assert!(matches!(parse("<table width=\"100%\"><tr><td>a<tr><td>b</table>").blocks.first(), Some(Block::Para(_))));

    let rtf = crate::rtf::parse(
        b"{\\rtf1 \\trowd\\cellx4200\\cellx6000\\intbl a\\cell b\\cell\\row\\trowd\\cellx-50\\cellx99999999\\intbl c\\cell d\\cell e\\cell\\row}",
    );
    let Some(Block::Table(t)) = rtf.blocks.first() else { panic!("expected a table") };
    let widths: Vec<Vec<u16>> = t.rows.iter().map(|r| r.iter().map(|c| c.width).collect()).collect();
    assert_eq!(widths, [vec![280, 120], vec![0, 4096, 0]]);
}

#[test]
fn style_sheets_pick_elements_by_name_first_class_and_id() {
    let doc = parse(
        "<html><head><title>t</title><style><!--\n\
         @import url(https://x.example/a.css); /* a note */\n\
         p.note, h2 { color:#b00020; font-weight:bold }\n\
         #lead { font-size:18pt }\n\
         .plain { color:#006400 }\n\
         .box td, p > b, p.two.marks, p[lang], * { color:#00ff00 }\n\
         @media screen { p { color:#0000ff } .gone { display:block } }\n\
         .gone { display:none }\n\
         p { margin:0; color:#444488 }\n\
         a:hover { color:#ff0000 } a:link { color:#008080 }\n\
         --></style></head><body>\
         <p id=LEAD>lead</p><p class=\"Note other\">note</p><p class=\"other plain\">second</p><p class=gone>hidden</p>\
         <h2>head</h2><p class=plain style=\"color:#101010\">own</p><p class=two>x <b>b</b> <a href=\"https://x.example/\">link</a></p>",
    );
    let p = paras(&doc);
    assert_eq!(p.iter().map(|p| p.text()).collect::<Vec<_>>(), ["lead", "note", "second", "head", "own", "x b link"]);
    let color = |i: usize, run: usize| p[i].runs[run].style.color;
    assert_eq!((p[0].runs[0].style.size, color(0, 0)), (Some(18.0), Some([0x44, 0x44, 0x88])), "the id is matched whatever its case");
    assert_eq!((color(1, 0), p[1].runs[0].style.bold), (Some([0xb0, 0x00, 0x20]), true), "the first class picks the rule");
    assert_eq!(color(2, 0), Some([0x44, 0x44, 0x88]), "a second class picks nothing");
    assert_eq!(color(3, 0), Some([0xb0, 0x00, 0x20]));
    assert_eq!(color(4, 0), Some([0x10, 0x10, 0x10]), "the element's own style comes last");
    assert_eq!(color(5, 1), Some([0x44, 0x44, 0x88]), "rules with other selectors, and rules inside at-rules, are skipped");
    assert_eq!(color(5, 3), Some([0x00, 0x80, 0x80]));
    assert!(!p[1].gap && !p[2].gap && p[3].gap, "the sheet takes the paragraphs' margins away, not the heading's");

    // A link has the link colour, whatever is around it, unless the link itself is given one.
    let doc = parse(
        "<body text=#333333><p><font color=red><a href=\"https://x.example/\">a</a></font> <a href=\"https://x.example/\" style=\"color:red\">b</a></p>",
    );
    let runs = &paras(&doc)[0].runs;
    assert_eq!((runs[0].style.color, runs[2].style.color), (None, Some([255, 0, 0])));
}

#[test]
fn cells_keep_their_room_their_fills_and_their_places() {
    use crate::rich::{TableWidth, VAlign};
    let table = |html: &str| match parse(html).blocks.into_iter().next() {
        Some(Block::Table(t)) => t,
        other => panic!("expected a table, got {other:?}"),
    };
    let t = table("<table><tr><td>a<td>b</table>");
    assert_eq!((t.padding, t.spacing), (1, 2));
    let t = table(
        "<table cellpadding=12 cellspacing=\"0px\"><tr><td>a<td style=\"padding:16px 32px 4px\">b<td style=\"padding:6pt\">c<td style=\"padding:1em\">d</table>",
    );
    assert_eq!((t.padding, t.spacing), (12, 0));
    assert_eq!(t.rows[0].iter().map(|c| c.padding).collect::<Vec<_>>(), [None, Some((16, 32)), Some((8, 8)), None]);
    let t = table("<table cellpadding=99999 cellspacing=-4 style=\"border-collapse:collapse\"><tr><td>a<td>b</table>");
    assert_eq!((t.padding, t.spacing), (64, 0));
    let t = table("<table cellspacing=1e39><tr><td>a<td>b</table>");
    assert_eq!(t.spacing, 2, "a size beyond any number is no size");

    // A row's background is its cells', unless a cell has its own.
    let t = table(
        "<style>tr.band { background-color:#ffd966 }</style>\
         <table><tr bgcolor=#1F4E79><td>a<td bgcolor=#aa0000>b</tr><tr><td>c<td>d</tr><tr class=band><td>e<td>f</table>",
    );
    let fills: Vec<Vec<Option<[u8; 3]>>> = t.rows.iter().map(|r| r.iter().map(|c| c.background).collect()).collect();
    assert_eq!(fills[0], [Some([0x1F, 0x4E, 0x79]), Some([0xaa, 0, 0])]);
    assert_eq!(fills[1], [None, None]);
    assert_eq!(fills[2], [Some([0xff, 0xd9, 0x66]); 2]);
    assert!(
        t.rows.iter().flatten().all(|c| c.blocks.iter().all(|b| matches!(b, Block::Para(p) if p.runs.iter().all(|r| r.style.background.is_none()))))
    );

    // What a cell holds stands in the middle unless the cell or its row says otherwise.
    let t = table(
        "<table><tr><td>a<td valign=top>b<td valign=BOTTOM>c<td style=\"vertical-align:top\">d</tr>\
         <tr valign=top><td>e<td valign=middle>f<td valign=sideways>g</table>",
    );
    let places: Vec<Vec<VAlign>> = t.rows.iter().map(|r| r.iter().map(|c| c.valign).collect()).collect();
    assert_eq!(places[0], [VAlign::Middle, VAlign::Top, VAlign::Bottom, VAlign::Top]);
    assert_eq!(places[1], [VAlign::Top, VAlign::Middle, VAlign::Top]);

    // Widths as shares, of the room and of the table.
    let t = table("<table style=\"width:50%\"><tr><td width=\"25%\">a<td width=\"75%\">b<td width=\"900%\">c<td width=\"0%\">d</table>");
    assert_eq!(t.width, TableWidth::Percent(50));
    assert_eq!(t.rows[0].iter().map(|c| (c.share, c.width)).collect::<Vec<_>>(), [(25, 0), (75, 0), (100, 0), (0, 0)]);
    let t = table("<style>table.wide { width:400px }</style><table class=wide><tr><td>a<td>b</table>");
    assert_eq!(t.width, TableWidth::Pixels(400));
}

#[test]
fn hostile_style_sheets_are_read_without_harm() {
    let sheets = [
        "p {".repeat(5000),
        "@media x {".repeat(5000),
        "}".repeat(5000) + "p{color:red}",
        ".a".repeat(5000) + "{color:red}",
        (0..3000).map(|i| format!("p.c{i}, #i{i}, p {{ color:#{:06x} }}", i)).collect::<String>(),
        "p { color:red; ".to_string() + &"x:y;".repeat(100_000) + "}",
        "/* never closed p { color:red }".to_string(),
        "\u{e9}\u{e9}.\u{e9}#\u{e9} { color:red } .\u{202e} {} # {} . {} : {} ,,, {}".to_string(),
        "a:link:hover, a::before, :link, p:LINK { color:red }".to_string(),
    ];
    for sheet in &sheets {
        let doc = parse(&format!("<style>{sheet}</style><p class=\"c1 a\" id=i1>text</p><table><tr class=c2><td id=i2>a<td class=a>b</table>"));
        assert!(doc.text().contains("text"), "{:?}", doc.text());
    }
    // A sheet that is never closed takes the rest of the message with it, as a browser has it.
    assert!(parse("<p>before</p><style>p { color:red } <p>after</p>").text().contains("before"));
    // Sheets past the size limit are not read.
    let big = format!("<style>{}</style><style>p {{ color:red }}</style><p>x</p>", " ".repeat(600 << 10));
    assert_eq!(paras(&parse(&big))[0].runs[0].style.color, None);
}

#[test]
fn pictures_written_into_the_body_are_kept_as_bytes_once_each() {
    let doc = parse(
        "<p><img src=\"data:image/png;base64,AQID\"><img src=\"DATA:image/png;charset=x;BASE64, AQ\r\n ID \"><img src=\"data:image/gif;base64,BAUG-_8=\">\
         <img src=\"data:image/png,AQID\"><img src=\"data:image/png;base64,AQ*D\"><img src=\"data:image/png;base64,\"><img src=\"data:;base64\">\
         <img src=\"data:image/png;base64,====\"></p>",
    );
    let kept: Vec<Option<usize>> =
        paras(&doc)[0].runs.iter().filter_map(|r| if let Some(Picture::Inline(i)) = r.picture { Some(i) } else { None }).collect();
    assert_eq!(kept, [Some(0), Some(0), Some(1), None, None, None, None, None], "the same bytes are kept once; what is not base64 is not kept");
    assert_eq!(doc.written.iter().map(|w| w.to_vec()).collect::<Vec<_>>(), [vec![1, 2, 3], vec![4, 5, 6, 0xfb, 0xff]]);
    assert_eq!(doc.remote_blocked, 0, "nothing was asked of anyone");
}

#[test]
fn hostile_written_pictures_are_capped() {
    // More pictures than one body may keep: each differs, and the rest are placeholders.
    let many: String =
        (0..200usize).map(|i| format!("<img src=\"data:image/png;base64,{}\">", ["AAAA", "AAAB", "AAAC", "AAAD"][i % 4].repeat(1 + i / 4))).collect();
    let doc = parse(&many);
    assert_eq!(doc.written.len(), 64);
    // An address longer than is read is cut short, and one body keeps only so many bytes.
    let long = format!("<img src=\"data:image/png;base64,{}\">", "QUJD".repeat(1 << 20));
    let doc = parse(&long);
    assert!(doc.written.iter().map(|w| w.len()).sum::<usize>() <= 3 << 20);
    let heavy: String =
        (0..8u8).map(|i| format!("<img src=\"data:image/png;base64,{}{}\">", char::from(b'A' + i), "A".repeat((3 << 20) - 64))).collect();
    let doc = parse(&heavy);
    assert!(doc.written.iter().map(|w| w.len()).sum::<usize>() <= 12 << 20);
    for src in
        ["data:", "data:,", "data:;base64,", "data:image/png;base64", "data:\u{0};base64,\u{202e}", &format!("data:{};base64,AAAA", "x".repeat(4000))]
    {
        let doc = parse(&format!("<img src=\"{src}\" alt=a>"));
        assert!(doc.written.is_empty(), "{src:?}");
    }
}

#[test]
fn pictures_stand_at_a_side_only_when_align_says_so() {
    let doc = parse(
        "<p><img src=cid:a align=left><img src=cid:b align=\" RIGHT \"><img src=cid:c align=middle><img src=cid:d style=\"float:left\">\
         <img src=cid:e align=\"leftward\"><img src=cid:f>text</p>",
    );
    let floats: Vec<Float> = paras(&doc)[0].runs.iter().filter(|r| r.picture.is_some()).map(|r| r.float).collect();
    assert_eq!(floats, [Float::Left, Float::Right, Float::None, Float::None, Float::None, Float::None]);
}

#[test]
fn fonts_are_kept_by_name_as_the_body_lists_them() {
    let doc = parse(
        "<style>p.note { font-family: Verdana, sans-serif } #code { font-family: 'Courier New' }</style>\
         <p><font face=\"Arial\">a</font><span style=\"font-family: 'Nosuch Font', Georgia , serif\">b</span>c\
         <span style=\"font-family:inherit\">d</span><font face=\"\">e</font></p><p class=note>f<b>g</b></p><p id=code>h</p>",
    );
    let p = paras(&doc);
    let family =
        |para: &Para, text: &str| para.runs.iter().find(|r| r.text.contains(text)).and_then(|r| r.style.family.as_deref().map(str::to_lowercase));
    assert_eq!(family(p[0], "a").as_deref(), Some("arial"));
    assert_eq!(family(p[0], "b").as_deref(), Some("'nosuch font', georgia , serif"));
    assert_eq!(family(p[0], "c"), None);
    assert_eq!(family(p[0], "d"), None);
    assert_eq!(family(p[0], "e"), None);
    assert_eq!(family(p[1], "f").as_deref(), Some("verdana, sans-serif"));
    assert_eq!(family(p[1], "g").as_deref(), Some("verdana, sans-serif"), "bold text keeps the paragraph's font");
    assert!(p[2].runs[0].style.mono && p[2].runs[0].style.family.is_some());

    // A name is hostile text: it is kept on one line and no longer than a list may be.
    let hostile = format!("<p style=\"font-family:{}\">x</p><font face=\"a\u{0}b\nc\u{1b}[31m\">y</font>", "N".repeat(100_000));
    let doc = parse(&hostile);
    for run in paras(&doc).iter().flat_map(|p| &p.runs) {
        let name = run.style.family.as_deref().unwrap();
        assert!(name.chars().count() <= 160 && !name.chars().any(char::is_control), "{name:?}");
    }
}

#[test]
fn rtf_text_has_the_fonts_its_font_table_names() {
    let doc = crate::rtf::parse(RTF);
    let p = paras(&doc);
    let family = |para: &Para, text: &str| para.runs.iter().find(|r| r.text == text).and_then(|r| r.style.family.as_deref().map(str::to_string));
    assert_eq!(family(p[0], "Title").as_deref(), Some("Arial"), "text that names no font has the default one");
    assert_eq!(family(p[1], "RP-1").as_deref(), Some("Courier New"));
    assert_eq!(family(p[1], "it").as_deref(), Some("Arial"));

    // Other names of a font and its data are not its name; a font the table lacks is no font.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi{\\fonttbl{\\f0\\froman{\\*\\panose 02020603050405020304}{\\*\\falt Other}Georgia;}{\\f1 {\\fontemb 0102}Verdana;extra}{\\f2 ;}}\
          \\pard\\f0 a\\f1 b\\f2 c\\f9 d\\f-3 e\\par}",
    );
    let fonts: Vec<(&str, Option<&str>)> = paras(&doc)[0].runs.iter().map(|r| (r.text.as_str(), r.style.family.as_deref())).collect();
    assert_eq!(fonts, [("a", Some("Georgia")), ("b", Some("Verdana")), ("cde", None)]);

    // A font table without end, names without end, a default that is not there.
    let mut table = b"{\\rtf1\\deff2147483647{\\fonttbl".to_vec();
    for i in 0..5000 {
        table.extend_from_slice(format!("{{\\f{i} {}\u{1};}}", "n".repeat(300)).as_bytes());
    }
    table.extend_from_slice(b"}\\pard\\f4999 x\\f3 y\\par}");
    let doc = crate::rtf::parse(&table);
    assert_eq!(doc.text(), "xy");
    for run in paras(&doc).iter().flat_map(|p| &p.runs) {
        assert!(run.style.family.as_deref().is_none_or(|n| n.chars().count() <= 64 && !n.chars().any(char::is_control)));
    }
}
