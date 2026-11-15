//! Acceptance for #1606: a period's harmonic on a wheel is left out of its
//! score. Set before any run, run once on main and once on the branch, on
//! the same fresh seeds:
//! 1. Power kept: a true 15 s and a true 30 s pulse (4th and 8th harmonics
//!    on the 3.75 s escape wheel), no wheel line. The share of takes that
//!    name the pulse's period falls by at most 3 points.
//! 2. False alarms may only fall: on a take with only the escape wheel's
//!    line, and on a steady take, the share that names any period other
//!    than the wheel's 3.75 s line or its 30 s alias in the readings does
//!    not rise.

use std::path::PathBuf;
use timegrapher_core::audio::write_wav;
use timegrapher_core::longrun;
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::periodicity::standard_wheels;
use timegrapher_core::steadiness::{self, Config, SeriesKind};
use timegrapher_core::stream::{analyze_file, StreamConfig};
use timegrapher_core::synth::{generate, SynthConfig};

const DRAWS: u64 = 40;
const FIRST_SEED: u64 = 2026111560;

/// Every period the rate's check names: the period search's line, its
/// weaker components, and the readings' cycle.
fn named(seed: u64, rate: impl Fn(f64) -> f64) -> Vec<f64> {
    let cfg = SynthConfig {
        duration_s: 900.0,
        bph: 28800,
        rate_s_per_day: 10.0,
        snr_db: 24.0,
        seed,
        ..Default::default()
    };
    let path: PathBuf =
        std::env::temp_dir().join(format!("tg-1606-{seed}-{}.wav", std::process::id()));
    write_wav(&path, &generate(&cfg, |_| 270.0, rate)).unwrap();
    let log = analyze_file(&path, &StreamConfig::default(), |_| {}).unwrap();
    std::fs::remove_file(&path).ok();
    let lc = LongConfig {
        wheels: standard_wheels(log.bph, 15),
        ..Default::default()
    };
    let long = longrun::analyse(&log, None, &lc);
    let r = steadiness::check(&log, None, &long, &lc, &Config::default());
    let s = r
        .series
        .iter()
        .find(|s| s.series == SeriesKind::Rate)
        .unwrap();
    let mut out = Vec::new();
    if let Some(p) = &s.period {
        out.push(p.period_s);
        out.extend(p.other_periods_s.iter().copied());
    }
    if let Some(c) = &s.cycle {
        out.push(c.period_s);
    }
    out
}

fn share(
    case: u64,
    hit: impl Fn(&[f64]) -> bool + Sync,
    rate: impl Fn(f64) -> f64 + Send + Sync + Copy,
) -> f64 {
    let n = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..4u64)
            .map(|k| {
                let hit = &hit;
                sc.spawn(move || {
                    (0..DRAWS)
                        .filter(|d| d % 4 == k)
                        .filter(|&d| hit(&named(FIRST_SEED + case * 1000 + d, rate)))
                        .count()
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).sum::<usize>()
    });
    n as f64 / DRAWS as f64
}

fn near(p: f64, q: f64) -> bool {
    (p - q).abs() < 0.02 * q
}

#[test]
#[ignore = "acceptance run for #1606, about 20 minutes"]
fn wheel_harmonic_power_and_false_alarms() {
    let tau = 2.0 * std::f64::consts::PI;
    for (case, period, width) in [(1u64, 15.0, 4.0), (2, 30.0, 8.0)] {
        let s = share(
            case,
            |n| n.iter().any(|&p| near(p, period)),
            move |t| if t % period < width { -30.0 } else { 0.0 },
        );
        println!("ACCEPT power {period} s pulse: {:.1}% named", 100.0 * s);
    }
    let other = |n: &[f64]| n.iter().any(|&p| !near(p, 3.75) && !near(p, 30.0));
    let s = share(3, other, move |t| 20.0 * (tau * t / 3.75).sin());
    println!(
        "ACCEPT false wheel line only: {:.1}% name another period",
        100.0 * s
    );
    let s = share(4, other, |_| 0.0);
    println!("ACCEPT false steady: {:.1}% name another period", 100.0 * s);
}
