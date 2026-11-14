//! A stretch of 10 to 15 s that holds no whole 10 s scatter window
//! (#1688) is judged as live judges it: on the windows looking back
//! [`timegrapher_core::live::SCATTER_LOOKBACK_S`] from its end, never
//! across a cut, and where none fits after a cut, by the beat sound
//! (Norm's ruling). A clean watch is heard at every offset, live and
//! saved alike; the stand's noise is not.

use timegrapher_core::breaks::Break;
use timegrapher_core::live::{LiveAnalyzer, LiveConfig};
use timegrapher_core::measure::{self, NoWatchReason, RATE_WINDOW_S, RATE_WINDOW_STEP_S};
use timegrapher_core::session::{self, Measurement};
use timegrapher_core::stream::{analyze_samples, BeatLog, StreamConfig};
use timegrapher_core::synth::{generate, noise_rms, Rng, SynthConfig, SCALE};

const SECONDS: f64 = 60.0;

fn config() -> StreamConfig {
    let mut cfg = StreamConfig::default();
    cfg.analysis.bph = Some(SynthConfig::default().bph);
    cfg
}

/// The issue's probe: a clean watch at the synthetic default rate and
/// 280°, `seconds` long.
fn watch_sound(seconds: f64) -> Vec<f32> {
    generate(
        &SynthConfig {
            duration_s: seconds,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    )
    .samples
}

/// The synthetic stand noise the noise-lead tests use, with no watch.
fn noise_sound(seconds: f64) -> Vec<f32> {
    let rms = noise_rms(SynthConfig::default().snr_db) * SCALE;
    let mut rng = Rng::new(5);
    let n = (seconds * 48_000.0) as usize;
    (0..n).map(|_| (rms * rng.normal()) as f32).collect()
}

fn log_of(samples: &[f32]) -> BeatLog {
    analyze_samples(samples, 48_000, &config())
}

fn watch() -> BeatLog {
    log_of(&watch_sound(SECONDS))
}

fn noise() -> BeatLog {
    log_of(&noise_sound(SECONDS))
}

/// The same sound heard live, at the same beat rate.
fn live(samples: &[f32]) -> LiveAnalyzer {
    let mut cfg = LiveConfig::default();
    cfg.analysis.bph = config().analysis.bph;
    let mut a = LiveAnalyzer::new(48_000, cfg);
    for c in samples.chunks(480) {
        a.push(c);
    }
    a
}

/// Every stretch of 10 to 15 s, starting each second across a period of
/// the 5 s grid, from 3 s in: with and without a whole window.
fn stretches() -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for len in [10.0, 11.0, 12.0, 13.0, 14.0, 15.0] {
        for k in 0..5 {
            let from = 3.0 + k as f64;
            out.push((from, from + len));
        }
    }
    out
}

fn read(log: &BeatLog, (from, to): (f64, f64)) -> Measurement {
    session::measure_masked(log, None, &[], &[], from, to)
}

#[test]
fn a_clean_watch_is_heard_over_every_short_stretch() {
    let log = watch();
    let mut without = 0;
    for s in stretches() {
        let holes = timegrapher_core::clean::Cuts::new(None, &[], &[]);
        if !session::holds_scatter_window(&holes, s.0, s.1, RATE_WINDOW_S, RATE_WINDOW_STEP_S) {
            without += 1;
        }
        let m = read(&log, s);
        assert!(m.watch_heard, "{s:?}: {:?}", m.caveats);
        assert!(m.rate_s_per_day.is_some(), "{s:?}");
        assert!(m.amplitude_deg.is_some(), "{s:?}");
        assert!(m.beat_error_ms.is_some(), "{s:?}");
    }
    // The case the ruling is about is among them.
    assert!(without > 0);
}

/// The stand's noise is never heard: the timing judges it, on its own
/// windows or those looking back, by its scatter (Scatter), or in the
/// doubt zone by the beat sound (#675).
#[test]
fn the_stand_noise_is_not_heard_over_any_short_stretch() {
    let log = noise();
    for s in stretches() {
        let m = read(&log, s);
        assert!(!m.watch_heard, "{s:?}");
        let reason = measure::no_watch_reason_in(&m.caveats);
        assert!(
            matches!(
                reason,
                Some(NoWatchReason::Scatter | NoWatchReason::NoBeatSound)
            ),
            "{s:?}: {reason:?}"
        );
    }
}

/// The stand's noise, then a new position and a clean watch: a short
/// stretch from the new position on that holds no window looks back to
/// the position's start, not into the noise before it, and the watch is
/// heard. Saved only: live, after a new position that follows noise,
/// says no watch here today, on stretches with a whole window too, a
/// gap older than this change (ISSUE).
#[test]
fn a_clean_watch_just_after_a_new_position_is_heard() {
    let entry = 30.0;
    let mut x = noise_sound(entry);
    x.extend(watch_sound(20.0));
    let log = log_of(&x);
    let at = [Break {
        audio_s: entry,
        gap_s: 0.0,
    }];
    let mut tried = 0;
    for len in [10.0, 11.0, 12.0, 13.0, 14.0, 15.0] {
        for k in 0..5 {
            let (from, to) = (entry + k as f64, entry + k as f64 + len);
            // One holding a whole window is judged on it, as before.
            let cut = timegrapher_core::clean::Cuts::new(None, &at, &[]);
            if to > entry + 20.0
                || session::holds_scatter_window(&cut, from, to, RATE_WINDOW_S, RATE_WINDOW_STEP_S)
            {
                continue;
            }
            tried += 1;
            let m = session::measure_masked(&log, None, &at, &[], from, to);
            assert!(m.watch_heard, "{from}..{to}: {:?}", m.caveats);
        }
    }
    assert!(tried > 0);
}

/// Live and saved say alike whether a watch is heard over every short
/// stretch of the clean watch and of the noise (#680).
#[test]
fn live_and_saved_agree_on_every_short_stretch() {
    for (name, x) in [
        ("watch", watch_sound(SECONDS)),
        ("noise", noise_sound(SECONDS)),
    ] {
        let log = log_of(&x);
        let a = live(&x);
        for s in stretches() {
            let saved = read(&log, s).watch_heard;
            let live = a.reading_at(s.1, s.1 - s.0).watch_heard;
            assert_eq!(saved, live, "{name} {s:?}");
        }
    }
}

/// Where a whole window lies, or none can, is told by where the windows
/// fall: 15 s or more always holds one; 3 to 13 s, on the 5 s grid,
/// holds none; a cut through the only window takes it away.
#[test]
fn whether_a_stretch_holds_a_window_is_where_the_windows_fall() {
    use timegrapher_core::breaks::Break;
    let holes = timegrapher_core::clean::Cuts::new(None, &[], &[]);
    let holds = |h: &timegrapher_core::clean::Cuts, a: f64, b: f64| {
        session::holds_scatter_window(h, a, b, RATE_WINDOW_S, RATE_WINDOW_STEP_S)
    };
    assert!(!holds(&holes, 3.0, 13.0));
    assert!(holds(&holes, 5.0, 15.0));
    for k in 0..10 {
        let from = k as f64 * 0.5;
        assert!(holds(&holes, from, from + 15.0), "{from}");
    }
    let cut = [Break {
        audio_s: 10.0,
        gap_s: 0.0,
    }];
    let cut = timegrapher_core::clean::Cuts::new(None, &cut, &[]);
    assert!(!holds(&cut, 5.0, 15.0));
    assert!(!measure::regularity_assessed(12.0, false));
}
