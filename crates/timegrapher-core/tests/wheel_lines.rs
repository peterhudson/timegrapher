//! The known wheels' lines and turns followed through a real take, as
//! `timegrapher series` reads them ([`lines_for_take`], [`turns_for_take`]).
//! Runs when `TIMEGRAPHER_CORPUS` names the corpus.

use std::path::{Path, PathBuf};
use timegrapher_core::longrun;
use timegrapher_core::longrun::LongReport;
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::periodicity::standard_wheels;
use timegrapher_core::session::Manifest;
use timegrapher_core::stream::{self, BeatLog};
use timegrapher_core::take;
use timegrapher_core::wheel_lines::{lines_for_take, turns_for_take, SIZE_BIN_DEG};

/// The corpus take `name`'s beats, read as `series` reads them with no
/// clock, or None with no corpus.
fn corpus_log(name: &str) -> Option<BeatLog> {
    let Some(corpus) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        eprintln!("SKIP: set TIMEGRAPHER_CORPUS to follow the wheels through a corpus take");
        return None;
    };
    let base = corpus.join("takes").join(name);
    let text = std::fs::read_to_string(base.join("session.toml")).unwrap();
    let m: Manifest = toml::from_str(&text).unwrap();
    let cfg = take::stream_settings(&m, None, take::Given::default());
    let sounds = take::sound_files(&[base]).unwrap();
    let files: Vec<&Path> = sounds.iter().map(PathBuf::as_path).collect();
    Some(stream::analyze_files(&files, &cfg, |_| {}).unwrap())
}

/// Dad's YM42, dial up for 2 h (28,800 bph, a 20-tooth escape wheel), read
/// with no clock and its calibre's wheels, as `series` follows them.
fn ym42() -> Option<(BeatLog, LongConfig, LongReport)> {
    let log = corpus_log("ym42_DU_2h")?;
    let lc = LongConfig {
        wheels: standard_wheels(log.bph, 20),
        ..Default::default()
    };
    let long = longrun::analyse(&log, None, &lc);
    Some((log, lc, long))
}

/// Each wheel's lines, read as the module says: the centre wheel's hour
/// turn is past half the 30 min detrending and is left out, and the escape
/// wheel's 5 s turn is under four of the amplitude's 2 s readings, so only
/// its rate is followed.
#[test]
fn a_real_takes_wheel_lines_are_followed() {
    let Some((log, lc, long)) = ym42() else {
        return;
    };
    let lines = lines_for_take(&log, None, &long, &lc, false);
    let named: Vec<_> = lines
        .iter()
        .map(|l| (l.wheel.as_str(), l.series, l.unit, l.window_s))
        .collect();
    assert_eq!(
        named,
        [
            ("escape wheel", "rate", "ms", 1200.0),
            ("fourth wheel", "rate", "ms", 1200.0),
            ("fourth wheel", "amplitude", "deg", 1200.0),
            ("fourth wheel", "beat_error", "ms", 1200.0),
        ]
    );
    let amps: Vec<f64> = log
        .amplitude_windows
        .iter()
        .filter_map(|w| w.mean())
        .collect();
    let lowest = amps.iter().copied().fold(f64::INFINITY, f64::min);
    let highest = amps.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    for l in &lines {
        assert!(l.windows.is_none(), "{} {}", l.wheel, l.series);
        assert!(!l.bins.is_empty(), "{} {}", l.wheel, l.series);
        // Lowest amplitude first, in bins the take's amplitude reaches.
        for (a, b) in l.bins.iter().zip(&l.bins[1..]) {
            assert!(a.from_deg < b.from_deg);
        }
        for b in &l.bins {
            assert_eq!(b.from_deg % SIZE_BIN_DEG, 0.0);
            assert!(b.from_deg + SIZE_BIN_DEG > lowest && b.from_deg <= highest);
            assert!(b.windows > 0 && b.size >= 0.0 && b.se > 0.0);
            // The rate's sizes in s/d too: a timing sine of A ms at period
            // P is a rate sine of 86.4 x 2 pi A / P s/d.
            let per_day = |v: f64| 86.4 * std::f64::consts::TAU * v / l.period_s;
            match l.series {
                "rate" => {
                    let close = |x: Option<f64>, y: f64| (x.unwrap() - y).abs() < 1e-9 * y.max(1.0);
                    assert!(close(b.size_s_per_day, per_day(b.size)));
                    assert!(close(b.se_s_per_day, per_day(b.se)));
                }
                _ => assert!(b.size_s_per_day.is_none() && b.se_s_per_day.is_none()),
            }
        }
    }
}

#[test]
fn a_real_takes_fourth_wheel_shows_in_its_rate() {
    let Some((log, lc, long)) = ym42() else {
        return;
    };
    let lines = lines_for_take(&log, None, &long, &lc, false);
    // This take's fourth wheel shows in its rate: in the bin most of it
    // was read in, the line stands more than five standard errors clear.
    let fourth = &lines[1].bins;
    let most = fourth.iter().max_by_key(|b| b.windows).unwrap();
    assert!(most.size > 5.0 * most.se, "{} ± {}", most.size, most.se);
    // In ms: a sine in the detrended timing is no larger than √2 times
    // that timing's rms (33 ms here, the slow wander the 30 min detrending
    // leaves), and this one, about 0.5 ms, is more than a hundredth of it.
    let y: Vec<f64> = long
        .timing
        .detrended
        .y
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .collect();
    let rms_ms = 1000.0 * (y.iter().map(|v| v * v).sum::<f64>() / y.len() as f64).sqrt();
    assert!(
        most.size <= std::f64::consts::SQRT_2 * rms_ms && most.size > 0.01 * rms_ms,
        "{} ms against {rms_ms} ms rms",
        most.size
    );
}

#[test]
fn a_real_takes_wheel_lines_keep_their_windows() {
    let Some((log, lc, long)) = ym42() else {
        return;
    };
    let lines = lines_for_take(&log, None, &long, &lc, false);
    // With its windows: the same bins, and every window in them.
    let windowed = lines_for_take(&log, None, &long, &lc, true);
    assert_eq!(windowed.len(), lines.len());
    for (w, l) in windowed.iter().zip(&lines) {
        let bins =
            |x: &timegrapher_core::wheel_lines::WheelLine| serde_json::to_value(&x.bins).unwrap();
        assert_eq!(bins(w), bins(l));
        let windows = w.windows.as_ref().unwrap();
        // Each window is the line's length, half a window on from the one
        // before (or further on, past a gap).
        for v in windows {
            assert!((v.end_s - v.start_s - w.window_s).abs() < 1.0, "{v:?}");
        }
        for (a, b) in windows.iter().zip(&windows[1..]) {
            assert!(b.start_s - a.start_s >= w.window_s / 2.0 - 1.0);
        }
        for b in &w.bins {
            let inside = windows
                .iter()
                .filter(|v| {
                    v.amplitude_deg >= b.from_deg && v.amplitude_deg < b.from_deg + SIZE_BIN_DEG
                })
                .count();
            assert_eq!(inside, b.windows, "{} {} {}", w.wheel, w.series, b.from_deg);
        }
    }
}

/// Each wheel's turn, averaged: the escape wheel's in the rate alone, as
/// for its lines.
#[test]
fn a_real_takes_wheel_turns_are_followed() {
    let Some((_, lc, long)) = ym42() else {
        return;
    };
    let turns = turns_for_take(&long, &lc);
    let named: Vec<_> = turns
        .iter()
        .map(|t| (t.wheel.as_str(), t.series, t.unit, t.bins))
        .collect();
    // A phase bin a reading: the timing's 0.25 s, the amplitude's 2 s, at
    // most 60.
    assert_eq!(
        named,
        [
            ("escape wheel", "rate", "ms", 20),
            ("fourth wheel", "rate", "ms", 60),
            ("fourth wheel", "amplitude", "deg", 30),
        ]
    );
    for t in &turns {
        let shapes = std::iter::once(&t.all).chain(&t.by_amplitude);
        for s in shapes {
            assert_eq!((s.profile.len(), s.se.len()), (t.bins, t.bins));
            let hi = s.profile.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let lo = s.profile.iter().copied().fold(f64::INFINITY, f64::min);
            assert!((s.peak_to_peak - (hi - lo)).abs() < 1e-9);
        }
        assert_eq!((t.all.from_deg, t.all.to_deg), (None, None));
        let in_bands: usize = t.by_amplitude.iter().map(|b| b.turns).sum();
        assert!(in_bands <= t.all.turns && t.all.turns > 0);
        for (a, b) in t.by_amplitude.iter().zip(&t.by_amplitude[1..]) {
            assert!(a.from_deg < b.from_deg);
        }
        // 2 h is 1,440 turns of the escape wheel and 120 of the fourth.
        assert!(t.all.turns as f64 <= 7200.0 / t.period_s);
    }
}

/// A line at a train wheel's period is that wheel's line, never a
/// harmonic of another found cycle (Norm): on DD3235's 30 min take the
/// 5 s escape wheel is a 30 s cycle's 6th harmonic, and the Periods pane
/// still names and lists it.
#[test]
fn a_real_takes_escape_wheel_is_listed_beside_a_cycle_it_is_a_harmonic_of() {
    let Some(log) = corpus_log("dd3235_DU_30min") else {
        return;
    };
    let lc = LongConfig {
        wheels: standard_wheels(log.bph, 20),
        ..Default::default()
    };
    let long = longrun::analyse(&log, None, &lc);
    let escape = long
        .timing
        .components
        .iter()
        .find(|c| (c.period_s - 5.0).abs() < 0.05)
        .expect("the escape wheel's line");
    assert_eq!(escape.wheel.as_deref(), Some("escape wheel"));
    assert!(escape.harmonic_of.is_none(), "{:?}", escape.harmonic_of);
    let r = timegrapher_core::steadiness::check(
        &log,
        None,
        &long,
        &lc,
        &timegrapher_core::steadiness::Config::default(),
    );
    let rate = r
        .series
        .iter()
        .find(|s| s.series == timegrapher_core::steadiness::SeriesKind::Rate)
        .unwrap();
    assert!(rate.headline.contains("5.0 s"), "{}", rate.headline);
}
