//! Long-run analysis end to end: a synthetic recording written to disk,
//! read back in chunks, and searched for periodic changes.

use std::path::PathBuf;
use timegrapher_core::audio::write_wav;
use timegrapher_core::clock::ClockFit;
use timegrapher_core::longrun::{self, LongReport};
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::periodicity::standard_wheels;
use timegrapher_core::stream::{analyze_file, StreamConfig};
use timegrapher_core::synth::{generate, SynthConfig};

fn run(
    name: &str,
    duration_s: f64,
    amp: impl Fn(f64) -> f64,
    rate: impl Fn(f64) -> f64,
    clock: Option<&ClockFit>,
) -> LongReport {
    let cfg = SynthConfig {
        duration_s,
        rate_s_per_day: 10.0,
        snr_db: 24.0,
        ..Default::default()
    };
    let path: PathBuf =
        std::env::temp_dir().join(format!("tg-long-{name}-{}.wav", std::process::id()));
    write_wav(&path, &generate(&cfg, amp, rate)).unwrap();
    let log = analyze_file(&path, &StreamConfig::default(), |_| {}).unwrap();
    std::fs::remove_file(&path).ok();
    let lc = LongConfig {
        wheels: standard_wheels(log.bph, 15),
        ..Default::default()
    };
    longrun::analyse(&log, clock, &lc)
}

#[test]
fn steady_watch_shows_no_cycles() {
    // Chunks are 47 s long; a boundary artefact would show up here.
    let r = run("steady", 600.0, |_| 270.0, |_| 0.0, None);
    assert_eq!(r.beats_found, 4800, "beats lost or doubled at chunk joins");
    let f = r.overall.unwrap();
    assert!(
        (f.rate_s_per_day - 10.0).abs() < 0.2,
        "{}",
        f.rate_s_per_day
    );
    assert!(
        r.rate_components.is_empty(),
        "{:?}",
        r.rate_components
            .iter()
            .map(|c| c.component.period_s)
            .collect::<Vec<_>>()
    );
    assert!(
        r.amplitude.components.is_empty(),
        "{:?}",
        r.amplitude
            .components
            .iter()
            .map(|c| c.period_s)
            .collect::<Vec<_>>()
    );
}

#[test]
fn finds_a_fourth_wheel_fault_in_rate_and_amplitude() {
    // Once a minute, for 8 s, the watch loses 30 s/d and amplitude drops 15 deg.
    let bad = |t: f64| (t % 60.0 - 20.0).abs() < 4.0;
    let r = run(
        "fault",
        600.0,
        |t| if bad(t) { 255.0 } else { 270.0 },
        |t| if bad(t) { -30.0 } else { 0.0 },
        None,
    );
    let c = &r.rate_components[0];
    assert!(
        (c.component.period_s - 60.0).abs() < 1.0,
        "{}",
        c.component.period_s
    );
    assert_eq!(c.component.wheel.as_deref(), Some("fourth wheel"));
    assert!(
        (c.rate_swing_s_per_day - 30.0).abs() < 8.0,
        "{}",
        c.rate_swing_s_per_day
    );
    let a = &r.amplitude.components[0];
    assert!((a.period_s - 60.0).abs() < 1.0, "{}", a.period_s);
    assert!((a.peak_to_peak - 15.0).abs() < 4.0, "{}", a.peak_to_peak);
}

#[test]
fn clock_calibration_corrects_the_rate() {
    // The sound card runs 25 ppm slow: true time passes 1.000025 s per
    // audio second, so the watch looks 2.16 s/d faster than it is.
    let pairs: Vec<(f64, f64)> = (0..11)
        .map(|i| (i as f64 * 60.0, 1.76e9 + i as f64 * 60.0 * 1.000025))
        .collect();
    let clock = ClockFit::new(&pairs).unwrap();
    let r = run("clock", 600.0, |_| 270.0, |_| 0.0, Some(&clock));
    let f = r.overall.unwrap();
    assert!(
        (f.rate_s_per_day - (10.0 - 2.16)).abs() < 0.2,
        "{}",
        f.rate_s_per_day
    );
}

#[test]
fn amplitude_change_alone_is_not_read_as_rate() {
    // The unlock moves relative to the drop as amplitude changes; the beat
    // time must follow the drop, not the template as a whole.
    let bad = |t: f64| (t % 60.0 - 20.0).abs() < 4.0;
    let r = run(
        "amp-only",
        600.0,
        |t| if bad(t) { 230.0 } else { 280.0 },
        |_| 0.0,
        None,
    );
    assert!((r.amplitude.components[0].period_s - 60.0).abs() < 1.0);
    let swing: Vec<f64> = r
        .rate_components
        .iter()
        .map(|c| c.rate_swing_s_per_day)
        .collect();
    assert!(
        swing.iter().all(|&s| s < 3.0),
        "rate swings {swing:?} from amplitude alone"
    );
}
