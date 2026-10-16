//! Show Progress: everything running on a thread of its own (sending, receiving, testing an
//! account, saving, exporting), how far each is, what came of those that ended, and a way to
//! stop one or all of them.

use super::{CommandSpec, always};
use crate::{CmdResult, EngineError, Session};
use serde_json::{Value, json};

fn running_any(s: &Session) -> bool {
    !s.saves.is_empty() || !s.exports.is_empty() || !s.jobs.is_empty()
}

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "mail.showProgress",
            label: "Show Progress",
            tab: "Send / Receive",
            group: "Download",
            shortcut: "",
            params: r#"no params; returns {"running": [{"id", "what", "done": 0 to 1, "stopping"}], "finished": [{"job", "said", "ok", "stopped"}] the newest last, "nextSendReceiveInSeconds": when Send/Receive comes on its own next, or null}"#,
            enabled: always,
            run: show,
        },
        CommandSpec {
            id: "mail.cancelTask",
            label: "Cancel Task",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id": as mail.showProgress lists it (every one when not given)}; it stops at the next step it comes to: what it did by then is kept, a data file being saved stays as it was and its changes unsaved, and a file being exported to is not written; returns {"stopping": how many}"#,
            enabled: running_any,
            run: cancel,
        },
    ]
}

fn show(s: &mut Session, _: &Value) -> CmdResult {
    let running: Vec<Value> =
        s.running().into_iter().map(|r| json!({ "id": r.id, "what": r.what, "done": r.done, "stopping": r.stopping })).collect();
    let finished: Vec<Value> = s
        .finished
        .iter()
        .map(|f| json!({ "job": f["job"], "said": f.get("said").cloned().unwrap_or(Value::Null), "ok": f.get("ok").cloned().unwrap_or(Value::Null), "stopped": f.get("stopped").cloned().unwrap_or(Value::Bool(false)) }))
        .collect();
    Ok(json!({
        "running": running,
        "finished": finished,
        "nextSendReceiveInSeconds": s.send_receive_next_in().map(|ms| ms.div_ceil(1000)),
    }))
}

fn cancel(s: &mut Session, p: &Value) -> CmdResult {
    let id = match p.get("id") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) => Some(id.as_str()),
        Some(_) => return Err(EngineError::BadParams("\"id\" is one that mail.showProgress lists, such as \"job-1\"".into())),
    };
    let mut stopping = 0;
    let mut stop = |this: &str, progress: &openmailclient_model::Progress| {
        if id.is_none_or(|id| id == this) {
            progress.stop();
            stopping += 1;
        }
    };
    for saving in &s.saves {
        stop(&format!("save-{}", saving.store), &saving.progress);
    }
    for (i, export) in s.exports.iter().enumerate() {
        stop(&format!("export-{i}"), &export.progress);
    }
    for job in &s.jobs {
        stop(&format!("job-{}", job.id), &job.progress);
    }
    if let Some(id) = id
        && stopping == 0
    {
        return Err(EngineError::BadParams(format!("nothing called {id} is running; it may have ended")));
    }
    s.status = match stopping {
        0 => "Nothing is running".to_string(),
        1 => "Stopping at the next step; what was done is kept".to_string(),
        n => format!("Stopping {n} tasks at their next step; what was done is kept"),
    };
    Ok(json!({ "stopping": stopping }))
}
