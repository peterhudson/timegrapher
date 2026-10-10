//! `timegrapher regress`: the engine checked against tg on stored takes.
//!
//! Every take folder under the given root holds a session file whose
//! readings carry tg's numbers for the same file at the same lift angle
//! (`reference = {...}`). This command measures every reading again and
//! compares it with tg and with a baseline: the same measurements from an
//! earlier engine, written by `--write-baseline`. A value fails when it is
//! further from tg than the baseline was by more than its margin; moving
//! closer to tg always passes. A reading also fails when it loses beats or
//! a value it had. Values with no tg number are reported when they change,
//! without failing. See docs/regress.md.

use crate::session_cmd::{self, Ctx, MANIFEST};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use timegrapher_core::session::{Manifest, Reading, Reference};

/// The layout of the baseline file.
pub const BASELINE_SCHEMA: &str = "timegrapher.regress-baseline/1";
/// Baseline file name, in the root folder unless `--baseline` says otherwise.
pub const BASELINE: &str = "regress-baseline.json";

/// How far a value may move away from tg before the check fails.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Margins {
    pub rate_s_per_day: f64,
    pub amplitude_deg: f64,
    pub beat_error_ms: f64,
    /// Beats a reading may lose, as a share of its baseline beats ...
    pub beats_fraction: f64,
    /// ... but never fewer than this many.
    pub beats_min: usize,
}

impl Default for Margins {
    fn default() -> Self {
        Margins {
            rate_s_per_day: 0.3,
            amplitude_deg: 1.5,
            beat_error_ms: 0.03,
            beats_fraction: 0.0005,
            beats_min: 3,
        }
    }
}

pub struct Options {
    pub root: PathBuf,
    pub baseline: Option<PathBuf>,
    pub write_baseline: bool,
    /// Only the takes whose folder name contains one of these.
    pub takes: Vec<String>,
    pub jobs: Option<usize>,
    pub margins: Margins,
    /// List every value, not only those that moved.
    pub all: bool,
    pub json: bool,
    /// No progress lines and no file names, for public logs.
    pub quiet: bool,
}

/// What one reading measured, as kept in the baseline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Values {
    pub label: String,
    pub rate_s_per_day: Option<f64>,
    pub amplitude_deg: Option<f64>,
    /// Signed, from the unlock (tg's beat error is the size of this).
    pub beat_error_unlock_ms: Option<f64>,
    /// Signed, from the drop; tg has no counterpart.
    pub beat_error_ms: Option<f64>,
    pub beats: usize,
}

impl Values {
    fn of(r: &Reading) -> Values {
        let m = &r.measurement;
        Values {
            label: r.label.clone(),
            rate_s_per_day: m.rate_s_per_day,
            amplitude_deg: m.amplitude_deg,
            beat_error_unlock_ms: m.beat_error_unlock_ms,
            beat_error_ms: m.beat_error_ms,
            beats: m.beats,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub schema: String,
    /// The software that wrote it.
    pub software: serde_json::Value,
    /// Readings of each take, keyed by its session file relative to the root.
    pub takes: BTreeMap<String, Vec<Values>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Within its margin of the baseline's distance from tg (or of the
    /// baseline itself, with no tg number).
    Same,
    /// Closer to tg than the baseline by more than the margin.
    Closer,
    /// Further from tg than the baseline by more than the margin: fails.
    Further,
    /// Moved by more than the margin, with no tg number to judge it.
    Changed,
    /// The baseline had a value and this run has none: fails.
    Lost,
    /// Fewer beats than the baseline by more than the margin: fails.
    BeatsLost,
    /// No baseline value to compare with.
    New,
}

impl Status {
    pub fn fails(self) -> bool {
        matches!(self, Status::Further | Status::Lost | Status::BeatsLost)
    }
}

/// One value of one reading, compared.
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub take: String,
    /// Position of the reading in its session file, from 1.
    pub reading: usize,
    pub label: String,
    /// rate_s_per_day, amplitude_deg, beat_error_unlock_ms, beat_error_ms or beats.
    pub value: &'static str,
    pub tg: Option<f64>,
    pub baseline: Option<f64>,
    pub now: Option<f64>,
    /// Distance from tg now and in the baseline (beat error by size).
    pub off_now: Option<f64>,
    pub off_baseline: Option<f64>,
    pub margin: f64,
    pub status: Status,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Counts {
    pub takes: usize,
    pub readings: usize,
    pub values: usize,
    pub same: usize,
    pub closer: usize,
    pub further: usize,
    pub changed: usize,
    pub lost: usize,
    pub beats_lost: usize,
    pub new: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub passed: bool,
    pub margins: Margins,
    pub counts: Counts,
    /// Takes in the baseline that this run did not read (missing audio or
    /// left out by --take); not a failure.
    pub not_run: Vec<String>,
    /// Every value that is not Same, or every value with --all.
    pub rows: Vec<Row>,
    /// How far the engine is from tg on each take: the largest distance
    /// of any reading, per value.
    pub worst: Vec<Worst>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Worst {
    pub take: String,
    pub readings: usize,
    pub rate_s_per_day: Option<f64>,
    pub amplitude_deg: Option<f64>,
    pub beat_error_ms: Option<f64>,
}

/// Session files under `root`, as paths relative to it ('/' separated).
fn find_sessions(root: &Path) -> Result<Vec<String>, String> {
    fn walk(dir: &Path, root: &Path, depth: usize, out: &mut Vec<String>) -> Result<(), String> {
        if dir.join(MANIFEST).is_file() {
            let rel = dir.strip_prefix(root).unwrap_or(dir);
            let mut parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            parts.push(MANIFEST.to_string());
            out.push(parts.join("/"));
        }
        if depth == 0 {
            return Ok(());
        }
        let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if p.is_dir() && !name.starts_with('.') && name != "session_report" {
                walk(&p, root, depth - 1, out)?;
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, 3, &mut out)?;
    out.sort();
    Ok(out)
}

fn take_name(session: &str) -> String {
    session
        .strip_suffix(MANIFEST)
        .unwrap_or(session)
        .trim_end_matches('/')
        .to_string()
}

/// A take's session file and what each of its readings measured.
type Measured = (Manifest, Vec<Result<Reading, String>>);

/// Measure every reading of every take, `jobs` at a time.
fn measure_all(
    root: &Path,
    sessions: &[String],
    jobs: usize,
    quiet: bool,
) -> Result<Vec<Measured>, String> {
    let opts = session_cmd::Options {
        bph: None,
        lift: None,
        tolerance: None,
        settle: None,
        out: None,
        json: false,
        init: false,
        no_shape: true,
        no_cycles: true,
    };
    let mut loaded = Vec::new();
    for s in sessions {
        let (m, base) = session_cmd::load_manifest(&[root.join(s)])?;
        for e in &m.recordings {
            session_cmd::position_of(e).map_err(|err| format!("{s}: {err}"))?;
        }
        loaded.push((m, base));
    }
    let work: Vec<(usize, usize)> = loaded
        .iter()
        .enumerate()
        .flat_map(|(t, (m, _))| (0..m.recordings.len()).map(move |i| (t, i)))
        .collect();
    let results: Mutex<BTreeMap<(usize, usize), Result<Reading, String>>> =
        Mutex::new(BTreeMap::new());
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1).min(work.len().max(1)) {
            scope.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(&(t, i)) = work.get(k) else { break };
                let (m, base) = &loaded[t];
                let ctx = Ctx {
                    m,
                    base,
                    o: &opts,
                    tty: false,
                    quiet,
                };
                let r = session_cmd::read_one(&m.recordings[i], &ctx);
                results.lock().unwrap().insert((t, i), r);
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    Ok(loaded
        .into_iter()
        .enumerate()
        .map(|(t, (m, _))| {
            let rs = (0..m.recordings.len())
                .map(|i| results.remove(&(t, i)).unwrap())
                .collect();
            (m, rs)
        })
        .collect())
}

/// Compare one value. `size` compares magnitudes (beat error against tg).
fn row(
    head: (&str, usize, &str),
    value: &'static str,
    tg: Option<f64>,
    baseline: Option<f64>,
    now: Option<f64>,
    margin: f64,
    size: bool,
) -> Row {
    let mag = |v: f64| if size { v.abs() } else { v };
    let off = |v: Option<f64>| match (v, tg) {
        (Some(v), Some(t)) => Some((mag(v) - t).abs()),
        _ => None,
    };
    let (off_now, off_baseline) = (off(now), off(baseline));
    let status = match (baseline, now) {
        (None, _) => Status::New,
        (Some(_), None) => Status::Lost,
        (Some(b), Some(n)) => match (off_baseline, off_now) {
            (Some(ob), Some(on)) if on > ob + margin => Status::Further,
            (Some(ob), Some(on)) if on < ob - margin => Status::Closer,
            (Some(_), Some(_)) => Status::Same,
            _ if (n - b).abs() > margin => Status::Changed,
            _ => Status::Same,
        },
    };
    Row {
        take: head.0.to_string(),
        reading: head.1,
        label: head.2.to_string(),
        value,
        tg,
        baseline,
        now,
        off_now,
        off_baseline,
        margin,
        status,
    }
}

/// Every comparison for one reading.
pub fn compare(
    take: &str,
    reading: usize,
    now: Option<&Values>,
    baseline: Option<&Values>,
    tg: Option<&Reference>,
    mg: &Margins,
) -> Vec<Row> {
    let label = now
        .or(baseline)
        .map_or_else(String::new, |v| v.label.clone());
    let head = (take, reading, label.as_str());
    let tg = tg.cloned().unwrap_or_default();
    let get = |v: Option<&Values>, f: fn(&Values) -> Option<f64>| v.and_then(f);
    let mut rows = vec![
        row(
            head,
            "rate_s_per_day",
            tg.rate_s_per_day,
            get(baseline, |v| v.rate_s_per_day),
            get(now, |v| v.rate_s_per_day),
            mg.rate_s_per_day,
            false,
        ),
        row(
            head,
            "amplitude_deg",
            tg.amplitude_deg,
            get(baseline, |v| v.amplitude_deg),
            get(now, |v| v.amplitude_deg),
            mg.amplitude_deg,
            false,
        ),
        row(
            head,
            "beat_error_unlock_ms",
            tg.beat_error_ms,
            get(baseline, |v| v.beat_error_unlock_ms),
            get(now, |v| v.beat_error_unlock_ms),
            mg.beat_error_ms,
            true,
        ),
        row(
            head,
            "beat_error_ms",
            None,
            get(baseline, |v| v.beat_error_ms),
            get(now, |v| v.beat_error_ms),
            mg.beat_error_ms,
            false,
        ),
    ];
    let (b, n) = (baseline.map(|v| v.beats), now.map(|v| v.beats));
    let allowed = b.map_or(0.0, |b| {
        (b as f64 * mg.beats_fraction).max(mg.beats_min as f64)
    });
    rows.push(Row {
        take: take.to_string(),
        reading,
        label: label.clone(),
        value: "beats",
        tg: None,
        baseline: b.map(|b| b as f64),
        now: n.map(|n| n as f64),
        off_now: None,
        off_baseline: None,
        margin: allowed,
        status: match (b, n) {
            (None, _) => Status::New,
            (Some(_), None) => Status::Lost,
            (Some(b), Some(n)) if (b as f64 - n as f64) > allowed => Status::BeatsLost,
            (Some(b), Some(n)) if (n as f64 - b as f64) > allowed => Status::Changed,
            _ => Status::Same,
        },
    });
    rows
}

fn tally(rows: &[Row]) -> Counts {
    let mut c = Counts {
        values: rows.len(),
        ..Default::default()
    };
    for r in rows {
        match r.status {
            Status::Same => c.same += 1,
            Status::Closer => c.closer += 1,
            Status::Further => c.further += 1,
            Status::Changed => c.changed += 1,
            Status::Lost => c.lost += 1,
            Status::BeatsLost => c.beats_lost += 1,
            Status::New => c.new += 1,
        }
    }
    c
}

fn max_of(it: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    it.flatten()
        .fold(None, |m, v| Some(m.map_or(v, |m: f64| m.max(v))))
}

pub fn run(o: &Options) -> Result<bool, String> {
    let root = &o.root;
    if !root.is_dir() {
        return Err(format!("{}: not a folder", root.display()));
    }
    let bpath = o.baseline.clone().unwrap_or_else(|| root.join(BASELINE));
    let old: Option<Baseline> = match fs::read_to_string(&bpath) {
        Ok(text) => {
            let b: Baseline =
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", bpath.display()))?;
            if b.schema != BASELINE_SCHEMA {
                return Err(format!(
                    "{}: baseline layout {} (this version reads {BASELINE_SCHEMA})",
                    bpath.display(),
                    b.schema
                ));
            }
            Some(b)
        }
        Err(_) if o.write_baseline => None,
        Err(e) => {
            return Err(format!(
                "{}: {e} (write one with --write-baseline)",
                bpath.display()
            ))
        }
    };
    let sessions: Vec<String> = find_sessions(root)?
        .into_iter()
        .filter(|s| o.takes.is_empty() || o.takes.iter().any(|t| s.contains(t.as_str())))
        .collect();
    if sessions.is_empty() {
        return Err(format!(
            "{}: no {MANIFEST} found in it or its folders",
            root.display()
        ));
    }
    let jobs = o.jobs.unwrap_or_else(|| {
        std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
    });
    let measured = measure_all(root, &sessions, jobs, o.quiet)?;

    let mut rows = Vec::new();
    let mut worst = Vec::new();
    let mut readings = 0;
    let mut fresh: BTreeMap<String, Vec<Values>> = BTreeMap::new();
    for (s, (m, rs)) in sessions.iter().zip(&measured) {
        let take = take_name(s);
        let before = old.as_ref().and_then(|b| b.takes.get(s));
        let mut values = Vec::new();
        let mut offs = (Vec::new(), Vec::new(), Vec::new());
        for (i, (e, r)) in m.recordings.iter().zip(rs).enumerate() {
            readings += 1;
            let now = match r {
                Ok(r) => Some(Values::of(r)),
                Err(err) => {
                    if o.quiet {
                        eprintln!("{take}: reading {} failed", i + 1);
                    } else {
                        eprintln!("{take}: reading {}: {err}", i + 1);
                    }
                    None
                }
            };
            // A baseline reading counts only if it is the same recording.
            let base = before.and_then(|b| b.get(i)).filter(|b| b.label == e.file);
            let rs = compare(
                &take,
                i + 1,
                now.as_ref(),
                base,
                e.reference.as_ref(),
                &o.margins,
            );
            offs.0.push(rs[0].off_now);
            offs.1.push(rs[1].off_now);
            offs.2.push(rs[2].off_now);
            rows.extend(rs);
            if let Some(v) = now {
                values.push(v);
            }
        }
        worst.push(Worst {
            take: take.clone(),
            readings: m.recordings.len(),
            rate_s_per_day: max_of(offs.0.into_iter()),
            amplitude_deg: max_of(offs.1.into_iter()),
            beat_error_ms: max_of(offs.2.into_iter()),
        });
        if values.len() == m.recordings.len() {
            fresh.insert(s.clone(), values);
        } else if o.write_baseline {
            return Err(format!(
                "{take}: some readings failed, so no baseline was written"
            ));
        }
    }
    let not_run: Vec<String> = old
        .as_ref()
        .map(|b| {
            b.takes
                .keys()
                .filter(|k| !sessions.contains(k))
                .map(|k| take_name(k))
                .collect()
        })
        .unwrap_or_default();
    let mut counts = tally(&rows);
    counts.takes = sessions.len();
    counts.readings = readings;
    let passed = !rows.iter().any(|r| r.status.fails());
    let shown: Vec<Row> = rows
        .into_iter()
        .filter(|r| o.all || r.status != Status::Same)
        .collect();
    let outcome = Outcome {
        passed,
        margins: o.margins,
        counts,
        not_run,
        rows: shown,
        worst,
    };

    if o.write_baseline {
        let mut takes = old.map(|b| b.takes).unwrap_or_default();
        takes.extend(fresh);
        let b = Baseline {
            schema: BASELINE_SCHEMA.to_string(),
            software: crate::output::software(),
            takes,
        };
        let text = serde_json::to_string_pretty(&b).map_err(|e| e.to_string())? + "\n";
        fs::write(&bpath, text).map_err(|e| format!("{}: {e}", bpath.display()))?;
    }

    let mut outcome = outcome;
    if o.quiet {
        for r in &mut outcome.rows {
            r.label.clear();
        }
    }
    if o.json {
        let input = serde_json::json!({
            "root": root.display().to_string(),
            "baseline": bpath.display().to_string(),
            "sessions": sessions,
        });
        crate::output::print("regress", input, &outcome)?;
    } else {
        print_text(&outcome, !o.quiet);
        if o.write_baseline {
            println!("Baseline written to {}", bpath.display());
        }
    }
    // Writing a baseline accepts this run, so it never fails.
    Ok(passed || o.write_baseline)
}

fn num(v: Option<f64>, value: &str) -> String {
    match (v, value) {
        (None, _) => "-".to_string(),
        (Some(v), "rate_s_per_day") => format!("{v:+.2}"),
        (Some(v), "amplitude_deg") => format!("{v:.1}"),
        (Some(v), "beats") => format!("{v:.0}"),
        (Some(v), _) => format!("{v:.3}"),
    }
}

fn name(value: &str) -> &str {
    match value {
        "rate_s_per_day" => "rate s/d",
        "amplitude_deg" => "amplitude °",
        "beat_error_unlock_ms" => "beat error ms",
        "beat_error_ms" => "beat error (drop) ms",
        _ => value,
    }
}

fn verdict(s: Status) -> &'static str {
    match s {
        Status::Same => "same",
        Status::Closer => "closer to tg",
        Status::Further => "FURTHER FROM TG",
        Status::Changed => "changed (no tg value)",
        Status::Lost => "LOST",
        Status::BeatsLost => "LOST BEATS",
        Status::New => "new",
    }
}

fn print_text(o: &Outcome, names: bool) {
    println!("Engine against tg, worst reading of each take (distance from tg):");
    println!(
        "  {:<34} {:>8} {:>10} {:>11} {:>10}",
        "take", "readings", "rate s/d", "amplitude °", "beat ms"
    );
    for w in &o.worst {
        let f = |v: Option<f64>, d: usize| v.map_or("-".to_string(), |v| format!("{v:.d$}"));
        println!(
            "  {:<34} {:>8} {:>10} {:>11} {:>10}",
            w.take,
            w.readings,
            f(w.rate_s_per_day, 2),
            f(w.amplitude_deg, 1),
            f(w.beat_error_ms, 3)
        );
    }
    if !o.rows.is_empty() {
        println!();
        println!("Moved since the baseline:");
    }
    for r in &o.rows {
        let tg = match r.tg {
            Some(_) => format!(", tg {}", num(r.tg, r.value)),
            None => String::new(),
        };
        println!(
            "  {} #{}{}: {} {} (was {}{tg}): {}",
            r.take,
            r.reading,
            if names {
                format!(" {}", r.label)
            } else {
                String::new()
            },
            name(r.value),
            num(r.now, r.value),
            num(r.baseline, r.value),
            verdict(r.status)
        );
    }
    let c = &o.counts;
    println!();
    println!(
        "{} takes, {} readings, {} values: {} further from tg, {} lost, {} lost beats; {} closer, {} changed with no tg value, {} new.",
        c.takes, c.readings, c.values, c.further, c.lost, c.beats_lost, c.closer, c.changed, c.new
    );
    if !o.not_run.is_empty() {
        println!("Not run (in the baseline only): {}", o.not_run.join(", "));
    }
    println!(
        "Margins: rate {} s/d, amplitude {}°, beat error {} ms, beats {}% (at least {}).",
        o.margins.rate_s_per_day,
        o.margins.amplitude_deg,
        o.margins.beat_error_ms,
        o.margins.beats_fraction * 100.0,
        o.margins.beats_min
    );
    println!("{}", if o.passed { "PASS" } else { "FAIL" });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(rate: f64, amp: f64, be: f64, beats: usize) -> Values {
        Values {
            label: "a.flac".into(),
            rate_s_per_day: Some(rate),
            amplitude_deg: Some(amp),
            beat_error_unlock_ms: Some(be),
            beat_error_ms: Some(be),
            beats,
        }
    }

    fn tg(rate: f64, amp: f64, be: f64) -> Reference {
        Reference {
            rate_s_per_day: Some(rate),
            amplitude_deg: Some(amp),
            beat_error_ms: Some(be),
            source: None,
        }
    }

    fn status(rows: &[Row], value: &str) -> Status {
        rows.iter().find(|r| r.value == value).unwrap().status
    }

    #[test]
    fn small_moves_pass() {
        let rows = compare(
            "t",
            1,
            Some(&v(5.1, 250.5, 0.21, 10_000)),
            Some(&v(5.0, 250.0, 0.20, 10_001)),
            Some(&tg(5.0, 250.0, 0.20)),
            &Margins::default(),
        );
        assert!(rows.iter().all(|r| r.status == Status::Same), "{rows:?}");
    }

    #[test]
    fn moving_away_from_tg_fails_and_toward_passes() {
        let mg = Margins::default();
        let r = &tg(5.0, 250.0, 0.20);
        // Amplitude 3° high was 0.5° high: further. Rate 1 s/d off was 2: closer.
        let rows = compare(
            "t",
            1,
            Some(&v(6.0, 253.0, 0.20, 10_000)),
            Some(&v(7.0, 250.5, 0.20, 10_000)),
            Some(r),
            &mg,
        );
        assert_eq!(status(&rows, "amplitude_deg"), Status::Further);
        assert_eq!(status(&rows, "rate_s_per_day"), Status::Closer);
        assert!(rows.iter().any(|r| r.status.fails()));
    }

    #[test]
    fn crossing_tg_is_judged_by_distance() {
        // From 1 s/d fast of tg to 1 s/d slow: same distance, so same.
        let rows = compare(
            "t",
            1,
            Some(&v(4.0, 250.0, 0.2, 100)),
            Some(&v(6.0, 250.0, 0.2, 100)),
            Some(&tg(5.0, 250.0, 0.2)),
            &Margins::default(),
        );
        assert_eq!(status(&rows, "rate_s_per_day"), Status::Same);
    }

    #[test]
    fn beat_error_compared_by_size() {
        // tg's beat error is unsigned; a signed -0.20 matches tg's 0.20.
        let rows = compare(
            "t",
            1,
            Some(&v(5.0, 250.0, -0.20, 100)),
            Some(&v(5.0, 250.0, 0.20, 100)),
            Some(&tg(5.0, 250.0, 0.20)),
            &Margins::default(),
        );
        assert_eq!(status(&rows, "beat_error_unlock_ms"), Status::Same);
        // The drop value has no tg number, so its sign flip is a change.
        assert_eq!(status(&rows, "beat_error_ms"), Status::Changed);
        assert!(!rows.iter().any(|r| r.status.fails()));
    }

    #[test]
    fn lost_beats_and_values_fail() {
        let mut now = v(5.0, 250.0, 0.2, 9_990);
        now.amplitude_deg = None;
        let rows = compare(
            "t",
            1,
            Some(&now),
            Some(&v(5.0, 250.0, 0.2, 10_000)),
            Some(&tg(5.0, 250.0, 0.2)),
            &Margins::default(),
        );
        assert_eq!(status(&rows, "beats"), Status::BeatsLost);
        assert_eq!(status(&rows, "amplitude_deg"), Status::Lost);
        // A reading that failed to analyse loses everything.
        let rows = compare(
            "t",
            1,
            None,
            Some(&v(5.0, 250.0, 0.2, 10_000)),
            None,
            &Margins::default(),
        );
        assert!(rows.iter().all(|r| r.status == Status::Lost));
    }

    #[test]
    fn no_baseline_is_new() {
        let rows = compare(
            "t",
            1,
            Some(&v(5.0, 250.0, 0.2, 100)),
            None,
            None,
            &Margins::default(),
        );
        assert!(rows.iter().all(|r| r.status == Status::New));
    }
}
