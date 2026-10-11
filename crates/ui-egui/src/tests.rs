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
    assert_eq!(ui.requests, vec![UiRequest::OpenFile, UiRequest::SaveAttachment(2)]);
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
    ];
    for dark in [false, true] {
        let tokens = theme::Tokens::named(dark);
        for html in &bodies {
            let doc = mailcraft_formats::html::parse(html);
            let ctx = egui::Context::default();
            install_fonts(&ctx);
            for width in [0.0, 30.0, 800.0] {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    ui.set_max_width(width);
                    body::draw(ui, &doc, &tokens, &pictures::Pictures::default());
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
    assert!(doc.blocks.iter().any(|b| matches!(b, mailcraft_formats::rich::Block::Table(t) if t.border && t.rows.len() == 3)));
    assert!(doc.text().contains("parking.example.net/help"));
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| body::draw(ui, &doc, &theme::Tokens::named(true), &pictures::Pictures::default()));
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
            body::draw(ui, &doc, &theme::Tokens::named(false), &shown);
        });
        output.textures_delta.clear();
    }

    // Content ids are matched as the message writes them: brackets and letter case do not matter.
    // A picture the message does not carry, or carries as something else, stays a placeholder.
    let mut item = mailcraft_model::Item::new_mail();
    for (name, id, data) in [
        ("a.png", "<Logo@Example>", sample_picture()),
        ("b.png", "notes", b"not a picture at all".to_vec()),
        ("c.svg", "drawing", b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>x()</script></svg>".to_vec()),
    ] {
        let mut a = mailcraft_model::Attachment::file(name, "image/png", data);
        a.props.set_str(pid::ATTACH_CONTENT_ID, id);
        item.attachments.push(a);
    }
    let html = "<p>a<img src=\"cid:logo@example\"> <img src=\"cid:logo@example\" width=5000><br>b</p><table><tr><td><img src=cid:notes alt=N>\
                <td><img src=cid:drawing><img src=cid:missing><img src=\"https://x.example/r.png\"></table>";
    let doc = mailcraft_formats::html::parse(html);
    let mut shown = pictures::Pictures::default();
    shown.load(&ctx, 1, &doc, &item);
    assert!(shown.get("logo@example").is_some());
    assert!(["notes", "drawing", "missing", "https://x.example/r.png"].iter().all(|id| shown.get(id).is_none()));
    for dark in [false, true] {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| body::draw(ui, &doc, &theme::Tokens::named(dark), &shown));
        output.textures_delta.clear();
    }
    // Another message: nothing of the last one is kept.
    shown.load(&ctx, 2, &mailcraft_formats::html::parse("<p>text</p>"), &item);
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
