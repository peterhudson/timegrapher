//! `timegrapher clock`: each sound input's clock error, measured once and
//! kept, so rates read on it can be corrected (see
//! `timegrapher_core::clockstore`).

use crate::output;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use timegrapher_core::audio;
use timegrapher_core::clock::{self, ClockFit};
use timegrapher_core::clockstore::{self, ClockStore, DeviceClock};
use timegrapher_core::recorder::utc;
use timegrapher_core::take;

pub enum Action {
    List,
    Set {
        device: String,
        ppm: f64,
    },
    Forget {
        device: String,
    },
    FromLog {
        log: PathBuf,
        recording: PathBuf,
        device: String,
        save: bool,
    },
    Measure {
        device: Option<String>,
        minutes: f64,
        save: bool,
    },
}

pub fn run(a: Action, json: bool) -> Result<(), String> {
    match a {
        Action::List => list(json),
        Action::Set { device, ppm } => {
            let d = DeviceClock {
                device,
                ppm,
                ppm_sd: None,
                span_s: 0.0,
                measured_utc: utc(SystemTime::now()),
                source: "set".into(),
            };
            store(d, json)
        }
        Action::Forget { device } => {
            let mut s = ClockStore::load()?;
            if !s.remove(&device) {
                return Err(format!("no clock correction stored for '{device}'"));
            }
            let p = s.save()?;
            if json {
                output::print("clock", json!({}), &json!({ "forgot": device, "store": p }))
            } else {
                println!(
                    "Forgot the clock correction for '{device}' ({})",
                    p.display()
                );
                Ok(())
            }
        }
        Action::FromLog {
            log,
            recording,
            device,
            save,
        } => {
            if save {
                ClockStore::load()?;
            }
            let fit = from_log(&log, &recording)?;
            let d = DeviceClock::from_fit(&device, &fit, utc(SystemTime::now()), "log");
            report(
                &d,
                &fit,
                save,
                json,
                json!({ "log": log, "recording": recording }),
            )
        }
        Action::Measure {
            device,
            minutes,
            save,
        } => {
            if !minutes.is_finite() || minutes <= 0.0 {
                return Err("--minutes must be a finite positive number of minutes".into());
            }
            let seconds = minutes * 60.0;
            timegrapher_core::capture::checked_duration(seconds)
                .map_err(|e| format!("--minutes converted to seconds {e}"))?;
            if save {
                ClockStore::load()?;
            }
            measure(device.as_deref(), minutes, save, json)
        }
    }
}

fn list(json: bool) -> Result<(), String> {
    let s = ClockStore::load()?;
    if json {
        return output::print(
            "clock",
            json!({}),
            &json!({ "store": clockstore::path(), "devices": s.devices }),
        );
    }
    match clockstore::path() {
        Some(p) => println!("Store        {}", p.display()),
        None => println!("Store        none (set TIMEGRAPHER_CONFIG_DIR)"),
    }
    if s.devices.is_empty() {
        println!("No clock corrections stored. Measure one with `timegrapher clock measure`.");
    }
    for d in &s.devices {
        println!("{}", describe(d));
    }
    Ok(())
}

/// One line about a stored correction.
pub fn describe(d: &DeviceClock) -> String {
    let sd = d.ppm_sd.map(|s| format!(" ± {s:.2}")).unwrap_or_default();
    let span = if d.span_s > 0.0 {
        format!(" over {:.0} min", d.span_s / 60.0)
    } else {
        String::new()
    };
    format!(
        "{}: {:.2}{sd} ppm {} (rates read {:.2} s/d {} on it); {} {}{span}",
        d.device,
        d.ppm.abs(),
        if d.ppm >= 0.0 { "slow" } else { "fast" },
        d.rate_error_s_per_day().abs(),
        if d.ppm >= 0.0 { "fast" } else { "slow" },
        match d.source.as_str() {
            "measure" => "measured",
            "log" => "from a clock log",
            _ => "set by hand",
        },
        d.measured_utc
    )
}

fn store(d: DeviceClock, json: bool) -> Result<(), String> {
    let mut s = ClockStore::load()?;
    s.set(d.clone());
    let p = s.save()?;
    if json {
        output::print("clock", json!({}), &json!({ "stored": d, "store": p }))
    } else {
        println!("Stored       {}", describe(&d));
        println!("             in {}", p.display());
        Ok(())
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "measures the sound card from its log; opens no take"
)]
fn from_log(log: &Path, recording: &Path) -> Result<ClockFit, String> {
    let info = audio::info(recording).map_err(|e| format!("{}: {e}", recording.display()))?;
    let text = fs::read_to_string(log).map_err(|e| format!("{}: {e}", log.display()))?;
    let pairs = clock::parse_log(&text, info.sample_rate, info.bytes_per_frame)
        .map_err(|e| format!("{}: {e}", log.display()))?;
    ClockFit::new(&pairs).map_err(|e| format!("{}: {e}", log.display()))
}

fn report(
    d: &DeviceClock,
    fit: &ClockFit,
    save: bool,
    json: bool,
    input: Value,
) -> Result<(), String> {
    let path = if save {
        // A correction too loose to correct a take's rates by is not stored
        // either, or it would correct every later take on the card.
        if let Some(w) = fit.unsure() {
            return Err(refusal(d, &w));
        }
        let mut s = ClockStore::load()?;
        s.set(d.clone());
        Some(s.save()?)
    } else {
        None
    };
    if json {
        return output::print(
            "clock",
            input,
            &json!({ "measured": d, "fit": fit, "stored_in": path }),
        );
    }
    println!("Clock        {}", describe(d));
    println!(
        "             {} entries ({} dropped as late), {:.1} ms rms about the fit",
        fit.points, fit.rejected, fit.residual_ms
    );
    match path {
        Some(p) => println!("Stored       in {}", p.display()),
        None => println!("Not stored   (add --save to keep it)"),
    }
    Ok(())
}

/// Why a measurement is not stored: what it measured, and the rule.
fn refusal(d: &DeviceClock, w: &clock::Withheld) -> String {
    format!(
        "not stored: {} entries over {:.0} s measure '{}' at {:+.1} ± {:.1} ppm; a stored correction needs {} entries or more and a standard error of at most {:.0} ppm (as a take's own log does to correct its rates)",
        w.points,
        w.span_s,
        d.device,
        w.ppm,
        w.ppm_sd,
        clock::MIN_POINTS,
        clock::MAX_SD_PPM
    )
}

/// Listen for `minutes`, comparing the audio that arrives against the
/// system clock.
#[cfg(feature = "live")]
fn measure(device: Option<&str>, minutes: f64, save: bool, json: bool) -> Result<(), String> {
    use timegrapher_core::capture::{self, Event};
    let (dev, info) = capture::find(device)?;
    let seconds = minutes * 60.0;
    let (cap, rec, _) = capture::open_for_duration(&dev, &info, None, seconds)?;
    eprintln!(
        "timing '{}' against the system clock for {minutes:.0} min; the system clock must be kept by NTP. Nothing needs to be on the microphone.",
        info.name
    );
    let mut tr = clock::ClockTracker::new(rec.sample_rate);
    let mut next = 60.0;
    while tr.audio_s() < seconds {
        match cap.rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Event::Audio(b)) => tr.push(b.samples.len(), b.at),
            Ok(Event::Error(e)) => return Err(format!("the input failed: {e}")),
            Ok(Event::End) => break,
            Err(_) => return Err("the input stopped delivering audio".into()),
        }
        if tr.audio_s() >= next {
            next += 60.0;
            if let Some(f) = tr.fit() {
                eprintln!(
                    "{:5.1} min  {:+.2} ± {:.2} ppm",
                    tr.audio_s() / 60.0,
                    f.ppm,
                    f.ppm_sd
                );
            }
        }
    }
    drop(cap);
    let fit = tr
        .fit()
        .ok_or("too little audio to measure the clock; listen for longer")?;
    let d = DeviceClock::from_fit(&info.name, &fit, utc(SystemTime::now()), "measure");
    let input =
        json!({ "source": serde_json::to_value(&rec).unwrap_or(Value::Null), "minutes": minutes });
    report(&d, &fit, save, json, input)
}

#[cfg(not(feature = "live"))]
fn measure(_: Option<&str>, _: f64, _: bool, _: bool) -> Result<(), String> {
    Err("this build has no sound input support; use `clock from-log`, or build with --features live".into())
}

/// A take's clock as every command reads it ([`take::clock`]), with where
/// it came from in words.
pub struct AppliedClock {
    pub fit: ClockFit,
    /// The USB lines' or the correction's ppm, when they give the rate.
    pub ppm: Option<f64>,
    pub from: String,
}

/// The clock for a take `total_s` long, read as `info` says: from the clock
/// `log` when there is one, with the recording's `usb` lines setting its
/// rate; else a `correction` known for the input, else the USB lines
/// ([`take::clock_from`]). A log makes a correction unneeded, so
/// `correction` is only asked for without one.
pub fn resolve(
    log: Option<&Path>,
    info: &audio::AudioInfo,
    total_s: f64,
    usb: impl FnOnce() -> Result<Option<(f64, String)>, String>,
    correction: impl FnOnce() -> Result<Option<(f64, String)>, String>,
) -> Result<Option<AppliedClock>, String> {
    let text = match log {
        Some(path) => {
            Some(fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?)
        }
        None => None,
    };
    let correction = match log {
        Some(_) => None,
        None => correction()?,
    };
    let usb = usb()?;
    let fit = take::clock(
        text.as_deref(),
        info.sample_rate,
        info.bytes_per_frame,
        usb.as_ref().map(|(ppm, _)| *ppm),
        correction.as_ref().map(|(ppm, _)| *ppm),
        total_s,
    )
    .map_err(|e| match log {
        Some(path) => format!("{}: {e}", path.display()),
        None => e,
    })?;
    let Some(source) = take::clock_from(
        log.is_some(),
        usb.as_ref().map(|(ppm, _)| *ppm),
        correction.as_ref().map(|(ppm, _)| *ppm),
    ) else {
        return Ok(None);
    };
    let ppm = match source {
        take::ClockFrom::Log => None,
        take::ClockFrom::LogRatedByUsb | take::ClockFrom::Usb => usb.as_ref().map(|(p, _)| *p),
        take::ClockFrom::Correction => correction.as_ref().map(|(p, _)| *p),
    };
    let from = take::clock_source_text(
        source,
        usb.as_ref().map(|(_, f)| f.as_str()),
        correction.as_ref().map(|(_, f)| f.as_str()),
        log,
    );
    Ok(fit.map(|fit| AppliedClock { fit, ppm, from }))
}

/// The correction to apply to a recording: `--card-ppm`, else the stored
/// one for `--device`, else the stored one for the input the recording
/// says it was made on. The text says where it came from.
pub fn correction(
    file: &Path,
    device: Option<&str>,
    card_ppm: Option<f64>,
) -> Result<Option<(f64, String)>, String> {
    if let Some(p) = card_ppm {
        return Ok(Some((p, "given with --card-ppm".into())));
    }
    match device {
        Some(d) => match ClockStore::load()?.get(d) {
            Some(e) => Ok(Some((e.ppm, e.source_text()))),
            None => Err(format!(
                "no clock correction stored for '{d}'; see `timegrapher clock list`"
            )),
        },
        None => Ok(take::stored_correction(file)?.map(|e| (e.ppm, e.source_text()))),
    }
}

/// The sound card's error read from the USB lines in the first of
/// `files`, with its source text; `None` when the recording has none.
pub fn usb(files: &[std::path::PathBuf]) -> Result<Option<(f64, String)>, String> {
    Ok(timegrapher_core::clock::usb_lines(files)?.map(|c| c.correction()))
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "tests build the clock they expect by hand"
)]
mod tests {
    use super::*;

    #[test]
    fn saved_report_merges_latest_store_and_refuses_late_invalid_store() {
        const MARKER: &str = "TIMEGRAPHER_CLOCK_REPORT_TEST_CHILD";
        if std::env::var_os(MARKER).is_none() {
            let root = std::env::temp_dir().join(format!("tg-clock-report-{}", std::process::id()));
            fs::create_dir_all(&root).unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "clock_cmd::tests::saved_report_merges_latest_store_and_refuses_late_invalid_store", "--nocapture"])
                .env(MARKER, "1").env("TIMEGRAPHER_CONFIG_DIR", &root).output().unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("1 passed"),
                "child must execute report test"
            );
            fs::remove_dir_all(root).unwrap();
            return;
        }
        // Model the real preflight -> potentially long measurement -> report sequence.
        ClockStore::load().unwrap();
        let fit = ClockFit::fixed(20.0, 1200.0);
        let measured = DeviceClock::from_fit("measured", &fit, "now".into(), "measure");
        let late = DeviceClock::from_fit(
            "added during measurement",
            &ClockFit::fixed(3.0, 60.0),
            "later".into(),
            "set",
        );
        let mut changed = ClockStore::default();
        changed.set(late.clone());
        let path = changed.save().unwrap();
        report(&measured, &fit, true, true, json!({})).unwrap();
        let saved = ClockStore::load().unwrap();
        assert_eq!(saved.get(&late.device), Some(&late));
        assert_eq!(saved.get(&measured.device), Some(&measured));
        // A store that becomes invalid after preflight must not be overwritten.
        ClockStore::load().unwrap();
        let invalid = r#"{"schema":"timegrapher.clocks/1","devices":[{"device":"late invalid","ppm":-1000000,"span_s":0,"measured_utc":"later","source":"set"}]}"#;
        fs::write(&path, invalid).unwrap();
        assert!(report(&measured, &fit, true, true, json!({}))
            .unwrap_err()
            .contains("ppm"));
        assert_eq!(fs::read_to_string(path).unwrap(), invalid);
    }

    #[test]
    fn a_log_too_short_to_correct_by_is_not_stored() {
        // Six entries over 42 s, as a short take's log: measured, but loose.
        let pairs: Vec<(f64, f64)> = [0.0, 0.029, -0.031, 0.012, 0.004, -0.018]
            .iter()
            .enumerate()
            .map(|(i, j)| (i as f64 * 8.4, 1.7e9 + i as f64 * 8.4 * (1.0 + 6e-6) + j))
            .collect();
        let fit = ClockFit::new(&pairs).unwrap();
        let d = DeviceClock::from_fit("card", &fit, "now".into(), "log");
        let e = report(&d, &fit, true, true, json!({})).unwrap_err();
        assert!(
            e.starts_with("not stored: 6 entries over 42 s measure 'card' at "),
            "{e}"
        );
        assert!(
            e.contains("10 entries or more and a standard error of at most 20 ppm"),
            "{e}"
        );
        // Without --save it is still reported.
        report(&d, &fit, false, true, json!({})).unwrap();
    }
}
