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
use timegrapher_core::live::{LiveAnalyzer, LiveConfig};
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
    /// Keep the latest readings here while recording (`timegrapher.live/1`).
    pub live: Option<PathBuf>,
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
        let mut feed = o
            .live
            .as_ref()
            .map(|p| Feed::new(p, pos, o.seconds, fs_hz, set));
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
            // With a live view, played back as it would have been heard.
            if let Some(f) = feed.as_mut() {
                f.push(&b.samples);
                let ahead =
                    n as f64 / fs_hz as f64 - t0.elapsed().unwrap_or_default().as_secs_f64();
                if ahead > 0.0 {
                    std::thread::sleep(Duration::from_secs_f64(ahead));
                }
            }
        }
        if let Some(f) = feed.as_mut() {
            f.finish();
        }
        return Ok((rec, label, fs_hz));
    }
    live(o, pos, set, info, start)
}

/// Seconds of beats on the live strip, and the strip's half width, ms: the
/// app's defaults.
const STRIP_S: f64 = 30.0;
/// Averaging time of the live readings, seconds: the app's default.
const AVERAGE_S: f64 = 10.0;
/// Most points kept in the live trend; older ones are thinned.
const MAX_TREND: usize = 300;

/// The latest readings while recording, kept in a file for a live view
/// (`timegrapher.live/1`): heard as the app hears it, written about once
/// a second of audio, each time whole, so a reader never sees half a file.
struct Feed {
    path: PathBuf,
    position: &'static str,
    seconds: f64,
    live: LiveAnalyzer,
    /// The strip's anchor, set once from the first beats as the app does.
    anchor: Option<(i64, f64)>,
    /// Time, rate, amplitude and beat error from the unlock, once a second.
    trend: Vec<[Option<f64>; 4]>,
    next_s: f64,
}

impl Feed {
    fn new(path: &Path, pos: Position, seconds: f64, fs_hz: u32, set: &Settings) -> Feed {
        let mut cfg = LiveConfig::default();
        cfg.analysis.bph = set.bph;
        cfg.analysis.amplitude.lift_deg = set.lift;
        let mut f = Feed {
            path: path.to_path_buf(),
            position: pos.common(),
            seconds,
            live: LiveAnalyzer::new(fs_hz, cfg),
            anchor: None,
            trend: Vec::new(),
            next_s: 1.0,
        };
        f.write(false);
        f
    }

    fn push(&mut self, samples: &[f32]) {
        self.live.push(samples);
        if self.live.duration_s() >= self.next_s {
            self.next_s = self.live.duration_s().floor() + 1.0;
            self.write(false);
        }
    }

    fn finish(&mut self) {
        self.write(true);
    }

    fn write(&mut self, done: bool) {
        let r = self.live.reading(AVERAGE_S);
        let t = self.live.duration_s();
        let period = self.live.bph().map(|b| 3600.0 / b as f64);
        let beats = self.live.beats();
        if self.anchor.is_none() && beats.len() >= 16 {
            if let Some(p) = period {
                let last = &beats[15];
                let mid = beats[..16]
                    .iter()
                    .map(|b| b.time - (b.index - last.index) as f64 * p)
                    .sum::<f64>()
                    / 16.0;
                self.anchor = Some((last.index, mid));
            }
        }
        if r.beats_used > 0 {
            self.trend.push([
                Some(t),
                r.rate_s_per_day,
                r.amplitude_deg,
                r.beat_error_unlock_ms,
            ]);
            if self.trend.len() > MAX_TREND {
                // Every other point of the older half goes.
                let half = self.trend.len() / 2;
                let mut k = 0;
                self.trend.retain(|_| {
                    k += 1;
                    k > half || k % 2 == 1
                });
            }
        }
        // Each beat's lead on the anchor, ms (positive: early), as the
        // app's strip draws it; Tick is side 0.
        let strip: Vec<[f64; 3]> = match (self.anchor, period) {
            (Some((index, time)), Some(p)) => beats
                .iter()
                .filter(|b| b.time >= t - STRIP_S)
                .map(|b| {
                    let expected = time + (b.index - index) as f64 * p;
                    [
                        b.time,
                        (expected - b.time) * 1000.0,
                        b.index.rem_euclid(2) as f64,
                    ]
                })
                .collect(),
            _ => Vec::new(),
        };
        let col = |i: usize| -> Vec<Option<f64>> { self.trend.iter().map(|p| p[i]).collect() };
        let doc = json!({
            "schema": "timegrapher.live/1",
            "position": self.position,
            "seconds": self.seconds,
            "recorded_s": t,
            "done": done,
            "average_s": AVERAGE_S,
            "beats_heard": beats.len(),
            "reading": r,
            "strip": { "span_s": STRIP_S, "period_s": period, "beats": strip },
            "trend": { "t_s": col(0), "rate": col(1), "amplitude": col(2), "beat_error": col(3) },
        });
        let tmp = self.path.with_extension("tmp");
        if fs::write(&tmp, doc.to_string()).is_ok() {
            let _ = fs::rename(&tmp, &self.path);
        }
    }
}

#[cfg(feature = "live")]
fn live(
    o: &Options,
    pos: Position,
    set: &Settings,
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
    let mut feed = o
        .live
        .as_ref()
        .map(|p| Feed::new(p, pos, o.seconds, fs_hz, set));
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
        if let Some(f) = feed.as_mut() {
            f.push(&b.samples);
        }
        got += take;
        let s = got as f64 / fs_hz as f64;
        if s >= next_note && got < want {
            eprintln!("  {s:.0} of {:.0} s", o.seconds);
            next_note += 10.0;
        }
    }
    eprintln!("done: the watch can be moved");
    if let Some(f) = feed.as_mut() {
        f.finish();
    }
    Ok((rec, cap.label.clone(), fs_hz))
}

#[cfg(not(feature = "live"))]
fn live(
    _: &Options,
    _: Position,
    _: &Settings,
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
