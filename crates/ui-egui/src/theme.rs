//! Colour and size tokens. Every colour the UI paints comes from here; the values are our own
//! choices for a blue-accented mail window, in a light and a dark set. A theme changes any of
//! the colours, for the light set and the dark set each; the built-in ones are our own too.

use egui::Color32;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Tokens {
    pub accent: Color32,
    pub accent_dark: Color32,
    pub accent_soft: Color32,
    pub on_accent: Color32,
    pub window: Color32,
    pub chrome: Color32,
    pub pane: Color32,
    pub surface: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub line: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub disabled: Color32,
    pub flag: Color32,
    pub danger: Color32,
    pub ok: Color32,
    /// Time marked out of the office in the calendar.
    pub away: Color32,
    pub info_bar: Color32,
    /// Title bar: background, text and the search field's fill.
    pub title_bg: Color32,
    pub title_text: Color32,
    pub title_field: Color32,
    /// Status bar background and text.
    pub bar_bg: Color32,
    pub bar_text: Color32,
    /// The head of a dialog and the words on it.
    pub dialog_head: Color32,
    pub dialog_text: Color32,
    pub dark: bool,

    pub title_h: f32,
    pub tabs_h: f32,
    /// Ribbon panel height: classic (labelled groups) and simplified (one line).
    pub ribbon_h: f32,
    pub simple_ribbon_h: f32,
    pub status_h: f32,
    /// Module strip width.
    pub nav_w: f32,
    pub folder_w: f32,
    pub list_w: f32,
}

impl Default for Tokens {
    fn default() -> Self {
        Self::light()
    }
}

impl Tokens {
    /// egui's own widgets (buttons, text fields, check boxes) in this set. Buttons have room
    /// round their words, a fill, a line round them and rounded corners, so they read as buttons
    /// and are easy to hit; the line turns the accent under the pointer and the fill when pressed.
    pub fn apply(&self, ctx: &egui::Context) {
        ctx.set_theme(if self.dark { egui::Theme::Dark } else { egui::Theme::Light });
        let theme = if self.dark { egui::Theme::Dark } else { egui::Theme::Light };
        ctx.style_mut_of(theme, |s| {
            s.spacing.button_padding = egui::vec2(14.0, 6.0);
            s.spacing.interact_size.y = 28.0;
            s.spacing.item_spacing.x = s.spacing.item_spacing.x.max(8.0);
            if let Some(f) = s.text_styles.get_mut(&egui::TextStyle::Button) {
                f.size = f.size.max(13.0);
            }
            // Windows, fields and what is selected in them in the same colours as the rest.
            let v = &mut s.visuals;
            v.window_fill = self.surface;
            v.panel_fill = self.window;
            v.window_stroke = egui::Stroke::new(1.0, self.line);
            v.extreme_bg_color = if self.dark { self.pane } else { self.surface };
            v.text_edit_bg_color = Some(v.extreme_bg_color);
            v.faint_bg_color = self.pane;
            v.code_bg_color = self.pane;
            v.hyperlink_color = self.accent;
            v.selection.bg_fill = self.selected;
            v.selection.stroke = egui::Stroke::new(1.0, self.accent);
            v.widgets.noninteractive.bg_fill = self.surface;
            v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, self.line);
            v.widgets.noninteractive.fg_stroke.color = self.text;
            let radius = egui::CornerRadius::same(5);
            let w = &mut s.visuals.widgets;
            w.inactive.weak_bg_fill = self.surface;
            w.inactive.bg_stroke = egui::Stroke::new(1.0, if self.dark { self.hover } else { self.line });
            w.inactive.fg_stroke.color = self.text;
            w.hovered.weak_bg_fill = self.hover;
            w.hovered.bg_stroke = egui::Stroke::new(1.0, self.accent);
            w.hovered.fg_stroke.color = self.text;
            w.active.weak_bg_fill = self.accent_soft;
            w.active.bg_stroke = egui::Stroke::new(1.0, self.accent);
            w.active.fg_stroke.color = self.text;
            for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
                state.corner_radius = radius;
                // No growing under the pointer, so a row of buttons does not shift.
                state.expansion = 0.0;
            }
        });
    }

    pub fn named(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    pub fn dark() -> Self {
        Self {
            accent: Color32::from_rgb(0x7D, 0xB8, 0xF0),
            accent_dark: Color32::from_rgb(0x5E, 0xA2, 0xE0),
            accent_soft: Color32::from_rgb(0x4E, 0x50, 0x54),
            on_accent: Color32::from_rgb(0x10, 0x20, 0x33),
            window: Color32::from_rgb(0x2E, 0x2F, 0x31),
            chrome: Color32::from_rgb(0x2E, 0x2F, 0x31),
            pane: Color32::from_rgb(0x37, 0x38, 0x3B),
            surface: Color32::from_rgb(0x42, 0x44, 0x47),
            hover: Color32::from_rgb(0x52, 0x55, 0x5A),
            selected: Color32::from_rgb(0x2C, 0x55, 0x80),
            line: Color32::from_rgb(0x26, 0x27, 0x29),
            text: Color32::from_rgb(0xF2, 0xF2, 0xF2),
            text_dim: Color32::from_rgb(0xC6, 0xC8, 0xCB),
            text_faint: Color32::from_rgb(0x9C, 0x9F, 0xA3),
            disabled: Color32::from_rgb(0x74, 0x77, 0x7B),
            flag: Color32::from_rgb(0xF0, 0x6A, 0x6A),
            danger: Color32::from_rgb(0xF0, 0x7C, 0x70),
            ok: Color32::from_rgb(0x6C, 0xC0, 0x70),
            away: Color32::from_rgb(0xB8, 0x8C, 0xD0),
            info_bar: Color32::from_rgb(0x5A, 0x4E, 0x1E),
            title_bg: Color32::from_rgb(0x22, 0x23, 0x25),
            title_text: Color32::from_rgb(0xF2, 0xF2, 0xF2),
            title_field: Color32::from_rgb(0x4A, 0x4C, 0x50),
            bar_bg: Color32::from_rgb(0x22, 0x23, 0x25),
            bar_text: Color32::from_rgb(0xDA, 0xDA, 0xDA),
            dialog_head: Color32::from_rgb(0x27, 0x46, 0x68),
            dialog_text: Color32::from_rgb(0xF2, 0xF2, 0xF2),
            dark: true,
            ..Self::light()
        }
    }

    pub fn light() -> Self {
        Self {
            accent: Color32::from_rgb(0x12, 0x62, 0xB3),
            accent_dark: Color32::from_rgb(0x0C, 0x4A, 0x8A),
            accent_soft: Color32::from_rgb(0x3D, 0x86, 0xCF),
            on_accent: Color32::WHITE,
            window: Color32::from_rgb(0xF4, 0xF4, 0xF4),
            chrome: Color32::from_rgb(0xF4, 0xF4, 0xF4),
            pane: Color32::from_rgb(0xEE, 0xEF, 0xF1),
            surface: Color32::WHITE,
            hover: Color32::from_rgb(0xE3, 0xEC, 0xF6),
            selected: Color32::from_rgb(0xCC, 0xE1, 0xF7),
            line: Color32::from_rgb(0xD6, 0xD8, 0xDB),
            text: Color32::from_rgb(0x21, 0x21, 0x21),
            text_dim: Color32::from_rgb(0x5C, 0x5F, 0x63),
            text_faint: Color32::from_rgb(0x8A, 0x8D, 0x91),
            disabled: Color32::from_rgb(0xAE, 0xB1, 0xB5),
            flag: Color32::from_rgb(0xC8, 0x2A, 0x2A),
            danger: Color32::from_rgb(0xB3, 0x26, 0x1E),
            ok: Color32::from_rgb(0x2E, 0x7D, 0x32),
            away: Color32::from_rgb(0x80, 0x39, 0x7B),
            info_bar: Color32::from_rgb(0xFF, 0xF4, 0xCE),
            title_bg: Color32::from_rgb(0x12, 0x62, 0xB3),
            title_text: Color32::WHITE,
            title_field: Color32::from_rgb(0x3D, 0x86, 0xCF),
            bar_bg: Color32::from_rgb(0x12, 0x62, 0xB3),
            bar_text: Color32::WHITE,
            dialog_head: Color32::from_rgb(0x12, 0x62, 0xB3),
            dialog_text: Color32::WHITE,
            dark: false,

            title_h: 48.0,
            tabs_h: 28.0,
            ribbon_h: 98.0,
            simple_ribbon_h: 48.0,
            status_h: 22.0,
            nav_w: 64.0,
            folder_w: 216.0,
            list_w: 348.0,
        }
    }
}

/// Colour for a category name: the colour word in the name picks it, anything else is grey.
pub fn category_color(name: &str) -> Color32 {
    let n = name.to_ascii_lowercase();
    let table: [(&str, (u8, u8, u8)); 6] = [
        ("red", (0xD6, 0x45, 0x41)),
        ("orange", (0xE8, 0x8B, 0x2E)),
        ("yellow", (0xE3, 0xC2, 0x2B)),
        ("green", (0x4C, 0xA6, 0x4C)),
        ("blue", (0x3F, 0x83, 0xD1)),
        ("purple", (0x8E, 0x5B, 0xB8)),
    ];
    table.iter().find(|(word, _)| n.contains(word)).map_or(Color32::from_rgb(0x8A, 0x8D, 0x91), |(_, (r, g, b))| Color32::from_rgb(*r, *g, *b))
}

/// A steady colour for a person's initials circle.
pub fn avatar_color(name: &str) -> Color32 {
    const COLORS: [(u8, u8, u8); 8] = [
        (0x3F, 0x83, 0xD1),
        (0x4C, 0xA6, 0x4C),
        (0xB8, 0x5B, 0x8E),
        (0xC7, 0x7B, 0x30),
        (0x5B, 0x6A, 0xB8),
        (0x2F, 0x9C, 0x95),
        (0xA6, 0x4C, 0x4C),
        (0x7A, 0x7F, 0x86),
    ];
    let hash = name.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    let (r, g, b) = COLORS.get(hash as usize % COLORS.len()).copied().unwrap_or((0x7A, 0x7F, 0x86));
    Color32::from_rgb(r, g, b)
}

/// The colours a theme may change: the name each is kept by, what it is called, and the group it
/// is shown in.
pub const COLORS: [(&str, &str, &str); 25] = [
    ("accent", "Accent", "Accent"),
    ("accentDark", "Accent, pressed", "Accent"),
    ("accentSoft", "Accent, soft", "Accent"),
    ("onAccent", "Text on the accent", "Accent"),
    ("selected", "Selected", "Accent"),
    ("hover", "Under the pointer", "Accent"),
    ("window", "Window", "Window"),
    ("pane", "Panes", "Window"),
    ("surface", "Lists, pages and dialogs", "Window"),
    ("line", "Lines", "Window"),
    ("text", "Text", "Text"),
    ("textDim", "Quieter text", "Text"),
    ("textFaint", "Faint text", "Text"),
    ("titleBar", "Title bar", "Bars"),
    ("titleText", "Title bar text", "Bars"),
    ("titleField", "Search field", "Bars"),
    ("statusBar", "Status bar", "Bars"),
    ("statusText", "Status bar text", "Bars"),
    ("dialogHead", "Dialog heads", "Bars"),
    ("dialogText", "Dialog head text", "Bars"),
    ("flag", "Flags", "Signals"),
    ("danger", "Warnings", "Signals"),
    ("ok", "Done", "Signals"),
    ("away", "Out of office", "Signals"),
    ("infoBar", "Info bar", "Signals"),
];

/// Pairs that must stay readable, the least contrast each needs (WCAG 2: 4.5 for text, 3 for
/// large text and marks), and what to call the pair when it is too low.
pub const READABLE: [(&str, &str, f32, &str); 8] = [
    ("text", "surface", 4.5, "Text on lists and dialogs"),
    ("text", "pane", 4.5, "Text on panes"),
    ("textDim", "surface", 3.0, "Quieter text"),
    ("onAccent", "accent", 3.0, "Text on the accent"),
    ("titleText", "titleBar", 4.5, "Title bar text"),
    ("statusText", "statusBar", 4.5, "Status bar text"),
    ("dialogText", "dialogHead", 4.5, "Dialog head text"),
    ("text", "selected", 4.5, "Text on what is selected"),
];

impl Tokens {
    /// A colour by the name a theme keeps it by.
    pub fn color(&self, key: &str) -> Option<Color32> {
        let mut t = self.clone();
        t.color_mut(key).map(|c| *c)
    }

    pub fn color_mut(&mut self, key: &str) -> Option<&mut Color32> {
        Some(match key {
            "accent" => &mut self.accent,
            "accentDark" => &mut self.accent_dark,
            "accentSoft" => &mut self.accent_soft,
            "onAccent" => &mut self.on_accent,
            "selected" => &mut self.selected,
            "hover" => &mut self.hover,
            "window" => &mut self.window,
            "pane" => &mut self.pane,
            "surface" => &mut self.surface,
            "line" => &mut self.line,
            "text" => &mut self.text,
            "textDim" => &mut self.text_dim,
            "textFaint" => &mut self.text_faint,
            "titleBar" => &mut self.title_bg,
            "titleText" => &mut self.title_text,
            "titleField" => &mut self.title_field,
            "statusBar" => &mut self.bar_bg,
            "statusText" => &mut self.bar_text,
            "dialogHead" => &mut self.dialog_head,
            "dialogText" => &mut self.dialog_text,
            "flag" => &mut self.flag,
            "danger" => &mut self.danger,
            "ok" => &mut self.ok,
            "away" => &mut self.away,
            "infoBar" => &mut self.info_bar,
            _ => return None,
        })
    }

    /// The pairs of `READABLE` that are too low in contrast here, with the contrast they have.
    pub fn unreadable(&self) -> Vec<(&'static str, f32)> {
        READABLE
            .iter()
            .filter_map(|(fore, back, least, what)| {
                let ratio = contrast(self.color(fore)?, self.color(back)?);
                (ratio < *least).then_some((*what, ratio))
            })
            .collect()
    }
}

/// Most themes a person keeps, and the longest name one may have.
pub const MAX_THEMES: usize = 40;
pub const MAX_NAME: usize = 60;

/// A set of colours over the built-in light and dark ones: what it changes in each.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Theme {
    pub name: String,
    pub light: BTreeMap<String, Color32>,
    pub dark: BTreeMap<String, Color32>,
    /// One of ours, which is not changed: a change makes a copy.
    pub built_in: bool,
}

impl Theme {
    pub fn changes(&self, dark: bool) -> &BTreeMap<String, Color32> {
        if dark { &self.dark } else { &self.light }
    }

    pub fn changes_mut(&mut self, dark: bool) -> &mut BTreeMap<String, Color32> {
        if dark { &mut self.dark } else { &mut self.light }
    }

    /// The colours to draw with in the light or the dark set.
    pub fn tokens(&self, dark: bool) -> Tokens {
        let mut t = Tokens::named(dark);
        for (key, color) in self.changes(dark) {
            if let Some(slot) = t.color_mut(key) {
                *slot = *color;
            }
        }
        // The window's edge is drawn in the window's colour.
        t.chrome = t.window;
        t
    }

    /// Change the accent and the colours that follow it, as the built-in themes are made.
    pub fn set_accent(&mut self, dark: bool, accent: Color32) {
        let family = accent_family(accent, dark);
        self.changes_mut(dark).extend(family);
    }

    pub fn to_json(&self) -> Value {
        let set = |m: &BTreeMap<String, Color32>| Value::Object(m.iter().map(|(k, c)| (k.clone(), json!(hex(*c)))).collect::<Map<_, _>>());
        json!({ "name": self.name, "light": set(&self.light), "dark": set(&self.dark) })
    }

    /// A theme as a settings file or a person wrote it. Colours of names we do not know, and
    /// those that are not `#RRGGBB`, are left out; a theme without a usable name is refused.
    pub fn from_json(v: &Value) -> Option<Theme> {
        let name = clean_name(v.get("name")?.as_str()?)?;
        let set = |key: &str| -> BTreeMap<String, Color32> {
            let Some(m) = v.get(key).and_then(Value::as_object) else { return BTreeMap::new() };
            m.iter()
                .take(COLORS.len() * 2)
                .filter(|(k, _)| COLORS.iter().any(|(name, ..)| name == k))
                .filter_map(|(k, c)| Some((k.clone(), parse_hex(c.as_str()?)?)))
                .collect()
        };
        Some(Theme { name, light: set("light"), dark: set("dark"), built_in: false })
    }
}

/// A name for a theme: one line, trimmed, at most `MAX_NAME` characters, not empty.
pub fn clean_name(name: &str) -> Option<String> {
    let name: String = name.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    let name = name.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// The themes that come with the program: blue as it has always been, and others of our own
/// choosing, each made from one accent for the light set and one for the dark.
pub fn built_in() -> Vec<Theme> {
    let rgb = Color32::from_rgb;
    let mut list = vec![Theme { name: "Blue".to_string(), built_in: true, ..Theme::default() }];
    for (name, light, dark) in [
        ("Teal", rgb(0x0B, 0x7A, 0x75), rgb(0x5C, 0xC8, 0xC0)),
        ("Forest", rgb(0x2F, 0x6E, 0x3A), rgb(0x86, 0xC9, 0x8E)),
        ("Plum", rgb(0x6B, 0x3F, 0x8F), rgb(0xC1, 0x9C, 0xE6)),
        ("Rose", rgb(0xA8, 0x2B, 0x5A), rgb(0xF0, 0x8F, 0xB4)),
        ("Amber", rgb(0x9A, 0x5B, 0x00), rgb(0xF2, 0xB5, 0x4C)),
        ("Graphite", rgb(0x4A, 0x55, 0x60), rgb(0xA8, 0xB4, 0xC0)),
    ] {
        let mut theme = Theme { name: name.to_string(), built_in: true, ..Theme::default() };
        theme.set_accent(false, light);
        theme.set_accent(true, dark);
        list.push(theme);
    }
    // Strong edges and plain colours for those who need more contrast than the rest give.
    let mut high = Theme { name: "High contrast".to_string(), built_in: true, ..Theme::default() };
    for (key, color) in [
        ("window", Color32::WHITE),
        ("pane", Color32::WHITE),
        ("surface", Color32::WHITE),
        ("line", Color32::BLACK),
        ("text", Color32::BLACK),
        ("textDim", rgb(0x20, 0x20, 0x20)),
        ("textFaint", rgb(0x40, 0x40, 0x40)),
        ("hover", rgb(0xFF, 0xF0, 0x9E)),
        ("selected", rgb(0xC8, 0xDC, 0xFF)),
    ] {
        high.light.insert(key.to_string(), color);
    }
    high.set_accent(false, rgb(0x00, 0x3C, 0x9E));
    for (key, color) in [
        ("window", Color32::BLACK),
        ("pane", Color32::BLACK),
        ("surface", Color32::BLACK),
        ("line", Color32::WHITE),
        ("text", Color32::WHITE),
        ("textDim", rgb(0xE6, 0xE6, 0xE6)),
        ("textFaint", rgb(0xC8, 0xC8, 0xC8)),
        ("hover", rgb(0x3A, 0x3A, 0x00)),
        ("titleBar", Color32::BLACK),
        ("statusBar", Color32::BLACK),
    ] {
        high.dark.insert(key.to_string(), color);
    }
    high.set_accent(true, rgb(0xFF, 0xD5, 0x00));
    high.dark.insert("selected".to_string(), rgb(0x00, 0x3C, 0x9E));
    high.dark.insert("titleBar".to_string(), Color32::BLACK);
    high.dark.insert("statusBar".to_string(), Color32::BLACK);
    list.push(high);
    list
}

/// The colours that follow an accent: darker and softer ones of it, the selection and the
/// pointer's mark, the bars and the dialog heads, and text that can be read on it.
fn accent_family(a: Color32, dark: bool) -> Vec<(String, Color32)> {
    let black = Color32::BLACK;
    let white = Color32::WHITE;
    let on = if luminance(a) > 0.4 { mix(a, black, 0.85) } else { white };
    let set: Vec<(&str, Color32)> = if dark {
        let ground = Color32::from_rgb(0x2E, 0x2F, 0x31);
        vec![
            ("accent", a),
            ("accentDark", mix(a, black, 0.15)),
            ("onAccent", mix(a, black, 0.85)),
            ("selected", mix(a, ground, 0.62)),
            ("dialogHead", mix(a, ground, 0.68)),
            ("dialogText", Color32::from_rgb(0xF2, 0xF2, 0xF2)),
        ]
    } else {
        vec![
            ("accent", a),
            ("accentDark", mix(a, black, 0.3)),
            ("accentSoft", mix(a, white, 0.2)),
            ("onAccent", on),
            ("hover", mix(a, white, 0.89)),
            ("selected", mix(a, white, 0.79)),
            ("titleBar", a),
            ("titleText", on),
            ("titleField", mix(a, white, 0.2)),
            ("statusBar", a),
            ("statusText", on),
            ("dialogHead", a),
            ("dialogText", on),
        ]
    };
    set.into_iter().map(|(k, c)| (k.to_string(), c)).collect()
}

/// `a` taken `share` of the way to `b`.
pub fn mix(a: Color32, b: Color32, share: f32) -> Color32 {
    let s = share.clamp(0.0, 1.0);
    let one = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * s).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(one(a.r(), b.r()), one(a.g(), b.g()), one(a.b(), b.b()))
}

/// Relative luminance, WCAG 2.
pub fn luminance(c: Color32) -> f32 {
    let one = |x: u8| {
        let v = f32::from(x) / 255.0;
        if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * one(c.r()) + 0.7152 * one(c.g()) + 0.0722 * one(c.b())
}

/// Contrast ratio of two colours, 1 to 21 (WCAG 2).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

pub fn hex(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

/// `#RRGGBB` (or without the `#`); anything else is not a colour.
pub fn parse_hex(text: &str) -> Option<Color32> {
    let t = text.trim();
    let t = t.strip_prefix('#').unwrap_or(t);
    if t.len() != 6 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let part = |at: usize| t.get(at..at + 2).and_then(|p| u8::from_str_radix(p, 16).ok());
    Some(Color32::from_rgb(part(0)?, part(2)?, part(4)?))
}
