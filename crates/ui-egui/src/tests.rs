use super::*;

#[test]
fn ui_commands_take_junk_without_panicking() {
    let junk = [
        Value::Null,
        json!(7),
        json!("x"),
        json!([1, 2]),
        json!({ "tab": 9, "index": -1, "folderId": "a", "command": [], "menu": {}, "x": 1e300, "y": "n" }),
    ];
    let mut ui = MailUi::new(Session::with_sample());
    for command in UI_COMMANDS {
        for params in &junk {
            let _ = ui.execute(command.id, params);
        }
    }
    assert!(ui.execute("ui.nothing", &Value::Null).is_err());
}

#[test]
fn tabs_parse_by_label_and_dialogs_only_queue_requests() {
    assert_eq!(Tab::parse("Send / Receive"), Some(Tab::SendReceive));
    assert_eq!(Tab::parse("sendreceive"), Some(Tab::SendReceive));
    assert_eq!(Tab::parse("nope"), None);
    let mut ui = MailUi::new(Session::with_sample());
    ui.execute("ui.openDialog", &Value::Null).unwrap();
    ui.execute("ui.saveAttachmentDialog", &json!({ "index": 2 })).unwrap();
    ui.execute("ui.saveAllAttachmentsDialog", &Value::Null).unwrap();
    assert_eq!(ui.requests, vec![UiRequest::OpenFile, UiRequest::SaveAttachment(2), UiRequest::SaveAllAttachments]);
    ui.execute("ui.ribbonTab", &json!({ "tab": "view" })).unwrap();
    assert_eq!(ui.tab(), Tab::View);
}

#[test]
fn engine_commands_run_through_the_ui_and_untrusted_text_is_flattened() {
    let mut ui = MailUi::new(Session::with_sample());
    ui.execute("mail.select", &json!({ "index": 0 })).unwrap();
    assert!(ui.session.open.is_some());
    assert!(ui.execute("mail.move", &json!({ "folder": "No/Such" })).is_err());
    assert_eq!(one_line("a\r\nb\u{7}"), "a  b ");
    assert_eq!(one_line(&"x".repeat(5000)).len(), 400);
}

#[test]
fn theme_and_ribbon_mode_switch_by_command() {
    let mut ui = MailUi::new(Session::with_sample());
    assert_eq!(ui.ribbon_mode(), RibbonMode::Simplified);
    assert!(!ui.tokens.dark);
    ui.execute("ui.theme", &Value::Null).unwrap();
    assert!(ui.tokens.dark);
    ui.execute("ui.theme", &json!({ "theme": "dark" })).unwrap();
    assert!(ui.tokens.dark);
    ui.execute("ui.theme", &json!({ "theme": "light" })).unwrap();
    assert!(!ui.tokens.dark);
    assert!(ui.execute("ui.theme", &json!({ "theme": 3 })).is_err());
    ui.execute("ui.ribbonMode", &Value::Null).unwrap();
    assert_eq!(ui.ribbon_mode(), RibbonMode::Classic);
    ui.execute("ui.ribbonMode", &json!({ "mode": "simplified" })).unwrap();
    assert_eq!(ui.ribbon_mode(), RibbonMode::Simplified);
    assert!(ui.execute("ui.ribbonMode", &json!({ "mode": "sideways" })).is_err());
    // The two sets differ where it matters and share their sizes.
    let (light, dark) = (theme::Tokens::light(), theme::Tokens::dark());
    assert_ne!(light.surface, dark.surface);
    assert_ne!(light.text, dark.text);
    assert_eq!(light.folder_w, dark.folder_w);
}

#[test]
fn formatted_bodies_are_read_once_and_plain_reading_switches_by_command() {
    let mut ui = MailUi::new(Session::with_sample());
    assert!(ui.body_doc().is_none(), "nothing is open");
    ui.execute("mail.select", &json!({ "index": 0 })).unwrap();
    let doc = ui.body_doc().expect("the launch message has an HTML body");
    assert!(doc.text().contains("launch checklist"));
    assert!(Arc::ptr_eq(&doc, &ui.body_doc().unwrap()), "the same message is not read twice");
    ui.execute("mail.select", &json!({ "index": 1 })).unwrap();
    assert!(ui.body_doc().is_none(), "a plain-text message has no formatted body");
    ui.execute("mail.select", &json!({ "index": 4 })).unwrap();
    let depot = ui.body_doc().unwrap();
    assert_eq!(depot.remote_blocked, 2, "the logo and the tracking pixel are counted, not fetched");
    assert!(!depot.text().contains("front counter."), "the hidden preview line is not shown");
    ui.execute("ui.bodyFormat", &Value::Null).unwrap();
    assert!(ui.plain_bodies() && ui.body_doc().is_none());
    ui.execute("ui.bodyFormat", &json!({ "format": "formatted" })).unwrap();
    assert!(ui.body_doc().is_some());
    assert!(ui.execute("ui.bodyFormat", &json!({ "format": "html" })).is_err());
}

#[test]
fn hostile_bodies_draw_without_panicking() {
    let bodies = [
        "<table>".repeat(200) + "<tr><td>a<td>b" + &"</table>".repeat(200),
        "<table border=1><tr>".to_string() + &"<td>c</td>".repeat(3000) + "</tr></table>",
        "<p style=\"font-size:99999pt\">big</p><p style=\"font-size:0.0001pt\">small</p><sup><sub>x</sub></sup>".to_string(),
        "<blockquote>".repeat(300) + "deep" + &"<ul><li>".repeat(300) + "item",
        "<table><tr><td colspan=60000>wide</td><td>x</td></tr><tr><td></td></tr></table><hr><img src=cid:x alt=\"\u{202e}alt\">".to_string(),
        "<a href=\"javascript:alert(1)\">link</a> <font color=\"#ffffff\">white</font><span style=\"background:#000\">black</span>".to_string(),
        "<pre>".to_string() + &"long ".repeat(20_000) + "</pre>",
        // Tables with a width, a place and filled cells, at sizes no pane has.
        "<table width=99999 align=right><tr><td bgcolor=#000 width=4096>x<td colspan=9 bgcolor=red>y<table width=100%><tr><td>z</table></table>"
            .to_string(),
        "<table width=1 align=center border=1><tr><td width=0 bgcolor=#fff>a<td width=-5>b<td width=1e39>c</table>".to_string(),
        "<table style=\"max-width:600px;margin:0 auto\"><tr>".to_string() + &"<td width=4096 bgcolor=#123456>w</td>".repeat(400) + "</tr></table>",
        "<table width=600 align=center><tr><td bgcolor=#1f4e79>".repeat(40) + "deep",
        // Room in and between cells, shares of the width and places in the cell, all out of range.
        "<table border=1 cellpadding=99999 cellspacing=99999 width=40><tr><td style=\"padding:9999px\">a<td valign=bottom>b<br>c<td width=\"900%\">d</table>".to_string(),
        "<table width=\"1%\" cellspacing=64><tr>".to_string() + &"<td width=\"100%\" valign=bottom>s<br>t</td>".repeat(12) + "</tr></table>",
        "<table border=1><tr><td>a<br>b<td valign=bottom>".repeat(30) + "deep",
        "<table width=\"50%\"><tr><td width=\"0%\"><td colspan=64 width=\"100%\" valign=middle>x<td>y</table>".to_string(),
        // Pictures that are not here, at every size they might ask for.
        "<p>a<img src=\"https://x.example/a.png\" width=200 height=80 alt=\"\u{202e}alt text that is far longer than the frame it has to fit in\">b\
         <img src=\"https://x.example/b.png\" width=4096 height=4096><img src=cid:none width=24 height=24><img src=\"data:image/png;base64,AAAA\" width=30 height=99999>\
         <a href=\"https://x.example/\"><img src=\"https://x.example/c.png\" width=25 height=25 alt=linked></a></p>".to_string(),
        "<table><tr><td width=30><img src=\"https://x.example/a.png\" width=600 height=300><td bgcolor=#000><img src=\"https://x.example/a.png\" width=60 height=30></table>".to_string(),
        // Pictures in the line of text and at its sides, at every size, and fonts by hostile names.
        "<p>".to_string() + &"<img src=cid:none width=4096 height=4096 align=left>w <img src=\"https://x.example/a.png\" width=1 height=4096 align=right>".repeat(60) + "</p>",
        "<p align=right><img src=cid:none align=left width=40 height=4000>a<br><br>b<img src=cid:none width=4096 height=1>".to_string() + &"long words ".repeat(300) + "</p>",
        "<table width=20><tr><td><img src=cid:none align=right width=200 height=200>x<td><img src=\"data:image/png;base64,AAAA\" align=left>y</table>".to_string(),
        "<p style=\"font-family:\u{202e}'',,,;;..\\x.ttf\">a<font face=\"".to_string() + &"Zz,".repeat(4000) + "\">b</font><b style=\"font-family:serif\">c</b></p>",
        // Cells that reach down further than the table, borders and padding wider than the pane,
        // and pictures at the side of everything that can follow them.
        "<table border=1><tr><td rowspan=0>a<td rowspan=1000 colspan=900 valign=bottom>b<tr><td rowspan=7>c<br>d<tr><tr><td>e</table>".to_string(),
        "<table border=99 cellspacing=99 style=\"border:99px double red\"><tr><td rowspan=3 bgcolor=#ffe699 style=\"border:99px dotted\">a".to_string()
            + &"<tr><td rowspan=1000>r<br>s".repeat(200)
            + "</table>",
        "<div style=\"border:99999px dashed #fff;padding:99999px\"><p><img src=cid:none align=left width=300 height=4000>a</p>".to_string()
            + &"<p style=\"border-left:12px dotted\">words that wrap </p>".repeat(40)
            + "</div>",
        "<p><img src=cid:none align=right width=200 height=3000>a</p><table border=1 width=100%><tr><td rowspan=2>t<td>u<tr><td>v</table><hr><ul><li>".to_string()
            + &"item words ".repeat(400)
            + "</ul><br clear=all><p style=\"border:1px solid;border-bottom:12px double #010101\">after</p>",
        "<p><img src=cid:none align=left width=100 height=100>a</p>".repeat(80) + &"<p style=\"border:3px double\">b</p>".repeat(80),
    ];
    for dark in [false, true] {
        let tokens = theme::Tokens::named(dark);
        for html in &bodies {
            let doc = openmailclient_formats::html::parse(html);
            let ctx = egui::Context::default();
            install_fonts(&ctx);
            for width in [0.0, 30.0, 800.0] {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    ui.set_max_width(width);
                    body::draw(ui, &doc, &tokens, &pictures::Pictures::default(), &faces::Faces::default());
                });
                // No renderer here: the font texture updates have nowhere to go.
                output.textures_delta.clear();
            }
        }
    }
}

#[test]
fn an_rtf_body_is_read_formatted_and_drawn() {
    let mut ui = MailUi::new(Session::with_sample());
    ui.execute("mail.select", &json!({ "folder": "Inbox/Receipts", "index": 0 })).unwrap();
    let doc = ui.body_doc().unwrap();
    assert!(doc.blocks.iter().any(
        |b| matches!(b, openmailclient_formats::rich::Block::Table(t) if t.rows.len() == 5 && t.rows.iter().flatten().all(|c| c.edges.is_some()))
    ));
    assert!(doc.text().contains("parking.example.net/help"));
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        body::draw(ui, &doc, &theme::Tokens::named(true), &pictures::Pictures::default(), &faces::Faces::default());
    });
    output.textures_delta.clear();
    ui.execute("ui.bodyFormat", &json!({ "format": "plain" })).unwrap();
    assert!(ui.body_doc().is_none());
}

/// The picture in the sample order mail: a PNG our own writer made.
fn sample_picture() -> Vec<u8> {
    let mut s = Session::with_sample();
    s.run("mail.select", &json!({ "index": 4 })).unwrap();
    s.open.as_ref().unwrap().attachments[0].data.clone()
}

#[test]
fn a_picture_attached_to_the_message_is_decoded_and_drawn() {
    let mut ui = MailUi::new(Session::with_sample());
    ui.execute("mail.select", &json!({ "index": 4 })).unwrap();
    let doc = ui.body_doc().unwrap();
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let mut shown = pictures::Pictures::default();
    shown.load(&ctx, 7, &doc, ui.session.open.as_ref().unwrap());
    let proof = shown.get("proof-48213@printdepot.example.com").expect("the banner proof is decoded");
    assert_eq!(proof.size, egui::vec2(240.0, 96.0));
    assert_eq!(proof.texture.size(), [240, 96]);
    for width in [0.0, 30.0, 800.0] {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_max_width(width);
            body::draw(ui, &doc, &theme::Tokens::named(false), &shown, &faces::Faces::default());
        });
        output.textures_delta.clear();
    }

    // Content ids are matched as the message writes them: brackets and letter case do not matter.
    // A picture the message does not carry, or carries as something else, stays a placeholder.
    let mut item = openmailclient_model::Item::new_mail();
    for (name, id, data) in [
        ("a.png", "<Logo@Example>", sample_picture()),
        ("b.png", "notes", b"not a picture at all".to_vec()),
        ("c.svg", "drawing", b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>x()</script></svg>".to_vec()),
    ] {
        let mut a = openmailclient_model::Attachment::file(name, "image/png", data);
        a.props.set_str(pid::ATTACH_CONTENT_ID, id);
        item.attachments.push(a);
    }
    let html = "<p>a<img src=\"cid:logo@example\"> <img src=\"cid:logo@example\" width=5000><br>b</p><table><tr><td><img src=cid:notes alt=N>\
                <td><img src=cid:drawing><img src=cid:missing><img src=\"https://x.example/r.png\"></table>";
    let doc = openmailclient_formats::html::parse(html);
    let mut shown = pictures::Pictures::default();
    shown.load(&ctx, 1, &doc, &item);
    assert!(shown.get("logo@example").is_some());
    assert!(["notes", "drawing", "missing", "https://x.example/r.png"].iter().all(|id| shown.get(id).is_none()));
    for dark in [false, true] {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            body::draw(ui, &doc, &theme::Tokens::named(dark), &shown, &faces::Faces::default());
        });
        output.textures_delta.clear();
    }
    // Another message: nothing of the last one is kept.
    shown.load(&ctx, 2, &openmailclient_formats::html::parse("<p>text</p>"), &item);
    assert!(shown.get("logo@example").is_none());
}

#[test]
fn hostile_pictures_are_refused_not_decoded() {
    let good = sample_picture();
    assert!(pictures::decode(&good).is_some());
    // Cut short anywhere, or with any one part damaged, a picture is dropped or shown; never a panic.
    for cut in (0..good.len()).step_by(97) {
        let _ = pictures::decode(&good[..cut]);
        let mut bent = good.clone();
        bent[cut] ^= 0x5A;
        let _ = pictures::decode(&bent);
    }
    // A header that claims more pixels than are ever decoded.
    let mut huge = good.clone();
    huge[16..24].copy_from_slice(&[0, 0, 0xEA, 0x60, 0, 0, 0xEA, 0x60]);
    assert!(pictures::decode(&huge).is_none());
    let mut bmp = b"BM".to_vec();
    bmp.extend_from_slice(&[0; 12]);
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&0x7FFF_FFFFi32.to_le_bytes());
    bmp.extend_from_slice(&0x7FFF_FFFFi32.to_le_bytes());
    bmp.extend_from_slice(&[1, 0, 24, 0]);
    bmp.extend_from_slice(&[0; 24]);
    assert!(pictures::decode(&bmp).is_none());
    for junk in [&b""[..], b"GIF89a", b"\xFF\xD8\xFF\xE0", b"\x89PNG\r\n\x1a\n", b"<svg/>", &[0u8; 64][..]] {
        assert!(pictures::decode(junk).is_none());
    }
    assert!(pictures::decode(&vec![0x89; (16 << 20) + 1]).is_none(), "too large a file is not looked at");
}

#[test]
fn a_picture_is_sized_as_the_body_asks_and_never_wider_than_the_pane() {
    use body::picture_size;
    let natural = egui::vec2(400.0, 200.0);
    assert_eq!(picture_size((0, 0), natural, 800.0), natural);
    assert_eq!(picture_size((100, 0), natural, 800.0), egui::vec2(100.0, 50.0));
    assert_eq!(picture_size((0, 50), natural, 800.0), egui::vec2(100.0, 50.0));
    assert_eq!(picture_size((300, 30), natural, 800.0), egui::vec2(300.0, 30.0));
    assert_eq!(picture_size((0, 0), natural, 100.0), egui::vec2(100.0, 50.0));
    for hostile in [picture_size((4096, 4096), egui::vec2(1.0, 8192.0), 0.0), picture_size((1, 0), egui::vec2(0.0, 0.0), -5.0)] {
        assert!(hostile.x >= 1.0 && hostile.y >= 1.0 && hostile.y <= 4096.0 && hostile.x.is_finite());
    }
}

/// Base64 as a body would write a picture into itself.
fn written(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::from("data:image/png;base64,");
    for three in data.chunks(3) {
        let n = three.iter().fold(0u32, |n, b| (n << 8) | u32::from(*b)) << (8 * (3 - three.len()));
        for i in 0..4 {
            out.push(if i <= three.len() { char::from(ABC[(n >> (18 - 6 * i)) as usize & 63]) } else { '=' });
        }
    }
    out
}

#[test]
fn pictures_written_into_the_body_and_at_its_sides_are_decoded_and_drawn() {
    let good = written(&sample_picture());
    let html = format!(
        "<p>a <img src=\"{good}\"> b <img src=\"{good}\" width=24 height=24> c <img src=\"{}\" alt=broken> d</p>\
         <p><img src=\"{good}\" align=left>{}</p><p align=center><img src=\"{good}\" align=right width=4000>short</p>",
        written(b"not a picture at all"),
        "words beside the picture ".repeat(80),
    );
    let doc = openmailclient_formats::html::parse(&html);
    assert_eq!(doc.written.len(), 2);
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let mut shown = pictures::Pictures::default();
    shown.load(&ctx, 3, &doc, &openmailclient_model::Item::new_mail());
    assert_eq!(shown.written(0).map(|p| p.size), Some(egui::vec2(240.0, 96.0)));
    assert!(shown.written(1).is_none() && shown.written(usize::MAX).is_none());
    for dark in [false, true] {
        for width in [0.0, 100.0, 300.0, 900.0] {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_max_width(width);
                body::draw(ui, &doc, &theme::Tokens::named(dark), &shown, &faces::Faces::default());
            });
            output.textures_delta.clear();
        }
    }
    shown.load(&ctx, 4, &openmailclient_formats::html::parse("<p>text</p>"), &openmailclient_model::Item::new_mail());
    assert!(shown.written(0).is_none(), "nothing of the last message is kept");
}

#[test]
fn text_beside_a_picture_is_cut_where_the_picture_ends() {
    use openmailclient_formats::rich::{Picture, Run};
    let text = |t: &str| Run { text: t.to_string(), ..Run::default() };
    let runs = [text("one two\n"), Run { text: "alt".to_string(), picture: Some(Picture::Remote), ..Run::default() }, text("thre\u{301}e four")];
    let ends = [8, 9, 20];
    let texts = |runs: &[Run]| runs.iter().map(|r| r.text.clone()).collect::<Vec<_>>();
    let (head, tail) = body::split(&runs, &ends, 8);
    assert_eq!((texts(&head), texts(&tail)), (vec!["one two".to_string()], vec!["alt".to_string(), "thre\u{301}e four".to_string()]));
    let (head, tail) = body::split(&runs, &ends, 13);
    assert_eq!(texts(&head), ["one two\n", "alt", "thre"]);
    assert_eq!(texts(&tail), ["\u{301}e four"]);
    let (head, tail) = body::split(&runs, &ends, 0);
    assert!(head.is_empty() && tail.len() == 3);
    // Counts that do not belong to these runs cut nothing in the middle of a character.
    for (ends, chars) in [(&[0usize, 0, 0][..], usize::MAX), (&[usize::MAX, 1, 0][..], 5), (&[3][..], 2), (&[][..], 9), (&[9, 9, 9, 9, 9][..], 4)] {
        let (head, tail) = body::split(&runs, ends, chars);
        assert!(head.len() + tail.len() <= runs.len() + 1);
    }
}

#[test]
fn fonts_a_message_names_are_looked_up_and_hostile_names_draw_in_the_readers_own() {
    let html = "<p style=\"font-family:Arial\">a<b>b</b><i>i</i><b><i>bi</i></b></p><p style=\"font-family:'No Such Font 7f3a', sans-serif\">c</p>\
                <p style=\"font-family:..\\..\\Windows\\Fonts\\arial.ttf\">d</p><p style=\"font-family:monospace\">e</p>"
        .to_string()
        + &(0..200).map(|i| format!("<font face=\"Nosuch{i}, Other{i}\">f</font>")).collect::<String>();
    let doc = openmailclient_formats::html::parse(&html);
    let ctx = egui::Context::default();
    let mut shown = faces::Faces::new(install_fonts(&ctx));
    assert!(shown.pick("Arial", false, false).is_none(), "nothing is drawn with a font before it is looked up");
    // The fonts found are ready a frame after they are given; every frame draws.
    for _ in 0..3 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            shown.load(ui.ctx(), 1, &doc);
            body::draw(ui, &doc, &theme::Tokens::named(false), &pictures::Pictures::default(), &shown);
        });
        output.textures_delta.clear();
    }
    assert!(shown.pick("monospace", false, false).is_none());
    assert!(shown.pick("never listed", true, true).is_none());
    // Whatever this machine has, a family that is picked is one egui can draw with.
    let bound = ctx.fonts(|f| f.families());
    for list in ["arial", "'no such font 7f3a', sans-serif", "..\\..\\windows\\fonts\\arial.ttf"] {
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let picked = shown.pick(list, bold, italic);
            assert!(picked.as_ref().is_none_or(|p| bound.contains(&p.family) && (italic || !p.slant)), "{list}");
        }
    }
    // Another message with other names: still drawn, and the caps hold.
    let other = openmailclient_formats::html::parse("<p style=\"font-family:Georgia\">g</p><p style=\"font-family:\">h</p>");
    for _ in 0..2 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            shown.load(ui.ctx(), 2, &other);
            body::draw(ui, &other, &theme::Tokens::named(true), &pictures::Pictures::default(), &shown);
            body::draw(ui, &doc, &theme::Tokens::named(true), &pictures::Pictures::default(), &shown);
        });
        output.textures_delta.clear();
    }
}

/// The height a body takes when it is drawn `width` wide.
fn drawn_height(html: &str, width: f32) -> f32 {
    let doc = openmailclient_formats::html::parse(html);
    let tokens = theme::Tokens::named(false);
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let mut height = 0.0;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.set_max_width(width);
        let top = ui.cursor().top();
        body::draw(ui, &doc, &tokens, &pictures::Pictures::default(), &faces::Faces::default());
        height = ui.cursor().top() - top;
    });
    output.textures_delta.clear();
    height
}

#[test]
fn what_follows_a_picture_at_the_side_stands_beside_it_until_it_ends_or_is_cleared() {
    let picture = "<p><img src=cid:none align=left width=100 height=300>a</p>";
    let alone = drawn_height(picture, 600.0);
    assert!(alone >= 300.0, "the picture is given its room: {alone}");
    // Short paragraphs after the picture's own stand beside it and add no height.
    let beside = drawn_height(&format!("{picture}<p>b</p><p>c</p>"), 600.0);
    assert!((beside - alone).abs() < 1.0, "{beside} against {alone}");
    // A paragraph after a break that clears stands under the picture.
    let cleared = drawn_height(&format!("{picture}<br clear=all><p>b</p>"), 600.0);
    assert!(cleared > alone + 10.0, "{cleared} against {alone}");
    // A table beside the picture is narrower than one under it, so the same words take more rows.
    let words = "word ".repeat(120);
    let narrow = drawn_height(&format!("{picture}<table border=1 width=100%><tr><td>{words}</table>"), 600.0);
    let wide = drawn_height(&format!("<table border=1 width=100%><tr><td>{words}</table>"), 600.0);
    assert!(narrow > wide, "{narrow} against {wide}");
}

#[test]
fn borders_and_cells_over_several_rows_take_the_room_they_ask_for() {
    let plain = drawn_height("<p>a</p>", 600.0);
    let framed = drawn_height("<p style=\"border:4px solid red;padding:10px\">a</p>", 600.0);
    assert!((framed - plain - 28.0).abs() < 1.5, "two lines and two paddings: {framed} against {plain}");
    // One border round two paragraphs is drawn once, not once round each.
    let shared = drawn_height("<div style=\"border:4px solid red\"><p>a</p><p>b</p></div>", 600.0);
    let each = drawn_height("<p style=\"border:4px solid red\">a</p><p style=\"border:4px solid red\">b</p>", 600.0);
    assert!((each - shared - 8.0).abs() < 1.5, "{each} against {shared}");

    // A cell over two rows is as tall as both; a tall one makes the rows it covers tall enough.
    let two = drawn_height("<table border=1><tr><td>a<td>b<tr><td>c<td>d</table>", 600.0);
    let reach = drawn_height("<table border=1><tr><td rowspan=2>a<td>b<tr><td>d</table>", 600.0);
    assert!((two - reach).abs() < 1.0, "{two} against {reach}");
    let tall = drawn_height("<table border=1><tr><td rowspan=2>a<br>b<br>c<br>d<br>e<br>f<td>b<tr><td>d</table>", 600.0);
    let six = drawn_height("<table border=1><tr><td>a<br>b<br>c<br>d<br>e<br>f</table>", 600.0);
    assert!(tall > two && tall >= six - 1.0, "{tall} against {two} and {six}");
    // A thick border round the table takes room on both sides of it.
    let thick = drawn_height("<table border=8 cellspacing=0><tr><td>a</table>", 600.0);
    let thin = drawn_height("<table border=1 cellspacing=0><tr><td>a</table>", 600.0);
    assert!((thick - thin - 14.0).abs() < 1.5, "{thick} against {thin}");
}

#[test]
fn hostile_rtf_bodies_draw_without_panicking() {
    let hex: String = sample_picture().iter().map(|b| format!("{b:02x}")).collect();
    let bodies = [
        // Pictures at every size, in and out of cells, and one that is not a picture.
        format!(
            "{{\\rtf1\\pard a{{\\pict\\pngblip\\picwgoal99999999\\pichgoal15 {hex}}}{{\\pict\\pngblip\\picscalex1\\picscaley10000\\picwgoal300\\pichgoal300 {hex}}}\
             {{\\pict\\pngblip 00ff00ff}}\\par\\trowd\\cellx300\\cellx600\\intbl{{\\pict\\pngblip\\picwgoal9000\\pichgoal9000 {hex}}}\\cell x\\cell\\row}}"
        ),
        // Lines of every kind and width round cells merged down past the end of the table.
        "{\\rtf1\\trowd\\clbrdrt\\brdrdb\\brdrw9999\\clbrdrl\\brdrdot\\brdrw9999\\clbrdrb\\brdrdash\\brdrw9999\\clbrdrr\\brdrs\\brdrw9999\\clvmgf\\clvertalb\\cellx15\
         \\clbrdrb\\brdrdot\\clvmgf\\cellx30\\cellx9000\\intbl a\\line b\\cell c\\cell d\\cell\\row"
            .to_string()
            + &"\\trowd\\clvmrg\\cellx15\\clvmrg\\cellx30\\clbrdrt\\brdrdot\\brdrw15\\cellx9000\\intbl\\cell\\cell e\\line f\\cell\\row".repeat(300)
            + "}",
        "{\\rtf1\\trowd\\trgaph99999\\clvmrg\\cellx-500\\clvmrg\\cellx-900\\clvmgf\\cellx0\\intbl a\\cell b\\cell c\\cell\\row\\trowd\\clvmrg\\cellx0\\intbl\\cell\\row}".to_string(),
    ];
    let tokens = theme::Tokens::named(false);
    for rtf in &bodies {
        let doc = openmailclient_formats::rtf::parse(rtf.as_bytes());
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let mut shown = pictures::Pictures::default();
        shown.load(&ctx, 1, &doc, &openmailclient_model::Item::new_mail());
        assert!(shown.written(0).is_none_or(|p| p.size.x > 0.0));
        for width in [0.0, 30.0, 800.0] {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_max_width(width);
                body::draw(ui, &doc, &tokens, &shown, &faces::Faces::default());
            });
            output.textures_delta.clear();
        }
    }
}

#[test]
fn an_attachment_in_the_text_is_a_card_that_asks_to_be_saved() {
    use openmailclient_formats::rich::{Block, Doc, Filed, Para, Picture, Run};
    let file = |filed: Filed| Run { text: format!("[Attachment: {}]", filed.name), picture: Some(Picture::File(filed)), ..Run::default() };
    let doc_of = |runs: Vec<Run>| Doc { blocks: vec![Block::Para(Para { runs, ..Para::default() })], ..Doc::default() };
    let notes = Filed { number: Some(3), name: "notes.txt".into(), kind: None, bytes: 2048, caution: None };
    let doc = doc_of(vec![file(notes)]);
    let draw = |ctx: &egui::Context, doc: &Doc, events: Vec<egui::Event>| {
        let mut picked = None;
        let input = egui::RawInput { events, ..egui::RawInput::default() };
        let mut output = ctx.run_ui(input, |ui| {
            picked = body::draw(ui, doc, &theme::Tokens::named(false), &pictures::Pictures::default(), &faces::Faces::default()).file;
        });
        output.textures_delta.clear();
        picked
    };
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    // Only looked at: nothing is asked for.
    assert_eq!(draw(&ctx, &doc, Vec::new()), None);
    // The card stands at the start of the first line. A click on it asks for that attachment.
    let at = egui::pos2(30.0, 14.0);
    let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    assert_eq!(draw(&ctx, &doc, vec![egui::Event::PointerMoved(at)]), None);
    assert_eq!(draw(&ctx, &doc, vec![button(true)]), None);
    assert_eq!(draw(&ctx, &doc, vec![button(false)]), Some(3));

    // One that is no file of its own is shown and cannot be asked for.
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let object = doc_of(vec![file(Filed { number: None, name: "Card".into(), kind: Some("Permit card".into()), bytes: 0, caution: None })]);
    for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)]] {
        assert_eq!(draw(&ctx, &object, events), None);
    }

    // Whatever a message makes of its attachments, in any room and on any colour: no panic.
    let long = Filed {
        number: Some(usize::MAX),
        name: "\u{202e}x".repeat(5000),
        kind: Some("k\n".repeat(5000)),
        bytes: usize::MAX,
        caution: Some("c".repeat(100_000)),
    };
    let mut hostile = doc_of((0..40).map(|_| file(long.clone())).collect());
    let filled = openmailclient_formats::html::parse("<table><tr><td bgcolor=#203040>x</td></tr></table>");
    if let Some(Block::Table(table)) = filled.blocks.first() {
        let mut table = table.clone();
        table.rows[0][0].blocks = hostile.blocks.clone();
        hostile.blocks.push(Block::Table(table));
    }
    for dark in [false, true] {
        for width in [0.0, 30.0, 800.0] {
            let ctx = egui::Context::default();
            install_fonts(&ctx);
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_max_width(width);
                body::draw(ui, &hostile, &theme::Tokens::named(dark), &pictures::Pictures::default(), &faces::Faces::default());
            });
            output.textures_delta.clear();
        }
    }
}

#[test]
fn a_click_on_a_link_asks_and_only_the_answer_opens_it() {
    use openmailclient_formats::rich::{Block, Doc, Para, Run, Style};
    let link: std::sync::Arc<str> = "https://example.org/help".into();
    let run = Run { text: "A link with some words in it".into(), style: Style { link: Some(link.clone()), ..Style::default() }, ..Run::default() };
    let doc = Doc { blocks: vec![Block::Para(Para { runs: vec![run], ..Para::default() })], ..Doc::default() };
    let draw = |ctx: &egui::Context, events: Vec<egui::Event>| {
        let mut picked = None;
        let input = egui::RawInput { events, ..egui::RawInput::default() };
        let mut output = ctx.run_ui(input, |ui| {
            picked = body::draw(ui, &doc, &theme::Tokens::named(false), &pictures::Pictures::default(), &faces::Faces::default()).link;
        });
        output.textures_delta.clear();
        picked
    };
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let at = egui::pos2(40.0, 10.0);
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    // Looking at it and resting the pointer on it ask for nothing.
    assert_eq!(draw(&ctx, Vec::new()), None);
    assert_eq!(draw(&ctx, vec![egui::Event::PointerMoved(at)]), None);
    assert_eq!(draw(&ctx, vec![button(at, true)]), None);
    assert_eq!(draw(&ctx, vec![button(at, false)]), Some(link));
    // A click beside the text is on no link.
    let beside = egui::pos2(600.0, 10.0);
    assert_eq!(draw(&ctx, vec![egui::Event::PointerMoved(beside), button(beside, true)]), None);
    assert_eq!(draw(&ctx, vec![button(beside, false)]), None);

    // In the window: asking shows where the link goes and opens nothing.
    let mut ui = MailUi::new(Session::with_sample());
    assert!(ui.execute("ui.linkDialog", &json!({"index": 0})).is_err(), "no message is open");
    ui.execute("mail.select", &json!({"folder": "Junk Email", "index": 0})).unwrap();
    assert!(ui.execute("ui.linkDialog", &json!({"address": "https://example.com/not-in-the-message"})).is_err());
    assert!(ui.execute("ui.linkDialog", &json!({"index": 99})).is_err() && ui.execute("ui.linkDialog", &Value::Null).is_err());
    ui.execute("ui.linkDialog", &json!({"index": 1})).unwrap();
    let ask = ui.link_ask.as_ref().unwrap();
    assert_eq!(
        (ask.goes.as_deref(), ask.text.as_str(), ask.mailbox, ask.refused.is_none()),
        (Some("sign-in.example.net"), "mail.example.org", false, true)
    );
    assert!(ask.concern.as_ref().is_some_and(|c| c.contains("sign-in.example.net")));
    assert!(ui.requests.is_empty());
    // A command from outside the window never hands an address to the system: only the
    // dialog's own button does.
    ui.execute("mail.openLink", &json!({"index": 4})).unwrap();
    ui.execute("mail.openLink", &json!({"index": 1, "confirm": true})).unwrap();
    assert!(ui.requests.is_empty());
    // The dialog is drawn, in both themes, and still nothing is opened.
    for dark in [false, true] {
        ui.execute("ui.theme", &json!({"theme": if dark { "dark" } else { "light" }})).unwrap();
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 700.0))),
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |u| ui.ui(u));
            output.textures_delta.clear();
        }
    }
    assert!(ui.requests.is_empty() && ui.link_ask.is_some());
    // A mailbox is told apart.
    ui.execute("ui.linkDialog", &json!({"index": 3})).unwrap();
    assert!(ui.link_ask.as_ref().is_some_and(|a| a.mailbox && a.goes.as_deref() == Some("mailbox-notice.example.net")));
}

#[test]
fn the_print_window_shows_the_pages_and_follows_the_setup() {
    let mut ui = MailUi::new(Session::new());
    assert!(ui.execute("ui.printPreview", &Value::Null).is_err(), "nothing is shown");
    assert!(ui.preview.is_none());
    let mut ui = MailUi::new(Session::with_sample());
    ui.execute("mail.select", &json!({"folder": "Inbox", "index": 4})).unwrap();
    ui.execute("ui.printPreview", &Value::Null).unwrap();
    assert!(ui.preview.as_ref().is_some_and(|p| p.page == 0 && p.laid.is_none()));

    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let frame = |ui: &mut MailUi, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))),
            events,
            ..egui::RawInput::default()
        };
        let mut output = ctx.run_ui(input, |u| ui.ui(u));
        output.textures_delta.clear();
    };
    let key = |key: egui::Key, modifiers: egui::Modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    for dark in [false, true] {
        ui.execute("ui.theme", &json!({"theme": if dark { "dark" } else { "light" }})).unwrap();
        for _ in 0..3 {
            frame(&mut ui, Vec::new());
        }
    }
    // The pages are laid out in the installed fonts; where none is installed the window says so.
    let pages = ui.preview.as_ref().unwrap().pages();
    let laid = ui.preview.as_ref().unwrap().laid.as_ref().unwrap().is_ok();
    assert_eq!(laid, pages > 0);
    // Looking at the pages prints nothing and changes nothing.
    assert!(ui.requests.is_empty());
    assert_eq!(ui.session.page_setup, openmailclient_layout::Setup::default());

    // A change of the setup, from the window or from outside, is laid out again.
    ui.execute("file.pageSetup", &json!({"paper": "a5", "orientation": "landscape", "margins": 30})).unwrap();
    frame(&mut ui, Vec::new());
    let now = ui.preview.as_ref().unwrap().pages();
    if laid {
        assert!(now > pages, "{now} pages on the small sheet, {pages} on the large");
        // Keys turn the pages, and stop at the first and the last.
        frame(&mut ui, vec![key(egui::Key::End, egui::Modifiers::NONE)]);
        assert_eq!(ui.preview.as_ref().unwrap().page, now - 1);
        frame(&mut ui, vec![key(egui::Key::PageDown, egui::Modifiers::NONE)]);
        assert_eq!(ui.preview.as_ref().unwrap().page, now - 1);
        frame(&mut ui, vec![key(egui::Key::PageUp, egui::Modifiers::NONE)]);
        assert_eq!(ui.preview.as_ref().unwrap().page, now - 2);
        frame(&mut ui, vec![key(egui::Key::Home, egui::Modifiers::NONE)]);
        assert_eq!(ui.preview.as_ref().unwrap().page, 0);
    }
    // A page there is none of is the last one.
    ui.execute("ui.printPreview", &json!({"page": 9_999})).unwrap();
    frame(&mut ui, Vec::new());
    assert_eq!(ui.preview.as_ref().unwrap().page, now.saturating_sub(1));
    // Another message is laid out in its place.
    ui.execute("mail.select", &json!({"folder": "Inbox", "index": 0})).unwrap();
    frame(&mut ui, Vec::new());
    assert!(ui.preview.as_ref().unwrap().laid.is_some());

    // Escape closes it, Ctrl+P opens it, and the command can close it; nothing was printed.
    frame(&mut ui, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    assert!(ui.preview.is_none());
    frame(&mut ui, vec![key(egui::Key::P, egui::Modifiers::COMMAND)]);
    frame(&mut ui, Vec::new());
    assert!(ui.preview.is_some());
    ui.execute("ui.printPreview", &json!({"close": true})).unwrap();
    assert!(ui.preview.is_none());
    assert!(ui.requests.is_empty());
    // With no message open there is no memo to show, and the window says so; the folder's
    // list can still be printed, as a table.
    ui.execute("ui.printPreview", &Value::Null).unwrap();
    ui.session.open = None;
    ui.session.selected = None;
    frame(&mut ui, Vec::new());
    assert!(ui.preview.as_ref().unwrap().laid.as_ref().unwrap().is_err());
    assert_eq!(ui.preview.as_ref().unwrap().pages(), 0);
    ui.execute("file.pageSetup", &json!({"style": "table", "paper": "a4", "orientation": "portrait"})).unwrap();
    frame(&mut ui, Vec::new());
    assert_eq!(ui.preview.as_ref().unwrap().pages(), usize::from(laid));
    // A message read or a list searched is laid out again.
    let before = ui.session.rows.len();
    ui.execute("search.query", &json!({"text": "rota"})).unwrap();
    assert!(ui.session.rows.len() < before);
    frame(&mut ui, Vec::new());
    assert_eq!(ui.preview.as_ref().unwrap().pages(), usize::from(laid));
    // With no folder shown either, there is nothing to print and the window goes away.
    ui.session.current = None;
    frame(&mut ui, Vec::new());
    assert!(ui.preview.is_none());
    assert!(ui.requests.is_empty());
}

/// What a paragraph's text is handed to the text shaper as, `wide` wide, and for each line what
/// stands in it from the left: where, and which of the stored characters.
fn turned_text(html: &str, wide: f32) -> Option<(String, Vec<Vec<(f32, f32, (usize, usize))>>)> {
    let doc = openmailclient_formats::html::parse(html);
    let Some(openmailclient_formats::rich::Block::Para(para)) = doc.blocks.first() else { return None };
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let mut out = None;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = wide;
        if para.align == openmailclient_formats::rich::Align::Right {
            job.halign = egui::Align::RIGHT;
        }
        for run in &para.runs {
            job.append(&run.text, 0.0, egui::TextFormat { font_id: font(16.0), ..egui::TextFormat::default() });
        }
        out = body::turned(ui, para, &job).map(|turned| {
            // As many lines are drawn as were made.
            let drawn = ui.painter().layout_job(turned.job.clone());
            assert_eq!(drawn.rows.len(), turned.rows.len());
            (turned.job.text.clone(), turned.rows)
        });
    });
    output.textures_delta.clear();
    out
}

#[test]
fn right_to_left_text_is_drawn_in_the_order_it_is_seen_in() {
    // Nothing right-to-left: the text is drawn as it is stored, and can be selected.
    assert!(turned_text("<p>plain words</p>", 400.0).is_none());
    // The text shaper turns a word of right-to-left letters round by itself, so such a word is
    // handed over as it is stored; what is worked out here is where each word stands.
    let (text, rows) = turned_text("<p>ab \u{5d0}\u{5d1}\u{5d2} cd</p>", 400.0).unwrap();
    assert_eq!(text, "ab \u{5d0}\u{5d1}\u{5d2} cd");
    assert_eq!(rows[0].iter().map(|(.., chars)| *chars).collect::<Vec<_>>(), [(0, 2), (2, 3), (3, 6), (6, 7), (7, 9)]);
    // A right-to-left paragraph: the word read first stands last, at the right; Latin words
    // are as they are read.
    let (text, rows) = turned_text("<p dir=rtl>\u{5d0}\u{5d1} ab cd (\u{5d2})</p>", 400.0).unwrap();
    assert_eq!(text, "(\u{5d2}) ab cd \u{5d0}\u{5d1}");
    assert_eq!(rows[0].last().unwrap().2, (0, 2));
    // Each thing in a line starts where the one before it ends.
    assert!(rows[0].windows(2).all(|pair| (pair[0].1 - pair[1].0).abs() < 0.01 && pair[0].1 > pair[0].0));
    // What the shaper would not turn is turned here: a full stop, a bracket alone, and Latin
    // letters that are told to run from the right.
    let (text, _) = turned_text("<p dir=rtl>ab cd.</p>", 400.0).unwrap();
    assert_eq!(text, ".ab cd");
    let (text, _) = turned_text("<p dir=rtl>\u{5d0} ( \u{5d1}</p>", 400.0).unwrap();
    assert_eq!(text, "\u{5d1} ) \u{5d0}");
    let (text, _) = turned_text("<p>a \u{202e}bcd\u{202c} e</p>", 400.0).unwrap();
    assert_eq!(text.replace(['\u{202e}', '\u{202c}'], ""), "a dcb e");
    // Digits written with Arabic letters run from the left, though the shaper takes them to
    // run from the right: they are turned here and turned back there.
    let (text, _) = turned_text("<p dir=rtl>\u{627} \u{661}\u{662}\u{663}</p>", 400.0).unwrap();
    assert_eq!(text, "\u{663}\u{662}\u{661} \u{627}");
    // Each line is turned by itself: the words read first stay on the first line.
    let words = ["\u{5d0}".repeat(8), "\u{5d1}".repeat(8), "\u{5d2}".repeat(8)];
    let (text, rows) = turned_text(&format!("<p dir=rtl>{}</p>", words.join(" ")), 100.0).unwrap();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert_eq!(lines, words);
    assert_eq!(rows.len(), 3);
    // A right-to-left line is counted from its right end.
    assert!(rows.iter().flatten().all(|(from, to, _)| *from < 0.0 && *to <= 0.01));
}

#[test]
fn hostile_right_to_left_bodies_draw_without_panicking() {
    let bodies = [
        "<p dir=rtl>".to_string() + &"\u{202e}".repeat(400) + "ab\u{5d0}" + &"\u{202c}".repeat(500) + "</p>",
        "<p dir=rtl></p><p dir=rtl> </p><p dir=rtl><br><br></p><p dir=auto>\u{644}\u{627}</p><p dir=auto>12</p>".to_string(),
        "<p dir=rtl>\u{644}<img src=cid:none width=40 height=40>\u{627}<img src=cid:none width=4096 height=4096 align=left>".to_string() + &"\u{5d0}\u{628} ".repeat(2000) + "</p>",
        "<p dir=rtl>".to_string() + &"\u{5d0} ".repeat(9000) + "</p><p>" + &"\u{5d0}".repeat(70_000) + "</p>",
        "<pre dir=rtl>\u{5d0}\ta\t\u{644}\u{644}\u{627}\n\n\t</pre><ul dir=rtl><li>\u{5d0}<ol><li>\u{627}\u{644}</ol></ul>".to_string(),
        "<table dir=rtl border=1 width=10><tr><td>\u{5d0}\u{5d1}<td style=\"direction:ltr;text-align:end\">\u{627}\u{644}\u{644}\u{647}<td dir=\"\u{202e}\">x</table>".to_string(),
        "<p style=\"direction:rtl;direction:ltr;direction:;text-align:start\"><a href=\"https://x.example/\">\u{5d0}\u{644}\u{627}(</a>\u{200f}\u{200e}\u{200d}\u{640}</p>".to_string(),
        "<blockquote dir=rtl>".repeat(200) + "\u{5d0}",
    ];
    let tokens = theme::Tokens::named(false);
    for html in &bodies {
        let doc = openmailclient_formats::html::parse(html);
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        for width in [0.0, 30.0, 800.0] {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_max_width(width);
                body::draw(ui, &doc, &tokens, &pictures::Pictures::default(), &faces::Faces::default());
            });
            output.textures_delta.clear();
        }
    }
}

/// The letters of a paragraph's lines from the left as the text shaper leaves them on the
/// screen: what it is handed, with every piece it takes to be right-to-left turned round.
fn seen_text(html: &str) -> Vec<String> {
    let doc = openmailclient_formats::html::parse(html);
    let Some(openmailclient_formats::rich::Block::Para(para)) = doc.blocks.first() else { return Vec::new() };
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let mut out = Vec::new();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = 2000.0;
        for run in &para.runs {
            job.append(&run.text, 0.0, egui::TextFormat { font_id: font(16.0), ..egui::TextFormat::default() });
        }
        let Some(turned) = body::turned(ui, para, &job) else { return };
        let drawn = ui.painter().layout_job(turned.job.clone());
        // A glyph with no width only keeps count of a letter that was drawn with another.
        out = drawn.rows.iter().map(|row| row.glyphs.iter().filter(|g| g.advance_width > 0.0).map(|g| g.chr).collect()).collect();
    });
    output.textures_delta.clear();
    out
}

#[test]
fn right_to_left_lines_reach_the_screen_with_latin_words_readable() {
    // Each piece is shaped by itself: a line that begins with a Hebrew word does not take its
    // Latin words round with it, and a Latin line leaves its Hebrew word turned.
    assert_eq!(seen_text("<p dir=rtl>\u{5d0}\u{5d1} ab cd (\u{5d2}).</p>"), [".(\u{5d2}) ab cd \u{5d1}\u{5d0}"]);
    assert_eq!(seen_text("<p>ab \u{5d0}\u{5d1}\u{5d2}, cd</p>"), ["ab \u{5d2}\u{5d1}\u{5d0}, cd"]);
    // Digits stay as they are read, in either kind of line.
    assert_eq!(seen_text("<p dir=rtl>\u{5d0}\u{5d1} 123 x</p>"), ["x 123 \u{5d1}\u{5d0}"]);
}

#[test]
fn text_at_the_right_of_a_narrow_cell_stays_inside_it() {
    // A cell narrower than the least a text wraps at: its text ends where the cell does.
    let doc = openmailclient_formats::html::parse("<table border=1><tr><td align=right>on<td align=right>tw<td>three</table>");
    let tokens = theme::Tokens::named(false);
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.set_max_width(600.0);
        body::draw(ui, &doc, &tokens, &pictures::Pictures::default(), &faces::Faces::default());
    });
    // Where each text was drawn, from its left to its right.
    let mut spans: Vec<(String, f32, f32)> = Vec::new();
    for clipped in &output.shapes {
        if let egui::Shape::Text(text) = &clipped.shape {
            let rect = text.galley.rect.translate(text.pos.to_vec2());
            spans.push((text.galley.job.text.clone(), rect.left(), rect.right()));
        }
    }
    output.textures_delta.clear();
    let find = |text: &str| spans.iter().find(|(t, ..)| t == text).unwrap();
    let (one, two, three) = (find("on"), find("tw"), find("three"));
    assert!(one.2 <= two.1 + 0.01 && two.2 <= three.1 + 0.01, "{spans:?}");
}

#[test]
fn the_window_keeps_its_settings_in_the_settings_file() {
    let dir = std::env::temp_dir().join(format!("openmailclient-ui-settings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let at = json!({ "path": dir.join("settings.json") });

    let mut ui = MailUi::new(Session::new());
    let fresh = ui.settings();
    ui.execute("ui.theme", &json!({ "theme": "dark" })).unwrap();
    ui.execute("ui.ribbonMode", &json!({ "mode": "classic" })).unwrap();
    ui.execute("ui.bodyFormat", &json!({ "format": "plain" })).unwrap();
    let kept = ui.settings();
    assert_ne!(kept, fresh);
    ui.execute("settings.save", &at).unwrap();

    let mut next = MailUi::new(Session::new());
    assert_eq!(next.settings(), fresh);
    next.execute("settings.load", &at).unwrap();
    assert_eq!(next.settings(), kept);

    // What the file says of the window is taken a part at a time, and junk changes nothing.
    std::fs::write(dir.join("settings.json"), json!({ "ui": { "theme": "plaid", "ribbon": 4, "bodyFormat": "formatted", "more": [1] } }).to_string())
        .unwrap();
    next.execute("settings.load", &at).unwrap();
    let mut mixed = kept.clone();
    mixed["bodyFormat"] = json!("formatted");
    assert_eq!(next.settings(), mixed);
    std::fs::remove_dir_all(&dir).unwrap();
}
