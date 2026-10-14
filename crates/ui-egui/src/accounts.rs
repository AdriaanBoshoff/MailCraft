//! Account Settings: the accounts mail is sent and received through, added, changed, tested and
//! removed in a window of their own. Everything is done by the `account.*` commands, as the
//! command line and an agent do it. The password is typed into a field that shows dots, handed
//! to the system's credential store when the account is saved, and forgotten by the window then:
//! it is never shown, copied out, put in the status line or kept in the settings.

use crate::line_edit::LineEdit;
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

/// The Account Settings window.
pub(crate) struct AccountsWindow {
    /// Each account's id and the name it is shown by, as `account.list` last said.
    pub accounts: Vec<(String, String)>,
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
            .filter_map(|a| Some((a.get("id")?.as_str()?.to_string(), one_line(a.get("name").and_then(Value::as_str).unwrap_or("")))))
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
        egui::Window::new("Account Settings").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -20.0)).show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(180.0);
                    ui.label(egui::RichText::new("Accounts").font(bold(13.0)).color(t.text));
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (id, name) in &window.accounts {
                            let on = window.form.id.as_deref() == Some(id.as_str());
                            if ui.selectable_label(on, name.as_str()).clicked() && !on {
                                ask = Some(Ask::Choose(id.clone()));
                            }
                        }
                        if window.form.id.is_none() {
                            let _ = ui.selectable_label(true, "New account");
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!busy, egui::Button::new("New…")).clicked() {
                            ask = Some(Ask::New);
                        }
                        let remove = if window.removing { "Remove it" } else { "Remove…" };
                        if ui.add_enabled(window.form.id.is_some() && !busy, egui::Button::new(remove)).clicked() {
                            ask = Some(Ask::Remove);
                        }
                    });
                    if window.removing {
                        ui.add(
                            egui::Label::new(egui::RichText::new("Mail already taken down stays where it is.").font(font(11.5)).color(t.text_dim))
                                .wrap(),
                        );
                    }
                });
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    let f = &mut window.form;
                    let field = |ui: &mut egui::Ui, text: &mut String, key: &str, hint: &str, limit: usize| {
                        ui.add(LineEdit::new(text, egui::Id::new(("account-field", key))).desired_width(250.0).char_limit(limit).hint(hint, None))
                    };
                    let heading = |ui: &mut egui::Ui, text: &str| {
                        ui.label(egui::RichText::new(text).font(bold(12.5)).color(t.text));
                        ui.end_row();
                    };
                    let security = |ui: &mut egui::Ui, key: &str, sec: &mut &'static str, port: &mut String| {
                        let shown = SECURITY.iter().find(|(k, _)| k == sec).map_or("SSL/TLS", |(_, s)| s);
                        egui::ComboBox::from_id_salt(("account-security", key)).selected_text(shown).width(130.0).show_ui(ui, |ui| {
                            for (k, label) in SECURITY {
                                if ui.selectable_label(*sec == k, label).clicked() && *sec != k {
                                    *sec = k;
                                    // The usual port for the new security, unless one is typed.
                                    port.clear();
                                }
                            }
                        });
                    };
                    egui::Grid::new("account-form").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                        heading(ui, "You");
                        ui.label("Your name");
                        field(ui, &mut f.display_name, "display", "as mail from you is signed", 200);
                        ui.end_row();
                        ui.label("Email address");
                        let email = field(ui, &mut f.email, "email", "you@example.com", 320);
                        if window.focus {
                            email.request_focus();
                            window.focus = false;
                        }
                        ui.end_row();
                        ui.label("Shown as");
                        field(ui, &mut f.name, "name", "the address", 100);
                        ui.end_row();

                        heading(ui, "Servers");

                        ui.label("Account type");
                        ui.horizontal(|ui| {
                            let was = f.pop3;
                            ui.radio_value(&mut f.pop3, false, "IMAP");
                            ui.radio_value(&mut f.pop3, true, "POP3");
                            if was != f.pop3 {
                                f.in_port.clear();
                            }
                        });
                        ui.end_row();
                        ui.label("Incoming mail server");
                        field(ui, &mut f.in_host, "in-host", "imap.example.com", 253);
                        ui.end_row();
                        ui.label("");
                        ui.horizontal(|ui| {
                            security(ui, "in", &mut f.in_sec, &mut f.in_port);
                            ui.label("Port");
                            ui.add(
                                LineEdit::new(&mut f.in_port, egui::Id::new(("account-field", "in-port")))
                                    .desired_width(60.0)
                                    .char_limit(5)
                                    .hint("usual", None),
                            );
                        });
                        ui.end_row();
                        ui.label("Outgoing mail server (SMTP)");
                        field(ui, &mut f.out_host, "out-host", "the incoming server", 253);
                        ui.end_row();
                        ui.label("");
                        ui.horizontal(|ui| {
                            security(ui, "out", &mut f.out_sec, &mut f.out_port);
                            ui.label("Port");
                            ui.add(
                                LineEdit::new(&mut f.out_port, egui::Id::new(("account-field", "out-port")))
                                    .desired_width(60.0)
                                    .char_limit(5)
                                    .hint("usual", None),
                            );
                        });
                        ui.end_row();

                        heading(ui, "Signing in");

                        ui.label("User name");
                        field(ui, &mut f.user, "user", "the address", 320);
                        ui.end_row();
                        ui.label("Password");
                        let hint = if f.has_password { "kept; type to change it" } else { "not kept yet" };
                        ui.add(
                            LineEdit::new(&mut f.password, egui::Id::new(("account-field", "password")))
                                .desired_width(250.0)
                                .char_limit(500)
                                .hint(hint, None)
                                .password(),
                        );
                        ui.end_row();
                    });
                    ui.add_space(4.0);
                    ui.checkbox(&mut f.sign_in, "The outgoing server is signed in to with the same name and password");
                    if f.pop3 {
                        ui.checkbox(&mut f.leave, "Leave a copy of each message on the server");
                    }
                });
            });
            ui.add_space(6.0);
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new("Signing in to both servers; nothing is sent…").font(font(12.5)).color(t.text_dim));
                });
            }
            for (words, trouble) in &window.said {
                let color: Color32 = if *trouble { t.danger } else { t.text_dim };
                ui.add(egui::Label::new(egui::RichText::new(words.as_str()).font(font(12.5)).color(color)).wrap());
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy, egui::Button::new("Save")).clicked() {
                    ask = Some(Ask::Save);
                }
                let can_test = self.session.connector.is_some();
                if ui
                    .add_enabled(!busy && can_test, egui::Button::new("Test Account Settings…"))
                    .on_disabled_hover_text(if can_test { "A test is under way" } else { "This build cannot reach mail servers" })
                    .clicked()
                {
                    ask = Some(Ask::Test);
                }
                if ui.button("Close").clicked() {
                    ask = Some(Ask::Close);
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                ask = Some(Ask::Close);
            }
        });
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
