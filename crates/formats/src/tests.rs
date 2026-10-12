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
        "<p onclick=\"evil()\">Text<script>alert('x')</script><style>p{}</style><iframe src=\"https://x.example/\">within</iframe>\
         <object data=x>embedded</object><img src=\"https://t.example/pixel.gif\" width=1 height=1><img src=\"https://x.example/a.png\" alt=\"Logo\">\
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
    for gone in ["evil", "alert", "within", "embedded", "hidden", "comment", "AAAA"] {
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
    // Only the line the row asks for: above its first cell.
    assert!(!table.border && table.snug);
    assert_eq!(table.rows[0][0].edges.map(|e| e.map(|e| e.width)), Some([1, 0, 0, 0]));
    assert!(table.rows[0][1].edges.is_none() && table.rows[1][0].edges.is_none());
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

fn tables(doc: &Doc) -> Vec<&crate::rich::Table> {
    doc.blocks.iter().filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).collect()
}

#[test]
fn cells_that_reach_down_push_the_cells_under_them_aside() {
    use crate::rich::any_edge;
    // HTML 4.01 section 11.2.6.1: a cell stands in the first column no cell from above covers.
    let doc = parse(
        "<table border=1><tr><td rowspan=2>a<td>b<td>c<tr><td>d<td rowspan=2>e<tr><td>f<td>g</table>\
         <table border=1><tr><td rowspan=3 colspan=2>a<td>b<tr><td>c<tr><td>d<tr><td>e<td>f<td>g</table>",
    );
    let t = tables(&doc);
    assert_eq!(t[0].places(), [vec![0, 1, 2], vec![1, 2], vec![0, 1]]);
    assert_eq!(t[0].columns(), 3);
    assert_eq!(t[0].rows[0][0].rows, 2);
    assert_eq!(t[1].places(), [vec![0, 2], vec![2], vec![2], vec![0, 1, 2]]);
    assert_eq!(t[1].columns(), 3);
    assert!(t[0].border && any_edge(&t[0].frame), "a border attribute frames the table");

    // Spans without end, of nothing, and of what is not a number: every cell still has a place
    // in a table of bounded width.
    let mut html = String::from("<table><tr><td rowspan=0>a<td rowspan=99999999999 colspan=60000>b<td rowspan=-4>c<td rowspan=x>d");
    for _ in 0..300 {
        html.push_str("<tr><td rowspan=1000 colspan=900>e<td>f");
    }
    html.push_str("</table>");
    let doc = parse(&html);
    for table in tables(&doc) {
        let places = table.places();
        assert_eq!(places.len(), table.rows.len());
        for (row, starts) in table.rows.iter().zip(&places) {
            assert_eq!(row.len(), starts.len());
            assert!(row.iter().all(|c| c.rows <= 1000));
            assert!(starts.windows(2).all(|w| w[0] <= w[1]), "cells keep their order: {starts:?}");
        }
        assert!(table.columns() <= 1024 + 60000);
    }
}

#[test]
fn borders_set_in_styles_are_kept_for_paragraphs_tables_and_cells() {
    use crate::rich::{Line, any_edge};
    let doc = parse(
        "<p style=\"border:2px solid #cc0000\">a</p>\
         <p style=\"border-left:4px solid rgb(31,78,121); padding-left:8px\">b</p>\
         <div style=\"border:1px dashed #2e7d32; padding:10px\">c<p>d</p></div>\
         <p style=\"border-bottom:3px double #000000\">e</p>\
         <p style=\"border:none\">f</p><p style=\"border:5px\">g</p><p>h</p>",
    );
    let p = paras(&doc);
    let frame = |i: usize| p[i].frame.as_deref();
    let a = frame(0).expect("a border");
    assert!(a.edges.iter().all(|e| e.width == 2 && e.color == Some([0xcc, 0, 0]) && e.line == Line::Solid));
    let b = frame(1).expect("a border at the left");
    assert_eq!(b.edges.iter().map(|e| e.width).collect::<Vec<_>>(), [0, 0, 0, 4]);
    assert_eq!(b.edges[3].color, Some([31, 78, 121]));
    // Paragraphs inside one bordered block share its border, so that it is drawn round them once.
    let (c, d) = (p[2].frame.as_ref().expect("c"), p[3].frame.as_ref().expect("d"));
    assert!(std::sync::Arc::ptr_eq(c, d));
    assert_eq!((c.edges[0].line, c.padding), (Line::Dashed, (10, 10)));
    assert!(!std::sync::Arc::ptr_eq(p[0].frame.as_ref().expect("a"), p[1].frame.as_ref().expect("b")));
    let e = frame(4).expect("a line under");
    assert_eq!((e.edges[2].width, e.edges[2].line, e.edges[0].width), (3, Line::Double, 0));
    // No kind of line is no line.
    assert!(frame(5).is_none() && frame(6).is_none() && frame(7).is_none());

    let doc = parse(
        "<div style=\"border:1px solid red\"><table style=\"border:3px solid #cc0000\"><tr><td>a<td>b</table></div>\
         <table><tr><td style=\"border:1px solid #1f4e79\">c<td style=\"border-bottom:2px solid #2e7d32\">d<td>e</table>\
         <table border=1 bordercolor=\"#cc0000\"><tr><td>f</table>\
         <table border=4 cellspacing=0><tr><td>g</table>",
    );
    let t = tables(&doc);
    assert!(t[0].frame.iter().all(|e| e.width == 3 && e.color == Some([0xcc, 0, 0])) && !t[0].border);
    // A border round a table is not a border round each paragraph in it.
    assert!(t[0].rows.iter().flatten().flat_map(|c| &c.blocks).all(|b| !matches!(b, Block::Para(p) if p.frame.is_some())));
    let row = &t[1].rows[0];
    assert!(row[0].edges.is_some_and(|e| e.iter().all(|e| e.width == 1)));
    assert_eq!(row[1].edges.map(|e| [e[0].width, e[1].width, e[2].width, e[3].width]), Some([0, 0, 2, 0]));
    assert!(row[2].edges.is_none() && !any_edge(&t[1].frame));
    assert_eq!(t[2].line, Some([0xcc, 0, 0]));
    assert!(t[3].frame.iter().all(|e| e.width == 4));
}

#[test]
fn a_break_that_clears_starts_below_a_picture_at_the_side() {
    let doc = parse("<p><img src=cid:a align=left>a</p><p>b</p><br clear=all><p>c</p><p style=\"clear:both\">d</p><p>e<br clear=none>f</p>");
    let clear: Vec<(String, bool)> = paras(&doc).iter().map(|p| (p.runs.iter().map(|r| r.text.as_str()).collect::<String>(), p.clear)).collect();
    let of = |text: &str| clear.iter().find(|(t, _)| t.contains(text)).map(|(_, c)| *c);
    assert_eq!((of("a"), of("b"), of("c"), of("d")), (Some(false), Some(false), Some(true), Some(true)));
    assert_eq!(of("e"), Some(false), "a break that clears nothing is a line break: {clear:?}");
}

#[test]
fn hostile_borders_are_capped() {
    let mut html = String::from(
        "<p style=\"border:99999px solid; padding:99999px\">a</p><p style=\"border:-5px dashed #zzz; padding:-1px\">b</p>\
         <p style=\"border:1e39pt double rgb(999,-1,x)\">c</p><table border=99999 bordercolor=nothing style=\"border:NaNpx solid\"><tr><td style=\"border:inf solid\">d</table>\
         <br clear=\"\u{202e}\">",
    );
    html.push_str(&"<div style=\"border:1px solid red;padding:4000px\">".repeat(400));
    html.push_str("deep");
    let doc = parse(&html);
    fn check(blocks: &[Block], depth: usize) {
        assert!(depth < 64);
        for block in blocks {
            match block {
                Block::Para(p) => {
                    if let Some(frame) = &p.frame {
                        assert!(frame.edges.iter().all(|e| e.width <= 12), "{:?}", frame.edges);
                    }
                }
                Block::Table(t) => {
                    assert!(t.frame.iter().all(|e| e.width <= 12));
                    for cell in t.rows.iter().flatten() {
                        assert!(cell.edges.is_none_or(|e| e.iter().all(|e| e.width <= 12)));
                        check(&cell.blocks, depth + 1);
                    }
                }
                Block::Rule => {}
            }
        }
    }
    check(&doc.blocks, 0);
    assert!(doc.text().contains("deep"));
}

/// A picture file's first bytes as RTF writes them: two hex digits to the byte.
fn hexed(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn rtf_pictures_are_kept_as_bytes_at_the_size_they_ask_for() {
    let png = [0x89u8, b'P', b'N', b'G', 13, 10, 26, 10, 1, 2, 3];
    let hex = hexed(&png);
    let (head, tail) = hex.split_at(7);
    let rtf = format!(
        "{{\\rtf1\\ansi\\pard a{{\\pict\\pngblip\\picw96\\pich64 {head}\r\n{tail}}}\
         b{{\\*\\shppict{{\\pict{{\\*\\picprop{{\\sp{{\\sn x}}{{\\sv 4142}}}}}}\\jpegblip\\picwgoal2880\\pichgoal960\\picscalex50\\picscaley200 ffd8ff}}}}\
         {{\\nonshppict{{\\pict\\wmetafile8 0102}}}}\
         c{{\\pict\\wmetafile8\\picwgoal1500\\pichgoal750 010203}}d{{\\pict\\pngblip {hex}}}{{\\v{{\\pict\\pngblip {hex}}}}}\\par}}"
    );
    let doc = crate::rtf::parse(rtf.as_bytes());
    assert_eq!(doc.text(), "abcd");
    let runs: Vec<_> = paras(&doc)[0].runs.iter().filter_map(|r| r.picture.clone().map(|p| (p, r.extent))).collect();
    assert_eq!(
        runs,
        [
            // At its own size: the size in pixels a picture file says it has is the file's to say.
            (Picture::Inline(Some(0)), (0, 0)),
            // Twips to pixels, scaled as asked.
            (Picture::Inline(Some(1)), (96, 128)),
            // A metafile is not read: a place of the size asked for.
            (Picture::Inline(None), (100, 50)),
            // The same picture again is kept once.
            (Picture::Inline(Some(0)), (0, 0)),
        ]
    );
    assert_eq!(doc.written.len(), 2);
    assert_eq!(&*doc.written[0], &png[..]);
    assert_eq!(&*doc.written[1], &[0xff, 0xd8, 0xff][..]);

    // Data that is not hex digits, half a byte, raw bytes: a place, and no bytes kept.
    for data in ["89504e4x", "89504", "\\bin4 abcd", ""] {
        let doc = crate::rtf::parse(format!("{{\\rtf1 a{{\\pict\\pngblip {data}}}b\\par}}").as_bytes());
        assert!(doc.written.is_empty(), "{data}");
        assert_eq!(paras(&doc)[0].runs.iter().filter_map(|r| r.picture.clone()).collect::<Vec<_>>(), [Picture::Inline(None)], "{data}");
    }
}

/// A bitmap of two by two pixels, three bytes to the pixel, without a file's header. The first
/// pixel kept (the lower left one) is blue.
fn small_dib() -> Vec<u8> {
    let mut dib = Vec::new();
    for n in [40u32, 2, 2] {
        dib.extend_from_slice(&n.to_le_bytes());
    }
    dib.extend_from_slice(&[1, 0, 24, 0]);
    for n in [0u32, 16, 0, 0, 0, 0] {
        dib.extend_from_slice(&n.to_le_bytes());
    }
    dib.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0]);
    dib
}

/// A Windows metafile of these records (what each does, and what follows that).
fn wmf(records: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut file = vec![1, 0, 9, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    for (function, body) in records.iter().chain(&[(0u16, Vec::new())]) {
        file.extend_from_slice(&u32::try_from((6 + body.len()) / 2).unwrap().to_le_bytes());
        file.extend_from_slice(&function.to_le_bytes());
        file.extend_from_slice(body);
    }
    file
}

/// The record that draws `dib` stretched (META_STRETCHDIB), copied as it is.
fn stretch(dib: &[u8]) -> (u16, Vec<u8>) {
    let mut body = vec![0x20, 0, 0xCC, 0, 0, 0];
    body.extend_from_slice(&[0; 16]);
    body.extend_from_slice(dib);
    (0x0F43, body)
}

/// An enhanced metafile that draws `dib` (EMR_STRETCHDIBITS) and nothing else.
fn emf(dib: &[u8]) -> Vec<u8> {
    let mut file = vec![0u8; 88];
    file[0] = 1;
    file[4] = 88;
    file[40..44].copy_from_slice(b" EMF");
    let (header, pixels) = dib.split_at(40);
    let mut record = vec![0u8; 80];
    let size = u32::try_from(80 + dib.len()).unwrap();
    for (at, n) in [(0, 81), (4, size), (48, 80), (52, 40), (56, 120), (60, u32::try_from(pixels.len()).unwrap()), (68, 0x00CC_0020)] {
        record[at..at + 4].copy_from_slice(&n.to_le_bytes());
    }
    file.extend_from_slice(&record);
    file.extend_from_slice(header);
    file.extend_from_slice(pixels);
    file.extend_from_slice(&[14, 0, 0, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20, 0, 0, 0]);
    file
}

#[test]
fn a_bitmap_and_a_metafile_that_holds_only_one_become_a_picture_file() {
    use crate::metafile::{dib_file, emf_bitmap, wmf_bitmap};
    let dib = small_dib();
    let file = dib_file(&dib).unwrap();
    assert_eq!(file.len(), 14 + dib.len());
    // "BM", the file's length, and where its pixels begin: after both headers.
    assert_eq!(&file[..14], &[b'B', b'M', 70, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0]);
    assert_eq!(&file[14..], &dib[..]);
    // Eight bits to the pixel: a table of 256 colours stands before the pixels.
    let mut indexed = dib.clone();
    indexed[14] = 8;
    indexed.resize(40 + 1024 + 8, 0);
    assert_eq!(dib_file(&indexed).unwrap()[10..14], (14u32 + 40 + 1024).to_le_bytes());

    // A metafile that sets things up and draws the one bitmap, placeable or not.
    let plain = wmf(&[(0x0103, vec![8, 0]), stretch(&dib)]);
    assert_eq!(wmf_bitmap(&plain), Some(file.clone()));
    let mut placeable = vec![0xD7, 0xCD, 0xC6, 0x9A];
    placeable.resize(22, 0);
    placeable.extend_from_slice(&plain);
    assert_eq!(wmf_bitmap(&placeable), Some(file.clone()));
    assert_eq!(emf_bitmap(&emf(&dib)), Some(file));

    // Anything else drawn (an ellipse), a second bitmap, a bitmap that is not copied as it is,
    // or no bitmap at all: not read.
    assert_eq!(wmf_bitmap(&wmf(&[stretch(&dib), (0x0418, vec![0; 8])])), None);
    assert_eq!(wmf_bitmap(&wmf(&[stretch(&dib), stretch(&dib)])), None);
    let mut inverted = stretch(&dib);
    inverted.1[0] = 0x21;
    assert_eq!(wmf_bitmap(&wmf(&[inverted])), None);
    assert_eq!(wmf_bitmap(&wmf(&[(0x0103, vec![8, 0])])), None);
}

#[test]
fn hostile_bitmaps_and_metafiles_are_not_read() {
    use crate::metafile::{dib_file, emf_bitmap, wmf_bitmap};
    let dib = small_dib();
    let files = [dib.clone(), wmf(&[stretch(&dib)]), emf(&dib)];
    for file in &files {
        // Cut short anywhere, and with any one byte turned over: no picture, or one that is
        // all there.
        for end in 0..file.len() {
            let _ = (dib_file(&file[..end]), wmf_bitmap(&file[..end]), emf_bitmap(&file[..end]));
            let mut changed = file.clone();
            changed[end] ^= 0xFF;
            for read in [dib_file(&changed), wmf_bitmap(&changed), emf_bitmap(&changed)].into_iter().flatten() {
                assert!(read.len() <= file.len() + 14);
            }
        }
    }
    assert_eq!(wmf_bitmap(&wmf(&[stretch(&dib)])[..40]), None);
    // A record of no length would never end; sizes beyond the data; a header that says its
    // colours fill more than there is.
    let mut endless = wmf(&[]);
    endless[18] = 0;
    assert_eq!(wmf_bitmap(&endless), None);
    let mut far = emf(&dib);
    far[88 + 48..88 + 52].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(emf_bitmap(&far), None);
    let mut greedy = dib.clone();
    greedy[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(dib_file(&greedy), None);
    assert_eq!(dib_file(&[0xFF; 64]), None);
}

#[test]
fn rtf_keeps_bitmaps_metafile_bitmaps_and_the_places_of_attachments() {
    let dib = small_dib();
    let rtf = format!(
        "{{\\rtf1\\ansi\\pard a{{\\pict\\dibitmap0\\picwgoal300\\pichgoal300 {}}}b{{\\pict\\wmetafile8 {}}}c{{\\pict\\emfblip {}}}\
         d{{\\pict\\wbitmap0 {}}}\\par e\\objattph\\'20f\\objattph\\par}}",
        hexed(&dib),
        hexed(&wmf(&[stretch(&dib)])),
        hexed(&emf(&dib)),
        hexed(&dib)
    );
    let doc = crate::rtf::parse(rtf.as_bytes());
    assert_eq!(doc.text(), "abcd\ne f");
    let pictures: Vec<_> = paras(&doc).iter().flat_map(|p| &p.runs).filter_map(|r| r.picture.clone().map(|p| (p, r.extent))).collect();
    assert_eq!(
        pictures,
        [
            (Picture::Inline(Some(0)), (20, 20)),
            // The same bitmap out of a metafile is the same picture, kept once.
            (Picture::Inline(Some(0)), (0, 0)),
            (Picture::Inline(Some(0)), (0, 0)),
            // A bitmap made for one device is not read.
            (Picture::Inline(None), (0, 0)),
            (Picture::Object(0), (0, 0)),
            (Picture::Object(1), (0, 0)),
        ]
    );
    assert_eq!(doc.written.len(), 1);
    assert_eq!(crate::metafile::dib_file(&dib).as_deref(), Some(&*doc.written[0]));
    assert_eq!(doc.objects, 2);

    // More places than are kept: the runs stop, the count goes on.
    let many = format!("{{\\rtf1 {}\\par}}", "\\objattph ".repeat(1000));
    let doc = crate::rtf::parse(many.as_bytes());
    assert_eq!(doc.objects, 1000);
    assert_eq!(paras(&doc).iter().flat_map(|p| &p.runs).filter(|r| r.picture.is_some()).count(), 256);
}

#[test]
fn a_large_rtf_picture_does_not_cut_the_text_after_it_short() {
    // More hex digits than the cap on text: they are data, and the text after them is read.
    let mut rtf = String::from("{\\rtf1\\ansi\\pard before{\\pict\\pngblip ");
    rtf.push_str(&"89504e47".repeat(400_000));
    rtf.push_str("}{\\*\\themedata ");
    rtf.push_str(&"504b0304".repeat(300_000));
    rtf.push_str("} after\\par}");
    let doc = crate::rtf::parse(rtf.as_bytes());
    assert!(!doc.truncated);
    assert_eq!(doc.text(), "before after");
    assert_eq!(doc.written.first().map(|p| p.len()), Some(1_600_000));
}

#[test]
fn rtf_cells_keep_their_lines_fills_places_and_merges() {
    use crate::rich::{Line, VAlign};
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi{\\colortbl;\\red204\\green0\\blue0;\\red255\\green230\\blue153;}\
          \\trowd\\trgaph108\\clbrdrt\\brdrs\\brdrw60\\clbrdrl\\brdrs\\clbrdrb\\brdrdb\\brdrw15\\clbrdrr\\brdrdash\\brdrw15\\brdrcf1\\clcbpat2\\clvertalb\\cellx2000\
          \\clbrdrl\\brdrdot\\clbrdrt\\brdrnone\\clbrdrb\\brdrs\\brdrw9999\\clvertalc\\cellx4000\\intbl a\\cell b\\cell\\row\
          \\trowd\\clbrdrt\\brdrs\\clbrdrb\\brdrs\\clvmgf\\cellx2000\\clbrdrt\\brdrs\\cellx4000\\intbl c\\cell d\\cell\\row\
          \\trowd\\clbrdrb\\brdrs\\brdrw30\\clvmrg\\cellx2000\\cellx4000\\intbl\\cell e\\cell\\row\
          \\trowd\\clvmrg\\cellx2000\\clvmrg\\cellx4000\\intbl f\\cell g\\cell\\row\\pard\\brdrb\\brdrs\\brdrw90 after\\par}",
    );
    let t = tables(&doc)[0];
    let a = &t.rows[0][0];
    let edges = a.edges.expect("lines");
    assert_eq!(edges.map(|e| e.width), [4, 1, 3, 1], "twips to pixels; a double line is three wide");
    assert_eq!(edges.map(|e| e.line), [Line::Solid, Line::Dashed, Line::Double, Line::Solid]);
    assert_eq!((edges[1].color, edges[0].color), (Some([204, 0, 0]), None));
    assert_eq!((a.background, a.valign, a.padding), (Some([255, 230, 153]), VAlign::Bottom, Some((1, 7))));
    // The line between two cells is drawn once, and no line is no line; widths are capped.
    let b = &t.rows[0][1];
    assert_eq!(b.edges.map(|e| e.map(|e| e.width)), Some([0, 0, 12, 0]));
    assert_eq!((b.valign, b.padding), (VAlign::Middle, Some((1, 7))));
    // A line under every cell of a row is the line above the next one.
    assert_eq!(t.rows[1][0].edges.map(|e| e.map(|e| e.width)), Some([0, 0, 2, 0]), "top shared; bottom is the last part's");
    assert_eq!(t.rows[1][1].edges, None, "its only line was the one it shares: {:?}", t.rows[1][1].edges);
    // The merged cell covers three rows and the cells under it are gone; a cell that continues
    // nothing is a cell of its own.
    assert_eq!(t.rows[1][0].rows, 3);
    assert_eq!(t.rows.iter().map(Vec::len).collect::<Vec<_>>(), [2, 2, 1, 1]);
    assert_eq!(t.places(), [vec![0, 1], vec![0, 1], vec![1], vec![1]]);
    assert_eq!(doc.text().lines().last(), Some("after"));
    // A paragraph's border is not the border of the cell defined last.
    assert!(t.rows[3][0].edges.is_none());
}

#[test]
fn hostile_rtf_pictures_and_tables_are_capped() {
    let mut rtf = String::from("{\\rtf1\\ansi{\\colortbl;\\red1\\green2\\blue3;}");
    // More pictures than are kept, each different, with sizes and scales out of range.
    for i in 0..300 {
        rtf.push_str(&format!("{{\\pict\\pngblip\\picwgoal2147483647\\pichgoal-5\\picscalex2147483647\\picscaley-2147483648 89504e47{i:08x}}}"));
    }
    // Rows of cells merged down from nothing, without end, with borders of every width.
    for i in 0..2000 {
        rtf.push_str("\\trowd\\trgaph2147483647");
        for c in 0..80 {
            rtf.push_str(&format!("\\clbrdrt\\brdrdb\\brdrw{}\\brdrcf9999\\clbrdrl\\brdrw-7\\clvmrg\\clcbpat-1\\cellx{}", i * 97, c * 100 - 300));
        }
        rtf.push_str("\\intbl x\\cell y\\cell\\row");
    }
    rtf.push_str("\\trowd\\clvmgf\\cellx-2147483648\\clvmgf\\cellx2147483647\\intbl a\\cell b\\cell\\row{\\pict\\pngblip 8950");
    let doc = crate::rtf::parse(rtf.as_bytes());
    assert!(doc.written.len() <= 64);
    fn check(blocks: &[Block]) {
        for block in blocks {
            match block {
                Block::Para(p) => assert!(p.runs.iter().all(|r| r.extent.0 <= 4096 && r.extent.1 <= 4096)),
                Block::Table(t) => {
                    assert_eq!(t.places().len(), t.rows.len());
                    for cell in t.rows.iter().flatten() {
                        assert!(cell.rows <= 1000 && cell.width <= 4096);
                        assert!(cell.edges.is_none_or(|e| e.iter().all(|e| e.width <= 12)));
                        assert!(cell.padding.is_some_and(|(v, h)| v <= 64 && h <= 64));
                        check(&cell.blocks);
                    }
                }
                Block::Rule => {}
            }
        }
    }
    check(&doc.blocks);
    // One cell merged down a thousand rows and more stays within the cap.
    let mut rtf = String::from("{\\rtf1\\trowd\\clvmgf\\cellx1000\\cellx2000\\intbl a\\cell b\\cell\\row");
    rtf.push_str(&"\\trowd\\clvmrg\\cellx1000\\cellx2000\\intbl\\cell c\\cell\\row".repeat(1500));
    let doc = crate::rtf::parse(rtf.as_bytes());
    assert!(tables(&doc).iter().all(|t| t.rows.iter().flatten().all(|c| c.rows <= 1000)));
}

#[test]
fn a_link_that_is_not_what_its_text_says_is_told_apart() {
    use crate::links::{Concern, Place, concern, concern_at, place, text_around};
    let at = |link: &str| place(link).map(|p| p.host);
    for (link, host) in [
        ("https://Example.ORG/path?x=1#y", Some("example.org")),
        ("http://www.example.org.", Some("example.org")),
        ("https://example.org:8443/a@b", Some("example.org")),
        ("https://user:secret@example.org/", Some("example.org")),
        ("https://[2001:db8::1]:443/", Some("2001:db8::1")),
        ("https:\\\\example.org\\path", Some("example.org")),
        ("ht\ttps://exa\nmple.org/", Some("example.org")),
        ("mailto:Ana.Costa@Example.org?subject=Hi", Some("example.org")),
        ("ftp://files.example.net", Some("files.example.net")),
        ("#top", None),
        ("page.html", None),
        ("tel:+15550100", None),
        ("cid:logo@example.org", None),
        ("https://", None),
        ("https:///path", None),
        ("mailto:nobody", None),
        ("", None),
        (":", None),
        ("1http://example.org", None),
    ] {
        assert_eq!(at(link).as_deref(), host, "{link:?}");
    }
    assert_eq!(place("https://bank.example@evil.test/"), Some(Place { scheme: "https".into(), host: "evil.test".into(), disguised: true }));

    let elsewhere = |says: &str, goes: &str| Some(Concern::Elsewhere { says: says.into(), goes: goes.into() });
    for (text, link, want) in [
        // What it seems.
        ("our help pages", "https://parking.example.net/help", None),
        ("parking.example.net/help", "https://parking.example.net/help", None),
        ("www.example.org", "https://example.org/welcome", None),
        ("example.org", "https://login.example.org/", None),
        ("https://login.example.org/start", "https://example.org/", None),
        ("See https://example.org/a.", "http://EXAMPLE.org/b", None),
        ("ana@example.org", "mailto:ana@example.org", None),
        ("Ana Costa", "mailto:ana@example.org", None),
        ("report.pdf", "https://files.example.net/report.pdf", None),
        ("Read more at once.", "https://news.example.net/", None),
        ("v1.2", "https://example.org/releases", None),
        ("ana@example.org", "https://example.net/people/ana", None),
        ("here", "#section", None),
        ("call us", "tel:+15550100", None),
        // Not what it seems.
        ("https://bank.example/login", "https://bank.example.evil.test/login", elsewhere("bank.example", "bank.example.evil.test")),
        ("www.bank.example", "http://evil.test/bank.example", elsewhere("bank.example", "evil.test")),
        ("bank.example", "https://evil.test/", elsewhere("bank.example", "evil.test")),
        ("Sign in at https://bank.example today", "https://notbank.example/", elsewhere("bank.example", "notbank.example")),
        ("help@bank.example", "mailto:help@evil.test", elsewhere("bank.example", "evil.test")),
        ("bank.example", "https://bank.example@evil.test/", Some(Concern::Disguised { goes: "evil.test".into() })),
        ("your account", "https://xn--bnk-5ma.example/", Some(Concern::OtherLetters { goes: "xn--bnk-5ma.example".into() })),
        ("your account", "https://b\u{430}nk.example/", Some(Concern::OtherLetters { goes: "b\u{430}nk.example".into() })),
        ("your account", "http://192.0.2.7/login", Some(Concern::Number { goes: "192.0.2.7".into() })),
        ("your account", "http://0xC0.0.2.7/", Some(Concern::Number { goes: "0xc0.0.2.7".into() })),
        ("your account", "http://3221225991/", Some(Concern::Number { goes: "3221225991".into() })),
        ("your account", "http://[2001:db8::1]/", Some(Concern::Number { goes: "2001:db8::1".into() })),
        ("abc.de", "https://abc.de/", None),
        ("click", "javascript:alert(1)", Some(Concern::NotAnAddress { scheme: "javascript".into() })),
        ("click", " Java\tScript:alert(1)", Some(Concern::NotAnAddress { scheme: "javascript".into() })),
        ("the file", "file:///C:/Windows/system.ini", Some(Concern::NotAnAddress { scheme: "file".into() })),
        ("open", "ms-word:ofe|u|https://example.org/x.docx", Some(Concern::NotAnAddress { scheme: "ms-word".into() })),
    ] {
        assert_eq!(concern(text, link), want, "{text:?} -> {link:?}");
    }
    assert_eq!(elsewhere("a.example", "b.example").unwrap().words(), "The text names a.example, but the link goes to b.example.");

    // A link's text is all the runs that go to the same address, however they are styled.
    let doc = parse(r#"<p>Go to <a href="https://evil.test/">www.<b>bank</b>.example</a> or <a href="https://bank.example/">bank.example</a>.</p>"#);
    let Some(Block::Para(para)) = doc.blocks.first() else { panic!("a paragraph") };
    let told: Vec<(String, bool)> = (0..para.runs.len()).map(|i| (text_around(&para.runs, i), concern_at(&para.runs, i).is_some())).collect();
    let links: Vec<&(String, bool)> = told.iter().filter(|(text, _)| !text.is_empty()).collect();
    assert_eq!(links.len(), 4, "three runs of the first link and one of the second");
    assert!(links[..3].iter().all(|l| **l == ("www.bank.example".to_string(), true)));
    assert_eq!(*links[3], ("bank.example".to_string(), false));

    // Hostile: nothing here panics, whatever stands in an address or a text.
    let pieces = [
        "", ":", "//", "@", "[", "]", "\\", "?", "#", ".", "..", "xn--", "http", "https://", "mailto:", "é", "\u{0}", "\u{202e}", "www.", "a.b", "0x",
    ];
    for a in pieces {
        for b in pieces {
            for c in pieces {
                let joined = format!("{a}{b}{c}");
                let _ = concern(&joined, &joined);
                let _ = concern(c, &joined);
                let _ = place(&joined);
            }
        }
    }
    let long = "a.".repeat(100_000);
    let _ = concern(&long, &format!("https://{long}@{long}/"));
    assert_eq!(text_around(&[], 3), "");
    assert_eq!(concern_at(&[], 0), None);
}

/// The words of a record's parameters.
fn words(list: &[i16]) -> Vec<u8> {
    list.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// The colour of one pixel of a BMP file of 24 bits to the pixel.
fn pixel(bmp: &[u8], x: usize, y: usize) -> [u8; 3] {
    let (w, h) = bmp_size(bmp);
    let at = 54 + (h - 1 - y) * ((w * 3 + 3) & !3) + x * 3;
    [bmp[at + 2], bmp[at + 1], bmp[at]]
}

fn bmp_size(bmp: &[u8]) -> (usize, usize) {
    let n = |at: usize| u32::from_le_bytes(bmp[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!((&bmp[..2], n(2), n(10)), (&b"BM"[..], bmp.len(), 54));
    (n(18), n(22))
}

/// Records that set a window of a hundred units each way, and a red brush.
fn red_window() -> Vec<(u16, Vec<u8>)> {
    vec![(0x020B, words(&[0, 0])), (0x020C, words(&[100, 100])), (0x02FC, vec![0, 0, 255, 0, 0, 0, 0, 0]), (0x012D, words(&[0]))]
}

const RED: [u8; 3] = [255, 0, 0];
const WHITE: [u8; 3] = [255; 3];

#[test]
fn a_metafile_that_is_a_drawing_is_drawn() {
    use crate::metafile::wmf_picture;
    // A red box with the pen's black line round it, drawn at twice the size it is shown at.
    let mut records = red_window();
    records.push((0x041B, words(&[80, 80, 20, 20])));
    let bmp = wmf_picture(&wmf(&records), Some((50, 50))).unwrap();
    assert_eq!(bmp_size(&bmp), (100, 100));
    assert_eq!(pixel(&bmp, 50, 50), RED);
    assert_eq!(pixel(&bmp, 5, 5), WHITE);
    assert_eq!(pixel(&bmp, 90, 50), WHITE);
    let edge = pixel(&bmp, 20, 50);
    assert!(edge != RED && edge != WHITE && edge[1] == 0, "{edge:?}");

    // No size asked for: a placeable metafile says its own (an inch by half an inch).
    let mut placeable = vec![0xD7, 0xCD, 0xC6, 0x9A, 0, 0];
    placeable.extend(words(&[0, 0, 1440, 720, 1440]));
    placeable.resize(22, 0);
    placeable.extend(wmf(&[(0x0418, words(&[720, 1440, 0, 0]))]));
    let bmp = wmf_picture(&placeable, None).unwrap();
    assert_eq!(bmp_size(&bmp), (96, 48));
    // The ellipse is filled with the white brush and lined in black: its ends are dark.
    assert_eq!(pixel(&bmp, 48, 24), WHITE);
    assert!(pixel(&bmp, 0, 24)[0] < 200 && pixel(&bmp, 95, 24)[0] < 200);
    assert_eq!(pixel(&bmp, 1, 1), WHITE);
    // A plain one that says nothing has its shape, at a size that can be looked at.
    let wide = wmf(&[(0x020C, words(&[50, 200])), (0x0213, words(&[50, 200]))]);
    assert_eq!(bmp_size(&wmf_picture(&wide, None).unwrap()), (600, 150));

    // One square inside another, both drawn the same way round: the inner one is a hole when
    // every other part is filled, and filled when all that is wound round is.
    let nested = |mode: i16| {
        let mut records = red_window();
        records.push((0x0106, words(&[mode])));
        records.push((0x0538, words(&[2, 4, 4, 10, 10, 90, 10, 90, 90, 10, 90, 40, 40, 60, 40, 60, 60, 40, 60])));
        wmf_picture(&wmf(&records), Some((50, 50))).unwrap()
    };
    assert_eq!((pixel(&nested(1), 50, 50), pixel(&nested(1), 25, 50)), (WHITE, RED));
    assert_eq!((pixel(&nested(2), 50, 50), pixel(&nested(2), 25, 50)), (RED, RED));

    // A pen that draws nothing, a pie, and a state that is saved and put back: what is cut
    // off inside it is whole again after.
    let mut records = red_window();
    records.extend([
        (0x02FA, vec![5, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        (0x012D, words(&[1])),
        (0x001E, Vec::new()),
        (0x0416, words(&[50, 50, 0, 0])),
        (0x041B, words(&[100, 100, 0, 0])),
        (0x0127, words(&[-1])),
        (0x081A, words(&[100, 75, 75, 100, 100, 100, 50, 50])),
    ]);
    let bmp = wmf_picture(&wmf(&records), Some((50, 50))).unwrap();
    assert_eq!((pixel(&bmp, 20, 20), pixel(&bmp, 80, 20), pixel(&bmp, 20, 80)), (RED, WHITE, WHITE));
    // The pie runs from the right of its ellipse round to the bottom, against the clock: all
    // of it but the quarter at the lower right.
    assert_eq!((pixel(&bmp, 65, 65), pixel(&bmp, 85, 85)), (RED, WHITE));
    assert_eq!((pixel(&bmp, 65, 85), pixel(&bmp, 85, 65)), (RED, RED));
}

#[test]
fn a_metafile_that_cannot_be_drawn_whole_is_not_drawn() {
    use crate::metafile::wmf_picture;
    let boxed = (0x041B, words(&[80, 80, 20, 20]));
    let with = |extra: (u16, Vec<u8>)| {
        let mut records = red_window();
        records.extend([boxed.clone(), extra]);
        wmf_picture(&wmf(&records), None)
    };
    assert!(with((0x0102, words(&[1]))).is_some());
    // Text, a bitmap beside the drawing, a pen that inverts, a region cut out, a hatched brush
    // that is filled with, a colour that is a number in a palette.
    assert_eq!(with((0x0A32, words(&[10, 10, 1, 0, 0x41]))), None);
    assert_eq!(with(stretch(&small_dib())), None);
    assert_eq!(with((0x0104, words(&[6]))), None);
    assert_eq!(with((0x0415, words(&[50, 50, 0, 0]))), None);
    assert_eq!(
        wmf_picture(&wmf(&[(0x020C, words(&[100, 100])), (0x02FC, vec![2, 0, 255, 0, 0, 0, 0, 0]), (0x012D, words(&[0])), boxed.clone()]), None),
        None
    );
    assert_eq!(wmf_picture(&wmf(&[(0x020C, words(&[100, 100])), (0x02FC, vec![0, 0, 5, 0, 0, 1, 0, 0]), boxed.clone()]), None), None);
    // Nothing drawn, no window to draw in, a window of no size, an object that was never made,
    // a state put back that was never saved.
    assert_eq!(wmf_picture(&wmf(&red_window()), None), None);
    assert_eq!(wmf_picture(&wmf(std::slice::from_ref(&boxed)), None), None);
    assert_eq!(wmf_picture(&wmf(&[(0x020C, words(&[0, 100])), boxed.clone()]), None), None);
    assert_eq!(with((0x012D, words(&[7]))), None);
    assert_eq!(with((0x0127, words(&[-1]))), None);
}

#[test]
fn hostile_drawings_never_panic() {
    use crate::metafile::wmf_picture;
    let mut records = red_window();
    records.extend([
        (0x02FA, vec![0, 0, 9, 0, 0, 0, 0, 0, 255, 0]),
        (0x012D, words(&[1])),
        (0x001E, Vec::new()),
        (0x0416, words(&[90, 90, 10, 10])),
        (0x0410, words(&[1, 2, 1, 2])),
        (0x020F, words(&[5, 5])),
        (0x0214, words(&[10, 10])),
        (0x0213, words(&[90, 90])),
        (0x0325, words(&[3, 0, 0, 50, 50, 100, 0])),
        (0x0324, words(&[3, 0, 0, 50, 50, 100, 0])),
        (0x0538, words(&[1, 3, 0, 0, 50, 50, 100, 0])),
        (0x0418, words(&[80, 80, 20, 20])),
        (0x061C, words(&[20, 20, 80, 80, 20, 20])),
        (0x0817, words(&[0, 50, 50, 0, 100, 100, 0, 0])),
        (0x0830, words(&[0, 50, 50, 0, 100, 100, 0, 0])),
        (0x041F, vec![0, 255, 0, 0, 30, 0, 30, 0]),
        (0x0127, words(&[1])),
        (0x01F0, words(&[0])),
    ]);
    let file = wmf(&records);
    assert!(wmf_picture(&file, Some((40, 40))).is_some());
    // Cut short anywhere, and with any one byte turned over or made the largest it can be.
    for end in 0..file.len() {
        let _ = wmf_picture(&file[..end], Some((24, 24)));
        for with in [file[end] ^ 0xFF, 0xFF, 0x80, 0] {
            let mut changed = file.clone();
            changed[end] = with;
            for bmp in [wmf_picture(&changed, Some((24, 24))), wmf_picture(&changed, Some((u32::MAX, 1)))].into_iter().flatten() {
                let (w, h) = bmp_size(&bmp);
                assert!(w <= 800 && h <= 800);
            }
        }
    }
    // More points than a record holds; more polygons than there are; a great many objects.
    assert_eq!(wmf_picture(&wmf(&[(0x020C, words(&[100, 100])), (0x0324, words(&[-1, 0, 0]))]), None), None);
    assert_eq!(wmf_picture(&wmf(&[(0x020C, words(&[100, 100])), (0x0538, words(&[-1, 3]))]), None), None);
    let many: Vec<_> = (0..5000).map(|_| (0x02FBu16, Vec::new())).collect();
    assert_eq!(wmf_picture(&wmf(&many), None), None);
    // A drawing that would take too long: a great many long lines of the widest pen.
    let mut slow = vec![(0x020C, words(&[100, 100])), (0x02FA, vec![0, 0, 100, 0, 0, 0, 0, 0, 0, 0]), (0x012D, words(&[0]))];
    let mut zigzag = vec![2000i16];
    zigzag.extend((0..2000).flat_map(|i| [if i % 2 == 0 { 0 } else { 100 }, i % 100]));
    slow.extend((0..200).map(|_| (0x0325, words(&zigzag))));
    let started = std::time::Instant::now();
    let _ = wmf_picture(&wmf(&slow), Some((400, 400)));
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}

#[test]
fn rtf_shows_a_metafile_that_is_a_drawing() {
    let mut records = red_window();
    records.push((0x041B, words(&[80, 80, 20, 20])));
    let rtf = format!("{{\\rtf1\\ansi\\pard a{{\\pict\\wmetafile8\\picwgoal600\\pichgoal300 {}}}b\\par}}", hexed(&wmf(&records)));
    let doc = crate::rtf::parse(rtf.as_bytes());
    let run = paras(&doc)[0].runs.iter().find(|r| r.picture.is_some()).unwrap();
    assert_eq!((run.picture.clone(), run.extent), (Some(Picture::Inline(Some(0))), (40, 20)));
    assert_eq!(bmp_size(&doc.written[0]), (80, 40));
    assert_eq!(pixel(&doc.written[0], 40, 20), RED);
}

/// The long numbers of a record.
fn longs(list: &[i32]) -> Vec<u8> {
    list.iter().flat_map(|n| n.to_le_bytes()).collect()
}

/// An enhanced metafile of these records (what each is, and what follows its size): a picture
/// a hundred pixels each way on a screen of 96 to the inch.
fn emf_of(records: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut file = vec![0u8; 88];
    for (at, n) in [(0, 1), (4, 88), (16, 99), (20, 99), (32, 2646), (36, 2646), (72, 1920), (76, 1080), (80, 508), (84, 286)] {
        file[at..at + 4].copy_from_slice(&i32::to_le_bytes(n));
    }
    file[40..44].copy_from_slice(b" EMF");
    for (kind, body) in records.iter().chain(&[(14u32, longs(&[0, 0, 20]))]) {
        file.extend(kind.to_le_bytes());
        file.extend(u32::try_from(8 + body.len()).unwrap().to_le_bytes());
        file.extend(body);
    }
    file
}

/// Records that make a red brush and select it, and select the pen that draws nothing.
fn red_brush() -> Vec<(u32, Vec<u8>)> {
    let mut brush = longs(&[1, 0]);
    brush.extend([255, 0, 0, 0, 0, 0, 0, 0]);
    vec![(39, brush), (37, longs(&[1])), (37, 0x8000_0008u32.to_le_bytes().to_vec())]
}

#[test]
fn an_enhanced_metafile_that_is_a_drawing_is_drawn() {
    use crate::metafile::emf_picture;
    let draw = |more: Vec<(u32, Vec<u8>)>, want| {
        let mut records = red_brush();
        records.extend(more);
        emf_picture(&emf_of(&records), want)
    };
    let at = |bmp: &[u8], points: [(usize, usize); 2]| points.map(|(x, y)| pixel(bmp, x, y));
    // A box, at twice the size it is shown at; with no size asked for, at the frame's own.
    let bmp = draw(vec![(43, longs(&[20, 20, 80, 80]))], Some((50, 50))).unwrap();
    assert_eq!(bmp_size(&bmp), (100, 100));
    assert_eq!(at(&bmp, [(50, 50), (10, 10)]), [RED, WHITE]);
    assert_eq!(bmp_size(&draw(vec![(43, longs(&[20, 20, 80, 80]))], None).unwrap()), (100, 100));

    // A path: a triangle that is closed and filled.
    let path = vec![
        (59, Vec::new()),
        (27, longs(&[10, 10])),
        (54, longs(&[90, 10])),
        (54, longs(&[50, 90])),
        (61, Vec::new()),
        (60, Vec::new()),
        (62, longs(&[0, 0, 99, 99])),
    ];
    assert_eq!(at(&draw(path.clone(), Some((50, 50))).unwrap(), [(50, 30), (12, 80)]), [RED, WHITE]);
    // A path that is only begun draws nothing.
    assert_eq!(draw(path[..6].to_vec(), None), None);

    // The world turned: half the size, and moved to the middle.
    let turn: Vec<u8> = [0.5f32, 0.0, 0.0, 0.5, 50.0, 50.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let bmp = draw(vec![(35, turn.clone()), (43, longs(&[0, 0, 100, 100]))], Some((50, 50))).unwrap();
    assert_eq!(at(&bmp, [(75, 75), (25, 25)]), [RED, WHITE]);
    // The same turn put after one that was there, and a state put back: the box is whole again.
    let mut after = turn.clone();
    after.extend(longs(&[3]));
    let bmp = draw(vec![(33, Vec::new()), (36, after), (34, longs(&[-1])), (43, longs(&[0, 0, 100, 100]))], Some((50, 50))).unwrap();
    assert_eq!(at(&bmp, [(75, 75), (25, 25)]), [RED, RED]);

    // A window of a thousand units onto a hundred of the device's.
    let window = vec![(17, longs(&[8])), (9, longs(&[1000, 1000])), (11, longs(&[100, 100])), (43, longs(&[200, 200, 800, 800]))];
    assert_eq!(at(&draw(window, Some((50, 50))).unwrap(), [(50, 50), (10, 10)]), [RED, WHITE]);

    // Points of short numbers: a polygon, and a curve that runs on from where the pen is.
    let mut polygon = longs(&[0, 0, 99, 99, 3]);
    polygon.extend(words(&[10, 10, 90, 10, 50, 90]));
    assert_eq!(at(&draw(vec![(86, polygon)], Some((50, 50))).unwrap(), [(50, 30), (12, 80)]), [RED, WHITE]);
    let mut curve = longs(&[0, 0, 99, 99, 3]);
    curve.extend(words(&[10, 90, 90, 90, 90, 10]));
    let bmp = draw(vec![(37, 0x8000_0007u32.to_le_bytes().to_vec()), (27, longs(&[10, 10])), (88, curve)], Some((50, 50))).unwrap();
    // It hangs down from both ends to seventy in the middle.
    assert!(pixel(&bmp, 50, 70)[0] < 200 && pixel(&bmp, 50, 30) == WHITE && pixel(&bmp, 50, 85) == WHITE);

    // A path that is one box is cut to; a region of no size brings the whole page back.
    let cut = vec![(59, Vec::new()), (43, longs(&[0, 0, 50, 50])), (60, Vec::new()), (67, longs(&[5])), (43, longs(&[0, 0, 100, 100]))];
    assert_eq!(at(&draw(cut.clone(), Some((50, 50))).unwrap(), [(25, 25), (75, 75)]), [RED, WHITE]);
    let mut back = cut;
    back.extend([(75, longs(&[0, 5])), (43, longs(&[0, 0, 100, 100]))]);
    assert_eq!(at(&draw(back, Some((50, 50))).unwrap(), [(25, 25), (75, 75)]), [RED, RED]);

    // A pie from the right of its ellipse round to the bottom, against the clock and with it.
    let pie = |way: i32| draw(vec![(57, longs(&[way])), (47, longs(&[0, 0, 100, 100, 100, 50, 50, 100]))], Some((50, 50))).unwrap();
    assert_eq!(at(&pie(1), [(30, 30), (70, 70)]), [RED, WHITE]);
    assert_eq!(at(&pie(2), [(30, 30), (70, 70)]), [WHITE, RED]);
}

#[test]
fn an_enhanced_metafile_that_cannot_be_drawn_whole_is_not_drawn() {
    use crate::metafile::emf_picture;
    let with = |extra: (u32, Vec<u8>)| {
        let mut records = red_brush();
        records.extend([(43, longs(&[20, 20, 80, 80])), extra]);
        emf_picture(&emf_of(&records), None)
    };
    assert!(with((18, longs(&[1]))).is_some());
    // Text, a bitmap beside the drawing, a region filled, a pen that inverts, a mapping mode in
    // parts of a millimetre, a box cut out, a drawing from right to left.
    assert_eq!(with(emf_text((10, 40), "A", 0)), None);
    assert_eq!(with((81, vec![0; 72])), None);
    assert_eq!(with((71, vec![0; 24])), None);
    assert_eq!(with((20, longs(&[6]))), None);
    assert_eq!(with((17, longs(&[2]))), None);
    assert_eq!(with((29, longs(&[0, 0, 10, 10]))), None);
    assert_eq!(with((115, longs(&[1]))), None);
    // A hatched brush that is filled with; an object that was never made; a state put back
    // that was never saved; a path that is cut to and is no box.
    let mut hatched = longs(&[2, 2]);
    hatched.extend([0; 8]);
    assert_eq!(emf_picture(&emf_of(&[(39, hatched), (37, longs(&[2])), (43, longs(&[20, 20, 80, 80]))]), None), None);
    assert_eq!(with((37, longs(&[9]))), None);
    assert_eq!(with((34, longs(&[-1]))), None);
    assert_eq!(with((67, longs(&[5]))), None);
    // Nothing drawn: only what is said beside the drawing, as in a file whose drawing is all
    // in its comments. No frame and no box that was drawn in.
    assert_eq!(emf_picture(&emf_of(&[(70, longs(&[4, 0x2B46_4D45]))]), None), None);
    let mut empty = emf_of(&[(43, longs(&[20, 20, 80, 80]))]);
    empty[8..40].copy_from_slice(&longs(&[0, 0, -1, -1, 0, 0, 0, 0]));
    assert_eq!(emf_picture(&empty, None), None);
}

#[test]
fn hostile_enhanced_drawings_never_panic() {
    use crate::metafile::emf_picture;
    let mut records = red_brush();
    let turn: Vec<u8> = [0.9f32, 0.1, -0.1, 0.9, 5.0, 5.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut pen = longs(&[2, 0, 3, 0]);
    pen.extend([0, 0, 255, 0]);
    let mut wide = longs(&[3, 0, 0, 0, 0, 0x0001_0000, 4, 0]);
    wide.extend([0, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut many = longs(&[0, 0, 99, 99, 2, 6, 3, 3]);
    many.extend(longs(&[10, 10, 90, 10, 50, 90, 20, 20, 80, 20, 50, 80]));
    let mut region = longs(&[48, 1, 32, 1, 1, 16, 0, 0, 90, 90]);
    region.extend(longs(&[5, 5, 90, 90]));
    records.extend([
        (38, pen),
        (37, longs(&[2])),
        (95, wide),
        (37, longs(&[3])),
        (33, Vec::new()),
        (75, region),
        (30, longs(&[10, 10, 90, 90])),
        (8, many.clone()),
        (7, many),
        (36, turn.iter().copied().chain(longs(&[2])).collect()),
        (42, longs(&[20, 20, 80, 80])),
        (44, longs(&[20, 20, 80, 80, 10, 10])),
        (45, longs(&[0, 0, 100, 100, 50, 0, 0, 50])),
        (46, longs(&[0, 0, 100, 100, 50, 0, 0, 50])),
        (55, longs(&[0, 0, 100, 100, 50, 0, 0, 50])),
        (59, Vec::new()),
        (27, longs(&[10, 10])),
        (6, longs(&[0, 0, 99, 99, 2, 90, 10, 50, 90])),
        (5, longs(&[0, 0, 99, 99, 3, 40, 80, 20, 60, 10, 10])),
        (61, Vec::new()),
        (60, Vec::new()),
        (63, longs(&[0, 0, 99, 99])),
        (15, longs(&[30, 30, 0x00FF_0000])),
        (34, longs(&[1])),
        (40, longs(&[1])),
    ]);
    let file = emf_of(&records);
    assert!(emf_picture(&file, Some((40, 40))).is_some());
    // Cut short anywhere, and with any one byte turned over or made the largest it can be.
    for end in 0..file.len() {
        let _ = emf_picture(&file[..end], Some((24, 24)));
        for with in [file[end] ^ 0xFF, 0xFF, 0x80, 0] {
            let mut changed = file.clone();
            changed[end] = with;
            for bmp in [emf_picture(&changed, Some((24, 24))), emf_picture(&changed, None)].into_iter().flatten() {
                let (w, h) = bmp_size(&bmp);
                assert!(w <= 800 && h <= 800);
            }
        }
    }
    // More points than a record holds; more polygons than there are; an object far out.
    assert_eq!(emf_picture(&emf_of(&[(3, longs(&[0, 0, 99, 99, -1, 0, 0]))]), None), None);
    assert_eq!(emf_picture(&emf_of(&[(8, longs(&[0, 0, 99, 99, -1, 3, 3]))]), None), None);
    assert_eq!(emf_picture(&emf_of(&[(39, longs(&[i32::MAX, 0, 0, 0])), (43, longs(&[20, 20, 80, 80]))]), None), None);
    // A turn that is no number.
    let nan: Vec<u8> = [f32::NAN, 0.0, 0.0, 1.0, 0.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(emf_picture(&emf_of(&[(35, nan), (43, longs(&[20, 20, 80, 80]))]), None), None);
}

#[test]
fn rtf_shows_an_enhanced_metafile_that_is_a_drawing() {
    let mut records = red_brush();
    records.push((43, longs(&[20, 20, 80, 80])));
    let rtf = format!("{{\\rtf1\\ansi\\pard a{{\\pict\\emfblip\\picwgoal600\\pichgoal600 {}}}b\\par}}", hexed(&emf_of(&records)));
    let doc = crate::rtf::parse(rtf.as_bytes());
    let run = paras(&doc)[0].runs.iter().find(|r| r.picture.is_some()).unwrap();
    assert_eq!((run.picture.clone(), run.extent), (Some(Picture::Inline(Some(0))), (40, 40)));
    assert_eq!(bmp_size(&doc.written[0]), (80, 80));
    assert_eq!(pixel(&doc.written[0], 40, 40), RED);
}

/// A font of boxes, which stands for whatever font a drawing names: every letter is a box
/// from 0.05 to 0.45 of the em across and 0.7 of it high, half an em from the next.
fn boxes(chars: &str) -> mailcraft_fonts::Face {
    mailcraft_fonts::Face { data: mailcraft_fonts::boxes::font(chars), index: 0 }
}

const BLUE: [u8; 3] = [0, 0, 255];

/// META_CREATEFONTINDIRECT for a font this high, turned so many tenths of a degree, with these
/// four bytes for italic, underlined, struck out and its character set.
fn wmf_font(height: i16, turn: i16, ways: [u8; 4]) -> (u16, Vec<u8>) {
    let mut body = words(&[height, 0, turn, 0, 400]);
    body.extend(ways);
    body.extend([0; 4]);
    body.extend(b"Box\0");
    (0x02FB, body)
}

/// A window of a hundred units each way, a font twenty units high that is selected, red
/// letters and nothing filled in behind them.
fn lettered(font: (u16, Vec<u8>)) -> Vec<(u16, Vec<u8>)> {
    vec![(0x020B, words(&[0, 0])), (0x020C, words(&[100, 100])), font, (0x012D, words(&[0])), (0x0209, vec![255, 0, 0, 0]), (0x0102, words(&[1]))]
}

/// META_EXTTEXTOUT of `text` at a point, with these options and what follows the text.
fn ext_text(at: (i16, i16), text: &[u8], options: i16, after: &[i16]) -> (u16, Vec<u8>) {
    let mut body = words(&[at.1, at.0, i16::try_from(text.len()).unwrap(), options]);
    body.extend(text);
    if text.len() % 2 == 1 {
        body.push(0);
    }
    body.extend(words(after));
    (0x0A32, body)
}

#[test]
fn text_in_a_metafile_is_drawn() {
    use crate::metafile::{wmf_drawn, wmf_picture};
    let face = boxes("AB ");
    let draw = |font: (u16, Vec<u8>), more: Vec<(u16, Vec<u8>)>| {
        let mut records = lettered(font);
        records.extend(more);
        wmf_drawn(&wmf(&records), Some((50, 50)), Some(&face))
    };
    let plain = || wmf_font(-20, 0, [0, 0, 0, 0]);

    // Two letters whose top left corner is the point given: boxes fourteen high that stand on
    // the line sixteen below it.
    let drawn = draw(plain(), vec![ext_text((10, 40), b"AB", 0, &[])]).unwrap();
    assert_eq!((bmp_size(&drawn.file), drawn.words.as_str()), ((100, 100), "AB"));
    for (x, y, colour) in [(15, 50, RED), (25, 50, RED), (20, 50, WHITE), (15, 40, WHITE), (15, 58, WHITE), (35, 50, WHITE)] {
        assert_eq!(pixel(&drawn.file, x, y), colour, "{x} {y}");
    }
    // With nobody to ask for the letters the drawing is not made at all.
    let mut records = lettered(plain());
    records.push(ext_text((10, 40), b"AB", 0, &[]));
    assert_eq!(wmf_picture(&wmf(&records), Some((50, 50))), None);

    // The colour behind the letters is filled in unless the drawing says not.
    let drawn = draw(plain(), vec![(0x0201, vec![0, 0, 255, 0]), (0x0102, words(&[2])), ext_text((10, 40), b"AB", 0, &[])]).unwrap();
    assert_eq!((pixel(&drawn.file, 20, 50), pixel(&drawn.file, 15, 50), pixel(&drawn.file, 20, 62)), (BLUE, RED, WHITE));

    // The drawing's own spacing is kept: the second letter thirty units on.
    let drawn = draw(plain(), vec![ext_text((10, 40), b"AB", 0, &[30, 30])]).unwrap();
    assert_eq!((pixel(&drawn.file, 45, 50), pixel(&drawn.file, 25, 50)), (RED, WHITE));

    // Set by its middle and by the line it stands on.
    let drawn = draw(plain(), vec![(0x012E, words(&[6 | 24])), ext_text((50, 50), b"AB", 0, &[])]).unwrap();
    assert_eq!((pixel(&drawn.file, 45, 45), pixel(&drawn.file, 55, 45), pixel(&drawn.file, 45, 52)), (RED, RED, WHITE));
    // Set by its right end and its bottom.
    let drawn = draw(plain(), vec![(0x012E, words(&[2 | 8])), ext_text((50, 50), b"AB", 0, &[])]).unwrap();
    assert_eq!((pixel(&drawn.file, 35, 40), pixel(&drawn.file, 45, 40), pixel(&drawn.file, 55, 40)), (RED, RED, WHITE));

    // A line turned a quarter round runs up the page.
    let drawn = draw(wmf_font(-20, 900, [0, 0, 0, 0]), vec![ext_text((50, 80), b"A", 0, &[])]).unwrap();
    assert_eq!((pixel(&drawn.file, 60, 75), pixel(&drawn.file, 45, 75), pixel(&drawn.file, 60, 85)), (RED, WHITE, WHITE));

    // A font given by the height of its lines, underlined and struck out.
    let drawn = draw(wmf_font(20, 0, [0, 1, 1, 0]), vec![ext_text((10, 40), b"A B", 0, &[])]).unwrap();
    assert_eq!(
        (pixel(&drawn.file, 15, 50), pixel(&drawn.file, 22, 58), pixel(&drawn.file, 22, 50), pixel(&drawn.file, 22, 54)),
        (RED, RED, RED, WHITE)
    );
    assert_eq!(drawn.words, "A B");

    // META_TEXTOUT, and lines that each start where the last one ended.
    let text_out = |text: &[u8; 1], at: (i16, i16)| {
        let mut body = words(&[1]);
        body.extend([text[0], 0]);
        body.extend(words(&[at.1, at.0]));
        (0x0521, body)
    };
    let drawn = draw(plain(), vec![text_out(b"A", (10, 40)), text_out(b"B", (60, 10))]).unwrap();
    assert_eq!((pixel(&drawn.file, 15, 50), pixel(&drawn.file, 65, 20), drawn.words.as_str()), (RED, RED, "A B"));
    let kept = vec![(0x0214, words(&[40, 10])), (0x012E, words(&[1])), text_out(b"A", (0, 0)), text_out(b"B", (0, 0))];
    let drawn = draw(plain(), kept).unwrap();
    assert_eq!((pixel(&drawn.file, 15, 50), pixel(&drawn.file, 25, 50), pixel(&drawn.file, 5, 10)), (RED, RED, WHITE));

    // The box of a line is filled, and the letters are cut to it.
    let boxed = vec![(0x0201, vec![0, 0, 255, 0]), ext_text((10, 40), b"AB", 2 | 4, &[])];
    let mut boxed_line = boxed.clone();
    boxed_line[1] = {
        let mut body = words(&[40, 10, 2, 2 | 4, 5, 35, 17, 65]);
        body.extend(b"AB");
        (0x0A32, body)
    };
    let drawn = draw(plain(), boxed_line).unwrap();
    assert_eq!(
        (pixel(&drawn.file, 7, 37), pixel(&drawn.file, 14, 50), pixel(&drawn.file, 18, 50), pixel(&drawn.file, 25, 50)),
        (BLUE, RED, WHITE, WHITE)
    );
    // A box with no text is only filled, and needs nobody who knows letters.
    let mut records = lettered(plain());
    records.push((0x0201, vec![0, 0, 255, 0]));
    records.push((0x0A32, words(&[0, 0, 0, 2, 20, 20, 80, 80])));
    let bmp = wmf_picture(&wmf(&records), Some((50, 50))).unwrap();
    assert_eq!((pixel(&bmp, 50, 50), pixel(&bmp, 10, 10)), (BLUE, WHITE));
}

#[test]
fn text_that_cannot_be_drawn_whole_is_not_drawn() {
    use crate::metafile::wmf_drawn;
    let face = boxes("AB ");
    let draw = |font: Option<(u16, Vec<u8>)>, more: Vec<(u16, Vec<u8>)>| {
        let mut records = lettered(font.clone().unwrap_or_else(|| (0x02FA, vec![0, 0, 1, 0, 0, 0, 0, 0, 0, 0])));
        records.extend(more);
        wmf_drawn(&wmf(&records), Some((50, 50)), Some(&face))
    };
    let plain = |set: u8| Some(wmf_font(-20, 0, [0, 0, 0, set]));
    assert!(draw(plain(0), vec![ext_text((10, 40), b"AB", 0, &[])]).is_some());
    // No font was chosen; a letter the font lacks; a font of symbols; a character set nobody
    // knows; a font of no height.
    assert_eq!(draw(None, vec![ext_text((10, 40), b"AB", 0, &[])]), None);
    assert_eq!(draw(plain(0), vec![ext_text((10, 40), b"AZ", 0, &[])]), None);
    assert_eq!(draw(plain(2), vec![ext_text((10, 40), b"AB", 0, &[])]), None);
    assert_eq!(draw(plain(77), vec![ext_text((10, 40), b"A\xE9", 0, &[])]), None);
    assert_eq!(draw(Some(wmf_font(0, 0, [0; 4])), vec![ext_text((10, 40), b"AB", 0, &[])]), None);
    // Letters that are joined or run right to left (Hebrew and Arabic, by their sets); a line
    // said to read right to left; numbers of glyphs in place of characters.
    assert_eq!(draw(plain(177), vec![ext_text((10, 40), b"\xE0\xE1", 0, &[])]), None);
    assert_eq!(draw(plain(178), vec![ext_text((10, 40), b"\xC7\xC8", 0, &[])]), None);
    assert_eq!(draw(plain(0), vec![(0x012E, words(&[256])), ext_text((10, 40), b"AB", 0, &[])]), None);
    assert_eq!(draw(plain(0), vec![ext_text((10, 40), b"AB", 0x10, &[])]), None);
    assert_eq!(draw(plain(0), vec![ext_text((10, 40), b"AB", 0x80, &[])]), None);
    // More text than the record holds.
    assert_eq!(draw(plain(0), vec![(0x0A32, words(&[40, 10, 300, 0, 0x4141]))]), None);
    assert_eq!(draw(plain(0), vec![(0x0521, words(&[300, 0x4141, 40, 10]))]), None);
}

/// EMR_EXTCREATEFONTINDIRECTW for object 2: a font this high, turned so many tenths of a
/// degree.
fn emf_font(height: i32, turn: i32) -> (u32, Vec<u8>) {
    let mut body = longs(&[2, height, 0, turn, 0, 400]);
    body.extend([0; 8]);
    let mut name: Vec<u8> = "Box".encode_utf16().flat_map(u16::to_le_bytes).collect();
    name.resize(64, 0);
    body.extend(name);
    (82, body)
}

/// EMR_EXTTEXTOUTW of `text` at a point, each character ten units from the next.
fn emf_text(at: (i32, i32), text: &str, options: i32) -> (u32, Vec<u8>) {
    let units: Vec<u16> = text.encode_utf16().collect();
    let count = i32::try_from(units.len()).unwrap();
    let after = 76 + 2 * count + (2 * count) % 4;
    let mut body = longs(&[0, 0, 0, 0, 1, 0, 0, at.0, at.1, count, 76, options, 5, 35, 17, 65, after]);
    body.extend(units.iter().flat_map(|unit| unit.to_le_bytes()));
    body.resize(usize::try_from(after).unwrap() - 8, 0);
    body.extend(longs(&vec![10; units.len()]));
    (84, body)
}

/// The font chosen, red letters and nothing filled in behind them.
fn emf_lettered(font: (u32, Vec<u8>)) -> Vec<(u32, Vec<u8>)> {
    vec![font, (37, longs(&[2])), (24, vec![255, 0, 0, 0]), (18, longs(&[1]))]
}

#[test]
fn text_in_an_enhanced_metafile_is_drawn() {
    use crate::metafile::{emf_drawn, emf_picture};
    let face = boxes("AB ");
    let draw = |font: (u32, Vec<u8>), more: Vec<(u32, Vec<u8>)>| {
        let mut records = emf_lettered(font);
        records.extend(more);
        emf_drawn(&emf_of(&records), Some((50, 50)), Some(&face))
    };
    let drawn = draw(emf_font(-20, 0), vec![emf_text((10, 40), "AB", 0)]).unwrap();
    assert_eq!((bmp_size(&drawn.file), drawn.words.as_str()), ((100, 100), "AB"));
    for (x, y, colour) in [(15, 50, RED), (25, 50, RED), (20, 50, WHITE), (15, 40, WHITE), (15, 58, WHITE)] {
        assert_eq!(pixel(&drawn.file, x, y), colour, "{x} {y}");
    }
    let mut records = emf_lettered(emf_font(-20, 0));
    records.push(emf_text((10, 40), "AB", 0));
    assert_eq!(emf_picture(&emf_of(&records), Some((50, 50))), None);

    // Text in bytes, in the font's character set.
    let mut bytes = longs(&[0, 0, 0, 0, 1, 0, 0, 10, 40, 2, 76, 0, 0, 0, 0, 0, 80]);
    bytes.extend(b"AB\0\0");
    bytes.extend(longs(&[30, 30]));
    let drawn = draw(emf_font(-20, 0), vec![(83, bytes)]).unwrap();
    assert_eq!((pixel(&drawn.file, 15, 50), pixel(&drawn.file, 45, 50), pixel(&drawn.file, 25, 50)), (RED, RED, WHITE));

    // The colour behind, the box filled and the letters cut to it.
    let drawn = draw(emf_font(-20, 0), vec![(25, vec![0, 0, 255, 0]), emf_text((10, 40), "AB", 2 | 4)]).unwrap();
    assert_eq!(
        (pixel(&drawn.file, 7, 37), pixel(&drawn.file, 14, 50), pixel(&drawn.file, 18, 50), pixel(&drawn.file, 25, 50)),
        (BLUE, RED, WHITE, WHITE)
    );
    let drawn = draw(emf_font(-20, 0), vec![(25, vec![0, 0, 255, 0]), (18, longs(&[2])), emf_text((10, 40), "AB", 0)]).unwrap();
    assert_eq!((pixel(&drawn.file, 20, 50), pixel(&drawn.file, 15, 50)), (BLUE, RED));

    // Set by its middle and the line it stands on; turned a quarter round.
    let drawn = draw(emf_font(-20, 0), vec![(22, longs(&[6 | 24])), emf_text((50, 50), "AB", 0)]).unwrap();
    assert_eq!((pixel(&drawn.file, 45, 45), pixel(&drawn.file, 55, 45), pixel(&drawn.file, 45, 52)), (RED, RED, WHITE));
    let drawn = draw(emf_font(-20, 900), vec![emf_text((50, 80), "A", 0)]).unwrap();
    assert_eq!((pixel(&drawn.file, 60, 75), pixel(&drawn.file, 45, 75)), (RED, WHITE));

    // The world made twice as large: letters twice the size, twice as far along.
    let twice: Vec<u8> = [2f32, 0.0, 0.0, 2.0, 0.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let drawn = draw(emf_font(-20, 0), vec![(35, twice), emf_text((5, 10), "AB", 0)]).unwrap();
    assert_eq!(
        (pixel(&drawn.file, 20, 40), pixel(&drawn.file, 40, 40), pixel(&drawn.file, 30, 40), pixel(&drawn.file, 20, 22)),
        (RED, RED, WHITE, WHITE)
    );

    // Not drawn: a world that is turned; a letter the font lacks; right to left; numbers of
    // glyphs; text in a path; one of the device's own fonts; more text than the record holds.
    let turned: Vec<u8> = [0f32, 1.0, -1.0, 0.0, 0.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(draw(emf_font(-20, 0), vec![(35, turned), emf_text((10, 40), "AB", 0)]), None);
    assert_eq!(draw(emf_font(-20, 0), vec![emf_text((10, 40), "AZ", 0)]), None);
    assert_eq!(draw(emf_font(-20, 0), vec![emf_text((10, 40), "\u{5d0}\u{5d1}", 0)]), None);
    assert_eq!(draw(emf_font(-20, 0), vec![emf_text((10, 40), "AB", 0x10)]), None);
    assert_eq!(draw(emf_font(-20, 0), vec![(59, Vec::new()), emf_text((10, 40), "AB", 0)]), None);
    assert_eq!(draw(emf_font(-20, 0), vec![(37, 0x8000_000Du32.to_le_bytes().to_vec()), emf_text((10, 40), "AB", 0)]), None);
    let mut long = emf_text((10, 40), "AB", 0);
    long.1[36..40].copy_from_slice(&5000i32.to_le_bytes());
    assert_eq!(draw(emf_font(-20, 0), vec![long]), None);
}

#[test]
fn hostile_text_in_drawings_never_panics() {
    use crate::metafile::{emf_drawn, wmf_drawn};
    let face = boxes("AB ");
    let mut records = lettered(wmf_font(-20, 450, [1, 1, 1, 0]));
    records.extend([
        (0x0201, vec![0, 0, 255, 0]),
        (0x012E, words(&[6 | 24])),
        ext_text((50, 50), b"AB", 0, &[]),
        (0x0A32, {
            let mut body = words(&[40, 10, 3, 2 | 4, 5, 35, 17, 65]);
            body.extend(b"A B\0");
            body.extend(words(&[9, 9, 9]));
            body
        }),
        (0x0521, words(&[2, 0x4241, 40, 10])),
    ]);
    let file = wmf(&records);
    assert!(wmf_drawn(&file, Some((40, 40)), Some(&face)).is_some());
    for end in 0..file.len() {
        let _ = wmf_drawn(&file[..end], Some((24, 24)), Some(&face));
        for with in [file[end] ^ 0xFF, 0xFF, 0x80, 0] {
            let mut changed = file.clone();
            changed[end] = with;
            if let Some(drawn) = wmf_drawn(&changed, Some((24, 24)), Some(&face)) {
                let (w, h) = bmp_size(&drawn.file);
                assert!(w <= 800 && h <= 800 && drawn.words.chars().count() <= 2000);
            }
        }
    }
    let mut records = emf_lettered(emf_font(-20, 450));
    records.extend([(25, vec![0, 0, 255, 0]), (22, longs(&[6 | 24])), emf_text((50, 50), "A B", 2 | 4), emf_text((10, 40), "AB", 0)]);
    let file = emf_of(&records);
    assert!(emf_drawn(&file, Some((40, 40)), Some(&face)).is_some());
    for end in 0..file.len() {
        let _ = emf_drawn(&file[..end], Some((24, 24)), Some(&face));
        for with in [file[end] ^ 0xFF, 0xFF, 0x80, 0] {
            let mut changed = file.clone();
            changed[end] = with;
            if let Some(drawn) = emf_drawn(&changed, Some((24, 24)), Some(&face)) {
                let (w, h) = bmp_size(&drawn.file);
                assert!(w <= 800 && h <= 800 && drawn.words.chars().count() <= 2000);
            }
        }
    }
    // A great many lines of text say no more than is kept.
    let mut records = lettered(wmf_font(-20, 0, [0; 4]));
    records.extend((0..2000).map(|_| ext_text((10, 40), b"AB AB", 0, &[])));
    let drawn = wmf_drawn(&wmf(&records), Some((24, 24)), Some(&face)).unwrap();
    assert!(drawn.words.chars().count() <= 2000 && drawn.words.starts_with("AB AB AB"));
    // A font that is not one, and one cut short, give no letters.
    for bad in [&b"not a font"[..], &face.data[..face.data.len() / 2], &[]] {
        let broken = mailcraft_fonts::Face { data: bad.to_vec(), index: 0 };
        let mut records = lettered(wmf_font(-20, 0, [0; 4]));
        records.push(ext_text((10, 40), b"AB", 0, &[]));
        assert_eq!(wmf_drawn(&wmf(&records), Some((24, 24)), Some(&broken)), None);
    }
}

#[test]
fn rtf_shows_the_text_of_a_drawing_and_says_what_it_says() {
    let face = boxes("Bay14 ");
    let mut records = lettered(wmf_font(-20, 0, [0; 4]));
    records.push(ext_text((10, 40), b"Bay 14", 0, &[]));
    let rtf = format!("{{\\rtf1\\ansi\\pard a{{\\pict\\wmetafile8\\picwgoal750\\pichgoal750 {}}}b\\par}}", hexed(&wmf(&records)));
    let doc = crate::rtf::parse_with(rtf.as_bytes(), Some(&face));
    let run = paras(&doc)[0].runs.iter().find(|r| r.picture.is_some()).unwrap();
    assert_eq!((run.picture.clone(), run.text.as_str()), (Some(Picture::Inline(Some(0))), "Bay 14"));
    assert_eq!(pixel(&doc.written[0], 15, 50), RED);
    // With nobody who knows letters the picture keeps its place and says nothing.
    let doc = crate::rtf::parse(rtf.as_bytes());
    let run = paras(&doc)[0].runs.iter().find(|r| r.picture.is_some()).unwrap();
    assert_eq!((run.picture.clone(), run.text.as_str(), doc.written.len()), (Some(Picture::Inline(None)), "", 0));
}
