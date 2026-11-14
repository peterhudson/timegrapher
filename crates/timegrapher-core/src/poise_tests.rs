use super::*;
use crate::synth::Rng;

fn pt(angle_deg: f64, rate: f64, se: f64) -> Point {
    Point {
        angle_deg,
        rate_s_per_day: rate,
        rate_se: Some(se),
        amplitude_deg: Some(180.0),
        amplitude_se: Some(1.0),
    }
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// Signed difference of two angles, degrees, -180 to 180.
fn angle_off(a: f64, b: f64) -> f64 {
    (a - b + 180.0).rem_euclid(360.0) - 180.0
}

/// Four positions 90° apart with equal ±: the closed form, a = (r0 −
/// r180)/2, b = (r90 − r270)/2, m the mean, SE(a) = ½√(se0² + se180²),
/// and the one check left, the cos 2α term (r0 + r180 − r90 − r270)/4 with
/// SE ¼√Σse².
#[test]
fn four_positions_give_the_closed_form_and_its_se() {
    let (r0, r90, r180, r270) = (-282.1, 46.2, 486.0, 54.0);
    let s = 3.0;
    let f = fit(
        &[
            pt(0.0, r0, s),
            pt(90.0, r90, s),
            pt(180.0, r180, s),
            pt(270.0, r270, s),
        ],
        None,
    )
    .unwrap();
    let (a, b) = ((r0 - r180) / 2.0, (r90 - r270) / 2.0);
    let size = a.hypot(b);
    let w = f.wave;
    assert!(close(w.mean.value, (r0 + r90 + r180 + r270) / 4.0, 1e-9));
    assert!(close(w.size.value, size, 1e-9));
    assert!(close(
        w.peak_deg.value,
        b.atan2(a).to_degrees().rem_euclid(360.0),
        1e-9
    ));
    // The user's 8-angle fit put the fast side at 181°.
    assert!(close(w.peak_deg.value, 180.6, 0.05), "{}", w.peak_deg.value);
    let se_ab = 0.5 * (2.0 * s * s).sqrt();
    assert!(close(w.size.se.unwrap(), se_ab, 1e-9));
    // The angle's ± by test inversion: asin(1.96·σ⊥/size)/1.96, which is
    // the delta method's σ⊥/size at this signal to within 0.01%.
    let inverted = (1.96 * se_ab / size).asin().to_degrees() / 1.96;
    assert!(close(w.peak_deg.se.unwrap(), inverted, 1e-9));
    assert!(close(
        inverted,
        (se_ab / size).to_degrees(),
        1e-4 * inverted
    ));
    let d = f.shape_cos2_s_per_day.unwrap();
    assert!(close(d.value, (r0 + r180 - r90 - r270) / 4.0, 1e-9));
    assert!(close(d.se.unwrap(), (4.0 * s * s).sqrt() / 4.0, 1e-9));
    assert!(
        f.shape_sin2_s_per_day.is_none(),
        "sin 2α is zero at 90° steps"
    );
}

/// Eight positions 45° apart recover a wave and both twice-round terms.
#[test]
fn eight_positions_recover_the_wave_and_both_shape_terms() {
    let rate = |deg: f64| {
        let t = deg.to_radians();
        5.0 + 30.0 * (t - 100f64.to_radians()).cos() + 4.0 * (2.0 * t).cos() - 3.0 * (2.0 * t).sin()
    };
    let pts: Vec<Point> = (0..8)
        .map(|i| pt(45.0 * f64::from(i), rate(45.0 * f64::from(i)), 1.0))
        .collect();
    let f = fit(&pts, None).unwrap();
    assert!(close(f.wave.size.value, 30.0, 1e-9));
    assert!(close(f.wave.peak_deg.value, 100.0, 1e-9));
    assert!(close(f.wave.mean.value, 5.0, 1e-9));
    assert!(close(f.shape_cos2_s_per_day.unwrap().value, 4.0, 1e-9));
    assert!(close(f.shape_sin2_s_per_day.unwrap().value, -3.0, 1e-9));
    assert_eq!(f.positions, 8);
}

/// Three positions fix the wave exactly and leave no check on its shape;
/// two cannot fix it.
#[test]
fn three_positions_fit_without_a_shape_check_and_two_do_not_fit() {
    let rate = |deg: f64| 2.0 + 10.0 * (deg.to_radians() - 1.0).cos();
    let three = [
        pt(0.0, rate(0.0), 1.0),
        pt(90.0, rate(90.0), 1.0),
        pt(180.0, rate(180.0), 1.0),
    ];
    let f = fit(&three, None).unwrap();
    assert!(close(f.wave.size.value, 10.0, 1e-9));
    assert!(close(f.wave.peak_deg.value, 1f64.to_degrees(), 1e-9));
    assert!(f.shape_cos2_s_per_day.is_none() && f.shape_sin2_s_per_day.is_none());
    assert!(fit(&three[..2], None).is_none());
}

/// A position without a ± leaves the fit's figures without one; one
/// without an amplitude leaves the heavy spot out.
#[test]
fn a_missing_se_or_amplitude_is_carried_through() {
    let mut pts: Vec<Point> = (0..4)
        .map(|i| pt(90.0 * f64::from(i), f64::from(i), 1.0))
        .collect();
    pts[2].rate_se = None;
    let f = fit(&pts, None).unwrap();
    assert!(f.wave.size.se.is_none() && f.wave.peak_deg.se.is_none());
    assert!(f.shape_cos2_s_per_day.unwrap().se.is_none());
    pts[1].amplitude_deg = None;
    let f = fit(&pts, None).unwrap();
    assert_eq!(f.withheld, Some(Withheld::NoAmplitude));
    assert!(f.heavy_spot.is_none() && f.amplitude_wave.is_none());
}

/// f = 2·J1(θ)/θ at the ruling's figures.
#[test]
fn the_amplitude_factor_matches_the_ruling() {
    assert!(close(amplitude_factor(0.0), 1.0, 1e-15));
    for (deg, want) in [
        (180.0, 0.181),
        (200.0, 0.08),
        (230.0, -0.036),
        (250.0, -0.088),
        (270.0, -0.12),
    ] {
        let f = amplitude_factor(deg);
        assert!(close(f, want, 0.006), "{deg}: {f}");
    }
    assert!(close(amplitude_factor(NULL_AMPLITUDE_DEG), 0.0, 1e-3));
    // f′ per degree: zero at rest, and the analytic series against a
    // central difference across the range a balance reaches.
    assert!(close(amplitude_factor_slope(0.0), 0.0, 1e-15));
    for deg in [30.0, 120.0, 180.0, 219.5, 260.0, 330.0] {
        let h = 1e-4;
        let diff = (amplitude_factor(deg + h) - amplitude_factor(deg - h)) / (2.0 * h);
        let slope = amplitude_factor_slope(deg);
        assert!(close(slope, diff, 1e-8), "{deg}: {slope} vs {diff}");
    }
    // Its deepest, near 300°, about -0.13.
    let deepest = (250..340)
        .map(|d| amplitude_factor(f64::from(d)))
        .fold(f64::INFINITY, f64::min);
    assert!(close(deepest, -0.132, 0.003), "{deepest}");
}

/// The null band: 219.5° within 2 SE plus 8% of the amplitude.
#[test]
fn the_null_band_reaches_two_se_and_eight_percent() {
    assert!(!in_null_band(203.0, Some(0.0)));
    assert!(in_null_band(204.0, Some(0.0)));
    assert!(in_null_band(238.0, None));
    assert!(!in_null_band(239.0, None));
    // A wide ± reaches further.
    assert!(in_null_band(197.0, Some(4.0)));
    assert!(!in_null_band(195.0, Some(4.0)));
    assert!(!in_null_band(180.0, Some(2.0)));
}

#[test]
fn hours_are_named_on_the_half_hour() {
    assert_eq!(hour_code(0.0), "12H");
    assert_eq!(hour_code(181.0), "6H");
    assert_eq!(hour_code(359.0), "12H");
    assert_eq!(hour_code(45.0), "1H30");
    assert_eq!(hour_code(315.0), "10H30");
    assert_eq!(hour_code(-90.0), "9H");
}

/// Synthetic positions: a poise error `u` (s/d at a small swing) with the
/// heavy spot lowest at rest with `psi` at the top, each position at true
/// amplitude `amp` plus `amp_wave`·cos(α − 90°), isochronism `iso` s/d per
/// degree, and rate noise of each position's ±. Each amplitude is read
/// with its declared 1° noise, then scaled by `lift` (a lift angle that
/// much out).
struct Truth {
    u: f64,
    psi: f64,
    amp: f64,
    amp_wave: f64,
    iso: f64,
    lift: f64,
}

const TRUTH: Truth = Truth {
    u: 0.0,
    psi: 130.0,
    amp: 180.0,
    amp_wave: 0.0,
    iso: 0.0,
    lift: 1.0,
};

/// Each position's rate ±, s/d: 1.5 to 2.5, 2 on average.
const RATE_SE_MEAN: f64 = 2.0;

fn synthetic(rng: &mut Rng, angles: &[f64], t: &Truth) -> Vec<Point> {
    angles
        .iter()
        .map(|&deg| {
            let a = t.amp + t.amp_wave * (deg - 90.0).to_radians().cos();
            let se = RATE_SE_MEAN - 0.5 + rng.next_f64();
            let truth = 3.0
                + t.u * amplitude_factor(a) * (deg - t.psi).to_radians().cos()
                + t.iso * (a - t.amp);
            Point {
                angle_deg: deg,
                rate_s_per_day: truth + se * rng.normal(),
                rate_se: Some(se),
                amplitude_deg: Some(t.lift * (a + rng.normal())),
                amplitude_se: Some(1.0),
            }
        })
        .collect()
}

fn eight() -> Vec<f64> {
    (0..8).map(|i| 45.0 * f64::from(i)).collect()
}

/// The poise error `u` whose rate wave at `amp` is `k` of its ± over `n`
/// positions: each coefficient's ± is about se·√(2/n), and the wave is
/// u·|f(amp)|.
fn u_at(k: f64, amp: f64, n: usize) -> f64 {
    k * RATE_SE_MEAN * (2.0 / n as f64).sqrt() / amplitude_factor(amp).abs()
}

/// Seeds the coverage runs use, fresh for the final run of the math review
/// of #1485 (3041–3059 and 5001–5040 were seen while fixing): 100 tests on
/// each.
const SEEDS: std::ops::RangeInclusive<u64> = 3061..=3069;

/// Runs `trial` 100 times on each of [`SEEDS`].
fn over_seeds(mut trial: impl FnMut(&mut Rng)) {
    for seed in SEEDS {
        let mut rng = Rng::new(seed);
        for _ in 0..100 {
            trial(&mut rng);
        }
    }
}

fn share(hits: usize, of: usize) -> f64 {
    hits as f64 / of.max(1) as f64
}

/// The ruling's acceptance cases. Either side of 219.5° (180° and 260°),
/// with a wave 4 SE clear and one far clear, the heavy spot is stated in
/// all but a few tests and its ± holds the true angle in about 95 of 100
/// of those; the measured fastest hour turns half round between the two;
/// with the lift 7% out (0.93×) at 260° it is never flipped; at 225° it
/// is withheld.
#[test]
fn the_heavy_spot_is_recovered_either_side_of_the_null_and_withheld_in_it() {
    for (amp, k, lift) in [
        (180.0, 4.0, 1.0),
        (260.0, 4.0, 1.0),
        (180.0, 25.0, 1.0),
        (260.0, 25.0, 1.0),
        (260.0, 4.0, 0.93),
    ] {
        let t = Truth {
            u: u_at(k, amp, 8),
            amp,
            lift,
            ..TRUTH
        };
        let (mut named, mut covered, mut trials) = (0, 0, 0);
        over_seeds(|rng| {
            trials += 1;
            let f = fit(&synthetic(rng, &eight(), &t), None).unwrap();
            let Some(h) = f.heavy_spot else {
                // A lift 7% out can carry a reading into the null band.
                let band = lift != 1.0 && matches!(f.withheld, Some(Withheld::NullBand { .. }));
                assert!(
                    band || f.withheld == Some(Withheld::WithinNoise),
                    "{amp} {k}"
                );
                return;
            };
            named += 1;
            let off = angle_off(h.peak_deg.value, t.psi);
            assert!(
                off.abs() < 90.0,
                "{amp}° {k} SE lift {lift}: flipped, {off}"
            );
            covered += usize::from(off.abs() <= 1.96 * h.peak_deg.se.unwrap());
            if k > 10.0 {
                let fastest = if amp < NULL_AMPLITUDE_DEG {
                    130.0
                } else {
                    310.0
                };
                assert!(angle_off(f.wave.peak_deg.value, fastest).abs() < 15.0);
            }
        });
        let (stated, cover) = (share(named, trials), share(covered, named));
        println!("heavy spot {amp}° {k} SE lift {lift}: stated {stated:.3}, covered {cover:.3}");
        if lift == 1.0 {
            assert!(stated >= 0.9, "{amp}° {k} SE: stated {stated}");
        }
        if lift == 1.0 {
            assert!((0.92..=0.975).contains(&cover), "{amp}° {k} SE: {cover}");
        }
    }
    let mut rng = Rng::new(3061);
    let t = Truth {
        u: 300.0,
        amp: 225.0,
        ..TRUTH
    };
    let f = fit(&synthetic(&mut rng, &eight(), &t), None).unwrap();
    assert!(f.heavy_spot.is_none());
    assert!(matches!(f.withheld, Some(Withheld::NullBand { .. })));
    assert!(
        f.heavy_text
            .ends_with("poise barely moves the rate. Heavy spot not stated."),
        "{}",
        f.heavy_text
    );
}

/// Seeded coverage of the measured wave's ±, with a wave 4 SE clear and
/// with none, on four and eight positions and with one of four missing:
/// the size and angle ± (or with no wave, the cos and sin components', as
/// a size is never below zero) hold the truth in about 95 of 100 tests.
/// With no wave the 2 SE gate names a direction in about e⁻² (13.5%) of
/// tests, the share of a two-dimensional normal beyond 2 SE; with a wave
/// 4 SE clear, in nearly all.
#[test]
fn the_measured_waves_se_covers_the_true_wave() {
    for (angles, k) in [
        (vec![0.0, 90.0, 180.0, 270.0], 4.0),
        (eight(), 4.0),
        (vec![0.0, 90.0, 270.0], 4.0),
        (vec![0.0, 90.0, 180.0, 270.0], 0.0),
        (eight(), 0.0),
    ] {
        let t = Truth {
            u: u_at(k, 180.0, angles.len().max(4)),
            ..TRUTH
        };
        let size = t.u * amplitude_factor(180.0);
        let (mut hits, mut named, mut trials) = ([0usize; 2], 0, 0);
        over_seeds(|rng| {
            trials += 1;
            let pts = synthetic(rng, &angles, &t);
            let w = fit(&pts, None).unwrap().wave;
            named += usize::from(clear(&w.size));
            if k > 0.0 {
                hits[0] += usize::from((w.size.value - size).abs() <= 1.96 * w.size.se.unwrap());
                let off = angle_off(w.peak_deg.value, t.psi);
                hits[1] += usize::from(off.abs() <= 1.96 * w.peak_deg.se.unwrap());
            } else {
                let rad = w.peak_deg.value.to_radians();
                let (a, b) = (w.size.value * rad.cos(), w.size.value * rad.sin());
                let ses: Vec<f64> = pts.iter().map(|p| p.rate_se.unwrap()).collect();
                let (sa, sb) = component_ses(&pts, &ses);
                hits[0] += usize::from(a.abs() <= 1.96 * sa);
                hits[1] += usize::from(b.abs() <= 1.96 * sb);
            }
        });
        let named = share(named, trials);
        let cover = hits.map(|h| share(h, trials));
        println!(
            "wave {} positions {k} SE: covered {cover:.3?}, named {named:.3}",
            angles.len()
        );
        for c in cover {
            assert!((0.92..=0.975).contains(&c), "{angles:?} {k} SE: {c}");
        }
        if k > 0.0 {
            assert!(named >= 0.9, "{angles:?}: named {named}");
        } else {
            assert!((0.09..=0.18).contains(&named), "{angles:?}: named {named}");
        }
    }
}

/// The SEs of the cos and sin coefficients of the weighted fit, worked
/// the long way for the null check.
fn component_ses(pts: &[Point], ses: &[f64]) -> (f64, f64) {
    let rows: Vec<Vec<f64>> = pts
        .iter()
        .map(|p| {
            let t = p.angle_deg.to_radians();
            vec![1.0, t.cos(), t.sin()]
        })
        .collect();
    let y = vec![0.0; pts.len()];
    let (_, cov) = wls(&rows, &y, Some(ses)).unwrap();
    let c = cov.unwrap();
    (c[1][1].sqrt(), c[2][2].sqrt())
}

/// The isochronism alias: no poise error, but the amplitude varies once
/// round the dial and the rate follows it. With no slope known, the
/// amplitude wave is named; with the slope taken off, the heavy spot is
/// withheld as within the noise.
#[test]
fn an_amplitude_wave_through_isochronism_is_named_or_taken_off() {
    let mut rng = Rng::new(3061);
    let t = Truth {
        amp_wave: 12.0,
        iso: 0.8,
        ..TRUTH
    };
    let pts = synthetic(&mut rng, &eight(), &t);
    let f = fit(&pts, None).unwrap();
    assert!(
        matches!(f.caveats.as_slice(), [Caveat::AmplitudeWave { size_deg, .. }] if close(*size_deg, 12.0, 2.0)),
        "{:?}",
        f.caveats
    );
    assert!(
        f.heavy_text
            .contains("so part of the wave may be isochronism."),
        "{}",
        f.heavy_text
    );
    assert!(clear(&f.wave.size), "the alias shows in the rate wave");
    let slope = Slope {
        s_per_day_per_deg: 0.8,
        se: 0.01,
    };
    let f = fit(&pts, Some(slope)).unwrap();
    assert!(matches!(
        f.caveats.as_slice(),
        [Caveat::IsochronismRemoved { .. }]
    ));
    assert_eq!(f.withheld, Some(Withheld::WithinNoise));
    let flat = synthetic(&mut rng, &eight(), &Truth { u: 300.0, ..TRUTH });
    assert!(fit(&flat, None).unwrap().caveats.is_empty());
}

/// The slope's ± is common to every position (Norm's correction), and
/// taking it off carries each amplitude's own noise into its rate; f(θ̂)
/// carries it into the regressor. With the amplitude going once round and
/// the slope given as the true one plus its ± drawn once per test, the
/// heavy spot's ± holds the true angle in about 95 of 100 tests where it
/// is stated, with the wave 4 and 3 SE clear on the full ±. The shares
/// stated were set before the run from a wave k SE clear in two normal
/// components, P(|Û| > 2 SE): 0.985 at 4 and 0.887 at 3, less 3 SE of a
/// share over 900 tests (0.973 and 0.855), rounded down to 0.96 and 0.85
/// for the ± itself being estimated.
#[test]
fn a_slopes_common_error_is_carried_into_the_heavy_spot() {
    let sigma = 0.15;
    for (k, least_stated) in [(4.0, 0.96), (3.0, 0.85)] {
        let base = Truth {
            amp_wave: 12.0,
            iso: 0.8,
            ..TRUTH
        };
        let t = Truth {
            u: u_full(k, sigma, &base),
            ..base
        };
        let (mut named, mut covered, mut trials) = (0, 0, 0);
        over_seeds(|rng| {
            trials += 1;
            let pts = synthetic(rng, &eight(), &t);
            let slope = Slope {
                s_per_day_per_deg: t.iso + sigma * rng.normal(),
                se: sigma,
            };
            let Some(h) = fit(&pts, Some(slope)).unwrap().heavy_spot else {
                return;
            };
            named += 1;
            let off = angle_off(h.peak_deg.value, t.psi);
            covered += usize::from(off.abs() <= 1.96 * h.peak_deg.se.unwrap());
        });
        let (stated, cover) = (share(named, trials), share(covered, named));
        println!("slope ± common, {k} SE full: stated {stated:.3}, covered {cover:.3}");
        assert!(stated >= least_stated, "{k} SE: stated {stated}");
        assert!((0.92..=0.975).contains(&cover), "{k} SE: {cover}");
    }
}

/// The poise error whose heavy-spot wave is `k` of its full ± (rate,
/// slope times amplitude ±, the slope's common ±, and f′ times amplitude
/// ±), read off a noise-free fit at the truth and solved by iteration,
/// since the f′ term grows with the wave. It starts well clear of the 2 SE
/// gate so the noise-free fit states the heavy spot.
fn u_full(k: f64, sigma: f64, t: &Truth) -> f64 {
    let mut u = u_at(3.0 * k, t.amp, 8);
    for _ in 0..20 {
        let truth = Truth { u, ..*t };
        let pts: Vec<Point> = eight()
            .iter()
            .map(|&deg| {
                let a = t.amp + t.amp_wave * (deg - 90.0).to_radians().cos();
                Point {
                    angle_deg: deg,
                    rate_s_per_day: 3.0
                        + truth.u * amplitude_factor(a) * (deg - t.psi).to_radians().cos()
                        + t.iso * (a - t.amp),
                    rate_se: Some(RATE_SE_MEAN),
                    amplitude_deg: Some(a),
                    amplitude_se: Some(1.0),
                }
            })
            .collect();
        let slope = Slope {
            s_per_day_per_deg: t.iso,
            se: sigma,
        };
        let h = fit(&pts, Some(slope)).unwrap().heavy_spot.unwrap();
        u = k * h.size.se.unwrap();
    }
    u
}

/// The 2 SE gate: a wave within its noise names no fastest hour, and one
/// with no ± names none either.
#[test]
fn no_direction_is_named_within_the_noise_or_without_a_pm() {
    let quiet: Vec<Point> = [(0.5, 0.0), (0.2, 90.0), (-0.4, 180.0), (0.1, 270.0)]
        .iter()
        .map(|&(r, a)| pt(a, r, 2.0))
        .collect();
    let f = fit(&quiet, None).unwrap();
    assert!(!clear(&f.wave.size));
    assert!(!f.wave_clear);
    assert!(
        f.short_text.contains("not clear of the noise"),
        "{}",
        f.short_text
    );
    assert!(!f.short_text.contains("fastest"), "{}", f.short_text);
    assert!(
        f.text
            .contains("not clear of the noise, so no fastest hour."),
        "{}",
        f.text
    );
    assert_eq!(f.withheld, Some(Withheld::WithinNoise));
    let mut bare = quiet.clone();
    for p in &mut bare {
        p.rate_s_per_day *= 100.0;
        p.rate_se = None;
    }
    let f = fit(&bare, None).unwrap();
    assert!(!f.short_text.contains("fastest") && f.short_text.contains("no ±"));
    assert_eq!(f.withheld, Some(Withheld::NoSe));
}

#[test]
fn the_sentence_states_the_wave_then_the_heavy_spot() {
    let mut pts: Vec<Point> = [(-282.1, 0.0), (46.2, 90.0), (486.0, 180.0), (54.0, 270.0)]
        .iter()
        .map(|&(r, a)| pt(a, r, 2.0))
        .collect();
    let f = fit(&pts, None).unwrap();
    assert_eq!(f.text, f.sentence());
    assert_eq!(
        f.text,
        "Once-round wave over 4 upright positions: 384.1 ± 1.5 s/d each way from the mean, \
         fastest at 181 ± 1° (nearest 6H at the top). Twice-round terms: cos 2α +25.9 ± \
         1.0 s/d. Amplitude 180°. Heavy spot hangs lowest with 6H at the top (181 ± 1°). The \
         heavy spot is from the fit at each position's own amplitude, so it rests on the lift \
         angle."
    );
    assert_eq!(
        f.short_text,
        "Once-round wave: 384.1 ± 1.5 s/d each way, fastest near 6H at the top · at 180°"
    );
    for p in &mut pts {
        p.amplitude_deg = Some(260.0);
    }
    let f = fit(&pts, None).unwrap();
    assert!(
        f.heavy_text
            .starts_with("Heavy spot hangs lowest with 12H at the top (1 ± 1°)"),
        "above 219.5° the heavy spot is opposite the fastest hour: {}",
        f.heavy_text
    );
    // The facts box's row says the same, as a name and a value (#1537).
    assert_eq!(
        f.heavy_spot_row(),
        (
            "Heavy spot".to_string(),
            "12H at the top, 1 ± 1°".to_string()
        )
    );
    assert_eq!(pending_text(), "Fitted once 5 positions are measured");
}

#[test]
fn a_done_take_with_too_few_positions_says_what_it_lacks() {
    assert_eq!(
        unfitted_text(4, true),
        "Needs 5 upright positions to fit; this take has 4"
    );
    // A test under way, or a take with five or more, waits as before.
    assert_eq!(unfitted_text(4, false), pending_text());
    assert_eq!(unfitted_text(5, true), pending_text());
}

/// The drawn curve passes through the fit: fastest at its peak, and it
/// closes on itself.
#[test]
fn the_curve_is_the_measured_wave() {
    let rate = |deg: f64| 7.0 + 12.0 * (deg - 90.0).to_radians().cos();
    let pts: Vec<Point> = (0..4)
        .map(|i| pt(90.0 * f64::from(i), rate(90.0 * f64::from(i)), 1.0))
        .collect();
    let c = fit(&pts, None).unwrap().curve(8);
    assert_eq!(c.len(), 9);
    for &(deg, r) in &c {
        assert!(close(r, rate(deg), 1e-9), "{deg} {r}");
    }
    assert!(close(c[2].1, 19.0, 1e-9), "fastest at 90°");
}

/// The fastest point is the curve's top, and the slowest its bottom
/// half way round: each its hour and its rate.
#[test]
fn the_fastest_point_is_the_waves_top() {
    let rate = |deg: f64| -3.0 + 5.0 * (deg - 270.0).to_radians().cos();
    let pts: Vec<Point> = (0..8)
        .map(|i| pt(45.0 * f64::from(i), rate(45.0 * f64::from(i)), 0.5))
        .collect();
    let (deg, top) = fit(&pts, None).unwrap().fastest();
    assert!(
        close(deg, 270.0, 1e-6) && close(top, 2.0, 1e-9),
        "{deg} {top}"
    );
    let (deg, bottom) = fit(&pts, None).unwrap().slowest();
    assert!(
        close(deg, 90.0, 1e-6) && close(bottom, -8.0, 1e-9),
        "{deg} {bottom}"
    );
}

/// Writes the fits the UX mock-ups are drawn from, when asked:
/// `POISE_SAMPLES=dir cargo test -p timegrapher-core poise_samples`.
#[test]
fn poise_samples() {
    let Ok(dir) = std::env::var("POISE_SAMPLES") else {
        return;
    };
    // The reddit user's table: rate and amplitude at their 0° to 315°.
    let user = [
        (-282.1, 196.0),
        (-263.1, 199.0),
        (46.2, 186.0),
        (345.6, 171.0),
        (486.0, 173.0),
        (376.9, 175.0),
        (54.0, 159.0),
        (-255.5, 175.0),
    ];
    let pts = |step: usize| -> Vec<Point> {
        user.iter()
            .enumerate()
            .step_by(step)
            .map(|(i, &(r, a))| Point {
                angle_deg: 45.0 * i as f64,
                rate_s_per_day: r,
                rate_se: Some(3.0),
                amplitude_deg: Some(a),
                amplitude_se: Some(1.0),
            })
            .collect()
    };
    for (name, step) in [("poise-4.json", 2), ("poise-8.json", 1)] {
        let f = fit(&pts(step), None).unwrap();
        let json = serde_json::to_string_pretty(&f).unwrap();
        std::fs::write(format!("{dir}/{name}"), json).unwrap();
    }
}

/// The corpus's four YM42 vertical takes (ym42_CU_8h, ym42_CL_90min,
/// ym42_CD_90min, ym42_CR_long; part 002 of each, `analyze --lift 55
/// --bph 28800`). They ran at 224–237°, inside the null band, so only the
/// measured wave is stated; the amplitude is 13° lower crown up, which is
/// named because it aliases into that wave.
/// The corpus YM42 take's four verticals, at 224–237°.
fn ym42_verticals() -> Vec<Point> {
    let rows = [
        (90.0, -15.99, 0.83, 223.8, 0.68),
        (180.0, 68.16, 2.29, 236.7, 0.43),
        (270.0, 28.80, 1.76, 236.9, 0.64),
        (0.0, -11.55, 1.68, 235.3, 0.74),
    ];
    let pts: Vec<Point> = rows
        .iter()
        .map(|&(a, r, rs, am, ams)| Point {
            angle_deg: a,
            rate_s_per_day: r,
            rate_se: Some(rs),
            amplitude_deg: Some(am),
            amplitude_se: Some(ams),
        })
        .collect();
    pts
}

#[test]
fn the_corpus_ym42_verticals_state_the_wave_and_withhold_the_heavy_spot() {
    let f = fit(&ym42_verticals(), None).unwrap();
    assert!(matches!(f.withheld, Some(Withheld::NullBand { .. })));
    assert!(f.heavy_spot.is_none());
    assert!(matches!(
        f.caveats.as_slice(),
        [Caveat::AmplitudeWave { .. }]
    ));
    assert!(clear(&f.wave.size));
    assert!(f.wave_clear, "the JSON flag agrees with the words");
    assert!((f.wave.size.value - 44.4).abs() < 0.05, "{}", f.text);
    assert!((f.wave.peak_deg.value - 217.0).abs() < 0.5, "{}", f.text);
    assert!(
        f.short_text.contains("fastest near 7H at the top"),
        "{}",
        f.short_text
    );
}

/// In the band the words say each fact once (Hugh, #1499): the range in
/// the heavy spot's line only, its caveat apart from it for a pane's
/// chip, and beside the band's note no second word on 219.5°.
#[test]
fn in_the_band_each_fact_is_said_once() {
    let f = fit(&ym42_verticals(), None).unwrap();
    let (spot, caveat) = (f.heavy_spot_line(), f.caveat_line());
    assert_eq!(
        spot,
        "At 224–237°, poise barely moves the rate. Heavy spot not stated."
    );
    assert_eq!(
        f.heavy_spot_row(),
        ("Heavy spot".to_string(), "Not stated".to_string())
    );
    assert!(
        caveat.starts_with("Amplitude also changes once round"),
        "{caveat}"
    );
    assert_eq!(f.heavy_text, format!("{spot} {caveat}"));
    assert_eq!(f.text.matches("224–237").count(), 1, "{}", f.text);
    let least = "least at 219.5° amplitude";
    assert!(f.text.contains(least), "{}", f.text);
    let beside = f.sentence_beside_note();
    assert!(!beside.contains(least), "{beside}");
    assert!(null_band_note().contains(least));
    assert!(f.text.starts_with(&beside), "{beside}");
}

/// The band's edges are where `in_null_band` turns with no ±, and the
/// words a pane shows round them come from the same constants.
#[test]
fn the_null_bands_edges_and_words_follow_its_rule() {
    let (lo, hi) = null_band_deg();
    assert!(
        close(lo, 203.24, 0.01) && close(hi, 238.59, 0.01),
        "{lo} {hi}"
    );
    assert!(in_null_band(lo + 0.01, None) && !in_null_band(lo - 0.01, None));
    assert!(in_null_band(hi - 0.01, None) && !in_null_band(hi + 0.01, None));
    let note = null_band_note();
    assert!(
        note.contains("219.5°") && note.contains("203°") && note.contains("239°"),
        "{note}"
    );
    assert_eq!(amplitude_now_text(184.4, None), "Amplitude now 184°");
    assert_eq!(
        amplitude_now_text(226.0, Some(0.6)),
        "Amplitude now 226 ± 1°: near 219.5°, where poise barely moves the rate"
    );
}

/// A reading becomes a point at its hour's angle with core's ±s; lying
/// flat it is none. The take is long enough for a rate ±: 30 of its 10 s
/// rates (#1470).
#[test]
fn a_reading_gives_its_point_upright_only() {
    let s = crate::scenarios::steady_red(330.0, 0.0, 7);
    let log = crate::scenarios::beat_log(&s, 7);
    let m = crate::session::measure(&log, None, 0.0, log.duration_s);
    use crate::session::Position as P;
    let p = point(P::H4h30, &m).unwrap();
    assert_eq!(p.angle_deg, 135.0);
    assert_eq!(Some(p.rate_s_per_day), m.rate_s_per_day);
    assert_eq!(p.rate_se, m.confidence.and_then(|c| c.rate.se));
    assert_eq!(p.amplitude_deg, m.amplitude_deg);
    assert!(p.rate_se.is_some() && p.amplitude_se.is_some());
    assert!(point(P::CH, &m).is_none() && point(P::CB, &m).is_none());
}
