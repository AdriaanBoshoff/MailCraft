use std::sync::Arc;

use mailcraft_fonts::{Face, Library, boxes};
use mailcraft_formats::rich::{Align, Block, Cell, Doc, Edge, Frame, Para, ParaKind, Picture, Run, Script, Style, Table, TableWidth, VAlign};

use super::*;

/// Every glyph is a box half as wide as the size, a space a quarter; a line is as tall as the size.
const CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789:[].- ";

/// A sheet 200 wide with 160 of room: 26 body characters to the line.
const SMALL: Paper = Paper { width: 200.0, height: 300.0, margin: 20.0 };

fn print_on(library: &Library, memo_: &Memo, paper: impl Into<Setup>) -> Result<Printed, LayoutError> {
    let mut fonts = Fonts::new(library);
    for family in ["Segoe UI", "Times New Roman", "Calibri"] {
        fonts.give(family, false, false, Face { data: boxes::font(CHARS), index: 0 });
    }
    memo(memo_, paper, &mut fonts)
}

fn print(body: &Doc, paper: Paper) -> Printed {
    print_on(&Library::empty(), &Memo { owner: "Owner", fields: &[], body, plain: false, sizes: &|_| None }, paper).unwrap()
}

fn run(text: &str) -> Run {
    Run { text: text.into(), ..Run::default() }
}

fn para(text: &str) -> Para {
    Para { runs: vec![run(text)], ..Para::default() }
}

fn doc(blocks: Vec<Block>) -> Doc {
    Doc { blocks, ..Doc::default() }
}

fn cell(text: &str) -> Cell {
    Cell { blocks: vec![Block::Para(para(text))], span: 1, rows: 1, ..Cell::default() }
}

/// The lines of a page without the owner's name at its head and the number at its foot.
fn body_lines(page: &Page) -> Vec<String> {
    let text = page.text();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines.pop();
    if lines.first().is_some_and(|l| l == "Owner") {
        lines.remove(0);
    }
    lines
}

fn within(printed: &Printed, paper: Paper) {
    for page in &printed.pages {
        for text in &page.texts {
            assert!(text.x >= paper.margin - 0.01 && text.x + text.width <= paper.width - paper.margin + 0.01, "{text:?}");
            assert!(text.y > paper.margin && text.y < paper.height - paper.margin + 0.01, "{text:?}");
            assert!(text.face < printed.faces.len());
        }
    }
}

#[test]
fn text_wraps_at_spaces_and_long_words_are_cut() {
    // A word of four is 24 wide and a space 3: six words fill 159 of the 160.
    let words = ["abcd"; 14].join(" ");
    let printed = print(&doc(vec![Block::Para(para(&words)), Block::Para(para(&"x".repeat(60)))]), SMALL);
    let lines = body_lines(&printed.pages[0]);
    let six = ["abcd"; 6].join(" ");
    assert_eq!(lines, [six.clone(), six, "abcd abcd".into(), "x".repeat(26), "x".repeat(26), "x".repeat(8)]);
    within(&printed, SMALL);
    // The lines stand a line apart.
    let ys: Vec<f32> = printed.pages[0].texts.iter().filter(|t| t.glyphs[0].1 == 'x').map(|t| t.y).collect();
    assert!((ys[1] - ys[0] - 12.0).abs() < 0.01 && (ys[2] - ys[1] - 12.0).abs() < 0.01, "{ys:?}");
}

#[test]
fn a_long_message_goes_on_to_more_pages_each_with_its_number() {
    let text: String = (0..100).map(|i| format!("line{i}\n")).collect();
    let body = Doc::plain(&text);
    let printed = print_on(
        &Library::empty(),
        &Memo { owner: "Owner", fields: &[("To", "someone".into()), ("Cc", " ".into())], body: &body, plain: true, sizes: &|_| None },
        SMALL,
    )
    .unwrap();
    assert!(printed.pages.len() > 3 && !printed.truncated);
    within(&printed, SMALL);
    let mut seen = Vec::new();
    for (n, page) in printed.pages.iter().enumerate() {
        assert_eq!((page.width, page.height), (200.0, 300.0));
        assert_eq!(page.text().lines().last().unwrap(), format!("{} of {}", n + 1, printed.pages.len()));
        // The number stands under everything else.
        let foot = page.texts.last().unwrap().y;
        assert!(page.texts.iter().filter(|t| t.y != foot).all(|t| t.y < foot - 8.0));
        seen.extend(body_lines(page));
    }
    // The field with a value is listed, the empty one is not.
    assert_eq!(seen[..2], ["To:someone".to_string(), "line0".into()]);
    assert_eq!(seen.len(), 101);
    assert_eq!(seen.last().unwrap(), "line99");
    // The owner's name stands over a thick line.
    assert!(printed.pages[0].rules.iter().any(|r| r.width > 2.0 && r.from.1 == r.to.1 && r.to.0 - r.from.0 > 150.0));
}

#[test]
fn looks_are_carried_onto_the_page() {
    let styled = |text: &str, style: Style| Run { text: text.into(), style, ..Run::default() };
    let body = doc(vec![
        Block::Para(Para {
            runs: vec![
                styled("under", Style { underline: true, color: Some([200, 0, 0]), ..Style::default() }),
                styled("back", Style { background: Some([255, 255, 0]), bold: true, italic: true, ..Style::default() }),
                styled("big", Style { size: Some(24.0), ..Style::default() }),
                styled("up", Style { script: Script::Super, ..Style::default() }),
                styled("link", Style { link: Some("https://example.org/".into()), ..Style::default() }),
            ],
            ..Para::default()
        }),
        Block::Para(Para { runs: vec![run("right")], align: Align::Right, ..Para::default() }),
        Block::Para(Para { runs: vec![run("item")], kind: ParaKind::Item("1.".into()), indent: 1, ..Para::default() }),
        Block::Para(Para { runs: vec![run("quoted")], quote: 2, ..Para::default() }),
        Block::Rule,
        Block::Para(Para { runs: vec![Run { text: "a cat".into(), picture: Some(Picture::Remote), ..Run::default() }], ..Para::default() }),
        Block::Para(Para { runs: vec![run("a\tb\u{a0}c\u{200b}d\u{7}")], ..Para::default() }),
    ]);
    let printed = print(&body, Paper::A4);
    let page = &printed.pages[0];
    let find = |text: &str| {
        page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap_or_else(|| panic!("{text}: {}", page.text()))
    };
    let under = find("under");
    assert_eq!(under.color, [200, 0, 0]);
    assert!(page.rules.iter().any(|r| r.color == [200, 0, 0] && r.from.0 == under.x && r.from.1 > under.y));
    // The face has no bold or italic of its own.
    let back = find("back");
    assert!(back.bold && back.slant);
    assert!(page.fills.iter().any(|f| f.color == [255, 255, 0] && f.x == back.x && (f.width - back.width).abs() < 0.01));
    // All of a line stands on one baseline, the raised text above it.
    let (big, up) = (find("big"), find("up"));
    assert_eq!((big.size, big.y), (24.0, under.y));
    assert!(up.y < under.y && up.size < 12.0);
    assert_eq!(find("link").color, [0, 84, 166]);
    let right = find("right");
    assert!((right.x + right.width - (Paper::A4.width - 36.0)).abs() < 0.01);
    let item = find("item");
    assert!((item.x - 66.0).abs() < 0.01);
    let dot = find(".");
    assert!(dot.x + dot.width < item.x && dot.y == item.y);
    let quoted = find("quoted");
    assert_eq!(page.rules.iter().filter(|r| r.from.0 == r.to.0 && r.from.1 < quoted.y && r.to.1 > quoted.y).count(), 2);
    assert!(page.rules.iter().any(|r| r.from.1 == r.to.1 && (r.to.0 - r.from.0 - (Paper::A4.width - 72.0)).abs() < 0.01));
    assert_eq!(find("[picture not downloaded: a cat]").size, 9.0);
    // A tab is room up to the next stop (every half inch where the paragraph has none of its
    // own), a hard space a space; marks that draw nothing are not there.
    let (a, rest) = (find("a"), find("b cd"));
    assert!((rest.x - (a.x + 36.0)).abs() < 0.01);
}

#[test]
fn a_table_stands_in_columns_inside_its_lines() {
    let mut wide = cell("wide");
    wide.span = 2;
    wide.background = Some([200, 220, 255]);
    let mut low = cell("low");
    low.valign = VAlign::Bottom;
    let tall =
        Cell { blocks: vec![Block::Para(para("one")), Block::Para(para("two")), Block::Para(para("three"))], span: 1, rows: 1, ..Cell::default() };
    let table = Table { rows: vec![vec![cell("a"), cell("bbbb")], vec![wide], vec![low, tall]], border: true, padding: 4, ..Table::default() };
    let printed = print(&doc(vec![Block::Para(para("before")), Block::Table(table), Block::Para(para("after"))]), Paper::A4);
    let page = &printed.pages[0];
    let find = |text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap();
    let (a, b, wide, low, three) = (find("a"), find("bbbb"), find("wide"), find("low"), find("three"));
    assert!(a.y == b.y && b.x > a.x + a.width && wide.y > a.y && wide.x == a.x);
    assert_eq!(low.y, three.y);
    assert!(find("before").y < a.y && find("after").y > three.y);
    // The second column starts where it does in every row.
    assert_eq!(find("one").x, b.x);
    // Five cells, each inside four lines.
    assert_eq!(page.rules.iter().filter(|r| r.width == 0.75).count(), 20);
    let fill = page.fills.iter().find(|f| f.color == [200, 220, 255]).unwrap();
    assert!(fill.x < wide.x && fill.x + fill.width > b.x + b.width && fill.y < wide.y && fill.y + fill.height > wide.y);
    // As wide as it needs to be and no wider: padding at both sides of both columns.
    assert!((fill.width - (18.0 + 30.0 + 4.0 * 3.0)).abs() < 0.01, "{}", fill.width);

    // A table that fills the page gives a column that asks for a width that width.
    let mut named = cell("n");
    named.width = 100;
    let full = Table { rows: vec![vec![named, cell("value")]], width: TableWidth::Fill, ..Table::default() };
    let printed = print(&doc(vec![Block::Table(full)]), Paper::A4);
    let page = &printed.pages[0];
    let x = |ch: char| page.texts.iter().find(|t| t.glyphs[0].1 == ch).unwrap().x;
    assert!((x('v') - x('n') - 75.0).abs() < 0.01);
}

#[test]
fn a_table_longer_than_a_page_goes_on_with_its_lines_and_frames() {
    let rows: Vec<Vec<Cell>> = (0..60).map(|i| vec![cell(&format!("r{i}")), cell("v")]).collect();
    let edge = Edge { width: 4, color: Some([0, 0, 90]), line: Line::Double };
    let frame = Arc::new(Frame { edges: [edge; 4], padding: [4; 4], indent: 0, between: Edge::default(), fill: None });
    let framed = |text: &str| Block::Para(Para { frame: Some(frame.clone()), ..para(text) });
    let body = doc(vec![Block::Table(Table { rows, border: true, frame: [edge; 4], ..Table::default() }), framed("in"), framed("frame")]);
    let printed = print(&body, SMALL);
    assert!(printed.pages.len() > 2);
    within(&printed, SMALL);
    let all: Vec<String> = printed.pages.iter().flat_map(body_lines).collect();
    let rows: Vec<&String> = all.iter().filter(|l| l.starts_with('r')).collect();
    assert_eq!(rows.len(), 60);
    assert!(rows.iter().enumerate().all(|(i, l)| **l == format!("r{i}v")));
    assert_eq!(all[60..], ["in".to_string(), "frame".into()]);
    for page in &printed.pages {
        assert!(page.rules.iter().all(|r| r.from.1 >= 19.0 && r.to.1 <= 281.0 && r.from.0 >= 19.0 && r.to.0 <= 181.0), "lines stay on the page");
    }
    // A double line is two thin ones; one frame goes round both paragraphs.
    let last = printed.pages.last().unwrap();
    assert!(last.rules.iter().filter(|r| r.color == [0, 0, 90] && r.width == 1.0).count() >= 8);
}

fn nested(depth: usize) -> Block {
    let mut block = Block::Para(para("deep"));
    for _ in 0..depth {
        let cell = Cell {
            blocks: vec![block],
            span: 60_000,
            rows: 60_000,
            width: u16::MAX,
            share: 255,
            padding: Some((u16::MAX, u16::MAX)),
            ..Cell::default()
        };
        block = Block::Table(Table {
            rows: vec![vec![cell]],
            border: true,
            padding: u16::MAX,
            spacing: u16::MAX,
            width: TableWidth::Pixels(u16::MAX),
            ..Table::default()
        });
    }
    block
}

#[test]
fn hostile_bodies_never_panic_and_stay_within_the_limits() {
    let wild = Edge { width: 255, color: None, line: Line::Double };
    let frame = Arc::new(Frame { edges: [wild; 4], padding: [u16::MAX; 4], indent: u8::MAX, between: wild, fill: Some([1, 2, 3]) });
    let odd = |size: f32| Run {
        text: "odd \u{10ffff}\u{0}\n\n\t".repeat(20),
        style: Style { size: Some(size), family: Some(",,,;\"".into()), ..Style::default() },
        ..Run::default()
    };
    let many: Vec<Cell> =
        (0..300).map(|i| Cell { span: i as u16, rows: 300 - i as u16, share: i as u8, width: i as u16 * 200, ..cell("c") }).collect();
    let blocks = vec![
        nested(300),
        Block::Para(Para {
            runs: [f32::NAN, f32::INFINITY, -1.0, 0.0, 1e30].map(odd).to_vec(),
            indent: 255,
            quote: 255,
            frame: Some(frame),
            ..Para::default()
        }),
        Block::Table(Table { rows: vec![many.clone(), Vec::new(), many], width: TableWidth::Percent(255), frame: [wild; 4], ..Table::default() }),
        Block::Table(Table { rows: vec![Vec::new(); 50], ..Table::default() }),
        Block::Para(Para { kind: ParaKind::Item("\u{0}".repeat(1000)), ..Para::default() }),
    ];
    let body = doc(blocks);
    for paper in [
        SMALL,
        Paper::A4,
        Paper { width: f32::NAN, height: f32::INFINITY, margin: -1e30 },
        Paper { width: 1.0, height: 1.0, margin: 1e30 },
        Paper { width: 1e30, height: 0.0, margin: f32::NAN },
    ] {
        let printed = print(&body, paper);
        assert!(!printed.pages.is_empty() && printed.pages.len() <= MAX_PAGES);
        for page in &printed.pages {
            assert!(page.width.is_finite() && page.height.is_finite());
            assert!(page.texts.iter().all(|t| t.x.is_finite() && t.y.is_finite() && t.width.is_finite() && t.face < printed.faces.len()));
            assert!(page.fills.iter().all(|f| f.x.is_finite() && f.y.is_finite() && f.width.is_finite() && f.height.is_finite()));
            assert!(page.rules.iter().all(|r| r.from.0.is_finite()
                && r.from.1.is_finite()
                && r.to.0.is_finite()
                && r.to.1.is_finite()
                && r.width.is_finite()));
        }
    }

    // More than the pages there may be: the rest is left out, and said to be.
    let endless = Doc::plain(&"a\n".repeat(60_000));
    let printed = print(&endless, Paper { width: 100.0, height: 100.0, margin: 10.0 });
    assert_eq!(printed.pages.len(), MAX_PAGES);
    assert!(printed.truncated);
}

#[test]
fn printing_needs_a_font() {
    let body = doc(vec![Block::Para(para("text"))]);
    let library = Library::empty();
    let mut fonts = Fonts::new(&library);
    assert_eq!(
        memo(&Memo { owner: "o", fields: &[], body: &body, plain: false, sizes: &|_| None }, Paper::A4, &mut fonts).err(),
        Some(LayoutError::NoFont)
    );
    // A font that may not be put into a document is none.
    let mut fonts = Fonts::new(&library);
    fonts.give("Arial", false, false, Face { data: boxes::font_with(CHARS, 0x0002), index: 0 });
    assert_eq!(
        memo(&Memo { owner: "o", fields: &[], body: &body, plain: false, sizes: &|_| None }, Paper::A4, &mut fonts).err(),
        Some(LayoutError::NoFont)
    );
    // Any font that is there will do for a character none has.
    let mut fonts = Fonts::new(&library);
    fonts.give("Arial", false, false, Face { data: boxes::font("o"), index: 0 });
    let printed = memo(&Memo { owner: "o", fields: &[], body: &body, plain: false, sizes: &|_| None }, Paper::A4, &mut fonts).unwrap();
    assert_eq!(printed.pages[0].texts[1].glyphs, [(0, 't'), (0, 'e'), (0, 'x'), (0, 't')]);
    assert!(printed.pages[0].texts[0].bold && !printed.pages[0].texts[1].bold);
}

#[test]
fn pictures_stand_in_the_line_at_the_size_asked_for() {
    let shown =
        |text: &str, id: &str, extent: (u16, u16)| Run { text: text.into(), picture: Some(Picture::Attached(id.into())), extent, ..Run::default() };
    let in_cell = Cell {
        blocks: vec![Block::Para(Para { runs: vec![shown("", "small", (0, 0))], ..Para::default() })],
        span: 1,
        rows: 1,
        valign: VAlign::Bottom,
        ..Cell::default()
    };
    let tall = Cell {
        blocks: vec![Block::Para(para("a")), Block::Para(para("b")), Block::Para(para("c")), Block::Para(para("d"))],
        span: 1,
        rows: 1,
        ..Cell::default()
    };
    let body = doc(vec![
        Block::Para(Para { runs: vec![run("ab"), shown("alt", "small", (0, 0)), run("cd")], ..Para::default() }),
        Block::Para(Para { runs: vec![shown("", "small", (80, 0))], align: Align::Right, ..Para::default() }),
        Block::Para(Para { runs: vec![shown("", "huge", (0, 0))], ..Para::default() }),
        Block::Para(Para { runs: vec![shown("gone", "missing", (40, 40))], ..Para::default() }),
        Block::Table(Table { rows: vec![vec![in_cell, tall]], ..Table::default() }),
    ]);
    let sizes = |picture: &Picture| match picture {
        Picture::Attached(id) if id == "small" => Some((40, 20)),
        Picture::Attached(id) if id == "huge" => Some((u16::MAX, u16::MAX)),
        _ => None,
    };
    let printed = print_on(&Library::empty(), &Memo { owner: "Owner", fields: &[], body: &body, plain: false, sizes: &sizes }, SMALL).unwrap();
    let all: Vec<&Placed> = printed.pages.iter().flat_map(|p| &p.pictures).collect();
    assert_eq!(all.len(), 4);
    for placed in &all {
        assert!(placed.x >= 19.99 && placed.x + placed.width <= 180.01 && placed.y >= 19.99 && placed.y + placed.height <= 264.01, "{placed:?}");
    }
    // 40 by 20 pixels are 30 by 15 points; the picture stands on the baseline between the words.
    let page = &printed.pages[0];
    let (ab, cd) = (page.texts.iter().find(|t| t.glyphs[0].1 == 'a').unwrap(), page.texts.iter().find(|t| t.glyphs[0].1 == 'c').unwrap());
    let first = all[0];
    assert_eq!((first.width, first.height), (30.0, 15.0));
    assert!((first.x - (ab.x + 12.0)).abs() < 0.01 && (cd.x - (first.x + 30.0)).abs() < 0.01);
    assert!((first.y + first.height - ab.y).abs() < 0.01 && ab.y == cd.y);
    // A width asked for keeps the picture's shape; the paragraph's place is the picture's.
    assert_eq!((all[1].width, all[1].height), (60.0, 30.0));
    assert!((all[1].x + 60.0 - 180.0).abs() < 0.01 && all[1].y >= first.y + first.height);
    // One larger than the page is made to fit it, and goes to a page with room.
    assert!((all[2].width - 160.0).abs() < 0.01 && (all[2].height - 160.0).abs() < 0.01);
    // One that cannot be printed is named.
    assert!(printed.pages.iter().any(|p| p.text().contains("[picture: gone]")));
    // In a cell it is moved with what else the cell holds: to the foot, beside the last line.
    let last_page = printed.pages.last().unwrap();
    let d = last_page.texts.iter().find(|t| t.glyphs[0].1 == 'd').unwrap();
    assert!(all[3].y + all[3].height <= d.y + 3.01 && all[3].y + all[3].height > d.y - 12.0, "{:?} {}", all[3], d.y);
}

#[test]
fn text_after_a_tab_stands_at_its_stop() {
    use mailcraft_formats::rich::{Tab, TabKind};
    let stop = |at, kind| Tab { at, kind, lead: Default::default() };
    // Stops are in pixels, four to three points: 30, 90 and 120 points from the start.
    let tabs = vec![stop(40, TabKind::Left), stop(120, TabKind::Right), stop(160, TabKind::Decimal)];
    let body = doc(vec![
        Block::Para(Para { tabs: tabs.clone(), ..para("a\tbb\tcc\t1.5") }),
        // Past the last stop, and where there are none: the stops every paragraph has.
        Block::Para(Para { tabs: vec![stop(4, TabKind::Left)], ..para("dddd\tee") }),
        // Text too long to end at its stop starts where the tab is.
        Block::Para(Para { tabs: vec![stop(16, TabKind::Right)], ..para("f\tgggggg") }),
        // Every line of a paragraph starts again.
        Block::Para(Para { tabs, ..para("h\tii\nj\tkk") }),
    ]);
    let printed = print(&body, Paper::A4);
    let page = &printed.pages[0];
    let find = |text: &str| {
        page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap_or_else(|| panic!("{text}: {}", page.text()))
    };
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    let (a, bb, cc, num) = (find("a"), find("bb"), find("cc"), find("1.5"));
    assert!(near(bb.x, a.x + 30.0), "{} {}", bb.x, a.x);
    assert!(near(cc.x + cc.width, a.x + 90.0));
    assert!(near(num.x + num.width / 3.0, a.x + 120.0));
    assert!(a.y == bb.y && bb.y == cc.y && cc.y == num.y);
    let (d, e) = (find("dddd"), find("ee"));
    assert!(near(e.x, d.x + 36.0), "{} {}", e.x, d.x + d.width);
    let (f, g) = (find("f"), find("gggggg"));
    assert!(near(g.x, f.x + f.width));
    let (h, ii, j, kk) = (find("h"), find("ii"), find("j"), find("kk"));
    assert!(near(ii.x, h.x + 30.0) && near(kk.x, j.x + 30.0) && near(h.x, j.x) && j.y > h.y);

    // Hostile: stops out of any range, a tab at every character, tabs past the end of a line.
    let wild = vec![stop(0, TabKind::Right), stop(u16::MAX, TabKind::Decimal), stop(u16::MAX, TabKind::Center)];
    let body = doc(vec![
        Block::Para(Para { tabs: wild.clone(), ..para(&"a\t".repeat(3000)) }),
        Block::Para(Para { tabs: wild, kind: ParaKind::Pre, ..para(&format!("{}\t\t\tb.\t.\t", "word ".repeat(200))) }),
    ]);
    within(&print(&body, SMALL), SMALL);
}

#[test]
fn lines_are_as_tall_as_their_paragraph_says() {
    use mailcraft_formats::rich::Leading;
    let two = |leading| Block::Para(Para { leading, ..para("aa\nbb") });
    // Four paragraphs of two lines: the step from the first line to the second.
    let body = doc(vec![two(Leading::Own), two(Leading::Times(200)), two(Leading::Exactly(40)), two(Leading::AtLeast(4)), two(Leading::Exactly(4))]);
    let printed = print(&body, Paper::A4);
    let page = &printed.pages[0];
    let ys = |text: &str| page.texts.iter().filter(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).map(|t| t.y).collect::<Vec<f32>>();
    let (a, b) = (ys("aa"), ys("bb"));
    assert_eq!((a.len(), b.len()), (5, 5));
    let step: Vec<f32> = a.iter().zip(&b).map(|(a, b)| b - a).collect();
    let own = step[0];
    assert!(own > 1.0);
    assert!((step[1] - own * 2.0).abs() < 0.01, "{step:?}");
    assert!((step[2] - 30.0).abs() < 0.01, "{step:?}");
    assert!((step[3] - own).abs() < 0.01, "{step:?}");
    assert!((step[4] - 3.0).abs() < 0.01, "{step:?}");
    // What a line gains stands below its text: the paragraph after a doubled one starts lower.
    let gap = |n: usize| a[n] - b[n - 1];
    assert!((gap(2) - gap(1) - own).abs() < 0.01 && (gap(3) - gap(1) - (30.0 - own)).abs() < 0.01);

    // Hostile: heights out of any range, on a small sheet, with more lines than a page.
    for leading in [Leading::Times(0), Leading::Times(u16::MAX), Leading::AtLeast(u16::MAX), Leading::Exactly(u16::MAX), Leading::Exactly(0)] {
        let body = doc(vec![Block::Para(Para { leading, ..para(&"word ".repeat(400)) })]);
        within(&print(&body, SMALL), SMALL);
    }
}

#[test]
fn leaders_fill_the_room_of_tabs_and_lines_stand_between_paragraphs() {
    use mailcraft_formats::rich::{Leader, Line, Tab, TabKind};
    let stop = |at, lead| Tab { at, kind: TabKind::Right, lead };
    let edge = Edge { width: 1, color: None, line: Line::Solid };
    let between = Edge { width: 2, color: Some([200, 0, 0]), line: Line::Solid };
    let frame = Arc::new(Frame { edges: [edge; 4], padding: [4; 4], indent: 0, between, fill: None });
    let body = doc(vec![
        Block::Para(Para { tabs: vec![stop(160, Leader::Dots)], ..para("aa\t1") }),
        Block::Para(Para { tabs: vec![stop(160, Leader::None)], ..para("bb\t2") }),
        Block::Para(Para { tabs: vec![stop(160, Leader::Equals)], ..para("cc\t3") }),
        Block::Para(Para { frame: Some(frame.clone()), ..para("dd") }),
        Block::Para(Para { frame: Some(frame), ..para("ee") }),
    ]);
    let printed = print(&body, Paper::A4);
    let page = &printed.pages[0];
    let find = |text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap();
    // On the line of the letters, between the text before the tab and the text after it.
    let on_line = |before: &str, after: &str| {
        let (a, b) = (find(before), find(after));
        page.rules
            .iter()
            .filter(|r| r.from.1 == r.to.1 && r.from.0 > a.x + a.width && r.to.0 < b.x && (r.from.1 - a.y).abs() < a.size * 0.5)
            .collect::<Vec<_>>()
    };
    let dots = on_line("aa", "1");
    assert_eq!(dots.len(), 1);
    assert_eq!(dots[0].line, Line::Dotted);
    assert!(dots[0].to.0 - dots[0].from.0 > 80.0);
    assert!(on_line("bb", "2").is_empty());
    assert_eq!(on_line("cc", "3").len(), 2);

    // The line between two paragraphs in one border: across the border, between their texts.
    let (d, e) = (find("dd"), find("ee"));
    let lines: Vec<_> = page.rules.iter().filter(|r| r.color == [200, 0, 0]).collect();
    assert_eq!(lines.len(), 1);
    let line = lines[0];
    assert!(line.from.1 == line.to.1 && line.from.1 > d.y && line.to.1 < e.y - e.size * 0.5, "{line:?} {} {}", d.y, e.y);
    assert!(line.from.0 < d.x && line.to.0 > 400.0 && (line.width - 1.5).abs() < 0.01);

    // Hostile: lines of any width between many paragraphs, leaders with no room to stand in.
    let wild = Edge { width: u8::MAX, color: None, line: Line::Double };
    let frame = Arc::new(Frame { edges: [wild; 4], padding: [u16::MAX; 4], indent: u8::MAX, between: wild, fill: Some([1, 2, 3]) });
    let mut blocks: Vec<Block> = (0..300).map(|_| Block::Para(Para { frame: Some(frame.clone()), ..para("a") })).collect();
    blocks.push(Block::Para(Para { tabs: (1..40).map(|n| stop(n, Leader::ThickLine)).collect(), ..para(&"a\t".repeat(500)) }));
    blocks.push(Block::Para(Para { tabs: vec![stop(u16::MAX, Leader::Hyphens)], ..para("a\tb\t\t") }));
    within(&print(&doc(blocks), SMALL), SMALL);
}

#[test]
fn a_filled_box_is_behind_its_paragraphs_with_its_room_on_each_side() {
    let frame =
        Arc::new(Frame { edges: Default::default(), padding: [6, 10, 14, 30], indent: 0, between: Edge::default(), fill: Some([0, 51, 102]) });
    let boxed = |text: &str| Block::Para(Para { frame: Some(frame.clone()), ..para(text) });
    let body = doc(vec![Block::Para(para("before")), boxed("one"), boxed("two"), Block::Para(para("after"))]);
    let printed = print(&body, Paper::A4);
    let page = &printed.pages[0];
    let find = |text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap();
    let fills: Vec<_> = page.fills.iter().filter(|f| f.color == [0, 51, 102]).collect();
    assert_eq!(fills.len(), 1, "one fill behind both paragraphs");
    let fill = fills[0];
    let (plain, one, two, after) = (find("before"), find("one"), find("two"), find("after"));
    // The box is as wide as the text's room; its text stands in from the left by the room asked.
    assert!((fill.x - plain.x).abs() < 0.01 && (one.x - fill.x - 30.0 * 0.75).abs() < 0.01, "{} {} {}", fill.x, plain.x, one.x);
    assert!(fill.y < one.y - one.size * 0.5 && fill.y + fill.height > two.y + 14.0 * 0.75 - 0.01);
    assert!(fill.y > plain.y && fill.y + fill.height < after.y);

    // A box longer than a page is filled on each page it is on, inside the page's room.
    let many: Vec<Block> = (0..80).map(|i| boxed(&format!("line {i}"))).collect();
    let printed = print(&doc(many), SMALL);
    assert!(printed.pages.len() > 2);
    within(&printed, SMALL);
    for page in &printed.pages {
        let fills: Vec<_> = page.fills.iter().filter(|f| f.color == [0, 51, 102]).collect();
        assert_eq!(fills.len(), 1);
        assert!(fills[0].y >= 19.9 && fills[0].y + fills[0].height <= 280.1 && fills[0].height > 0.0);
    }
}

fn print_in(font: Vec<u8>, body: &Doc, paper: Paper) -> Printed {
    let library = Library::empty();
    let mut fonts = Fonts::new(&library);
    for family in ["Segoe UI", "Times New Roman", "Calibri"] {
        fonts.give(family, false, false, Face { data: font.clone(), index: 0 });
    }
    memo(&Memo { owner: "Owner", fields: &[], body, plain: false, sizes: &|_| None }, paper, &mut fonts).unwrap()
}

#[test]
fn pairs_a_face_kerns_stand_as_close_as_it_says() {
    // "AV" stands a tenth of the size closer, "Va" a twentieth, "ab" further apart.
    let pairs = [('A', 'V', -100), ('V', 'a', -50), ('a', 'b', 30), ('V', 'A', 0)];
    let body = doc(vec![Block::Para(para("AVab VA")), Block::Para(Para { align: Align::Right, ..para("xAV") }), Block::Para(para("A\tV"))]);
    let plain = print(&body, Paper::A4);
    for gpos in [false, true] {
        let printed = print_in(boxes::font_kerned(CHARS, &pairs, gpos), &body, Paper::A4);
        let page = &printed.pages[0];
        let find = |page: &Page, text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap().clone();
        let (set, was) = (find(page, "AVab VA"), find(&plain.pages[0], "AVab VA"));
        // The body is set at 12 points: a box is 6 wide, and a thousandth of the size 0.012.
        let near = |a: f32, b: f32| (a - b).abs() < 0.001;
        assert!(was.kerns.is_empty() && near(was.width, 6.0 * 6.0 + 3.0));
        assert_eq!(set.kerns.len(), 7, "gpos {gpos}");
        let want = [-1.2, -0.6, 0.36, 0.0, 0.0, 0.0, 0.0];
        assert!(set.kerns.iter().zip(want).all(|(k, w)| near(*k, w)), "gpos {gpos}: {:?}", set.kerns);
        assert!(near(set.width, was.width - 1.2 - 0.6 + 0.36), "{}", set.width);
        // Text set to the right ends where the room ends, kerned or not.
        let (right, right_was) = (find(page, "xAV"), find(&plain.pages[0], "xAV"));
        assert!(near(right.x + right.width, right_was.x + right_was.width) && near(right.width, right_was.width - 1.2));
        // Nothing is kerned across a tab.
        assert!(page.texts.iter().filter(|t| t.glyphs.len() == 1 && matches!(t.glyphs[0].1, 'A' | 'V')).all(|t| t.kerns.is_empty()));
        within(&printed, Paper::A4);
    }

    // More fits on a line that is kerned tighter: 26 boxes fill the small sheet, 27 with pairs.
    let tight: Vec<(char, char, i16)> = vec![('A', 'V', -200), ('V', 'A', -200)];
    let word = "AV".repeat(14);
    let body = doc(vec![Block::Para(para(&word))]);
    assert_eq!(body_lines(&print(&body, SMALL).pages[0]).len(), 2);
    let printed = print_in(boxes::font_kerned(CHARS, &tight, true), &body, SMALL);
    assert_eq!(body_lines(&printed.pages[0]), std::slice::from_ref(&word));
    within(&printed, SMALL);

    // A pair in two sizes or two looks is not one pair.
    let two = Para { runs: vec![run("A"), Run { style: Style { size: Some(20.0), ..Style::default() }, ..run("V") }], ..Para::default() };
    let printed = print_in(boxes::font_kerned(CHARS, &pairs, false), &doc(vec![Block::Para(two)]), Paper::A4);
    assert!(printed.pages[0].texts.iter().all(|t| t.kerns.is_empty()));
}

#[test]
fn hostile_kerning_tables_move_nothing_far() {
    let body = doc(vec![Block::Para(para("AVAVAV abc VA")), Block::Para(para(&"AV".repeat(400)))]);
    let whole = boxes::font_kerned(CHARS, &[('A', 'V', i16::MIN), ('V', 'A', i16::MAX)], true);
    // Amounts beyond reason are held to half an em, and no glyph moves back past its start.
    let printed = print_in(whole.clone(), &body, SMALL);
    within(&printed, SMALL);
    for text in printed.pages.iter().flat_map(|p| &p.texts) {
        assert!(text.width >= -0.001 && text.kerns.iter().all(|k| k.abs() <= text.size / 2.0 + 0.001), "{text:?}");
    }
    // Tables that are cut short, or filled with anything, are read as far as they hold.
    let kern = boxes::font_kerned(CHARS, &[('A', 'V', -100)], false);
    for font in [&whole, &kern] {
        let tag: &[u8; 4] = if std::ptr::eq(font, &whole) { b"GPOS" } else { b"kern" };
        let record = (0..usize::from(u16::from_be_bytes([font[4], font[5]]))).map(|i| 12 + i * 16).find(|at| &font[*at..*at + 4] == tag).unwrap();
        let start = u32::from_be_bytes(font[record + 8..record + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(font[record + 12..record + 16].try_into().unwrap()) as usize;
        for cut in 0..len {
            let mut short = font.clone();
            short[record + 12..record + 16].copy_from_slice(&(cut as u32).to_be_bytes());
            within(&print_in(short, &body, SMALL), SMALL);
        }
        for (step, fill) in [(1usize, 0xFFu8), (2, 0x00), (3, 0x7F), (5, 0x01), (7, 0x80)] {
            let mut wild = font.clone();
            for at in (start..start + len).step_by(step) {
                wild[at] = fill;
            }
            within(&print_in(wild, &body, SMALL), SMALL);
        }
    }
    for junk in [vec![], vec![0u8; 3], vec![0xFF; 64], vec![0, 1, 0, 0, 0, 10, 0, 10, 0, 10, 0xFF, 0xFF], (0..=255u8).collect::<Vec<u8>>()] {
        for tag in [*b"GPOS", *b"kern"] {
            within(&print_in(boxes::font_and(CHARS, vec![(tag, junk.clone())]), &body, SMALL), SMALL);
        }
    }
}

#[test]
fn text_stands_beside_a_picture_at_one_side() {
    use mailcraft_formats::rich::Float;
    let side = |id: &str, float: Float| Run { picture: Some(Picture::Attached(id.into())), float, ..Run::default() };
    let sizes = |picture: &Picture| match picture {
        // 80 by 40 pixels are 60 by 30 points; with its gap a picture takes 66 of the width.
        Picture::Attached(id) if id == "small" => Some((80, 40)),
        Picture::Attached(id) if id == "wide" => Some((100, 40)),
        Picture::Attached(id) if id == "tall" => Some((80, 160)),
        Picture::Attached(id) if id == "huge" => Some((u16::MAX, u16::MAX)),
        _ => None,
    };
    let print = |body: &Doc, paper: Paper| {
        print_on(&Library::empty(), &Memo { owner: "Owner", fields: &[], body, plain: false, sizes: &sizes }, paper).unwrap()
    };
    let words = |n: usize| vec!["word"; n].join(" ");
    let (left, right) = (36.0f32, 595.28f32 - 36.0);
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    let lines_of = |page: &Page| -> Vec<Text> { page.texts.iter().filter(|t| t.glyphs.first().is_some_and(|g| g.1 == 'w')).cloned().collect() };

    for float in [Float::Left, Float::Right] {
        let body = doc(vec![Block::Para(Para { runs: vec![side("small", float), run(&words(90))], ..Para::default() }), Block::Para(para("after"))]);
        let printed = print(&body, Paper::A4);
        let page = &printed.pages[0];
        assert_eq!(page.pictures.len(), 1);
        let picture = &page.pictures[0];
        let lines = lines_of(page);
        assert!(lines.len() >= 5, "{}", lines.len());
        // The picture is at its side, its top at the top of the first line (12 points tall,
        // with its baseline 9.6 down).
        assert_eq!((picture.width, picture.height), (60.0, 30.0));
        assert!(near(picture.x, if float == Float::Left { left } else { right - 60.0 }), "{picture:?}");
        assert!(near(picture.y, lines[0].y - 9.6), "{} {}", picture.y, lines[0].y);
        // Three lines start above its foot and are shorter by its width and the gap; the
        // fourth is under it, as wide as the page's room.
        for (n, line) in lines.iter().enumerate() {
            let (from, to) = match (n < 3, float) {
                (true, Float::Left) => (left + 66.0, right),
                (true, _) => (left, right - 66.0),
                (false, _) => (left, right),
            };
            assert!(near(line.x, from) && line.x + line.width <= to + 0.01, "line {n} of {float:?}: {} {}", line.x, line.width);
            // As many words as there is room for: one more would not fit.
            assert!(n + 1 == lines.len() || line.width + 27.0 > to - from, "line {n} of {float:?} is short");
        }
        assert!(lines[3].y > picture.y + picture.height);
        within(&printed, Paper::A4);
    }

    // A picture taller than its paragraph: what follows stands beside it too, until a
    // paragraph asks to start below it, or something that is no plain paragraph comes.
    let tall = |then: Block| {
        doc(vec![
            Block::Para(Para { runs: vec![side("tall", Float::Left), run("one")], ..Para::default() }),
            Block::Para(para("two")),
            then,
            Block::Para(para("end")),
        ])
    };
    let find = |page: &Page, text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap().clone();
    for then in [
        Block::Para(Para { clear: true, ..para("below") }),
        Block::Table(Table { rows: vec![vec![cell("below")]], ..Table::default() }),
        Block::Para(Para {
            frame: Some(Arc::new(Frame { edges: Default::default(), padding: [0; 4], indent: 0, between: Edge::default(), fill: Some([1, 2, 3]) })),
            ..para("below")
        }),
    ] {
        let printed = print(&tall(then), Paper::A4);
        let page = &printed.pages[0];
        let picture = &page.pictures[0];
        assert_eq!((picture.width, picture.height), (60.0, 120.0));
        let (one, two, below, end) = (find(page, "one"), find(page, "two"), find(page, "below"), find(page, "end"));
        assert!(near(one.x, left + 66.0) && near(two.x, left + 66.0) && two.y > one.y && two.y < picture.y + 120.0);
        assert!(below.x < left + 20.0 && below.y - 9.6 >= picture.y + 120.0 - 0.01, "{} {}", below.x, below.y);
        assert!(near(end.x, left) && end.y > below.y);
    }
    // Blocks that end beside a picture end under it: a cell is as tall as its picture, and
    // the next cell's text does not stand beside a picture that is not its own.
    let floated = Cell {
        blocks: vec![Block::Para(Para { runs: vec![side("tall", Float::Right), run("in")], ..Para::default() })],
        span: 1,
        rows: 1,
        ..Cell::default()
    };
    let body = doc(vec![
        Block::Table(Table { rows: vec![vec![floated, cell("next")]], width: TableWidth::Percent(100), ..Table::default() }),
        Block::Para(para("after")),
    ]);
    let printed = print(&body, Paper::A4);
    let page = &printed.pages[0];
    let (picture, next, after) = (&page.pictures[0], find(page, "next"), find(page, "after"));
    assert!(after.y - 9.6 >= picture.y + 120.0 - 0.01 && near(after.x, left));
    assert!(next.x >= picture.x + picture.width - 0.01 && next.x + next.width <= right + 0.01, "{picture:?} {} {}", next.x, next.width);

    // Where too little room would be left beside it, a picture stands in the line.
    let body = doc(vec![Block::Para(Para { runs: vec![side("wide", Float::Left), run("ab cd")], ..Para::default() })]);
    let printed = print(&body, SMALL);
    let (picture, ab) = (&printed.pages[0].pictures[0], find(&printed.pages[0], "ab cd"));
    assert!(near(picture.x, 20.0) && near(ab.x, 20.0 + 75.0) && near(picture.y + picture.height, ab.y));
    // With room for it, it stands at the side on the small sheet too.
    let body = doc(vec![Block::Para(Para { runs: vec![side("small", Float::Right), run(&words(12))], ..Para::default() })]);
    let printed = print(&body, SMALL);
    assert!(near(printed.pages[0].pictures[0].x, 180.0 - 60.0));
    assert!(lines_of(&printed.pages[0])[0].width <= 94.01);
    within(&printed, SMALL);

    // A picture that has no room left on its page goes to the next with its text.
    let mut blocks: Vec<Block> = (0..17).map(|i| Block::Para(para(&format!("line {i}")))).collect();
    blocks.push(Block::Para(Para { runs: vec![side("small", Float::Left), run("beside it")], ..Para::default() }));
    let printed = print(&doc(blocks), SMALL);
    assert_eq!(printed.pages.len(), 2);
    assert!(printed.pages[0].pictures.is_empty());
    let (picture, beside) = (&printed.pages[1].pictures[0], find(&printed.pages[1], "beside it"));
    assert!(near(picture.y, 20.0) && near(beside.x, 20.0 + 66.0) && near(beside.y, 20.0 + 9.6));

    // Hostile: pictures of any size at the sides of everything, nested, by the thousand.
    let mut blocks: Vec<Block> = Vec::new();
    for i in 0..400u16 {
        let id = ["small", "wide", "tall", "huge", "none"][usize::from(i % 5)];
        let float = if i % 2 == 0 { Float::Left } else { Float::Right };
        let runs = vec![
            run("x"),
            Run { extent: (i.wrapping_mul(977), i.wrapping_mul(331)), ..side(id, float) },
            side("tall", float),
            run(&words(usize::from(i % 7))),
        ];
        let p = Para {
            runs,
            clear: i % 11 == 0,
            indent: (i % 9) as u8,
            quote: (i % 4) as u8,
            align: if i % 3 == 0 { Align::Right } else { Align::Left },
            ..Para::default()
        };
        blocks.push(if i % 6 == 0 {
            Block::Table(Table {
                rows: vec![vec![Cell { blocks: vec![Block::Para(p.clone()), Block::Para(p)], span: 1, rows: 1, ..Cell::default() }, cell("c")]],
                ..Table::default()
            })
        } else {
            Block::Para(p)
        });
    }
    for paper in [SMALL, Paper::A4, Paper { width: 80.0, height: 90.0, margin: 30.0 }] {
        let printed = print(&doc(blocks.clone()), paper);
        assert!(!printed.pages.is_empty());
        for page in &printed.pages {
            for placed in &page.pictures {
                assert!(placed.width > 0.0 && placed.height > 0.0 && placed.x.is_finite() && placed.y.is_finite());
                assert!(placed.y + placed.height <= page.height + 0.01 && placed.y >= -0.01, "{placed:?}");
            }
        }
    }
}

#[test]
fn pages_are_set_up_as_asked() {
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    let words = |n: usize| vec!["word"; n].join(" ");
    // The glyphs on one baseline, with where they start and end.
    let on = |page: &Page, y: f32| -> Vec<(f32, f32, char)> {
        page.texts.iter().filter(|t| (t.y - y).abs() < 0.01).map(|t| (t.x, t.x + t.width, t.glyphs[0].1)).collect()
    };
    let said = |line: &[(f32, f32, char)]| line.iter().map(|g| g.2).collect::<String>();
    let set = |setup: &Setup, body: &Doc| {
        print_on(&Library::empty(), &Memo { owner: "Owner", fields: &[], body, plain: false, sizes: &|_| None }, setup).unwrap()
    };
    let part = |s: &str| s.to_string();
    let long = doc(vec![Block::Para(para(&words(900)))]);

    // A sheet on its long side, with a margin of its own at each edge.
    let setup = Setup {
        landscape: true,
        margins: [72.0, 50.0, 60.0, 40.0],
        header: [part("[Subject]"), part("[date] x"), part("p[ Page ].[PAGES]")],
        footer: [part("[User]"), String::new(), part("[Page of Pages]")],
        words: vec![(part("Subject"), part("[Page] of [Pages]")), (part("Date"), part("d")), (part("User"), part("me"))],
        ..Setup::from(Paper::A4)
    };
    let printed = set(&setup, &long);
    assert!(printed.pages.len() > 1);
    for (n, page) in printed.pages.iter().enumerate() {
        assert_eq!((page.width, page.height), (841.89, 595.28));
        // The header is one line in the room over the body: its parts at the left, in the
        // middle and at the right. What a name gives is not looked through for names.
        let head = on(page, 72.0 + 4.0 + 6.4);
        let pages = printed.pages.len();
        assert_eq!(said(&head), format!("[Page] of [Pages]d xp{}.{pages}", n + 1));
        let (left, middle, right) = (&head[..17], &head[17..20], &head[20..]);
        assert!(near(left[0].0, 40.0), "{left:?}");
        assert!(near(right.last().unwrap().1, 841.89 - 50.0), "{right:?}");
        assert!(near(middle[0].0 - 40.0, 841.89 - 50.0 - middle[2].1), "{middle:?}");
        let foot = on(page, 595.28 - 60.0 - 4.0);
        assert_eq!(said(&foot), format!("me{} of {pages}", n + 1));
        assert!(near(foot[0].0, 40.0) && near(foot.last().unwrap().1, 841.89 - 50.0));
        // The body keeps inside the margins, under the header and over the footer.
        for text in page.texts.iter().filter(|t| t.size > 8.0) {
            assert!(text.x >= 40.0 - 0.01 && text.x + text.width <= 841.89 - 50.0 + 0.01, "{text:?}");
            assert!(text.y > 72.0 + 16.0 && text.y <= 595.28 - 60.0 - 16.0 + 0.01, "{text:?}");
        }
    }

    // With no header and no footer the body has their room: it starts at the top margin and
    // goes down to the bottom one.
    let bare = Setup { header: Default::default(), footer: Default::default(), ..setup.clone() };
    let printed = set(&bare, &doc(vec![Block::Para(para(&words(3000)))]));
    let page = &printed.pages[0];
    assert!(page.texts.iter().all(|t| t.size > 8.0));
    let (top, bottom) = page.texts.iter().fold((f32::MAX, 0.0f32), |(a, b), t| (a.min(t.y), b.max(t.y)));
    assert!(top < 72.0 + 16.0 && bottom > 595.28 - 60.0 - 16.0 && bottom <= 595.28 - 60.0 + 0.01, "{top} {bottom}");
    // A sheet that is wider than tall already lies as it is, and upright is as given.
    let wide = Paper { width: 300.0, height: 200.0, margin: 20.0 };
    for landscape in [false, true] {
        let page = &set(&Setup { landscape, ..Setup::from(wide) }, &long).pages[0];
        assert_eq!((page.width, page.height), (300.0, 200.0));
    }
    assert_eq!(set(&Setup { landscape: true, ..Setup::from(SMALL) }, &long).pages[0].width, 300.0);

    // A paper alone is the sheet upright with its number at the foot, as before.
    assert_eq!(
        Setup::from(Paper::LETTER),
        Setup {
            paper: Paper::LETTER,
            landscape: false,
            margins: [36.0; 4],
            header: Default::default(),
            footer: [String::new(), part("[Page of Pages]"), String::new()],
            words: vec![]
        }
    );
    assert_eq!(Setup::default().paper, Paper::A4);
    for name in Paper::NAMES {
        let paper = Paper::named(&name.to_uppercase()).unwrap();
        assert_eq!(paper.name(), Some(name));
        assert!(paper.height > paper.width && paper.margin == 36.0);
    }
    assert!(Paper::named("foolscap").is_none() && SMALL.name().is_none());

    // Brackets that name nothing of ours stand as they are; a part longer than its share of
    // the line is cut, and the parts do not run into each other.
    let odd = Setup {
        header: [part("[nothing] [ [Page"), part("a]b[]"), part("[Page]]")],
        footer: [words(40), words(40), words(40)],
        ..Setup::from(SMALL)
    };
    let printed = set(&odd, &doc(vec![Block::Para(para("body"))]));
    let page = &printed.pages[0];
    assert!(said(&on(page, 20.0 + 4.0 + 6.4)).starts_with("[nothing]"));
    let roomy = set(&Setup { paper: Paper::A4, margins: [36.0; 4], ..odd.clone() }, &doc(vec![Block::Para(para("body"))]));
    assert_eq!(said(&on(&roomy.pages[0], 36.0 + 4.0 + 6.4)), "[nothing] [ [Pagea]b[]1]");
    let foot = on(page, 300.0 - 20.0 - 4.0);
    assert!(foot.len() > 6 && foot.windows(2).all(|w| w[1].0 >= w[0].1 - 0.01), "{foot:?}");
    assert!(near(foot[0].0, 20.0) && foot.last().unwrap().1 <= 180.01);
    within(&printed, SMALL);

    // Hostile: margins and sheets that are no sizes, headers of any length, names without end.
    let wild = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1e30, 1e30, 0.0, 5000.0];
    let brackets = "[".repeat(60_000) + &"]".repeat(60_000);
    let many: Vec<(String, String)> = (0..5000).map(|i| (format!("n{i}"), "[n0]".repeat(5000))).collect();
    for (i, a) in wild.iter().enumerate() {
        for b in wild {
            let setup = Setup {
                paper: Paper { width: *a, height: b, margin: *a },
                landscape: i % 2 == 0,
                margins: [*a, b, b, *a],
                header: [brackets.clone(), "[n0][n4999][Page]".repeat(4000), "\u{0}\n\t[Pages]".into()],
                footer: ["[n1]".repeat(9000), brackets.clone(), "x".repeat(100_000)],
                words: many.clone(),
            };
            let printed = set(&setup, &doc(vec![Block::Para(para(&words(60)))]));
            assert!(!printed.pages.is_empty());
            for page in &printed.pages {
                assert!(page.width >= 72.0 && page.height >= 72.0 && page.width <= 14_400.0 && page.height <= 14_400.0);
                for text in &page.texts {
                    assert!(text.x.is_finite() && text.y.is_finite() && text.x >= -0.01 && text.x + text.width <= page.width + 0.01, "{text:?}");
                    assert!(text.y >= 0.0 && text.y <= page.height, "{text:?}");
                }
            }
        }
    }
}
