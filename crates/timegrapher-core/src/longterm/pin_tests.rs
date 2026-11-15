//! Pins for the period search's and shape fit's rules that no other test
//! checked (#1438): each test names the rule it holds.

use super::*;
use crate::periodicity::Wheel;
use crate::synth::Rng;

fn series(n: usize, step: f64, f: impl Fn(f64) -> f64, noise: f64, seed: u64) -> Grid {
    let mut rng = Rng::new(seed);
    Grid {
        t0: 0.0,
        step,
        y: (0..n)
            .map(|i| f((i as f64 + 0.5) * step) + noise * rng.normal())
            .collect(),
    }
}

fn sine(period: f64) -> impl Fn(f64) -> f64 {
    move |t| (std::f64::consts::TAU * t / period).sin()
}

/// Below the median window's frequency (periods longer than it) the
/// noise model passes everything; above it, the (1 − sinc)² shape with
/// the median's extra scatter.
#[test]
fn the_noise_model_passes_periods_longer_than_the_detrend_window() {
    let w = 1800.0;
    for fw in [0.25, 0.5, 1.0] {
        assert_eq!(noise_pass(fw / w, w), 1.0, "f·w {fw}");
    }
    let x = std::f64::consts::PI * 2.0;
    let h = x.sin() / x;
    let want = (1.0 - h).powi(2) + (std::f64::consts::FRAC_PI_2 - 1.0) * h * h;
    assert!((noise_pass(2.0 / w, w) - want).abs() < 1e-12);
}

/// Under four readings nothing is taken out: three can't hold a trend
/// and a quadratic through them would zero them.
#[test]
fn under_four_readings_are_not_detrended() {
    let g = Grid {
        t0: 0.0,
        step: 10.0,
        y: vec![1.0, f64::NAN, 5.0, 2.0],
    };
    let d = detrend(&g, DETREND_S);
    assert_eq!(d.y[0], 1.0);
    assert!(d.y[1].is_nan());
    assert_eq!(&d.y[2..], &[5.0, 2.0]);
}

/// The running median is at least 3 steps wide, however short the
/// window asked for.
#[test]
fn the_running_median_spans_at_least_three_steps() {
    let g = Grid {
        t0: 0.0,
        step: 600.0,
        y: vec![0.0; 4],
    };
    assert_eq!(detrend_window(&g, 60.0), Some(3));
    // A quarter of the run is one step here: still three.
    assert_eq!(detrend_window(&g, 5000.0), Some(3));
}

/// On an even fold the top harmonic (the Nyquist bin) has one coefficient
/// per pair of bins, not two: an alternating profile is given back as it
/// is, not doubled.
#[test]
fn the_top_harmonic_of_an_even_fold_is_sized_once() {
    let p: Vec<f64> = (0..8)
        .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
        .collect();
    let s = sized_shape(&p, 4, &|_, _| true);
    for (a, b) in s.iter().zip(&p) {
        assert!((a - b).abs() < 1e-12, "{s:?}");
    }
}

/// A harmonic shows when its power passes the 1% χ²(2) bar on its two
/// coefficients' share of the noise, times π/2 for the fold's medians:
/// 1.2 times the bar without the π/2 does not show, 1.2 times with it
/// does.
#[test]
fn a_harmonic_shows_past_its_bar_times_half_pi() {
    let resid = series(3600, 1.0, |_| 0.0, 1.0, 31);
    let period = 60.0;
    let shows = harmonic_shows(&resid, period);
    let rho = crate::confidence::long_run_rho(&resid.y, crate::confidence::Slow::Line);
    let n = resid.y.len() as f64;
    for h in [2usize, 3, 5] {
        let noise = variance(&resid.y) * (1.0 - rho * rho) * ar1_shape(h as f64 / period, rho);
        let chi = 9.21 * 2.0 / n * noise;
        assert!(!shows(h, 1.2 * chi), "harmonic {h}");
        assert!(
            shows(h, 1.2 * chi * std::f64::consts::FRAC_PI_2),
            "harmonic {h}"
        );
    }
}

/// A fold has one bin per grid step, from 8 to 240.
#[test]
fn a_fold_has_a_bin_a_step_from_8_to_240() {
    assert_eq!(fold_bins(3.0, 1.0), 8);
    assert_eq!(fold_bins(100.0, 1.0), 100);
    assert_eq!(fold_bins(200.0, 1.0), 200);
    assert_eq!(fold_bins(2000.0, 1.0), 240);
}

/// A line at the 5th to 8th harmonic of a found cycle is labelled so,
/// as the 2nd to 4th are.
#[test]
fn a_line_up_to_the_8th_harmonic_is_labelled() {
    let g = series(600, 1.0, sine(60.0), 0.1, 13);
    let one = analyse(&g, &LongConfig::default()).components.remove(0);
    let at = |p: f64| Component {
        period_s: p,
        resolution_s: p * p / 7200.0,
        harmonic_of: None,
        ..one.clone()
    };
    for n in [2u8, 5, 6, 8] {
        let mut cs = vec![at(600.0), at(600.0 / f64::from(n))];
        label_harmonics(&mut cs);
        assert_eq!(cs[1].harmonic_of.map(|h| h.n), Some(n));
    }
}

/// A reading whose phase falls a hair under a bin edge in floating point
/// goes to the bin above, as one exactly on it does: a 15 s cycle read
/// every second in 30 bins puts each reading on an odd bin edge, and
/// 61.5 s / 15 s × 30 comes to 122.99999999999999.
#[test]
fn a_phase_a_hair_under_a_bin_edge_goes_to_the_bin_above() {
    assert_eq!(phase_bin(61.5, 15.0, 30), (4, 3));
    assert_eq!(phase_bin(73.5, 9.0, 30), (8, 5));
    let g = Grid {
        t0: 0.0,
        step: 1.0,
        y: (0..600).map(|i| ((2 * i + 1) % 30) as f64).collect(),
    };
    let f = fold(&g, 15.0, 30);
    for b in 0..30 {
        if b % 2 == 1 {
            assert_eq!((f.counts[b], f.profile[b]), (40, b as f64), "bin {b}");
        } else {
            assert_eq!(f.counts[b], 0, "bin {b}");
        }
    }
}

/// No period longer than half the run is searched, even when a longer
/// one is asked for: under two turns can't show a cycle.
#[test]
fn no_period_past_half_the_run_is_searched() {
    let g = series(3600, 1.0, |_| 0.0, 1.0, 17);
    let r = analyse(
        &g,
        &LongConfig {
            max_period_s: Some(3600.0),
            ..Default::default()
        },
    );
    let longest = r.search.period_s.iter().copied().fold(0.0, f64::max);
    assert!(longest > 1000.0 && longest <= 1800.0 + 1e-9, "{longest}");
}

/// A component's fold is of the series before its own shape is taken
/// out, so the raster shows the cycle: a 60 s sine of size 1 folds to a
/// profile about 2 from peak to peak.
#[test]
fn a_components_fold_shows_the_cycle_itself() {
    let g = series(1800, 1.0, sine(60.0), 0.1, 5);
    let r = analyse(&g, &LongConfig::default());
    let c = &r.components[0];
    let fin = c.fold.profile.iter().copied().filter(|v| v.is_finite());
    let (lo, hi) = fin.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
        (a.min(v), b.max(v))
    });
    assert!(hi - lo > 1.6, "{lo} to {hi}");
}

/// The misfit takes out no more of a cycle's harmonics than whole turns
/// in the run: a cycle said to need 8 harmonics over 2.5 turns is taken
/// out as one of 2 would be.
#[test]
fn the_misfit_takes_out_no_more_harmonics_than_whole_turns() {
    let (step, n) = (0.25, 7200);
    let mut z = Rng::new(41);
    let mut walk = 0.0;
    let y = (0..n)
        .map(|i| {
            walk += 5e-7 * z.normal();
            walk + 1e-4 * z.normal() + 3e-4 * sine(720.0)((i as f64 + 0.5) * step)
        })
        .collect();
    let cfg = LongConfig::default();
    let d = detrend(&Grid { t0: 0.0, step, y }, cfg.detrend_s);
    let one = analyse(&series(600, 1.0, sine(60.0), 0.1, 13), &cfg)
        .components
        .remove(0);
    let with = |harmonics: u8| {
        let c = Component {
            period_s: 720.0,
            harmonics,
            ..one.clone()
        };
        format!("{:?}", misfit_without(&d, &[c], &cfg))
    };
    assert_eq!(with(8), with(2));
    assert_ne!(with(1), with(2));
}

/// A sine fit needs at least two points for each term: one period's fit
/// (a mean and two terms) takes six.
#[test]
fn a_sine_fit_needs_two_points_a_term() {
    let g = |n: usize| series(n, 1.0, sine(4.0), 0.0, 1);
    assert!(sines_fit(&g(5), &[4.0]).is_none());
    assert!(sines_fit(&g(6), &[4.0]).is_some());
}

/// Of a period and its submultiples, the shortest whose fold explains
/// 90% of the best one's is taken: a 60 s sine with a weaker 120 s one
/// beside it (5% of the variance) scores best at 120 s, where both its
/// harmonics stand, and is read at 60 s first.
#[test]
fn a_submultiple_explaining_nine_tenths_is_taken() {
    let f = |t: f64| sine(60.0)(t) + 0.25 * sine(120.0)(t + 13.0);
    let g = series(7200, 1.0, f, 0.5, 23);
    let r = analyse(&g, &LongConfig::default());
    let s = &r.search;
    let top = s.period_s[(0..s.score.len())
        .max_by(|&a, &b| s.score[a].total_cmp(&s.score[b]))
        .unwrap()];
    assert!((top - 120.0).abs() < 2.0, "best scored {top}");
    let ratio = explained_at(&r.detrended, 60.0, 60) / explained_at(&r.detrended, 120.0, 120);
    assert!(ratio > 0.9 && ratio < 0.99, "{ratio}");
    assert!(
        (r.components[0].period_s - 60.0).abs() < 0.5,
        "{}",
        r.components[0].period_s
    );
}

/// The period is refined in a second, twenty times finer pass: a clean
/// 57.3 s sine over 2 h (a resolution of 0.46 s) is read within 0.005 s,
/// where the coarse pass alone, in steps of a twentieth of a resolution,
/// can leave 0.011 s.
#[test]
fn a_period_is_refined_in_a_finer_pass() {
    let g = series(7200, 1.0, sine(57.3), 0.01, 3);
    let p = analyse(&g, &LongConfig::default()).components[0].period_s;
    assert!((p - 57.3).abs() < 0.005, "{p}");
}

fn wheels() -> Vec<Wheel> {
    [
        ("fourth wheel", 60.0),
        ("third wheel", 450.0),
        ("centre wheel", 3600.0),
    ]
    .into_iter()
    .map(|(name, period_s)| Wheel {
        name: name.into(),
        period_s,
    })
    .collect()
}

/// The nearest wheel is named with how far off it is, never the farthest.
#[test]
fn the_nearest_wheel_is_named() {
    let (_, nearest) = wheel_of(&wheels(), 61.2, 0.5);
    let (name, off) = nearest.unwrap();
    assert_eq!(name, "fourth wheel");
    assert!((off - 2.0).abs() < 1e-9, "{off}");
    assert_eq!(wheel_of(&wheels(), 440.0, 1.0).1.unwrap().0, "third wheel");
}

/// A period within 1% of a wheel's is that wheel's, though past the
/// resolution; beyond 1% and the resolution, it isn't.
#[test]
fn a_period_within_one_percent_is_the_wheels() {
    assert_eq!(
        wheel_of(&wheels(), 60.5, 0.1).0.as_deref(),
        Some("fourth wheel")
    );
    assert_eq!(wheel_of(&wheels(), 60.7, 0.1).0, None);
    assert_eq!(
        wheel_of(&wheels(), 60.7, 0.8).0.as_deref(),
        Some("fourth wheel")
    );
}

/// The caveat that a cycle may be noise is given from ρ 0.7, not below.
#[test]
fn a_cycle_may_be_noise_from_rho_0_7() {
    let g = series(1800, 1.0, sine(60.0), 0.1, 5);
    let mut r = analyse(&g, &LongConfig::default());
    assert!(!r.components.is_empty());
    for (rho, given) in [(0.6, false), (0.69, false), (0.7, true), (0.9, true)] {
        r.search.rho = rho;
        assert_eq!(period_may_be_noise(&r).is_some(), given, "ρ {rho}");
    }
}

/// The rate profile's last bin reads across to its first, as the average
/// cycle wraps: on a sine its last bin's rate is the centred slope.
#[test]
fn the_rate_profiles_last_bin_wraps_to_its_first() {
    let (n, period) = (12, 120.0);
    let p: Vec<f64> = (0..n)
        .map(|i| 1e-3 * (std::f64::consts::TAU * i as f64 / n as f64).sin())
        .collect();
    let dt = period / n as f64;
    let r = rate_profile(&p, period);
    for i in [0, n - 1] {
        let (a, b) = (p[(i + n - 1) % n], p[(i + 1) % n]);
        let want = -(b - a) / (2.0 * dt) * 86400.0;
        assert!(
            (r[i] - want).abs() < 1e-9,
            "bin {i}: {} against {want}",
            r[i]
        );
    }
}

#[test]
fn a_skipped_harmonic_is_left_out_of_the_sum_and_the_term_count() {
    // #1606: a harmonic on a wheel adds neither its power nor a term, so
    // the gamma tail is read over the terms summed, not the harmonic
    // number. White powers at j = 1: 10 at every harmonic but the 4th,
    // the wheel's, which is skipped (its power is 0, so only the term
    // count can tell the two readings apart).
    let white: Vec<f64> = (0..9)
        .map(|h| match h {
            0 | 4 => 0.0,
            _ => 10.0,
        })
        .collect();
    let got = harmonic_score(&white, 1, 8, |h| h == 4);
    // 7 terms summing to 70 at the 8th harmonic: the best count.
    assert_eq!(got.1, 8, "{got:?}");
    assert!((got.0 + log10_gamma_tail(7, 70.0)).abs() < 1e-12, "{got:?}");
    // Read over the harmonic number (8 terms), it would score lower.
    assert!(got.0 > -log10_gamma_tail(8, 70.0) + 1.0, "{got:?}");
    // With nothing skipped the 4th counts as a term of 0.
    let full = harmonic_score(&white, 1, 8, |_| false);
    assert!(
        (full.0 + log10_gamma_tail(8, 70.0)).abs() < 1e-12,
        "{full:?}"
    );
}
