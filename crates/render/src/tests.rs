use mailcraft_fonts::{Face, Library, boxes};
use mailcraft_formats::rich::{Block, Doc, Para, Run};
use mailcraft_layout::{Fill, Fonts, Memo, Paper, Placed, Rule, memo};

use super::*;

const CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789:[].- ";
const SMALL: Paper = Paper { width: 200.0, height: 300.0, margin: 20.0 };

fn printed(text: &str) -> Printed {
    let library = Library::empty();
    let mut fonts = Fonts::new(&library);
    for family in ["Segoe UI", "Times New Roman", "Calibri"] {
        fonts.give(family, false, false, Face { data: boxes::font(CHARS), index: 0 });
    }
    let body =
        Doc { blocks: vec![Block::Para(Para { runs: vec![Run { text: text.into(), ..Run::default() }], ..Para::default() })], ..Doc::default() };
    memo(&Memo { owner: "", fields: &[], body: &body, plain: false, sizes: &|_| None }, SMALL, &mut fonts).unwrap()
}

fn at(image: &Image, x: usize, y: usize) -> [u8; 3] {
    let i = (y * image.width + x) * 3;
    [image.rgb[i], image.rgb[i + 1], image.rgb[i + 2]]
}

fn none<'a>(_: &Picture) -> Option<Bitmap<'a>> {
    None
}

#[test]
fn a_page_is_drawn_as_it_was_laid_out() {
    let mut printed = printed("ab c");
    let body = printed.pages[0].texts.iter().find(|t| t.size == 12.0).unwrap().clone();
    assert_eq!(body.glyphs.len(), 4);
    let (x, y) = (body.x, body.y);
    {
        let page = &mut printed.pages[0];
        page.fills.push(Fill { x: 100.0, y: 100.0, width: 40.0, height: 20.0, color: [200, 30, 30], depth: 0 });
        page.rules.push(Rule { from: (20.0, 200.0), to: (180.0, 200.0), width: 2.0, color: [0, 0, 255], line: Line::Solid });
        page.rules.push(Rule { from: (20.0, 220.0), to: (180.0, 220.0), width: 2.0, color: [0, 0, 0], line: Line::Dashed });
        page.pictures.push(Placed { x: 100.0, y: 130.0, width: 20.0, height: 10.0, picture: Picture::Attached("p".into()) });
        page.pictures.push(Placed { x: 150.0, y: 130.0, width: 20.0, height: 10.0, picture: Picture::Attached("gone".into()) });
    }
    // Two pixels of green and see-through, side by side.
    let pixels = [0u8, 255, 0, 255, 9, 9, 9, 0];
    let pictures = |picture: &Picture| matches!(picture, Picture::Attached(id) if id == "p").then_some(Bitmap { width: 2, height: 1, rgba: &pixels });

    for scale in [1.0f32, 2.0, 3.5] {
        let image = page(&printed, 0, scale, &pictures).unwrap();
        assert_eq!((image.width, image.height), ((200.0 * scale).round() as usize, (300.0 * scale).round() as usize));
        assert_eq!(image.rgb.len(), image.width * image.height * 3);
        let px = |x: f32, y: f32| at(&image, (x * scale) as usize, (y * scale) as usize);
        // The test font's glyphs are boxes from 0.05 to 0.45 of the size across and 0.7 of it
        // tall, each 0.5 further on; a space is 0.25 wide and draws nothing.
        assert_eq!(px(x + 3.0, y - 4.0), [0, 0, 0], "a at {scale}");
        assert_eq!(px(x + 9.0, y - 4.0), [0, 0, 0], "b at {scale}");
        assert_eq!(px(x + 13.5, y - 4.0), [255, 255, 255], "the space at {scale}");
        assert_eq!(px(x + 18.0, y - 4.0), [0, 0, 0], "c at {scale}");
        assert_eq!(px(x + 3.0, y - 9.5), [255, 255, 255], "over a");
        assert_eq!(px(x + 3.0, y + 1.5), [255, 255, 255], "under a");
        // The room between two boxes is 1.2 points: a whole pixel of it only when drawn larger.
        assert!(scale < 2.0 || px(x + 5.9, y - 4.0) == [255, 255, 255], "between a and b at {scale}");
        // The fill, the lines and the picture are where the page has them.
        assert_eq!(px(120.0, 110.0), [200, 30, 30]);
        assert_eq!(px(141.5, 110.0), [255, 255, 255]);
        assert_eq!(px(100.0, 200.0), [0, 0, 255]);
        assert_eq!(px(100.0, 202.5), [255, 255, 255]);
        assert_eq!(px(23.0, 220.0), [0, 0, 0]);
        assert_eq!(px(29.0, 220.0), [255, 255, 255], "the gap of a dashed line");
        assert_eq!(px(35.0, 220.0), [0, 0, 0]);
        assert_eq!(px(105.0, 135.0), [0, 255, 0]);
        assert_eq!(px(115.0, 135.0), [255, 255, 255], "the part of the picture that is seen through");
        assert_eq!(px(160.0, 135.0), [255, 255, 255], "a picture there are no pixels for");
    }

    // Kerned glyphs stand as much closer; glyphs thickened for want of a bold face are wider.
    let mut moved = printed.clone();
    for text in moved.pages[0].texts.iter_mut().filter(|t| t.size == 12.0) {
        text.kerns = vec![-3.0, 0.0, 0.0, 0.0];
        text.bold = true;
    }
    let image = page(&moved, 0, 4.0, &none).unwrap();
    let px = |x: f32, y: f32| at(&image, (x * 4.0) as usize, (y * 4.0) as usize);
    assert_eq!(px(x + 6.0, y - 4.0), [0, 0, 0], "b, three points closer");
    assert_eq!(px(x + 9.5, y - 4.0), [255, 255, 255]);
    assert_eq!(px(x + 0.5, y - 4.0), [0, 0, 0], "a, thickened");
    let plain = page(&printed, 0, 4.0, &none).unwrap();
    assert_eq!(at(&plain, ((x + 0.3) * 4.0) as usize, ((y - 4.0) * 4.0) as usize), [255, 255, 255]);
    assert_ne!(at(&plain, ((x + 0.5) * 4.0) as usize, ((y - 4.0) * 4.0) as usize), [0, 0, 0]);

    assert!(page(&printed, 1, 1.0, &none).is_none());
    assert!(page(&Printed::default(), 0, 1.0, &none).is_none());
}

#[test]
fn hostile_pages_are_drawn_within_bounds() {
    let base = printed("abc abc");
    let wild = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1e30, 1e30, 0.0, -5.0, 1e-30];
    // A scale that is no scale, and sheets of any size.
    for scale in wild {
        let image = page(&base, 0, scale, &none).unwrap();
        assert!(image.width >= 1 && image.height >= 1 && image.width <= MAX_SIDE && image.height <= MAX_SIDE);
        assert_eq!(image.rgb.len(), image.width * image.height * 3);
    }
    for size in wild {
        let mut odd = base.clone();
        odd.pages[0].width = size;
        odd.pages[0].height = 14_400.0;
        if let Some(image) = page(&odd, 0, 8.0, &none) {
            assert!(image.width <= MAX_SIDE && image.height <= MAX_SIDE && image.rgb.len() == image.width * image.height * 3);
        }
    }
    // Everything on the page anywhere, of any size, pointing at faces and glyphs there are none of.
    let pixels = vec![7u8; 4 * 4 * 4];
    for (i, a) in wild.iter().enumerate() {
        for b in wild {
            let mut odd = base.clone();
            let page_ = &mut odd.pages[0];
            page_.fills.push(Fill { x: *a, y: b, width: b, height: *a, color: [1, 2, 3], depth: 0 });
            page_.fills.push(Fill { x: -1e6, y: -1e6, width: 2e6, height: 2e6, color: [1, 2, 3], depth: 0 });
            for line in [Line::Solid, Line::Dashed, Line::Dotted, Line::Double] {
                page_.rules.push(Rule { from: (*a, b), to: (b, *a), width: *a, color: [0, 0, 0], line });
                page_.rules.push(Rule { from: (-1e7, 10.0), to: (1e7, 10.0), width: 1e-4, color: [0, 0, 0], line });
            }
            page_.pictures.push(Placed { x: *a, y: b, width: b, height: *a, picture: Picture::Attached("p".into()) });
            page_.pictures.push(Placed { x: -1e6, y: -1e6, width: 3e6, height: 3e6, picture: Picture::Attached("p".into()) });
            for text in &mut page_.texts {
                text.x = *a;
                text.kerns = vec![b; 3];
            }
            let mut text = page_.texts[0].clone();
            text.x = 30.0;
            text.y = b;
            text.size = *a;
            text.face = i * 7;
            text.glyphs = (0..5000u32).map(|g| ((g * 131 % 65_536) as u16, 'x')).collect();
            page_.texts.push(text);
            for (w, h) in [(4u32, 4u32), (u32::MAX, u32::MAX), (0, 9), (4, 5)] {
                let pictures = |_: &Picture| Some(Bitmap { width: w, height: h, rgba: &pixels });
                let image = page(&odd, 0, 2.0, &pictures).unwrap();
                assert_eq!((image.width, image.height), (400, 600));
            }
        }
    }
    // A font file that is cut short or is no font draws no text and nothing else.
    for data in [Vec::new(), vec![0xFF; 64], boxes::font(CHARS)[..200].to_vec()] {
        let library = Library::empty();
        let mut fonts = Fonts::new(&library);
        fonts.give("Arial", false, false, Face { data: boxes::font(CHARS), index: 0 });
        let body =
            Doc { blocks: vec![Block::Para(Para { runs: vec![Run { text: "abc".into(), ..Run::default() }], ..Para::default() })], ..Doc::default() };
        let printed = memo(&Memo { owner: "", fields: &[], body: &body, plain: false, sizes: &|_| None }, SMALL, &mut fonts).unwrap();
        assert!(glyph_shapes(&data, 0, false, &[1, 2, 3]).is_none_or(|shapes| shapes.iter().all(Vec::is_empty)));
        assert!(page(&printed, 0, 1.0, &none).is_some());
    }
    assert_eq!(glyph_shapes(&boxes::font(CHARS), 0, true, &[0, 1, 60_000]).unwrap().iter().map(Vec::len).collect::<Vec<_>>(), [0, 1, 0]);
}
