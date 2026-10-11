//! Repository tasks: `cargo xtask <task>`.
//!
//! ```text
//! cargo xtask ci        fmt, clippy -D warnings, tests, assets, layers, parity, wasm
//! cargo xtask assets    every asset file has a row in ATTRIBUTION.md
//! cargo xtask layers    no crate depends on a layer above it; no GUI crates below the UI
//! cargo xtask parity    write docs/parity-checklist.md from the feature catalog
//! cargo xtask wasm      the engine and everything under it builds for wasm32
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Crate directory → layer. A crate may only depend on crates in a lower layer.
const LAYERS: &[(&str, &str, u8)] = &[
    ("mailcraft-model", "crates/model", 1),
    ("mailcraft-fonts", "crates/fonts", 1),
    ("mailcraft-mime", "crates/mime", 2),
    ("mailcraft-msg", "crates/msg", 2),
    ("mailcraft-pst", "crates/pst", 2),
    ("mailcraft-formats", "crates/formats", 2),
    ("mailcraft-engine", "crates/engine", 4),
    ("mailcraft-ui-egui", "crates/ui-egui", 6),
    ("mailcraft", "apps/mailcraft", 7),
    ("mailcraft-cli", "apps/mailcraft-cli", 7),
];

/// Crates that must never appear below the UI layer.
const GUI_CRATES: &[&str] = &["egui", "eframe", "winit", "rfd", "egui_kittest", "wgpu"];
/// Everything the web build needs must compile for wasm32.
const WASM_CRATES: &[&str] = &["mailcraft-model", "mailcraft-mime", "mailcraft-msg", "mailcraft-pst", "mailcraft-formats", "mailcraft-engine"];
const ASSET_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "ico", "icns", "svg", "ttf", "otf", "woff", "woff2", "wav", "mp3", "ogg", "flac", "cur", "ani",
    "blend", "psd",
];
const SKIP_DIRS: &[&str] = &["target", "plan", ".git", "node_modules"];

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let result = match task.as_str() {
        "ci" => ci(),
        "assets" => assets(),
        "layers" => layers(),
        "parity" => parity(),
        "wasm" => wasm(),
        _ => Err("usage: cargo xtask <ci | assets | layers | parity | wasm>".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {task}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn cargo(args: &[&str]) -> Result<(), String> {
    let program = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    println!("> cargo {}", args.join(" "));
    let status = Command::new(program).args(args).current_dir(root()).status().map_err(|e| format!("cannot run cargo: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("cargo {} failed", args.join(" "))) }
}

fn ci() -> Result<(), String> {
    cargo(&["fmt", "--all", "--", "--check"])?;
    cargo(&["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])?;
    cargo(&["test", "--workspace"])?;
    assets()?;
    layers()?;
    parity()?;
    wasm()
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if depth < 16 && !SKIP_DIRS.contains(&name.as_str()) {
                walk(&path, depth + 1, out);
            }
        } else {
            out.push(path);
        }
    }
}

fn assets() -> Result<(), String> {
    let root = root();
    let table = std::fs::read_to_string(root.join("ATTRIBUTION.md")).map_err(|e| format!("ATTRIBUTION.md: {e}"))?;
    let mut files = Vec::new();
    walk(&root, 0, &mut files);
    let mut missing = Vec::new();
    let mut count = 0;
    for file in files {
        let ext = file.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if !ASSET_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        count += 1;
        let rel = file.strip_prefix(&root).unwrap_or(&file).to_string_lossy().replace('\\', "/");
        if !table.contains(&format!("`{rel}`")) {
            missing.push(rel);
        }
    }
    if missing.is_empty() {
        println!("assets: {count} asset files, all attributed");
        Ok(())
    } else {
        Err(format!("no row in ATTRIBUTION.md for: {}", missing.join(", ")))
    }
}

/// Dependency names in a manifest's `[dependencies]` table (dev-dependencies are test-only).
fn dependencies(manifest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == "[dependencies]";
        } else if inside && let Some(name) = line.split(['=', ' ', '.']).next().filter(|n| !n.is_empty() && !n.starts_with('#')) {
            out.push(name.to_string());
        }
    }
    out
}

fn layers() -> Result<(), String> {
    let root = root();
    let mut problems = Vec::new();
    for (name, dir, layer) in LAYERS {
        let manifest = std::fs::read_to_string(root.join(dir).join("Cargo.toml")).map_err(|e| format!("{dir}/Cargo.toml: {e}"))?;
        for dep in dependencies(&manifest) {
            if let Some((_, _, dep_layer)) = LAYERS.iter().find(|(n, _, _)| *n == dep)
                && dep_layer >= layer
            {
                problems.push(format!("{name} (L{layer}) depends on {dep} (L{dep_layer})"));
            }
            if *layer < 6 && GUI_CRATES.contains(&dep.as_str()) {
                problems.push(format!("{name} (L{layer}) depends on the GUI crate {dep}"));
            }
        }
    }
    if problems.is_empty() {
        println!("layers: {} crates, no upward dependencies", LAYERS.len());
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

fn parity() -> Result<(), String> {
    let path = root().join("docs").join("parity-checklist.md");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, mailcraft_engine::catalog::checklist()).map_err(|e| format!("{}: {e}", path.display()))?;
    let (done, total) = mailcraft_engine::catalog::summary();
    println!("parity: {done} of {total} catalogued features have a command; wrote docs/parity-checklist.md");
    Ok(())
}

fn wasm() -> Result<(), String> {
    let mut args = vec!["check", "--target", "wasm32-unknown-unknown"];
    for name in WASM_CRATES {
        args.extend(["-p", name]);
    }
    cargo(&args)
}
