//! The window body: navigation strip, folder pane, message list, reading pane and the inline
//! compose form.

use crate::icons::{self, Icon};
use crate::{MailUi, body, bold, clip_text, font, one_line, text_width, theme};
use egui::{Align2, Rect, ScrollArea, Sense, Ui, UiBuilder, pos2, vec2};
use mailcraft_engine::{Module, ReadingPane, SortKey};
use mailcraft_model::{Flag, FolderId, FolderInfo, FolderKind, RECIP_CC, RECIP_TO, Summary, time};
use serde_json::{Value, json};

const ROW_H: f32 = 64.0;
const GROUP_H: f32 = 24.0;
/// Body text beyond this is not laid out (a message body can be hundreds of megabytes).
const MAX_BODY_CHARS: usize = 200_000;

enum Entry {
    Group(usize),
    Row(usize),
}

fn folder_icon(kind: FolderKind) -> Icon {
    match kind {
        FolderKind::Inbox => Icon::Mail,
        FolderKind::Sent | FolderKind::Outbox => Icon::Send,
        FolderKind::Drafts => Icon::Rename,
        FolderKind::Deleted => Icon::Delete,
        FolderKind::Junk => Icon::Junk,
        FolderKind::Archive => Icon::Archive,
        FolderKind::Calendar => Icon::Calendar,
        FolderKind::Contacts => Icon::People,
        FolderKind::Tasks => Icon::Tasks,
        FolderKind::Search => Icon::Search,
        _ => Icon::Folder,
    }
}

fn initials(name: &str) -> String {
    let mut out: String =
        name.split_whitespace().filter_map(|w| w.chars().find(|c| c.is_alphanumeric())).take(2).flat_map(char::to_uppercase).collect();
    if out.is_empty() {
        out.push('?');
    }
    out
}

fn human_size(bytes: usize) -> String {
    mailcraft_engine::attachments::size_words(bytes)
}

impl MailUi {
    pub(crate) fn body(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let rest = rect;
        if self.session.module != Module::Mail {
            let p = ui.painter_at(rest);
            p.rect_filled(rest, 0.0, t.surface);
            let (icon, name) = match self.session.module {
                Module::Calendar => (Icon::Calendar, "Calendar"),
                Module::People => (Icon::People, "People"),
                _ => (Icon::Tasks, "Tasks"),
            };
            icons::paint(&p, Rect::from_center_size(rest.center() - vec2(0.0, 40.0), vec2(56.0, 56.0)), icon, t.disabled);
            p.text(rest.center() + vec2(0.0, 12.0), Align2::CENTER_CENTER, format!("{name} is not built yet"), font(16.0), t.text_dim);
            p.text(
                rest.center() + vec2(0.0, 36.0),
                Align2::CENTER_CENTER,
                "This first milestone covers mail and data files.",
                font(12.5),
                t.text_faint,
            );
            return;
        }
        let mut left = rest.left();
        if self.session.folder_pane {
            let w = t.folder_w.min(rest.width() * 0.4);
            self.folder_pane(ui, Rect::from_min_size(pos2(left, rest.top()), vec2(w, rest.height())));
            left += w;
        }
        let area = Rect::from_min_max(pos2(left, rest.top()), rest.max);
        let composing = self.session.compose.is_some();
        let pane = if composing && self.session.reading_pane == ReadingPane::Off { ReadingPane::Right } else { self.session.reading_pane };
        let (list, reading) = match pane {
            ReadingPane::Off => (area, None),
            ReadingPane::Right => {
                let w = t.list_w.min(area.width() * 0.5);
                (Rect::from_min_size(area.min, vec2(w, area.height())), Some(Rect::from_min_max(pos2(area.left() + w, area.top()), area.max)))
            }
            ReadingPane::Bottom => {
                let h = (area.height() * 0.42).max(120.0).min(area.height());
                (Rect::from_min_size(area.min, vec2(area.width(), h)), Some(Rect::from_min_max(pos2(area.left(), area.top() + h), area.max)))
            }
        };
        self.message_list(ui, list);
        if let Some(r) = reading {
            let p = ui.painter_at(r);
            p.rect_filled(r, 0.0, t.surface);
            if pane == ReadingPane::Bottom {
                p.hline(r.x_range(), r.top() + 0.5, (1.0, t.line));
            } else {
                p.vline(r.left() + 0.5, r.y_range(), (1.0, t.line));
            }
            if composing {
                self.compose_form(ui, r.shrink2(vec2(1.0, 1.0)));
            } else {
                self.reading_pane(ui, r);
            }
        }
    }

    pub(crate) fn nav_strip(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.pane);
        p.vline(rect.right() - 0.5, rect.y_range(), (1.0, t.line));
        let modules = [
            (Module::Mail, Icon::Mail, "Mail"),
            (Module::Calendar, Icon::Calendar, "Calendar"),
            (Module::People, Icon::People, "People"),
            (Module::Tasks, Icon::Tasks, "Tasks"),
        ];
        let pitch = 48.0;
        for (n, (module, icon, label)) in modules.into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(rect.left(), rect.top() + 8.0 + n as f32 * pitch), vec2(rect.width() - 1.0, 40.0));
            let resp = ui.interact(r, egui::Id::new(("nav", label)), Sense::click());
            let active = self.session.module == module;
            if resp.hovered() || active {
                p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 4.0, if active { t.selected } else { t.hover });
            }
            if active {
                p.rect_filled(Rect::from_min_size(pos2(r.left() + 1.0, r.top() + 9.0), vec2(3.0, 22.0)), 1.5, t.accent);
            }
            icons::paint(&p, Rect::from_center_size(r.center(), vec2(20.0, 20.0)), icon, if active { t.accent } else { t.text_dim });
            if resp.clicked() {
                self.act("nav.module", json!({ "module": label.to_ascii_lowercase() }));
            }
            resp.on_hover_text(label);
        }
    }

    fn folder_pane(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        ui.painter_at(rect).rect_filled(rect, 0.0, t.pane);
        ui.painter_at(rect).vline(rect.right() - 0.5, rect.y_range(), (1.0, t.line));
        // Flatten the tree first: (store, depth, folder) rows in display order.
        struct Line {
            store: usize,
            depth: usize,
            folder: Option<FolderInfo>,
            title: String,
            open: bool,
            parent: bool,
        }
        let mut lines: Vec<Line> = Vec::new();
        for (store, folders) in self.session.folders.iter().enumerate() {
            let title = self.session.stores.get(store).map(|s| s.display_name()).unwrap_or_default();
            let open = !self.collapsed.contains(&(store, u64::MAX));
            lines.push(Line { store, depth: 0, folder: None, title, open, parent: true });
            if !open {
                continue;
            }
            // Depth-first with an explicit stack; a folder is visited once even if parents loop.
            let mut seen: Vec<FolderId> = Vec::new();
            let mut stack: Vec<(usize, &FolderInfo)> = children(folders, None).into_iter().rev().map(|f| (1, f)).collect();
            while let Some((depth, f)) = stack.pop() {
                if seen.contains(&f.id) || lines.len() > 20_000 {
                    continue;
                }
                seen.push(f.id);
                let kids = children(folders, Some(f.id));
                let open = !self.collapsed.contains(&(store, f.id.0));
                lines.push(Line { store, depth, folder: Some(f.clone()), title: f.name.clone(), open, parent: !kids.is_empty() });
                if open && depth < 32 {
                    stack.extend(kids.into_iter().rev().map(|k| (depth + 1, k)));
                }
            }
        }
        let current = self.session.current;
        let mut acts: Vec<(&str, Value)> = Vec::new();
        ui.scope_builder(UiBuilder::new().max_rect(rect.shrink2(vec2(1.0, 0.0))), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ScrollArea::vertical().id_salt("folders").auto_shrink(false).show_rows(ui, 26.0, lines.len(), |ui, range| {
                for line in lines.iter().skip(range.start).take(range.len()) {
                    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                    let p = ui.painter();
                    let folder_id = line.folder.as_ref().map(|f| f.id);
                    let selected = folder_id.is_some_and(|id| current == Some((line.store, id)));
                    if selected {
                        p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 3.0, t.selected);
                    } else if resp.hovered() {
                        p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 3.0, t.hover);
                    }
                    let x = r.left() + 8.0 + line.depth as f32 * 14.0;
                    let chevron = Rect::from_center_size(pos2(x + 5.0, r.center().y), vec2(9.0, 9.0));
                    let toggle = line.parent && resp.clicked() && resp.interact_pointer_pos().is_some_and(|pos| pos.x < chevron.right() + 6.0);
                    if line.parent {
                        icons::paint(p, chevron, if line.open { Icon::ChevronDown } else { Icon::ChevronRight }, t.text_dim);
                    }
                    let mut right = r.right() - 10.0;
                    match &line.folder {
                        None => {
                            clip_text(p, pos2(x + 16.0, r.center().y - 8.5), right - x - 16.0, &line.title, bold(12.5), t.text);
                            if resp.clicked() {
                                acts.push(("ui.toggleFolder", json!({ "store": line.store, "folderId": u64::MAX })));
                            }
                        }
                        Some(f) => {
                            let count = match f.kind {
                                FolderKind::Drafts | FolderKind::Outbox if f.total > 0 => Some((format!("[{}]", f.total), t.text_dim)),
                                _ if f.unread > 0 => Some((f.unread.to_string(), t.accent)),
                                _ => None,
                            };
                            if let Some((text, color)) = count {
                                right -= text_width(p, &text, bold(12.0));
                                p.text(pos2(right, r.center().y), Align2::LEFT_CENTER, &text, bold(12.0), color);
                                right -= 6.0;
                            }
                            icons::paint(
                                p,
                                Rect::from_center_size(pos2(x + 23.0, r.center().y), vec2(14.0, 14.0)),
                                folder_icon(f.kind),
                                if selected { t.accent } else { t.text_dim },
                            );
                            let name_font = if f.unread > 0 { bold(12.5) } else { font(12.5) };
                            clip_text(p, pos2(x + 36.0, r.center().y - 8.5), right - x - 36.0, &line.title, name_font, t.text);
                            if toggle {
                                acts.push(("ui.toggleFolder", json!({ "store": line.store, "folderId": f.id.0 })));
                            } else if resp.clicked() {
                                acts.push(("folder.select", json!({ "store": line.store, "folderId": f.id.0 })));
                            }
                        }
                    }
                }
            });
        });
        for (id, params) in acts {
            self.act(id, params);
        }
    }

    fn message_list(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.surface);
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), 34.0));
        p.hline(rect.x_range(), header.bottom() - 0.5, (1.0, t.line));

        // "All | Unread" on the left, the arrangement on the right.
        let mut x = header.left() + 14.0;
        for (label, unread_only) in [("All", false), ("Unread", true)] {
            let active = self.session.unread_only == unread_only;
            let w = text_width(&p, label, bold(13.0));
            let r = Rect::from_min_size(pos2(x - 4.0, header.top() + 4.0), vec2(w + 8.0, 26.0));
            let resp = ui.interact(r, egui::Id::new(("list-filter", label)), Sense::click());
            p.text(
                pos2(x, header.center().y),
                Align2::LEFT_CENTER,
                label,
                if active { bold(13.0) } else { font(13.0) },
                if active { t.accent } else { t.text_dim },
            );
            if active {
                p.rect_filled(Rect::from_min_size(pos2(x, header.bottom() - 5.0), vec2(w, 2.5)), 1.0, t.accent);
            }
            if resp.clicked() && !active {
                self.act("view.filterUnread", json!({ "on": unread_only }));
            }
            x += w + 18.0;
        }
        let sort_name = match self.session.sort {
            SortKey::Date => "Date",
            SortKey::From => "From",
            SortKey::To => "To",
            SortKey::Subject => "Subject",
            SortKey::Size => "Size",
            SortKey::Importance => "Importance",
            SortKey::Flag => "Flag",
            SortKey::Attachments => "Attachments",
        };
        let sort_label = format!("By {sort_name}  {}", if self.session.sort_desc { "↓" } else { "↑" });
        let sort_w = text_width(&p, &sort_label, font(12.0));
        let sort_rect = Rect::from_min_size(pos2(header.right() - sort_w - 16.0, header.top() + 4.0), vec2(sort_w + 12.0, 26.0));
        let sort_resp = ui.interact(sort_rect, egui::Id::new("list-sort"), Sense::click());
        if sort_resp.hovered() {
            p.rect_filled(sort_rect, 3.0, t.hover);
        }
        p.text(sort_rect.center(), Align2::CENTER_CENTER, &sort_label, font(12.0), t.text_dim);
        if sort_resp.clicked() {
            self.act("ui.menu", json!({ "menu": "arrange", "x": sort_rect.left(), "y": sort_rect.bottom() }));
        }

        let body = Rect::from_min_max(header.left_bottom(), rect.max);
        let s = &self.session;
        if s.rows.is_empty() {
            let message = if s.stores.is_empty() {
                "Open a data file from the File tab"
            } else if !s.search.is_empty() {
                "Nothing matches the search"
            } else {
                "There is nothing to show here"
            };
            p.text(pos2(body.center().x, body.top() + 40.0), Align2::CENTER_CENTER, message, font(12.5), t.text_faint);
            return;
        }

        // Lay the rows out once, each group under its heading, however the list is arranged.
        let groups = s.groups();
        let now_day = time::civil(s.now, s.tz_offset_min).day_number;
        let mut entries: Vec<(f32, Entry)> = Vec::with_capacity(s.rows.len() + groups.len());
        let mut y = 0.0;
        let mut next = 0;
        for i in 0..s.rows.len() {
            if groups.get(next).is_some_and(|g| g.start == i) {
                entries.push((y, Entry::Group(next)));
                y += GROUP_H;
                next += 1;
            }
            entries.push((y, Entry::Row(i)));
            y += ROW_H;
        }
        let total = y;
        let selected = s.selected;
        let scroll_to = std::mem::take(&mut self.scroll_to_selected);
        let tz = s.tz_offset_min;
        let rows = &s.rows;
        let mut clicked: Option<u64> = None;
        ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ScrollArea::vertical().id_salt("messages").auto_shrink(false).show_viewport(ui, |ui, viewport| {
                let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), total), Sense::hover());
                let p = ui.painter().clone();
                let first = entries.partition_point(|(y, _)| *y + ROW_H < viewport.top());
                for (y, entry) in entries.iter().skip(first) {
                    if *y > viewport.bottom() && !scroll_to {
                        break;
                    }
                    match entry {
                        Entry::Group(at) => {
                            let Some(group) = groups.get(*at) else { continue };
                            let r = Rect::from_min_size(pos2(area.left(), area.top() + y), vec2(area.width(), GROUP_H));
                            icons::paint(
                                &p,
                                Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(8.0, 8.0)),
                                Icon::ChevronDown,
                                t.text_dim,
                            );
                            // How many the group holds, and how many of them wait to be read.
                            let count = match group.unread {
                                0 => group.count.to_string(),
                                unread => format!("{unread} unread of {}", group.count),
                            };
                            let counted = p.text(pos2(r.right() - 12.0, r.center().y), Align2::RIGHT_CENTER, count, font(11.0), t.text_faint);
                            clip_text(
                                &p,
                                pos2(r.left() + 24.0, r.center().y - 8.0),
                                (counted.left() - r.left() - 32.0).max(0.0),
                                &group.label,
                                bold(11.5),
                                t.text_dim,
                            );
                        }
                        Entry::Row(i) => {
                            let Some(row) = rows.get(*i) else { continue };
                            let r = Rect::from_min_size(pos2(area.left(), area.top() + y), vec2(area.width(), ROW_H));
                            let is_selected = selected == Some(row.id);
                            if is_selected && scroll_to {
                                ui.scroll_to_rect(r, None);
                            }
                            if *y > viewport.bottom() {
                                continue;
                            }
                            let resp = ui.interact(r, egui::Id::new(("row", row.id)), Sense::click());
                            if is_selected {
                                p.rect_filled(r, 0.0, t.selected);
                            } else if resp.hovered() {
                                p.rect_filled(r, 0.0, t.hover);
                            }
                            draw_row(&p, r, row, now_day, tz, &t);
                            if resp.clicked() {
                                clicked = Some(row.id);
                            }
                        }
                    }
                }
            });
        });
        if let Some(id) = clicked {
            self.act("mail.select", json!({ "id": id }));
        }
    }

    fn reading_pane(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let doc = self.body_doc();
        if let (Some(doc), Some((key, _)), Some(item)) = (&doc, &self.body_doc, self.session.open.as_ref()) {
            self.pictures.load(ui.ctx(), *key, doc, item);
            self.faces.load(ui.ctx(), *key, doc);
        }
        let p = ui.painter_at(rect);
        let Some(item) = self.session.open.as_ref() else {
            icons::paint(&p, Rect::from_center_size(rect.center() - vec2(0.0, 26.0), vec2(52.0, 52.0)), Icon::Mail, t.disabled);
            p.text(rect.center() + vec2(0.0, 22.0), Align2::CENTER_CENTER, "Select an item to read", font(14.0), t.text_dim);
            return;
        };
        let pad = 18.0;
        let left = rect.left() + pad;
        let width = rect.width() - pad * 2.0;
        let mut y = rect.top() + 14.0;
        clip_text(&p, pos2(left, y), width, &item.subject(), font(18.0), t.text);
        y += 34.0;

        // Reply / Reply All / Forward on the right of the sender line.
        let mut bx = rect.right() - pad;
        let mut acts: Vec<&'static str> = Vec::new();
        for (icon, label, cmd) in
            [(Icon::Forward, "Forward", "mail.forward"), (Icon::ReplyAll, "Reply All", "mail.replyAll"), (Icon::Reply, "Reply", "mail.reply")]
        {
            let w = text_width(&p, label, font(12.0)) + 32.0;
            bx -= w;
            let r = Rect::from_min_size(pos2(bx, y), vec2(w - 4.0, 26.0));
            let resp = ui.interact(r, egui::Id::new(("reading-action", cmd)), Sense::click());
            p.rect_filled(r, 3.0, if resp.hovered() { t.hover } else { t.surface });
            p.rect_stroke(r, 3.0, (1.0, t.line), egui::StrokeKind::Inside);
            icons::paint(&p, Rect::from_center_size(pos2(r.left() + 13.0, r.center().y), vec2(14.0, 14.0)), icon, t.accent);
            p.text(pos2(r.left() + 24.0, r.center().y), Align2::LEFT_CENTER, label, font(12.0), t.text);
            if resp.clicked() {
                acts.push(cmd);
            }
        }
        let sender = item.sender_name();
        let email = item.sender_email();
        p.circle_filled(pos2(left + 18.0, y + 18.0), 18.0, theme::avatar_color(&sender));
        p.text(pos2(left + 18.0, y + 18.0), Align2::CENTER_CENTER, initials(&sender), bold(13.0), t.on_accent);
        let text_x = left + 46.0;
        let text_w = bx - text_x - 10.0;
        let from = if email.is_empty() || email == sender { sender.clone() } else { format!("{sender} <{email}>") };
        clip_text(&p, pos2(text_x, y - 1.0), text_w, &from, bold(13.0), t.text);
        let to = item.display_recipients(RECIP_TO);
        clip_text(&p, pos2(text_x, y + 18.0), text_w, &format!("To  {to}"), font(12.0), t.text_dim);
        y += 38.0;
        let cc = item.display_recipients(RECIP_CC);
        if !cc.is_empty() {
            clip_text(&p, pos2(text_x, y), width - 46.0, &format!("Cc  {cc}"), font(12.0), t.text_dim);
            y += 18.0;
        }
        let stamp = time::civil(item.time(), self.session.tz_offset_min).long_stamp();
        clip_text(&p, pos2(text_x, y), width - 46.0, &stamp, font(12.0), t.text_faint);
        y += 24.0;

        // Flag, importance and categories.
        let mut tx = left;
        if item.flag() != Flag::None {
            let done = item.flag() == Flag::Complete;
            icons::paint(
                &p,
                Rect::from_min_size(pos2(tx, y + 2.0), vec2(13.0, 13.0)),
                if done { Icon::Check } else { Icon::Flag },
                if done { t.ok } else { t.flag },
            );
            tx += 18.0;
            tx += clip_text(&p, pos2(tx, y), 200.0, if done { "Completed" } else { "Follow up" }, font(12.0), t.text_dim) + 14.0;
        }
        if item.importance() == 2 {
            icons::paint(&p, Rect::from_min_size(pos2(tx, y + 2.0), vec2(13.0, 13.0)), Icon::Importance, t.danger);
            tx += 16.0;
            tx += clip_text(&p, pos2(tx, y), 200.0, "High importance", font(12.0), t.text_dim) + 14.0;
        }
        for category in item.categories.iter().take(8) {
            let name = one_line(category);
            let w = text_width(&p, &name, font(11.5)).min(180.0) + 14.0;
            if tx + w > rect.right() - pad {
                break;
            }
            let chip = Rect::from_min_size(pos2(tx, y), vec2(w, 18.0));
            p.rect_filled(chip, 3.0, theme::category_color(category).gamma_multiply(0.25));
            p.rect_filled(Rect::from_min_size(chip.min, vec2(4.0, 18.0)), 2.0, theme::category_color(category));
            clip_text(&p, pos2(chip.left() + 8.0, chip.top() + 1.0), w - 10.0, &name, font(11.5), t.text);
            tx += w + 6.0;
        }
        if tx > left {
            y += 26.0;
        }

        // Attachments: a click only asks where to save. Nothing is opened or run.
        // The name is shown as it would be saved, so nothing in it is hidden or reversed.
        let files: Vec<(usize, String, usize, Option<String>)> = mailcraft_engine::attachments::files(item)
            .map(|(i, a)| (i, mailcraft_engine::attachments::file_name(a), a.size(), mailcraft_engine::attachments::caution_of(a).map(|c| c.words())))
            .take(32)
            .collect();
        let mut save: Option<usize> = None;
        let mut link: Option<std::sync::Arc<str>> = None;
        let mut save_all = false;
        if !files.is_empty() {
            let mut ax = left;
            for (index, name, size, caution) in &files {
                let label = format!("{}  ({})", one_line(name), human_size(*size));
                let w = (text_width(&p, &label, font(12.0)) + 36.0).min(width);
                if ax + w > rect.right() - pad && ax > left {
                    ax = left;
                    y += 32.0;
                }
                let chip = Rect::from_min_size(pos2(ax, y), vec2(w, 28.0));
                let resp = ui.interact(chip, egui::Id::new(("attachment", *index)), Sense::click());
                p.rect_filled(chip, 3.0, if resp.hovered() { t.hover } else { t.pane });
                // An attachment a reader should think twice about says so before it is pointed at.
                let (line, icon, tint) = match caution {
                    Some(_) => ((1.5, t.danger), Icon::Importance, t.danger),
                    None => ((1.0, t.line), Icon::Clip, t.text_dim),
                };
                p.rect_stroke(chip, 3.0, line, egui::StrokeKind::Inside);
                icons::paint(&p, Rect::from_center_size(pos2(chip.left() + 14.0, chip.center().y), vec2(14.0, 14.0)), icon, tint);
                // A long name is cut in its middle: the ending is what tells the kind of file.
                let room = w - 32.0;
                let label = if text_width(&p, &label, font(12.0)) > room { crate::middle_cut(&p, &label, room, font(12.0)) } else { label };
                clip_text(&p, pos2(chip.left() + 26.0, chip.center().y - 8.0), room, &label, font(12.0), t.text);
                if resp.clicked() {
                    save = Some(*index);
                }
                resp.on_hover_text(match caution {
                    Some(words) => format!("{words}\nSave this attachment to a file. Nothing is opened or run from here."),
                    None => "Save this attachment to a file. Nothing is opened or run from here.".to_string(),
                });
                ax += w + 8.0;
            }
            if files.len() > 1 {
                let label = "Save all…";
                let w = text_width(&p, label, font(12.0)) + 20.0;
                if ax + w > rect.right() - pad && ax > left {
                    ax = left;
                    y += 32.0;
                }
                let chip = Rect::from_min_size(pos2(ax, y), vec2(w, 28.0));
                let resp = ui.interact(chip, egui::Id::new("attachments-all"), Sense::click());
                if resp.hovered() {
                    p.rect_filled(chip, 3.0, t.hover);
                }
                p.text(chip.center(), egui::Align2::CENTER_CENTER, label, font(12.0), t.accent);
                save_all = resp.clicked();
                resp.on_hover_text("Save every attachment into a folder. No file there is replaced.");
            }
            y += 38.0;
        }
        // Says only what was held back; a formatted body needs no notice of its own.
        let notice = match &doc {
            Some(d) if d.truncated => Some("This message is very long; the rest is not shown. Remote content is never downloaded."),
            Some(d) if d.remote_blocked > 0 => Some("Pictures and other remote content in this message were not downloaded."),
            Some(_) => None,
            None if item.body_html().is_some() || item.body_rtf().is_some() => {
                Some("Shown as plain text. Pictures and remote content are never downloaded.")
            }
            None => None,
        };
        if let Some(notice) = notice {
            let bar = Rect::from_min_size(pos2(rect.left() + 1.0, y), vec2(rect.width() - 1.0, 24.0));
            p.rect_filled(bar, 0.0, t.info_bar);
            icons::paint(&p, Rect::from_center_size(pos2(bar.left() + pad, bar.center().y), vec2(13.0, 13.0)), Icon::Info, t.text_dim);
            clip_text(&p, pos2(bar.left() + pad + 12.0, bar.center().y - 8.0), bar.width() - 40.0, notice, font(11.5), t.text_dim);
            y += 30.0;
        }
        p.hline((rect.left() + 1.0)..=rect.right(), y, (1.0, t.line));
        y += 1.0;

        let plain = match &doc {
            Some(_) => String::new(),
            None => {
                let full = item.body_text();
                let cut = full.char_indices().nth(MAX_BODY_CHARS).map(|(i, _)| i);
                let mut body: String = full.get(..cut.unwrap_or(full.len())).unwrap_or("").replace('\t', "    ");
                body.retain(|c| c == '\n' || !c.is_control());
                if cut.is_some() {
                    body.push_str("\n\n[The rest of this message is not shown.]");
                }
                body
            }
        };
        let body_rect = Rect::from_min_max(pos2(rect.left() + 1.0, y), rect.max);
        ui.scope_builder(UiBuilder::new().max_rect(body_rect), |ui| {
            ScrollArea::vertical().id_salt(("body", self.session.selected)).auto_shrink(false).show(ui, |ui| {
                egui::Frame::NONE.inner_margin(egui::Margin::symmetric(pad as i8, 12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match &doc {
                        // An attachment that stands in the text is saved like one in the list above.
                        Some(doc) => {
                            let picked = body::draw(ui, doc, &t, &self.pictures, &self.faces);
                            save = picked.file.or(save);
                            link = picked.link;
                        }
                        None => {
                            ui.add(egui::Label::new(egui::RichText::new(plain).font(font(13.5)).color(t.text)).wrap().selectable(true));
                        }
                    }
                });
            });
        });
        for cmd in acts {
            self.act(cmd, Value::Null);
        }
        if let Some(index) = save {
            self.act("ui.saveAttachmentDialog", json!({ "index": index }));
        }
        if let Some(link) = link {
            self.act("ui.linkDialog", json!({ "address": link.chars().filter(|c| !c.is_control()).collect::<String>() }));
        }
        if save_all {
            self.act("ui.saveAllAttachmentsDialog", Value::Null);
        }
    }

    fn compose_form(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let Some(draft) = self.session.compose.as_ref() else { return };
        let mut fields = [
            ("to", "To", draft.to.clone(), true),
            ("cc", "Cc", draft.cc.clone(), true),
            ("bcc", "Bcc", draft.bcc.clone(), draft.show_bcc),
            ("subject", "Subject", draft.subject.clone(), true),
        ];
        let mut body = draft.body.clone();
        let attachments: Vec<(String, usize)> = draft.attachments.iter().map(|a| (a.name(), a.size())).collect();
        let show_bcc = draft.show_bcc;
        // What the writer should know is on show while the message can still be changed.
        let cautions: Vec<String> = mailcraft_engine::outgoing::cautions(draft).iter().map(|c| c.words()).collect();
        let mut acts: Vec<(&str, Value)> = Vec::new();
        ui.scope_builder(UiBuilder::new().max_rect(rect.shrink2(vec2(14.0, 10.0))), |ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            ui.horizontal(|ui| {
                let send = ui.add(
                    egui::Button::new(
                        egui::RichText::new(if cautions.is_empty() { "Send" } else { "Send Anyway" }).font(bold(13.0)).color(t.on_accent),
                    )
                    .fill(t.accent)
                    .min_size(vec2(72.0, 30.0)),
                );
                if send.on_hover_text("Files the message in Outbox. Sending needs an account, which is not built yet.").clicked() {
                    acts.push(("compose.send", json!({ "anyway": !cautions.is_empty() })));
                }
                for (label, cmd, params) in [
                    ("Attach File…", "ui.attachDialog", Value::Null),
                    (if show_bcc { "Hide Bcc" } else { "Bcc" }, "compose.showBcc", Value::Null),
                    ("Save Draft", "compose.saveDraft", Value::Null),
                    ("Discard", "compose.discard", Value::Null),
                ] {
                    if ui.add(egui::Button::new(egui::RichText::new(label).font(font(12.5))).min_size(vec2(0.0, 30.0))).clicked() {
                        acts.push((cmd, params));
                    }
                }
            });
            for caution in &cautions {
                ui.horizontal(|ui| {
                    let (mark, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
                    icons::paint(ui.painter(), Rect::from_center_size(mark.center(), vec2(14.0, 14.0)), Icon::Importance, t.danger);
                    ui.add(egui::Label::new(egui::RichText::new(one_line(caution)).font(font(12.0)).color(t.text)).wrap());
                });
            }
            ui.add_space(4.0);
            for (key, label, value, shown) in &mut fields {
                if !*shown {
                    continue;
                }
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(58.0, 24.0), egui::Label::new(egui::RichText::new(*label).font(font(12.5)).color(t.text_dim)));
                    let edit = egui::TextEdit::singleline(value).font(font(13.0)).desired_width(f32::INFINITY).id(egui::Id::new(("compose", *key)));
                    if ui.add(edit).changed() {
                        acts.push(("compose.setField", json!({ "field": key, "value": value })));
                    }
                });
            }
            if !attachments.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for (i, (name, size)) in attachments.iter().enumerate() {
                        let label = format!("{}  ({})  ✕", one_line(name), human_size(*size));
                        if ui.add(egui::Button::new(egui::RichText::new(label).font(font(12.0)))).on_hover_text("Remove this attachment").clicked() {
                            acts.push(("compose.removeAttachment", json!({ "index": i })));
                        }
                    }
                });
            }
            ui.separator();
            ScrollArea::vertical().id_salt("compose-body").auto_shrink(false).show(ui, |ui| {
                let edit = egui::TextEdit::multiline(&mut body)
                    .font(font(13.5))
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY)
                    .desired_rows(16)
                    .id(egui::Id::new(("compose", "body")));
                if ui.add(edit).changed() {
                    acts.push(("compose.setField", json!({ "field": "body", "value": body })));
                }
            });
        });
        for (id, params) in acts {
            self.act(id, params);
        }
    }
}

/// Children of `parent` in pane order: special folders first, then by name.
fn children(folders: &[FolderInfo], parent: Option<FolderId>) -> Vec<&FolderInfo> {
    let mut out: Vec<&FolderInfo> = folders.iter().filter(|f| f.parent == parent && f.kind != FolderKind::Root).collect();
    out.sort_by(|a, b| a.kind.sort_rank().cmp(&b.kind.sort_rank()).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

fn draw_row(p: &egui::Painter, r: Rect, row: &Summary, now_day: i64, tz: i32, t: &theme::Tokens) {
    p.hline(r.x_range(), r.bottom() - 0.5, (1.0, t.line.gamma_multiply(0.6)));
    if !row.read {
        p.rect_filled(Rect::from_min_size(r.min + vec2(0.0, 1.0), vec2(3.0, r.height() - 2.0)), 0.0, t.accent);
    }
    let left = r.left() + 14.0;
    let mut right = r.right() - 10.0;

    // Line 1: who, with the status icons on the right.
    if row.flag != Flag::None {
        let done = row.flag == Flag::Complete;
        right -= 14.0;
        icons::paint(
            p,
            Rect::from_min_size(pos2(right, r.top() + 9.0), vec2(13.0, 13.0)),
            if done { Icon::Check } else { Icon::Flag },
            if done { t.ok } else { t.flag },
        );
        right -= 5.0;
    }
    if row.has_attachments {
        right -= 14.0;
        icons::paint(p, Rect::from_min_size(pos2(right, r.top() + 9.0), vec2(13.0, 13.0)), Icon::Clip, t.text_dim);
        right -= 5.0;
    }
    if row.importance == 2 {
        right -= 12.0;
        icons::paint(p, Rect::from_min_size(pos2(right, r.top() + 9.0), vec2(13.0, 13.0)), Icon::Importance, t.danger);
        right -= 5.0;
    }
    let who = if row.from.is_empty() { &row.to } else { &row.from };
    clip_text(p, pos2(left, r.top() + 6.0), right - left, who, if row.read { font(13.5) } else { bold(13.5) }, t.text);

    // Line 2: subject and when.
    let c = time::civil(row.time, tz);
    let when = if row.time <= 0 {
        String::new()
    } else {
        match now_day.saturating_sub(c.day_number) {
            i64::MIN..=0 => c.time_short(),
            1..=6 => format!("{} {}", c.weekday_name().get(..3).unwrap_or(""), c.time_short()),
            _ => format!("{}/{}/{}", c.month, c.day, c.year),
        }
    };
    let when_w = crate::text_width(p, &when, font(11.5));
    p.text(pos2(r.right() - 10.0, r.top() + 34.0), Align2::RIGHT_CENTER, &when, font(11.5), if row.read { t.text_faint } else { t.accent });
    let subject = if row.subject.trim().is_empty() { "(no subject)" } else { &row.subject };
    clip_text(
        p,
        pos2(left, r.top() + 25.0),
        r.right() - 22.0 - when_w - left,
        subject,
        if row.read { font(12.5) } else { bold(12.5) },
        if row.read { t.text } else { t.accent },
    );

    // Line 3: preview, with category squares on the right.
    let mut cat_x = r.right() - 10.0;
    for category in row.categories.iter().take(4) {
        cat_x -= 11.0;
        p.rect_filled(Rect::from_min_size(pos2(cat_x, r.top() + 46.0), vec2(9.0, 9.0)), 2.0, theme::category_color(category));
        cat_x -= 3.0;
    }
    clip_text(p, pos2(left, r.top() + 43.0), cat_x - left - 4.0, &row.preview, font(12.0), t.text_faint);
}
