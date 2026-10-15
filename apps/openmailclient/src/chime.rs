//! The sound new mail is told of with: two soft bell notes made here in code, our own, so no
//! sound file is shipped. Played on a thread of its own, one at a time; a system with no sound
//! device, or none to play it with, stays quiet.

use std::sync::atomic::{AtomicBool, Ordering};

const RATE: u32 = 44_100;
/// Seconds the chime lasts.
const LENGTH: f32 = 0.75;
/// Each note: when it starts (seconds), its pitch (Hz) and how loud it is.
const NOTES: [(f32, f32, f32); 2] = [(0.0, 880.0, 0.22), (0.12, 1_318.5, 0.18)];

static PLAYING: AtomicBool = AtomicBool::new(false);

/// The chime's samples, mono, from -1 to 1.
fn samples() -> Vec<f32> {
    let count = (RATE as f32 * LENGTH) as usize;
    (0..count)
        .map(|n| {
            let t = n as f32 / RATE as f32;
            let mut v = 0.0;
            for (start, pitch, loud) in NOTES {
                let at = t - start;
                if at < 0.0 {
                    continue;
                }
                // A quick rise so it does not click, then a bell's fall; a quieter overtone
                // above the note gives it a bell's colour.
                let rise = (at / 0.006).min(1.0);
                let fall = (-at / 0.2).exp();
                let phase = std::f32::consts::TAU * pitch * at;
                v += loud * rise * fall * (phase.sin() + 0.3 * (2.76 * phase).sin() * (-at / 0.08).exp());
            }
            // The last moments fade to nothing.
            let tail = ((LENGTH - t) / 0.05).clamp(0.0, 1.0);
            (v * tail).clamp(-1.0, 1.0)
        })
        .collect()
}

/// Play the chime unless one is playing already.
pub fn play() {
    if PLAYING.swap(true, Ordering::AcqRel) {
        return;
    }
    let begun = std::thread::Builder::new().name("chime".into()).spawn(|| {
        if let Err(e) = out(samples()) {
            log::info!("the new-mail sound was not played: {e}");
        }
        PLAYING.store(false, Ordering::Release);
    });
    if begun.is_err() {
        PLAYING.store(false, Ordering::Release);
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn out(samples: Vec<f32>) -> Result<(), String> {
    let (_stream, handle) = rodio::OutputStream::try_default().map_err(|e| e.to_string())?;
    let sink = rodio::Sink::try_new(&handle).map_err(|e| e.to_string())?;
    sink.append(rodio::buffer::SamplesBuffer::new(1, RATE, samples));
    sink.sleep_until_end();
    Ok(())
}

/// Elsewhere the chime is written as a WAV file of our own and handed to the sound server's
/// player, or ALSA's.
#[cfg(not(any(windows, target_os = "macos")))]
fn out(samples: Vec<f32>) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("openmailclient-chime-{}.wav", std::process::id()));
    std::fs::write(&path, wav(&samples)).map_err(|e| e.to_string())?;
    let played = ["paplay", "pw-play", "aplay"].iter().any(|player| {
        let mut command = std::process::Command::new(player);
        if *player == "aplay" {
            command.arg("-q");
        }
        command
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    });
    let _ = std::fs::remove_file(&path);
    if played { Ok(()) } else { Err("no sound player was found".into()) }
}

/// 16-bit mono PCM in a RIFF WAVE file.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn wav(samples: &[f32]) -> Vec<u8> {
    let data = u32::try_from(samples.len().saturating_mul(2)).unwrap_or(u32::MAX);
    let mut w = Vec::with_capacity(44 + samples.len() * 2);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&data.saturating_add(36).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&RATE.to_le_bytes());
    w.extend_from_slice(&(RATE * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        w.extend_from_slice(&((s * f32::from(i16::MAX)) as i16).to_le_bytes());
    }
    w
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_chime_is_short_quiet_and_ends_in_silence() {
        let s = super::samples();
        assert_eq!(s.len(), (super::RATE as f32 * super::LENGTH) as usize);
        let loudest = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(loudest > 0.1 && loudest < 0.6, "{loudest}");
        assert!(s.last().is_some_and(|v| v.abs() < 1e-3));
        let w = super::wav(&s);
        assert_eq!(w.len(), 44 + s.len() * 2);
        assert_eq!(&w[..4], b"RIFF");
    }
}
