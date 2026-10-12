//! The print window: the pages as they will be printed, beside how they are set up.
//!
//! The pages are drawn from what `file.print` writes, so what is seen here is what the file
//! holds. Every control runs `file.pageSetup`; printing itself is asked of the host.

use egui::{Align2, Color32, ColorImage, Key, Rect, Sense, TextureHandle, TextureOptions, pos2, vec2};
use openmailclient_engine::print::{self, Laid, Links};
use openmailclient_layout::{MM, Paper, Setup};
use serde_json::{Value, json};

use crate::{MailUi, UiRequest, bold, font};

/// The room the page is shown in, in points of the screen.
const ROOM: egui::Vec2 = vec2(470.0, 610.0);
/// The widest a page is drawn, in pixels.
const MAX_PIXELS: f32 = 2400.0;

/// The print window while it is open.
#[derive(Default)]
pub(crate) struct Preview {
    /// The page looked at; the first is 0.
    pub page: usize,
    /// What the pages were laid out for: the message and the setup.
    of: Option<(Option<u64>, Setup)>,
    /// The pages, or why there are none.
    pub laid: Option<Result<Laid, String>>,
    /// The drawn page, with which page it is and how many pixels wide.
    shown: Option<(TextureHandle, usize, usize)>,
}

impl Preview {
    /// The window opened at `page`.
    pub(crate) fn at(page: usize) -> Self {
        Self { page, ..Self::default() }
    }

    pub(crate) fn pages(&self) -> usize {
        self.laid.as_ref().and_then(|laid| laid.as_ref().ok()).map_or(0, |laid| laid.printed.pages.len())
    }
}

fn paper_label(name: &str) -> &'static str {
    match name {
        "a4" => "A4",
        "letter" => "Letter",
        "legal" => "Legal",
        "a3" => "A3",
        "a5" => "A5",
        "b5" => "B5",
        "executive" => "Executive",
        "tabloid" => "Tabloid",
        _ => "Custom",
    }
}

impl MailUi {
    pub(crate) fn preview_window(&mut self, ctx: &egui::Context) {
        let Some(mut pv) = self.preview.take() else { return };
        // The window is about the open message: with none there is nothing to show.
        if self.session.open.is_none() {
            return;
        }
        let t = self.tokens.clone();
        let of = (self.session.selected, self.session.page_setup.clone());
        if pv.of.as_ref() != Some(&of) {
            pv.of = Some(of);
            pv.laid = None;
            pv.shown = None;
        }
        if pv.laid.is_none() {
            let s = &self.session;
            pv.laid = s.open.as_ref().map(|item| print::laid(item, &print::job(s, &s.page_setup, Links::default())).map_err(|e| e.to_string()));
        }
        let pages = pv.pages();
        pv.page = pv.page.min(pages.saturating_sub(1));

        let setup = self.session.page_setup.clone();
        // What the person asked for this frame: changes to the setup, then print or close.
        let mut changes: Vec<Value> = Vec::new();
        let (mut print_now, mut close) = (false, false);
        egui::Window::new("Print").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            let dim = |text: &str| egui::Label::new(egui::RichText::new(text).font(font(12.0)).color(t.text_dim));
            let head = |text: &str| egui::Label::new(egui::RichText::new(text).font(bold(12.5)).color(t.text));
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(236.0);
                    ui.add(head("Memo Style"));
                    ui.add_space(6.0);
                    ui.add(dim("Paper"));
                    let now = setup.paper.name().unwrap_or("");
                    egui::ComboBox::from_id_salt("print-paper").selected_text(paper_label(now)).width(150.0).show_ui(ui, |ui| {
                        for name in Paper::NAMES {
                            if ui.selectable_label(name == now, paper_label(name)).clicked() && name != now {
                                changes.push(json!({ "paper": name }));
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        for (label, landscape) in [("Portrait", false), ("Landscape", true)] {
                            if ui.selectable_label(setup.landscape == landscape, label).clicked() && setup.landscape != landscape {
                                changes.push(json!({ "orientation": label.to_ascii_lowercase() }));
                            }
                        }
                    });
                    ui.add_space(8.0);
                    ui.add(dim("Margins (mm)"));
                    egui::Grid::new("print-margins").num_columns(4).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
                        for (row, edges) in [[("Top", 0usize), ("Bottom", 2)], [("Left", 3), ("Right", 1)]].into_iter().enumerate() {
                            for (label, at) in edges {
                                let was = (f64::from(setup.margins.get(at).copied().unwrap_or(0.0) / MM) * 10.0).round() / 10.0;
                                let mut mm = was;
                                ui.add(dim(label));
                                let field = egui::DragValue::new(&mut mm).range(0.0..=150.0).speed(0.5).max_decimals(1);
                                if ui.add(field).changed() && (mm - was).abs() > 0.04 {
                                    changes.push(json!({ "margins": { label.to_ascii_lowercase(): mm } }));
                                }
                            }
                            if row == 0 {
                                ui.end_row();
                            }
                        }
                    });
                    ui.add_space(8.0);
                    for (label, key, parts) in [("Header", "header", &setup.header), ("Footer", "footer", &setup.footer)] {
                        ui.add(dim(label));
                        for (part, hint) in parts.iter().zip(["left", "center", "right"]) {
                            let mut text = part.clone();
                            let field = egui::TextEdit::singleline(&mut text).hint_text(hint).desired_width(226.0).font(font(12.5));
                            if ui.add(field).changed() {
                                changes.push(json!({ key: { hint: text } }));
                            }
                        }
                        ui.add_space(4.0);
                    }
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new("[Page]  [Pages]  [Page of Pages]  [Date]  [Time]  [User]  [Subject]")
                                .font(font(11.0))
                                .color(t.text_faint),
                        )
                        .wrap(),
                    );
                    ui.add_space(6.0);
                    if ui.add_enabled(setup != Setup::default(), egui::Button::new("Defaults")).clicked() {
                        changes.push(json!({ "reset": true }));
                    }
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let go = egui::Button::new(egui::RichText::new("Print to PDF…").color(t.on_accent)).fill(t.accent);
                        if ui.add_enabled(pages > 0, go).clicked() {
                            print_now = true;
                        }
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    let (room, _) = ui.allocate_exact_size(ROOM, Sense::hover());
                    ui.painter().rect_filled(room, 2.0, t.pane);
                    match pv.laid.as_ref() {
                        Some(Ok(laid)) => {
                            if let Some(page) = laid.printed.pages.get(pv.page) {
                                // The page as large as the room lets it be, a little in from its edges.
                                let fit = ((room.width() - 24.0) / page.width.max(1.0)).min((room.height() - 24.0) / page.height.max(1.0));
                                let size = vec2(page.width * fit, page.height * fit);
                                let sheet = Rect::from_center_size(room.center(), size);
                                let pixels = (size.x * ui.ctx().pixels_per_point()).clamp(16.0, MAX_PIXELS).round() as usize;
                                if !pv.shown.as_ref().is_some_and(|(_, page, wide)| *page == pv.page && *wide == pixels) {
                                    pv.shown = print::raster(laid, pv.page, pixels as f32 / page.width.max(1.0)).map(|image| {
                                        let picture = ColorImage::from_rgb([image.width, image.height], &image.rgb);
                                        (ui.ctx().load_texture("print-preview", picture, TextureOptions::LINEAR), pv.page, pixels)
                                    });
                                }
                                ui.painter().rect_filled(sheet.translate(vec2(2.0, 2.0)), 0.0, Color32::from_black_alpha(40));
                                ui.painter().rect_filled(sheet, 0.0, Color32::WHITE);
                                if let Some((texture, ..)) = &pv.shown {
                                    ui.painter().image(texture.id(), sheet, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                                }
                            }
                        }
                        Some(Err(why)) => {
                            let words = egui::Label::new(egui::RichText::new(why.as_str()).font(font(13.0)).color(t.text)).wrap();
                            ui.put(room.shrink(40.0), words);
                        }
                        None => {}
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(pv.page > 0, egui::Button::new("Previous")).clicked() {
                            pv.page -= 1;
                        }
                        if ui.add_enabled(pv.page + 1 < pages, egui::Button::new("Next")).clicked() {
                            pv.page += 1;
                        }
                        if pages > 0 {
                            ui.add(dim(&format!("Page {} of {pages}", pv.page + 1)));
                        }
                        if pv.laid.as_ref().is_some_and(|laid| laid.as_ref().is_ok_and(|laid| laid.printed.truncated)) {
                            ui.add(egui::Label::new(
                                egui::RichText::new("The message is longer than may be printed.").font(font(12.0)).color(t.danger),
                            ));
                        }
                    });
                });
            });
            // Keys turn the pages when nothing is being typed in.
            if ui.memory(|m| m.focused().is_none()) {
                let hit = |key: Key| ui.input(|i| i.key_pressed(key));
                if (hit(Key::PageDown) || hit(Key::ArrowRight)) && pv.page + 1 < pages {
                    pv.page += 1;
                }
                if (hit(Key::PageUp) || hit(Key::ArrowLeft)) && pv.page > 0 {
                    pv.page -= 1;
                }
                if hit(Key::Home) {
                    pv.page = 0;
                }
                if hit(Key::End) {
                    pv.page = pages.saturating_sub(1);
                }
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                close = true;
            }
        });
        for change in changes {
            if let Err(e) = self.execute("file.pageSetup", &change) {
                self.session.status = e;
            }
        }
        if print_now {
            self.requests.push(UiRequest::Print);
        }
        if !close && !print_now {
            self.preview = Some(pv);
        }
    }
}
