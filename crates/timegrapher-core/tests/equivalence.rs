//! Cross-pipeline comparisons for defined inputs and measurement intervals.
//! `analyze` and `session` share the chunked file beat pipeline; live pushed
//! audio and replayed logs have their own settlement and latest-piece semantics.
//! See docs/research/validation.md for the scope and disposition of each ignored case.
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
use timegrapher_core::live::{LiveAnalyzer, LiveConfig, LiveReading, READ_LAG_CAP_S};
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
/// note in docs/reference/agent-interface.md).
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
    // in a different order). The rate's 180 s hold 18 of the 30 values
    // its ± takes its scatter from, so neither gives one (#1470).
    let se = |c: &Option<timegrapher_core::confidence::Confidence>| {
        c.map(|c| [c.rate.se, c.amplitude.se, c.beat_error.se])
    };
    let (a, b) = (se(&s.confidence).unwrap(), se(&m.confidence).unwrap());
    assert!(a[0].is_none() && b[0].is_none(), "{a:?} against {b:?}");
    for (x, y) in a.iter().zip(&b).skip(1) {
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
#[ignore = "gap 2: unresolved stronger timestamp invariant; historical observation: chunk sizes moved beats by up to 34 µs while readings agreed within margins; current cause and magnitude not remeasured (docs/research/validation.md)"]
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
#[ignore = "gap 3: unresolved live block-size invariant; historical counts: 1,432, 1,435 and 1,436 beats of 1,440 for blocks of 480, 4,096 and 48,000 samples; not remeasured (docs/research/validation.md)"]
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

/// The takes live is checked on against a file's reading: the ETA bare,
/// not in its holder, which reads unreliably either way
/// (docs/research/validation.md).
const LIVE_TAKES: [&str; 4] = [
    "daytona4130_DU_30min",
    "ym42_DD_30min",
    "aquanaut324_DU_30min",
    "eta2824_bare_DU_30min",
];

/// Live against a file's reading of the same take, like with like (#1327):
/// over the same interval, from the latest cut either has to live's last
/// settled beat. Live reads its first chunk again with a template built as
/// the file builds it, so both read the whole take with the same template.
#[test]
fn live_matches_analyze_on_real_takes() {
    for path in corpus_takes(&LIVE_TAKES) {
        let a = load(&path).unwrap();
        // Pushed as `pushed` does, timing each push: reading the first
        // chunk again (#1327) is spread over the pushes after it.
        let mut l = LiveAnalyzer::new(a.sample_rate, LiveConfig::default());
        let (mut slowest, mut usual) = (0.0f64, Vec::new());
        for c in a.samples.chunks(4800) {
            let t = std::time::Instant::now();
            l.push(c);
            let took = t.elapsed().as_secs_f64();
            slowest = slowest.max(took);
            usual.push(took);
        }
        usual.sort_by(f64::total_cmp);
        eprintln!(
            "{}: pushes of 0.1 s took {:.4} s at the median, {:.4} s at most; built again at {:?} s",
            path.display(),
            usual[usual.len() / 2],
            slowest,
            l.template_switch_s()
        );
        assert!(
            l.template_switch_s().is_some(),
            "{}: not built again",
            path.display()
        );
        assert!(
            !l.gathering(),
            "{}: the sound gathered is kept",
            path.display()
        );
        let log = stream(&path, 47.0);
        let end = l.reading(a.duration()).time_s;
        let last = |v: &[f64]| v.iter().copied().filter(|&b| b < end).fold(0.0, f64::max);
        let from = last(l.breaks()).max(last(&log.jumps));
        let r = l.reading_at(end, end - from);
        let m = session::measure(&log, None, from, end);
        let what = format!("{} over {from:.2} to {end:.2} s", path.display());
        eprintln!(
            "{what}: rate {:?} against {:?}, amplitude {:?} against {:?}, \
             beat error (unlock) {:?} against {:?}, (drop) {:?} against {:?}",
            r.rate_s_per_day,
            m.rate_s_per_day,
            r.amplitude_deg,
            m.amplitude_deg,
            r.beat_error_unlock_ms,
            m.beat_error_unlock_ms,
            r.beat_error_ms,
            m.beat_error_ms
        );
        assert_none(&what, live_vs_session(&r, &m));
    }
}

/// A live reading against session's of the same interval, beat errors by
/// size, as [`live_vs_analyze`].
fn live_vs_session(r: &LiveReading, m: &Measurement) -> Vec<String> {
    [
        close("rate", r.rate_s_per_day, m.rate_s_per_day, RATE),
        close("amplitude", r.amplitude_deg, m.amplitude_deg, AMPLITUDE),
        close(
            "beat error (unlock)",
            size(r.beat_error_unlock_ms),
            size(m.beat_error_unlock_ms),
            BEAT_ERROR,
        ),
        close(
            "beat error (drop)",
            size(r.beat_error_ms),
            size(m.beat_error_ms),
            BEAT_ERROR,
        ),
    ]
    .into_iter()
    .filter_map(Result::err)
    .collect()
}

#[test]
#[ignore = "gap 5: replay latest-piece and session whole-selection are distinct quantities/cut contexts; historical ETA holder rates 11.49 against 11.14 s/d, not remeasured; universal equality is not the supported contract (docs/research/validation.md)"]
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

/// Whether a watch is heard does not depend on how it is averaged (#1965).
/// An average under 10 s is judged over the sound its scatter is read
/// from, its 10 s look back, as a 10 s average is: opened at 2 s, an empty
/// stand scattering 4% of a beat was not judged, and its strip drew the
/// noise as a watch at 125,000 s/d. Every second from 10 s, live, opened
/// and saved, at each Average Over under 10 s against 10 s.
#[test]
fn a_short_average_hears_a_watch_as_a_ten_second_one_does() {
    let a = empty_stand();
    let path = on_disk(&a, "t1965");
    let log = stream(&path, 47.0);
    std::fs::remove_file(&path).ok();
    let opened = LiveAnalyzer::from_log(log.clone(), LiveConfig::default());
    let live = pushed(&a, 4800);
    short_averages_agree(&[("live", &live), ("opened", &opened)], &log, &opened);
}

/// A short reading's look back counts the sound heard, masked seconds
/// left out (#1965, #2017): live, before a beat rate is found, 11 s into
/// an empty stand with 3 s masked holds 8 s of sound, so its regularity
/// is not assessed, and with nothing masked it is, as noise.
#[test]
fn a_mask_in_a_short_readings_look_back_is_no_sound_heard() {
    let a = empty_stand();
    let live = pushed(&a, 4800);
    let mut masked = live.clone();
    masked.set_masked(&[(1.0, 4.0)]);
    let scatter = Caveat::NoWatch {
        reason: NoWatchReason::Scatter,
    };
    let (r, m) = (live.reading_at(11.0, 2.0), masked.reading_at(11.0, 2.0));
    assert_eq!((r.bph, m.bph), (None, None), "before a beat rate is found");
    assert!(r.caveats.contains(&scatter), "12 s heard: judged, noise");
    assert!(!m.caveats.contains(&scatter), "8 s heard: not judged");
    assert!(m.rate_s_per_day.is_none());
}

/// A saved short stretch whose look back reads no scatter judges nothing
/// there, and is judged on its own span (#1965, #2150): the app reads a
/// take after a settle from a log without the settle's beats, so 5 s of a
/// clean watch after 20 s with no beats read as noise, and the app showed
/// no beat error where `session` showed one.
#[test]
fn a_look_back_with_no_beats_judges_nothing() {
    let beats = (160..200)
        .map(|index| timegrapher_core::beats::Beat {
            time: index as f64 * 0.125 + if index % 2 == 1 { 0.0002 } else { 0.0 },
            index,
            quality: 1.0,
        })
        .collect();
    let log = BeatLog {
        lock_template_kept: false,
        sample_rate: 48_000,
        duration_s: 25.0,
        bph: 28_800,
        lift_deg: 52.0,
        escapement: Default::default(),
        beats,
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    let m = session::measure(&log, None, 20.0, 25.0);
    assert!(m.jitter_us.is_none(), "no scatter read in the look back");
    assert!(m.watch_heard, "{:?}", m.caveats);
    assert!(m.rate_s_per_day.is_some());
}

/// [`a_short_average_hears_a_watch_as_a_ten_second_one_does`] for
/// `engines`: the verdict at each Average Over under 10 s, every second
/// from 10 s, is the 10 s one's, and a reading not heard shows no
/// figures. Saved, a stretch that short over the sound of `log`, where its
/// look back reads a scatter, gets the verdict `opened`, the engine built
/// from `log`, gives it (#1688).
fn short_averages_agree(engines: &[(&str, &LiveAnalyzer)], log: &BeatLog, opened: &LiveAnalyzer) {
    let shorts = timegrapher_core::live::AVERAGES
        .into_iter()
        .filter(|&s| s < 10.0);
    let shorts: Vec<f64> = shorts.collect();
    assert!(!shorts.is_empty());
    for &(how, live) in engines {
        let mut t = 10.0;
        while t <= live.duration_s() {
            let ten = live.reading_at(t, 10.0);
            for &s in &shorts {
                let r = live.reading_at(t, s);
                assert_eq!(
                    (r.watch_heard, r.timing_scatter_fraction),
                    (ten.watch_heard, ten.timing_scatter_fraction),
                    "{how}, {s} s ending at {t} s: {:?} s/d",
                    r.rate_s_per_day
                );
                assert!(r.watch_heard || r.rate_s_per_day.is_none());
            }
            t += 1.0;
        }
    }
    let mut t = 10.0;
    while t <= log.duration_s {
        for &s in &shorts {
            let m = session::measure(log, None, t - s, t);
            let r = opened.reading_at(t, s);
            // Where its look back reads a scatter: one that reads none
            // judges nothing, saved, where live calls it noise (#2150).
            assert!(
                m.jitter_us.is_none() || m.watch_heard == r.watch_heard,
                "saved, {s} s ending at {t} s"
            );
            assert!(m.watch_heard || m.rate_s_per_day.is_none());
        }
        t += 1.0;
    }
}

/// The first `seconds` of a recording.
fn cut(mut a: Audio, seconds: f64) -> Audio {
    a.samples
        .truncate((seconds * a.sample_rate as f64) as usize);
    a
}

/// Stop lengths the under-10 s rule is tried at: every second from 1 to
/// 12 s, and either side of the 10 s edge (Norm on #2333).
const SHORT_STOPS: [f64; 16] = [
    1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 9.9, 10.0, 10.1, 10.5, 11.0, 12.0,
];

/// Whether a reading has no verdict either way: no "no watch" caveat.
fn no_verdict(caveats: &[Caveat]) -> bool {
    timegrapher_core::measure::no_watch_reason_in(caveats).is_none()
}

/// Under 10 s of sound heard there are no figures and no verdict either
/// way, on every reader, however the beats sound: a watch whose beats
/// sound plainly, and an empty stand's hiss, read alike until 10 s, then
/// the watch reads and the hiss is no watch (Norm on #2333, revising
/// #2014, where the beats' sound spoke sooner and a take stopped at
/// 1.5 s read a rate from 11 beats while live still waited). Read whole
/// by `analyze`, saved by `session`, live at every Average Over, and
/// opened (the log replayed), as the app reads a stopped take.
#[test]
fn under_ten_seconds_heard_reads_no_figures_and_no_verdict_anywhere() {
    use timegrapher_core::live::AVERAGES;
    for seconds in SHORT_STOPS {
        let waits = seconds < 10.0;
        for (what, a) in [
            ("watch", cut(watch(17), seconds)),
            ("stand", cut(empty_stand(), seconds)),
        ] {
            let at = format!("{what} {seconds} s");
            let is_watch = what == "watch";
            let path = on_disk(&a, &format!("t5w{what}{seconds}"));
            let log = stream(&path, 47.0);
            std::fs::remove_file(&path).ok();
            let s = analyze(&a, &AnalysisConfig::default()).summary;
            let m = whole(&log);
            let opened = LiveAnalyzer::from_log(log.clone(), LiveConfig::default());
            let o = opened.reading(log.duration_s);
            if waits {
                assert!(
                    s.overall.is_none() && no_verdict(&s.caveats),
                    "{at} analyze: {:?}",
                    s.caveats
                );
                assert!(
                    m.rate_s_per_day.is_none()
                        && m.amplitude_deg.is_none()
                        && no_verdict(&m.caveats),
                    "{at} session: {:?}",
                    m.caveats
                );
                assert!(
                    o.rate_s_per_day.is_none()
                        && o.amplitude_deg.is_none()
                        && no_verdict(&o.caveats),
                    "{at} opened: {:?}",
                    o.caveats
                );
                // Where the watch's beats are read, the reading says it
                // waits: "Under 10 s Heard" on a stopped or opened take.
                assert!(
                    o.beats_used == 0 || o.listening.is_some(),
                    "{at} opened: {o:?}"
                );
            } else if is_watch {
                assert!(
                    s.watch_heard && s.overall.is_some(),
                    "{at} analyze: {:?}",
                    s.caveats
                );
                assert!(m.watch_heard && m.rate_s_per_day.is_some(), "{at} session");
                assert!(
                    o.watch_heard && o.rate_s_per_day.is_some() && o.listening.is_none(),
                    "{at} opened: {:?}",
                    o.caveats
                );
            } else {
                assert!(
                    !s.watch_heard && s.overall.is_none() && !no_verdict(&s.caveats),
                    "{at} analyze: {:?}",
                    s.caveats
                );
                assert!(!m.watch_heard && m.rate_s_per_day.is_none(), "{at} session");
                assert!(!o.watch_heard && o.rate_s_per_day.is_none(), "{at} opened");
            }
            // Live listening, at every Average Over: no figures and no
            // verdict while under 10 s is heard; the stand never reads.
            let live = pushed(&a, 4800);
            for average in AVERAGES {
                let r = live.reading_at(a.duration(), average);
                let at = format!("{at} live at {average} s");
                assert!(
                    r.watch_heard || r.rate_s_per_day.is_none(),
                    "{at}: {:?}",
                    r.caveats
                );
                if waits {
                    assert!(
                        r.rate_s_per_day.is_none()
                            && r.amplitude_deg.is_none()
                            && r.beat_error_ms.is_none()
                            && no_verdict(&r.caveats),
                        "{at}: {r:?}"
                    );
                }
            }
        }
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

/// Kept as its analysis while it is recorded, a real take is kept as when
/// its sound is kept and measured after, every file of `analysis/` byte
/// for byte (#1514): the sound pushed in blocks of an odd size, as it
/// comes from a sound card, with one stretch's sound shape kept.
#[test]
fn kept_while_recorded_matches_kept_after_on_real_takes() {
    use timegrapher_core::analysis_take::{files, measure};
    use timegrapher_core::keeper::{KeepAs, Keeper};
    for path in corpus_takes(&[LIVE_TAKES[1], LIVE_TAKES[3]]) {
        let cfg = timegrapher_core::stream::StreamConfig::default();
        let software = serde_json::json!({"name": "timegrapher", "version": "test"});
        let shapes = vec![Some((300.0, 360.0))];
        let after = measure(&[path.as_path()], &cfg, &shapes, software.clone(), |_| {}).unwrap();
        let info = timegrapher_core::audio::info(&path).unwrap();
        let keeper = Keeper::start(KeepAs {
            sample_rate: info.sample_rate,
            bytes_per_frame: info.bytes_per_frame,
            cfg: cfg.clone(),
            shapes,
            software,
            checkpoints: None,
        });
        timegrapher_core::audio::stream(&path, 4097, |b| assert!(keeper.push(b))).unwrap();
        let live = keeper.finish().unwrap();
        eprintln!(
            "{}: {} beats, {} jumps, {} profiles",
            path.display(),
            after.log.beats.len(),
            after.header.jumps.len(),
            after.profiles.len()
        );
        let (a, b) = (files(&after).unwrap(), files(&live).unwrap());
        assert_eq!(a.len(), b.len());
        for ((name, x), (_, y)) in a.iter().zip(&b) {
            assert!(x == y, "{}: {name} differs", path.display());
        }
    }
}

/// The take that runs down: its second sound file, about 600 to 1200 s
/// into the excerpt, where the beats jump 14 times (`series` on 2fedcb8).
const DYING_TAKE: &str = "ym42_DU_65h_excerpt_dying";
const DYING_FILE: &str = "take_part01-002.flac";

/// Kept while recorded, a stretch of real sound where the beats jump again
/// and again is kept as when it is measured after, every file byte for
/// byte (#1514). Runs where `TIMEGRAPHER_CORPUS` is set.
#[test]
fn kept_while_recorded_matches_kept_after_where_the_beats_jump() {
    use timegrapher_core::analysis_take::{files, measure};
    use timegrapher_core::keeper::{KeepAs, Keeper};
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(
            std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_none(),
            "TIMEGRAPHER_CORPUS is not set"
        );
        eprintln!("SKIP kept where the beats jump: TIMEGRAPHER_CORPUS not set");
        return;
    };
    let path = root.join("takes").join(DYING_TAKE).join(DYING_FILE);
    let cfg = timegrapher_core::stream::StreamConfig::default();
    let software = serde_json::json!({"name": "timegrapher", "version": "test"});
    let shapes = vec![Some((60.0, 120.0))];
    let after = measure(&[path.as_path()], &cfg, &shapes, software.clone(), |_| {}).unwrap();
    let info = timegrapher_core::audio::info(&path).unwrap();
    let keeper = Keeper::start(KeepAs {
        sample_rate: info.sample_rate,
        bytes_per_frame: info.bytes_per_frame,
        cfg: cfg.clone(),
        shapes,
        software,
        checkpoints: None,
    });
    timegrapher_core::audio::stream(&path, 4097, |b| assert!(keeper.push(b))).unwrap();
    let live = keeper.finish().unwrap();
    eprintln!(
        "{}: {} beats, jumps at {:?}, {} profiles",
        path.display(),
        after.log.beats.len(),
        after.header.jumps,
        after.profiles.len()
    );
    assert!(after.header.jumps.len() >= 5, "{:?}", after.header.jumps);
    // Every beat of the kept beat sound finds its beat across the jumps,
    // and reads back in place (#2069, Norm).
    let dir = std::env::temp_dir().join(format!("tg-eq-kept-dying-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    timegrapher_core::analysis_take::write(&dir, &after).unwrap();
    let back = timegrapher_core::analysis_take::read(&dir).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert!(!after.log.snippets.is_empty());
    assert_eq!(back.log.snippets.len(), after.log.snippets.len());
    for (a, b) in after.log.snippets.iter().zip(&back.log.snippets) {
        assert!((a.0 - b.0).abs() <= 0.5e-6 + 1e-9, "{} {}", a.0, b.0);
        assert!(a.1 == b.1, "the window at {} s", a.0);
    }
    let (a, b) = (files(&after).unwrap(), files(&live).unwrap());
    assert_eq!(a.len(), b.len());
    for ((name, x), (_, y)) in a.iter().zip(&b) {
        assert!(x == y, "{}: {name} differs", path.display());
    }
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
/// Auto again, mid-listen keeps the beats heard so far: every measured figure and
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
        let (n, mut k) = (never.reading(10.0), kept.reading(10.0));
        let t = n.time_s;
        assert_eq!(n.bph_set, None);
        assert_eq!(k.bph_set, (i < auto).then_some(28_800));
        // Selecting a calibre changes the rate's provenance, not its figures.
        // Compare every other field exactly after checking that distinction.
        k.bph_set = n.bph_set;
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

/// Read again as live reads what it has heard (#2261): at each ask,
/// before the first chunk is whole, across its end and up to and past
/// two chunks, a read resumed from the last read's state is a full read's
/// log to the bit, every field. Runs where `TIMEGRAPHER_CORPUS` is set.
#[test]
fn a_resumed_read_of_real_sound_is_a_full_reads_to_the_bit() {
    use timegrapher_core::stream::{analyze_samples, analyze_samples_resumed};
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(
            std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_none(),
            "TIMEGRAPHER_CORPUS is not set"
        );
        eprintln!("SKIP resumed reads: TIMEGRAPHER_CORPUS not set");
        return;
    };
    let cfg = StreamConfig::default();
    for take in ["ym42_DD_30min", "eta2824_holder_DU_30min"] {
        let audio = cut(
            load(&root.join("takes").join(take).join(REAL_FILE)).unwrap(),
            100.0,
        );
        let fs = audio.sample_rate;
        let mut from = None;
        let mut resumed = 0;
        for s in [30.0, 47.5, 48.0, 48.5, 70.0, 93.9, 94.0, 94.5, 100.0] {
            let x = &audio.samples[..((s * fs as f64) as usize).min(audio.samples.len())];
            let had = from.clone();
            let (log, next) = analyze_samples_resumed(x, fs, &cfg, from.take());
            resumed += usize::from(
                had.as_ref()
                    .zip(next.as_ref())
                    .is_some_and(|(a, b)| std::sync::Arc::ptr_eq(a, b)),
            );
            let full = analyze_samples(x, fs, &cfg);
            assert!(!full.beats.is_empty(), "{take} to {s} s");
            assert!(
                format!("{log:?}") == format!("{full:?}"),
                "{take} to {s} s: the resumed log differs from a full read's"
            );
            from = next;
        }
        assert_eq!(resumed, 6, "{take}: resumed at every ask past 48 s");
    }
}

/// The silent-start take (corpus 1c35570): about 10 s of exact zeros, about
/// 70 s of the empty stand, then the Yacht-Master 42 replica (#2189, #2191).
const SILENT_START_TAKE: &str = "fakemic_silentstart_stand_ym42_DU";
/// Its one file's SHA-256 at corpus 1c35570.
const SILENT_START_SHA256: &str =
    "313d25b9687a0080a0a4811e26a2d75077f693dfae31ffa25dcb0e0501d18fc6";

/// A take that opens on exact zeros, then stand noise, read at a given
/// 28,800 bph leaves the lead out and reads the watch's rate, exactly as
/// Auto does (#2189, fixed by #2190). Norm's acceptance on #2191, set
/// before any run: the lead left out lies in 75..=85 s, the rate is
/// +52.54 ± 0.1 s/d, and the two reads agree exactly. Runs where
/// `TIMEGRAPHER_CORPUS` is set.
#[test]
fn a_silent_start_take_leaves_its_lead_out_at_a_given_rate_as_on_auto() {
    let Some(path) = corpus_takes(&[SILENT_START_TAKE]).pop() else {
        return;
    };
    let hash = timegrapher_core::archive::sha256_hex(&std::fs::read(&path).unwrap());
    assert_eq!(
        hash, SILENT_START_SHA256,
        "the file pinned at corpus 1c35570"
    );
    let audio = load(&path).unwrap();
    let read = |bph| {
        let s = analyze(
            &audio,
            &AnalysisConfig {
                bph,
                ..Default::default()
            },
        )
        .summary;
        let lead = s.caveats.iter().find_map(|c| match c {
            Caveat::LeadLeftOut { seconds } => Some(*seconds),
            _ => None,
        });
        eprintln!(
            "bph {bph:?}: lead left out {lead:?} s, rate {:?} s/d",
            s.rate_s_per_day
        );
        (lead, s.rate_s_per_day)
    };
    let given = read(Some(28_800));
    let auto = read(None);
    let (Some(lead), Some(rate)) = given else {
        panic!("given 28,800 bph: no lead left out or no rate: {given:?}");
    };
    assert!((75.0..=85.0).contains(&lead), "lead left out {lead} s");
    assert!((rate - 52.54).abs() <= 0.1, "rate {rate} s/d");
    assert!(given == auto, "given {given:?} against Auto {auto:?}");
}

fn corpus_files() -> Vec<PathBuf> {
    corpus_takes(&REAL_TAKES)
}

/// [`corpus_files`] for the takes `ids`.
fn corpus_takes(ids: &[&str]) -> Vec<PathBuf> {
    let root = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from);
    corpus_takes_at(
        root.as_deref(),
        std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some(),
        ids,
    )
    .unwrap_or_else(|e| panic!("required real fixtures: {e}"))
}

/// Resolve and authenticate exact fixture names before running any comparison.
/// Explicitly supplied corpora never silently shrink to the available subset.
fn corpus_files_at(root: Option<&Path>, required: bool) -> Result<Vec<PathBuf>, String> {
    corpus_takes_at(root, required, &REAL_TAKES)
}

/// [`corpus_files_at`] for the takes `ids`.
fn corpus_takes_at(
    root: Option<&Path>,
    required: bool,
    ids: &[&str],
) -> Result<Vec<PathBuf>, String> {
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
    for &id in ids {
        let members: Vec<_> = takes.iter().filter(|t| t["id"] == id).collect();
        if members.len() != 1
            || !matches!(
                members[0]["tier"].as_str(),
                Some("referenced" | "paired" | "smoke")
            )
        {
            return Err(format!(
                "corpus.json: expected one development reference or smoke take {id}"
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

// 4. The faint-side rule, live and saved.

/// A watch whose amplitude swings 140° to 260° inside each 2 s window for
/// a stretch, which smears those windows' sound 1 so the faint-side rule
/// withholds some of their sides (#1738), steady at 270° around it.
fn smeared(seed: u64) -> Audio {
    let swung = |t: f64| (60.0..150.0).contains(&t);
    generate(
        &SynthConfig {
            duration_s: 240.0,
            snr_db: 26.0,
            seed,
            ..Default::default()
        },
        move |t| {
            if swung(t) {
                200.0 + 60.0 * (t * 1.7).sin()
            } else {
                270.0
            }
        },
        |_| 0.0,
    )
}

/// Saved and live judge the amplitude windows alike (Norm on #1800):
/// read to the end and settled at the stop, live withholds the same sides
/// as too faint to read, on one chunk grid
/// ([`timegrapher_core::stream::amplitude_chunk_of`]), and leaves out the
/// same sides as outliers after that, window for window. Live reads every
/// window but the last, which waits for a beat's guard of sound after it.
/// Which sides a window found edges for at all is the read's, not this
/// judging's, and is not compared here.
#[test]
fn saved_and_live_withhold_the_same_faint_sides() {
    let a = smeared(1738);
    let path = on_disk(&a, "faint");
    let saved = stream(&path, 47.0);
    std::fs::remove_file(&path).ok();
    let mut live = pushed(&a, 4_800);
    live.settle();
    type Judged = (i64, [bool; 2], [bool; 2]);
    let judged = |ws: &[timegrapher_core::amplitude::AmplitudeWindow]| -> Vec<Judged> {
        ws.iter()
            .map(|w| {
                (
                    (w.start_s * 1000.0).round() as i64,
                    w.faint_withheld(),
                    w.outliers().map(|d| d.is_some()),
                )
            })
            .collect()
    };
    let (s, l) = (
        judged(&saved.amplitude_windows),
        judged(live.amplitude_windows()),
    );
    let faint = |v: &[Judged]| v.iter().filter(|j| j.1.contains(&true)).count();
    let out = |v: &[Judged]| v.iter().filter(|j| j.2.contains(&true)).count();
    assert!(faint(&s) > 0, "the smear withholds sides");
    assert!(out(&s) > 0, "the smear leaves out an outlier");
    assert_eq!(l.len() + 1, s.len(), "live reads all but the last window");
    assert_eq!(&s[..l.len()], &l[..]);
}

/// A live read stopped in the middle of a chunk withholds nothing in it
/// until [`LiveAnalyzer::settle`], then the same sides as a saved read of
/// the same sound, whose last chunk is that one (#1738, Trevor on #1800).
#[test]
fn settle_judges_the_open_chunk_as_a_saved_read_does() {
    let whole = smeared(1738);
    let cut = Audio {
        samples: whole.samples[..170 * whole.sample_rate as usize].to_vec(),
        sample_rate: whole.sample_rate,
    };
    let path = on_disk(&cut, "faint-cut");
    let saved = stream(&path, 47.0);
    std::fs::remove_file(&path).ok();
    let mut live = pushed(&cut, 4_800);
    let w = AnalysisConfig::default().amplitude_window_s;
    let chunk_of = |s: f64| timegrapher_core::stream::amplitude_chunk_of(s, w);
    let faint = |ws: &[timegrapher_core::amplitude::AmplitudeWindow]| -> Vec<(i64, [bool; 2])> {
        ws.iter()
            .filter(|w| w.faint_withheld().contains(&true))
            .map(|w| ((w.start_s * 1000.0).round() as i64, w.faint_withheld()))
            .collect()
    };
    let open = chunk_of(live.amplitude_windows().last().expect("windows").start_s);
    let in_open = |v: Vec<(i64, [bool; 2])>| -> Vec<(i64, [bool; 2])> {
        v.into_iter()
            .filter(|f| chunk_of(f.0 as f64 / 1000.0) == open)
            .collect()
    };
    assert!(
        in_open(faint(live.amplitude_windows())).is_empty(),
        "nothing judged before the stop"
    );
    assert!(
        !in_open(faint(&saved.amplitude_windows)).is_empty(),
        "the open chunk has a faint side"
    );
    live.settle();
    assert_eq!(
        faint(live.amplitude_windows()),
        faint(&saved.amplitude_windows)
    );
}

/// Twenty minutes of an empty stand, cut from the end of a run-down: its
/// README says "No watch: the stand's noise only".
const EMPTY_STAND_TAKE: &str = "ym42_DU_65h_excerpt_emptystand";

/// An empty stand is no watch on every path a take is read on (#1965): as
/// a whole, live as its sound comes in, and opened, where the engine is
/// built from its beat log. Opened, one 10 s reading at 390 s scattered
/// 0.78% of a beat at 18,000 bph, under the 0.8% from which the beat sound
/// was asked, and the stopped take's Rate card showed it: −183.6 s/d and
/// 0.14 ms. In its first 8 s, opened, it read +120,000 s/d (#2014).
/// Every reading, every 5 s from 1 s, at every Average Over: at 2 and 4 s,
/// opened, it read 125,000 s/d and its strip drew a watch (#1965).
///
/// Runs where `TIMEGRAPHER_CORPUS` is set.
#[test]
fn an_empty_stand_is_no_watch_however_it_is_read() {
    let Some(path) = corpus_takes(&[EMPTY_STAND_TAKE]).pop() else {
        return;
    };
    let log = stream(&path, 47.0);
    assert!(!whole(&log).watch_heard, "the take as a whole");
    let audio = load(&path).unwrap();
    let pushed = pushed(&audio, audio.sample_rate as usize / 10);
    let opened = LiveAnalyzer::from_log(log.clone(), LiveConfig::default());
    for (how, live) in [("live", &pushed), ("opened", &opened)] {
        // From the first second: under 10 s of sound a reading gives no
        // figures, so no rate (#2014).
        // Each second up to 10 s, where regularity isn't judged yet and
        // only the figures are checked, then every 5 s.
        let mut t = 1.0;
        while t <= live.duration_s() {
            for average_s in timegrapher_core::live::AVERAGES {
                let r = live.reading_at(t, average_s);
                let figures = [r.rate_s_per_day, r.amplitude_deg, r.beat_error_ms];
                assert!(
                    (t < 10.0 || !r.watch_heard) && figures == [None; 3],
                    "{how}, {average_s} s ending at {t} s: {:?} s/d, scatter {:?}",
                    r.rate_s_per_day,
                    r.timing_scatter_fraction
                );
            }
            t += if t < 10.0 { 1.0 } else { 5.0 };
        }
    }
    short_averages_agree(&[("live", &pushed), ("opened", &opened)], &log, &opened);
    // The charts' run, the strip's verdict: no point of it hears a watch,
    // at any Average Over. At 2 s and 4 s, opened, the first seconds'
    // readings waiting for their verdict counted as heard, and the strip
    // drew the noise as a watch (#1965). Opened as the app opens it too,
    // at the take's 28,800 bph and 55° from its session.json.
    let mut cfg = LiveConfig::default();
    cfg.analysis.bph = Some(28_800);
    cfg.analysis.amplitude.lift_deg = 55.0;
    let set = LiveAnalyzer::from_log(log, cfg);
    for (how, live) in [
        ("live", &pushed),
        ("opened", &opened),
        ("opened at 28800", &set),
    ] {
        for average_s in timegrapher_core::live::AVERAGES {
            let heard: Vec<f64> = live
                .trend(average_s)
                .iter()
                .filter(|p| p.heard)
                .map(|p| p.t)
                .collect();
            assert!(
                heard.is_empty(),
                "{how}, {average_s} s: heard at {heard:?} s"
            );
        }
    }
}

/// A take that starts and ends on the empty stand, a watch heard between,
/// as the commonest first use goes (press Listen, put the watch on, take it
/// off, Stop): read as one stretch, as the app reads its whole take and a
/// Positions Test row (`session::measure_together`), its lead of noise is
/// left out, as `analyze` and a take's entries leave it out (#844). It read
/// +449 s/d in the app's whole take, against +50.5 s/d everywhere else
/// (#2177). The reading is the watch's stretch alone, with the caveat.
///
/// Runs where `TIMEGRAPHER_CORPUS` is set.
#[test]
fn a_take_between_two_empty_stands_reads_the_watch_alone() {
    let paths = corpus_takes(&["silence_empty_stand", "ym42_DU_25min_newstand"]);
    let [stand, watch] = paths.as_slice() else {
        return;
    };
    let (stand, watch) = (load(stand).unwrap(), load(watch).unwrap());
    assert_eq!(stand.sample_rate, watch.sample_rate);
    let fs = stand.sample_rate as usize;
    let mut samples = stand.samples[..70 * fs].to_vec();
    samples.extend_from_slice(&watch.samples[..100 * fs]);
    samples.extend_from_slice(&stand.samples[..60 * fs]);
    let audio = Audio {
        samples,
        sample_rate: stand.sample_rate,
    };
    let path = on_disk(&audio, "t2177");
    let log = stream(&path, 47.0);
    std::fs::remove_file(&path).ok();
    let d = log.duration_s;
    let m = session::measure_together(&log, None, &[], &[], (&[(0.0, d)], &[(0.0, d)])).unwrap();
    let lead = session::lead_s(&log, None, &[], &[], 0.0, d).expect("a lead of noise");
    // All of the stand left out, and the watch's stretch kept.
    assert!((69.0..120.0).contains(&lead), "the watch from {lead} s");
    assert!(
        m.caveats
            .iter()
            .any(|c| matches!(c, Caveat::LeadLeftOut { seconds } if *seconds == lead)),
        "{:?}",
        m.caveats
    );
    // Read from a later start, as a take's entry reads from its settle's
    // end, the caveat still counts from where the entry starts.
    let settled = session::measure_from_lead(&log, None, &[], &[], (20.0, d), &[], 0.0);
    assert!(
        settled
            .caveats
            .iter()
            .any(|c| matches!(c, Caveat::LeadLeftOut { seconds } if *seconds == lead)),
        "{:?}",
        settled.caveats
    );
    let alone = session::measure(&log, None, lead, d);
    assert_eq!(
        m.rate_s_per_day.map(f64::to_bits),
        alone.rate_s_per_day.map(f64::to_bits),
        "{:?} s/d over the whole take, {:?} s/d from the watch on",
        m.rate_s_per_day,
        alone.rate_s_per_day
    );
    let watch_only = session::measure(&log, None, lead, 169.0);
    let (r, w) = (
        m.rate_s_per_day.unwrap(),
        watch_only.rate_s_per_day.unwrap(),
    );
    assert!((r - w).abs() < 3.0, "{r} s/d against the watch's {w} s/d");
}

/// A clean watch read in two stretches with the time between them masked,
/// as a Positions Test reads a position heard twice: no lead of noise is
/// found, and nothing before the mask is left out. Looked for across the
/// mask, the watch's first window, its pair on the far side, read as a
/// lead up to the mask's end (#2187).
#[test]
fn a_mask_is_not_a_lead_of_noise() {
    let audio = generate(
        &SynthConfig {
            duration_s: 60.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let path = on_disk(&audio, "t2187mask");
    let log = stream(&path, 47.0);
    std::fs::remove_file(&path).ok();
    let masked = [(15.0, 40.0)];
    assert_eq!(session::lead_s(&log, None, &[], &masked, 5.0, 60.0), None);
    let spans = [(5.0, 20.0), (35.0, 60.0)];
    let m = session::measure_together(&log, None, &[], &masked, (&spans, &spans)).unwrap();
    assert!(
        !m.caveats
            .iter()
            .any(|c| matches!(c, Caveat::LeadLeftOut { .. })),
        "{:?}",
        m.caveats
    );
    assert!((m.masked_s - 25.0).abs() < 1e-9, "{}", m.masked_s);
    // With the beats' jumps inside the mask and at its ends, where the
    // app's reading cuts sit (a stretch's end at 20 s, the next stretch's
    // start at 35 s): those are in the sound the mask leaves out, so still
    // no lead (Vernon 4 on #2187).
    let mut cut = log.clone();
    cut.jumps = vec![15.0, 20.0, 35.0, 40.0];
    assert_eq!(session::lead_s(&cut, None, &[], &masked, 5.0, 60.0), None);
}

/// A take that opens with 70 s of the stand's noise, then the watch, with
/// a 5 s mask by hand in the noise, just after the watch arrives, or well
/// into it: the lead is found as with no mask, and the whole reading is
/// the watch's. Stopping the lead's search at the first mask found none
/// for a mask in the first minute, and the reading took in the noise,
/// −2,253 and −3,829 s/d against −58.7 (Norm 4 on #2187).
///
/// Runs where `TIMEGRAPHER_CORPUS` is set.
#[test]
fn a_mask_by_hand_keeps_the_lead_of_noise_out() {
    let paths = corpus_takes(&["silence_empty_stand", "ym42_DU_25min_newstand"]);
    let [stand, watch] = paths.as_slice() else {
        return;
    };
    let (stand, watch) = (load(stand).unwrap(), load(watch).unwrap());
    let fs = stand.sample_rate as usize;
    let mut samples = stand.samples[..70 * fs].to_vec();
    samples.extend_from_slice(&watch.samples[..300 * fs]);
    let audio = Audio {
        samples,
        sample_rate: stand.sample_rate,
    };
    let path = on_disk(&audio, "t2187masks");
    let log = stream(&path, 47.0);
    std::fs::remove_file(&path).ok();
    let d = log.duration_s;
    let read = |masked: &[(f64, f64)]| {
        let lead = session::lead_s(&log, None, &[], masked, 0.0, d);
        let m = session::measure_together(&log, None, &[], masked, (&[(0.0, d)], &[(0.0, d)]));
        (lead, m.unwrap().rate_s_per_day.unwrap())
    };
    let (lead, rate) = read(&[]);
    let lead = lead.expect("a lead of noise");
    // In the noise, over the watch going on the stand, just after it, and
    // well into it (Norm 4's and Vernon 4's cases on #2187).
    for mask in [
        (30.0, 35.0),
        (66.0, 74.0),
        (80.0, 85.0),
        (100.0, 105.0),
        (200.0, 205.0),
    ] {
        let (l, r) = read(&[mask]);
        let l = l.unwrap_or_else(|| panic!("mask {mask:?}: no lead, {r} s/d"));
        if mask.1 < lead {
            assert_eq!(l, lead, "mask {mask:?}");
        } else {
            // Inside the watch: the lead ends where it did, or at the
            // mask's end where the mask took the watch's first windows.
            assert!(
                (lead..=mask.1 + 1.0).contains(&l),
                "mask {mask:?}: lead {l} s"
            );
        }
        // The watch's own reading from there, the mask left out.
        let alone = session::measure_masked(&log, None, &[], &[mask], l, d);
        let want = alone.rate_s_per_day.unwrap();
        assert!(
            (r - want).abs() < 0.1,
            "mask {mask:?}: {r} s/d against {want}"
        );
        assert!(
            (r - rate).abs() < 3.0,
            "mask {mask:?}: {r} s/d against {rate}"
        );
    }
}

/// A watch put on the stand after 40 s of its noise, heard live: once its
/// beat is being found, every later reading finds it or reads it, at every
/// Average Over, never "No Regular Beat" between. At 1 min its "Finding the
/// Beat" ended 20 s in while the minute judged still held the noise, and
/// for 10 to 15 s the cards said "No Regular Beat" and its dots left the
/// strip (#2178). Each second from 1 s.
///
/// Runs where `TIMEGRAPHER_CORPUS` is set.
#[test]
fn an_arriving_watch_is_found_until_it_is_read() {
    let paths = corpus_takes(&["silence_empty_stand", "ym42_DU_25min_newstand"]);
    let [stand, watch] = paths.as_slice() else {
        return;
    };
    let (stand, watch) = (load(stand).unwrap(), load(watch).unwrap());
    let fs = stand.sample_rate as usize;
    let mut samples = stand.samples[..40 * fs].to_vec();
    samples.extend_from_slice(&watch.samples[..120 * fs]);
    let audio = Audio {
        samples,
        sample_rate: stand.sample_rate,
    };
    let live = pushed(&audio, fs / 10);
    for average_s in timegrapher_core::live::AVERAGES {
        let mut found = None;
        let mut t = 1.0;
        while t <= live.duration_s() {
            let r = live.reading_at(t, average_s);
            let waits = r.finding_beat || r.watch_heard;
            if found.is_none() && r.finding_beat && t > 40.0 {
                found = Some(t);
            }
            if let Some(from) = found {
                assert!(
                    waits,
                    "{average_s} s: found at {from} s, No Regular Beat at {t} s"
                );
            }
            t += 1.0;
        }
        let read = live.reading_at(live.duration_s(), average_s);
        assert!(
            read.watch_heard && read.rate_s_per_day.is_some(),
            "{average_s} s"
        );
    }
}

/// Kept as its analysis alone, an empty stand still reads no watch, with
/// no figures in its first seconds (#2014), now that the beats' sound the
/// verdict reads is kept with it (#2069): as opened with its sound, every
/// second up to 10 s and every 5 s after, at every Average Over (#1965).
/// The beat sound kept stays within its budget, whatever the length.
///
/// Runs where `TIMEGRAPHER_CORPUS` is set.
#[test]
fn an_empty_stand_kept_as_its_analysis_is_still_no_watch() {
    use timegrapher_core::analysis_take::{measure, read, write};
    let Some(path) = corpus_takes(&[EMPTY_STAND_TAKE]).pop() else {
        return;
    };
    let cfg = StreamConfig::default();
    let software = serde_json::json!({"name": "timegrapher", "version": "test"});
    let s = measure(&[path.as_path()], &cfg, &[], software, |_| {}).unwrap();
    let dir = std::env::temp_dir().join(format!("tg-eq-kept-stand-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    write(&dir, &s).unwrap();
    let kept = read(&dir).unwrap().log;
    std::fs::remove_dir_all(&dir).ok();
    let budget = timegrapher_core::sound_sample::SOUND_SAMPLE_BEATS;
    let len = timegrapher_core::profile::template_len(
        kept.sample_rate as f64,
        timegrapher_core::amplitude::UnlockWindow::SHORT,
    );
    assert!(!kept.snippets.is_empty() && kept.snippets.len() <= budget);
    assert!(kept.snippets.iter().all(|w| w.1.len() == len));
    assert_eq!(kept.snippets.len(), s.log.snippets.len());
    assert!(!whole(&kept).watch_heard, "the kept take as a whole");
    let sound = LiveAnalyzer::from_log(stream(&path, 47.0), LiveConfig::default());
    let opened = LiveAnalyzer::from_log(kept, LiveConfig::default());
    let mut t = 1.0;
    while t <= opened.duration_s() {
        for average_s in timegrapher_core::live::AVERAGES {
            let (a, r) = (
                sound.reading_at(t, average_s),
                opened.reading_at(t, average_s),
            );
            let figures = [r.rate_s_per_day, r.amplitude_deg, r.beat_error_ms];
            assert!(
                (t < 10.0 || !r.watch_heard) && figures == [None; 3],
                "kept, {average_s} s ending at {t} s: {:?} s/d",
                r.rate_s_per_day
            );
            assert_eq!(r.watch_heard, a.watch_heard, "{average_s} s at {t} s");
            assert_eq!(r.listening, a.listening, "{average_s} s at {t} s");
        }
        t += if t < 10.0 { 1.0 } else { 5.0 };
    }
}

/// An empty stand is never a beat being found (#2008, Norm on #2013):
/// "Finding the Beat" says a regular beat is heard, so a run of stand
/// noise after a pause, heard on its own, must fail the rule every reader
/// uses, the beat sound's veto included. Every second at a 10 s average,
/// live and opened.
///
/// Runs where `TIMEGRAPHER_CORPUS` is set.
#[test]
fn an_empty_stand_is_never_a_beat_being_found() {
    let Some(path) = corpus_takes(&[EMPTY_STAND_TAKE]).pop() else {
        return;
    };
    let log = stream(&path, 47.0);
    let audio = load(&path).unwrap();
    let pushed = pushed(&audio, audio.sample_rate as usize / 10);
    let opened = LiveAnalyzer::from_log(log, LiveConfig::default());
    for (how, live) in [("live", &pushed), ("opened", &opened)] {
        // At every Average Over: the wait is longer at 30 s and 1 min
        // (`live::finding_s`, #2178).
        for average_s in timegrapher_core::live::AVERAGES {
            let mut t = 1.0;
            while t <= live.duration_s() {
                let r = live.reading_at(t, average_s);
                assert!(
                    !r.finding_beat,
                    "{how}, {average_s} s ending at {t} s: scatter {:?}",
                    r.timing_scatter_fraction
                );
                t += 1.0;
            }
        }
    }
}

/// The take #1721 was found on, in its holder: its USB lines put its card
/// 19.3 ppm slow, 1.67 s/d.
const USB_TAKE: &str = "eta2824_holder_DU_30min";

/// At `t`, live's rate on the clock and on the card against the saved
/// take's report and the saved take opened in the app: read as a file,
/// the saved take opened to the bit, the report's to its last few bits
/// (it puts the clock on the beat times, live on the rate); after, within
/// half the 0.1 s/d digit.
fn live_is_saved(
    t: f64,
    as_file: bool,
    (live_on, live_card): (f64, f64),
    (saved_on, saved_card): (f64, f64),
    opened: (Option<f64>, Option<f64>),
) {
    if as_file {
        assert_eq!(
            (Some(live_on), Some(live_card)),
            opened,
            "{t} s: read as a file, live is the saved take opened"
        );
        assert_eq!(live_card.to_bits(), saved_card.to_bits(), "{t} s");
        assert!((live_on - saved_on).abs() < 1e-9, "{t} s");
    } else {
        assert!(
            (live_on - saved_on).abs() < 0.05,
            "{t} s: live {live_on:.3} s/d against the saved take's {saved_on:.3}"
        );
    }
}

/// Heard live, a real take's readings are on the clock the same take
/// saved and opened at that moment is read on (#1721, #1918), with the
/// USB lines read from the sound as it comes in, never put in by hand.
/// Under 30 s the lines aren't read yet, and the rate says so rather than
/// being compared. At each length a live read is due:
/// - the lines are the ones a take saved then reads, to the bit, and the
///   clock is the one it opens on;
/// - the clock moves the live rate exactly as far as it moves the saved
///   take's (1.66 s/d here);
/// - stopped there, while live's figures are read as a file (before the
///   template is built again), the live rate is the saved take's to the
///   bit, on the clock and on the card's (#1987);
/// - after, the live rate is the saved take's to within half its 0.1 s/d
///   digit, at every length (#1987 took away the first minute's gap).
///
/// Runs where `TIMEGRAPHER_CORPUS` is set.
#[test]
fn a_live_rate_takes_the_usb_lines_a_reopened_real_take_takes() {
    use timegrapher_core::usbclock::{self, LiveClock};
    use timegrapher_core::{clock, excerpt, take};
    let Some(path) = corpus_takes(&[USB_TAKE]).pop() else {
        return;
    };
    let pcm = excerpt::read(&path).unwrap();
    let a = load(&path).unwrap();
    let fs = a.sample_rate;
    let block = fs as usize / 10;
    let mut l = LiveAnalyzer::new(fs, LiveConfig::default()).with_figures_as_file();
    let mut lines = usbclock::Live::new(fs);
    // One length before the first read, then each length a read is due.
    let mut checks = [20.0].into_iter().chain(usbclock::LIVE_READS_S).peekable();
    let dir = std::env::temp_dir().join(format!("tg-eq-usb-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (i, c) in a.samples.chunks(block).enumerate() {
        // As the app follows them: the lines read when due, and the
        // readings put on the clock they give.
        l.push(c);
        lines.push(c);
        let heard = lines.measure_due();
        let fit = take::live_clock(&heard, None, l.duration_s());
        l.set_rate_clock(fit.as_ref().filter(|f| f.corrects()));
        let t = ((i + 1) * block) as f64 / f64::from(fs);
        if checks.next_if(|&at| t >= at).is_none() {
            continue;
        }
        if t < usbclock::MIN_DURATION_S {
            let said = heard
                .not_yet(false)
                .expect("said before the lines are read");
            assert!(said.contains("USB lines"), "{said}");
            assert!(fit.is_none(), "{t} s: on the card's clock until then");
            continue;
        }
        assert_eq!(heard.not_yet(false), None, "{t} s");
        // The take saved now: its sound to this sample, opened again.
        let saved = dir.join(format!("take-{t:.0}.flac"));
        let mut part = excerpt::Pcm {
            samples: pcm.samples[..(t * f64::from(fs)) as usize].to_vec(),
            ..pcm
        };
        part.samples.shrink_to_fit();
        excerpt::write_flac(&saved, &part).unwrap();
        let usb = clock::usb_lines(std::slice::from_ref(&saved)).unwrap();
        let LiveClock::Measured { clock: live, .. } = heard else {
            panic!("{t} s: live read no lines: {heard:?}");
        };
        assert_eq!(Some(live), usb, "{t} s: the lines a saved take reads");
        let opened = take::clock(None, 0, 0, usb.map(|u| u.ppm), None, t)
            .unwrap()
            .expect("the lines' clock");
        assert_eq!(format!("{fit:?}"), format!("{:?}", Some(&opened)), "{t} s");
        // Both over the same interval, as `live_matches_analyze_on_real_takes`.
        let log = stream(&saved, 47.0);
        // Stopped now: what was heard read as a file once more.
        let mut stopped = l.clone();
        stopped.read_as_file_now();
        let as_file = stopped.reads_as_file();
        let end = stopped.reading(t).time_s;
        let last = |v: &[f64]| v.iter().copied().filter(|&b| b < end).fold(0.0, f64::max);
        let from = last(stopped.breaks()).max(last(&log.jumps));
        let rate = |c: Option<&clock::ClockFit>| {
            let mut x = stopped.clone();
            x.set_rate_clock(c);
            x.reading_at(end, end - from)
                .rate_s_per_day
                .expect("a live rate")
        };
        let saved_rate = |c: Option<&clock::ClockFit>| {
            session::measure(&log, c, from, end)
                .rate_s_per_day
                .expect("a saved rate")
        };
        let (live_on, live_card) = (rate(Some(&opened)), rate(None));
        let (saved_on, saved_card) = (saved_rate(Some(&opened)), saved_rate(None));
        eprintln!(
            "{t} s, {from:.1} to {end:.1} s: lines {:.3} ppm; live {live_on:.3} s/d \
             ({live_card:.3} on the card), saved {saved_on:.3} ({saved_card:.3})",
            live.ppm
        );
        let moved = |on: f64, card: f64| on - card;
        assert!(
            (moved(live_on, live_card) - moved(saved_on, saved_card)).abs() < 1e-3,
            "{t} s: the clock moved live {:.4} s/d, the saved take {:.4}",
            moved(live_on, live_card),
            moved(saved_on, saved_card)
        );
        let opened_rate = |c: Option<&timegrapher_core::clock::ClockFit>| {
            let mut x = LiveAnalyzer::from_log(log.clone(), LiveConfig::default());
            x.set_rate_clock(c);
            x.reading_at(end, end - from).rate_s_per_day
        };
        let opened_rates = (opened_rate(Some(&opened)), opened_rate(None));
        live_is_saved(
            t,
            as_file,
            (live_on, live_card),
            (saved_on, saved_card),
            opened_rates,
        );
        std::fs::remove_file(&saved).unwrap();
    }
    assert_eq!(checks.next(), None, "every length was heard");
    std::fs::remove_dir(&dir).unwrap();
}

/// How long, seconds, `samples` take to read as a file reads them.
fn read_time(samples: &[f32], fs: u32) -> f64 {
    let t0 = std::time::Instant::now();
    let _ = timegrapher_core::stream::analyze_samples(samples, fs, &StreamConfig::default());
    t0.elapsed().as_secs_f64()
}

/// A reading's figures, their ±s and the caveats on them, as text: what
/// a read as a file gives (#1987), not the listening state.
fn figures(r: &LiveReading) -> String {
    let listening = |c: &&Caveat| {
        matches!(
            c,
            Caveat::NoWatch { .. } | Caveat::SetRateDisagrees { .. } | Caveat::Clipped { .. }
        )
    };
    let caveats: Vec<&Caveat> = r.caveats.iter().filter(|c| !listening(c)).collect();
    format!(
        "{} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {} {} {} {:?} {:?} {} {} {} {} {} {caveats:?}",
        r.time_s,
        r.rate_s_per_day,
        r.beat_error_ms,
        r.beat_error_unlock_ms,
        r.beat_error_source,
        r.amplitude_deg,
        r.amplitude_error_deg,
        r.amplitude_no_pm,
        r.amplitude_even_deg,
        r.amplitude_odd_deg,
        r.jitter_us,
        r.timing_scatter_fraction,
        r.rate_error_s_per_day,
        r.beats_used,
        r.span_s,
        r.whole,
        r.rate_no_pm,
        r.beat_error_unlock_error_ms,
        r.rate_noise_values,
        r.rate_own_values,
        r.amplitude_values,
        r.amplitude_noise_values,
        r.whole_take,
    )
}

/// Before live builds its template again (#1327), every figure it shows is
/// the saved take's of the sound its reading states (#1987): listening
/// on, the newest read of what was heard, read as a file reads it; and
/// stopped, everything heard, at 10, 20, 30 and 46 s. The stop's read and
/// the lag of the figures behind the sound are timed and printed: about
/// twice a read of the most heard before the template is built again.
///
/// Runs where `TIMEGRAPHER_CORPUS` is set.
#[test]
fn live_figures_before_the_template_is_built_again_are_the_saved_takes() {
    use std::time::Instant;
    use timegrapher_core::excerpt;
    let Some(path) = corpus_takes(&[USB_TAKE]).pop() else {
        return;
    };
    let pcm = excerpt::read(&path).unwrap();
    let a = load(&path).unwrap();
    let fs = a.sample_rate;
    let block = fs as usize / 10;
    let dir = std::env::temp_dir().join(format!("tg-eq-as-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // The first `n` samples saved as a take and opened in the app.
    let opened = |n: usize| {
        let saved = dir.join(format!("take-{n}.flac"));
        let part = excerpt::Pcm {
            samples: pcm.samples[..n].to_vec(),
            ..pcm
        };
        excerpt::write_flac(&saved, &part).unwrap();
        let log = stream(&saved, 47.0);
        std::fs::remove_file(&saved).unwrap();
        LiveAnalyzer::from_log(log, LiveConfig::default())
    };
    // The figures, their ±s and caveats on them: the listening state
    // (the watch heard, the beat being found, SNR, clipping) is live's.
    let same = |live: &LiveAnalyzer, saved: &LiveAnalyzer, what: &str| {
        for average_s in [10.0, 60.0] {
            assert_eq!(
                figures(&live.reading(average_s)),
                figures(&saved.reading(average_s)),
                "{what}, over {average_s} s"
            );
        }
        assert_eq!(
            format!("{:?}", live.trend(10.0)),
            format!("{:?}", saved.trend(10.0)),
            "{what}, the trend"
        );
    };
    let mut l = LiveAnalyzer::new(fs, LiveConfig::default()).with_figures_as_file();
    let mut stops = [10.0, 20.0, 30.0, 46.0].into_iter().peekable();
    let (mut listening, mut heard_most) = (0, 0);
    let (fed_from, mut most_behind) = (Instant::now(), 0.0_f64);
    for (i, c) in a.samples.chunks(block).enumerate() {
        // How far the sound fed is ahead of the clock as this block comes.
        let ahead = (i * block) as f64 / f64::from(fs) - fed_from.elapsed().as_secs_f64();
        l.push(c);
        let n = (i * block + c.len()).min(a.samples.len());
        if l.template_switch_s().is_some() {
            break;
        }
        heard_most = n;
        // Fed ahead of the clock by more than the cap, the push waits, so
        // the figures are within the cap and a block; a feed fallen back to
        // the clock (a slow machine, a stop's read) waits for nothing.
        let lag = n as f64 / f64::from(fs) - l.read_as_file_to().unwrap_or(0.0);
        let cap = READ_LAG_CAP_S + block as f64 / f64::from(fs);
        assert!(
            ahead <= cap || lag <= cap + 1e-9,
            "{n}: the figures {lag} s behind"
        );
        most_behind = most_behind.max(lag);
        if stops
            .next_if(|&at| n as f64 >= at * f64::from(fs))
            .is_none()
        {
            continue;
        }
        let t = n as f64 / f64::from(fs);
        assert!(l.reads_as_file(), "{t} s");
        if let Some(to) = l.read_as_file_to() {
            assert_eq!(l.reading(10.0).time_s, to, "{t} s: states its sound");
            let m = (to * f64::from(fs)).round() as usize;
            same(
                &l,
                &opened(m),
                &format!("listening at {t} s, read to {to} s"),
            );
            listening += 1;
        }
        let mut stopped = l.clone();
        let t0 = Instant::now();
        stopped.settle();
        eprintln!(
            "stopped at {t} s: read in {:.3} s",
            t0.elapsed().as_secs_f64()
        );
        same(&stopped, &opened(n), &format!("stopped at {t} s"));
    }
    assert_eq!(stops.next(), None, "every stop came before the switch");
    assert!(listening > 0, "a read finished while listening");
    // The longest read: the most heard before the switch.
    let read = read_time(&a.samples[..heard_most], fs);
    eprintln!(
        "switch after {:.1} s heard: its read {read:.3} s, so the figures lag the sound by \
         {:.3} s at most",
        heard_most as f64 / f64::from(fs),
        2.0 * read
    );
    eprintln!("fed fast, the figures were {most_behind:.3} s behind at most");
    // Heard as it comes nothing waits: the figures trail by two reads at
    // most, which must stay within this (Norm's ruling on #1987).
    const LIVE_LAG_MAX_S: f64 = 2.0;
    assert!(
        2.0 * read <= LIVE_LAG_MAX_S,
        "the figures trail by {} s",
        2.0 * read
    );
    std::fs::remove_dir(&dir).unwrap();
}

/// #2291's take (#2283): the reporter's USB microphone plugged into a
/// Thunderbolt drive's USB ports, whose own controller makes the 1 ms
/// frames. Its six USB lines read the card 151.1 ppm fast, past
/// `usbclock::MAX_CARD_PPM`; straight into the computer (#2292) the same
/// microphone read 9.3 ppm. The reporter gave the sound as CC0.
const USB_HUB_TAKE: &str = "usbhub_dd3285_explorer2_DU_95s";

/// The settings #2291's report read its excerpt with: 28,800 bph, 59°.
fn usb_hub_settings() -> AnalysisConfig {
    let mut cfg = AnalysisConfig {
        bph: Some(28_800),
        lift_known: true,
        ..Default::default()
    };
    cfg.amplitude.lift_deg = 59.0;
    cfg
}

/// Norm's acceptance 2 on #2283: lines through a hub's controller are
/// kept, with the reason they are not applied, and correct nothing; the
/// rate is the raw +14.1 s/d, and Hugh's hint says why and what to do.
///
/// Runs where `TIMEGRAPHER_CORPUS` is set.
#[test]
fn usb_lines_through_a_hub_are_kept_and_not_applied() {
    use timegrapher_core::{analyze_doc, clock, take, usbclock};
    let Some(path) = corpus_takes(&[USB_HUB_TAKE]).pop() else {
        return;
    };
    let s = analyze(&load(&path).unwrap(), &usb_hub_settings()).summary;
    let usb = s.usb_clock.expect("the lines kept");
    assert_eq!(usb.lines, 6, "{usb:?}");
    assert!((usb.ppm - 151.1).abs() < 0.05, "{usb:?}");
    assert_eq!(usb.withheld(), Some(usbclock::IMPLAUSIBLE));
    let json = serde_json::to_value(s.usb_clock).unwrap();
    assert_eq!(json["withheld"], "usb_lines_implausible");
    assert_eq!(json["ppm"], usb.ppm);
    let raw = s.rate_s_per_day.expect("a rate");
    assert!((raw - 14.1).abs() < 0.05, "{raw}");
    assert_eq!(s.rate_corrected_s_per_day, None);
    assert_eq!(analyze_doc::usb_clock(&s), None);
    let hint = s
        .caveats
        .iter()
        .find(|c| matches!(c, Caveat::UsbLinesNotUsed { .. }))
        .expect("said");
    assert_eq!(
        hint.long(None).as_deref(),
        Some(
            "The USB lines read the card 151 ppm off, more than a sound card's clock should \
             be, so they are not used. If the mic is plugged into a hub, dock or Thunderbolt \
             device, plug it straight into the computer."
        )
    );
    // Saved and opened: the lines read the same, and set no clock.
    let files = [path.clone()];
    assert_eq!(clock::usb_lines(&files).unwrap(), Some(usb));
    assert_eq!(take::usb(&files).unwrap(), None);
}

/// Norm's acceptance 4 on #2283: on #2291's take, each live read of the
/// lines is withheld as the take saved at that length withholds them, and
/// the rate stays on the card's clock throughout.
///
/// Runs where `TIMEGRAPHER_CORPUS` is set.
#[test]
fn a_live_read_withholds_the_lines_a_saved_take_withholds() {
    use timegrapher_core::usbclock::{self, LiveClock};
    use timegrapher_core::{clock, excerpt, take};
    let Some(path) = corpus_takes(&[USB_HUB_TAKE]).pop() else {
        return;
    };
    let pcm = excerpt::read(&path).unwrap();
    let a = load(&path).unwrap();
    let fs = a.sample_rate;
    let mut lines = usbclock::Live::new(fs);
    let dir = std::env::temp_dir().join(format!("tg-eq-usbhub-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut seen = Vec::new();
    for c in a.samples.chunks(fs as usize / 10) {
        lines.push(c);
        let Some(t) = lines.due() else {
            continue;
        };
        let heard = lines.measure_due();
        let LiveClock::Implausible { clock: live, .. } = heard else {
            panic!("{t} s: {heard:?}");
        };
        assert!(take::live_clock(&heard, None, t).is_none(), "{t} s");
        // The take saved now: its sound to this sample, opened again.
        let saved = dir.join(format!("take-{t:.0}.flac"));
        let part = excerpt::Pcm {
            samples: pcm.samples[..(t * f64::from(fs)) as usize].to_vec(),
            ..pcm
        };
        excerpt::write_flac(&saved, &part).unwrap();
        let files = [saved];
        assert_eq!(clock::usb_lines(&files).unwrap(), Some(live), "{t} s");
        assert_eq!(take::usb(&files).unwrap(), None, "{t} s");
        assert_eq!(
            heard.read_note(None),
            live.not_used_text(),
            "{t} s: the hint said"
        );
        seen.push(t);
    }
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(seen, [30.0, 60.0]);
}
