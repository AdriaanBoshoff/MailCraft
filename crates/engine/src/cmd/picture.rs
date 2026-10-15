//! Pictures on a server, downloaded for the open message when the reader asks and never by
//! themselves: from the info bar above the message, or a right-click on a picture not shown.

use super::{CommandSpec, has_selection, p_bool, p_str};
use crate::{CmdResult, EngineError, Session, remote};
use openmailclient_model::StoreError;
use openmailclient_net::http;
use serde_json::{Value, json};

/// How a download of pictures is named while it runs.
const DOWNLOADING: &str = "Downloading";

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "mail.downloadPictures",
        label: "Download Pictures",
        tab: "",
        group: "",
        shortcut: "",
        params: r#"{"address": one picture as the message names it (every picture of the open message when not given), "wait": false to fetch on a thread of its own}; fetches the message's pictures over HTTPS only, from servers off this machine and its network; returns {"downloaded", "untrusted_content": {"failed": [{"address", "said"}]}}; what is fetched is kept in memory only"#,
        enabled: has_selection,
        run: download,
    }]
}

/// The pictures the open message names on a server.
fn named(s: &Session) -> Result<Vec<String>, EngineError> {
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    Ok(item.body_html().map(|html| remote::addresses(&openmailclient_formats::html::parse(&html))).unwrap_or_default())
}

fn download(s: &mut Session, p: &Value) -> CmdResult {
    let mut wanted = named(s)?;
    // Only a picture the message has: a caller cannot have any address of its own fetched.
    if let Some(address) = p_str(p, "address") {
        if !wanted.iter().any(|a| a == address) {
            return Err(EngineError::BadParams("the open message has no picture at that address".into()));
        }
        wanted = vec![address.to_string()];
    }
    wanted.retain(|a| !s.downloaded.contains(a));
    if wanted.is_empty() {
        s.status = "There are no pictures to download".into();
        return Ok(json!({ "downloaded": 0, "untrusted_content": { "failed": [] } }));
    }
    // Asked again while they are coming: the download under way brings them.
    if s.jobs.iter().any(|job| job.what.starts_with(DOWNLOADING)) {
        return Ok(json!({ "started": false, "said": "the pictures are being downloaded" }));
    }
    let connector = s.connector.clone().ok_or_else(|| EngineError::Disabled("Downloading pictures (no network)".into()))?;
    let what = if wanted.len() == 1 { format!("{DOWNLOADING} a picture") } else { format!("{DOWNLOADING} {} pictures", wanted.len()) };
    s.begin(what, p_bool(p, "wait") != Some(false), move |progress| {
        progress.begin(wanted.len() as u64);
        let mut got = Vec::new();
        let mut failed = Vec::new();
        for address in wanted {
            if progress.stopped() {
                break;
            }
            match http::get(&*connector, &address, http::MAX_PICTURE) {
                Ok(fetched) if crate::pictures::decode(&fetched.bytes).is_some() => got.push((address, fetched.bytes)),
                Ok(_) => failed.push(json!({ "address": address, "said": "what the server sent is not a picture that is shown" })),
                Err(e) => failed.push(json!({ "address": address, "said": e.to_string() })),
            }
            progress.step();
        }
        Box::new(move |s: &mut Session| {
            let downloaded = got.len();
            for (address, bytes) in got {
                s.downloaded.keep(address, bytes);
            }
            let pictures = |n: usize| if n == 1 { "1 picture".to_string() } else { format!("{n} pictures") };
            let status = match (downloaded, failed.len()) {
                (_, 0) => format!("{} downloaded", pictures(downloaded)),
                (0, n) => format!("{} could not be downloaded", pictures(n)),
                (d, n) => format!("{} downloaded; {} could not be", pictures(d), pictures(n)),
            };
            (status, json!({ "downloaded": downloaded, "untrusted_content": { "failed": failed } }))
        })
    })
}
