//! `timegrapher record`: record one position of a test, for as long as
//! asked, into a session folder, and add it to the folder's session file.
//!
//! Each run is one take: a folder made by [`Recorder`] exactly as the
//! desktop app makes it (`audio-001.wav`, `clock.csv`, `session.json`),
//! with the app's `session.toml` inside it, so the app opens it with its
//! settings. The take is then listed in the session folder's own
//! `session.toml` with its position and state of wind, and `timegrapher
//! session FOLDER` reads them all together. The microphone is open only
//! while the command runs, for the time it says it will.

use crate::output;
use serde::Serialize;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use timegrapher_core::analysis::Clipping;
use timegrapher_core::audio;
use timegrapher_core::calibres;
use timegrapher_core::capture::Block;
use timegrapher_core::recorder::{self, Recorder, SessionInfo};
use timegrapher_core::session::{Manifest, Position};
use timegrapher_core::{analyze, AnalysisConfig};

const MANIFEST: &str = "session.toml";

/// Seconds skipped at the start of each take in a session file this
/// command writes: the watch has just been turned to a new position.
const SETTLE_S: f64 = 30.0;

pub struct Options {
    pub folder: PathBuf,
    pub position: String,
    pub seconds: f64,
    pub device: Option<String>,
    pub file: Option<PathBuf>,
    pub watch: Option<String>,
    pub calibre: Option<String>,
    pub bph: Option<u32>,
    pub lift: Option<f64>,
    pub wind_h: f64,
    pub json: bool,
}

/// A quick look at the take, enough to say whether to keep it.
#[derive(Serialize)]
struct Check {
    watch_heard: bool,
    bph: u32,
    beats_found: usize,
    beats_expected: usize,
    rate_s_per_day: Option<f64>,
    amplitude_deg: Option<f64>,
    beat_error_ms: Option<f64>,
    clipping: Clipping,
}

#[derive(Serialize)]
struct Report {
    /// The take's folder.
    take: String,
    /// The session file the take was added to; null when it was not.
    session_file: Option<String>,
    position: &'static str,
    wind_h: f64,
    seconds: f64,
    source: String,
    sample_rate: u32,
    check: Check,
    /// `ok`, or why the take was not added to the session file:
    /// `no_watch_heard` or `clipping`.
    verdict: &'static str,
}

/// The session settings: what the folder's session file says, filled in
/// from the options and the calibre table.
struct Settings {
    watch: Option<String>,
    calibre: Option<String>,
    bph: Option<u32>,
    lift: f64,
    existing: bool,
}

/// Returns whether the take was fit to keep.
pub fn run(o: &Options) -> Result<bool, String> {
    let pos = Position::parse(&o.position).ok_or_else(|| {
        format!(
            "'{}' is not a position; use DU, DD, CD, CL, CU or CR (or CH, CB, 9H, 6H, 3H, 12H)",
            o.position
        )
    })?;
    if !o.seconds.is_finite() || o.seconds <= 0.0 {
        return Err("--seconds must be more than 0".into());
    }
    if !o.wind_h.is_finite() || o.wind_h < 0.0 {
        return Err("--wind-h must be 0 or more".into());
    }
    fs::create_dir_all(&o.folder).map_err(|e| format!("{}: {e}", o.folder.display()))?;
    let set = settings(o)?;

    let started = recorder::utc(SystemTime::now());
    let (rec, source, sample_rate) = capture(o, pos, &set)?;
    let take = rec.dir().to_path_buf();
    let dir = rec
        .finish(None)
        .map_err(|e| format!("{}: {e}", take.display()))?;
    write_take_manifest(&dir, pos, &set)?;

    let check = check(&dir, &set)?;
    let verdict = if !check.watch_heard {
        "no_watch_heard"
    } else if check.clipping.beat_fraction > Clipping::WARN_FRACTION {
        "clipping"
    } else {
        "ok"
    };
    let session_file = if verdict == "ok" {
        Some(add_to_session(o, pos, &set, &dir, &started)?)
    } else {
        None
    };

    let rep = Report {
        take: dir.display().to_string(),
        session_file: session_file.map(|p| p.display().to_string()),
        position: pos.common(),
        wind_h: o.wind_h,
        seconds: o.seconds,
        source,
        sample_rate,
        check,
        verdict,
    };
    if o.json {
        let files: Vec<PathBuf> = o.file.iter().cloned().collect();
        let settings = json!({
            "bph": set.bph, "lift_deg": set.lift, "calibre": set.calibre,
            "device": o.device, "seconds": o.seconds,
        });
        output::print("record", output::input(&files, settings), &rep)?;
    } else {
        print_report(&rep);
    }
    Ok(verdict == "ok")
}

fn settings(o: &Options) -> Result<Settings, String> {
    let file = o.folder.join(MANIFEST);
    let m: Option<Manifest> = if file.is_file() {
        let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        Some(toml::from_str(&text).map_err(|e| format!("Can't read {}: {e}", file.display()))?)
    } else {
        None
    };
    let table = match o.calibre.as_deref() {
        Some(n) => Some(calibres::find(n).ok_or_else(|| {
            let known: Vec<&str> = calibres::all().iter().map(|c| c.calibre.as_str()).collect();
            format!(
                "calibre '{n}' is not in the table; known: {}",
                known.join(", ")
            )
        })?),
        None => None,
    };
    let bph = o.bph.or(table.map(|c| c.bph));
    let lift = o.lift.or(table.and_then(|c| c.lift_angle_deg));
    let calibre = table.map(|c| c.calibre.clone());
    let Some(m) = m else {
        return Ok(Settings {
            watch: o.watch.clone(),
            calibre,
            bph,
            lift: lift.unwrap_or(52.0),
            existing: false,
        });
    };
    // A session's takes must agree on how they are measured.
    let differs = |what: &str, given: String, there: String| {
        format!(
            "{} says {what} {there}, and this take would be {given}; \
             make them agree (edit the file, or record into another folder)",
            file.display()
        )
    };
    if let (Some(a), Some(b)) = (bph, m.bph) {
        if a != b {
            return Err(differs("bph", a.to_string(), b.to_string()));
        }
    }
    if let (Some(a), Some(b)) = (lift, m.lift) {
        if (a - b).abs() > 1e-9 {
            return Err(differs("lift", format!("{a}"), format!("{b}")));
        }
    }
    Ok(Settings {
        watch: m.watch.or(o.watch.clone()),
        calibre: m.calibre.or(calibre),
        bph: m.bph.or(bph),
        lift: m.lift.or(lift).unwrap_or(52.0),
        existing: true,
    })
}

/// Record into a new take folder under the session folder.
fn capture(o: &Options, pos: Position, set: &Settings) -> Result<(Recorder, String, u32), String> {
    let info = |device: String, sample_rate: u32, bits: u16| SessionInfo {
        watch: set.watch.clone().unwrap_or_default(),
        position: pos.common().to_string(),
        lift_deg: set.lift,
        bph: set.bph,
        device,
        sample_rate,
        bits,
        software: format!("timegrapher {}", env!("CARGO_PKG_VERSION")),
    };
    let start = |i: SessionInfo| {
        Recorder::start(&o.folder, i).map_err(|e| format!("{}: {e}", o.folder.display()))
    };
    if let Some(f) = &o.file {
        // A file stands in for the microphone, read at once rather than
        // in real time: for trying the command, and for tests.
        let a = audio::load(f).map_err(|e| format!("{}: {e}", f.display()))?;
        let fs_hz = a.sample_rate;
        let label = f.display().to_string();
        let mut rec = start(info(label.clone(), fs_hz, 16))?;
        let want = ((o.seconds * fs_hz as f64).round() as usize).min(a.samples.len());
        let t0 = SystemTime::now();
        let block = (fs_hz / 50).max(1) as usize;
        let mut n = 0;
        while n < want {
            let m = (want - n).min(block);
            let b = Block {
                samples: a.samples[n..n + m].to_vec(),
                at: t0 + Duration::from_secs_f64((n + m) as f64 / fs_hz as f64),
            };
            rec.write(&b)
                .map_err(|e| format!("{}: {e}", rec.dir().display()))?;
            n += m;
        }
        return Ok((rec, label, fs_hz));
    }
    live(o, pos, info, start)
}

#[cfg(feature = "live")]
fn live(
    o: &Options,
    pos: Position,
    info: impl Fn(String, u32, u16) -> SessionInfo,
    start: impl Fn(SessionInfo) -> Result<Recorder, String>,
) -> Result<(Recorder, String, u32), String> {
    use timegrapher_core::capture::{self, Event};
    let cap = capture::open_input(o.device.as_deref(), None)?;
    let mut rec = start(info(cap.label.clone(), cap.sample_rate, cap.bits))?;
    let fs_hz = cap.sample_rate;
    eprintln!(
        "recording {} ({}) for {:.0} s from '{}' into {}; keep the watch still until it says done",
        pos.common(),
        pos.description(),
        o.seconds,
        cap.label,
        rec.dir().display()
    );
    let want = (o.seconds * fs_hz as f64).round() as usize;
    let mut got = 0usize;
    let mut next_note = 10.0;
    while got < want {
        let b = match cap.rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Event::Audio(b)) => b,
            Ok(Event::Error(e)) => return Err(format!("recording failed: {e}")),
            Ok(Event::End) => break,
            Err(_) => return Err("the input stopped delivering audio".into()),
        };
        let take = (want - got).min(b.samples.len());
        let b = Block {
            samples: b.samples[..take].to_vec(),
            at: b.at,
        };
        rec.write(&b)
            .map_err(|e| format!("{}: {e}", rec.dir().display()))?;
        got += take;
        let s = got as f64 / fs_hz as f64;
        if s >= next_note && got < want {
            eprintln!("  {s:.0} of {:.0} s", o.seconds);
            next_note += 10.0;
        }
    }
    eprintln!("done: the watch can be moved");
    Ok((rec, cap.label.clone(), fs_hz))
}

#[cfg(not(feature = "live"))]
fn live(
    _: &Options,
    _: Position,
    _: impl Fn(String, u32, u16) -> SessionInfo,
    _: impl Fn(SessionInfo) -> Result<Recorder, String>,
) -> Result<(Recorder, String, u32), String> {
    Err("this build has no sound input support; pass --file, or build with --features live".into())
}

fn check(dir: &Path, set: &Settings) -> Result<Check, String> {
    let wav = dir.join("audio-001.wav");
    let a = audio::load(&wav).map_err(|e| format!("{}: {e}", wav.display()))?;
    let mut cfg = AnalysisConfig {
        bph: set.bph,
        ..Default::default()
    };
    cfg.amplitude.lift_deg = set.lift;
    let s = analyze(&a, &cfg).summary;
    let heard = s.watch_heard;
    Ok(Check {
        watch_heard: heard,
        bph: s.bph,
        beats_found: s.beats_found,
        beats_expected: (s.duration_s * s.bph as f64 / 3600.0).round() as usize,
        rate_s_per_day: s.overall.filter(|_| heard).map(|f| f.rate_s_per_day),
        amplitude_deg: s.amplitude_deg.filter(|_| heard),
        beat_error_ms: s.beat_error_unlock_ms.filter(|_| heard),
        clipping: s.clipping,
    })
}

/// A TOML string literal.
fn q(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// The settings lines of a session file.
fn header(set: &Settings) -> String {
    let mut t = String::new();
    if let Some(w) = &set.watch {
        t += &format!("watch = {}\n", q(w));
    }
    if let Some(c) = &set.calibre {
        t += &format!("calibre = {}\n", q(c));
    }
    if let Some(b) = set.bph {
        t += &format!("bph = {b}\n");
    }
    t += &format!("lift = {}\n", toml::Value::Float(set.lift));
    t
}

/// The app's session file inside the take, so the app opens the take with
/// its watch, position and settings, as a take the app saved.
fn write_take_manifest(dir: &Path, pos: Position, set: &Settings) -> Result<(), String> {
    let text = format!(
        "{}\n[[recording]]\nfile = \".\"\nposition = {}\nclock = \"clock.csv\"\n",
        header(set),
        q(pos.common())
    );
    let p = dir.join(MANIFEST);
    fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))
}

/// List the take in the session folder's session file, writing the file
/// first when there is none. The entry is appended, so the rest of the
/// file, comments included, is left as it was.
fn add_to_session(
    o: &Options,
    pos: Position,
    set: &Settings,
    take: &Path,
    started: &str,
) -> Result<PathBuf, String> {
    let file = o.folder.join(MANIFEST);
    let before = if set.existing {
        fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?
    } else {
        format!(
            "# Written by `timegrapher record`; edit freely.\n{}settle_s = {}\n",
            header(set),
            toml::Value::Float(SETTLE_S)
        )
    };
    let name = take
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut text = before.clone();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text += &format!(
        "\n[[recording]]\nfile = {}\nposition = {}\nwind_h = {}\ndate = {}\n",
        q(&name),
        q(pos.common()),
        toml::Value::Float(o.wind_h),
        q(started),
    );
    toml::from_str::<Manifest>(&text)
        .map_err(|e| format!("adding the take would break {}: {e}", file.display()))?;
    fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(file)
}

fn print_report(r: &Report) {
    let c = &r.check;
    println!("Take         {}", r.take);
    println!(
        "Position     {} for {:.0} s, {} h after full wind",
        r.position, r.seconds, r.wind_h
    );
    if !c.watch_heard {
        println!("No watch heard: is it running and clamped against the microphone?");
    } else {
        let f = |v: Option<f64>, d: usize, u: &str| {
            v.map_or("-".to_string(), |x| format!("{x:+.d$} {u}"))
        };
        println!(
            "Quick look   {} beats of {} expected at {} bph; rate {}, amplitude {}, beat error {}",
            c.beats_found,
            c.beats_expected,
            c.bph,
            f(c.rate_s_per_day, 1, "s/d"),
            c.amplitude_deg.map_or("-".into(), |a| format!("{a:.0}°")),
            c.beat_error_ms
                .map_or("-".into(), |b| format!("{:.2} ms", b.abs())),
        );
    }
    match &r.session_file {
        Some(f) => println!("Added to     {f}"),
        None if r.verdict == "clipping" => println!(
            "Not added to the session: {:.0}% of beats clipped; lower the input level (see `doctor`) and record it again.",
            100.0 * c.clipping.beat_fraction
        ),
        None => println!("Not added to the session; record it again once the watch is heard."),
    }
}
