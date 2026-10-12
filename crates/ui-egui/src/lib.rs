//! OpenMailClient's egui front end: an Outlook-Classic-style window (title bar with search, ribbon,
//! navigation strip, folder pane, message list, reading pane, status bar).
//!
//! The UI is thin. It reads [`Session`] state and changes it only by running commands, the same
//! ids the CLI and the control channel use. Commands that only make sense in a window (`ui.*`)
//! live here. Message text is drawn as text: nothing in a message is ever run or fetched.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod body;
mod faces;
pub mod icons;
mod line_edit;
mod panes;
mod pictures;
mod preview;
mod ribbon;
pub mod theme;

use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Key, Modifiers, Painter, Pos2, Rect, Sense, Ui, pos2, vec2};
use icons::Icon;
use openmailclient_engine::{Module, Session};
use openmailclient_formats::rich::Doc;
use openmailclient_model::{PropValue, pid};
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
    Export,
    Print,
    SaveAttachment(usize),
    SaveAllAttachments,
    Attach,
    /// Hand a web address to the system's browser. Queued only when the person presses Open in
    /// the link dialog, with an address the engine has checked.
    OpenLink(String),
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
    UiCommand { id: "ui.exportDialog", label: "Export to a Data File…", params: "" },
    UiCommand { id: "ui.printDialog", label: "Print to PDF…", params: "" },
    UiCommand {
        id: "ui.printPreview",
        label: "Print…",
        params: r#"{"page": which page to show, from 1 (default 1), "close": true to close the window}; shows the pages as they will be printed, with the page setup"#,
    },
    UiCommand { id: "ui.saveAttachmentDialog", label: "Save Attachment…", params: r#"{"index": attachment number}"# },
    UiCommand { id: "ui.saveAllAttachmentsDialog", label: "Save All Attachments…", params: "" },
    UiCommand { id: "ui.attachDialog", label: "Attach File…", params: "" },
    UiCommand {
        id: "ui.linkDialog",
        label: "Open Link…",
        params: r#"{"index": link number from mail.links} or {"address": as the message writes it}; shows where the link goes and asks; nothing is opened until the person answers"#,
    },
    UiCommand { id: "ui.toggleFolder", label: "Expand or Collapse Folder", params: r#"{"store": index, "folderId": id}"# },
    UiCommand { id: "ui.prompt", label: "Ask for a Name", params: r#"{"command": "folder.new" | "folder.rename"}"# },
    UiCommand {
        id: "ui.menu",
        label: "Open Menu",
        params: r#"{"menu": "categorize" | "followup" | "move" | "arrange" | "reading", "x": px, "y": px}"#,
    },
    UiCommand { id: "ui.ribbonMode", label: "Simplified Ribbon", params: r#"{"mode": "classic" | "simplified"}; none switches"# },
    UiCommand {
        id: "ui.layout",
        label: "Pane Sizes",
        params: r#"{"folders": px, "list": px, "listShare": 0.15 to 0.85 of the height with the reading pane at the bottom, "reset": true for the sizes it started with and the window where it started}; each is optional; returns the sizes now"#,
    },
    UiCommand { id: "ui.theme", label: "Dark Mode", params: r#"{"theme": "light" | "dark"}; none switches"# },
    UiCommand { id: "ui.bodyFormat", label: "Read as Plain Text", params: r#"{"format": "formatted" | "plain"}; none switches"# },
    UiCommand { id: "ui.about", label: "About OpenMailClient", params: "" },
    UiCommand { id: "ui.quit", label: "Exit", params: "" },
];

/// Where the window stands, in points: what is kept between runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowPlace {
    /// The inside of the window when it is neither maximized nor minimized.
    pub size: [f32; 2],
    /// Its top left corner, kept only while that is on the first screen.
    pub pos: Option<[f32; 2]>,
    pub maximized: bool,
}

/// The smallest window, and the size of a first one.
pub const MIN_WINDOW: [f32; 2] = [760.0, 480.0];
pub const FIRST_WINDOW: [f32; 2] = [1280.0, 800.0];
/// No screen is larger than this; a settings file that says so is not believed.
const MAX_WINDOW: f32 = 16384.0;
/// Where a window is put when the place it was kept at is not on the screen any more.
const SAFE_CORNER: [f32; 2] = [40.0, 40.0];
const FOLDER_W: (f32, f32) = (120.0, 600.0);
const LIST_W: (f32, f32) = (200.0, 1200.0);
const LIST_SHARE: (f32, f32) = (0.15, 0.85);
pub(crate) const FIRST_LIST_SHARE: f32 = 0.42;

/// True when a window with its corner at `pos` can be reached on a screen of `screen` points
/// that starts at the origin: enough of its title bar is on it to take hold of.
///
/// Only the first screen starts at the origin, and egui does not say where the others are: a
/// place on another screen is not kept, so that a window is never put where no screen is.
pub fn on_first_screen(pos: [f32; 2], screen: [f32; 2]) -> bool {
    let [x, y] = pos;
    let [w, h] = screen;
    [x, y, w, h].iter().all(|v| v.is_finite()) && x >= -8.0 && y >= -8.0 && x + 160.0 <= w && y + 60.0 <= h
}

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

/// A link the person is being asked about.
pub(crate) struct LinkAsk {
    /// As the message writes it: what `mail.openLink` is given.
    pub address: String,
    pub text: String,
    pub goes: Option<String>,
    pub concern: Option<String>,
    /// Why it cannot be opened, when it cannot.
    pub refused: Option<String>,
    /// It is a mailbox: answering starts a message.
    pub mailbox: bool,
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
    /// Exit was asked for once while changes were unsaved.
    quit_asked: bool,
    /// Messages are read as plain text even when they have a formatted body.
    pub(crate) plain_bodies: bool,
    /// The open message's formatted body, kept until another message is opened.
    body_doc: Option<(u64, Arc<Doc>)>,
    pictures: pictures::Pictures,
    faces: faces::Faces,
    pub(crate) actions: Vec<(String, Value)>,
    pub(crate) search: String,
    pub(crate) collapsed: HashSet<(usize, u64)>,
    pub(crate) menu: Option<(Menu, Pos2)>,
    pub(crate) menu_fresh: bool,
    pub(crate) prompt: Option<Prompt>,
    pub(crate) link_ask: Option<LinkAsk>,
    pub(crate) preview: Option<preview::Preview>,
    pub(crate) scroll_to_selected: bool,
    fonts_ready: bool,
    /// The widths of the folder pane and of the list beside the reading pane, and the share
    /// of the height the list takes above it: dragged at their edges, kept between runs.
    pub(crate) folder_w: f32,
    pub(crate) list_w: f32,
    pub(crate) list_share: f32,
    /// Where the window stands; `None` until a frame or a settings file has said.
    pub window: Option<WindowPlace>,
    /// The window was put where the settings said: the first frames see that it is on a screen.
    check_place: bool,
    /// The window is to go back to where a first one stands.
    reset_window: bool,
}

pub(crate) fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub(crate) fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// One line of untrusted text: control characters flattened and length capped before layout.
/// The marks that turn the text after them round or set it apart (UAX #9, 2.1 to 2.4: the
/// embeddings, overrides and isolates) are left out, so a name such as an attachment's cannot
/// be made to show another ending than the one it has; letters keep their own direction.
pub(crate) fn one_line(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        .take(400)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// One line of text as it is seen: a line with right-to-left writing in it is put in the
/// order it is read in (UAX #9, L2), its direction that of its first letter. The text shaper
/// takes each piece it is handed to run one way, so letters written from the right are handed
/// to it apart from everything else, as in a message's body.
pub(crate) fn seen_line(text: &str, font: FontId, color: Color32) -> LayoutJob {
    seen_line_from(text, None, font, color)
}

/// [`seen_line`] for a line whose direction is known from where it stands: `Some(true)` for
/// one read from the right whatever its first letter is.
pub(crate) fn seen_line_from(text: &str, rtl: Option<bool>, font: FontId, color: Color32) -> LayoutJob {
    use openmailclient_fonts::script;
    let text = one_line(text);
    if rtl != Some(true) && !script::has_rtl(&text) {
        return LayoutJob::simple_singleline(text, font, color);
    }
    let chars: Vec<char> = text.chars().collect();
    let rtl = rtl.or_else(|| script::first_strong(&text)).unwrap_or(false);
    let levels = script::levels(&text, Some(rtl));
    // Pieces: characters that stand equally deep and are of one kind (blanks, letters written
    // from the right, anything else).
    let mut pieces: Vec<(usize, usize, u8, u8)> = Vec::new();
    for (at, c) in chars.iter().enumerate() {
        let level = levels.get(at).copied().unwrap_or(0);
        let kind = if c.is_whitespace() {
            0
        } else if script::is_rtl(*c) {
            1
        } else {
            2
        };
        match pieces.last_mut() {
            Some(last) if last.2 == level && last.3 == kind => last.1 = at + 1,
            _ => pieces.push((at, at + 1, level, kind)),
        }
    }
    let stand_ins: Vec<char> = pieces.iter().map(|p| if p.3 == 0 { ' ' } else { 'a' }).collect();
    let depth: Vec<u8> = pieces.iter().map(|p| p.2).collect();
    let format = egui::TextFormat::simple(font, color);
    let mut job = LayoutJob::default();
    for i in script::order(&stand_ins, &depth, u8::from(rtl)) {
        let Some((from, to, level, _)) = pieces.get(i).copied() else { continue };
        let Some(piece) = chars.get(from..to) else { continue };
        body::section_of_its_own(&mut job, &body::spelled(piece, level), 0.0, format.clone());
    }
    job
}

/// Draw one line clipped to `width` with an ellipsis; returns the width used.
pub(crate) fn clip_text(p: &Painter, pos: Pos2, width: f32, text: &str, font: FontId, color: Color32) -> f32 {
    if width < 6.0 {
        return 0.0;
    }
    let galley = if openmailclient_fonts::script::has_rtl(text) {
        // What does not fit is cut from the end of the text as it is read, which is not the
        // right end of the line when the line is read from the right.
        let chars: Vec<char> = one_line(text).chars().collect();
        let cut = |keep: usize| {
            let mut kept: String = chars.iter().take(keep).collect();
            if keep < chars.len() {
                kept.push('…');
            }
            p.layout_job(seen_line(&kept, font.clone(), color))
        };
        let whole = cut(chars.len());
        if whole.size().x <= width {
            whole
        } else {
            let (mut fits, mut too_long) = (0, chars.len());
            while too_long - fits > 1 {
                let middle = fits + (too_long - fits) / 2;
                if cut(middle).size().x <= width {
                    fits = middle;
                } else {
                    too_long = middle;
                }
            }
            cut(fits)
        }
    } else {
        let mut job = LayoutJob::simple_singleline(one_line(text), font, color);
        job.wrap = TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
        p.layout_job(job)
    };
    let used = galley.size().x;
    p.galley(pos, galley, color);
    used
}

/// `text` cut in its middle to fit `width`: its start and its end are kept, since the end of a
/// file name is what tells its kind.
pub(crate) fn middle_cut(p: &Painter, text: &str, width: f32, font: FontId) -> String {
    let chars: Vec<char> = one_line(text).chars().take(400).collect();
    let tail = chars.len().min(24);
    let end: String = chars.iter().skip(chars.len() - tail).collect();
    let mut head = chars.len() - tail;
    while head > 0 {
        head -= 1;
        let cut: String = chars.iter().take(head).chain(['…'].iter()).collect::<String>() + &end;
        if head == 0 || text_width(p, &cut, font.clone()) <= width {
            return cut;
        }
    }
    end
}

pub(crate) fn text_width(p: &Painter, text: &str, font: FontId) -> f32 {
    p.layout_no_wrap(text.to_owned(), font, Color32::BLACK).size().x
}

/// System UI fonts, used when installed. Nothing proprietary is bundled: without them the
/// built-in open fonts are used.
fn install_fonts(ctx: &egui::Context) -> FontDefinitions {
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
    ctx.set_fonts(defs.clone());
    defs
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
            quit_asked: false,
            plain_bodies: false,
            body_doc: None,
            pictures: pictures::Pictures::default(),
            faces: faces::Faces::default(),
            actions: Vec::new(),
            search,
            collapsed: HashSet::new(),
            menu: None,
            menu_fresh: false,
            prompt: None,
            link_ask: None,
            preview: None,
            scroll_to_selected: false,
            fonts_ready: false,
            folder_w: Tokens::default().folder_w,
            list_w: Tokens::default().list_w,
            list_share: FIRST_LIST_SHARE,
            window: None,
            check_place: false,
            reset_window: false,
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

    /// What the window keeps between runs.
    pub fn settings(&self) -> Value {
        json!({
            "theme": if self.tokens.dark { "dark" } else { "light" },
            "ribbon": if self.ribbon_mode == RibbonMode::Classic { "classic" } else { "simplified" },
            "bodyFormat": if self.plain_bodies { "plain" } else { "formatted" },
            "folderWidth": self.folder_w.round(),
            "listWidth": self.list_w.round(),
            "listShare": (f64::from(self.list_share) * 100.0).round() / 100.0,
            "window": self.window.map(|w| json!({
                "width": w.size[0].round(),
                "height": w.size[1].round(),
                "x": w.pos.map(|p| p[0].round()),
                "y": w.pos.map(|p| p[1].round()),
                "maximized": w.maximized,
            })),
        })
    }

    /// The sizes of the panes, each held to what leaves the others room.
    fn set_layout(&mut self, folders: Option<f64>, list: Option<f64>, share: Option<f64>) {
        let held = |v: Option<f64>, (low, high): (f32, f32), now: f32| match v {
            Some(v) if v.is_finite() => (v as f32).clamp(low, high),
            _ => now,
        };
        self.folder_w = held(folders, FOLDER_W, self.folder_w);
        self.list_w = held(list, LIST_W, self.list_w);
        self.list_share = held(share, LIST_SHARE, self.list_share);
    }

    /// Keep where the window stands, as this frame sees it. A maximized window keeps the size
    /// it goes back to; a minimized one changes nothing.
    fn note_window(&mut self, ctx: &egui::Context) {
        let (inner, outer, screen, maximized, minimized, fullscreen) = ctx.input(|i| {
            let v = i.viewport();
            (v.inner_rect, v.outer_rect, v.monitor_size, v.maximized.unwrap_or(false), v.minimized.unwrap_or(false), v.fullscreen.unwrap_or(false))
        });
        if self.reset_window {
            self.reset_window = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(FIRST_WINDOW.into()));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(SAFE_CORNER.into()));
            return;
        }
        let (Some(inner), false) = (inner, minimized) else { return };
        if self.check_place
            && let (Some(outer), Some(screen)) = (outer, screen)
        {
            // The screen may have changed since the place was kept: a window that cannot be
            // reached is brought back to the corner of the first screen.
            self.check_place = false;
            if !maximized && !on_first_screen([outer.left(), outer.top()], [screen.x, screen.y]) {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(SAFE_CORNER.into()));
                return;
            }
        }
        let mut now = self.window.unwrap_or(WindowPlace { size: FIRST_WINDOW, pos: None, maximized: false });
        now.maximized = maximized;
        if !maximized && !fullscreen {
            let size = inner.size();
            if size.x >= MIN_WINDOW[0] - 1.0 && size.y >= MIN_WINDOW[1] - 1.0 && size.x <= MAX_WINDOW && size.y <= MAX_WINDOW {
                now.size = [size.x, size.y];
            }
            now.pos = match (outer, screen) {
                (Some(outer), Some(screen)) if on_first_screen([outer.left(), outer.top()], [screen.x, screen.y]) => {
                    Some([outer.left(), outer.top()])
                }
                _ => None,
            };
        }
        self.window = Some(now);
    }

    /// Take what a settings file says of the window; what makes no sense in it is left out.
    fn apply_settings(&mut self, ui: &Value) {
        for (key, id, param) in [("theme", "ui.theme", "theme"), ("ribbon", "ui.ribbonMode", "mode"), ("bodyFormat", "ui.bodyFormat", "format")] {
            if let Some(v) = ui.get(key).and_then(Value::as_str) {
                let _ = self.ui_command(id, &json!({ param: v }));
            }
        }
        let number = |v: &Value, key: &str| v.get(key).and_then(Value::as_f64).filter(|n| n.is_finite());
        self.set_layout(number(ui, "folderWidth"), number(ui, "listWidth"), number(ui, "listShare"));
        // A window's size is believed only between the smallest window and the largest
        // screen, and its place only where a first screen could be.
        if let Some(w) = ui.get("window").filter(|w| w.is_object()) {
            let side = |key: &str, at: usize| {
                let low = MIN_WINDOW.get(at).copied().unwrap_or(0.0);
                number(w, key).map(|n| n as f32).filter(|n| (low..=MAX_WINDOW).contains(n))
            };
            let size = match (side("width", 0), side("height", 1)) {
                (Some(width), Some(height)) => [width, height],
                _ => self.window.map_or(FIRST_WINDOW, |w| w.size),
            };
            let pos = match (number(w, "x"), number(w, "y")) {
                (Some(x), Some(y)) if on_first_screen([x as f32, y as f32], [MAX_WINDOW, MAX_WINDOW]) => Some([x as f32, y as f32]),
                _ => None,
            };
            self.window = Some(WindowPlace { size, pos, maximized: w.get("maximized").and_then(Value::as_bool).unwrap_or(false) });
            self.check_place = pos.is_some();
        }
    }

    /// True when the window may close. With unsaved changes the first asking only says so;
    /// asking again straight after throws them away. No data file is written by closing.
    pub fn may_quit(&mut self) -> bool {
        if !self.session.unsaved() || self.quit_asked {
            return true;
        }
        self.quit_asked = true;
        // Unsaved changes are journalled beside their data file and come back when it is
        // opened; only when that could not be done does leaving lose them.
        self.session.status = if self.session.unjournalled() {
            "There are unsaved changes: Ctrl+S saves them. Exit again to leave without them.".to_string()
        } else {
            "There are unsaved changes: Ctrl+S saves them. Exit again to leave; they come back, unsaved, when the data file is opened.".to_string()
        };
        false
    }

    /// Run a command by id: engine commands and `ui.*` commands alike. Never opens a dialog by
    /// itself; dialog commands only queue a [`UiRequest`] for the host.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        if id != "ui.quit" {
            self.quit_asked = false;
        }
        if id.starts_with("ui.") {
            return self.ui_command(id, params);
        }
        // The settings file carries what the window keeps for itself beside the engine's own.
        if id == "settings.save" && params.is_object() {
            let mut with_ui = params.clone();
            with_ui["ui"] = self.settings();
            return self.session.run(id, &with_ui).map_err(|e| e.to_string());
        }
        let before = self.session.selected;
        let result = self.session.run(id, params).map_err(|e| e.to_string());
        if id == "settings.load"
            && let Ok(found) = &result
        {
            self.apply_settings(found.get("ui").unwrap_or(&Value::Null));
        }
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
            "ui.exportDialog" => self.requests.push(UiRequest::Export),
            "ui.printDialog" => self.requests.push(UiRequest::Print),
            "ui.printPreview" => {
                if params.get("close").and_then(Value::as_bool) == Some(true) {
                    self.preview = None;
                } else if !self.session.is_enabled("file.print") {
                    return Err("There is nothing to print".to_string());
                } else {
                    self.menu = None;
                    let page = number("page").and_then(|n| usize::try_from(n).ok()).unwrap_or(1).saturating_sub(1);
                    match self.preview.as_mut() {
                        Some(preview) => preview.page = page,
                        None => self.preview = Some(preview::Preview::at(page)),
                    }
                }
            }
            "ui.saveAttachmentDialog" => {
                self.requests.push(UiRequest::SaveAttachment(number("index").and_then(|n| usize::try_from(n).ok()).unwrap_or(0)))
            }
            "ui.saveAllAttachmentsDialog" => self.requests.push(UiRequest::SaveAllAttachments),
            "ui.attachDialog" => self.requests.push(UiRequest::Attach),
            "ui.linkDialog" => {
                let item = self.session.open.as_ref().ok_or("No message is open")?;
                let found = openmailclient_engine::links::of(item);
                let link = match (number("index"), text("address")) {
                    (Some(index), _) => usize::try_from(index).ok().and_then(|index| found.get(index)),
                    (None, Some(address)) => found.iter().find(|link| link.address == address),
                    (None, None) => return Err("\"index\" or \"address\" is missing".to_string()),
                }
                .ok_or("The message has no such link")?;
                let target = openmailclient_formats::links::target(&link.address);
                self.menu = None;
                self.link_ask = Some(LinkAsk {
                    address: link.address.clone(),
                    text: link.text.clone(),
                    goes: link.goes.clone(),
                    concern: link.concern.as_ref().map(|c| c.words()),
                    mailbox: matches!(target, Ok(openmailclient_formats::links::Target::Mailbox(_))),
                    refused: target.err().map(str::to_string),
                });
            }
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
            "ui.layout" => {
                let size = |key: &str| params.get(key).and_then(Value::as_f64);
                if params.get("reset").and_then(Value::as_bool) == Some(true) {
                    let first = Tokens::default();
                    (self.folder_w, self.list_w, self.list_share) = (first.folder_w, first.list_w, FIRST_LIST_SHARE);
                    self.reset_window = true;
                }
                self.set_layout(size("folders"), size("list"), size("listShare"));
                return Ok(json!({ "folders": self.folder_w, "list": self.list_w, "listShare": self.list_share }));
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
                    format!("OpenMailClient {}: an independent open-source mail client. Not affiliated with Microsoft.", env!("CARGO_PKG_VERSION"));
            }
            "ui.quit" => {
                if self.may_quit() {
                    self.requests.push(UiRequest::Quit);
                }
            }
            _ => return Err(format!("unknown command {}", one_line(id))),
        }
        Ok(Value::Null)
    }

    /// The open message's HTML or RTF body as a formatted document, read once per message. `None`
    /// when the message has neither, the body holds nothing to show, or plain reading is on.
    /// The HTML body wins when a message has both.
    pub(crate) fn body_doc(&mut self) -> Option<Arc<Doc>> {
        if self.plain_bodies {
            return None;
        }
        let item = self.session.open.as_ref()?;
        let (html, len) = match (item.props.get(pid::HTML), item.props.get(pid::RTF_COMPRESSED)) {
            (Some(PropValue::Binary(b)), _) => (true, b.len()),
            (Some(PropValue::String(s)), _) => (true, s.len()),
            (_, Some(PropValue::Binary(b))) => (false, b.len()),
            _ => return None,
        };
        let mut hasher = DefaultHasher::new();
        (self.session.selected, html, len, item.subject()).hash(&mut hasher);
        let key = hasher.finish();
        let doc = match &self.body_doc {
            Some((k, doc)) if *k == key => doc.clone(),
            _ => {
                let doc = if html {
                    openmailclient_formats::html::parse(&item.body_html()?)
                } else {
                    let mut doc = openmailclient_formats::rtf::parse_with(&item.body_rtf()?, Some(openmailclient_engine::pictures::lettering()));
                    openmailclient_engine::pictures::settle(&mut doc, item);
                    doc
                };
                let doc = Arc::new(doc);
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
            self.faces = faces::Faces::new(install_fonts(&ctx));
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
        self.note_window(&ctx);
        self.shortcuts(ui);

        let title = Rect::from_min_size(full.min, vec2(full.width(), t.title_h));
        let status = Rect::from_min_max(pos2(full.left(), full.bottom() - t.status_h), full.max);
        self.title_bar(ui, title);
        if self.tab == Tab::File {
            let tabs = Rect::from_min_size(title.left_bottom(), vec2(full.width(), t.tabs_h));
            self.tab_strip(ui, tabs);
            self.backstage(ui, Rect::from_min_max(tabs.left_bottom(), full.max));
        } else {
            // The module strip runs the full height on the left; tabs, ribbon and panes sit beside it.
            // Both ribbon layouts share this shell; only the panel's height and contents differ.
            let simple = self.simplified();
            let nav = Rect::from_min_max(title.left_bottom(), pos2(full.left() + t.nav_w, status.top()));
            self.nav_strip(ui, nav);
            let tabs = Rect::from_min_max(nav.right_top(), pos2(full.right(), nav.top() + t.tabs_h));
            self.tab_strip(ui, tabs);
            let top = tabs.bottom() + 2.0;
            let height = if simple { t.simple_ribbon_h } else { t.ribbon_h };
            let ribbon = Rect::from_min_max(pos2(tabs.left(), top), pos2(full.right() - 16.0, top + height));
            if simple {
                self.ribbon_simple(ui, ribbon);
            } else {
                self.ribbon(ui, ribbon);
            }
            let body = Rect::from_min_max(pos2(tabs.left(), ribbon.bottom() + 7.0), status.right_top());
            self.body(ui, body);
            self.status_bar(ui, status);
        }
        self.menu_popup(&ctx);
        self.prompt_window(&ctx);
        self.link_window(&ctx);
        self.preview_window(&ctx);

        for (id, params) in std::mem::take(&mut self.actions) {
            if let Err(e) = self.execute(&id, &params) {
                self.session.status = e;
            }
        }
    }

    fn shortcuts(&mut self, ui: &mut Ui) {
        if self.prompt.is_some() || self.link_ask.is_some() || self.preview.is_some() {
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
        if hit(ctrl, Key::S) && self.session.is_enabled("file.save") {
            run.push(("file.save", Value::Null));
        }
        if hit(ctrl, Key::P) && self.session.is_enabled("file.print") {
            run.push(("ui.printPreview", Value::Null));
        }
        if !typing && hit(ctrl, Key::Z) {
            run.push(("edit.undo", Value::Null));
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
        let caption = if store.is_empty() { format!("{place} - OpenMailClient") } else { format!("{place} - {store} - OpenMailClient") };

        // Lined up with the message list's left edge.
        let left = rect.left() + t.nav_w + if self.session.folder_pane { self.folder_w } else { 0.0 };
        let left = left.min(rect.right() - 170.0).max(rect.left() + 44.0);
        let search = Rect::from_min_size(pos2(left, rect.center().y - 15.0), vec2((rect.right() - left - 150.0).clamp(120.0, 304.0), 30.0));
        clip_text(&p, pos2(rect.left() + 40.0, rect.center().y - 8.0), search.left() - rect.left() - 52.0, &caption, font(12.5), t.title_text);
        p.rect_filled(search, 4.0, t.title_field);
        icons::paint(&p, Rect::from_center_size(pos2(search.left() + 14.0, search.center().y), vec2(13.0, 13.0)), Icon::Search, t.title_text);
        let edit_rect =
            Rect::from_min_max(pos2(search.left() + 26.0, search.center().y - 10.0), pos2(search.right() - 6.0, search.center().y + 10.0));
        let before = self.search.clone();
        let edit = line_edit::LineEdit::new(&mut self.search, egui::Id::new("openmailclient-search"))
            .bare()
            .hint("Search", Some(t.title_text.gamma_multiply(0.75)))
            .text_color(t.title_text)
            .font(font(12.5))
            .desired_width(edit_rect.width());
        ui.put(edit_rect, edit).on_hover_text(
            "Words are all looked for; \"quoted words\" together; -word leaves messages out; OR takes either.\n\
             from:  to:  subject:  category:  has:attachment  is:unread  is:flagged  is:important\n\
             before:2026-10-05  after:2026-09  on:2026  size:>2mb",
        );
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
