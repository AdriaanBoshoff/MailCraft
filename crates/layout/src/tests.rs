use std::sync::Arc;

use mailcraft_fonts::{Face, Library, boxes};
use mailcraft_formats::rich::{Align, Block, Cell, Doc, Edge, Frame, Para, ParaKind, Picture, Run, Script, Style, Table, TableWidth, VAlign};

use super::*;

/// Every glyph is a box half as wide as the size, a space a quarter; a line is as tall as the size.
const CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789:[].- ";

/// A sheet 200 wide with 160 of room: 26 body characters to the line.
const SMALL: Paper = Paper { width: 200.0, height: 300.0, margin: 20.0 };

fn print_on(library: &Library, memo_: &Memo, paper: Paper) -> Result<Printed, LayoutError> {
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
        assert_eq!(page.text().lines().last().unwrap(), (n + 1).to_string());
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
    // A tab is room, a hard space a space; marks that draw nothing are not there.
    let (a, rest) = (find("a"), find("b cd"));
    assert!((rest.x - (a.x + 6.0 + 12.0)).abs() < 0.01);
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
    let frame = Arc::new(Frame { edges: [edge; 4], padding: (4, 4) });
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
    let frame = Arc::new(Frame { edges: [wild; 4], padding: (u16::MAX, u16::MAX) });
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
