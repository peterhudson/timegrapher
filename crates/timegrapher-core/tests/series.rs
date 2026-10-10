//! The steadiness check end to end: synthetic recordings written to disk,
//! read back in chunks, and each series given its verdict.

use std::path::PathBuf;
use timegrapher_core::audio::write_wav;
use timegrapher_core::longrun;
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::periodicity::standard_wheels;
use timegrapher_core::steadiness::{self, Config, Report, SeriesKind, Verdict};
use timegrapher_core::stream::{analyze_file, StreamConfig};
use timegrapher_core::synth::{generate, SynthConfig};

fn run(name: &str, duration_s: f64, amp: impl Fn(f64) -> f64, rate: impl Fn(f64) -> f64) -> Report {
    let cfg = SynthConfig {
        duration_s,
        rate_s_per_day: 10.0,
        snr_db: 24.0,
        ..Default::default()
    };
    let path: PathBuf =
        std::env::temp_dir().join(format!("tg-series-{name}-{}.wav", std::process::id()));
    write_wav(&path, &generate(&cfg, amp, rate)).unwrap();
    let log = analyze_file(&path, &StreamConfig::default(), |_| {}).unwrap();
    std::fs::remove_file(&path).ok();
    let lc = LongConfig {
        wheels: standard_wheels(log.bph, 15),
        ..Default::default()
    };
    let long = longrun::analyse(&log, None, &lc);
    steadiness::check(&log, None, &long, &lc, &Config::default())
}

fn series(r: &Report, k: SeriesKind) -> &steadiness::SeriesCheck {
    r.series.iter().find(|s| s.series == k).unwrap()
}

#[test]
fn steady_watch_is_steady() {
    let r = run("steady", 900.0, |_| 270.0, |_| 0.0);
    for s in &r.series {
        assert_eq!(s.verdict, Verdict::Steady, "{}", s.headline);
    }
    assert!(r.findings.is_empty());
}

#[test]
fn once_a_minute_fault_is_the_fourth_wheel() {
    // 8 s of every minute: 30 s/d slower and 15° lower.
    let bad = |t: f64| t % 60.0 < 8.0;
    let r = run(
        "minute",
        900.0,
        move |t| if bad(t) { 255.0 } else { 270.0 },
        move |t| if bad(t) { -30.0 } else { 0.0 },
    );
    for k in [SeriesKind::Rate, SeriesKind::Amplitude] {
        let s = series(&r, k);
        assert_eq!(s.verdict, Verdict::Periodic, "{}", s.headline);
        let c = s.cycle.as_ref().unwrap();
        assert!((c.period_s - 60.0).abs() < 2.0, "{}", c.period_s);
        assert_eq!(c.wheel.as_deref(), Some("fourth wheel"));
    }
    assert!(r.findings.iter().any(|f| f.code == "periodic"));
}

#[test]
fn a_step_in_rate_is_a_shifting_mean() {
    let r = run(
        "step",
        1200.0,
        |_| 270.0,
        |t| if t < 700.0 { 0.0 } else { 15.0 },
    );
    let s = series(&r, SeriesKind::Rate);
    assert_eq!(s.verdict, Verdict::ShiftingMean, "{}", s.headline);
    let ch = s.changes.as_ref().unwrap();
    assert_eq!(ch.segments.len(), 2, "{:?}", ch.segments);
    assert!((ch.segments[1].start_s - 700.0).abs() < 60.0);
}
