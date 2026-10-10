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
        } => measure(device.as_deref(), minutes, save, json),
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

/// Listen for `minutes`, comparing the audio that arrives against the
/// system clock.
#[cfg(feature = "live")]
fn measure(device: Option<&str>, minutes: f64, save: bool, json: bool) -> Result<(), String> {
    use timegrapher_core::capture::{self, Event};
    let (dev, info) = capture::find(device)?;
    let (cap, rec) = capture::open(&dev, &info, None)?;
    eprintln!(
        "timing '{}' against the system clock for {minutes:.0} min; the system clock must be kept by NTP. Nothing needs to be on the microphone.",
        info.name
    );
    let mut tr = clock::ClockTracker::new(rec.sample_rate);
    let mut next = 60.0;
    while tr.audio_s() < minutes * 60.0 {
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

/// The input a recording was made on, from its provenance sidecar
/// (`take.wav.json`, written by `doctor --save`) or the `session.json` of
/// an app recording folder.
pub fn recorded_device(file: &Path) -> Option<String> {
    let read =
        |p: PathBuf| -> Option<Value> { serde_json::from_str(&fs::read_to_string(p).ok()?).ok() };
    if let Some(v) = read(output::sidecar_path(file)) {
        let name = v.pointer("/recording/source/device/name")?;
        return name.as_str().map(String::from);
    }
    let v = read(file.parent()?.join("session.json"))?;
    v.get("device")?.as_str().map(String::from)
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
    let store = ClockStore::load()?;
    match device {
        Some(d) => match store.get(d) {
            Some(e) => Ok(Some((e.ppm, stored_text(e)))),
            None => Err(format!(
                "no clock correction stored for '{d}'; see `timegrapher clock list`"
            )),
        },
        None => Ok(recorded_device(file)
            .and_then(|d| store.get(&d).cloned())
            .map(|e| (e.ppm, stored_text(&e)))),
    }
}

fn stored_text(e: &DeviceClock) -> String {
    format!("stored for '{}', {}", e.device, e.measured_utc)
}
