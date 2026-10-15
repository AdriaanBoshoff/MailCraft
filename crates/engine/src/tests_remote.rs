//! Pictures on a server, downloaded for the open message only when asked, from the fake web
//! server: nothing leaves the process.

use crate::Session;
use crate::remote::{Downloaded, MAX_KEPT, addresses};
use openmailclient_model::{Item, PropValue, pid};
use openmailclient_net::fake::{Fake, State};
use serde_json::json;
use std::sync::Arc;

/// A PNG of one pixel, made here.
fn png() -> Vec<u8> {
    use image::ImageEncoder;
    let mut file = Vec::new();
    image::codecs::png::PngEncoder::new(&mut file).write_image(&[200, 30, 30, 255], 1, 1, image::ExtendedColorType::Rgba8).unwrap();
    file
}

fn answer(body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out
}

/// A session reading a message that names `html`'s pictures, and a fake web server.
fn reading(html: &str, pages: Vec<(String, Vec<u8>)>) -> (Session, Fake) {
    let fake = Fake::new(State { pages, ..State::default() });
    let mut s = Session::new();
    s.connector = Some(Arc::new(fake.clone()));
    let mut item = Item::new_mail();
    item.props.set(pid::HTML, PropValue::Binary(html.as_bytes().to_vec()));
    s.open = Some(item);
    s.selected = Some(1);
    (s, fake)
}

#[test]
fn pictures_are_downloaded_only_when_asked_and_only_the_message_s_own() {
    let html = r#"<p><img src="https://pictures.example.com/a.png" width="40" height="40">
        <img src="https://pictures.example.com/a.png"> <img src="https://pictures.example.com/gone.png">
        <img src="http://pictures.example.com/plain.png"> <img src="https://pictures.example.com/text.png"></p>"#;
    let pages =
        vec![("pictures.example.com/a.png".to_string(), answer(&png())), ("pictures.example.com/text.png".to_string(), answer(b"not a picture"))];
    let (mut s, fake) = reading(html, pages);
    // Opening and reading fetch nothing.
    assert!(fake.state().heard.is_empty());
    assert!(s.downloaded.is_empty());
    // A caller cannot have an address of its own fetched.
    assert!(s.run("mail.downloadPictures", &json!({ "address": "https://elsewhere.example.com/x.png" })).is_err());
    assert!(fake.state().heard.is_empty());

    let got = s.run("mail.downloadPictures", &json!({})).unwrap();
    assert_eq!(got["downloaded"], 1, "{got}");
    // The missing one, the plain-HTTP one and the one that is not a picture are said to have failed.
    assert_eq!(got["untrusted_content"]["failed"].as_array().unwrap().len(), 3, "{got}");
    assert!(s.downloaded.contains("https://pictures.example.com/a.png"));
    assert!(s.status.contains("1 picture downloaded"), "{}", s.status);
    // The same picture named twice is fetched once, and once kept is not fetched again.
    let asked = |fake: &Fake| fake.state().heard.iter().filter(|line| line.starts_with("GET /a.png")).count();
    assert_eq!(asked(&fake), 1);
    let again = s.run("mail.downloadPictures", &json!({ "address": "https://pictures.example.com/a.png" })).unwrap();
    assert_eq!(again["downloaded"], 0);
    assert_eq!(asked(&fake), 1);
}

#[test]
fn a_download_runs_on_a_thread_of_its_own() {
    let (mut s, _fake) =
        reading(r#"<img src="https://pictures.example.com/a.png">"#, vec![("pictures.example.com/a.png".to_string(), answer(&png()))]);
    let started = s.run("mail.downloadPictures", &json!({ "wait": false })).unwrap();
    assert_eq!(started["started"], true, "{started}");
    // Asked again meanwhile, it does not start another.
    assert_eq!(s.run("mail.downloadPictures", &json!({ "wait": false })).unwrap()["started"], false);
    let mut waited = 0;
    while !s.jobs.is_empty() {
        s.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
        assert!(waited < 1000, "never ended: {}", s.status);
    }
    assert!(s.downloaded.contains("https://pictures.example.com/a.png"));
}

#[test]
fn without_a_network_or_a_message_nothing_is_fetched() {
    let (mut s, _) = reading(r#"<img src="https://pictures.example.com/a.png">"#, Vec::new());
    s.connector = None;
    assert!(s.run("mail.downloadPictures", &json!({})).is_err());
    s.open = None;
    assert!(s.run("mail.downloadPictures", &json!({})).is_err());
}

#[test]
fn what_is_kept_is_capped_and_the_oldest_let_go() {
    let mut kept = Downloaded::default();
    let big = MAX_KEPT / 3 + 1;
    for n in 0..4 {
        kept.keep(format!("https://p.example.com/{n}"), vec![0; big]);
    }
    assert!(!kept.contains("https://p.example.com/0"));
    assert!(kept.contains("https://p.example.com/3"));
    assert_eq!(kept.len(), 2);
    let before = kept.generation;
    kept.keep("https://p.example.com/huge".into(), vec![0; MAX_KEPT + 1]);
    assert_eq!(kept.generation, before, "one too large is not kept");
    // Addresses are taken from tables too, each once, and only so many.
    let many: String = (0..100).map(|n| format!(r#"<table><tr><td><img src="https://p.example.com/{n}.png"></td></tr></table>"#)).collect();
    let found = addresses(&openmailclient_formats::html::parse(&many));
    assert_eq!(found.len(), crate::remote::MAX_PER_MESSAGE);
    assert_eq!(found.first().map(String::as_str), Some("https://p.example.com/0.png"));
}
