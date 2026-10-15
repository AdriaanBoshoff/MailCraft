//! Headless window screenshots: draws the real UI off-screen (no window, no focus stealing) and
//! writes PNGs.
//!
//! ```text
//! cargo run --release -p openmailclient-ui-egui --example ui_shot -- script.jsonl
//! ```
//!
//! The script is JSON lines, run in order:
//!
//! ```text
//! {"size": [1280, 800]}                      window size in points (before the first shot)
//! {"sample": true}                           open the sample mailbox
//! {"command": "mail.select", "params": {"index": 0}}
//! {"shot": "plan/shots/inbox.png"}
//! {"click": [640, 18]}                       press and let go at a place in the window
//! {"type": "words"}                          type into what has the keyboard
//! {"fake": {"ownCertificate": "AB:CD:…"}}    reach the in-process fake server (with a
//!                                            certificate of its own) instead of none
//! {"settle": true}                           draw until the jobs under way have ended
//! ```
//!
//! Without a script it opens the sample mailbox and writes `plan/shots/ui_shot.png`.

use egui_kittest::Harness;
use openmailclient_engine::Session;
use openmailclient_ui_egui::MailUi;
use serde_json::{Value, json};
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ui_shot: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let steps: Vec<Value> = match std::env::args().nth(1) {
        Some(path) => {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
            text.lines()
                .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
                .map(|l| serde_json::from_str(l).map_err(|e| format!("{path}: {e}: {l}")))
                .collect::<Result<_, _>>()?
        }
        None => vec![
            json!({ "sample": true }),
            json!({ "command": "mail.select", "params": { "index": 0 } }),
            json!({ "shot": "plan/shots/ui_shot.png" }),
        ],
    };
    let size = steps.iter().find_map(|s| s.get("size")).and_then(Value::as_array).map_or((1280.0, 800.0), |a| {
        let at = |i: usize| a.get(i).and_then(Value::as_f64).map_or(800.0, |v| v.clamp(320.0, 4096.0) as f32);
        (at(0), at(1))
    });
    let scale = steps.iter().find_map(|s| s.get("scale")).and_then(Value::as_f64).map_or(1.0, |v| v.clamp(0.5, 3.0) as f32);
    let mut harness = Harness::builder()
        .with_size(size)
        .with_pixels_per_point(scale)
        .wgpu()
        .build_ui_state(|ui, app: &mut MailUi| app.ui(ui), MailUi::new(Session::new()));
    harness.run_steps(3);
    for step in &steps {
        if step.get("sample").and_then(Value::as_bool) == Some(true) {
            harness.state_mut().execute("file.openSample", &Value::Null)?;
        } else if let Some(id) = step.get("command").and_then(Value::as_str) {
            let params = step.get("params").cloned().unwrap_or(Value::Null);
            if let Err(e) = harness.state_mut().execute(id, &params) {
                eprintln!("ui_shot: {id}: {e}");
            }
        } else if let Some(at) = step.get("click").and_then(Value::as_array) {
            let part = |i: usize| at.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
            let at = egui::pos2(part(0), part(1));
            harness.hover_at(at);
            harness.run_steps(1);
            harness.drag_at(at);
            harness.run_steps(1);
            harness.drop_at(at);
        } else if let Some(fake) = step.get("fake") {
            let server = openmailclient_net::fake::Fake::default();
            server.state().own_certificate = fake.get("ownCertificate").and_then(Value::as_str).and_then(openmailclient_net::Fingerprint::parse);
            harness.state_mut().session.connector = Some(std::sync::Arc::new(server));
        } else if step.get("settle").and_then(Value::as_bool) == Some(true) {
            for _ in 0..500 {
                if harness.state().session.jobs.is_empty() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
                harness.run_steps(1);
            }
        } else if let Some(text) = step.get("type").and_then(Value::as_str) {
            harness.event(egui::Event::Text(text.to_string()));
        } else if let Some(path) = step.get("shot").and_then(Value::as_str) {
            harness.run_steps(4);
            let image = harness.render()?;
            if let Some(dir) = Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            image.save(path).map_err(|e| format!("{path}: {e}"))?;
            println!("wrote {path} ({}x{})", image.width(), image.height());
        }
        harness.run_steps(2);
    }
    Ok(())
}
