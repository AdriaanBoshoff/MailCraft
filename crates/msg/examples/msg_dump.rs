//! Structure of a `.msg`/`.oft`: every storage and stream with its size, the named property streams
//! in hex, and each object's property tags. Values are not printed, so the output is safe to read
//! next to a private file; it is a tool for comparing our writer's files with Outlook's.
//!
//! `cargo run -p mailcraft-msg --example msg_dump -- file.msg [--values]`

use std::io::Read;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: msg_dump <file> [--values]");
        return;
    };
    let values = args.iter().any(|a| a == "--values");
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => return eprintln!("{path}: {e}"),
    };
    let mut cf = match cfb::CompoundFile::open(std::io::Cursor::new(data)) {
        Ok(cf) => cf,
        Err(e) => return eprintln!("{path}: {e}"),
    };
    println!("root clsid {}", cf.root_entry().clsid());
    let entries: Vec<_> = cf.walk().map(|e| (e.path().to_path_buf(), e.is_stream(), e.len())).collect();
    for (p, is_stream, len) in entries {
        let shown = p.to_string_lossy().replace('\\', "/");
        if !is_stream {
            println!("{shown}/");
            continue;
        }
        println!("{shown}  {len}");
        let named = shown.contains("__nameid_version1.0");
        let props = shown.ends_with("__properties_version1.0");
        if !(named || props || values) || len > 4096 {
            continue;
        }
        let mut bytes = Vec::new();
        if cf.open_stream(&p).and_then(|mut s| s.read_to_end(&mut bytes)).is_err() {
            continue;
        }
        if props {
            // Header: 32 bytes at the top, 24 in an attached message, 8 in a recipient or attachment.
            let depth = shown.matches('/').count();
            let header = if depth == 1 {
                32
            } else if shown.contains("__substg1.0_3701000D/__properties") {
                24
            } else {
                8
            };
            println!("    header {}", hex(bytes.get(..header).unwrap_or(&[])));
            for e in bytes.get(header..).unwrap_or(&[]).chunks_exact(16) {
                let tag = u32::from_le_bytes([e[0], e[1], e[2], e[3]]);
                let flags = u32::from_le_bytes([e[4], e[5], e[6], e[7]]);
                let value = if values || tag >> 16 >= 0x8000 { hex(&e[8..16]) } else { String::new() };
                println!("    {tag:08X} flags {flags:X} {value}");
            }
        } else {
            for row in bytes.chunks(16) {
                println!("    {}", hex(row));
            }
        }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}
