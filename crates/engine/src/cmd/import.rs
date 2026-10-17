//! Import from Outlook Classic: the accounts of its profiles with their servers, and the data
//! files that hold their mail, calendar, contacts and tasks, and the profile's other data files.
//!
//! What would be imported is shown first (`file.importOutlook` with no `"import"`), and only
//! what is chosen is done. Nothing of Outlook's is changed: its settings are read, never
//! written, and each data file is copied before it is read, with every other program kept
//! from writing to it while it is copied, so a copy is never half old and half new. An
//! offline file (`.ost`) becomes a `.pst` of our own; a `.pst` is copied as it is. Passwords
//! are not read, and the owner is asked for each.

use super::{CommandSpec, p_bool};
use crate::accounts::{self, Account, MAX_ACCOUNTS};
use crate::outlook::{self, Found, MAX_PROFILES, Profile};
use crate::{CmdResult, EngineError, Session};
use openmailclient_model::Progress;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Ids one import takes at most.
const MAX_CHOSEN: usize = 512;
/// What is copied at a time.
const CHUNK: usize = 1 << 20;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "file.importOutlook",
        label: "Import from Outlook",
        tab: "File",
        group: "Open & Export",
        shortcut: "",
        params: r#"{} shows what Outlook Classic has set up here: {"profiles": [{"name", "default", "accounts": [{"id", "name", "email", "kind", "user", "incoming", "outgoing", "dataFile": {"path", "size", "state": "ready", "inUse" (Outlook has it open: close Outlook first), "missing" or "unreadable"} or null, "here": an account for its address is here already, "notes"}], "dataFiles": [{"id", "name", "path", "size", "state"}], "skipped": [{"name", "why"}]}], "importTo"}; {"import": [ids], "withData": false to bring an account's settings without its data file, "wait": false to copy on a thread of its own} imports them: each account is added without its password (asked for in Account Settings), each data file copied into the program's own folder and opened; returns {"accounts": [{"id", "name", "email", "dataFile"}], "dataFiles": [{"name", "path"}], "failed": [{"name", "why"}]}; nothing of Outlook's is changed"#,
        enabled: |s| s.outlook.is_some(),
        run: import,
    }]
}

/// Whether a data file can be copied now, and how large it is.
fn state(path: &Path) -> (&'static str, Option<u64>) {
    let Ok(meta) = std::fs::metadata(path) else { return ("missing", None) };
    if !meta.is_file() {
        return ("missing", None);
    }
    match open_alone(path) {
        Ok(_) => ("ready", Some(meta.len())),
        Err(e) if in_use(&e) => ("inUse", Some(meta.len())),
        Err(_) => ("unreadable", Some(meta.len())),
    }
}

/// Open a file to read it while no other program may write to it: refused while one has it open
/// to write, as Outlook has its data files while it runs.
fn open_alone(path: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ only: others may read it too, none may write.
        o.share_mode(0x1);
    }
    o.open(path)
}

fn in_use(e: &std::io::Error) -> bool {
    // ERROR_SHARING_VIOLATION and ERROR_LOCK_VIOLATION.
    matches!(e.raw_os_error(), Some(32 | 33))
}

fn server_json(s: &outlook::Server) -> Value {
    json!({ "host": s.host, "port": s.port, "security": s.security })
}

/// The account as `account.add` takes it.
fn account_json(f: &Found) -> Value {
    let mut v = json!({
        "email": f.email,
        "kind": f.kind.name(),
        "name": f.name,
        "displayName": f.display_name,
        "user": f.user,
        "incoming": server_json(&f.incoming),
        "signInToSend": f.sign_in_to_send,
    });
    if let Some(out) = &f.outgoing {
        v["outgoing"] = server_json(out);
    }
    v
}

fn file_json(path: &Path) -> Value {
    let (state, size) = state(path);
    json!({ "path": path.display().to_string(), "size": size, "state": state })
}

fn here(s: &Session, email: &str) -> bool {
    s.accounts.iter().any(|a| a.email.eq_ignore_ascii_case(email))
}

fn preview(s: &Session, profiles: &[Profile]) -> Value {
    let shown: Vec<Value> = profiles
        .iter()
        .map(|p| {
            let accounts: Vec<Value> = p
                .accounts
                .iter()
                .map(|a| {
                    let mut v = account_json(a);
                    v["id"] = json!(a.id);
                    v["dataFile"] = a.data_file.as_deref().map_or(Value::Null, file_json);
                    v["here"] = json!(here(s, &a.email));
                    v["notes"] = json!(a.notes);
                    v
                })
                .collect();
            let files: Vec<Value> = p
                .files
                .iter()
                .map(|f| {
                    let mut v = file_json(&f.path);
                    v["id"] = json!(f.id);
                    v["name"] = json!(f.name);
                    v
                })
                .collect();
            let skipped: Vec<Value> = p.skipped.iter().map(|k| json!({ "name": k.name, "why": k.why })).collect();
            json!({ "name": p.name, "default": p.default, "accounts": accounts, "dataFiles": files, "skipped": skipped })
        })
        .collect();
    json!({ "profiles": shown, "importTo": s.account_dir.as_ref().map(|d| d.display().to_string()) })
}

/// One data file to copy.
struct Copy {
    name: String,
    from: PathBuf,
    dir: PathBuf,
    /// The name it takes, without `.pst`.
    stem: String,
    /// The account it is the data file of, by its place among those imported.
    account: Option<usize>,
}

/// What a name keeps in a file name: no separators, nothing a file system refuses.
fn file_stem(name: &str) -> String {
    let plain: String =
        name.chars().filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).take(100).collect();
    let plain = plain.trim().trim_matches('.').trim().to_string();
    if plain.is_empty() { "Imported".to_string() } else { plain }
}

/// A path in `dir` for `stem` that nothing has yet: "stem.pst", else "stem (2).pst"…
fn free_path(dir: &Path, stem: &str) -> Option<PathBuf> {
    (1..=500u32).map(|n| if n == 1 { dir.join(format!("{stem}.pst")) } else { dir.join(format!("{stem} ({n}).pst")) }).find(|p| !p.exists())
}

fn import(s: &mut Session, p: &Value) -> CmdResult {
    let read = s.outlook.clone().ok_or_else(|| EngineError::BadParams("Outlook's settings are not read in this run".into()))?;
    let profiles: Vec<Profile> = read().iter().take(MAX_PROFILES).enumerate().map(|(n, r)| outlook::read(r, n)).collect();
    let Some(chosen) = p.get("import").filter(|v| !v.is_null()) else { return Ok(preview(s, &profiles)) };
    let ids: Vec<&str> = chosen
        .as_array()
        .filter(|a| a.len() <= MAX_CHOSEN)
        .and_then(|a| a.iter().map(|v| v.as_str().filter(|s| s.len() <= 32)).collect::<Option<Vec<_>>>())
        .ok_or_else(|| EngineError::BadParams(format!("\"import\" is a list of at most {MAX_CHOSEN} ids, as file.importOutlook gives them")))?;
    if ids.is_empty() {
        return Err(EngineError::BadParams("nothing is chosen to import".into()));
    }
    let with_data = p_bool(p, "withData").unwrap_or(true);
    let wait = p_bool(p, "wait") != Some(false);
    let dir = s
        .account_dir
        .clone()
        .ok_or_else(|| EngineError::BadParams("this run keeps no data files of its own, so nothing can be imported into one".into()))?;
    let files_dir = dir.parent().map_or_else(|| dir.join("Imported"), |d| d.join("Imported"));

    let mut adding: Vec<Account> = Vec::new();
    let mut copies: Vec<Copy> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    for id in ids {
        let account = profiles.iter().flat_map(|p| &p.accounts).find(|a| a.id == id);
        let file = profiles.iter().flat_map(|p| &p.files).find(|f| f.id == id);
        match (account, file) {
            (Some(a), _) => {
                // An account here already keeps its settings; its data file may still be brought.
                if here(s, &a.email) || adding.iter().any(|b| b.email.eq_ignore_ascii_case(&a.email)) {
                    if let Some(from) = a.data_file.clone().filter(|_| with_data) {
                        copies.push(Copy { name: a.name.clone(), from, dir: files_dir.clone(), stem: file_stem(&a.name), account: None });
                    } else {
                        failed.push(json!({ "name": a.name, "why": format!("there is an account for {} here already", a.email) }));
                    }
                    continue;
                }
                if s.accounts.len().saturating_add(adding.len()) >= MAX_ACCOUNTS {
                    failed.push(json!({ "name": a.name, "why": format!("no more than {MAX_ACCOUNTS} accounts are kept") }));
                    continue;
                }
                // Checked as anything else from outside is: what makes no sense is refused.
                match Account::from_json(&account_json(a), "importing", None) {
                    Ok(account) => {
                        if let Some(from) = a.data_file.clone().filter(|_| with_data) {
                            let stem = accounts::data_file_name(&account).trim_end_matches(".pst").to_string();
                            copies.push(Copy { name: a.name.clone(), from, dir: dir.clone(), stem, account: Some(adding.len()) });
                        }
                        adding.push(account);
                    }
                    Err(e) => failed.push(json!({ "name": a.name, "why": e.to_string() })),
                }
            }
            (None, Some(f)) => {
                copies.push(Copy { name: f.name.clone(), from: f.path.clone(), dir: files_dir.clone(), stem: file_stem(&f.name), account: None })
            }
            (None, None) => {
                return Err(EngineError::BadParams(format!(
                    "Outlook's settings have nothing called {} now; read them again",
                    id.chars().take(32).collect::<String>()
                )));
            }
        }
    }
    let seed = crate::io::new_store_id(s.now);
    s.begin("Importing from Outlook".to_string(), wait, move |progress| {
        let total: u64 = copies.iter().map(|c| std::fs::metadata(&c.from).map_or(0, |m| m.len()).div_ceil(CHUNK as u64).saturating_add(1)).sum();
        progress.begin(total);
        let done: Vec<(String, Option<usize>, Result<PathBuf, String>)> = copies
            .into_iter()
            .enumerate()
            .map(|(n, c)| {
                let mut seed = seed;
                if let Some(b) = seed.first_mut() {
                    *b ^= u8::try_from(n % 256).unwrap_or(0);
                }
                let got = if progress.stopped() { Err("stopped before it was copied".to_string()) } else { bring(&c, progress, seed) };
                progress.step();
                (c.name, c.account, got)
            })
            .collect();
        Box::new(move |s: &mut Session| finish(s, adding, done, failed))
    })
}

/// Copy one data file into the program's own folder, as a `.pst` it can open: a `.pst` as it
/// is, an `.ost` written anew. Outlook's file is only read.
fn bring(c: &Copy, progress: &Progress, seed: [u8; 16]) -> Result<PathBuf, String> {
    std::fs::create_dir_all(&c.dir).map_err(|e| format!("its folder could not be made: {e}"))?;
    let ext = c.from.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    let nonce: String = seed.iter().take(4).map(|b| format!("{b:02x}")).collect();
    let copy = c.dir.join(format!("{}.importing-{nonce}.{ext}", c.stem));
    let copied = copy_alone(&c.from, &copy, progress);
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&copy);
        return Err(e);
    }
    let made = (|| {
        let Some(target) = free_path(&c.dir, &c.stem) else { return Err("no free name for it was found".to_string()) };
        let opened = crate::io::open_path(&copy).map_err(|e| format!("the copy could not be read: {e}"))?;
        let crate::io::Opened::Store(mut store) = opened else { return Err("it is not a data file".to_string()) };
        if ext == "pst" {
            drop(store);
            std::fs::rename(&copy, &target).map_err(|e| e.to_string())?;
            return Ok(target);
        }
        // An offline file is written anew as a data file of our own, with all it holds.
        let arc = std::sync::Arc::new(Progress::default());
        let (work, tmp) = crate::io::export_begin(store.as_mut(), &target, seed, crate::io::PstKind::Unicode)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "this data file cannot be written anew here".to_string())?;
        let written = work(arc).map_err(|e| e.to_string());
        drop(store);
        written?;
        crate::io::export_end(&tmp, &target).map_err(|e| e.to_string())?;
        Ok(target)
    })();
    let _ = std::fs::remove_file(&copy);
    made
}

/// Copy `from` to `to` (a file not there yet) while no other program may write to `from`.
fn copy_alone(from: &Path, to: &Path, progress: &Progress) -> Result<(), String> {
    use std::io::{Read, Write};
    let mut src = open_alone(from).map_err(|e| {
        if in_use(&e) { "Outlook has it open: close Outlook and import it again".to_string() } else { format!("it could not be read: {e}") }
    })?;
    let mut dst = std::fs::OpenOptions::new().write(true).create_new(true).open(to).map_err(|e| format!("its copy could not be made: {e}"))?;
    let mut buf = vec![0u8; CHUNK];
    loop {
        if progress.stopped() {
            return Err("stopped while it was copied; nothing of it was kept".to_string());
        }
        let n = src.read(&mut buf).map_err(|e| format!("it could not be read: {e}"))?;
        if n == 0 {
            break;
        }
        dst.write_all(buf.get(..n).unwrap_or(&[])).map_err(|e| format!("its copy could not be written: {e}"))?;
        progress.step();
    }
    dst.sync_all().map_err(|e| format!("its copy could not be written: {e}"))
}

/// On the session's thread: the accounts are added, each with its data file, and the other data
/// files opened.
fn finish(
    s: &mut Session,
    adding: Vec<Account>,
    done: Vec<(String, Option<usize>, Result<PathBuf, String>)>,
    mut failed: Vec<Value>,
) -> (String, Value) {
    let mut added: Vec<Value> = Vec::new();
    let mut files: Vec<Value> = Vec::new();
    for (at, mut account) in adding.into_iter().enumerate() {
        let name = account.name.clone();
        // Added meanwhile, from elsewhere.
        if here(s, &account.email) || s.accounts.len() >= MAX_ACCOUNTS {
            failed.push(json!({ "name": name, "why": format!("there is an account for {} here already", account.email) }));
            continue;
        }
        account.id = accounts::new_id(&s.accounts);
        match done.iter().find(|(_, a, _)| *a == Some(at)) {
            Some((_, _, Ok(path))) => account.data_file = Some(path.clone()),
            Some((_, _, Err(why))) => failed.push(json!({ "name": name, "why": format!("its data file was not brought: {why}") })),
            None => {}
        }
        let (id, email) = (account.id.clone(), account.email.clone());
        s.accounts.push(account);
        let opened = super::account::account_store(s, s.accounts.len().saturating_sub(1));
        if let Err(e) = &opened {
            failed.push(json!({ "name": name, "why": format!("its data file could not be opened: {e}") }));
        }
        let data_file = s.accounts.last().and_then(|a| a.data_file.as_ref()).map(|p| p.display().to_string());
        added.push(json!({ "id": id, "name": name, "email": email, "dataFile": data_file }));
    }
    for (name, account, got) in done {
        if account.is_some() {
            continue;
        }
        match got {
            Ok(path) => {
                let shown = path.display().to_string();
                match super::file::open_data_file(s, &shown) {
                    Ok(()) => files.push(json!({ "name": name, "path": shown })),
                    Err(e) => failed.push(json!({ "name": name, "why": format!("it was copied to {shown} but could not be opened: {e}") })),
                }
            }
            Err(why) => failed.push(json!({ "name": name, "why": why })),
        }
    }
    let mut said = match (added.len(), files.len()) {
        (0, 0) => "Nothing was imported from Outlook".to_string(),
        (a, 0) => format!("Imported {a} account{} from Outlook", if a == 1 { "" } else { "s" }),
        (0, f) => format!("Imported {f} data file{} from Outlook", if f == 1 { "" } else { "s" }),
        (a, f) => format!("Imported {a} account{} and {f} data file{} from Outlook", if a == 1 { "" } else { "s" }, if f == 1 { "" } else { "s" }),
    };
    if !added.is_empty() {
        said.push_str("; give each account its password in Account Settings");
    }
    if !failed.is_empty() {
        said.push_str(&format!("; {} could not be", failed.len()));
    }
    (said, json!({ "accounts": added, "dataFiles": files, "failed": failed }))
}
