//! The sample mailbox: invented people at example.org writing invented mail. Every word here is
//! ours; nothing comes from a real mailbox.

use mailcraft_model::{Attachment, Flag, FolderKind, Item, MSGFLAG_READ, MSGFLAG_UNSENT, MemStore, PropValue, RECIP_CC, RECIP_TO, pid};

const MINUTE: i64 = 60 * 10_000_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

const ME: (&str, &str) = ("Alex Morgan", "alex.morgan@example.org");

struct Mail {
    from: (&'static str, &'static str),
    to: &'static [(&'static str, &'static str)],
    cc: &'static [(&'static str, &'static str)],
    subject: &'static str,
    body: &'static str,
    html: Option<&'static str>,
    age: i64,
    read: bool,
}

impl Mail {
    fn item(&self, now: i64) -> Item {
        let mut it = Item::new_mail();
        it.set_subject(self.subject);
        it.set_body_text(self.body);
        if let Some(html) = self.html {
            it.props.set(pid::HTML, PropValue::Binary(html.as_bytes().to_vec()));
            it.props.set(pid::INTERNET_CODEPAGE, PropValue::Int32(65001));
        }
        it.set_sender(self.from.0, self.from.1);
        for (name, email) in self.to {
            it.add_recipient(RECIP_TO, name, email);
        }
        for (name, email) in self.cc {
            it.add_recipient(RECIP_CC, name, email);
        }
        let when = now.saturating_sub(self.age);
        it.props.set(pid::CLIENT_SUBMIT_TIME, PropValue::Time(when.saturating_sub(MINUTE)));
        it.props.set(pid::MESSAGE_DELIVERY_TIME, PropValue::Time(when));
        it.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(if self.read { MSGFLAG_READ } else { 0 }));
        it
    }
}

const PRIYA: (&str, &str) = ("Priya Raman", "priya.raman@example.org");
const TOMAS: (&str, &str) = ("Tomás Lindqvist", "tomas.lindqvist@example.org");
const JUNE: (&str, &str) = ("June Okafor", "june.okafor@example.org");
const FACILITIES: (&str, &str) = ("Facilities Desk", "facilities@example.org");
const DEPOT: (&str, &str) = ("Northgate Print Depot", "orders@printdepot.example.com");
const HELENA: (&str, &str) = ("Helena Brandt", "helena.brandt@example.org");
const TRAVEL: (&str, &str) = ("Travel Bookings", "bookings@travel.example.net");

const LAUNCH_HTML: &str = "<html><body style=\"font-family:Aptos,Calibri,sans-serif;font-size:11pt\">\
<p>Hi Alex,</p>\
<p>The <b>launch checklist</b> is in good shape. Three things are still open:</p>\
<ul><li>Final sign-off on the pricing page</li><li>Support rota for the first week</li><li>The <i>welcome</i> email copy</li></ul>\
<p>Can you take the first one before <span style=\"color:#C00000\">Thursday noon</span>? I have attached the current timeline.</p>\
<p>Thanks,<br>Priya</p></body></html>";

/// A writable in-memory mailbox dated around `now` (UTC FILETIME ticks).
pub fn sample_store(now: i64) -> MemStore {
    let mut s = MemStore::with_default_folders("alex.morgan@example.org");
    let folder = |s: &MemStore, kind| s.folder_by_kind(kind);
    let (Some(inbox), Some(sent), Some(drafts), Some(deleted)) =
        (folder(&s, FolderKind::Inbox), folder(&s, FolderKind::Sent), folder(&s, FolderKind::Drafts), folder(&s, FolderKind::Deleted))
    else {
        return s;
    };
    let projects = s.push_folder(Some(inbox), "Projects", "IPF.Note");
    s.push_folder(Some(inbox), "Receipts", "IPF.Note");

    let mut launch = Mail {
        from: PRIYA,
        to: &[ME],
        cc: &[TOMAS],
        subject: "Launch checklist: three items left",
        body: "Hi Alex,\n\nThe launch checklist is in good shape. Three things are still open:\n\n  - Final sign-off on the pricing page\n  - Support rota for the first week\n  - The welcome email copy\n\nCan you take the first one before Thursday noon? I have attached the current timeline.\n\nThanks,\nPriya",
        html: Some(LAUNCH_HTML),
        age: 25 * MINUTE,
        read: false,
    }
    .item(now);
    launch.attachments.push(Attachment::file(
        "launch-timeline.csv",
        "text/csv",
        b"milestone,owner,due\nPricing page sign-off,Alex,Thursday\nSupport rota,June,Friday\nWelcome email,Priya,Friday\n".to_vec(),
    ));
    launch.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(0x10));
    launch.props.set(pid::IMPORTANCE, PropValue::Int32(2));
    launch.set_flag(Flag::Flagged);
    launch.categories = vec!["Red category".to_string()];
    s.push_item(inbox, launch);

    let inbox_mail = [
        Mail {
            from: TOMAS,
            to: &[ME, PRIYA],
            cc: &[],
            subject: "RE: Pricing page wording",
            body: "Works for me. I would drop the second paragraph entirely: it repeats the table above it.\n\nIf we keep it, \"per seat\" needs to be \"per person\" to match the rest of the site.\n\nTomás",
            html: None,
            age: 2 * HOUR + 10 * MINUTE,
            read: false,
        },
        Mail {
            from: FACILITIES,
            to: &[ME],
            cc: &[],
            subject: "Lift maintenance on Friday morning",
            body: "The east lift is out of service on Friday from 7:00 to 10:00 for its yearly inspection. The west lift and the stairs stay open.\n\nDeliveries that need the goods lift should be booked for the afternoon.\n\nFacilities Desk",
            html: None,
            age: 5 * HOUR,
            read: true,
        },
        Mail {
            from: JUNE,
            to: &[ME],
            cc: &[],
            subject: "Support rota draft",
            body: "Alex,\n\nFirst pass at the rota for launch week is below. I have you down for Tuesday and Thursday mornings; shout if that clashes with anything.\n\nMon: June, Tomás\nTue: Alex, Helena\nWed: Priya, June\nThu: Alex, Tomás\nFri: Helena, Priya\n\nJune",
            html: None,
            age: DAY + 3 * HOUR,
            read: false,
        },
        Mail {
            from: DEPOT,
            to: &[ME],
            cc: &[],
            subject: "Your order 48213 is ready for collection",
            body: "Hello Alex Morgan,\n\nYour order 48213 (200 folded leaflets, 2 roll-up banners) is ready. Collection is from the front counter, weekdays 8:30 to 17:00. Please bring this message with you.\n\nNorthgate Print Depot",
            html: None,
            age: DAY + 7 * HOUR,
            read: true,
        },
        Mail {
            from: HELENA,
            to: &[ME],
            cc: &[JUNE],
            subject: "Notes from the customer call",
            body: "Short version: they like the import tool, they are worried about the migration window, and they want a named contact for the first month.\n\nI promised a written plan by the end of next week. Can we find thirty minutes on Monday to split it up?\n\nHelena",
            html: None,
            age: 2 * DAY + 2 * HOUR,
            read: true,
        },
        Mail {
            from: TRAVEL,
            to: &[ME],
            cc: &[],
            subject: "Itinerary: Rotterdam, two nights",
            body: "Booking reference KX7Q2M\n\nOutbound: Tuesday 07:42, platform to be announced, seat 54 coach 8\nReturn: Thursday 18:15, seat 31 coach 4\nHotel: Maaskade Rooms, two nights, breakfast included\n\nChanges are free until 24 hours before departure.",
            html: None,
            age: 4 * DAY,
            read: true,
        },
        Mail {
            from: PRIYA,
            to: &[ME],
            cc: &[],
            subject: "Welcome email copy, second draft",
            body: "Shorter this time, as you asked. The opening line now says what the product does instead of how excited we are.\n\nStill unsure about the sign-off. \"The team\" feels anonymous; a name might be better.\n\nPriya",
            html: None,
            age: 8 * DAY + 4 * HOUR,
            read: true,
        },
        Mail {
            from: TOMAS,
            to: &[ME],
            cc: &[],
            subject: "Budget numbers for the offsite",
            body: "Venue 1,800, catering 950, travel about 1,200 depending on who drives. That leaves a little over 500 of the 4,500 for anything we forgot.\n\nTomás",
            html: None,
            age: 16 * DAY,
            read: true,
        },
    ];
    for (i, m) in inbox_mail.iter().enumerate() {
        let mut it = m.item(now);
        match i {
            2 => it.categories = vec!["Blue category".to_string()],
            4 => it.set_flag(Flag::Complete),
            _ => {}
        }
        s.push_item(inbox, it);
    }

    let project_mail = [
        Mail {
            from: HELENA,
            to: &[ME],
            cc: &[],
            subject: "Import tool: field mapping",
            body: "The mapping table is done for contacts and companies. Deals are next; the stage names differ between the two systems, so that one needs a decision from you.\n\nHelena",
            html: None,
            age: 3 * DAY,
            read: true,
        },
        Mail {
            from: JUNE,
            to: &[ME, HELENA],
            cc: &[],
            subject: "Import tool: test results",
            body: "Ran the importer against the three sample exports. Two went through clean. The third stops at row 4,112 on a date written as text.\n\nJune",
            html: None,
            age: 6 * DAY,
            read: false,
        },
    ];
    for m in &project_mail {
        s.push_item(projects, m.item(now));
    }

    for (to, subject, body, age) in [
        (PRIYA, "RE: Launch checklist: three items left", "I will take the pricing page. Sign-off by Thursday 11:00.\n\nAlex", 10 * MINUTE),
        (HELENA, "RE: Notes from the customer call", "Monday 10:00 works. I will bring a first outline.\n\nAlex", 2 * DAY),
    ] {
        let mut it = Mail { from: ME, to: &[], cc: &[], subject, body, html: None, age, read: true }.item(now);
        it.add_recipient(RECIP_TO, to.0, to.1);
        it.props.remove(pid::MESSAGE_DELIVERY_TIME);
        it.props.set(pid::CLIENT_SUBMIT_TIME, PropValue::Time(now.saturating_sub(age)));
        s.push_item(sent, it);
    }

    let mut draft = Item::new_mail();
    draft.set_subject("Offsite agenda");
    draft.set_body_text("Morning: where we are. Afternoon: what we stop doing.\n\n(to finish)");
    draft.set_sender(ME.0, ME.1);
    draft.add_recipient(RECIP_TO, TOMAS.0, TOMAS.1);
    draft.props.set(pid::LAST_MODIFICATION_TIME, PropValue::Time(now.saturating_sub(DAY)));
    draft.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(MSGFLAG_READ | MSGFLAG_UNSENT));
    s.push_item(drafts, draft);

    let old = Mail {
        from: FACILITIES,
        to: &[ME],
        cc: &[],
        subject: "Car park barrier fixed",
        body: "The barrier at the rear car park works again. Thank you for your patience.\n\nFacilities Desk",
        html: None,
        age: 9 * DAY,
        read: true,
    };
    s.push_item(deleted, old.item(now));
    s
}
