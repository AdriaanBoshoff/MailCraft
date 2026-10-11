//! Colour and size tokens. Every colour the UI paints comes from here; the values are our own
//! choices for a light, blue-accented mail window.

use egui::Color32;

#[derive(Clone, Debug)]
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
    pub info_bar: Color32,

    pub title_h: f32,
    pub tabs_h: f32,
    pub ribbon_h: f32,
    pub status_h: f32,
    pub nav_w: f32,
    pub folder_w: f32,
    pub list_w: f32,
}

impl Default for Tokens {
    fn default() -> Self {
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
            info_bar: Color32::from_rgb(0xFF, 0xF4, 0xCE),

            title_h: 34.0,
            tabs_h: 28.0,
            ribbon_h: 94.0,
            status_h: 22.0,
            nav_w: 46.0,
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
