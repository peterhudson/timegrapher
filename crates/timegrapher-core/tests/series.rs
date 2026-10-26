//! The steadiness check end to end: synthetic recordings written to disk,
//! read back in chunks, and each series given its verdict.

use std::path::PathBuf;
use timegrapher_core::audio::write_wav;
use timegrapher_core::longrun;
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::periodicity::standard_wheels;
use timegrapher_core::steadiness::{self, Config, Report, SeriesKind};
use timegrapher_core::stream::{analyze_file, StreamConfig};
use timegrapher_core::synth::{generate, SynthConfig};

fn run(name: &str, duration_s: f64, amp: impl Fn(f64) -> f64, rate: impl Fn(f64) -> f64) -> Report {
    run_with(name, duration_s, amp, rate, 15)
}

fn run_with(
    name: &str,
    duration_s: f64,
    amp: impl Fn(f64) -> f64,
    rate: impl Fn(f64) -> f64,
    escape_teeth: u32,
) -> Report {
    run_at(name, duration_s, 28800, amp, rate, escape_teeth)
}

fn run_at(
    name: &str,
    duration_s: f64,
    bph: u32,
    amp: impl Fn(f64) -> f64,
    rate: impl Fn(f64) -> f64,
    escape_teeth: u32,
) -> Report {
    let cfg = SynthConfig {
        duration_s,
        bph,
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
        wheels: standard_wheels(log.bph, escape_teeth),
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
        assert!(s.cycle.is_none(), "{}", s.headline);
        assert!(s.faster_than_readings().is_none(), "{}", s.headline);
        assert!(
            s.headline
                .contains("no cycle, level change, trend or correlation"),
            "{}",
            s.headline
        );
    }
    assert!(r.findings.is_empty());
}

#[test]
fn an_escape_wheel_line_faster_than_the_readings_is_named_not_denied() {
    // A 20-tooth escape wheel at 28,800 bph turns in 5 s: under three
    // 10 s readings (each averages two whole turns away), so no cycle can
    // show in them, but the period search on the beat timing finds it.
    let tau = 2.0 * std::f64::consts::PI;
    let r = run_with(
        "escape",
        900.0,
        |_| 270.0,
        move |t| 20.0 * (tau * t / 5.0).sin(),
        20,
    );
    let s = series(&r, SeriesKind::Rate);
    assert!(s.cycle.is_none(), "{}", s.headline);
    let p = s.faster_than_readings().expect(&s.headline);
    assert!((p.period_s - 5.0).abs() < 0.05, "{}", p.period_s);
    assert!(!s.headline.contains("no cycle"), "{}", s.headline);
    assert!(!s.summary.contains("no cycle"), "{}", s.summary);
    // In place of "no cycle", or as its own sentence after another lead.
    assert!(
        s.headline
            .to_lowercase()
            .contains("the beat timing repeats every 5.0 s, the escape wheel's period,"),
        "{}",
        s.headline
    );
    assert!(
        s.headline
            .contains("10 s readings are too coarse to show it"),
        "{}",
        s.headline
    );
}

#[test]
fn a_fast_line_aliased_in_the_readings_is_not_a_cycle() {
    // The 15-tooth escape wheel at 28,800 bph turns in 3.75 s; 10 s
    // readings average 2.67 turns, so it leaves a 30 s alias in them
    // ("Rate repeats every 30 s" before the guard).
    let tau = 2.0 * std::f64::consts::PI;
    let r = run(
        "alias",
        900.0,
        |_| 270.0,
        move |t| 20.0 * (tau * t / 3.75).sin(),
    );
    let s = series(&r, SeriesKind::Rate);
    assert!(s.cycle.is_none(), "{}", s.headline);
    let p = s.faster_than_readings().expect(&s.headline);
    assert!((p.period_s - 3.75).abs() < 0.05, "{}", p.period_s);
    assert!(!s.headline.contains("repeats every 30 s"), "{}", s.headline);
    assert!(
        s.headline
            .to_lowercase()
            .contains("the beat timing repeats every 3.8 s, the escape wheel's period,"),
        "{}",
        s.headline
    );
}

#[test]
fn a_slow_cycle_beside_an_aliased_fast_line_is_still_found() {
    // The same 3.75 s line, and the fourth wheel's 60 s (two turns of the
    // 30 s alias): 8 s of every minute 30 s/d slower.
    let tau = 2.0 * std::f64::consts::PI;
    let r = run(
        "alias-and-minute",
        900.0,
        |_| 270.0,
        move |t| 20.0 * (tau * t / 3.75).sin() + if t % 60.0 < 8.0 { -30.0 } else { 0.0 },
    );
    let s = series(&r, SeriesKind::Rate);
    let c = s.cycle.as_ref().expect(&s.headline);
    assert!(
        (c.period_s - 60.0).abs() < 2.0,
        "{} {}",
        c.period_s,
        s.headline
    );
    assert_eq!(c.wheel.as_deref(), Some("fourth wheel"));
    // The period search's line, under three readings, is named beside it.
    let p = s.faster_than_readings().expect(&s.headline);
    assert!(
        s.headline.contains(&format!(
            ". The beat timing also repeats every {:.1} s",
            p.period_s
        )) && s
            .headline
            .ends_with("10 s readings are too coarse to show it."),
        "{}",
        s.headline
    );
}

#[test]
fn a_fourth_wheel_cycle_on_the_escape_wheels_alias_is_still_found() {
    // At 19,800 bph a 15-tooth escape wheel turns in 5.4545 s, which
    // aliases to exactly 60 s in 10 s readings: the fourth wheel's turn.
    // The period search finds the 60 s cycle in the beat timing itself,
    // so it is listed, not set aside as the alias.
    let tau = 2.0 * std::f64::consts::PI;
    let r = run_at(
        "alias-on-minute",
        900.0,
        19800,
        |_| 270.0,
        move |t| 20.0 * (tau * t / (60.0 / 11.0)).sin() + if t % 60.0 < 8.0 { -30.0 } else { 0.0 },
        15,
    );
    let s = series(&r, SeriesKind::Rate);
    let p = s.period.as_ref().expect(&s.headline);
    assert!(
        std::iter::once(p.period_s)
            .chain(p.other_periods_s.iter().copied())
            .any(|q| (q - 60.0 / 11.0).abs() < 0.05),
        "{:?}",
        p
    );
    let c = s.cycle.as_ref().expect(&s.headline);
    assert!(
        (c.period_s - 60.0).abs() < 2.0,
        "{} {}",
        c.period_s,
        s.headline
    );
    assert_eq!(c.wheel.as_deref(), Some("fourth wheel"));
    assert!(!s.headline.contains("no cycle"), "{}", s.headline);
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
        assert!(
            r.findings
                .iter()
                .any(|f| f.series == k && f.code == "periodic"),
            "{}",
            s.headline
        );
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
    assert!(
        r.findings
            .iter()
            .any(|f| f.series == SeriesKind::Rate && f.code == "shifting_mean"),
        "{}",
        s.headline
    );
    let ch = s.changes.as_ref().unwrap();
    assert_eq!(ch.segments.len(), 2, "{:?}", ch.segments);
    assert!((ch.segments[1].start_s - 700.0).abs() < 60.0);
}

#[test]
fn a_48_s_cycle_is_not_read_as_a_multiple() {
    // A 48 s cycle in rate and amplitude, behind a strong 5 s escape-wheel
    // wobble and rate noise. 48 s is not a whole number of 10 s rate
    // readings, so the autocorrelation peaks fall between lags; the cycle
    // must still come out at 48 s, not missed and not at 240 s.
    let w = |t: f64| (2.0 * std::f64::consts::PI * t / 48.0).sin();
    let r = run(
        "48s",
        1800.0,
        move |t| 250.0 + 3.0 * w(t),
        move |t| {
            2.0 * w(t)
                + 4.0 * hash_noise(t / 3.0)
                + 100.0 * (2.0 * std::f64::consts::PI * t / 5.0).sin()
        },
    );
    for k in [SeriesKind::Rate, SeriesKind::Amplitude] {
        let s = series(&r, k);
        assert!(
            r.findings
                .iter()
                .any(|f| f.series == k && f.code == "periodic"),
            "{}",
            s.headline
        );
        let c = s.cycle.as_ref().unwrap();
        assert!((c.period_s - 48.0).abs() < 2.0, "{k:?}: {}", c.period_s);
    }
}

/// Uniform noise in [-1, 1], constant over each unit of `x`.
fn hash_noise(x: f64) -> f64 {
    let v = (x.floor() * 12.9898).sin() * 43758.5453;
    2.0 * (v - v.floor()) - 1.0
}

#[test]
fn rate_against_amplitude_gives_slope_r_and_p() {
    // Amplitude swings ±10° every 75 s as it falls 20° over 20 minutes,
    // and the rate follows it at 0.5 s/d per degree.
    let amp =
        |t: f64| 270.0 - 20.0 * t / 1200.0 + 10.0 * (2.0 * std::f64::consts::PI * t / 75.0).sin();
    let r = run("isochronism", 1200.0, amp, move |t| 0.5 * (amp(t) - 255.0));
    for s in &r.series {
        assert_eq!(s.step_s, 10.0, "{:?}", s.series);
    }
    let x = r.rate_amplitude.as_ref().unwrap();
    assert!(x.r > 0.9 && x.p_value < 0.01, "{}", x.headline);
    assert!((x.s_per_day_per_deg - 0.5).abs() < 0.1, "{}", x.headline);
    // Always stated as numbers, never as a finding.
    assert!(x.headline.contains("per degree") && x.headline.contains(" p "));
    assert!(r
        .findings
        .iter()
        .all(|f| f.code != "rate_moves_with_amplitude"));
}
