//! Pictures of the kinds added after the first four: WebP, TIFF and SVG, pictures that move,
//! and photographs that say which way up they are. Every file here is made in the test.

use crate::pictures::{decode, film};
use image::{ImageEncoder, Rgba, RgbaImage};
use serde_json::json;

/// Red at the left, blue at the right, see-through in the last column.
fn flag(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, _| match x {
        x if x + 1 == width => Rgba([0, 0, 0, 0]),
        x if x < width / 2 => Rgba([255, 0, 0, 255]),
        _ => Rgba([0, 0, 255, 255]),
    })
}

fn pixel(pixels: &crate::pictures::Pixels, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * pixels.width + x) * 4) as usize;
    pixels.rgba[at..at + 4].try_into().unwrap()
}

/// A GIF of `frames`, each one colour and shown as long as its number says, in hundredths.
fn moving(width: u16, height: u16, frames: &[([u8; 3], u16)], times: Option<u16>) -> Vec<u8> {
    use image::codecs::gif::{GifEncoder, Repeat};
    let mut file = Vec::new();
    {
        let mut encoder = GifEncoder::new_with_speed(&mut file, 30);
        encoder.set_repeat(times.map_or(Repeat::Infinite, Repeat::Finite)).unwrap();
        for (colour, hundredths) in frames {
            let whole = RgbaImage::from_pixel(u32::from(width), u32::from(height), Rgba([colour[0], colour[1], colour[2], 255]));
            let delay = image::Delay::from_numer_denom_ms(u32::from(*hundredths) * 10, 1);
            encoder.encode_frame(image::Frame::from_parts(whole, 0, 0, delay)).unwrap();
        }
    }
    file
}

#[test]
fn webp_and_tiff_pictures_are_read() {
    let picture = flag(12, 6);
    let mut webp = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut webp).write_image(&picture, 12, 6, image::ExtendedColorType::Rgba8).unwrap();
    let mut tiff = std::io::Cursor::new(Vec::new());
    image::codecs::tiff::TiffEncoder::new(&mut tiff).write_image(&picture, 12, 6, image::ExtendedColorType::Rgba8).unwrap();
    for (kind, file) in [("webp", webp), ("tiff", tiff.into_inner())] {
        let pixels = decode(&file).unwrap_or_else(|| panic!("{kind}"));
        assert_eq!((pixels.width, pixels.height, pixels.natural), (12, 6, (12, 6)), "{kind}");
        assert_eq!(pixels.rgba, picture.as_raw().as_slice(), "{kind}: to the last pixel");
        assert!(film(&file).is_none(), "{kind} stands still");
        // Cut short anywhere, or with any one part damaged: dropped or shown, never a panic.
        for cut in 0..file.len() {
            let _ = decode(&file[..cut]);
            let mut bent = file.clone();
            bent[cut] ^= 0xA5;
            let _ = decode(&bent);
            let _ = film(&bent);
        }
    }
    // A header that claims more than is ever decoded: a TIFF 60000 pixels each way.
    let mut huge = b"II*\0\x08\0\0\0".to_vec();
    let entries: [(u16, u16, u32); 8] =
        [(256, 4, 60000), (257, 4, 60000), (258, 3, 8), (259, 3, 1), (262, 3, 1), (273, 4, 200), (278, 4, 60000), (279, 4, 16)];
    huge.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, kind, value) in entries {
        huge.extend_from_slice(&tag.to_le_bytes());
        huge.extend_from_slice(&kind.to_le_bytes());
        huge.extend_from_slice(&1u32.to_le_bytes());
        huge.extend_from_slice(&value.to_le_bytes());
    }
    huge.extend_from_slice(&[0; 128]);
    assert!(decode(&huge).is_none());
}

#[test]
fn a_photograph_is_shown_the_way_up_it_was_taken() {
    // Exif says the camera was turned: orientation 6, the top of the picture is at the right.
    let mut exif = b"II*\0\x08\0\0\0\x01\0".to_vec();
    exif.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0]);
    let wide = image::RgbImage::from_fn(32, 16, |x, _| if x < 16 { image::Rgb([255, 255, 255]) } else { image::Rgb([0, 0, 0]) });
    let photograph = |exif: Option<Vec<u8>>| {
        let mut file = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 95);
        if let Some(exif) = exif {
            encoder.set_exif_metadata(exif).unwrap();
        }
        encoder.write_image(&wide, 32, 16, image::ExtendedColorType::Rgb8).unwrap();
        file
    };
    let plain = decode(&photograph(None)).unwrap();
    assert_eq!((plain.width, plain.height, plain.natural), (32, 16, (32, 16)));
    let turned = decode(&photograph(Some(exif))).unwrap();
    assert_eq!((turned.width, turned.height, turned.natural), (16, 32, (16, 32)), "its sides change places");
    // What was at the left is at the top.
    assert!(pixel(&turned, 8, 4)[0] > 200 && pixel(&turned, 8, 28)[0] < 50, "{:?} {:?}", pixel(&turned, 8, 4), pixel(&turned, 8, 28));
    // Exif that is not Exif changes nothing and breaks nothing.
    for junk in [vec![], vec![0xFF; 40], b"II*\0\xFF\xFF\xFF\xFF".to_vec(), b"MM\0*\0\0\0\x08\xFF\xFF".to_vec()] {
        let same = decode(&photograph(Some(junk))).unwrap();
        assert_eq!((same.width, same.height), (32, 16));
    }
}

#[test]
fn an_svg_drawing_is_drawn_and_nothing_in_it_is_run_or_fetched() {
    let head = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink""#;
    let drawing = format!(
        r##"<?xml version="1.0"?>{head} width="20" height="10"><rect width="10" height="10" fill="#ff0000"/><text x="11" y="8" font-size="6">ab</text></svg>"##
    );
    let pixels = decode(drawing.as_bytes()).expect("a drawing");
    // It has no pixels of its own: it is said to be its own size, and kept twice as large.
    assert_eq!((pixels.natural, pixels.width, pixels.height), ((20, 10), 40, 20));
    assert_eq!(pixel(&pixels, 8, 8), [255, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 30, 1)[3], 0, "nothing is drawn there, and it is see-through");
    assert!(film(drawing.as_bytes()).is_none());
    // With a mark before it and space, and with no size but a box.
    let boxed = format!("\u{feff}\n  {head} viewBox=\"0 0 4 2\"><circle cx=\"1\" cy=\"1\" r=\"1\" fill=\"#00f\"/></svg>");
    let pixels = decode(boxed.as_bytes()).expect("a drawing with a box");
    assert_eq!(pixels.natural.0, pixels.natural.1 * 2);
    assert!(pixels.rgba.chunks_exact(4).any(|p| p == [0, 0, 255, 255]));
    // One far larger than is kept is drawn smaller, in its shape.
    let vast = format!("{head} width=\"1000000\" height=\"500000\"><rect width=\"1000000\" height=\"500000\" fill=\"#0f0\"/></svg>");
    let pixels = decode(vast.as_bytes()).expect("a vast drawing");
    assert_eq!((pixels.natural, pixels.width, pixels.height), ((8192, 4096), 2048, 1024));
    assert_eq!(pixel(&pixels, 2000, 1000), [0, 255, 0, 255]);

    // A picture a drawing refers to is not read: not a file of this machine, not an address,
    // not one written into it. What is left draws nothing, and is no picture.
    let mut own = std::io::Cursor::new(Vec::new());
    image::codecs::png::PngEncoder::new(&mut own).write_image(&flag(4, 4), 4, 4, image::ExtendedColorType::Rgba8).unwrap();
    use std::fmt::Write;
    let mut written = String::new();
    for byte in own.into_inner() {
        write!(written, "%{byte:02X}").unwrap();
    }
    let here = std::env::temp_dir().join(format!("omc-svg-{}.png", std::process::id()));
    flag(4, 4).save(&here).unwrap();
    for href in [
        here.display().to_string(),
        format!("file:///{}", here.display()),
        "https://example.invalid/a.png".to_string(),
        format!("data:image/png,{written}"),
    ] {
        let refers = format!("{head} width=\"8\" height=\"8\"><image width=\"8\" height=\"8\" xlink:href=\"{href}\" href=\"{href}\"/></svg>");
        let seen = decode(refers.as_bytes());
        assert!(seen.is_none_or(|p| p.rgba.chunks_exact(4).all(|p| p[3] == 0)), "{href}");
    }
    let _ = std::fs::remove_file(&here);

    // Nothing to draw is no picture; a script is not a thing to draw.
    for empty in [
        "<svg/>".to_string(),
        format!("{head}/>"),
        format!("{head}><script>x()</script></svg>"),
        format!("{head} width=\"0\" height=\"9\"><rect width=\"9\" height=\"9\"/></svg>"),
    ] {
        assert!(decode(empty.as_bytes()).is_none(), "{empty}");
    }
    // Not a drawing at all, though it begins like XML or says the word.
    for other in ["<html><body>svg</body></html>", "<?xml version=\"1.0\"?><a/>", "svg", "<", "<svg", "\u{feff}"] {
        assert!(decode(other.as_bytes()).is_none(), "{other}");
    }
}

#[test]
fn hostile_drawings_are_refused_or_drawn_and_never_stop_the_program() {
    let head = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="40" height="40""#;
    // Each of ten groups holds ten of the one before: ten thousand million shapes, were they
    // all made.
    let mut many = format!("{head}><defs><g id=\"a0\"><rect width=\"1\" height=\"1\"/></g>");
    for level in 1..=10 {
        many.push_str(&format!("<g id=\"a{level}\">"));
        many.push_str(&format!("<use xlink:href=\"#a{}\"/>", level - 1).repeat(10));
        many.push_str("</g>");
    }
    many.push_str("</defs><use xlink:href=\"#a10\"/></svg>");
    // The same in the words of XML: names that stand for ten of the name before.
    let mut laughs = "<?xml version=\"1.0\"?><!DOCTYPE svg [<!ENTITY e0 \"ha\">".to_string();
    for level in 1..=10 {
        laughs.push_str(&format!("<!ENTITY e{level} \"{}\">", format!("&e{};", level - 1).repeat(10)));
    }
    laughs.push_str(&format!("]>{head}><text y=\"9\">&e10;</text></svg>"));
    // Groups inside one another, deeper than any reader follows; and shapes that use themselves.
    let deep = format!("{head}>{}<rect width=\"9\" height=\"9\"/>{}</svg>", "<g>".repeat(6000), "</g>".repeat(6000));
    let chain: String = (0..3000).map(|n| format!("<g id=\"c{n}\"><use xlink:href=\"#c{}\"/></g>", n + 1)).collect();
    let chained = format!("{head}><defs>{chain}<rect id=\"c3000\" width=\"9\" height=\"9\"/></defs><use xlink:href=\"#c0\"/></svg>");
    let itself = format!(
        "{head}><g id=\"s\"><use xlink:href=\"#s\"/><rect width=\"9\" height=\"9\"/></g><rect fill=\"url(#p)\" width=\"9\" height=\"9\"/><pattern id=\"p\" width=\"1\" height=\"1\"><rect fill=\"url(#p)\" width=\"1\" height=\"1\"/></pattern></svg>"
    );
    // Numbers that are not sizes, and work that is large for a small picture.
    let odd = format!(
        "{head} viewBox=\"0 0 1e38 -5\"><rect width=\"NaN\" height=\"1e39\" stroke-width=\"1e30\" stroke=\"red\" transform=\"scale(1e30) rotate(1e9)\"/><circle r=\"-1\"/><path d=\"M0 0 L 1e38 1e38 A 1e-40 1e40 0 1 1 5 5 Z\"/></svg>"
    );
    let blurred = format!(
        "{head}><filter id=\"f\" x=\"-1000\" y=\"-1000\" width=\"100000\" height=\"100000\"><feGaussianBlur stdDeviation=\"100000\"/><feTurbulence baseFrequency=\"9\" numOctaves=\"1000\"/></filter><rect width=\"40\" height=\"40\" filter=\"url(#f)\"/></svg>"
    );
    let began = std::time::Instant::now();
    for (what, drawing) in
        [("many", many), ("laughs", laughs), ("deep", deep), ("chained", chained), ("itself", itself), ("odd", odd), ("blurred", blurred)]
    {
        if let Some(pixels) = decode(drawing.as_bytes()) {
            assert!(pixels.width <= 2048 && pixels.height <= 2048, "{what}");
            assert_eq!(pixels.rgba.len(), (pixels.width * pixels.height * 4) as usize, "{what}");
        }
    }
    assert!(began.elapsed() < std::time::Duration::from_secs(40), "each is given up in a few seconds at most");
    // Cut short anywhere, and with a byte of any part changed.
    let good = format!(
        "{head}><defs><linearGradient id=\"g\"><stop offset=\"0\" stop-color=\"#f00\"/><stop offset=\"1\"/></linearGradient></defs><path d=\"M1 1 C 9 9 3 30 30 30 Z\" fill=\"url(#g)\" stroke=\"#00f\"/><text x=\"2\" y=\"20\" style=\"font-family:serif\">Hi &amp; bye</text></svg>"
    );
    assert!(decode(good.as_bytes()).is_some());
    for cut in (0..good.len()).step_by(7) {
        let _ = decode(&good.as_bytes()[..cut]);
        let mut bent = good.clone().into_bytes();
        bent[cut] = b"<>\"&#\xFF\0"[cut % 7];
        let _ = decode(&bent);
    }
    // Too large a file is not looked at.
    let mut heavy = format!("{head}><rect width=\"9\" height=\"9\"/><!--").into_bytes();
    heavy.resize((4 << 20) + 1, b'x');
    assert!(decode(&heavy).is_none());
}

#[test]
fn the_frames_of_a_picture_that_moves_are_read_with_their_times() {
    let file = moving(8, 4, &[([255, 0, 0], 0), ([0, 255, 0], 5), ([0, 0, 255], 20), ([9, 9, 9], 1)], Some(2));
    let read = film(&file).expect("four frames");
    assert_eq!((read.width, read.height, read.natural, read.loops), (8, 4, (8, 4), Some(2)));
    // A frame that asks for no time, or next to none, is given a tenth of a second.
    assert_eq!(read.frames.iter().map(|f| f.millis).collect::<Vec<_>>(), [100, 50, 200, 100]);
    assert!(read.frames.iter().all(|f| f.rgba.len() == 8 * 4 * 4));
    assert_eq!((&read.frames[1].rgba[..4], &read.frames[2].rgba[..4]), (&[0, 255, 0, 255][..], &[0, 0, 255, 255][..]));
    // Where it is shown still, as on paper, it is its first frame.
    let still = decode(&file).unwrap();
    assert_eq!(still.rgba, read.frames[0].rgba);
    assert_eq!(film(&moving(8, 4, &[([1, 2, 3], 5), ([3, 2, 1], 5)], None)).unwrap().loops, None, "without end");
    // One frame is a picture, not a film; so is any picture of a kind that does not move.
    let one = moving(8, 4, &[([255, 0, 0], 5)], None);
    assert!(film(&one).is_none() && decode(&one).is_some());
    assert!(film(b"GIF89a").is_none() && film(b"").is_none() && film(&[0x89; 64]).is_none());
    let mut png = std::io::Cursor::new(Vec::new());
    image::codecs::png::PngEncoder::new(&mut png).write_image(&flag(4, 4), 4, 4, image::ExtendedColorType::Rgba8).unwrap();
    assert!(film(&png.into_inner()).is_none());
    // Larger than a frame is kept, every frame is made smaller in its shape; too large a
    // canvas to play is not played, and the picture still stands.
    let large = film(&moving(1600, 400, &[([255, 0, 0], 5), ([0, 255, 0], 5)], None)).unwrap();
    assert_eq!((large.width, large.height, large.natural), (1024, 256, (1600, 400)));
    let vast = moving(2100, 2100, &[([255, 0, 0], 5), ([0, 255, 0], 5)], None);
    assert!(film(&vast).is_none());
    assert_eq!(decode(&vast).map(|p| p.natural), Some((2100, 2100)));
    // More frames than are kept: shown still.
    let frames: Vec<([u8; 3], u16)> = (0..401).map(|n| ([(n % 256) as u8, (n / 256) as u8, 7], 2)).collect();
    let long = moving(2, 2, &frames, None);
    assert!(film(&long).is_none() && decode(&long).is_some());
    assert_eq!(film(&moving(2, 2, &frames[..400], None)).unwrap().frames.len(), 400);
    // Cut short it plays as far as it can be read, or stands still; damaged, it never panics.
    let mut shortest = usize::MAX;
    for cut in 0..file.len() {
        if let Some(part) = film(&file[..cut]) {
            assert!((2..=4).contains(&part.frames.len()));
            shortest = shortest.min(part.frames.len());
        }
        let mut bent = file.clone();
        bent[cut] ^= 0x3C;
        if let Some(bent) = film(&bent) {
            assert!(bent.frames.iter().all(|f| f.rgba.len() == (bent.width * bent.height * 4) as usize && (20..=60_000).contains(&f.millis)));
        }
    }
    assert!(shortest < 4, "a film cut short was played in part");
}

#[test]
fn whether_pictures_move_is_a_command_and_is_kept() {
    let mut s = Session::with_sample();
    assert!(s.animate, "they move until the owner says otherwise");
    assert_eq!(s.run("view.animations", &json!({})).unwrap()["on"], json!(false));
    assert_eq!(crate::cmd::settings::current(&s, None)["animations"], json!(false));
    assert_eq!(s.run("view.animations", &json!({"on": true})).unwrap()["on"], json!(true));
    assert_eq!(s.run("view.animations", &json!({"on": "never"})).unwrap()["on"], json!(false), "what is not yes or no toggles");
}

use crate::Session;
