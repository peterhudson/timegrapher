//! Auto lets go of a rate the beats don't bear out (#1617).

use super::*;
use crate::synth::{generate, SynthConfig};

/// Seconds of silence, then `lure_s` of clicks at 36,000 bph (the
/// handling as a watch is put back), then an 18,000 bph watch for 40 s,
/// heard live with the rate `bph` set (`None` for Auto). Returns the
/// analyzer, every rate it was on, and its 10 s reading at the end.
fn put_back(lure_s: f64, bph: Option<u32>) -> (LiveAnalyzer, Vec<Option<u32>>, LiveReading) {
    let watch = |bph: u32, duration_s: f64, seed: u64| {
        generate(
            &SynthConfig {
                bph,
                duration_s,
                seed,
                ..Default::default()
            },
            |_| 260.0,
            |_| 0.0,
        )
    };
    let lure = watch(36_000, lure_s, 1);
    let fs = lure.sample_rate as f64;
    let mut samples = vec![0.0; (6.0 * fs) as usize];
    samples.extend_from_slice(&lure.samples);
    samples.extend_from_slice(&watch(18_000, 40.0, 2).samples);
    let cfg = LiveConfig {
        analysis: AnalysisConfig {
            bph,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut live = LiveAnalyzer::new(lure.sample_rate, cfg);
    let mut rates = Vec::new();
    for chunk in samples.chunks(960) {
        live.push(chunk);
        if rates.last() != Some(&live.bph()) {
            rates.push(live.bph());
        }
    }
    let reading = live.reading(10.0);
    (live, rates, reading)
}

/// Auto, locked on to 36,000 by two seconds of clicks, reads the 18,000
/// bph watch that follows at its own rate, with readings, as Tim's
/// pocket watch put back on the stand should have been (#1617).
#[test]
fn auto_lets_go_of_twice_the_rate_after_a_put_back() {
    let (live, rates, r) = put_back(2.0, None);
    assert!(
        rates.contains(&Some(36_000)),
        "the clicks lock on first: {rates:?}"
    );
    assert_eq!(live.bph(), Some(18_000), "{rates:?}");
    assert!(r.watch_heard, "{r:?}");
    let rate = r.rate_s_per_day.expect("a rate");
    assert!((rate - 5.0).abs() < 2.0, "rate {rate} s/d");
    assert!(r.amplitude_deg.is_some(), "{r:?}");
}

/// A rate set by hand is never doubted, however the beats read.
#[test]
fn a_rate_set_by_hand_stands() {
    let (live, rates, _) = put_back(2.0, Some(36_000));
    assert_eq!(rates, vec![Some(36_000)]);
    assert_eq!(live.bph(), Some(36_000));
}

/// A watch Auto locked on to rightly keeps its rate: 30 s at standard
/// rates from 18,000 to 36,000, with a large beat error and with noise,
/// never lets the lock go.
#[test]
fn a_right_lock_is_never_let_go() {
    for bph in [18_000, 21_600, 28_800, 36_000] {
        for (be, snr_db) in [(0.4, 30.0), (4.0, 30.0), (0.4, 20.0)] {
            let audio = generate(
                &SynthConfig {
                    bph,
                    duration_s: 30.0,
                    beat_error_ms: be,
                    snr_db,
                    seed: 3,
                    ..Default::default()
                },
                |t| 280.0 - 2.0 * t,
                |_| 0.0,
            );
            let mut live = LiveAnalyzer::new(audio.sample_rate, LiveConfig::default());
            let mut rates = Vec::new();
            for chunk in audio.samples.chunks(960) {
                live.push(chunk);
                if rates.last() != Some(&live.bph()) {
                    rates.push(live.bph());
                }
            }
            assert_eq!(
                rates,
                vec![None, Some(bph)],
                "{bph} bph, {be} ms, {snr_db} dB"
            );
        }
    }
}
