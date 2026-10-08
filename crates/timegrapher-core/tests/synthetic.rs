//! End-to-end checks on synthetic recordings with known answers.

use timegrapher_core::synth::{generate, SynthConfig};
use timegrapher_core::{analyze, AnalysisConfig};

fn run(
    cfg: &SynthConfig,
    amp: impl Fn(f64) -> f64,
    rate: impl Fn(f64) -> f64,
) -> timegrapher_core::Analysis {
    analyze(&generate(cfg, amp, rate), &AnalysisConfig::default())
}

#[test]
fn rate_beat_error_and_amplitude() {
    for (bph, amp) in [
        (28800, 280.0),
        (21600, 250.0),
        (18000, 220.0),
        (36000, 270.0),
    ] {
        let cfg = SynthConfig {
            bph,
            rate_s_per_day: -7.0,
            beat_error_ms: 0.6,
            duration_s: 30.0,
            ..Default::default()
        };
        let a = run(&cfg, |_| amp, |_| 0.0);
        let s = &a.summary;
        assert_eq!(s.bph, bph, "beat rate guessed wrongly");
        let f = s.overall.expect("fit");
        assert!(
            (f.rate_s_per_day + 7.0).abs() < 0.3,
            "{bph}: rate {}",
            f.rate_s_per_day
        );
        assert!(
            (f.beat_error_ms.abs() - 0.6).abs() < 0.05,
            "{bph}: beat error {}",
            f.beat_error_ms
        );
        let measured = s.amplitude_deg.expect("amplitude");
        assert!(
            (measured - amp).abs() < 8.0,
            "{bph}: amplitude {measured} vs {amp}"
        );
    }
}

#[test]
fn noisy_signal_still_tracks() {
    let cfg = SynthConfig {
        snr_db: 12.0,
        duration_s: 30.0,
        ..Default::default()
    };
    let a = run(&cfg, |_| 270.0, |_| 0.0);
    let f = a.summary.overall.expect("fit");
    assert!(
        (f.rate_s_per_day - 5.0).abs() < 0.5,
        "rate {}",
        f.rate_s_per_day
    );
    assert!(a.beats.len() >= 235);
}

#[test]
fn finds_a_once_a_minute_rate_swing() {
    // The watch loses 40 s/d for 10 s of every minute, like a bad fourth-wheel tooth.
    let cfg = SynthConfig {
        duration_s: 300.0,
        ..Default::default()
    };
    let swing = |t: f64| if (t % 60.0) < 10.0 { -40.0 } else { 0.0 };
    let a = run(&cfg, |_| 270.0, swing);
    let top = &a.summary.timing_periods[0];
    assert!(
        (top.period_s - 60.0).abs() < 4.0,
        "{:?}",
        a.summary.timing_periods
    );
    assert_eq!(top.wheel.as_deref(), Some("fourth wheel"));
}

#[test]
fn finds_an_amplitude_dip_every_minute() {
    let cfg = SynthConfig {
        duration_s: 300.0,
        ..Default::default()
    };
    let dip = |t: f64| {
        if (t % 60.0 - 30.0).abs() < 4.0 {
            245.0
        } else {
            275.0
        }
    };
    let a = run(&cfg, dip, |_| 0.0);
    let top = &a.summary.amplitude_periods[0];
    assert!(
        (top.period_s - 60.0).abs() < 4.0,
        "{:?}",
        a.summary.amplitude_periods
    );
}
