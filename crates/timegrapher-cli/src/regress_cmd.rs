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
/// The regression harness's own thresholds, chosen by the project for a
/// developer check that watch owners never see: no corpus take reads a
/// median amplitude above 330° where the unlock was found in at least 60%
/// of its windows, so one that does points at the amplitude measurement.
const AMPLITUDE_HIGH_DEG: f64 = 330.0;
const UNLOCK_FOUND_SHARE: f64 = 0.6;

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
    /// Download the audio of a corpus checkout first.
    pub fetch: bool,
    /// Also run a corpus's smoke takes through the engine.
    pub smoke: bool,
    /// Read only the corpus's holdout takes, judged against tg and their
    /// known faults; prints counts only.
    pub holdout: bool,
    /// With `holdout`: print every reading's numbers too (milestone runs,
    /// never in CI).
    pub reveal: bool,
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
    /// None means historical origin unknown; the document writer is not its origin.
    #[serde(default)]
    pub provenance: Option<crate::regress_provenance::EntryProvenance>,
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
            provenance: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub schema: String,
    /// The document writer; individual entries can have different or unknown origins.
    pub software: serde_json::Value,
    /// Readings of each take, keyed by its session file relative to the root.
    pub takes: BTreeMap<String, Vec<Values>>,
}

/// Selected successful entries replace only their own takes, including origin.
fn updated_baseline(old: Option<Baseline>, fresh: BTreeMap<String, Vec<Values>>) -> Baseline {
    let mut takes = old.map(|b| b.takes).unwrap_or_default();
    takes.extend(fresh);
    Baseline {
        schema: BASELINE_SCHEMA.into(),
        software: crate::output::software(),
        takes,
    }
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
    /// Baseline takes not read, including intentional selection exclusions.
    pub not_run: Vec<String>,
    /// Required inputs and recording identities, checked independently of values.
    #[serde(flatten)]
    pub integrity: Integrity,
    /// Every value that is not Same, or every value with --all.
    pub rows: Vec<Row>,
    /// How far the engine is from tg on each take: the largest distance
    /// of any reading, per value.
    pub worst: Vec<Worst>,
    /// The corpus's smoke takes, with --smoke.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub smoke: Vec<crate::corpus::Smoke>,
    /// Readings whose median amplitude is over [`AMPLITUDE_HIGH_DEG`],
    /// with the unlock found in at least [`UNLOCK_FOUND_SHARE`] of their windows, as
    /// "take #reading". No corpus take should: fails.
    pub amplitude_high: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Worst {
    pub take: String,
    pub readings: usize,
    pub rate_s_per_day: Option<f64>,
    pub amplitude_deg: Option<f64>,
    pub beat_error_ms: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Integrity {
    pub unreadable: Vec<String>,
    pub missing_sessions: Vec<String>,
    pub missing_readings: Vec<String>,
    pub recording_mismatches: Vec<String>,
}

impl Integrity {
    fn incomplete(&self) -> bool {
        !self.unreadable.is_empty()
            || !self.missing_sessions.is_empty()
            || !self.missing_readings.is_empty()
    }

    fn fails(&self) -> bool {
        self.incomplete() || !self.recording_mismatches.is_empty()
    }

    fn check_reading(
        &mut self,
        take: &str,
        reading: usize,
        now: Option<&Values>,
        before: Option<&Values>,
    ) {
        let id = format!("{take} #{reading}");
        match (now, before) {
            (None, _) => self.unreadable.push(id),
            (Some(n), Some(b)) if n.label != b.label => self.recording_mismatches.push(id),
            _ => {}
        }
    }

    fn check_count(&mut self, take: &str, now: usize, before: usize) {
        self.missing_readings
            .extend((now..before).map(|i| format!("{take} #{}", i + 1)));
    }
}

fn selected_session(
    s: &str,
    takes: &[String],
    holdout_ids: &std::collections::BTreeSet<String>,
    holdout: bool,
) -> bool {
    (takes.is_empty() || takes.iter().any(|t| s.contains(t.as_str())))
        && holdout_ids.contains(s.split('/').next().unwrap_or(s)) == holdout
}

fn required_missing_sessions(
    baseline: Option<&Baseline>,
    sessions: &[String],
    takes: &[String],
    holdout_ids: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    baseline
        .map(|b| {
            b.takes
                .keys()
                .filter(|s| selected_session(s, takes, holdout_ids, false) && !sessions.contains(s))
                .map(|s| take_name(s))
                .collect()
        })
        .unwrap_or_default()
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

/// How `regress` reads a session: as its file says, without shape or cycles.
fn session_opts() -> session_cmd::Options {
    session_cmd::Options {
        bph: None,
        lift: None,
        calibre: None,
        settle: None,
        out: None,
        json: false,
        init: false,
        no_shape: true,
        no_cycles: true,
    }
}

/// A take's session file and what each of its readings measured.
struct MeasuredReading {
    reading: Reading,
    provenance: Option<crate::regress_provenance::EntryProvenance>,
}
type Measured = (Manifest, Vec<Result<MeasuredReading, String>>);

struct ProvenanceRun<'a> {
    corpus: Option<(&'a Path, &'a crate::corpus::Corpus)>,
    verified: &'a [crate::corpus::VerifiedFile],
}

/// Measure every reading of every take, `jobs` at a time.
fn measure_all(
    root: &Path,
    sessions: &[String],
    jobs: usize,
    quiet: bool,
    provenance: Option<&ProvenanceRun<'_>>,
) -> Result<Vec<Measured>, String> {
    let opts = session_opts();
    let mut loaded = Vec::new();
    for s in sessions {
        let (m, base, digest) = session_cmd::load_manifest_file(&root.join(s))?;
        for e in &m.recordings {
            session_cmd::position_of(e).map_err(|err| format!("{s}: {err}"))?;
        }
        // Benchmarks must not depend on the machine's user catalogue.
        let calibre = opts.calibre_with(&m, &Default::default())?;
        loaded.push((m, base, digest, calibre));
    }
    let work: Vec<(usize, usize)> = loaded
        .iter()
        .enumerate()
        .flat_map(|(t, (m, _, _, _))| (0..m.recordings.len()).map(move |i| (t, i)))
        .collect();
    let results: Mutex<BTreeMap<(usize, usize), Result<MeasuredReading, String>>> =
        Mutex::new(BTreeMap::new());
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1).min(work.len().max(1)) {
            scope.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(&(t, i)) = work.get(k) else { break };
                let (m, base, digest, calibre) = &loaded[t];
                let ctx = Ctx {
                    m,
                    calibre,
                    base,
                    o: &opts,
                    tty: false,
                    quiet,
                };
                let r = if let Some(p) = provenance {
                    session_cmd::read_one_with_context(&m.recordings[i], &ctx).map(
                        |(reading, context)| MeasuredReading {
                            reading,
                            provenance: Some(context.finish(
                                &sessions[t],
                                digest,
                                i + 1,
                                &m.recordings[i],
                                p.corpus,
                                p.verified,
                            )),
                        },
                    )
                } else {
                    session_cmd::read_one(&m.recordings[i], &ctx).map(|reading| MeasuredReading {
                        reading,
                        provenance: None,
                    })
                };
                results.lock().unwrap().insert((t, i), r);
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    Ok(loaded
        .into_iter()
        .enumerate()
        .map(|(t, (m, _, _, _))| {
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
    if !o.root.is_dir() {
        return Err(format!("{}: not a folder", o.root.display()));
    }
    let bpath = o.baseline.clone().unwrap_or_else(|| o.root.join(BASELINE));
    // A corpus checkout keeps its takes in takes/.
    let corpus = crate::corpus::load(&o.root)?;
    if corpus.is_none() && (o.fetch || o.smoke || o.holdout) {
        return Err(format!(
            "{}: --fetch, --smoke and --holdout need a corpus checkout (no {} in it)",
            o.root.display(),
            crate::corpus::INDEX
        ));
    }
    if o.holdout && (o.write_baseline || o.smoke) {
        return Err("--holdout runs alone: holdout takes are never in the baseline".into());
    }
    if o.reveal && !o.holdout {
        return Err("--reveal goes with --holdout".into());
    }
    let verified = if let (Some(c), true) = (&corpus, o.fetch) {
        crate::corpus::fetch(&o.root, c, &o.takes, o.smoke, o.holdout, o.quiet).map_err(|e| {
            if o.holdout {
                "the audio of a holdout take could not be fetched".to_string()
            } else {
                e
            }
        })?
    } else {
        Vec::new()
    };
    let holdout_ids: std::collections::BTreeSet<String> = corpus
        .as_ref()
        .map(|c| {
            crate::corpus::holdout_ids(c)
                .into_iter()
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let root_buf = match corpus {
        Some(_) => crate::corpus::takes_dir(&o.root),
        None => o.root.clone(),
    };
    let root = &root_buf;
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
        // Holdout takes are judged against tg alone.
        Err(_) if o.holdout => None,
        // Only an absent baseline may be created. Other read errors must not
        // discard required or unselected entries from an existing baseline.
        Err(e) if o.write_baseline && e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            let hint = if e.kind() == std::io::ErrorKind::NotFound {
                " (write one with --write-baseline)"
            } else {
                " (existing baseline could not be read; not replaced)"
            };
            return Err(format!("{}: {e}{hint}", bpath.display()));
        }
    };
    // Holdout takes are read by --holdout and nothing else.
    let sessions: Vec<String> = find_sessions(root)?
        .into_iter()
        .filter(|s| selected_session(s, &o.takes, &holdout_ids, o.holdout))
        .collect();
    if o.holdout {
        return holdout(root, &sessions, o);
    }
    if sessions.is_empty() && !o.smoke {
        return Err(format!(
            "{}: no {MANIFEST} found in it or its folders",
            root.display()
        ));
    }
    let jobs = o.jobs.unwrap_or_else(|| {
        std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
    });
    let provenance = ProvenanceRun {
        corpus: corpus.as_ref().map(|c| (o.root.as_path(), c)),
        verified: &verified,
    };
    let measured = measure_all(
        root,
        &sessions,
        jobs,
        o.quiet,
        o.write_baseline.then_some(&provenance),
    )?;

    let mut rows = Vec::new();
    let mut worst = Vec::new();
    let mut readings = 0;
    let mut fresh: BTreeMap<String, Vec<Values>> = BTreeMap::new();
    let mut amplitude_high = Vec::new();
    let mut integrity = Integrity {
        missing_sessions: required_missing_sessions(
            old.as_ref(),
            &sessions,
            &o.takes,
            &holdout_ids,
        ),
        ..Default::default()
    };
    for (s, (m, rs)) in sessions.iter().zip(&measured) {
        let take = take_name(s);
        let before = old.as_ref().and_then(|b| b.takes.get(s));
        integrity.check_count(&take, m.recordings.len(), before.map_or(0, Vec::len));
        let mut values = Vec::new();
        let mut offs = (Vec::new(), Vec::new(), Vec::new());
        for (i, (e, r)) in m.recordings.iter().zip(rs).enumerate() {
            readings += 1;
            let now = match r {
                Ok(result) => {
                    let r = &result.reading;
                    let m = &r.measurement;
                    let unlock_found = !m.unlock_coverage.is_some_and(|c| c < UNLOCK_FOUND_SHARE);
                    if unlock_found && m.amplitude_deg.is_some_and(|a| a > AMPLITUDE_HIGH_DEG) {
                        amplitude_high.push(format!("{take} #{}", i + 1));
                    }
                    let mut values = Values::of(r);
                    values.provenance = result.provenance.clone();
                    Some(values)
                }
                Err(err) => {
                    if o.quiet {
                        eprintln!("{take}: reading {} failed", i + 1);
                    } else {
                        eprintln!("{take}: reading {}: {err}", i + 1);
                    }
                    None
                }
            };
            let base = before.and_then(|b| b.get(i));
            integrity.check_reading(&take, i + 1, now.as_ref(), base);
            // Labels include explicit intervals rounded to seconds, not a full
            // input identity. Changed labels must not count as acceptable New.
            let base = base.filter(|b| now.as_ref().is_none_or(|n| n.label == b.label));
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
    let smoke = match (&corpus, o.smoke) {
        (Some(c), true) => crate::corpus::smoke(&o.root, c, &o.takes),
        _ => Vec::new(),
    };
    let mut counts = tally(&rows);
    counts.takes = sessions.len();
    counts.readings = readings;
    let passed = !integrity.fails()
        && !rows.iter().any(|r| r.status.fails())
        && !smoke.iter().any(|s| s.fails())
        && amplitude_high.is_empty();
    let shown: Vec<Row> = rows
        .into_iter()
        .filter(|r| o.all || r.status != Status::Same)
        .collect();
    let outcome = Outcome {
        passed,
        margins: o.margins,
        counts,
        not_run,
        integrity,
        rows: shown,
        worst,
        smoke,
        amplitude_high,
    };

    if o.write_baseline {
        if outcome.integrity.incomplete() {
            return Err("required sessions or readings are missing or unreadable, so no baseline was written".into());
        }
        let b = updated_baseline(old, fresh);
        let text = serde_json::to_string_pretty(&b).map_err(|e| e.to_string())? + "\n";
        fs::write(&bpath, text).map_err(|e| format!("{}: {e}", bpath.display()))?;
    }

    let mut outcome = outcome;
    if o.quiet {
        for r in &mut outcome.rows {
            r.label.clear();
        }
        for r in &mut outcome.smoke {
            r.file.clear();
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

/// How far a holdout reading may be from tg, against tg alone (research's
/// margins, docs/regress.md). Amplitude is 8° because the engine and tg
/// place the unlock differently on some movements: on the referenced takes
/// the engine sits within 8° of tg on 93% of readings, offset by movement
/// and mount, so a reading further out shows something new.
pub const HOLDOUT_MARGINS: Margins = Margins {
    rate_s_per_day: 0.5,
    amplitude_deg: 8.0,
    beat_error_ms: 0.2,
    beats_fraction: 0.0,
    beats_min: 0,
};

/// One holdout value against tg: whether it is within the margin, or not
/// judged (no tg number, or nothing measured).
#[derive(Debug, Clone, Serialize)]
pub struct Judged {
    pub tg: Option<f64>,
    pub now: Option<f64>,
    pub off: Option<f64>,
    pub pass: Option<bool>,
}

fn judge(tg: Option<f64>, now: Option<f64>, margin: f64, size: bool) -> Judged {
    let off = tg
        .zip(now)
        .map(|(t, n)| ((if size { n.abs() } else { n }) - t).abs());
    Judged {
        tg,
        now,
        off,
        pass: off.map(|d| d <= margin),
    }
}

/// One holdout reading, judged. Kept from the printout unless --reveal.
#[derive(Debug, Clone, Serialize)]
pub struct HoldoutReading {
    pub take: String,
    pub reading: usize,
    pub label: String,
    pub rate_s_per_day: Judged,
    pub amplitude_deg: Judged,
    pub beat_error_ms: Judged,
    /// The fault the session file names (`id` or `id:side`); none for a
    /// healthy watch.
    pub fault: Option<String>,
}

/// Pass, fail and not judged, for one value over every holdout reading.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Tally {
    pub pass: usize,
    pub fail: usize,
    pub not_judged: usize,
}

impl Tally {
    fn of<'a>(js: impl Iterator<Item = &'a Judged>) -> Tally {
        let mut t = Tally::default();
        for j in js {
            match j.pass {
                Some(true) => t.pass += 1,
                Some(false) => t.fail += 1,
                None => t.not_judged += 1,
            }
        }
        t
    }
}

/// What `regress --holdout` reports. `readings` stays empty unless --reveal.
#[derive(Debug, Clone, Serialize)]
pub struct HoldoutOutcome {
    pub takes: usize,
    pub readings_measured: usize,
    /// Readings that could not be measured: the only failure.
    pub unreadable: usize,
    pub margins: Margins,
    pub rate_s_per_day: Tally,
    pub amplitude_deg: Tally,
    pub beat_error_ms: Tally,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub readings: Vec<HoldoutReading>,
}

/// `regress --holdout`: measure the holdout takes, judge them against tg
/// with [`HOLDOUT_MARGINS`], and print the counts. Exits 0 whatever the
/// result; only an unreadable reading fails.
fn holdout(root: &Path, sessions: &[String], o: &Options) -> Result<bool, String> {
    let jobs = o.jobs.unwrap_or_else(|| {
        std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
    });
    let measured = measure_all(root, sessions, jobs, true, None)?;
    let mg = HOLDOUT_MARGINS;
    let mut readings = Vec::new();
    let mut unreadable = 0;
    for (s, (m, rs)) in sessions.iter().zip(&measured) {
        for (i, (e, r)) in m.recordings.iter().zip(rs).enumerate() {
            let Ok(r) = r else {
                unreadable += 1;
                continue;
            };
            let r = &r.reading;
            let me = &r.measurement;
            let tg = e.reference.clone().unwrap_or_default();
            let named = e.fault.as_ref();
            readings.push(HoldoutReading {
                take: take_name(s),
                reading: i + 1,
                label: r.label.clone(),
                rate_s_per_day: judge(
                    tg.rate_s_per_day,
                    me.rate_s_per_day,
                    mg.rate_s_per_day,
                    false,
                ),
                amplitude_deg: judge(tg.amplitude_deg, me.amplitude_deg, mg.amplitude_deg, false),
                beat_error_ms: judge(
                    tg.beat_error_ms,
                    me.beat_error_unlock_ms,
                    mg.beat_error_ms,
                    true,
                ),
                fault: named.map(|k| match &k.side {
                    Some(side) => format!("{}:{side}", k.id),
                    None => k.id.clone(),
                }),
            });
        }
    }
    let outcome = HoldoutOutcome {
        takes: sessions.len(),
        readings_measured: readings.len(),
        unreadable,
        margins: mg,
        rate_s_per_day: Tally::of(readings.iter().map(|r| &r.rate_s_per_day)),
        amplitude_deg: Tally::of(readings.iter().map(|r| &r.amplitude_deg)),
        beat_error_ms: Tally::of(readings.iter().map(|r| &r.beat_error_ms)),
        readings: if o.reveal { readings } else { Vec::new() },
    };
    if o.json {
        crate::output::print("regress-holdout", serde_json::json!({}), &outcome)?;
    } else {
        print_holdout(&outcome);
    }
    Ok(unreadable == 0)
}

fn print_holdout(o: &HoldoutOutcome) {
    if o.takes == 0 {
        println!("No holdout takes in this corpus.");
        return;
    }
    if !o.readings.is_empty() {
        println!(
            "Holdout readings (revealed: copy into each take's README with the git hash and date):"
        );
        let f = |j: &Judged, d: usize| match (j.now, j.tg) {
            (Some(n), Some(t)) => format!(
                "{n:.d$} (tg {t:.d$}){}",
                if j.pass == Some(false) { " FAIL" } else { "" }
            ),
            (Some(n), None) => format!("{n:.d$}"),
            _ => "-".to_string(),
        };
        for r in &o.readings {
            println!(
                "  {} #{} {}: rate {}, amplitude {}, beat error {}; fault named {}",
                r.take,
                r.reading,
                r.label,
                f(&r.rate_s_per_day, 2),
                f(&r.amplitude_deg, 1),
                f(&r.beat_error_ms, 3),
                r.fault.as_deref().unwrap_or("none"),
            );
        }
        println!();
    }
    println!(
        "Holdout: {} takes, {} readings measured{}.",
        o.takes,
        o.readings_measured,
        if o.unreadable > 0 {
            format!(", {} could not be measured", o.unreadable)
        } else {
            String::new()
        }
    );
    let t = |name: &str, t: &Tally, margin: String| {
        println!(
            "  {name:<11} {} pass, {} fail, {} not judged (margin {margin})",
            t.pass, t.fail, t.not_judged
        );
    };
    t(
        "rate",
        &o.rate_s_per_day,
        format!("{} s/d", o.margins.rate_s_per_day),
    );
    t(
        "amplitude",
        &o.amplitude_deg,
        format!(
            "{}° against tg, which is not the true amplitude",
            o.margins.amplitude_deg
        ),
    );
    t(
        "beat error",
        &o.beat_error_ms,
        format!("{} ms", o.margins.beat_error_ms),
    );
    println!("Holdout results never fail the run; only a reading that cannot be measured does.");
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
    for (name, failures) in [
        ("Unreadable readings", &o.integrity.unreadable),
        (
            "Missing required baseline sessions",
            &o.integrity.missing_sessions,
        ),
        (
            "Missing required baseline readings",
            &o.integrity.missing_readings,
        ),
        (
            "Recording identity changed",
            &o.integrity.recording_mismatches,
        ),
    ] {
        if !failures.is_empty() {
            println!("{name}: {}: FAIL", failures.join(", "));
        }
    }
    if !o.amplitude_high.is_empty() {
        println!(
            "Median amplitude over 330° (the harness's own check): {}: FAIL",
            o.amplitude_high.join(", ")
        );
    }
    for r in &o.smoke {
        let what = match (r.outcome, r.beats, &r.detail) {
            ("ran", Some(b), _) => format!("ran, {b} beats"),
            ("stopped", _, Some(d)) => format!("stopped cleanly: {d}"),
            (_, _, d) => format!("PANICKED: {}", d.as_deref().unwrap_or("?")),
        };
        let heard = match (r.watch_heard, r.no_watch) {
            (Some(false), true) => "; no watch heard, as expected",
            (Some(true), true) => "; a watch heard on an empty stand: FAIL",
            (None, true) => "; analyze did not run: FAIL",
            (Some(false), false) => "; no watch heard",
            _ => "",
        };
        let what = format!("{what}{heard}");
        if names {
            println!("Smoke {} {}: {what}", r.take, r.file);
        } else {
            println!("Smoke {}: {what}", r.take);
        }
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
    #[test]
    fn regression_session_resolution_ignores_malformed_ambient_catalogue() {
        const MARKER: &str = "TIMEGRAPHER_TEST_REGRESSION_CATALOGUE_ROOT";
        if let Some(root) = std::env::var_os(MARKER) {
            let root = PathBuf::from(root);
            let measured =
                measure_all(&root, &["take/session.toml".into()], 1, true, None).unwrap();
            assert_eq!(measured.len(), 1);
            let error = measured[0].1[0].as_ref().err().unwrap();
            assert!(error.contains("missing.wav"), "{error}");
            assert!(!error.contains("calibres.json"), "{error}");
            return;
        }
        let root =
            std::env::temp_dir().join(format!("tg-regress-user-catalogue-{}", std::process::id()));
        fs::create_dir_all(root.join("take")).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(root.join("config/calibres.json"), "malformed JSON").unwrap();
        fs::write(
            root.join("take/session.toml"),
            "calibre=\"Rolex 3235\"\n[[recording]]\nfile=\"missing.wav\"\nposition=\"DU\"\n",
        )
        .unwrap();
        let result=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["regress_cmd::tests::regression_session_resolution_ignores_malformed_ambient_catalogue","--exact"])
            .env(MARKER,&root).env("TIMEGRAPHER_CONFIG_DIR",root.join("config")).output().unwrap();
        let _ = fs::remove_dir_all(&root);
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }

    use super::*;

    #[test]
    fn changed_intervals_fail_identity_but_readable_new_entries_pass() {
        let mut before = v(5.0, 250.0, 0.2, 100);
        before.label = "take (600–1200 s)".into();
        let mut now = before.clone();
        let mut integrity = Integrity::default();
        integrity.check_reading("t", 1, Some(&now), Some(&before));
        assert!(!integrity.fails());
        now.label = "take (1200–1800 s)".into();
        integrity.check_reading("t", 1, Some(&now), Some(&before));
        assert_eq!(integrity.recording_mismatches, ["t #1"]);
        assert!(integrity.fails());
        assert!(!integrity.incomplete()); // A successful changed interval can be rebaselined.
        let mut integrity = Integrity::default();
        integrity.check_reading("new", 1, Some(&now), None);
        assert!(!integrity.fails());
    }

    fn v(rate: f64, amp: f64, be: f64, beats: usize) -> Values {
        Values {
            label: "a.flac".into(),
            rate_s_per_day: Some(rate),
            amplitude_deg: Some(amp),
            beat_error_unlock_ms: Some(be),
            beat_error_ms: Some(be),
            beats,
            provenance: None,
        }
    }

    #[test]
    fn old_entries_are_unknown_and_partial_updates_preserve_origin_and_numbers() {
        let legacy = r#"{"label":"legacy","rate_s_per_day":1.23456789012345,"amplitude_deg":270.123456789,"beat_error_unlock_ms":-0.123456789,"beat_error_ms":0.23456789,"beats":12345}"#;
        let unknown: Values = serde_json::from_str(legacy).unwrap();
        assert!(unknown.provenance.is_none());
        let mut retained = unknown.clone();
        retained.provenance = Some(crate::regress_provenance::EntryProvenance {
            schema: "timegrapher.regress-entry-provenance/1".into(),
            software: serde_json::json!({"git_hash":"original-entry-engine"}),
            manifest: serde_json::json!({"sha256":"old-manifest"}),
            inputs: vec![],
            settings: serde_json::json!({}),
            measurement_context: serde_json::json!({}),
            comparator: serde_json::json!({}),
            additional_fields: BTreeMap::from([(
                "future_annotation".into(),
                serde_json::json!({"preserve":"exact"}),
            )]),
        });
        let old = Baseline {
            schema: BASELINE_SCHEMA.into(),
            software: serde_json::json!({"git_hash":"different-document-writer"}),
            takes: BTreeMap::from([
                ("unknown".into(), vec![unknown.clone()]),
                ("retained".into(), vec![retained.clone()]),
                ("selected".into(), vec![unknown.clone()]),
            ]),
        };
        let fresh = v(3.0, 250.0, 0.1, 999);
        let result = updated_baseline(
            Some(old),
            BTreeMap::from([("selected".into(), vec![fresh.clone()])]),
        );
        let encoded = serde_json::to_string(&result).unwrap();
        let result: Baseline = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            result.takes["unknown"].as_slice(),
            std::slice::from_ref(&unknown)
        );
        assert_eq!(result.takes["retained"], [retained]);
        assert_eq!(result.takes["selected"], [fresh]);
        assert_eq!(
            result.takes["unknown"][0].rate_s_per_day.unwrap().to_bits(),
            unknown.rate_s_per_day.unwrap().to_bits()
        );
        assert_eq!(
            serde_json::to_value(&result.takes["unknown"][0]).unwrap()["provenance"],
            serde_json::Value::Null
        );
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

    #[test]
    fn unreadable_new_readings_fail_even_when_all_rows_are_new() {
        let rows = compare("new", 1, None, None, None, &Margins::default());
        assert!(rows.iter().all(|r| r.status == Status::New));
        let mut integrity = Integrity::default();
        integrity.check_reading("new", 1, None, None);
        assert_eq!(integrity.unreadable, ["new #1"]);
        assert!(integrity.fails());
        assert!(integrity.incomplete());
    }

    #[test]
    fn removed_trailing_readings_fail_but_additions_do_not() {
        let mut integrity = Integrity::default();
        integrity.check_count("t", 1, 3);
        assert_eq!(integrity.missing_readings, ["t #2", "t #3"]);
        assert!(integrity.fails());
        assert!(integrity.incomplete());
        let mut integrity = Integrity::default();
        integrity.check_count("t", 3, 1);
        assert!(!integrity.fails());
    }

    #[test]
    fn missing_sessions_respect_take_selection_and_holdout_exclusions() {
        let baseline = Baseline {
            schema: BASELINE_SCHEMA.into(),
            software: serde_json::json!({}),
            takes: [
                "kept/session.toml",
                "removed/session.toml",
                "sealed/session.toml",
            ]
            .into_iter()
            .map(|s| (s.into(), vec![v(5.0, 250.0, 0.2, 100)]))
            .collect(),
        };
        let sessions = vec!["kept/session.toml".into()];
        let holdout = ["sealed".into()].into_iter().collect();
        assert_eq!(
            required_missing_sessions(Some(&baseline), &sessions, &[], &holdout),
            ["removed"]
        );
        assert!(
            required_missing_sessions(Some(&baseline), &sessions, &["kept".into()], &holdout)
                .is_empty()
        );
        assert_eq!(
            required_missing_sessions(Some(&baseline), &sessions, &["removed".into()], &holdout),
            ["removed"]
        );
        assert!(selected_session("sealed/session.toml", &[], &holdout, true));
        assert!(!selected_session("kept/session.toml", &[], &holdout, true));
    }

    #[test]
    fn holdout_values_are_judged_against_tg_alone() {
        let j = judge(Some(0.25), Some(-0.4), 0.2, true);
        assert_eq!(j.pass, Some(true));
        let j = judge(
            Some(276.0),
            Some(287.0),
            HOLDOUT_MARGINS.amplitude_deg,
            false,
        );
        assert_eq!(j.pass, Some(false));
        assert_eq!(judge(None, Some(1.0), 0.5, false).pass, None);
        assert_eq!(judge(Some(1.0), None, 0.5, false).pass, None);
    }

    #[test]
    fn a_session_file_names_a_fault() {
        let m: Manifest = toml::from_str(
            r#"
[[recording]]
file = "a.flac"
fault = { id = "unlock_too_strong", side = "Tick", source = "bench: deep lock" }

[[recording]]
file = "b.flac"
"#,
        )
        .unwrap();
        let k = m.recordings[0].fault.as_ref().unwrap();
        assert_eq!(k.id, "unlock_too_strong");
        assert_eq!(k.side.as_deref(), Some("Tick"));
        assert!(m.recordings[1].fault.is_none());
    }
}
