//! The printers of this system, and printing on one of them.
//!
//! What is printed is laid out here as the PDF file is, and handed to the system as it takes
//! it best: CUPS (Linux, macOS, the BSDs) is given the PDF file through `lp`; Windows is given
//! the pages drawn here at 300 dots to the inch, through the printing of the .NET Framework
//! that comes with every Windows, started from PowerShell with nothing typed into a shell. A
//! printer is named by what the system lists, and a name that is not listed is refused, so
//! nothing a message holds reaches the system's commands.
//!
//! The host chooses the spooler (`Session::use_this_system`); a session starts with none, and
//! tests use [`Recorder`].

use crate::print::Laid;
use openmailclient_model::Progress;
use std::sync::{Arc, Mutex};

/// The most printers listed, the longest name taken, and the most copies asked for.
pub const MAX_PRINTERS: usize = 200;
pub const MAX_NAME: usize = 260;
pub const MAX_COPIES: u32 = 99;
/// Dots to the inch the pages are drawn at for a system that is given drawn pages.
pub const DOTS: f32 = 300.0;

/// A printer as the system lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Printer {
    pub name: String,
    /// The one the system prints on when none is named.
    pub default: bool,
}

/// What is sent to a printer.
pub struct Sheets {
    pub printer: String,
    /// What the job is called in the printer's queue.
    pub title: String,
    pub copies: u32,
    /// The pages printed, the first is 0, from and to both included.
    pub from: usize,
    pub to: usize,
    /// The pages, laid out, and the PDF file written from them.
    pub laid: Laid,
    pub pdf: Vec<u8>,
}

/// How printers are found and printed on.
pub trait Spooler: Send + Sync {
    /// The printers there are, the default among them.
    fn printers(&self) -> Result<Vec<Printer>, String>;
    /// Print `sheets`; what went wrong, in words, when it did not.
    fn send(&self, sheets: Sheets, progress: &Progress) -> Result<(), String>;
}

/// No printers: what a session has until the host gives it this system's.
pub struct NoPrinters;

impl Spooler for NoPrinters {
    fn printers(&self) -> Result<Vec<Printer>, String> {
        Ok(Vec::new())
    }
    fn send(&self, _: Sheets, _: &Progress) -> Result<(), String> {
        Err("there are no printers here".into())
    }
}

/// What was sent, as a printer that keeps it: for tests.
#[derive(Default)]
pub struct Recorder {
    pub listed: Vec<Printer>,
    /// Printer, title, copies, first and last page, pages laid out, and whether the PDF file
    /// was whole, of each job.
    pub sent: Mutex<Vec<(String, String, u32, usize, usize, usize, bool)>>,
    /// Said by every send, when set: a printer that is out of paper.
    pub fails: Option<String>,
}

impl Spooler for Arc<Recorder> {
    fn printers(&self) -> Result<Vec<Printer>, String> {
        Ok(self.listed.clone())
    }
    fn send(&self, sheets: Sheets, progress: &Progress) -> Result<(), String> {
        if let Some(why) = &self.fails {
            return Err(why.clone());
        }
        progress.begin(1);
        let whole = sheets.pdf.starts_with(b"%PDF") && sheets.pdf.ends_with(b"%%EOF\n");
        if let Ok(mut sent) = self.sent.lock() {
            sent.push((sheets.printer, sheets.title, sheets.copies, sheets.from, sheets.to, sheets.laid.printed.pages.len(), whole));
        }
        progress.step();
        Ok(())
    }
}

/// A name as a printer may have one: no control characters, not empty, not too long.
pub fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= MAX_NAME && !name.chars().any(char::is_control)).then(|| name.to_string())
}

/// This system's printers and its own way to print.
#[cfg(not(target_arch = "wasm32"))]
pub struct System;

#[cfg(not(target_arch = "wasm32"))]
impl Spooler for System {
    fn printers(&self) -> Result<Vec<Printer>, String> {
        let mut all = system::printers()?;
        all.sort_by_key(|p| (!p.default, p.name.to_lowercase()));
        all.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
        all.truncate(MAX_PRINTERS);
        Ok(all)
    }
    fn send(&self, sheets: Sheets, progress: &Progress) -> Result<(), String> {
        system::send(sheets, progress)
    }
}

/// Run `command` to its end, giving `input` on its standard input; what it wrote on its
/// standard error when it failed.
#[cfg(all(any(windows, unix), not(target_arch = "wasm32")))]
fn run(mut command: std::process::Command, input: &[u8]) -> Result<String, String> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("the system's printing could not be started: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A program that does not read what it is given has closed it; that is not a fault.
        let _ = stdin.write_all(input);
    }
    let out = child.wait_with_output().map_err(|e| format!("the system's printing did not end: {e}"))?;
    let said = |bytes: &[u8]| String::from_utf8_lossy(bytes).chars().filter(|c| !c.is_control() || *c == '\n').take(400).collect::<String>();
    if out.status.success() { Ok(said(&out.stdout)) } else { Err(said(&out.stderr).trim().to_string()) }
}

/// A folder of our own for what is handed to the system, cleared away when dropped.
#[cfg(all(any(windows, unix), not(target_arch = "wasm32")))]
struct Spool(std::path::PathBuf);

#[cfg(all(any(windows, unix), not(target_arch = "wasm32")))]
impl Spool {
    fn new() -> Result<Self, String> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("openmailclient-print-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("no room to make the pages in: {e}"))?;
        Ok(Spool(dir))
    }
}

#[cfg(all(any(windows, unix), not(target_arch = "wasm32")))]
impl Drop for Spool {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(windows)]
mod system {
    use super::{DOTS, MAX_PRINTERS, Printer, Sheets, Spool, clean_name, run};
    use openmailclient_model::Progress;

    /// The printers of this machine and the connections to others', from the registry: the
    /// list Windows keeps of them, read and nothing written.
    pub fn printers() -> Result<Vec<Printer>, String> {
        use windows_registry::{CURRENT_USER, LOCAL_MACHINE};
        // "Name,winspool,Ne01:": the default printer is named before the first comma.
        let device =
            CURRENT_USER.open(r"Software\Microsoft\Windows NT\CurrentVersion\Windows").and_then(|k| k.get_string("Device")).unwrap_or_default();
        let default = device.split(',').next().unwrap_or("").trim().to_string();
        let mut names: Vec<String> = Vec::new();
        if let Ok(key) = LOCAL_MACHINE.open(r"SYSTEM\CurrentControlSet\Control\Print\Printers")
            && let Ok(keys) = key.keys()
        {
            names.extend(keys.take(MAX_PRINTERS));
        }
        // A printer shared by another machine is kept as ",,server,printer".
        if let Ok(key) = CURRENT_USER.open(r"Printers\Connections")
            && let Ok(keys) = key.keys()
        {
            names.extend(keys.take(MAX_PRINTERS).map(|k| k.replace(',', "\\")));
        }
        Ok(names.iter().filter_map(|n| clean_name(n)).map(|name| Printer { default: name.eq_ignore_ascii_case(&default), name }).collect())
    }

    /// Draws the pages written beside it on the printer named, on paper of the size asked, the
    /// whole sheet to each page: the printer's own edge, where it cannot print, is where the
    /// page's margin is. Everything it is given comes in through the environment, not the
    /// script's text.
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
trap { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }
Add-Type -AssemblyName System.Drawing
$global:files = @(Get-ChildItem -LiteralPath $env:OMC_PAGES -Filter 'page-*.png' | Sort-Object Name)
if ($global:files.Count -eq 0) { [Console]::Error.WriteLine('There were no pages to print.'); exit 3 }
$doc = New-Object System.Drawing.Printing.PrintDocument
$doc.PrinterSettings.PrinterName = $env:OMC_PRINTER
if (-not $doc.PrinterSettings.IsValid) { [Console]::Error.WriteLine('That printer is not there.'); exit 2 }
$doc.DocumentName = $env:OMC_TITLE
$doc.PrinterSettings.Copies = [int16]$env:OMC_COPIES
$doc.PrinterSettings.Collate = $true
$doc.PrintController = New-Object System.Drawing.Printing.StandardPrintController
if ($env:OMC_PREVIEW -eq '1') { $doc.PrintController = New-Object System.Drawing.Printing.PreviewPrintController }
$w = [int]$env:OMC_WIDTH; $h = [int]$env:OMC_HEIGHT
$size = $null
foreach ($s in $doc.PrinterSettings.PaperSizes) { if ([Math]::Abs($s.Width - $w) -le 4 -and [Math]::Abs($s.Height - $h) -le 4) { $size = $s; break } }
if ($size -eq $null) { $size = New-Object System.Drawing.Printing.PaperSize('OpenMailClient', $w, $h) }
$doc.DefaultPageSettings.PaperSize = $size
$doc.DefaultPageSettings.Landscape = ($env:OMC_LANDSCAPE -eq '1')
$doc.DefaultPageSettings.Margins = New-Object System.Drawing.Printing.Margins(0, 0, 0, 0)
$doc.OriginAtMargins = $false
$global:at = 0
$doc.add_PrintPage({
  param($sender, $e)
  $img = [System.Drawing.Image]::FromFile($global:files[$global:at].FullName)
  try {
    $g = $e.Graphics
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.DrawImage($img, [single](-$e.PageSettings.HardMarginX), [single](-$e.PageSettings.HardMarginY), [single]$e.PageBounds.Width, [single]$e.PageBounds.Height)
  } finally { $img.Dispose() }
  $global:at++
  $e.HasMorePages = $global:at -lt $global:files.Count
})
$doc.Print()
[Console]::Out.WriteLine("printed $($global:at)")
"#;

    pub fn send(sheets: Sheets, progress: &Progress) -> Result<(), String> {
        let spool = Spool::new()?;
        let count = sheets.to.saturating_sub(sheets.from).saturating_add(1);
        progress.begin(count as u64 + 1);
        let mut size = None;
        for (n, page) in (sheets.from..=sheets.to).enumerate() {
            if progress.stopped() {
                return Err("Stopped before it was sent to the printer.".into());
            }
            let image = crate::print::raster(&sheets.laid, page, DOTS / 72.0).ok_or("a page could not be drawn")?;
            if size.is_none()
                && let Some(p) = sheets.laid.printed.pages.get(page)
            {
                size = Some((p.width, p.height));
            }
            let png = crate::print::png(&image).map_err(|e| e.to_string())?;
            std::fs::write(spool.0.join(format!("page-{n:05}.png")), png).map_err(|e| format!("a page could not be kept for the printer: {e}"))?;
            progress.step();
        }
        // Hundredths of an inch, upright: the sheet's own size, and which way it lies.
        let (width, height) = size.unwrap_or((595.28, 841.89));
        let landscape = width > height;
        let (short, long) = (width.min(height), width.max(height));
        let hundredths = |points: f32| (points / 72.0 * 100.0).round().clamp(100.0, 20_000.0) as u32;
        let mut command = std::process::Command::new("powershell.exe");
        command
            // The script whole, as Windows PowerShell takes it: base64 of its UTF-16 text. Read
            // from standard input it would be run a line at a time.
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"])
            .arg(utf16_base64(SCRIPT))
            .env("OMC_PAGES", &spool.0)
            .env("OMC_PRINTER", &sheets.printer)
            .env("OMC_TITLE", sheets.title.chars().filter(|c| !c.is_control()).take(120).collect::<String>())
            .env("OMC_COPIES", sheets.copies.clamp(1, super::MAX_COPIES).to_string())
            .env("OMC_WIDTH", hundredths(short).to_string())
            .env("OMC_HEIGHT", hundredths(long).to_string())
            .env("OMC_LANDSCAPE", if landscape { "1" } else { "0" })
            .env("OMC_PREVIEW", if std::env::var_os("OPENMAILCLIENT_PRINT_PREVIEW").is_some() { "1" } else { "0" });
        {
            use std::os::windows::process::CommandExt;
            // No console window flashes up.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let said = run(command, b"")?;
        // It says how many pages it drew; without that, nothing was printed.
        if !said.lines().any(|line| line.trim().strip_prefix("printed ").is_some_and(|n| n.trim().parse::<usize>().is_ok_and(|n| n > 0))) {
            return Err("Windows printed nothing".into());
        }
        progress.step();
        Ok(())
    }

    /// `text` as UTF-16 (little end first) in base64 (RFC 4648, section 4).
    pub(super) fn utf16_base64(text: &str) -> String {
        const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
            let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
            for at in 0..4 {
                if at <= chunk.len() {
                    out.push(char::from(ABC.get((n >> (18 - 6 * at) & 63) as usize).copied().unwrap_or(b'A')));
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[cfg(test)]
    #[test]
    fn base64_of_utf16_is_as_powershell_takes_it() {
        // "ab" is 61 00 62 00; "é" is E9 00.
        assert_eq!(utf16_base64("ab"), "YQBiAA==");
        assert_eq!(utf16_base64("a"), "YQA=");
        assert_eq!(utf16_base64("é!"), "6QAhAA==");
        assert_eq!(utf16_base64(""), "");
    }
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
mod system {
    use super::{Printer, Sheets, Spool, clean_name, run};
    use openmailclient_model::Progress;

    /// The printers CUPS has, and its default.
    pub fn printers() -> Result<Vec<Printer>, String> {
        let lpstat = |args: &[&str]| {
            let mut command = std::process::Command::new("lpstat");
            command.args(args);
            run(command, b"")
        };
        let listed = lpstat(&["-e"]).or_else(|_| lpstat(&["-a"])).map_err(|_| "CUPS, which prints here, did not answer".to_string())?;
        // "system default destination: name"; none when there is no default.
        let default = lpstat(&["-d"]).ok().and_then(|said| said.rsplit_once(':').map(|(_, name)| name.trim().to_string())).unwrap_or_default();
        Ok(listed
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .filter_map(clean_name)
            .map(|name| Printer { default: name == default, name })
            .collect())
    }

    pub fn send(sheets: Sheets, progress: &Progress) -> Result<(), String> {
        progress.begin(1);
        let spool = Spool::new()?;
        let file = spool.0.join("print.pdf");
        std::fs::write(&file, &sheets.pdf).map_err(|e| format!("the pages could not be kept for the printer: {e}"))?;
        let copies = sheets.copies.clamp(1, super::MAX_COPIES).to_string();
        let title: String = sheets.title.chars().filter(|c| !c.is_control()).take(120).collect();
        let pages = format!("{}-{}", sheets.from.saturating_add(1), sheets.to.saturating_add(1));
        let mut command = std::process::Command::new("lp");
        command.arg("-d").arg(&sheets.printer).arg("-n").arg(&copies).arg("-t").arg(&title).arg("-P").arg(&pages).arg("--").arg(&file);
        run(command, b"")?;
        progress.step();
        Ok(())
    }
}

#[cfg(not(any(windows, unix, target_arch = "wasm32")))]
mod system {
    use super::{Printer, Sheets};
    use openmailclient_model::Progress;

    pub fn printers() -> Result<Vec<Printer>, String> {
        Ok(Vec::new())
    }

    pub fn send(_: Sheets, _: &Progress) -> Result<(), String> {
        Err("printing is not known on this system".into())
    }
}
