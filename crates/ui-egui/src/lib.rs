//! MailCraft's egui front end: an Outlook-Classic-style window (title bar with search, ribbon,
//! navigation strip, folder pane, message list, reading pane, status bar).
//!
//! The UI is thin. It reads [`Session`] state and changes it only by running commands, the same
//! ids the CLI and the control channel use. Commands that only make sense in a window (`ui.*`)
//! live here. Message text is drawn as text: nothing in a message is ever run or fetched.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod body;
pub mod icons;
mod panes;
mod ribbon;
pub mod theme;

use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Key, Modifiers, Painter, Pos2, Rect, Sense, Ui, pos2, vec2};
use icons::Icon;
use mailcraft_engine::{Module, Session};
use mailcraft_formats::rich::Doc;
use mailcraft_model::{PropValue, pid};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use theme::Tokens;

/// Ribbon tabs. `File` shows the full-window file page instead of a ribbon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    File,
    Home,
    SendReceive,
    Folder,
    View,
    Help,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::File, Tab::Home, Tab::SendReceive, Tab::Folder, Tab::View, Tab::Help];

    pub fn label(self) -> &'static str {
        match self {
            Tab::File => "File",
            Tab::Home => "Home",
            Tab::SendReceive => "Send / Receive",
            Tab::Folder => "Folder",
            Tab::View => "View",
            Tab::Help => "Help",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let key: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        Tab::ALL.into_iter().find(|t| t.label().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase() == key)
    }
}

/// Things only the host application can do (native dialogs, closing the window). The UI queues
/// them; the app drains [`MailUi::requests`] each frame. Headless hosts ignore them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiRequest {
    OpenFile,
    SaveAs,
    SaveAttachment(usize),
    Attach,
    Quit,
}

/// A window-only command.
pub struct UiCommand {
    pub id: &'static str,
    pub label: &'static str,
    pub params: &'static str,
}

pub const UI_COMMANDS: &[UiCommand] = &[
    UiCommand { id: "ui.ribbonTab", label: "Show Ribbon Tab", params: r#"{"tab": "file" | "home" | "sendreceive" | "folder" | "view" | "help"}"# },
    UiCommand { id: "ui.openDialog", label: "Open…", params: "" },
    UiCommand { id: "ui.saveAsDialog", label: "Save As…", params: "" },
    UiCommand { id: "ui.saveAttachmentDialog", label: "Save Attachment…", params: r#"{"index": attachment number}"# },
    UiCommand { id: "ui.attachDialog", label: "Attach File…", params: "" },
    UiCommand { id: "ui.toggleFolder", label: "Expand or Collapse Folder", params: r#"{"store": index, "folderId": id}"# },
    UiCommand { id: "ui.prompt", label: "Ask for a Name", params: r#"{"command": "folder.new" | "folder.rename"}"# },
    UiCommand {
        id: "ui.menu",
        label: "Open Menu",
        params: r#"{"menu": "categorize" | "followup" | "move" | "arrange" | "reading", "x": px, "y": px}"#,
    },
    UiCommand { id: "ui.ribbonMode", label: "Simplified Ribbon", params: r#"{"mode": "classic" | "simplified"}; none switches"# },
    UiCommand { id: "ui.theme", label: "Dark Mode", params: r#"{"theme": "light" | "dark"}; none switches"# },
    UiCommand { id: "ui.bodyFormat", label: "Read as Plain Text", params: r#"{"format": "formatted" | "plain"}; none switches"# },
    UiCommand { id: "ui.about", label: "About MailCraft", params: "" },
    UiCommand { id: "ui.quit", label: "Exit", params: "" },
];

/// The ribbon's two layouts: three rows with labelled groups, or a single line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RibbonMode {
    Classic,
    Simplified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Menu {
    Categorize,
    FollowUp,
    Move,
    Arrange,
    Reading,
}

pub(crate) struct Prompt {
    pub title: &'static str,
    pub command: &'static str,
    pub value: String,
    pub focus: bool,
}

pub struct MailUi {
    pub session: Session,
    pub requests: Vec<UiRequest>,
    pub tokens: Tokens,
    pub(crate) tab: Tab,
    pub(crate) ribbon_mode: RibbonMode,
    theme_applied: Option<bool>,
    /// Messages are read as plain text even when they have a formatted body.
    pub(crate) plain_bodies: bool,
    /// The open message's formatted body, kept until another message is opened.
    body_doc: Option<(u64, Arc<Doc>)>,
    pub(crate) actions: Vec<(String, Value)>,
    pub(crate) search: String,
    pub(crate) collapsed: HashSet<(usize, u64)>,
    pub(crate) menu: Option<(Menu, Pos2)>,
    pub(crate) menu_fresh: bool,
    pub(crate) prompt: Option<Prompt>,
    pub(crate) scroll_to_selected: bool,
    fonts_ready: bool,
}

pub(crate) fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub(crate) fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// One line of untrusted text: control characters flattened and length capped before layout.
pub(crate) fn one_line(s: &str) -> String {
    s.chars().take(400).map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// Draw one line clipped to `width` with an ellipsis; returns the width used.
pub(crate) fn clip_text(p: &Painter, pos: Pos2, width: f32, text: &str, font: FontId, color: Color32) -> f32 {
    if width < 6.0 {
        return 0.0;
    }
    let mut job = LayoutJob::simple_singleline(one_line(text), font, color);
    job.wrap = TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = p.layout_job(job);
    let used = galley.size().x;
    p.galley(pos, galley, color);
    used
}

pub(crate) fn text_width(p: &Painter, text: &str, font: FontId) -> f32 {
    p.layout_no_wrap(text.to_owned(), font, Color32::BLACK).size().x
}

/// System UI fonts, used when installed. Nothing proprietary is bundled: without them the
/// built-in open fonts are used.
fn install_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let mut bold_family = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let candidates: [(&str, &[&str]); 2] = [
        (
            "ui-regular",
            &["C:\\Windows\\Fonts\\segoeui.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/System/Library/Fonts/Supplemental/Arial.ttf"],
        ),
        (
            "ui-bold",
            &[
                "C:\\Windows\\Fonts\\segoeuib.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
                "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
            ],
        ),
    ];
    for (name, paths) in candidates {
        let Some(bytes) = paths.iter().find_map(|p| std::fs::read(p).ok()) else { continue };
        defs.font_data.insert(name.to_string(), Arc::new(FontData::from_owned(bytes)));
        if name == "ui-regular" {
            defs.families.entry(FontFamily::Proportional).or_default().insert(0, name.to_string());
            bold_family.insert(0, name.to_string());
        } else {
            bold_family.insert(0, name.to_string());
        }
    }
    defs.families.insert(FontFamily::Name("bold".into()), bold_family);
    ctx.set_fonts(defs);
}

impl MailUi {
    pub fn new(session: Session) -> Self {
        let search = session.search.clone();
        Self {
            session,
            requests: Vec::new(),
            tokens: Tokens::default(),
            tab: Tab::Home,
            ribbon_mode: RibbonMode::Simplified,
            theme_applied: None,
            plain_bodies: false,
            body_doc: None,
            actions: Vec::new(),
            search,
            collapsed: HashSet::new(),
            menu: None,
            menu_fresh: false,
            prompt: None,
            scroll_to_selected: false,
            fonts_ready: false,
        }
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    pub fn ribbon_mode(&self) -> RibbonMode {
        self.ribbon_mode
    }

    pub(crate) fn simplified(&self) -> bool {
        self.ribbon_mode == RibbonMode::Simplified
    }

    /// Run a command by id: engine commands and `ui.*` commands alike. Never opens a dialog by
    /// itself; dialog commands only queue a [`UiRequest`] for the host.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        if id.starts_with("ui.") {
            return self.ui_command(id, params);
        }
        let before = self.session.selected;
        let result = self.session.run(id, params).map_err(|e| e.to_string());
        if self.session.selected != before {
            self.scroll_to_selected = true;
        }
        if id.starts_with("search.") || id.starts_with("folder.") {
            self.search = self.session.search.clone();
        }
        result
    }

    fn ui_command(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let text = |key: &str| params.get(key).and_then(Value::as_str);
        let number = |key: &str| params.get(key).and_then(Value::as_u64);
        match id {
            "ui.ribbonTab" => {
                let name = text("tab").ok_or("\"tab\" is missing")?;
                self.menu = None;
                self.tab = Tab::parse(name).ok_or_else(|| format!("\"{}\" is not a ribbon tab", one_line(name)))?;
            }
            "ui.openDialog" => self.requests.push(UiRequest::OpenFile),
            "ui.saveAsDialog" => self.requests.push(UiRequest::SaveAs),
            "ui.saveAttachmentDialog" => {
                self.requests.push(UiRequest::SaveAttachment(number("index").and_then(|n| usize::try_from(n).ok()).unwrap_or(0)))
            }
            "ui.attachDialog" => self.requests.push(UiRequest::Attach),
            "ui.toggleFolder" => {
                let key = (number("store").and_then(|n| usize::try_from(n).ok()).unwrap_or(0), number("folderId").ok_or("\"folderId\" is missing")?);
                if !self.collapsed.remove(&key) {
                    self.collapsed.insert(key);
                }
            }
            "ui.prompt" => {
                let (title, command) = match text("command") {
                    Some("folder.new") => ("New folder name", "folder.new"),
                    Some("folder.rename") => ("Rename folder to", "folder.rename"),
                    _ => return Err("\"command\" must be folder.new or folder.rename".to_string()),
                };
                if !self.session.is_enabled(command) {
                    return Err("That is not available for this folder".to_string());
                }
                let value = if command == "folder.rename" {
                    self.session.current_folder().map(|f| f.name.clone()).unwrap_or_default()
                } else {
                    String::new()
                };
                self.prompt = Some(Prompt { title, command, value, focus: true });
            }
            "ui.menu" => {
                let menu = match text("menu") {
                    Some("categorize") => Menu::Categorize,
                    Some("followup") => Menu::FollowUp,
                    Some("move") => Menu::Move,
                    Some("arrange") => Menu::Arrange,
                    Some("reading") => Menu::Reading,
                    _ => return Err("unknown menu".to_string()),
                };
                let coord = |key: &str| params.get(key).and_then(Value::as_f64).map_or(100.0, |v| v.clamp(0.0, 20_000.0) as f32);
                self.menu = Some((menu, pos2(coord("x"), coord("y"))));
                self.menu_fresh = true;
            }
            "ui.ribbonMode" => {
                self.menu = None;
                self.ribbon_mode = match (params.get("mode"), text("mode")) {
                    (None, _) if self.simplified() => RibbonMode::Classic,
                    (None, _) => RibbonMode::Simplified,
                    (_, Some("classic")) => RibbonMode::Classic,
                    (_, Some("simplified")) => RibbonMode::Simplified,
                    _ => return Err("\"mode\" must be classic or simplified".to_string()),
                };
            }
            "ui.theme" => {
                let dark = match (params.get("theme"), text("theme")) {
                    (None, _) => !self.tokens.dark,
                    (_, Some("light")) => false,
                    (_, Some("dark")) => true,
                    _ => return Err("\"theme\" must be light or dark".to_string()),
                };
                self.tokens = Tokens::named(dark);
            }
            "ui.bodyFormat" => {
                self.plain_bodies = match (params.get("format"), text("format")) {
                    (None, _) => !self.plain_bodies,
                    (_, Some("formatted")) => false,
                    (_, Some("plain")) => true,
                    _ => return Err("\"format\" must be formatted or plain".to_string()),
                };
            }
            "ui.about" => {
                self.session.status =
                    format!("MailCraft {}: an independent open-source mail client. Not affiliated with Microsoft.", env!("CARGO_PKG_VERSION"));
            }
            "ui.quit" => self.requests.push(UiRequest::Quit),
            _ => return Err(format!("unknown command {}", one_line(id))),
        }
        Ok(Value::Null)
    }

    /// The open message's HTML body as a formatted document, read once per message. `None` when
    /// the message has no HTML body, the body holds nothing to show, or plain reading is on.
    pub(crate) fn body_doc(&mut self) -> Option<Arc<Doc>> {
        if self.plain_bodies {
            return None;
        }
        let item = self.session.open.as_ref()?;
        let len = match item.props.get(pid::HTML) {
            Some(PropValue::Binary(b)) => b.len(),
            Some(PropValue::String(s)) => s.len(),
            _ => return None,
        };
        let mut hasher = DefaultHasher::new();
        (self.session.selected, len, item.subject()).hash(&mut hasher);
        let key = hasher.finish();
        let doc = match &self.body_doc {
            Some((k, doc)) if *k == key => doc.clone(),
            _ => {
                let doc = Arc::new(mailcraft_formats::html::parse(&item.body_html()?));
                self.body_doc = Some((key, doc.clone()));
                doc
            }
        };
        (!doc.is_empty()).then_some(doc)
    }

    pub fn plain_bodies(&self) -> bool {
        self.plain_bodies
    }

    pub(crate) fn act(&mut self, id: &str, params: Value) {
        self.actions.push((id.to_string(), params));
    }

    /// Draw one frame into the whole of `ui`.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let full = ctx.content_rect();
        if !self.fonts_ready {
            // Fonts take effect next frame; the "bold" family must exist before anything uses it.
            install_fonts(&ctx);
            self.fonts_ready = true;
            ui.painter().rect_filled(full, 0.0, self.tokens.window);
            ctx.request_repaint();
            return;
        }
        let t = self.tokens.clone();
        if self.theme_applied != Some(t.dark) {
            // egui's own widgets (text fields, the name prompt) follow the same light or dark set.
            ctx.set_theme(if t.dark { egui::Theme::Dark } else { egui::Theme::Light });
            self.theme_applied = Some(t.dark);
        }
        ui.painter().rect_filled(full, 0.0, t.window);
        self.shortcuts(ui);

        let simple = self.simplified();
        let title = Rect::from_min_size(full.min, vec2(full.width(), if simple { t.simple_title_h } else { t.title_h }));
        let status = Rect::from_min_max(pos2(full.left(), full.bottom() - t.status_h), full.max);
        self.title_bar(ui, title);
        if self.tab == Tab::File {
            let tabs = Rect::from_min_size(title.left_bottom(), vec2(full.width(), t.tabs_h));
            self.tab_strip(ui, tabs);
            self.backstage(ui, Rect::from_min_max(tabs.left_bottom(), full.max));
        } else if simple {
            // The module strip runs the full height on the left; tabs, ribbon and panes sit beside it.
            let nav = Rect::from_min_max(title.left_bottom(), pos2(full.left() + t.simple_nav_w, status.top()));
            self.nav_strip(ui, nav);
            let tabs = Rect::from_min_max(nav.right_top(), pos2(full.right(), nav.top() + t.tabs_h));
            self.tab_strip(ui, tabs);
            let top = tabs.bottom() + 2.0;
            let ribbon = Rect::from_min_max(pos2(tabs.left(), top), pos2(full.right() - 16.0, top + t.simple_ribbon_h));
            self.ribbon_simple(ui, ribbon);
            let body = Rect::from_min_max(pos2(tabs.left(), ribbon.bottom() + 7.0), status.right_top());
            self.body(ui, body, false);
            self.status_bar(ui, status);
        } else {
            let tabs = Rect::from_min_size(title.left_bottom(), vec2(full.width(), t.tabs_h));
            self.tab_strip(ui, tabs);
            let ribbon = Rect::from_min_size(tabs.left_bottom(), vec2(full.width(), t.ribbon_h));
            self.ribbon(ui, ribbon);
            let body = Rect::from_min_max(ribbon.left_bottom(), status.right_top());
            self.body(ui, body, true);
            self.status_bar(ui, status);
        }
        self.menu_popup(&ctx);
        self.prompt_window(&ctx);

        for (id, params) in std::mem::take(&mut self.actions) {
            if let Err(e) = self.execute(&id, &params) {
                self.session.status = e;
            }
        }
    }

    fn shortcuts(&mut self, ui: &mut Ui) {
        if self.prompt.is_some() {
            return;
        }
        let typing = ui.ctx().memory(|m| m.focused().is_some());
        let hit = |mods: Modifiers, key: Key| ui.input_mut(|i| i.consume_key(mods, key));
        let ctrl = Modifiers::COMMAND;
        let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let mut run: Vec<(&str, Value)> = Vec::new();
        // Longer chords first: consume_key matches a chord whose modifiers are a superset.
        if hit(ctrl_shift, Key::R) {
            run.push(("mail.replyAll", Value::Null));
        }
        if hit(ctrl_shift, Key::E) {
            run.push(("ui.prompt", json!({ "command": "folder.new" })));
        }
        for (key, id) in [
            (Key::N, "mail.new"),
            (Key::R, "mail.reply"),
            (Key::F, "mail.forward"),
            (Key::O, "ui.openDialog"),
            (Key::Q, "mail.markRead"),
            (Key::U, "mail.markUnread"),
            (Key::D, "mail.delete"),
            (Key::Enter, "compose.send"),
            (Key::S, "compose.saveDraft"),
        ] {
            if hit(ctrl, key) {
                run.push((id, Value::Null));
            }
        }
        for (key, module) in [(Key::Num1, "mail"), (Key::Num2, "calendar"), (Key::Num3, "people"), (Key::Num4, "tasks")] {
            if hit(ctrl, key) {
                run.push(("nav.module", json!({ "module": module })));
            }
        }
        if hit(Modifiers::NONE, Key::F12) {
            run.push(("ui.saveAsDialog", Value::Null));
        }
        if !typing {
            for (key, id) in [
                (Key::ArrowDown, "mail.next"),
                (Key::ArrowUp, "mail.previous"),
                (Key::Delete, "mail.delete"),
                (Key::Insert, "mail.flag"),
                (Key::Backspace, "mail.archive"),
            ] {
                if hit(Modifiers::NONE, key) {
                    run.push((id, Value::Null));
                }
            }
            if hit(Modifiers::NONE, Key::F2) {
                run.push(("ui.prompt", json!({ "command": "folder.rename" })));
            }
        }
        if hit(Modifiers::NONE, Key::Escape) {
            if self.menu.is_some() {
                self.menu = None;
            } else if !self.session.search.is_empty() {
                run.push(("search.clear", Value::Null));
            }
        }
        for (id, params) in run {
            if id.starts_with("ui.") || self.session.is_enabled(id) {
                self.act(id, params);
            }
        }
    }

    fn title_bar(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.title_bg);
        icons::paint(&p, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(16.0, 16.0)), Icon::Mail, t.title_text);
        let place = self.session.current_folder().map(|f| f.name.clone()).unwrap_or_else(|| "No data file".to_string());
        let store = self.session.current.and_then(|(s, _)| self.session.stores.get(s)).map(|s| s.display_name()).unwrap_or_default();
        let caption = if store.is_empty() { format!("{place} - MailCraft") } else { format!("{place} - {store} - MailCraft") };

        let search = if self.simplified() {
            // Lined up with the message list's left edge.
            let left = rect.left() + t.simple_nav_w + if self.session.folder_pane { t.folder_w } else { 0.0 };
            let left = left.min(rect.right() - 170.0).max(rect.left() + 44.0);
            Rect::from_min_size(pos2(left, rect.center().y - 15.0), vec2((rect.right() - left - 150.0).clamp(120.0, 304.0), 30.0))
        } else {
            Rect::from_center_size(rect.center(), vec2((rect.width() * 0.36).clamp(160.0, 520.0), 24.0))
        };
        clip_text(&p, pos2(rect.left() + 40.0, rect.center().y - 8.0), search.left() - rect.left() - 52.0, &caption, font(12.5), t.title_text);
        p.rect_filled(search, 4.0, t.title_field);
        icons::paint(&p, Rect::from_center_size(pos2(search.left() + 14.0, search.center().y), vec2(13.0, 13.0)), Icon::Search, t.title_text);
        let edit_rect =
            Rect::from_min_max(pos2(search.left() + 26.0, search.center().y - 10.0), pos2(search.right() - 6.0, search.center().y + 10.0));
        let before = self.search.clone();
        let edit = egui::TextEdit::singleline(&mut self.search)
            .id(egui::Id::new("mailcraft-search"))
            .frame(egui::Frame::NONE)
            .hint_text(egui::RichText::new("Search").color(t.title_text.gamma_multiply(0.75)))
            .text_color(t.title_text)
            .font(font(12.5))
            .desired_width(edit_rect.width());
        ui.put(edit_rect, edit);
        if self.search != before {
            if self.search.trim().is_empty() {
                self.act("search.clear", Value::Null);
            } else if self.session.is_enabled("search.query") {
                self.act("search.query", json!({ "text": self.search }));
            }
        }
    }

    fn tab_strip(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.chrome);
        let mut x = rect.left() + 6.0;
        for tab in Tab::ALL {
            let w = text_width(&p, tab.label(), font(12.5)) + 24.0;
            let r = Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height()));
            let resp = ui.interact(r, egui::Id::new(("tab", tab.label())), Sense::click());
            let is_file = tab == Tab::File;
            let active = self.tab == tab;
            if is_file {
                p.rect_filled(r.shrink2(vec2(2.0, 3.0)), 3.0, if resp.hovered() { t.accent_dark } else { t.accent });
            } else if resp.hovered() && !active {
                p.rect_filled(r.shrink2(vec2(2.0, 3.0)), 3.0, t.hover);
            }
            let color = if is_file {
                t.on_accent
            } else if active {
                t.accent
            } else {
                t.text
            };
            p.text(r.center(), Align2::CENTER_CENTER, tab.label(), if active && !is_file { bold(12.5) } else { font(12.5) }, color);
            if active && !is_file {
                p.rect_filled(Rect::from_min_max(pos2(r.left() + 10.0, r.bottom() - 3.0), pos2(r.right() - 10.0, r.bottom())), 1.5, t.accent);
            }
            if resp.clicked() {
                let next = if is_file && active { Tab::Home } else { tab };
                self.act("ui.ribbonTab", json!({ "tab": next.label() }));
            }
            x += w;
        }
    }

    fn status_bar(&mut self, ui: &mut Ui, rect: Rect) {
        let t = &self.tokens;
        let s = &self.session;
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.bar_bg);
        let y = rect.center().y - 7.5;
        let mut x = rect.left() + 10.0;
        if s.module == Module::Mail {
            let unread = s.current_folder().map_or(0, |f| f.unread);
            let counts =
                if s.search.is_empty() && !s.unread_only { format!("Items: {}", s.rows.len()) } else { format!("Filtered: {}", s.rows.len()) };
            x += clip_text(&p, pos2(x, y), 160.0, &counts, font(11.5), t.bar_text) + 16.0;
            if unread > 0 {
                x += clip_text(&p, pos2(x, y), 160.0, &format!("Unread: {unread}"), font(11.5), t.bar_text) + 16.0;
            }
        }
        let right = if s.stores.is_empty() {
            "No data file open"
        } else if s.current_writable() {
            "Local data file"
        } else {
            "Read-only data file"
        };
        let right_w = text_width(&p, right, font(11.5));
        p.text(pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, right, font(11.5), t.bar_text);
        clip_text(&p, pos2(x, y), rect.right() - right_w - 30.0 - x, &s.status, font(11.5), t.bar_text);
    }
}

/// Minutes east of UTC for this machine right now; hosts set [`Session::tz_offset_min`] from it.
pub fn local_offset_minutes() -> i32 {
    chrono::Local::now().offset().local_minus_utc() / 60
}

#[cfg(test)]
mod tests;
