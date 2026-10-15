//! New mail told of and Send/Receive on a timer: see `crate::arrival`.

use super::{CommandSpec, always, p_u64};
use crate::arrival::MAX_EVERY_MIN;
use crate::{CmdResult, EngineError, Session};
use serde_json::{Value, json};

fn has_arrived(s: &Session) -> bool {
    !s.arrived.list.is_empty()
}

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "mail.sendReceiveEvery",
            label: "Send/Receive Groups",
            tab: "Send / Receive",
            group: "Send & Receive",
            shortcut: "",
            params: r#"{"minutes": 0 to 1440}; default: 30 as it is at first, or off when on and on when off. Every account is sent and received this often while the window is open, counted from the end of the last Send/Receive (F9 puts the next one off), and a few seconds after the window opens; 0 only when asked. Kept with the settings; returns {"minutes"}"#,
            enabled: always,
            run: every,
        },
        CommandSpec {
            id: "mail.newMailAlerts",
            label: "New Mail Alerts",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"alert": an alert at the corner of the screen, "sound": a short sound of our own, "flash": the taskbar button asks for attention}, each true or false, those not given as they are. Mail already on a server when its account is first received from is not told of. Kept with the settings; returns {"alert", "sound", "flash"}"#,
            enabled: always,
            run: alerts,
        },
        CommandSpec {
            id: "mail.openArrived",
            label: "Open New Message",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"seq": which of the new messages (the newest when not given)}; goes to its folder and selects it, and it is no longer new; returns {"store", "item"}"#,
            enabled: has_arrived,
            run: open_arrived,
        },
        CommandSpec {
            id: "mail.dismissArrived",
            label: "Dismiss New Mail",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"seq": one of the new messages (every one when not given)}; it is no longer told of; returns {"left": how many are}"#,
            enabled: always,
            run: dismiss_arrived,
        },
    ]
}

fn every(s: &mut Session, p: &Value) -> CmdResult {
    s.send_receive_min = match p.get("minutes") {
        None | Some(Value::Null) if s.send_receive_min == 0 => 30,
        None | Some(Value::Null) => 0,
        Some(v) => match v.as_u64() {
            Some(m) if m <= MAX_EVERY_MIN => u32::try_from(m).unwrap_or(30),
            _ => return Err(EngineError::BadParams(format!("\"minutes\" is a whole number from 0 to {MAX_EVERY_MIN}"))),
        },
    };
    s.status = match s.send_receive_min {
        0 => "Mail is sent and received only when asked".to_string(),
        1 => "Mail is sent and received every minute".to_string(),
        m => format!("Mail is sent and received every {m} minutes"),
    };
    Ok(json!({ "minutes": s.send_receive_min }))
}

fn alerts(s: &mut Session, p: &Value) -> CmdResult {
    if !p.is_null() && !p.is_object() {
        return Err(EngineError::BadParams("give {\"alert\", \"sound\", \"flash\"}, each true or false".into()));
    }
    s.alerts = s.alerts.with(p).ok_or_else(|| EngineError::BadParams("\"alert\", \"sound\" and \"flash\" are each true or false".into()))?;
    let on: Vec<&str> = [(s.alerts.alert, "an alert"), (s.alerts.sound, "a sound"), (s.alerts.flash, "the taskbar")]
        .into_iter()
        .filter_map(|(on, what)| on.then_some(what))
        .collect();
    s.status = match on.as_slice() {
        [] => "New mail is not told of".to_string(),
        [one] => format!("New mail is told of with {one}"),
        [a, b] => format!("New mail is told of with {a} and {b}"),
        _ => "New mail is told of with an alert, a sound and the taskbar".to_string(),
    };
    Ok(s.alerts.to_json())
}

/// Which of the new messages `p` names: by "seq", or the newest.
fn named(s: &Session, p: &Value) -> Result<usize, EngineError> {
    match p.get("seq") {
        None | Some(Value::Null) => s.arrived.list.len().checked_sub(1).ok_or_else(|| EngineError::BadParams("there is no new message".into())),
        Some(_) => {
            let seq = p_u64(p, "seq").ok_or_else(|| EngineError::BadParams("\"seq\" is a whole number".into()))?;
            s.arrived
                .list
                .iter()
                .position(|a| a.seq == seq)
                .ok_or_else(|| EngineError::BadParams(format!("there is no new message {seq}; it may have been opened or dismissed")))
        }
    }
}

fn open_arrived(s: &mut Session, p: &Value) -> CmdResult {
    let at = named(s, p)?;
    let Some(a) = s.arrived.list.get(at).cloned() else { return Err(EngineError::BadParams("there is no new message".into())) };
    // Gone from the list whatever comes of it: a message that was moved or deleted meanwhile is
    // not told of again.
    s.arrived.list.remove(at);
    s.select_folder(a.store, a.folder)?;
    if !s.rows.iter().any(|r| r.id == a.item.0) {
        return Err(EngineError::BadParams("that message is no longer in its folder".into()));
    }
    s.select_item(a.item.0)?;
    Ok(json!({ "store": a.store, "item": a.item.0 }))
}

fn dismiss_arrived(s: &mut Session, p: &Value) -> CmdResult {
    match p.get("seq") {
        None | Some(Value::Null) => s.arrived.list.clear(),
        Some(_) => {
            let at = named(s, p)?;
            s.arrived.list.remove(at);
        }
    }
    Ok(json!({ "left": s.arrived.list.len() }))
}
