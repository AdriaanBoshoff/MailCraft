//! Icons, drawn in code on a 16×16 grid. Every shape here is our own drawing: no icon is copied,
//! traced or recreated from Microsoft Outlook or any other product (see CLAUDE.md, "Assets").

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Mail,
    MailOpen,
    NewMail,
    Calendar,
    People,
    Tasks,
    Reply,
    ReplyAll,
    Forward,
    Delete,
    Archive,
    Junk,
    Move,
    Rules,
    Flag,
    Category,
    Filter,
    Search,
    Folder,
    NewFolder,
    Rename,
    Broom,
    Info,
    Sort,
    Swap,
    Panes,
    Sidebar,
    Sync,
    Clip,
    Send,
    Save,
    Close,
    Importance,
    Check,
    ChevronDown,
    ChevronRight,
    Back,
    Open,
}

struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    s: Stroke,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.r.min.x + x / 16.0 * self.r.width(), self.r.min.y + y / 16.0 * self.r.height())
    }

    fn line(&self, pts: &[(f32, f32)]) {
        self.p.add(Shape::line(pts.iter().map(|&(x, y)| self.at(x, y)).collect(), self.s));
    }

    fn closed(&self, pts: &[(f32, f32)]) {
        self.p.add(Shape::closed_line(pts.iter().map(|&(x, y)| self.at(x, y)).collect(), self.s));
    }

    fn filled(&self, pts: &[(f32, f32)]) {
        self.p.add(Shape::convex_polygon(pts.iter().map(|&(x, y)| self.at(x, y)).collect(), self.s.color, Stroke::NONE));
    }

    fn circle(&self, x: f32, y: f32, radius: f32) {
        self.p.circle_stroke(self.at(x, y), radius / 16.0 * self.r.width(), self.s);
    }

    fn dot(&self, x: f32, y: f32, radius: f32) {
        self.p.circle_filled(self.at(x, y), radius / 16.0 * self.r.width(), self.s.color);
    }

    fn envelope(&self, top: f32, bottom: f32) {
        self.closed(&[(1.5, top), (14.5, top), (14.5, bottom), (1.5, bottom)]);
        self.line(&[(1.5, top), (8.0, top + (bottom - top) * 0.55), (14.5, top)]);
    }

    fn folder(&self) {
        self.closed(&[(1.5, 3.5), (6.0, 3.5), (7.5, 5.5), (14.5, 5.5), (14.5, 13.0), (1.5, 13.0)]);
    }
}

/// Paint `icon` into `rect` (any size; the grid scales) in one colour.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let pen = Pen { p, r: rect, s: Stroke::new((rect.width() / 14.0).clamp(1.0, 2.0), color) };
    match icon {
        Icon::Mail => pen.envelope(3.5, 12.5),
        Icon::MailOpen => {
            pen.closed(&[(1.5, 6.5), (8.0, 2.0), (14.5, 6.5), (14.5, 13.5), (1.5, 13.5)]);
            pen.line(&[(1.5, 6.5), (8.0, 10.5), (14.5, 6.5)]);
        }
        Icon::NewMail => {
            pen.envelope(5.0, 13.5);
            pen.line(&[(12.5, 0.5), (12.5, 4.5)]);
            pen.line(&[(10.5, 2.5), (14.5, 2.5)]);
        }
        Icon::Calendar => {
            pen.closed(&[(2.0, 3.5), (14.0, 3.5), (14.0, 13.5), (2.0, 13.5)]);
            pen.line(&[(2.0, 6.5), (14.0, 6.5)]);
            pen.line(&[(5.0, 2.0), (5.0, 5.0)]);
            pen.line(&[(11.0, 2.0), (11.0, 5.0)]);
            for (x, y) in [(5.0, 9.0), (8.0, 9.0), (11.0, 9.0), (5.0, 11.5), (8.0, 11.5)] {
                pen.dot(x, y, 0.7);
            }
        }
        Icon::People => {
            pen.circle(6.0, 5.5, 2.4);
            pen.line(&[(1.5, 13.5), (2.2, 10.6), (4.4, 9.4), (7.6, 9.4), (9.8, 10.6), (10.5, 13.5)]);
            pen.circle(11.5, 6.5, 1.8);
            pen.line(&[(12.0, 9.8), (13.6, 10.6), (14.5, 13.0)]);
        }
        Icon::Tasks => {
            pen.closed(&[(3.0, 2.5), (13.0, 2.5), (13.0, 14.0), (3.0, 14.0)]);
            pen.line(&[(5.5, 8.5), (7.3, 10.5), (10.8, 6.0)]);
        }
        Icon::Reply => {
            pen.line(&[(6.5, 3.5), (2.0, 7.5), (6.5, 11.5)]);
            pen.line(&[(2.0, 7.5), (10.0, 7.5), (13.0, 9.0), (14.0, 12.5)]);
        }
        Icon::ReplyAll => {
            pen.line(&[(5.5, 3.5), (1.5, 7.5), (5.5, 11.5)]);
            pen.line(&[(9.0, 3.5), (5.0, 7.5), (9.0, 11.5)]);
            pen.line(&[(5.0, 7.5), (11.5, 7.5), (13.8, 9.0), (14.5, 12.5)]);
        }
        Icon::Forward => {
            pen.line(&[(9.5, 3.5), (14.0, 7.5), (9.5, 11.5)]);
            pen.line(&[(14.0, 7.5), (6.0, 7.5), (3.0, 9.0), (2.0, 12.5)]);
        }
        Icon::Delete => {
            pen.line(&[(2.5, 4.5), (13.5, 4.5)]);
            pen.line(&[(6.0, 4.5), (6.5, 2.5), (9.5, 2.5), (10.0, 4.5)]);
            pen.line(&[(4.0, 4.5), (4.8, 14.0), (11.2, 14.0), (12.0, 4.5)]);
            pen.line(&[(6.8, 7.0), (7.0, 11.5)]);
            pen.line(&[(9.2, 7.0), (9.0, 11.5)]);
        }
        Icon::Archive => {
            pen.closed(&[(1.5, 3.0), (14.5, 3.0), (14.5, 6.0), (1.5, 6.0)]);
            pen.line(&[(2.5, 6.0), (2.5, 13.5), (13.5, 13.5), (13.5, 6.0)]);
            pen.line(&[(6.0, 9.0), (10.0, 9.0)]);
        }
        Icon::Junk => {
            pen.circle(8.0, 8.0, 6.0);
            pen.line(&[(3.8, 3.8), (12.2, 12.2)]);
        }
        Icon::Move => {
            pen.folder();
            pen.line(&[(5.0, 9.3), (11.0, 9.3)]);
            pen.line(&[(9.0, 7.3), (11.0, 9.3), (9.0, 11.3)]);
        }
        Icon::Rules => {
            pen.closed(&[(2.0, 2.5), (9.5, 2.5), (9.5, 8.0), (2.0, 8.0)]);
            pen.line(&[(5.7, 8.0), (5.7, 11.5), (9.0, 11.5)]);
            pen.closed(&[(9.0, 9.5), (14.0, 9.5), (14.0, 13.5), (9.0, 13.5)]);
        }
        Icon::Flag => {
            pen.line(&[(4.0, 14.5), (4.0, 2.0)]);
            pen.filled(&[(4.0, 2.5), (12.5, 2.5), (10.5, 5.5), (12.5, 8.5), (4.0, 8.5)]);
        }
        Icon::Category => {
            for (x, y) in [(2.5, 2.5), (9.0, 2.5), (2.5, 9.0), (9.0, 9.0)] {
                pen.closed(&[(x, y), (x + 4.5, y), (x + 4.5, y + 4.5), (x, y + 4.5)]);
            }
        }
        Icon::Filter => {
            pen.closed(&[(2.0, 3.0), (14.0, 3.0), (9.5, 8.5), (9.5, 13.5), (6.5, 12.0), (6.5, 8.5)]);
        }
        Icon::Search => {
            pen.circle(7.0, 7.0, 4.3);
            pen.line(&[(10.2, 10.2), (14.0, 14.0)]);
        }
        Icon::Folder => pen.folder(),
        Icon::NewFolder => {
            pen.closed(&[(1.5, 5.0), (5.5, 5.0), (7.0, 7.0), (13.5, 7.0), (13.5, 14.0), (1.5, 14.0)]);
            pen.line(&[(12.5, 0.5), (12.5, 4.5)]);
            pen.line(&[(10.5, 2.5), (14.5, 2.5)]);
        }
        Icon::Rename => {
            pen.closed(&[(1.5, 5.0), (14.5, 5.0), (14.5, 11.0), (1.5, 11.0)]);
            pen.line(&[(5.0, 3.0), (5.0, 13.0)]);
            pen.line(&[(3.5, 3.0), (6.5, 3.0)]);
            pen.line(&[(3.5, 13.0), (6.5, 13.0)]);
        }
        Icon::Broom => {
            pen.line(&[(12.5, 2.0), (7.5, 8.0)]);
            pen.closed(&[(5.5, 7.0), (9.5, 10.0), (6.5, 14.0), (2.0, 12.5)]);
            pen.line(&[(4.5, 10.0), (6.5, 11.5)]);
        }
        Icon::Info => {
            pen.circle(8.0, 8.0, 6.0);
            pen.line(&[(8.0, 7.3), (8.0, 11.3)]);
            pen.dot(8.0, 5.0, 0.8);
        }
        Icon::Sort => {
            pen.line(&[(2.0, 4.0), (9.0, 4.0)]);
            pen.line(&[(2.0, 8.0), (7.0, 8.0)]);
            pen.line(&[(2.0, 12.0), (5.0, 12.0)]);
            pen.line(&[(12.0, 3.0), (12.0, 13.0)]);
            pen.line(&[(10.0, 11.0), (12.0, 13.0), (14.0, 11.0)]);
        }
        Icon::Swap => {
            pen.line(&[(5.0, 13.0), (5.0, 3.0)]);
            pen.line(&[(3.0, 5.0), (5.0, 3.0), (7.0, 5.0)]);
            pen.line(&[(11.0, 3.0), (11.0, 13.0)]);
            pen.line(&[(9.0, 11.0), (11.0, 13.0), (13.0, 11.0)]);
        }
        Icon::Panes => {
            pen.closed(&[(1.5, 3.0), (14.5, 3.0), (14.5, 13.0), (1.5, 13.0)]);
            pen.line(&[(9.0, 3.0), (9.0, 13.0)]);
            pen.line(&[(10.7, 6.0), (13.0, 6.0)]);
            pen.line(&[(10.7, 8.5), (13.0, 8.5)]);
        }
        Icon::Sidebar => {
            pen.closed(&[(1.5, 3.0), (14.5, 3.0), (14.5, 13.0), (1.5, 13.0)]);
            pen.line(&[(6.0, 3.0), (6.0, 13.0)]);
            pen.line(&[(3.0, 6.0), (4.5, 6.0)]);
            pen.line(&[(3.0, 8.5), (4.5, 8.5)]);
        }
        Icon::Sync => {
            pen.line(&[(13.0, 6.5), (11.5, 4.0), (8.0, 2.8), (4.5, 4.0), (3.0, 7.0)]);
            pen.line(&[(13.5, 3.0), (13.0, 6.5), (9.8, 6.0)]);
            pen.line(&[(3.0, 9.5), (4.5, 12.0), (8.0, 13.2), (11.5, 12.0), (13.0, 9.0)]);
            pen.line(&[(2.5, 13.0), (3.0, 9.5), (6.2, 10.0)]);
        }
        Icon::Clip => {
            pen.line(&[
                (11.5, 7.0),
                (7.0, 11.5),
                (5.0, 11.5),
                (4.0, 10.0),
                (4.5, 8.5),
                (10.0, 3.0),
                (12.0, 3.0),
                (13.0, 4.5),
                (12.5, 6.0),
                (7.5, 11.0),
            ]);
        }
        Icon::Send => {
            pen.closed(&[(2.0, 2.5), (14.5, 8.0), (2.0, 13.5), (4.5, 8.0)]);
            pen.line(&[(4.5, 8.0), (14.5, 8.0)]);
        }
        Icon::Save => {
            pen.closed(&[(2.0, 2.0), (11.5, 2.0), (14.0, 4.5), (14.0, 14.0), (2.0, 14.0)]);
            pen.closed(&[(5.0, 9.0), (11.0, 9.0), (11.0, 14.0), (5.0, 14.0)]);
            pen.line(&[(5.0, 2.0), (5.0, 5.5), (10.0, 5.5), (10.0, 2.0)]);
        }
        Icon::Close => {
            pen.line(&[(3.5, 3.5), (12.5, 12.5)]);
            pen.line(&[(12.5, 3.5), (3.5, 12.5)]);
        }
        Icon::Importance => {
            pen.filled(&[(7.0, 2.0), (9.0, 2.0), (8.6, 10.0), (7.4, 10.0)]);
            pen.dot(8.0, 12.8, 1.1);
        }
        Icon::Check => pen.line(&[(2.5, 8.5), (6.0, 12.0), (13.5, 4.0)]),
        Icon::ChevronDown => pen.line(&[(4.0, 6.0), (8.0, 10.0), (12.0, 6.0)]),
        Icon::ChevronRight => pen.line(&[(6.0, 4.0), (10.0, 8.0), (6.0, 12.0)]),
        Icon::Back => {
            pen.circle(8.0, 8.0, 6.3);
            pen.line(&[(11.5, 8.0), (4.8, 8.0)]);
            pen.line(&[(7.5, 5.2), (4.8, 8.0), (7.5, 10.8)]);
        }
        Icon::Open => {
            pen.line(&[(14.0, 7.0), (14.0, 5.5), (7.5, 5.5), (6.0, 3.5), (1.5, 3.5), (1.5, 13.0)]);
            pen.closed(&[(4.0, 7.5), (15.0, 7.5), (12.5, 13.0), (1.5, 13.0)]);
        }
    }
}
