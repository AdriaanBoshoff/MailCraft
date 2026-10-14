//! Account Settings: the accounts mail is sent and received through, added, changed, tested and
//! removed in a window of their own. Everything is done by the `account.*` commands, as the
//! command line and an agent do it. The password is typed into a field that shows dots, handed
//! to the system's credential store when the account is saved, and forgotten by the window then:
//! it is never shown, copied out, put in the status line or kept in the settings.

use crate::line_edit::LineEdit;
use crate::theme::Tokens;
use crate::{MailUi, bold, font, one_line};
use egui::{Align2, Color32, vec2};
use serde_json::{Value, json};

/// The kinds of security a server is reached with, as the engine names them and as shown.
const SECURITY: [(&str, &str); 3] = [("tls", "SSL/TLS"), ("starttls", "STARTTLS"), ("none", "None (not safe)")];

/// One account as the window shows it: each field as text until it is saved.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Form {
    /// `None` for an account not added yet.
    pub id: Option<String>,
    pub email: String,
    pub display_name: String,
    pub name: String,
    pub pop3: bool,
    pub in_host: String,
    pub in_port: String,
    pub in_sec: &'static str,
    pub out_host: String,
    pub out_port: String,
    pub out_sec: &'static str,
    pub user: String,
    /// What was typed since the account was last saved; empty keeps the password kept now.
    pub password: String,
    /// The credential store holds a password for it.
    pub has_password: bool,
    pub sign_in: bool,
    pub leave: bool,
    /// The data file its mail is kept in, where it has one of its own.
    pub data_file: String,
}

impl Form {
    fn blank() -> Self {
        Self {
            id: None,
            email: String::new(),
            display_name: String::new(),
            name: String::new(),
            pop3: false,
            in_host: String::new(),
            in_port: String::new(),
            in_sec: "tls",
            out_host: String::new(),
            out_port: String::new(),
            out_sec: "tls",
            user: String::new(),
            password: String::new(),
            has_password: false,
            sign_in: true,
            leave: true,
            data_file: String::new(),
        }
    }

    /// The form for an account as `account.list` gives it.
    fn of(a: &Value) -> Self {
        let text = |key: &str| a.get(key).and_then(Value::as_str).map(one_line).unwrap_or_default();
        let sec = |v: Option<&Value>| {
            let s = v.and_then(|v| v.get("security")).and_then(Value::as_str).unwrap_or("tls");
            SECURITY.iter().find(|(k, _)| *k == s).map_or("tls", |(k, _)| k)
        };
        let host = |key: &str| a.get(key).and_then(|v| v.get("host")).and_then(Value::as_str).map(one_line).unwrap_or_default();
        let port = |key: &str| a.get(key).and_then(|v| v.get("port")).and_then(Value::as_u64).map(|p| p.to_string()).unwrap_or_default();
        let name = text("name");
        let email = text("email");
        let user = text("user");
        Self {
            id: a.get("id").and_then(Value::as_str).map(str::to_string),
            // What follows from the address is shown as following from it: left empty.
            name: if name == email { String::new() } else { name },
            user: if user == email { String::new() } else { user },
            email,
            display_name: text("displayName"),
            pop3: a.get("kind").and_then(Value::as_str) == Some("pop3"),
            in_host: host("incoming"),
            in_port: port("incoming"),
            in_sec: sec(a.get("incoming")),
            out_host: host("outgoing"),
            out_port: port("outgoing"),
            out_sec: sec(a.get("outgoing")),
            password: String::new(),
            has_password: a.get("hasPassword").and_then(Value::as_bool) == Some(true),
            sign_in: a.get("signInToSend").and_then(Value::as_bool) != Some(false),
            leave: a.get("leaveOnServer").and_then(Value::as_bool) != Some(false),
            data_file: a.get("dataFile").and_then(Value::as_str).map(one_line).unwrap_or_default(),
        }
    }

    /// The fields as `account.add` and `account.edit` take them, the password left out.
    fn params(&self) -> Result<Value, String> {
        let port = |text: &str, which: &str| -> Result<Value, String> {
            let text = text.trim();
            if text.is_empty() {
                // None given: the usual one for the security chosen.
                return Ok(Value::Null);
            }
            text.parse::<u16>().ok().filter(|p| *p > 0).map(|p| json!(p)).ok_or_else(|| format!("The {which} port is a number from 1 to 65535"))
        };
        let incoming = json!({ "host": self.in_host.trim(), "port": port(&self.in_port, "incoming")?, "security": self.in_sec });
        // An outgoing server not named is the incoming one.
        let out_host = if self.out_host.trim().is_empty() { self.in_host.trim() } else { self.out_host.trim() };
        let outgoing = json!({ "host": out_host, "port": port(&self.out_port, "outgoing")?, "security": self.out_sec });
        let mut p = json!({
            "email": self.email.trim(),
            "displayName": self.display_name.trim(),
            "name": self.name.trim(),
            "kind": if self.pop3 { "pop3" } else { "imap" },
            "user": self.user.trim(),
            "incoming": incoming,
            "outgoing": outgoing,
            "signInToSend": self.sign_in,
            "leaveOnServer": self.leave,
        });
        if let (Some(id), Some(map)) = (&self.id, p.as_object_mut()) {
            map.insert("id".into(), json!(id));
        }
        Ok(p)
    }
}

/// Text that held a password: written over before it is let go.
fn forget(text: &mut String) {
    let mut bytes = std::mem::take(text).into_bytes();
    bytes.fill(0);
    std::hint::black_box(&bytes);
}

/// An account in the window's list.
pub(crate) struct Listed {
    pub id: String,
    pub name: String,
    /// Its kind and address, under its name.
    pub detail: String,
}

/// The Account Settings window.
pub(crate) struct AccountsWindow {
    /// Each account as `account.list` last said.
    pub accounts: Vec<Listed>,
    pub form: Form,
    /// What came of the last thing done, and whether it went wrong.
    pub said: Vec<(String, bool)>,
    /// The test under way: the name of its job, until it has ended.
    pub testing: Option<String>,
    /// Remove was clicked once: the next click removes.
    pub removing: bool,
    focus: bool,
}

impl Drop for AccountsWindow {
    fn drop(&mut self) {
        forget(&mut self.form.password);
    }
}

/// What a click in the window asks for; done once the window is drawn.
enum Ask {
    Choose(String),
    New,
    Save,
    Test,
    Remove,
    Close,
}

impl MailUi {
    /// Open Account Settings on the account `id`, a new one when `new`, or the first.
    pub(crate) fn open_accounts(&mut self, id: Option<&str>, new: bool) -> Result<(), String> {
        let mut window = AccountsWindow { accounts: Vec::new(), form: Form::blank(), said: Vec::new(), testing: None, removing: false, focus: true };
        let listed = self.list_accounts(&mut window)?;
        let chosen = match id {
            Some(id) => Some(listed.iter().find(|a| a["id"] == id).ok_or_else(|| format!("There is no account {}", one_line(id)))?),
            None if new => None,
            None => listed.first(),
        };
        if let Some(a) = chosen {
            window.form = Form::of(a);
        }
        self.account_window = Some(window);
        Ok(())
    }

    fn list_accounts(&mut self, window: &mut AccountsWindow) -> Result<Vec<Value>, String> {
        let listed = self.execute("account.list", &Value::Null)?;
        let all = listed.get("accounts").and_then(Value::as_array).cloned().unwrap_or_default();
        window.accounts = all
            .iter()
            .filter_map(|a| {
                let text = |key: &str| one_line(a.get(key).and_then(Value::as_str).unwrap_or(""));
                let kind = if text("kind") == "pop3" { "POP3" } else { "IMAP" };
                Some(Listed { id: a.get("id")?.as_str()?.to_string(), name: text("name"), detail: format!("{kind} · {}", text("email")) })
            })
            .collect();
        Ok(all)
    }

    /// Show the account `id` as it is kept now.
    fn show_account(&mut self, window: &mut AccountsWindow, id: &str) {
        match self.list_accounts(window) {
            Ok(all) => {
                if let Some(a) = all.iter().find(|a| a["id"] == id) {
                    forget(&mut window.form.password);
                    window.form = Form::of(a);
                }
            }
            Err(e) => window.said = vec![(e, true)],
        }
    }

    /// Keep the form: add the account or change it, then hand any password typed to the
    /// credential store and forget it. Returns the account's id once it is kept.
    pub(crate) fn save_account(&mut self, window: &mut AccountsWindow) -> Option<String> {
        let mut p = match window.form.params() {
            Ok(p) => p,
            Err(e) => {
                window.said = vec![(e, true)];
                return None;
            }
        };
        let typed = !window.form.password.is_empty();
        let id = match window.form.id.clone() {
            None => {
                if let (true, Some(map)) = (typed, p.as_object_mut()) {
                    map.insert("password".into(), json!(window.form.password));
                }
                let added = self.execute("account.add", &p);
                if let Some(map) = p.as_object_mut() {
                    map.remove("password");
                }
                match added {
                    Ok(v) => v.get("id").and_then(Value::as_str).map(str::to_string),
                    Err(e) => {
                        window.said = vec![(e, true)];
                        return None;
                    }
                }
            }
            Some(id) => {
                if let Err(e) = self.execute("account.edit", &p) {
                    window.said = vec![(e, true)];
                    return None;
                }
                if typed && let Err(e) = self.execute("account.setPassword", &json!({ "id": id, "password": window.form.password })) {
                    window.said = vec![(e, true)];
                    return None;
                }
                Some(id)
            }
        }?;
        forget(&mut window.form.password);
        self.show_account(window, &id);
        let kept = if window.form.has_password { "the password is kept in the system's credential store" } else { "no password is kept for it yet" };
        window.said = vec![(format!("Saved; {kept}"), false)];
        Some(id)
    }

    /// Test what the form says, on a thread of its own: it is kept first.
    pub(crate) fn test_account(&mut self, window: &mut AccountsWindow) {
        let Some(id) = self.save_account(window) else { return };
        match self.execute("account.test", &json!({ "id": id, "wait": false })) {
            Ok(_) => {
                window.testing = self.session.jobs.last().map(|j| j.what.clone());
                window.said.clear();
            }
            Err(e) => window.said = vec![(e, true)],
        }
    }

    /// The Account Settings window, while it is open.
    pub(crate) fn accounts_window(&mut self, ctx: &egui::Context) {
        let Some(mut window) = self.account_window.take() else { return };
        // A test that ended since the last frame: what each server said.
        if let Some(job) = window.testing.clone()
            && !self.session.jobs.iter().any(|j| j.what == job)
        {
            window.testing = None;
            if let Some(result) = self.session.finished.iter().rev().find(|f| f["job"] == job.as_str()).map(|f| f["result"].clone()) {
                window.said = ["incoming", "outgoing"]
                    .iter()
                    .filter_map(|side| {
                        let r = result.get(side)?;
                        let ok = r.get("ok").and_then(Value::as_bool) == Some(true);
                        let words = one_line(r.get("said").and_then(Value::as_str).unwrap_or(if ok { "signed in" } else { "no answer" }));
                        let server = if *side == "incoming" { "Incoming server" } else { "Outgoing server" };
                        Some((format!("{server}: {words}"), !ok))
                    })
                    .collect();
                if window.said.is_empty() {
                    window.said = vec![(one_line(&self.session.status), true)];
                }
            }
        }
        let t = self.tokens.clone();
        let mut ask: Option<Ask> = None;
        let busy = window.testing.is_some();
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(18));
        egui::Window::new("Account Settings").frame(frame).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -20.0)).show(
            ctx,
            |ui| {
                ui.label(egui::RichText::new("Email accounts").font(bold(16.0)).color(t.text));
                ui.label(
                    egui::RichText::new("Mail is sent and received through these accounts, each with a data file of its own.")
                        .font(font(12.0))
                        .color(t.text_dim),
                );
                ui.add_space(12.0);
                ui.horizontal_top(|ui| {
                    // The accounts, each as a card: its name, its kind and its address.
                    ui.vertical(|ui| {
                        ui.set_width(LIST_W);
                        egui::Frame::new()
                            .fill(t.surface)
                            .stroke(egui::Stroke::new(1.0, t.line))
                            .corner_radius(6.0)
                            .inner_margin(egui::Margin::same(4))
                            .show(ui, |ui| {
                                ui.set_min_size(vec2(LIST_W - 10.0, 330.0));
                                egui::ScrollArea::vertical().max_height(330.0).show(ui, |ui| {
                                    for a in &window.accounts {
                                        let on = window.form.id.as_deref() == Some(a.id.as_str());
                                        if account_card(ui, &t, &a.name, &a.detail, on).clicked() && !on {
                                            ask = Some(Ask::Choose(a.id.clone()));
                                        }
                                    }
                                    if window.form.id.is_none() {
                                        let name = if window.form.email.trim().is_empty() { "New account" } else { window.form.email.trim() };
                                        let _ = account_card(ui, &t, name, "not saved yet", true);
                                    }
                                });
                            });
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!busy, egui::Button::new("+ New")).on_hover_text("Add another account").clicked() {
                                ask = Some(Ask::New);
                            }
                            let remove = if window.removing { "Remove it" } else { "Remove…" };
                            let button = egui::Button::new(egui::RichText::new(remove).color(if window.removing { t.on_accent } else { t.text }));
                            let button = if window.removing { button.fill(t.danger) } else { button };
                            if ui.add_enabled(window.form.id.is_some() && !busy, button).clicked() {
                                ask = Some(Ask::Remove);
                            }
                        });
                        if window.removing {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new("Click again to remove it. Its mail stays in its data file.")
                                        .font(font(11.5))
                                        .color(t.text_dim),
                                )
                                .wrap(),
                            );
                        }
                    });
                    ui.add_space(18.0);
                    ui.vertical(|ui| {
                        ui.set_width(FORM_W);
                        let f = &mut window.form;
                        let field = |ui: &mut egui::Ui, text: &mut String, key: &str, hint: &str, limit: usize| {
                            ui.add(
                                LineEdit::new(text, egui::Id::new(("account-field", key))).desired_width(FIELD_W).char_limit(limit).hint(hint, None),
                            )
                        };
                        let port = |ui: &mut egui::Ui, text: &mut String, key: &str| {
                            ui.label(egui::RichText::new("Port").color(t.text_dim));
                            ui.add(LineEdit::new(text, egui::Id::new(("account-field", key))).desired_width(58.0).char_limit(5).hint("usual", None));
                        };
                        let security = |ui: &mut egui::Ui, key: &str, sec: &mut &'static str, port: &mut String| {
                            let shown = SECURITY.iter().find(|(k, _)| k == sec).map_or("SSL/TLS", |(_, s)| s);
                            egui::ComboBox::from_id_salt(("account-security", key)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                                for (k, label) in SECURITY {
                                    if ui.selectable_label(*sec == k, label).clicked() && *sec != k {
                                        *sec = k;
                                        // The usual port for the new security, unless one is typed.
                                        port.clear();
                                    }
                                }
                            });
                        };

                        section(ui, &t, "Your details", "account-you", |ui| {
                            label(ui, &t, "Your name");
                            field(ui, &mut f.display_name, "display", "as mail from you is signed", 200);
                            ui.end_row();
                            label(ui, &t, "Email address");
                            let email = field(ui, &mut f.email, "email", "you@example.com", 320);
                            if window.focus {
                                email.request_focus();
                                window.focus = false;
                            }
                            ui.end_row();
                            label(ui, &t, "Shown as");
                            field(ui, &mut f.name, "name", "the address", 100);
                            ui.end_row();
                        });

                        section(ui, &t, "Servers", "account-servers", |ui| {
                            label(ui, &t, "Account type");
                            ui.horizontal(|ui| {
                                let was = f.pop3;
                                ui.radio_value(&mut f.pop3, false, "IMAP").on_hover_text("Mail stays on the server and is read from it");
                                ui.radio_value(&mut f.pop3, true, "POP3").on_hover_text("Mail is taken down and kept here");
                                if was != f.pop3 {
                                    f.in_port.clear();
                                }
                            });
                            ui.end_row();
                            label(ui, &t, "Incoming server");
                            field(ui, &mut f.in_host, "in-host", if f.pop3 { "pop.example.com" } else { "imap.example.com" }, 253);
                            ui.end_row();
                            label(ui, &t, "");
                            ui.horizontal(|ui| {
                                security(ui, "in", &mut f.in_sec, &mut f.in_port);
                                port(ui, &mut f.in_port, "in-port");
                            });
                            ui.end_row();
                            label(ui, &t, "Outgoing (SMTP)");
                            field(ui, &mut f.out_host, "out-host", "the incoming server", 253);
                            ui.end_row();
                            label(ui, &t, "");
                            ui.horizontal(|ui| {
                                security(ui, "out", &mut f.out_sec, &mut f.out_port);
                                port(ui, &mut f.out_port, "out-port");
                            });
                            ui.end_row();
                        });

                        section(ui, &t, "Signing in", "account-sign-in", |ui| {
                            label(ui, &t, "User name");
                            field(ui, &mut f.user, "user", "the address", 320);
                            ui.end_row();
                            label(ui, &t, "Password");
                            let hint = if f.has_password { "kept; type to change it" } else { "not kept yet" };
                            ui.horizontal(|ui| {
                                ui.add(
                                    LineEdit::new(&mut f.password, egui::Id::new(("account-field", "password")))
                                        .desired_width(FIELD_W - 26.0)
                                        .char_limit(500)
                                        .hint(hint, None)
                                        .password(),
                                );
                                if f.has_password {
                                    mark(ui, &t, true).on_hover_text("A password is kept in the credential store of this system");
                                }
                            });
                            ui.end_row();
                            label(ui, &t, "");
                            ui.checkbox(&mut f.sign_in, "Sign in to the outgoing server the same way");
                            ui.end_row();
                            if f.pop3 {
                                label(ui, &t, "");
                                ui.checkbox(&mut f.leave, "Leave a copy of each message on the server");
                                ui.end_row();
                            }
                        });

                        // Where its mail is kept.
                        ui.horizontal(|ui| {
                            ui.add_space(LABEL_W + 10.0);
                            let (words, full) = match f.data_file.as_str() {
                                "" if f.id.is_none() => {
                                    ("Its mail is kept in a data file of its own, made when it is saved".to_string(), String::new())
                                }
                                "" => ("Its mail goes in the data file new mail is kept in".to_string(), String::new()),
                                path => {
                                    let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                                    (format!("Its mail is kept in {}", one_line(&name)), one_line(path))
                                }
                            };
                            let shown = ui.add(egui::Label::new(egui::RichText::new(words).font(font(11.5)).color(t.text_dim)).wrap());
                            if !full.is_empty() {
                                shown.on_hover_text(full);
                            }
                        });
                    });
                });

                // What the last test or save came to: a mark for each server.
                if busy || !window.said.is_empty() {
                    ui.add_space(10.0);
                    egui::Frame::new()
                        .fill(t.pane)
                        .stroke(egui::Stroke::new(1.0, t.line))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            if busy {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(egui::RichText::new("Signing in to both servers; nothing is sent…").font(font(12.5)).color(t.text_dim));
                                });
                            }
                            for (words, trouble) in &window.said {
                                ui.horizontal(|ui| {
                                    mark(ui, &t, !*trouble);
                                    let color: Color32 = if *trouble { t.danger } else { t.text };
                                    ui.add(egui::Label::new(egui::RichText::new(words.as_str()).font(font(12.5)).color(color)).wrap());
                                });
                            }
                        });
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("A test signs in to both servers; nothing is sent.").font(font(11.5)).color(t.text_faint));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            ask = Some(Ask::Close);
                        }
                        let can_test = self.session.connector.is_some();
                        if ui
                            .add_enabled(!busy && can_test, egui::Button::new("Test Account Settings…"))
                            .on_disabled_hover_text(if can_test { "A test is under way" } else { "This build cannot reach mail servers" })
                            .clicked()
                        {
                            ask = Some(Ask::Test);
                        }
                        let save = egui::Button::new(egui::RichText::new("Save").color(t.on_accent).font(bold(13.0)))
                            .fill(t.accent)
                            .min_size(vec2(76.0, 0.0));
                        if ui.add_enabled(!busy, save).clicked() {
                            ask = Some(Ask::Save);
                        }
                    });
                });
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    ask = Some(Ask::Close);
                }
            },
        );
        if !matches!(ask, Some(Ask::Remove) | None) {
            window.removing = false;
        }
        match ask {
            None => {}
            Some(Ask::Close) => return,
            Some(Ask::Choose(id)) => {
                window.said.clear();
                self.show_account(&mut window, &id);
            }
            Some(Ask::New) => {
                forget(&mut window.form.password);
                window.form = Form::blank();
                window.said.clear();
                window.focus = true;
            }
            Some(Ask::Save) => {
                let _ = self.save_account(&mut window);
            }
            Some(Ask::Test) => self.test_account(&mut window),
            Some(Ask::Remove) if !window.removing => window.removing = true,
            Some(Ask::Remove) => {
                window.removing = false;
                if let Some(id) = window.form.id.clone() {
                    match self.execute("account.remove", &json!({ "id": id })) {
                        Ok(_) => {
                            let next = self.list_accounts(&mut window).ok().and_then(|all| all.first().cloned());
                            forget(&mut window.form.password);
                            window.form = next.as_ref().map_or_else(Form::blank, Form::of);
                            window.said = vec![("The account was removed, and its password with it".to_string(), false)];
                        }
                        Err(e) => window.said = vec![(e, true)],
                    }
                }
            }
        }
        self.account_window = Some(window);
    }
}

/// The widths the window is laid out in.
const LIST_W: f32 = 210.0;
const FORM_W: f32 = 440.0;
const LABEL_W: f32 = 120.0;
const FIELD_W: f32 = 290.0;

/// One account in the list: a round mark with its first letter, its name, and what it is.
fn account_card(ui: &mut egui::Ui, t: &Tokens, name: &str, detail: &str, on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), egui::Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 5.0, t.selected);
        p.rect_filled(egui::Rect::from_min_size(rect.min + vec2(0.0, 8.0), vec2(3.0, rect.height() - 16.0)), 1.5, t.accent);
    } else if response.hovered() {
        p.rect_filled(rect, 5.0, t.hover);
    }
    let centre = egui::pos2(rect.left() + 22.0, rect.center().y);
    p.circle_filled(centre, 13.0, if on { t.accent } else { t.line });
    let letter = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
    p.text(centre, Align2::CENTER_CENTER, letter, bold(12.5), if on { t.on_accent } else { t.text });
    let clip = egui::Rect::from_min_max(egui::pos2(rect.left() + 42.0, rect.top()), egui::pos2(rect.right() - 4.0, rect.bottom()));
    let text = p.with_clip_rect(clip);
    text.text(egui::pos2(clip.left(), rect.center().y - 8.0), Align2::LEFT_CENTER, name, bold(12.5), t.text);
    text.text(egui::pos2(clip.left(), rect.center().y + 8.0), Align2::LEFT_CENTER, detail, font(11.0), t.text_dim);
    response.on_hover_text(format!("{name}\n{detail}"))
}

/// A part of the form: its heading over a line, and its fields in a grid whose label column is
/// as wide as every other part's, so the fields line up down the window.
fn section(ui: &mut egui::Ui, t: &Tokens, heading: &str, id: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.label(egui::RichText::new(heading).font(bold(12.5)).color(t.accent));
    let (y, left) = (ui.cursor().top() + 1.0, ui.min_rect().left());
    ui.painter().hline(left..=left + FORM_W, y, egui::Stroke::new(1.0, t.line));
    ui.add_space(6.0);
    egui::Grid::new(id).num_columns(2).min_col_width(LABEL_W).spacing(vec2(10.0, 7.0)).show(ui, body);
    ui.add_space(10.0);
}

/// A field's label, set against the field at the right of its column.
fn label(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.allocate_ui_with_layout(vec2(LABEL_W, 22.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(egui::RichText::new(text).color(t.text_dim));
    });
}

/// A round mark: a tick for what went well, a cross for what did not.
fn mark(ui: &mut egui::Ui, t: &Tokens, good: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
    let p = ui.painter();
    p.circle_filled(rect.center(), 8.0, if good { t.ok } else { t.danger });
    let ink = egui::Stroke::new(1.8, Color32::WHITE);
    let c = rect.center();
    if good {
        p.line_segment([c + vec2(-4.0, 0.2), c + vec2(-1.2, 3.0)], ink);
        p.line_segment([c + vec2(-1.2, 3.0), c + vec2(4.0, -2.8)], ink);
    } else {
        p.line_segment([c + vec2(-3.0, -3.0), c + vec2(3.0, 3.0)], ink);
        p.line_segment([c + vec2(3.0, -3.0), c + vec2(-3.0, 3.0)], ink);
    }
    response
}
