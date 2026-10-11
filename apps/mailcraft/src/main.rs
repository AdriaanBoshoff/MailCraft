//! The MailCraft desktop app.
//!
//! ```text
//! mailcraft [--sample] [--dark] [--classic-ribbon] [file.pst | file.ost | file.msg | file.eml | file.oft]...
//! ```
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use mailcraft_engine::Session;
use mailcraft_ui_egui::{MailUi, UiRequest};
use serde_json::{Value, json};
use std::process::ExitCode;

struct App {
    ui: MailUi,
}

impl App {
    fn run(&mut self, id: &str, params: Value) {
        if let Err(e) = self.ui.execute(id, &params) {
            self.ui.session.status = e;
        }
    }

    /// Native dialogs for what the UI asked for. Only a user action queues these.
    fn serve(&mut self, ctx: &egui::Context) {
        for request in std::mem::take(&mut self.ui.requests) {
            match request {
                UiRequest::OpenFile => {
                    let picked = rfd::FileDialog::new()
                        .set_title("Open data file or message")
                        .add_filter("Mail files", &["pst", "ost", "msg", "eml", "oft"])
                        .add_filter("Outlook data files", &["pst", "ost"])
                        .add_filter("Messages", &["msg", "eml", "oft"])
                        .add_filter("All files", &["*"])
                        .pick_files();
                    for path in picked.unwrap_or_default() {
                        self.run("file.open", json!({ "path": path }));
                    }
                }
                UiRequest::SaveAs => {
                    let name = self.ui.session.open.as_ref().map(|it| safe_file_name(&it.subject())).unwrap_or_default();
                    let picked = rfd::FileDialog::new()
                        .set_title("Save message as")
                        .set_file_name(format!("{name}.msg"))
                        .add_filter("Outlook message", &["msg"])
                        .add_filter("Internet message", &["eml"])
                        .add_filter("Outlook template", &["oft"])
                        .save_file();
                    if let Some(path) = picked {
                        self.run("file.saveAs", json!({ "path": path }));
                    }
                }
                UiRequest::Print => {
                    let name = self.ui.session.open.as_ref().map(|it| safe_file_name(&it.subject())).unwrap_or_default();
                    let picked = rfd::FileDialog::new()
                        .set_title("Print to PDF")
                        .set_file_name(format!("{name}.pdf"))
                        .add_filter("PDF document", &["pdf"])
                        .save_file();
                    if let Some(path) = picked {
                        self.run("file.print", json!({ "path": path }));
                    }
                }
                UiRequest::SaveAttachment(index) => {
                    let name = self
                        .ui
                        .session
                        .open
                        .as_ref()
                        .and_then(|it| it.attachments.get(index))
                        .map(mailcraft_engine::attachments::file_name)
                        .unwrap_or_default();
                    if let Some(path) = rfd::FileDialog::new().set_title("Save attachment").set_file_name(name).save_file() {
                        self.run("file.saveAttachment", json!({ "index": index, "path": path }));
                    }
                }
                UiRequest::SaveAllAttachments => {
                    if let Some(folder) = rfd::FileDialog::new().set_title("Save all attachments in").pick_folder() {
                        self.run("file.saveAllAttachments", json!({ "folder": folder }));
                    }
                }
                UiRequest::Attach => {
                    for path in rfd::FileDialog::new().set_title("Attach file").pick_files().unwrap_or_default() {
                        self.run("compose.attach", json!({ "path": path }));
                    }
                }
                UiRequest::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }
}

/// A file name suggestion from untrusted text: no path separators, reserved characters or dots
/// at the ends, and a sensible length.
fn safe_file_name(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .take(80)
        .map(|c| if c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() { "message".to_string() } else { trimmed.to_string() }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui.ui(ui);
        let ctx = ui.ctx().clone();
        self.serve(&ctx);
    }
}

fn main() -> ExitCode {
    let mut session = Session::new();
    session.tz_offset_min = mailcraft_ui_egui::local_offset_minutes();
    let mut app = App { ui: MailUi::new(session) };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--sample" => app.run("file.openSample", Value::Null),
            "--dark" => app.run("ui.theme", json!({ "theme": "dark" })),
            "--classic-ribbon" => app.run("ui.ribbonMode", json!({ "mode": "classic" })),
            "--help" | "-h" => {
                println!("mailcraft [--sample] [--dark] [--classic-ribbon] [file.pst | file.ost | file.msg | file.eml | file.oft]...");
                return ExitCode::SUCCESS;
            }
            flag if flag.starts_with("--") => {
                eprintln!("mailcraft: unknown option {flag}");
                return ExitCode::FAILURE;
            }
            path => app.run("file.open", json!({ "path": path })),
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("MailCraft").with_inner_size([1280.0, 800.0]).with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    match eframe::run_native("MailCraft", options, Box::new(move |_cc| Ok(Box::new(app)))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mailcraft: {e}");
            ExitCode::FAILURE
        }
    }
}
