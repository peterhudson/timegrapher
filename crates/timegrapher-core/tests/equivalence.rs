//! Cross-pipeline comparisons for defined inputs and measurement intervals.
//! `analyze` and `session` share the chunked file beat pipeline; live pushed
//! audio and replayed logs have their own settlement and latest-piece semantics.
//! See docs/validation.md for the scope and disposition of each ignored case.
//!
//! Synthetic cases run everywhere. Real cases require all four named fixtures
//! when `TIMEGRAPHER_CORPUS` is supplied. With neither it nor
//! `TIMEGRAPHER_REQUIRE_CORPUS` set they explicitly skip; CI requires the corpus
//! for the active analyze/session real-recording comparison.
//! Ignored cases retain stronger unresolved invariants or comparisons of different
//! intervals/cut contexts. Their numbers are historical observations, not current
//! measurements or release gates. Their assertions are preserved as diagnostics.

use std::path::{Path, PathBuf};
use timegrapher_core::analysis::{analyze, AnalysisConfig, Clipping, Summary};
use timegrapher_core::audio::{load, write_wav, Audio};
use timegrapher_core::live::{LiveAnalyzer, LiveConfig, LiveReading};
use timegrapher_core::longrun;
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::measure::{Caveat, NoWatchReason};
use timegrapher_core::periodicity::standard_wheels;
use timegrapher_core::session::{self, Measurement};
use timegrapher_core::stream::{analyze_file, BeatLog, StreamConfig};
use timegrapher_core::synth::{generate, SynthConfig};

/// The regress margins: what counts as the same reading.
const RATE: f64 = 0.3;
const AMPLITUDE: f64 = 1.5;
const BEAT_ERROR: f64 = 0.03;

fn watch(seed: u64) -> Audio {
    generate(
        &SynthConfig {
            duration_s: 180.0,
            rate_s_per_day: 10.0,
            beat_error_ms: 0.6,
            snr_db: 24.0,
            seed,
            ..Default::default()
        },
        |t| 270.0 + 10.0 * (t / 60.0).sin(),
        |_| 0.0,
    )
}

fn on_disk(audio: &Audio, name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("tg-eq-{name}-{}.wav", std::process::id()));
    write_wav(&path, audio).unwrap();
    path
}

fn stream(path: &Path, chunk_s: f64) -> BeatLog {
    let cfg = StreamConfig {
        chunk_s,
        ..Default::default()
    };
    analyze_file(path, &cfg, |_| {}).unwrap()
}

fn whole(log: &BeatLog) -> Measurement {
    session::measure(log, None, 0.0, log.duration_s)
}

fn pushed(audio: &Audio, block: usize) -> LiveAnalyzer {
    let mut l = LiveAnalyzer::new(audio.sample_rate, LiveConfig::default());
    for c in audio.samples.chunks(block) {
        l.push(c);
    }
    l
}

fn close(what: &str, a: Option<f64>, b: Option<f64>, margin: f64) -> Result<(), String> {
    match (a, b) {
        (Some(x), Some(y)) if (x - y).abs() <= margin => Ok(()),
        (None, None) => Ok(()),
        _ => Err(format!("{what}: {a:?} against {b:?} (margin {margin})")),
    }
}

fn size(x: Option<f64>) -> Option<f64> {
    x.map(f64::abs)
}

/// analyze against session on one recording, values signed: both number
/// the beats from the recording's first beat.
fn analyze_vs_session(s: &Summary, m: &Measurement) -> Vec<String> {
    [
        close(
            "rate",
            s.overall.map(|f| f.rate_s_per_day),
            m.rate_s_per_day,
            RATE,
        ),
        close("amplitude", s.amplitude_deg, m.amplitude_deg, AMPLITUDE),
        close(
            "beat error (unlock)",
            s.beat_error_unlock_ms,
            m.beat_error_unlock_ms,
            BEAT_ERROR,
        ),
        close(
            "beat error (drop)",
            s.overall.map(|f| f.beat_error_ms),
            m.beat_error_ms,
            BEAT_ERROR,
        ),
        (s.watch_heard == m.watch_heard)
            .then_some(())
            .ok_or_else(|| format!("watch heard: {} against {}", s.watch_heard, m.watch_heard)),
    ]
    .into_iter()
    .filter_map(Result::err)
    .collect()
}

/// A live reading against analyze, beat errors by size: the live analyser
/// may lock on the other beat first, which flips the sign (see the sign
/// note in docs/agent-interface.md).
fn live_vs_analyze(r: &LiveReading, s: &Summary) -> Vec<String> {
    [
        close(
            "rate",
            r.rate_s_per_day,
            s.overall.map(|f| f.rate_s_per_day),
            RATE,
        ),
        close("amplitude", r.amplitude_deg, s.amplitude_deg, AMPLITUDE),
        close(
            "beat error (unlock)",
            size(r.beat_error_unlock_ms),
            size(s.beat_error_unlock_ms),
            BEAT_ERROR,
        ),
        close(
            "beat error (drop)",
            size(r.beat_error_ms),
            size(s.overall.map(|f| f.beat_error_ms)),
            BEAT_ERROR,
        ),
    ]
    .into_iter()
    .filter_map(Result::err)
    .collect()
}

fn assert_none(what: &str, problems: Vec<String>) {
    assert!(problems.is_empty(), "{what}:\n  {}", problems.join("\n  "));
}

// 1. The whole file, in memory and streamed.

#[test]
fn analyze_and_session_agree_on_a_whole_file() {
    let a = watch(7);
    let path = on_disk(&a, "t1");
    let s = analyze(&a, &AnalysisConfig::default()).summary;
    let mut m = whole(&stream(&path, 47.0));
    std::fs::remove_file(&path).ok();
    assert_none("analyze against session", analyze_vs_session(&s, &m));
    // The same conditions too: the clock is unknown to both, and session
    // takes the lift angle as known unless told.
    if !AnalysisConfig::default().lift_known {
        m.lift_unknown();
    }
    assert_eq!(s.caveats, m.caveats);
    // The same ± on each reading, to within 1% (the streamed read sums
    // in a different order).
    let se = |c: &Option<timegrapher_core::confidence::Confidence>| {
        c.map(|c| [c.rate.se, c.amplitude.se, c.beat_error.se])
    };
    let (a, b) = (se(&s.confidence).unwrap(), se(&m.confidence).unwrap());
    for (x, y) in a.iter().zip(&b) {
        let (x, y) = (x.unwrap(), y.unwrap());
        assert!(
            (x - y).abs() <= 0.01 * x.abs().max(y.abs()),
            "{a:?} against {b:?}"
        );
    }
}

#[test]
fn analyze_and_session_agree_on_real_takes() {
    let files = corpus_files();
    for path in &files {
        let a = load(path).unwrap();
        assert!(
            !a.samples.is_empty(),
            "{}: decoded no samples",
            path.display()
        );
        let s = analyze(&a, &AnalysisConfig::default()).summary;
        let m = whole(&stream(path, 47.0));
        assert!(
            s.watch_heard && m.watch_heard,
            "{}: required watch not heard",
            path.display()
        );
        for (field, value) in [
            ("analyze rate", s.overall.map(|f| f.rate_s_per_day)),
            ("session rate", m.rate_s_per_day),
            ("analyze amplitude", s.amplitude_deg),
            ("session amplitude", m.amplitude_deg),
            ("analyze unlock beat error", s.beat_error_unlock_ms),
            ("session unlock beat error", m.beat_error_unlock_ms),
            (
                "analyze drop beat error",
                s.overall.map(|f| f.beat_error_ms),
            ),
            ("session drop beat error", m.beat_error_ms),
        ] {
            assert!(
                value.is_some_and(f64::is_finite),
                "{}: {field} absent or nonfinite: {value:?}",
                path.display()
            );
        }
        assert_none(&path.display().to_string(), analyze_vs_session(&s, &m));
        eprintln!("Real equivalence comparison completed: {}", path.display());
    }
    if !files.is_empty() {
        eprintln!(
            "Real equivalence comparisons completed: {} / {}",
            files.len(),
            REAL_TAKES.len()
        );
    }
}

// 2. Chunk and block sizes.

#[test]
fn chunk_size_does_not_change_the_readings() {
    let a = watch(8);
    let path = on_disk(&a, "t2");
    let logs: Vec<BeatLog> = [47.0, 10.0, 3.7].map(|c| stream(&path, c)).into();
    std::fs::remove_file(&path).ok();
    for log in &logs[1..] {
        assert_eq!(log.beats.len(), logs[0].beats.len());
        assert_none("chunk size", analyze_vs_session_log(&logs[0], log));
    }
}

#[test]
#[ignore = "gap 2: unresolved stronger timestamp invariant; historical observation: chunk sizes moved beats by up to 34 µs while readings agreed within margins; current cause and magnitude not remeasured (docs/validation.md)"]
fn chunk_size_does_not_change_the_beat_times() {
    let a = watch(8);
    let path = on_disk(&a, "t2b");
    let logs: Vec<BeatLog> = [47.0, 10.0, 3.7].map(|c| stream(&path, c)).into();
    std::fs::remove_file(&path).ok();
    for log in &logs[1..] {
        let worst = log
            .beats
            .iter()
            .zip(&logs[0].beats)
            .map(|(x, y)| (x.time - y.time).abs())
            .fold(0.0, f64::max);
        assert!(worst < 1e-6, "beat times moved by {:.1} µs", worst * 1e6);
    }
}

fn analyze_vs_session_log(a: &BeatLog, b: &BeatLog) -> Vec<String> {
    let (x, y) = (whole(a), whole(b));
    [
        close("rate", x.rate_s_per_day, y.rate_s_per_day, RATE),
        close("amplitude", x.amplitude_deg, y.amplitude_deg, AMPLITUDE),
        close(
            "beat error (unlock)",
            x.beat_error_unlock_ms,
            y.beat_error_unlock_ms,
            BEAT_ERROR,
        ),
    ]
    .into_iter()
    .filter_map(Result::err)
    .collect()
}

#[test]
#[ignore = "gap 3: unresolved live block-size invariant; historical counts: 1,432, 1,435 and 1,436 beats of 1,440 for blocks of 480, 4,096 and 48,000 samples; not remeasured (docs/validation.md)"]
fn push_size_does_not_change_the_live_beats() {
    let a = watch(9);
    let counts: Vec<usize> = [480, 4096, 48_000]
        .map(|b| pushed(&a, b).beats().len())
        .into();
    assert!(counts.iter().all(|&n| n == counts[0]), "beats: {counts:?}");
}

// 3. Live against the whole recording.

#[test]
fn live_matches_analyze_on_a_whole_file() {
    let a = watch(10);
    let s = analyze(&a, &AnalysisConfig::default()).summary;
    for block in [480, 48_000] {
        let r = pushed(&a, block).reading(a.duration());
        assert_none(
            &format!("pushed in blocks of {block}"),
            live_vs_analyze(&r, &s),
        );
    }
}

#[test]
fn a_replayed_take_matches_session() {
    let a = watch(11);
    let path = on_disk(&a, "t3");
    let log = stream(&path, 47.0);
    std::fs::remove_file(&path).ok();
    let m = whole(&log);
    let r = LiveAnalyzer::from_log(log.clone(), LiveConfig::default()).reading(log.duration_s);
    assert_none(
        "replay against session",
        [
            close("rate", r.rate_s_per_day, m.rate_s_per_day, RATE),
            close("amplitude", r.amplitude_deg, m.amplitude_deg, AMPLITUDE),
            close(
                "beat error (unlock)",
                r.beat_error_unlock_ms,
                m.beat_error_unlock_ms,
                BEAT_ERROR,
            ),
        ]
        .into_iter()
        .filter_map(Result::err)
        .collect(),
    );
}

#[test]
#[ignore = "gap 4: real live/whole-file interval, settlement and tracking equivalence unresolved; historical observations, not remeasured: (Daytona beat error from the unlock 0.19 against 0.36 ms; ETA holder amplitude 324° against 332° at lift 52°, measured before analyze shared the stream's beats, beat error from the drop 0.56 against 0.13 ms; Patek both beat errors' signs flipped); comparator already uses beat-error magnitudes (docs/validation.md)"]
fn live_matches_analyze_on_real_takes() {
    for path in corpus_files() {
        let a = load(&path).unwrap();
        let s = analyze(&a, &AnalysisConfig::default()).summary;
        let r = pushed(&a, 4800).reading(a.duration());
        assert_none(&path.display().to_string(), live_vs_analyze(&r, &s));
    }
}

#[test]
#[ignore = "gap 5: replay latest-piece and session whole-selection are distinct quantities/cut contexts; historical ETA holder rates 11.49 against 11.14 s/d, not remeasured; universal equality is not the supported contract (docs/validation.md)"]
fn a_replayed_real_take_matches_session() {
    for path in corpus_files() {
        let log = stream(&path, 47.0);
        let m = whole(&log);
        let r = LiveAnalyzer::from_log(log.clone(), LiveConfig::default()).reading(log.duration_s);
        assert_none(
            &path.display().to_string(),
            [close("rate", r.rate_s_per_day, m.rate_s_per_day, RATE)]
                .into_iter()
                .filter_map(Result::err)
                .collect(),
        );
    }
}

// 4. long's slices against the whole take.

#[test]
fn the_slices_average_to_the_whole_take() {
    let a = watch(12);
    let path = on_disk(&a, "t4");
    let log = stream(&path, 47.0);
    std::fs::remove_file(&path).ok();
    let lc = LongConfig {
        wheels: standard_wheels(log.bph, 15),
        ..Default::default()
    };
    let r = longrun::analyse(&log, None, &lc);
    let (mut weight, mut sum) = (0.0, 0.0);
    for s in &r.slices {
        if let Some(x) = s.rate_s_per_day {
            weight += s.end_s - s.start_s;
            sum += (s.end_s - s.start_s) * x;
        }
    }
    let whole = r.overall.map(|f| f.rate_s_per_day).unwrap();
    assert!(
        (sum / weight - whole).abs() < 0.05,
        "slices {:.3} against the whole take {whole:.3} s/d",
        sum / weight
    );
}

// 5. The same caveats from every pipeline.

fn empty_stand() -> Audio {
    // Room noise with no watch: seeded so the test is repeatable.
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let samples = (0..48_000 * 60)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            ((x >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.02
        })
        .collect();
    Audio {
        samples,
        sample_rate: 48_000,
    }
}

#[test]
fn an_empty_stand_gives_no_readings() {
    let a = empty_stand();
    let path = on_disk(&a, "t5a");
    let log = stream(&path, 47.0);
    std::fs::remove_file(&path).ok();
    let s = analyze(&a, &AnalysisConfig::default()).summary;
    assert!(!s.watch_heard && s.overall.is_none() && s.amplitude_deg.is_none());
    let m = whole(&log);
    assert!(!m.watch_heard && m.rate_s_per_day.is_none() && m.amplitude_deg.is_none());
    assert!(
        s.caveats.contains(&Caveat::NoWatch {
            reason: NoWatchReason::Scatter
        }) && m.caveats.contains(&Caveat::NoWatch {
            reason: NoWatchReason::Scatter
        })
    );
    assert!(s.confidence.is_none() && m.confidence.is_none());
    let r = pushed(&a, 4800).reading(a.duration());
    assert!(r.rate_s_per_day.is_none() && r.amplitude_deg.is_none());
}

#[test]
fn an_empty_stand_replayed_gives_no_readings() {
    let a = empty_stand();
    let path = on_disk(&a, "t5b");
    let log = stream(&path, 47.0);
    std::fs::remove_file(&path).ok();
    let r = LiveAnalyzer::from_log(log.clone(), LiveConfig::default()).reading(log.duration_s);
    assert!(
        r.rate_s_per_day.is_none() && r.beat_error_ms.is_none(),
        "rate {:?}, beat error {:?}",
        r.rate_s_per_day,
        r.beat_error_ms
    );
}

/// The first `seconds` of a recording.
fn cut(mut a: Audio, seconds: f64) -> Audio {
    a.samples
        .truncate((seconds * a.sample_rate as f64) as usize);
    a
}

/// Under 10 s the scatter is not assessed; the beats' averaged sound is,
/// and hiss alone shows no unlock or drop on either side, where it had
/// read as a watch with readings. Every pipeline says so with the same
/// reason.
#[test]
fn hiss_under_ten_seconds_gives_no_readings() {
    let no_sound = Caveat::NoWatch {
        reason: NoWatchReason::NoBeatSound,
    };
    for seconds in [3.0, 5.0, 9.0] {
        let a = cut(empty_stand(), seconds);
        let path = on_disk(&a, &format!("t5c{seconds}"));
        let log = stream(&path, 47.0);
        std::fs::remove_file(&path).ok();
        let s = analyze(&a, &AnalysisConfig::default()).summary;
        assert!(
            !s.watch_heard && s.overall.is_none() && s.caveats.contains(&no_sound),
            "{seconds} s analyze: {:?}",
            s.caveats
        );
        let m = whole(&log);
        assert!(
            !m.watch_heard && m.rate_s_per_day.is_none() && m.caveats.contains(&no_sound),
            "{seconds} s session: {:?}",
            m.caveats
        );
        // Live listening, with the beat rate set as a watch's calibre
        // sets it, keeps too few of these beats to read.
        let mut l = LiveAnalyzer::new(
            a.sample_rate,
            LiveConfig {
                analysis: AnalysisConfig {
                    bph: Some(21_600),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        for c in a.samples.chunks(4800) {
            l.push(c);
        }
        let r = l.reading(a.duration());
        assert!(
            r.rate_s_per_day.is_none(),
            "{seconds} s live: {:?}",
            r.caveats
        );
        // A file opened in the app: the log replayed.
        let r = LiveAnalyzer::from_log(log.clone(), LiveConfig::default()).reading(log.duration_s);
        assert!(
            !r.watch_heard && r.rate_s_per_day.is_none() && r.caveats.contains(&no_sound),
            "{seconds} s replayed: {:?}",
            r.caveats
        );
    }
}

/// A watch under 10 s keeps its readings: its beats' sound shows an
/// unlock and a drop.
#[test]
fn a_watch_under_ten_seconds_keeps_its_readings() {
    for seconds in [3.0, 5.0, 9.0] {
        let a = cut(watch(17), seconds);
        let path = on_disk(&a, &format!("t5d{seconds}"));
        let log = stream(&path, 47.0);
        std::fs::remove_file(&path).ok();
        let s = analyze(&a, &AnalysisConfig::default()).summary;
        assert!(
            s.watch_heard && s.overall.is_some(),
            "{seconds} s analyze: {:?}",
            s.caveats
        );
        let m = whole(&log);
        assert!(
            m.watch_heard && m.rate_s_per_day.is_some(),
            "{seconds} s session: {:?}",
            m.caveats
        );
        let r = pushed(&a, 4800).reading(a.duration());
        assert!(r.watch_heard, "{seconds} s live: {:?}", r.caveats);
        let r = LiveAnalyzer::from_log(log.clone(), LiveConfig::default()).reading(log.duration_s);
        assert!(
            r.watch_heard && r.rate_s_per_day.is_some(),
            "{seconds} s replayed: {:?}",
            r.caveats
        );
    }
}

/// The synthetic watch with its peaks driven `over` times past full scale.
fn clipped(over: f32) -> Audio {
    let mut a = watch(13);
    let peak = a.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
    for v in &mut a.samples {
        *v = (*v * over / peak).clamp(-1.0, 1.0);
    }
    a
}

#[test]
fn clipping_is_named_in_every_pipeline() {
    let a = clipped(2.0);
    let path = on_disk(&a, "t5c");
    let log = stream(&path, 47.0);
    std::fs::remove_file(&path).ok();
    let s = analyze(&a, &AnalysisConfig::default()).summary;
    assert!(s.doubts.clipped, "analyze: {:?}", s.clipping);
    let m = whole(&log);
    assert!(Clipping::noted_fraction(m.clipped_fraction), "session");
    let named = |c: &[Caveat]| c.iter().any(|c| matches!(c, Caveat::Clipped { .. }));
    assert!(named(&s.caveats) && named(&m.caveats));
    let r = pushed(&a, 4800).reading(a.duration());
    assert!(
        r.clipped_fraction.is_some_and(Clipping::noted_fraction),
        "live: {:?}",
        r.clipped_fraction
    );
}

#[test]
fn heavy_clipping_still_reads_live() {
    let a = clipped(5.0);
    let s = analyze(&a, &AnalysisConfig::default()).summary;
    let r = pushed(&a, 4800).reading(a.duration());
    assert!(s.watch_heard && s.doubts.clipped);
    assert!(
        r.beats_used > 0 && r.clipped_fraction.is_some_and(Clipping::noted_fraction),
        "live read {} beats",
        r.beats_used
    );
}

const REAL_TAKES: [&str; 4] = [
    "daytona4130_DU_30min",
    "ym42_DD_30min",
    "aquanaut324_DU_30min",
    "eta2824_holder_DU_30min",
];
const REAL_FILE: &str = "take_part01-001.flac";

/// The Dandong 3235 take a calibre at its own rate is picked on, then
/// Auto, mid-listen.
const SWITCH_TAKE: &str = "dd3235_DU_30min";

/// Picking a calibre at the rate Auto found (Dandong 3235, 28800), then
/// Auto again, mid-listen keeps the beats heard so far: every reading and
/// each side's averaged sound after it, every 2 s for over a minute, are
/// exactly a listener's that never switched. Runs where
/// `TIMEGRAPHER_CORPUS` is set.
#[test]
fn a_calibre_then_auto_at_the_found_rate_keeps_every_reading() {
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(
            std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_none(),
            "TIMEGRAPHER_CORPUS is not set"
        );
        eprintln!("SKIP calibre then Auto: TIMEGRAPHER_CORPUS not set");
        return;
    };
    let audio = cut(
        load(&root.join("takes").join(SWITCH_TAKE).join(REAL_FILE)).unwrap(),
        120.0,
    );
    let block = audio.sample_rate as usize / 10;
    let at = |s: f64| (s * audio.sample_rate as f64) as usize / block;
    let (pick, auto) = (at(40.0), at(50.0));
    let mut never = LiveAnalyzer::new(audio.sample_rate, LiveConfig::default());
    let mut kept = LiveAnalyzer::new(audio.sample_rate, LiveConfig::default());
    let mut compared = 0;
    for (i, c) in audio.samples.chunks(block).enumerate() {
        if i == pick {
            assert_eq!(kept.bph(), Some(28_800));
            assert!(kept.set_bph(Some(28_800)), "the beats stand");
        }
        if i == auto {
            assert!(kept.set_bph(None), "the beats stand");
        }
        never.push(c);
        kept.push(c);
        if i < pick || i % 20 != 0 {
            continue;
        }
        let (n, k) = (never.reading(10.0), kept.reading(10.0));
        let t = n.time_s;
        assert_eq!(format!("{k:?}"), format!("{n:?}"), "{t:.1} s: the reading");
        assert_eq!(
            format!("{:?}", kept.tick_profiles()),
            format!("{:?}", never.tick_profiles()),
            "{t:.1} s: the averaged sound"
        );
        compared += usize::from(n.amplitude_deg.is_some());
    }
    assert!(compared >= 30, "{compared} moments with readings");
}

fn corpus_files() -> Vec<PathBuf> {
    let root = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from);
    corpus_files_at(
        root.as_deref(),
        std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some(),
    )
    .unwrap_or_else(|e| panic!("required real fixtures: {e}"))
}

/// Resolve and authenticate exact fixture names before running any comparison.
/// Explicitly supplied corpora never silently shrink to the available subset.
fn corpus_files_at(root: Option<&Path>, required: bool) -> Result<Vec<PathBuf>, String> {
    let Some(root) = root else {
        if required {
            return Err("TIMEGRAPHER_CORPUS is not set".into());
        }
        eprintln!(
            "SKIP real equivalence: TIMEGRAPHER_CORPUS not set; no acoustic validation performed"
        );
        return Ok(Vec::new());
    };
    if !root.is_dir() {
        return Err(format!(
            "{}: corpus root is not a directory",
            root.display()
        ));
    }
    let index = root.join("corpus.json");
    let bytes = std::fs::read(&index).map_err(|e| format!("{}: {e}", index.display()))?;
    let corpus: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", index.display()))?;
    if corpus["schema"] != 1 {
        return Err(format!("{}: expected corpus schema 1", index.display()));
    }
    let takes = corpus["takes"]
        .as_array()
        .ok_or("corpus.json: missing takes array")?;
    eprintln!(
        "Real equivalence corpus.json SHA-256: {}",
        timegrapher_core::archive::sha256_hex(&bytes)
    );
    let mut files = Vec::new();
    for id in REAL_TAKES {
        let members: Vec<_> = takes.iter().filter(|t| t["id"] == id).collect();
        if members.len() != 1
            || !matches!(members[0]["tier"].as_str(), Some("referenced" | "paired"))
        {
            return Err(format!(
                "corpus.json: expected one development reference take {id}"
            ));
        }
        let entries = members[0]["files"]
            .as_array()
            .ok_or_else(|| format!("{id}: missing files array"))?;
        let members: Vec<_> = entries.iter().filter(|f| f["name"] == REAL_FILE).collect();
        if members.len() != 1 {
            return Err(format!("corpus.json: expected one {id}/{REAL_FILE}"));
        }
        let entry = members[0];
        // Match the previous helper's per-take layout selection. Once a
        // directory is selected, a bad member must fail rather than fall back.
        let nested = root.join("takes").join(id);
        let dir = if nested.is_dir() {
            nested
        } else {
            root.join(id)
        };
        let path = dir.join(REAL_FILE);
        let metadata = std::fs::metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(format!(
                "{}: required fixture is not a nonempty regular file",
                path.display()
            ));
        }
        let audio = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let hash = timegrapher_core::archive::sha256_hex(&audio);
        if entry["bytes"].as_u64() != Some(audio.len() as u64)
            || entry["sha256"].as_str() != Some(hash.as_str())
        {
            return Err(format!(
                "{}: bytes or SHA-256 differ from corpus.json",
                path.display()
            ));
        }
        eprintln!(
            "Real equivalence verified input: {id}/{REAL_FILE}; bytes={}; sha256={hash}",
            audio.len()
        );
        files.push(path);
    }
    Ok(files)
}

mod fixture_validation {
    use super::*;
    use serde_json::json;
    use std::fs;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("tg-eq-fixtures-{}-{id}", std::process::id()));
            let content = b"fixture authentication only; not acoustic test data";
            let hash = timegrapher_core::archive::sha256_hex(content);
            let takes: Vec<_> = REAL_TAKES
                .iter()
                .map(|id| {
                    let dir = root.join("takes").join(id);
                    fs::create_dir_all(&dir).unwrap();
                    fs::write(dir.join(REAL_FILE), content).unwrap();
                    json!({"id": id, "tier": "referenced", "files": [{
                        "name": REAL_FILE, "bytes": content.len(), "sha256": hash,
                    }]})
                })
                .collect();
            fs::write(
                root.join("corpus.json"),
                json!({"schema": 1, "takes": takes}).to_string(),
            )
            .unwrap();
            Self(root)
        }

        fn first(&self) -> PathBuf {
            self.0.join("takes").join(REAL_TAKES[0]).join(REAL_FILE)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn only_an_unconfigured_optional_run_can_skip() {
        assert!(corpus_files_at(None, false).unwrap().is_empty());
        assert!(corpus_files_at(None, true).unwrap_err().contains("not set"));
        let fixture = Fixture::new();
        for required in [false, true] {
            assert!(corpus_files_at(Some(&fixture.0.join("absent")), required).is_err());
            assert!(corpus_files_at(Some(&fixture.first()), required).is_err());
        }
    }

    #[test]
    fn exact_complete_manifest_inputs_are_selected_in_both_layouts() {
        let fixture = Fixture::new();
        fs::write(
            fixture.first().with_file_name("000-unlisted.flac"),
            b"not selected",
        )
        .unwrap();
        let files = corpus_files_at(Some(&fixture.0), true).unwrap();
        assert_eq!(files.len(), 4);
        assert_eq!(files[0], fixture.first());
        assert!(files.iter().all(|p| p.file_name().unwrap() == REAL_FILE));
        // Mixed layouts remain supported; an unrelated takes/ directory must
        // not hide complete flat fixtures.
        fs::rename(
            fixture.0.join("takes").join(REAL_TAKES[0]),
            fixture.0.join(REAL_TAKES[0]),
        )
        .unwrap();
        let files = corpus_files_at(Some(&fixture.0), true).unwrap();
        assert_eq!(files.len(), 4);
        assert_eq!(files[0], fixture.0.join(REAL_TAKES[0]).join(REAL_FILE));
        for id in &REAL_TAKES[1..] {
            fs::rename(fixture.0.join("takes").join(id), fixture.0.join(id)).unwrap();
        }
        let files = corpus_files_at(Some(&fixture.0), false).unwrap();
        assert_eq!(files.len(), 4);
        assert_eq!(files[0], fixture.0.join(REAL_TAKES[0]).join(REAL_FILE));
        // A present nested take wins selection. Its missing member must not be
        // masked by the complete flat copy of that same take.
        fs::create_dir(fixture.0.join("takes").join(REAL_TAKES[0])).unwrap();
        assert!(corpus_files_at(Some(&fixture.0), false).is_err());
        assert!(corpus_files_at(Some(&fixture.0), true).is_err());
    }

    #[test]
    fn partial_or_unauthenticated_inputs_fail_even_in_optional_mode() {
        for fault in [
            "missing manifest",
            "invalid manifest",
            "wrong schema",
            "missing take",
            "duplicate take",
            "holdout",
            "missing member",
            "missing file",
            "empty file",
            "not a file",
            "wrong size",
            "wrong hash",
        ] {
            let fixture = Fixture::new();
            let index = fixture.0.join("corpus.json");
            let mut manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
            match fault {
                "wrong schema" => manifest["schema"] = json!(2),
                "missing take" => {
                    manifest["takes"].as_array_mut().unwrap().remove(0);
                }
                "duplicate take" => {
                    let first = manifest["takes"][0].clone();
                    manifest["takes"].as_array_mut().unwrap().push(first);
                }
                "holdout" => manifest["takes"][0]["tier"] = json!("holdout"),
                "missing member" => manifest["takes"][0]["files"] = json!([]),
                "missing file" => fs::remove_file(fixture.first()).unwrap(),
                "empty file" => fs::write(fixture.first(), []).unwrap(),
                "not a file" => {
                    fs::remove_file(fixture.first()).unwrap();
                    fs::create_dir(fixture.first()).unwrap();
                }
                "wrong size" => manifest["takes"][0]["files"][0]["bytes"] = json!(1),
                "wrong hash" => manifest["takes"][0]["files"][0]["sha256"] = json!("0".repeat(64)),
                _ => {}
            }
            fs::write(&index, manifest.to_string()).unwrap();
            if fault == "missing manifest" {
                fs::remove_file(&index).unwrap();
            }
            if fault == "invalid manifest" {
                fs::write(&index, b"not JSON").unwrap();
            }
            for required in [false, true] {
                assert!(
                    corpus_files_at(Some(&fixture.0), required).is_err(),
                    "{fault}; required={required}"
                );
            }
        }
    }
}
