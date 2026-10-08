//! `timegrapher doctor`: is the microphone set up well enough to measure
//! a watch, and if not, what exactly should change?
//!
//! It records a few seconds (or reads a file), checks the level, clipping,
//! automatic gain, background noise and whether a steady beat is heard,
//! reads the sound card's mixer where it can, and proposes fixes. It
//! changes nothing on the computer unless `--apply` is given.

use crate::mixer::{self, Fix, Mixer};
use crate::output;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use timegrapher_core::audio::{self, Audio};
use timegrapher_core::diagnose::{self, DiagnoseConfig, Severity, SignalCheck};

pub struct Options {
    pub device: Option<String>,
    pub card: Option<String>,
    pub seconds: f64,
    pub file: Option<PathBuf>,
    pub save: Option<PathBuf>,
    pub apply: bool,
    pub json: bool,
    pub bph: Option<u32>,
}

#[derive(Serialize)]
struct Report {
    /// Where the audio came from: a device (with its configuration) or a file.
    source: Value,
    check: SignalCheck,
    /// The sound card's mixer controls (Linux), when they could be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    mixer: Option<Mixer>,
    /// Proposed changes; none are made without `--apply`.
    fixes: Vec<Fix>,
    /// True when `--apply` ran the fixes' commands.
    applied: bool,
    /// The check repeated after applying, when recording live.
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<SignalCheck>,
    /// `ok` when nothing spoils a measurement, else `needs_attention`.
    verdict: &'static str,
}

/// Returns whether the input is fit to measure with.
pub fn run(o: &Options) -> Result<bool, String> {
    let cfg = DiagnoseConfig {
        bph: o.bph,
        ..Default::default()
    };
    let (audio, source) = acquire(o)?;
    let check = diagnose::check(&audio, &cfg);

    let card = o.card.clone().or_else(|| {
        source["device"]["id"]
            .as_str()
            .and_then(mixer::card_from_device)
            .or_else(|| (o.file.is_none()).then(mixer::single_usb_card).flatten())
    });
    let os = std::env::consts::OS;
    let mixer = (os == "linux")
        .then(|| card.as_deref().and_then(mixer::read_alsa))
        .flatten();
    let fixes = if os == "linux" {
        mixer::linux_fixes(&check, mixer.as_ref())
    } else {
        mixer::other_fixes(&check, os)
    };

    if let Some(p) = &o.save {
        save(p, &audio, &source, mixer.as_ref())?;
    }

    let mut applied = false;
    let mut after = None;
    if o.apply && fixes.iter().any(|f| f.command.is_some()) {
        mixer::apply(&fixes)?;
        applied = true;
        if o.file.is_none() {
            let (audio, _) = acquire(o)?;
            after = Some(diagnose::check(&audio, &cfg));
        }
    }
    let ok = after.as_ref().unwrap_or(&check).ok();
    let rep = Report {
        source,
        check,
        mixer,
        fixes,
        applied,
        after,
        verdict: if ok { "ok" } else { "needs_attention" },
    };
    if o.json {
        let files: Vec<PathBuf> = o.file.iter().cloned().collect();
        let settings =
            json!({ "seconds": o.seconds, "bph": o.bph, "device": o.device, "card": o.card });
        output::print("doctor", output::input(&files, settings), &rep)?;
    } else {
        print_report(&rep, o.apply);
    }
    Ok(ok)
}

fn acquire(o: &Options) -> Result<(Audio, Value), String> {
    if let Some(f) = &o.file {
        let a = audio::load(f).map_err(|e| format!("{}: {e}", f.display()))?;
        return Ok((a, json!({ "file": f.display().to_string() })));
    }
    live(o)
}

#[cfg(feature = "live")]
fn live(o: &Options) -> Result<(Audio, Value), String> {
    let (dev, info) = crate::capture::find(o.device.as_deref())?;
    eprintln!(
        "listening to '{}' for {:.0} s; keep the watch clamped against the microphone",
        info.name, o.seconds
    );
    let (a, rec) = crate::capture::record(&dev, info, o.seconds)?;
    let v = serde_json::to_value(&rec).map_err(|e| e.to_string())?;
    Ok((a, v))
}

#[cfg(not(feature = "live"))]
fn live(_: &Options) -> Result<(Audio, Value), String> {
    Err("this build has no sound input support; pass --file, or build with --features live".into())
}

fn save(p: &Path, a: &Audio, source: &Value, mixer: Option<&Mixer>) -> Result<(), String> {
    audio::write_wav(p, a).map_err(|e| format!("{}: {e}", p.display()))?;
    let side = output::sidecar_path(p);
    let doc = json!({
        "schema": format!("timegrapher.recording/{}", output::SCHEMA_VERSION),
        "software": output::software(),
        "recording": { "source": source, "mixer": mixer },
    });
    std::fs::write(
        &side,
        serde_json::to_string_pretty(&doc).unwrap_or_default(),
    )
    .map_err(|e| format!("{}: {e}", side.display()))
}

fn opt(v: Option<f64>, unit: &str) -> String {
    v.map_or("-".into(), |x| format!("{x:.1}{unit}"))
}

fn print_check(c: &SignalCheck) {
    println!(
        "Level        peak {:.1} dBFS, RMS {:.1} dBFS, {} clipped samples",
        c.peak_dbfs, c.rms_dbfs, c.clipped_samples
    );
    println!(
        "Ticks        {} of about {} beats at {} bph; tick {}, background {:.1} dBFS, margin {}",
        c.beats_found,
        c.beats_expected,
        c.bph,
        opt(c.tick_level_dbfs, " dBFS"),
        c.noise_level_dbfs,
        opt(c.tick_to_noise_db, " dB")
    );
    println!(
        "Gain pumping background rises {} between ticks",
        opt(c.gap_rise_db, " dB")
    );
    if let (Some(r), Some(b)) = (c.rate_s_per_day, c.beat_error_ms) {
        println!("Quick look   rate {r:+.1} s/d, beat error {b:.1} ms (sound-card clock; use analyze for a full reading)");
    }
    if c.issues.is_empty() {
        println!("No problems found.");
    }
    for i in &c.issues {
        let tag = match i.severity {
            Severity::Problem => "PROBLEM",
            Severity::Advice => "advice ",
        };
        println!("{tag}      {}", i.message);
    }
}

fn print_report(r: &Report, apply: bool) {
    match r.source.get("file").and_then(Value::as_str) {
        Some(f) => println!("Source       {f}"),
        None => println!(
            "Source       {} ({} Hz, {} channel(s), {})",
            r.source["device"]["name"].as_str().unwrap_or("?"),
            r.source["sample_rate"],
            r.source["channels"],
            r.source["sample_format"].as_str().unwrap_or("?")
        ),
    }
    print_check(&r.check);
    if let Some(m) = &r.mixer {
        let agc = m.agc().map(|c| {
            format!(
                "'{}' {}",
                c.name,
                if c.switch == Some(true) { "on" } else { "off" }
            )
        });
        let lvl = m.capture_level().map(|c| {
            let (cur, _, hi) = c.capture.unwrap_or_default();
            format!(
                "'{}' {cur}/{hi}{}",
                c.name,
                c.capture_db
                    .map_or(String::new(), |d| format!(" ({d:+.1} dB)"))
            )
        });
        println!(
            "Mixer        card {}: {}",
            m.card,
            [lvl, agc]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !r.fixes.is_empty() {
        println!(
            "Suggested changes{}:",
            if r.applied { " (applied)" } else { "" }
        );
        for f in &r.fixes {
            println!("  - {}", f.description);
            if let Some(c) = &f.command {
                println!("      {}", mixer::shell_words(c));
            }
        }
        if !apply && r.fixes.iter().any(|f| f.command.is_some()) {
            println!("Nothing was changed. Run again with --apply to make these changes.");
        }
    }
    if let Some(a) = &r.after {
        println!("After the changes:");
        print_check(a);
    }
}
