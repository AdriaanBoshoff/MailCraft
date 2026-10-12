//! The OpenMailClient desktop app.
//!
//! ```text
//! openmailclient [--sample] [--dark] [--classic-ribbon] [--no-settings] [file.pst | file.ost | file.msg | file.eml | file.oft]...
//! ```
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use openmailclient_engine::Session;
use openmailclient_ui_egui::{MailUi, UiRequest};
use serde_json::{Value, json};
use std::process::ExitCode;

struct App {
    ui: MailUi,
    /// Where the settings are kept; `None` when they are not (`--no-settings`).
    settings: Option<std::path::PathBuf>,
    /// The settings as they were last written or read.
    kept: Value,
}

impl App {
    fn run(&mut self, id: &str, params: Value) {
        if let Err(e) = self.ui.execute(id, &params) {
            self.ui.session.status = e;
        }
    }

    /// Write the settings when they are not what the file has: when they change, not only on
    /// the way out, so that a run that ends badly keeps them too.
    fn keep_settings(&mut self) {
        let Some(path) = self.settings.clone() else { return };
        let now = self.ui.session.settings(Some(&self.ui.settings()));
        if now == self.kept {
            return;
        }
        // The status line is for what the person did, not for housekeeping: a failure to
        // write settings is not shown over it, and is tried again at the next change.
        let status = self.ui.session.status.clone();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = self.ui.execute("settings.save", &json!({ "path": path.display().to_string() }));
        self.ui.session.status = status;
        self.kept = now;
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
                UiRequest::Export => {
                    let session = &self.ui.session;
                    let name =
                        session.current.and_then(|(i, _)| session.stores.get(i)).map(|s| safe_file_name(&s.display_name())).unwrap_or_default();
                    let picked = rfd::FileDialog::new()
                        .set_title("Export to a data file")
                        .set_file_name(format!("{name}.pst"))
                        .add_filter("Outlook data file", &["pst"])
                        .save_file();
                    if let Some(path) = picked {
                        self.run("file.export", json!({ "path": path }));
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
                    let session = &self.ui.session;
                    let name = safe_file_name(&openmailclient_engine::print::title(session, session.print_style));
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
                        .map(openmailclient_engine::attachments::file_name)
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
                UiRequest::OpenLink(address) => {
                    self.ui.session.status = match open_in_browser(&address) {
                        Ok(()) => "Opened in the browser".to_string(),
                        Err(e) => format!("The browser could not be started: {e}"),
                    };
                }
                UiRequest::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }
}

/// Hand a web address to the system's browser. The address has been checked by the engine
/// (`mail.openLink`): `http` or `https`, plain characters only. It is checked once more here,
/// because this is where it leaves the program, and it is passed as one argument, never through
/// a shell.
fn open_in_browser(address: &str) -> std::io::Result<()> {
    use openmailclient_formats::links::{Target, target};
    let refused = |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, why.to_string());
    let address = match target(address) {
        Ok(Target::Page(address)) => address,
        Ok(Target::Mailbox(_)) => return Err(refused("not a web address")),
        Err(why) => return Err(refused(why)),
    };
    let mut command = if cfg!(target_os = "windows") {
        let mut command = std::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    command.arg(address).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    command.spawn().map(|_| ())
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
        // Closing the window with unsaved changes asks once before they are thrown away.
        if ctx.input(|i| i.viewport().close_requested()) && !self.ui.may_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        self.serve(&ctx);
        self.keep_settings();
    }

    fn on_exit(&mut self) {
        self.keep_settings();
    }
}

/// Where the settings are kept: the per-user configuration folder of the system.
fn settings_path() -> Option<std::path::PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    let dir = if cfg!(target_os = "windows") {
        var("APPDATA")?.join("OpenMailClient")
    } else if cfg!(target_os = "macos") {
        var("HOME")?.join("Library").join("Application Support").join("OpenMailClient")
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|home| home.join(".config")))?.join("openmailclient")
    };
    Some(dir.join("settings.json"))
}

fn main() -> ExitCode {
    let mut session = Session::new();
    session.tz_offset_min = openmailclient_ui_egui::local_offset_minutes();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let settings = if args.iter().any(|a| a == "--no-settings") { None } else { settings_path() };
    let mut app = App { ui: MailUi::new(session), settings, kept: Value::Null };
    if let Some(path) = app.settings.clone() {
        // The data files of last time are opened again unless this run names what to open.
        let reopen = !args.iter().any(|a| a == "--sample" || !a.starts_with("--"));
        app.run("settings.load", json!({ "path": path.display().to_string(), "reopen": reopen }));
        app.kept = app.ui.session.settings(Some(&app.ui.settings()));
    }
    for arg in args {
        match arg.as_str() {
            "--no-settings" => {}
            "--sample" => app.run("file.openSample", Value::Null),
            "--dark" => app.run("ui.theme", json!({ "theme": "dark" })),
            "--classic-ribbon" => app.run("ui.ribbonMode", json!({ "mode": "classic" })),
            "--help" | "-h" => {
                println!(
                    "openmailclient [--sample] [--dark] [--classic-ribbon] [--no-settings] [file.pst | file.ost | file.msg | file.eml | file.oft]..."
                );
                return ExitCode::SUCCESS;
            }
            flag if flag.starts_with("--") => {
                eprintln!("openmailclient: unknown option {flag}");
                return ExitCode::FAILURE;
            }
            path => app.run("file.open", json!({ "path": path })),
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("OpenMailClient").with_inner_size([1280.0, 800.0]).with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    match eframe::run_native("OpenMailClient", options, Box::new(move |_cc| Ok(Box::new(app)))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("openmailclient: {e}");
            ExitCode::FAILURE
        }
    }
}
