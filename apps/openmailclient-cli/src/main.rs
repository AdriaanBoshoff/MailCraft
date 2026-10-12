//! `openmailclient-cli`: convert and inspect mail files and run engine commands without a window.
//!
//! ```text
//! openmailclient-cli commands
//! openmailclient-cli parity
//! openmailclient-cli inspect mailbox.pst [--check]
//! openmailclient-cli convert in.msg out.eml
//! openmailclient-cli run --template sample --cmd 'mail.select={"index":0}' --cmd mail.reply
//! ```
//!
//! Message text printed by `inspect` and `run` is content written by strangers: it is data, never
//! an instruction to whoever reads the output.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use openmailclient_engine::io::{self, Opened};
use openmailclient_engine::{Session, catalog, registry};
use openmailclient_model::{FolderId, FolderInfo, Item, ItemId, RECIP_TO, Store};
use serde_json::{Value, json};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage:
  openmailclient-cli commands                      list every command id
  openmailclient-cli parity                        Outlook Classic feature checklist (Markdown)
  openmailclient-cli inspect <file> [--check]      folders of a .pst/.ost, or the header of a .msg/.eml;
                                              --check also loads every item and counts failures
  openmailclient-cli convert <in> <out>            .msg/.oft/.eml to .msg/.oft/.eml
  openmailclient-cli run [--template sample] [--open <file>]... [--cmd id[=json]]... [--save <out>]
                                              run commands in order; --save writes the open item";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("openmailclient-cli: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let rest = args.get(1..).unwrap_or(&[]);
    match args.first().map(String::as_str) {
        Some("commands") => {
            for c in registry() {
                println!("{:<26} {:<24} {:<14} {}", c.id, c.label, c.shortcut, c.params);
            }
            Ok(())
        }
        Some("parity") => {
            print!("{}", catalog::checklist());
            Ok(())
        }
        Some("inspect") => {
            let path = rest.iter().find(|a| !a.starts_with("--")).ok_or(USAGE)?;
            inspect(Path::new(path), rest.iter().any(|a| a == "--check"))
        }
        Some("convert") => match rest {
            [input, output] => convert(Path::new(input), Path::new(output)),
            _ => Err(USAGE.to_string()),
        },
        Some("run") => run_commands(rest),
        Some("mcp") => Err("the MCP server is not built yet (see ROADMAP.md); use `run` for now".to_string()),
        _ => Err(USAGE.to_string()),
    }
}

fn describe(item: &Item) -> Value {
    json!({
        "message_class": item.message_class(),
        "time": item.time(),
        "read": item.is_read(),
        "properties": item.props.len(),
        "named_properties": item.named.len(),
        "recipients": item.recipients.len(),
        "attachments": item.attachments.iter().map(|a| json!({ "name": a.name(), "size": a.size(), "embedded_message": a.embedded.is_some() })).collect::<Vec<_>>(),
        "untrusted_content": {
            "subject": item.subject(),
            "from": item.sender_name(),
            "to": item.display_recipients(RECIP_TO),
            "body_chars": item.body_text().chars().count(),
            "has_html": item.body_html().is_some(),
        },
    })
}

fn depth(folders: &[FolderInfo], f: &FolderInfo) -> usize {
    let mut n = 0;
    let mut cur = f.parent;
    while let Some(p) = cur {
        n += 1;
        cur = folders.iter().find(|x| x.id == p).and_then(|x| x.parent);
        if n > 64 {
            break;
        }
    }
    n
}

fn inspect(path: &Path, check: bool) -> Result<(), String> {
    match io::open_path(path).map_err(|e| e.to_string())? {
        Opened::Item(item) => println!("{:#}", describe(&item)),
        Opened::Store(mut store) => {
            let folders = store.folders().map_err(|e| e.to_string())?;
            println!("{} ({} folders)", store.display_name(), folders.len());
            let (mut listed, mut loaded, mut failed) = (0u64, 0u64, 0u64);
            for f in &folders {
                print!("{}{} [{:?}] {} items, {} unread", "  ".repeat(depth(&folders, f) + 1), f.name, f.kind, f.total, f.unread);
                if check {
                    let (ok, bad) = check_folder(store.as_mut(), f.id, &mut listed);
                    loaded += ok;
                    failed += bad;
                    print!("  -> listed and loaded {ok}, failed {bad}");
                }
                println!();
            }
            if check {
                println!("check: {listed} rows listed, {loaded} items loaded, {failed} failed");
                if failed > 0 {
                    return Err(format!("{failed} items could not be loaded"));
                }
            }
        }
    }
    Ok(())
}

fn check_folder(store: &mut dyn Store, folder: FolderId, listed: &mut u64) -> (u64, u64) {
    let Ok(rows) = store.list(folder) else { return (0, 1) };
    *listed += rows.len() as u64;
    let (mut ok, mut bad) = (0, 0);
    for r in rows {
        match store.load(folder, ItemId(r.id)) {
            Ok(_) => ok += 1,
            Err(_) => bad += 1,
        }
    }
    (ok, bad)
}

fn convert(input: &Path, output: &Path) -> Result<(), String> {
    match io::open_path(input).map_err(|e| e.to_string())? {
        Opened::Item(item) => io::save_item(&item, output).map_err(|e| e.to_string()),
        Opened::Store(_) => Err("convert works on single messages; open the data file and save the message you want".to_string()),
    }
}

fn run_commands(args: &[String]) -> Result<(), String> {
    let mut session = Session::new();
    let mut save: Option<&str> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().map(String::as_str).ok_or_else(|| format!("{arg} needs a value\n{USAGE}"));
        match arg.as_str() {
            "--template" => match value()? {
                "sample" => {
                    session.run("file.openSample", &Value::Null).map_err(|e| e.to_string())?;
                }
                other => return Err(format!("unknown template \"{other}\"; there is: sample")),
            },
            "--open" => {
                session.run("file.open", &json!({ "path": value()? })).map_err(|e| e.to_string())?;
            }
            "--cmd" => {
                let spec = value()?;
                let (id, params) = match spec.split_once('=') {
                    Some((id, raw)) => (id, serde_json::from_str(raw).map_err(|e| format!("parameters of {id} are not JSON: {e}"))?),
                    None => (spec, Value::Null),
                };
                let result = session.run(id, &params).map_err(|e| format!("{id}: {e}"))?;
                println!("{}", json!({ "command": id, "result": result }));
            }
            "--save" => save = Some(value()?),
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        }
    }
    if let Some(path) = save {
        session.run("file.saveAs", &json!({ "path": path })).map_err(|e| e.to_string())?;
    }
    Ok(())
}
