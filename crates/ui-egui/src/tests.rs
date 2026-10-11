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
                    body::draw(ui, &doc, &tokens);
                });
                // No renderer here: the font texture updates have nowhere to go.
                output.textures_delta.clear();
            }
        }
    }
}
