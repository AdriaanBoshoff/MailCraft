//! Show Progress: a window that lists what runs on a thread of its own (sending, receiving,
//! testing an account, saving, exporting), each with how far it is and Cancel Task, Cancel All,
//! when Send/Receive comes next on its own, and what came of those that ended. It stands beside
//! the mailbox, which stays in use while it is open.

use crate::icons::{self, Icon};
use crate::{MailUi, bold, font, one_line};
use egui::{Align2, Color32, Rect, Sense, Ui, vec2};
use serde_json::{Value, json};

/// Ended work listed at most, the newest first.
const MAX_ENDED: usize = 8;

impl MailUi {
    pub(crate) fn progress_window(&mut self, ctx: &egui::Context) {
        if !self.progress_open {
            return;
        }
        let t = self.tokens.clone();
        let running = self.session.running();
        let next = self.session.send_receive_next_in();
        let every = self.session.send_receive_min;
        let no_account = self.session.accounts.is_empty();
        let watching = self.session.watching();
        let ended: Vec<Value> = self.session.finished.iter().rev().take(MAX_ENDED).cloned().collect();
        let mut cancel: Option<Option<String>> = None;
        let mut close = false;
        let shown = crate::dialog::Dialog::new("progress", "Send/Receive Progress", Icon::Sync)
            .width(460.0)
            .anchor(Align2::RIGHT_BOTTOM, vec2(-24.0, -(t.status_h + 24.0)))
            .show(ctx, &t, |ui| {
                let when = match next {
                    _ if every == 0 => "Mail is sent and received only when asked (F9).".to_string(),
                    Some(ms) if ms < 60_000 => format!("Send/Receive comes on its own in {} seconds.", ms.div_ceil(1000)),
                    Some(ms) => format!("Send/Receive comes on its own in {} minutes.", ms.div_ceil(60_000)),
                    None if no_account => format!("Send/Receive comes on its own every {every} minutes once an account is added."),
                    None => format!("Send/Receive comes on its own every {every} minutes."),
                };
                ui.label(egui::RichText::new(when).font(font(12.5)).color(t.text_dim));
                // Each account whose Inbox is watched, and how that stands: mail that comes
                // there is received at once.
                for (account, said) in &watching {
                    let line = one_line(&format!("{account}: {said}"));
                    ui.add(egui::Label::new(crate::seen_line(&line, font(12.0), t.text_dim)).truncate());
                }
                ui.add_space(8.0);
                if running.is_empty() {
                    ui.label(egui::RichText::new("Nothing is running.").font(font(13.0)).color(t.text));
                }
                for r in &running {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(330.0);
                            ui.label(egui::RichText::new(one_line(&r.what)).font(bold(13.0)).color(t.text));
                            bar(ui, r.done, r.stopping, &t);
                        });
                        if ui.add_enabled(!r.stopping, crate::dialog::plain("Cancel Task")).clicked() {
                            cancel = Some(Some(r.id.clone()));
                        }
                    });
                    ui.add_space(6.0);
                }
                ui.add_space(4.0);
                if !ended.is_empty() {
                    crate::dialog::heading(ui, &t, "Ended");
                    for e in &ended {
                        let ok = e.get("ok").and_then(Value::as_bool);
                        let stopped = e.get("stopped").and_then(Value::as_bool) == Some(true);
                        let (mark, color) = match ok {
                            _ if stopped => (None, t.text_dim),
                            Some(false) => (Some(Icon::Close), t.danger),
                            _ => (Some(Icon::Check), t.ok),
                        };
                        let job = e.get("job").and_then(Value::as_str).unwrap_or("");
                        let said = if stopped {
                            // What it said when it stopped may be about what it got to, not that it stopped.
                            format!("{job}: stopped; what was done is kept")
                        } else {
                            e.get("said").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or(job).to_string()
                        };
                        ui.horizontal(|ui| {
                            // Done, failed or stopped, drawn our own way.
                            let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                            match mark {
                                Some(icon) => icons::paint(ui.painter(), rect, icon, color),
                                None => {
                                    ui.painter().rect_filled(rect.shrink(3.0), 1.0, color);
                                }
                            }
                            ui.add(egui::Label::new(crate::seen_line(&one_line(&said), font(12.0), t.text)).truncate());
                        });
                    }
                }
                crate::dialog::foot(ui, &t, |ui| {
                    if ui.add_enabled(running.iter().any(|r| !r.stopping), crate::dialog::plain("Cancel All")).clicked() {
                        cancel = Some(None);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(crate::dialog::primary(&t, "Close")).clicked() {
                            close = true;
                        }
                    });
                });
            });
        if shown.closed || close {
            self.progress_open = false;
        }
        if let Some(id) = cancel {
            let params = id.map_or(Value::Null, |id| json!({ "id": id }));
            if let Err(e) = self.session.run("mail.cancelTask", &params) {
                self.session.status = e.to_string();
            }
        }
        if !running.is_empty() || next.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(if running.is_empty() { 1000 } else { 100 }));
        }
    }
}

/// How far a piece of work is: a bar that fills, or one that moves while it is not known.
pub(crate) fn bar(ui: &mut Ui, done: f32, stopping: bool, t: &crate::theme::Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(330.0, 16.0), Sense::hover());
    let p = ui.painter_at(rect);
    let track = Rect::from_min_size(rect.min + vec2(0.0, 5.0), vec2(250.0, 6.0));
    p.rect_filled(track, 3.0, t.line);
    let fill: Color32 = if stopping { t.text_dim } else { t.accent };
    if done > 0.0 {
        p.rect_filled(Rect::from_min_size(track.min, vec2(track.width() * done.clamp(0.0, 1.0), track.height())), 3.0, fill);
    } else {
        // Not known yet how much there is: a short piece that goes along.
        let at = (ui.input(|i| i.time) as f32 * 0.8).fract();
        let x = track.left() + (track.width() - 50.0) * at;
        p.rect_filled(Rect::from_min_size(egui::pos2(x, track.top()), vec2(50.0, track.height())), 3.0, fill);
    }
    let said = if stopping {
        "Stopping…".to_string()
    } else if done > 0.0 {
        format!("{:.0}%", done * 100.0)
    } else {
        String::new()
    };
    p.text(egui::pos2(track.right() + 10.0, rect.center().y), Align2::LEFT_CENTER, said, font(12.0), t.text_dim);
}
