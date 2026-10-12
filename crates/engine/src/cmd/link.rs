//! Link commands: the links of the open message, and following one. The engine opens nothing
//! itself: it says which address may be opened, and whoever asked opens it.

use super::{CommandSpec, has_selection, p_bool, p_str, p_u64};
use crate::links::{self, Found};
use crate::{CmdResult, EngineError, Session};
use openmailclient_formats::links::{Target, target};
use openmailclient_model::StoreError;
use serde_json::{Value, json};

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "mail.links",
            label: "List Links",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"returns {"count", "untrusted_content": {"links": [{"index", "text", "address", "goes": the place it goes to, "concern": what is wrong with it}]}}; nothing is followed"#,
            enabled: has_selection,
            run: list,
        },
        CommandSpec {
            id: "mail.openLink",
            label: "Open Link",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"index": link number from mail.links} or {"address": as the message writes it}; "confirm": true is needed for a link with a concern. A mailbox starts a new message to it. A web page is not opened by the engine: returns {"untrusted_content": {"open": the checked address, "goes"}} for the asker to open"#,
            enabled: has_selection,
            run: open,
        },
    ]
}

fn told(index: usize, link: &Found) -> Value {
    json!({
        "index": index,
        "text": link.text,
        "address": link.address,
        "goes": link.goes,
        "concern": link.concern.as_ref().map(|c| c.words()),
    })
}

fn list(s: &mut Session, _: &Value) -> CmdResult {
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    let found = links::of(item);
    let listed: Vec<Value> = found.iter().enumerate().map(|(index, link)| told(index, link)).collect();
    Ok(json!({"count": listed.len(), "untrusted_content": {"links": listed}}))
}

fn open(s: &mut Session, p: &Value) -> CmdResult {
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    let found = links::of(item);
    // Only a link the message has: a caller cannot have any address of its own opened.
    let link = match (p_u64(p, "index"), p_str(p, "address")) {
        (Some(index), _) => usize::try_from(index).ok().and_then(|index| found.get(index)),
        (None, Some(address)) => found.iter().find(|link| link.address == address),
        (None, None) => return Err(EngineError::BadParams("\"index\" (number) or \"address\" (text) is required".to_string())),
    }
    .ok_or_else(|| EngineError::BadParams(format!("the message has no such link; it has {}", found.len())))?;
    if let Some(concern) = &link.concern
        && p_bool(p, "confirm") != Some(true)
    {
        return Err(EngineError::BadParams(format!("{} Pass \"confirm\": true to follow it all the same.", concern.words())));
    }
    match target(&link.address).map_err(|why| EngineError::BadParams(why.to_string()))? {
        Target::Mailbox(mailbox) => {
            super::compose::start_to(s, mailbox.clone());
            s.status = format!("New message to {mailbox}");
            Ok(json!({"untrusted_content": {"mailbox": mailbox}}))
        }
        Target::Page(address) => Ok(json!({"untrusted_content": {"open": address, "goes": link.goes}})),
    }
}
