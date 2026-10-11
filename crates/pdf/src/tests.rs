use super::*;
use mailcraft_fonts::boxes;
use std::collections::BTreeSet;

fn boxed(chars: &str) -> Arc<[u8]> {
    boxes::font(chars).into()
}

/// The streams of a file, unpacked, in the order they stand in it.
fn streams(file: &[u8]) -> Vec<Vec<u8>> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(start) = file[at..].windows(7).position(|w| w == b"stream\n").map(|p| at + p + 7) {
        let end = start + file[start..].windows(10).position(|w| w == b"\nendstream").unwrap();
        found.push(miniz_oxide::inflate::decompress_to_vec_zlib(&file[start..end]).unwrap());
        at = end + 10;
    }
    found
}

fn has(haystack: &[u8], needle: &str) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn a_file_holds_its_pages_text_and_fonts() {
    let mut w = Writer::new();
    let font = w.font(boxed("ab \u{c9}"), 0).unwrap();
    assert!(w.page(595.0, 842.0));
    w.fill(36.0, 36.0, 100.0, 20.0, [255, 230, 153]);
    w.line((36.0, 60.0), (136.0, 60.0), 1.5, [0, 0, 0], Stroke::Dashed);
    w.text(Piece {
        font,
        size: 12.0,
        x: 36.0,
        baseline: 50.0,
        color: [204, 0, 0],
        glyphs: &[(1, 'a'), (2, 'b'), (3, ' '), (1, 'a')],
        bold: false,
        slant: false,
    });
    assert!(w.page(595.0, 842.0));
    w.text(Piece { font, size: 9.0, x: 36.0, baseline: 50.0, color: [0, 0, 0], glyphs: &[(4, '\u{c9}')], bold: true, slant: true });
    let file = w.finish("A title: \u{e9}\n(x)").unwrap();

    assert!(file.starts_with(b"%PDF-1.7\n"));
    assert!(file.ends_with(b"%%EOF\n"));
    assert!(has(&file, "/Type /Catalog") && has(&file, "/Count 2") && has(&file, "/MediaBox [0 0 595 842]"));
    assert!(has(&file, "/Subtype /CIDFontType2") && has(&file, "/Encoding /Identity-H") && has(&file, "/FontFile2"));
    assert!(has(&file, "/BaseFont /AAAAAA+Font"));
    // Glyph widths in thousandths of the size: the box, the space.
    assert!(has(&file, "/W [1 [500] 2 [500] 3 [250] 4 [500]]"), "{}", String::from_utf8_lossy(&file));
    // The title as UTF-16, with nothing of a line break or a bracket that could end it early.
    assert!(has(&file, "/Title <FEFF00410020007400690074006C0065003A002000E9002800780029>"));

    // The table at the end points at every object.
    let text = String::from_utf8_lossy(&file);
    let table_at: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
    assert!(file[table_at..].starts_with(b"xref\n"));
    let table = String::from_utf8_lossy(&file[table_at..]);
    let entries: Vec<&str> = table.lines().skip(3).take_while(|l| l.ends_with(" n ")).collect();
    for (n, entry) in entries.iter().enumerate() {
        let start: usize = entry[..10].parse().unwrap();
        assert!(file[start..].starts_with(format!("{} 0 obj\n", n + 1).as_bytes()), "object {}", n + 1);
    }
    assert!(entries.len() >= 12);

    let unpacked = streams(&file);
    let page_one = unpacked.iter().find(|s| has(s, "re f")).unwrap();
    let page_one = String::from_utf8_lossy(page_one);
    assert!(page_one.contains("1 0.9 0.6 rg 36 786 100 20 re f"), "{page_one}");
    assert!(page_one.contains("0 0 0 RG 1.5 w [4.5 4.5] 0 d 36 782 m 136 782 l S"), "{page_one}");
    assert!(page_one.contains("BT /F0 12 Tf 0.8 0 0 rg 0 Tr 1 0 0 1 36 792 Tm <0001000200030001> Tj ET"), "{page_one}");
    let page_two = unpacked.iter().find(|s| has(s, "2 Tr")).unwrap();
    assert!(has(page_two, "1 0 0.2 1 36 792 Tm <0004> Tj"));
    let map = unpacked.iter().find(|s| has(s, "beginbfchar")).unwrap();
    assert!(has(map, "4 beginbfchar\n<0001> <0061>\n<0002> <0062>\n<0003> <0020>\n<0004> <00C9>\n"));
}

#[test]
fn only_the_glyphs_used_are_carried() {
    let all = "abcdefghijklmnopqrstuvwxyz\u{c9}";
    let data = boxed(all);
    let whole = font::file(&data, 0, &BTreeSet::new(), Embedding::Whole).unwrap();
    let none = font::file(&data, 0, &BTreeSet::new(), Embedding::Subset).unwrap();
    let one = font::file(&data, 0, &BTreeSet::from([5]), Embedding::Subset).unwrap();
    // The composed glyph (27) brings the glyph it is made of (1).
    let composed = font::file(&data, 0, &BTreeSet::from([27]), Embedding::Subset).unwrap();
    assert!(none.len() < one.len() && one.len() < composed.len() && composed.len() < whole.len());

    for (file, kept) in [(&none, vec![]), (&one, vec![5usize]), (&composed, vec![1, 27]), (&whole, (1..=27).collect())] {
        // Each is a font again, with every glyph number where it was.
        assert_eq!(mailcraft_fonts::embedding(file, 0), Some(Embedding::Subset));
        let m = font::measures(file, 0).unwrap();
        assert_eq!((m.units, m.advances.len(), m.advances[5]), (1000.0, 28, 500));
        let font = skrifa::FontRef::new(file).unwrap();
        use skrifa::raw::TableProvider;
        let loca = font.data_for_tag(skrifa::raw::types::Tag::new(b"loca")).unwrap().as_bytes().to_vec();
        let at = |i: usize| u32::from_be_bytes(loca[i * 4..i * 4 + 4].try_into().unwrap());
        for glyph in 0..28usize {
            assert_eq!(at(glyph) < at(glyph + 1), kept.contains(&glyph), "glyph {glyph} of {kept:?}");
        }
        // The file's words add up to the number the format names.
        let sum = file.chunks(4).fold(0u32, |s, c| s.wrapping_add(u32::from_be_bytes([c[0], c[1], c[2], c[3]])));
        assert_eq!(sum, 0xB1B0_AFBA);
    }
}

#[test]
fn fonts_that_may_not_be_embedded_are_refused() {
    let mut w = Writer::new();
    assert_eq!(w.font(boxes::font_with("ab", 0x0002).into(), 0), Err(PdfError::Font));
    assert_eq!(w.font(Arc::from(&b"not a font"[..]), 0), Err(PdfError::Font));
    assert_eq!(w.font(boxed("ab"), 9), Err(PdfError::Font));
    let whole = w.font(boxes::font_with("abc", 0x0100).into(), 0).unwrap();
    assert!(w.page(100.0, 100.0));
    w.text(Piece { font: whole, size: 10.0, x: 0.0, baseline: 10.0, color: [0, 0, 0], glyphs: &[(1, 'a')], bold: false, slant: false });
    let file = w.finish("").unwrap();
    // All three boxes are in the file, though one was used.
    let program = streams(&file).into_iter().find(|s| s.starts_with(&[0, 1, 0, 0])).unwrap();
    assert_eq!(program.len(), font::file(&boxes::font_with("abc", 0x0100), 0, &BTreeSet::new(), Embedding::Whole).unwrap().len());
}

#[test]
fn hostile_drawing_never_panics_and_stays_a_file() {
    let mut w = Writer::new();
    // Nothing is drawn before there is a page.
    w.fill(0.0, 0.0, 1.0, 1.0, [0; 3]);
    w.text(Piece { font: FontId(7), size: 1.0, x: 0.0, baseline: 0.0, color: [0; 3], glyphs: &[(1, 'a')], bold: false, slant: false });
    let font = w.font(boxed("a"), 0).unwrap();
    assert!(w.page(f32::NAN, -5.0));
    assert!(w.page(f32::INFINITY, 1e30));
    for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1e30, 1e30, -0.0, 1e-30] {
        w.fill(v, v, v, v, [1, 2, 3]);
        w.line((v, v), (v, v), v, [1, 2, 3], Stroke::Dotted);
        w.text(Piece {
            font,
            size: v,
            x: v,
            baseline: v,
            color: [0; 3],
            glyphs: &[(65535, '\u{10ffff}'), (0, '\0'), (9, ')')],
            bold: true,
            slant: true,
        });
        w.text(Piece { font: FontId(usize::MAX), size: v, x: v, baseline: v, color: [0; 3], glyphs: &[], bold: false, slant: false });
    }
    let long: Vec<(u16, char)> = (0..100_000u32).map(|i| ((i % 7) as u16, 'a')).collect();
    w.text(Piece { font, size: 10.0, x: 0.0, baseline: 0.0, color: [0; 3], glyphs: &long, bold: false, slant: false });
    let file = w.finish(&"\u{202e}(\\)>".repeat(10_000)).unwrap();
    for page in streams(&file).iter().filter(|s| has(s, " Tj ET")) {
        let ops = String::from_utf8_lossy(page);
        assert!(!ops.contains("NaN") && !ops.contains("inf") && !ops.contains("e+") && !ops.contains("e-"), "numbers stay plain");
        assert!(ops.lines().all(|l| l.len() < 20_000));
    }
    assert!(has(&file, "/MediaBox [0 0 1 1]") && has(&file, "/MediaBox [0 0 1 14400]"));

    let mut many = Writer::new();
    for _ in 0..MAX_PAGES {
        assert!(many.page(10.0, 10.0));
    }
    assert!(!many.page(10.0, 10.0));
    for _ in 0..MAX_FONTS {
        many.font(boxed("a"), 0).unwrap();
    }
    assert_eq!(many.font(boxed("a"), 0), Err(PdfError::TooManyFonts));
}

#[test]
fn damaged_fonts_never_panic() {
    let good = boxes::font("abc\u{c9}");
    let used = BTreeSet::from([0, 1, 4, 9, 65535]);
    for cut in (0..good.len()).step_by(7) {
        let part = &good[..cut];
        let _ = font::measures(part, 0);
        let _ = font::file(part, 0, &used, Embedding::Subset);
        let _ = font::file(part, 0, &used, Embedding::Whole);
    }
    // Every byte in turn set to its worst: offsets, counts and flags among them.
    for at in 0..good.len() {
        for worst in [0xffu8, 0x80, 0x00] {
            let mut bad = good.clone();
            bad[at] = worst;
            let _ = font::measures(&bad, 0);
            if let Some(file) = font::file(&bad, 0, &used, Embedding::Subset) {
                // At most a place in `loca` for each of the glyphs the font claims to have.
                assert!(file.len() <= good.len() + 65_537 * 4);
            }
            let mut w = Writer::new();
            if let Ok(font) = w.font(bad.into(), 0) {
                w.page(100.0, 100.0);
                w.text(Piece {
                    font,
                    size: 10.0,
                    x: 0.0,
                    baseline: 0.0,
                    color: [0; 3],
                    glyphs: &[(4, 'x'), (60000, 'y')],
                    bold: false,
                    slant: false,
                });
                let _ = w.finish("t");
            }
        }
    }
    // A glyph made of itself, and of glyphs that are not there.
    let mut looped = good.clone();
    let glyf_at = looped.windows(4).position(|w| w == [0xff, 0xff, 0x00, 0x32]).unwrap();
    looped[glyf_at + 12..glyf_at + 14].copy_from_slice(&4u16.to_be_bytes());
    looped[glyf_at + 10..glyf_at + 12].copy_from_slice(&0x0022u16.to_be_bytes());
    assert!(font::file(&looped, 0, &BTreeSet::from([4]), Embedding::Subset).is_some());
}
