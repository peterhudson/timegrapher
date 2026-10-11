//! The same watch read four ways must give the same figures: `analyze`
//! (the whole recording in memory), `session` (the stream, in chunks),
//! `long`'s slices, and the live analyser (pushed audio, or a stored take
//! replayed from its beat log). See /docs/agent-interface.md for the fields.
//!
//! Synthetic cases run everywhere. Cases on real recordings run only when
//! `TIMEGRAPHER_CORPUS` names a corpus checkout with its audio fetched (the
//! takes under `takes/` or at the top level); otherwise they pass at once.
//! Tests marked `ignore` record a disagreement found on today's code; each
//! names it, and the shared measurement (`measure.rs`) is meant to close it.

use std::path::{Path, PathBuf};
use timegrapher_core::analysis::{analyze, AnalysisConfig, Summary};
use timegrapher_core::audio::{load, write_wav, Audio};
use timegrapher_core::live::{LiveAnalyzer, LiveConfig, LiveReading};
use timegrapher_core::longrun;
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::measure::Caveat;
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
    // The same judgement too: the clock is unknown to both, and session
    // takes the lift angle as known unless told.
    if !AnalysisConfig::default().lift_known {
        m.lift_unknown();
    }
    assert_eq!(s.caveats, m.caveats);
    let grades = |c: &Option<timegrapher_core::confidence::Confidence>| {
        c.map(|c| (c.rate.grade, c.amplitude.grade, c.beat_error.grade))
    };
    assert_eq!(grades(&s.confidence), grades(&m.confidence));
}

#[test]
fn analyze_and_session_agree_on_real_takes() {
    for path in corpus_files() {
        let a = load(&path).unwrap();
        let s = analyze(&a, &AnalysisConfig::default()).summary;
        let m = whole(&stream(&path, 47.0));
        assert_none(&path.display().to_string(), analyze_vs_session(&s, &m));
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
#[ignore = "gap 2: a beat's time depends on the chunk size by up to 34 µs (each chunk times its beats against its own template), though the readings agree within the margins"]
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
#[ignore = "gap 3: the live analyser's beats depend on how the audio is pushed (1,432, 1,435 and 1,436 beats of 1,440 for blocks of 480, 4,096 and 48,000 samples)"]
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
#[ignore = "gap 4: on real takes live readings differ from analyze (Daytona beat error from the unlock 0.19 against 0.36 ms; ETA holder amplitude 324° against 332° at lift 52°, measured before analyze shared the stream's beats, beat error from the drop 0.56 against 0.13 ms; Patek both beat errors' signs flipped)"]
fn live_matches_analyze_on_real_takes() {
    for path in corpus_files() {
        let a = load(&path).unwrap();
        let s = analyze(&a, &AnalysisConfig::default()).summary;
        let r = pushed(&a, 4800).reading(a.duration());
        assert_none(&path.display().to_string(), live_vs_analyze(&r, &s));
    }
}

#[test]
#[ignore = "gap 5: a replayed real take reads a different rate from session over the same beats (ETA holder 11.49 against 11.14 s/d): the replay covers only the run of beats after the last dropout"]
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
    assert!(s.caveats.contains(&Caveat::NoWatch) && m.caveats.contains(&Caveat::NoWatch));
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
fn clipping_warns_in_every_pipeline() {
    let a = clipped(2.0);
    let path = on_disk(&a, "t5c");
    let log = stream(&path, 47.0);
    std::fs::remove_file(&path).ok();
    let s = analyze(&a, &AnalysisConfig::default()).summary;
    assert!(s.doubts.clipped, "analyze: {:?}", s.clipping);
    let m = whole(&log);
    assert!(m.clipping_warns(), "session");
    let named = |c: &[Caveat]| c.iter().any(|c| matches!(c, Caveat::Clipped { .. }));
    assert!(named(&s.caveats) && named(&m.caveats));
    let r = pushed(&a, 4800).reading(a.duration());
    assert!(r.clipping_warns(), "live: {:?}", r.clipped_fraction);
}

#[test]
fn heavy_clipping_still_reads_live() {
    let a = clipped(5.0);
    let s = analyze(&a, &AnalysisConfig::default()).summary;
    let r = pushed(&a, 4800).reading(a.duration());
    assert!(s.watch_heard && s.doubts.clipped);
    assert!(
        r.beats_used > 0 && r.clipping_warns(),
        "live read {} beats",
        r.beats_used
    );
}

/// The first file of each take the gaps were found on, when a corpus is
/// at hand.
fn corpus_files() -> Vec<PathBuf> {
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        eprintln!("TIMEGRAPHER_CORPUS not set: no real takes checked");
        return Vec::new();
    };
    let takes = [
        "daytona4130_DU_30min",
        "ym42_DD_30min",
        "patek324_DU_30min",
        "eta2824_holder_DU_30min",
    ];
    takes
        .iter()
        .filter_map(|t| {
            let dir = [root.join("takes").join(t), root.join(t)]
                .into_iter()
                .find(|d| d.is_dir())?;
            let mut f: Vec<PathBuf> = std::fs::read_dir(dir)
                .ok()?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "flac"))
                .collect();
            f.sort();
            f.into_iter().next()
        })
        .collect()
}
