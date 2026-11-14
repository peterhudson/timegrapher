//! Dynamic poise: the once-round wave of rate as the watch turns in its
//! own plane, and the heavy spot it points to (#1474; the math ruling is
//! the math standards guide's Poise entry).
//!
//! The positions are upright, at dial angle α: the hour mark at the top,
//! clockwise from 12H (12H 0°, 3H 90°, 6H 180°, 9H 270°, and the 45° steps
//! between). Two fits are made, and kept apart:
//!
//! - **The measured wave (a fact).** rate(α) = m + A·cos(α − φ): its size A
//!   each way from the mean (half the peak to peak) and the angle φ at
//!   which the watch ran fastest, each with its ±.
//! - **The heavy spot (an inference).** A poise error U changes the rate by
//!   U·f(θ)·cos(α − ψ), with f(θ) = 2·J1(θ)/θ of each position's own
//!   amplitude θ. f is 1 for a small swing, 0.18 at 180°, 0 at 219.5° and
//!   negative above, so the same heavy spot makes the watch fastest with
//!   it low below 219.5° and with it high above. The regressors are
//!   f(θ_i)·cos α_i and f(θ_i)·sin α_i and a constant, so ψ (the hour at the
//!   top when the heavy spot is lowest at rest) rests on the amplitude, and
//!   so on the lift angle. It is withheld when any position's amplitude is
//!   within reach of 219.5° ([`in_null_band`]).
//!
//! Both are weighted least squares, each position by 1/SE² of its rate,
//! and the ± comes from the fit; with any position lacking a ± the fit is
//! unweighted and gives none. The twice-round terms (cos 2α, sin 2α), fitted
//! beside the wave, are a check on the residual and no part of the answer:
//! pivots and the hairspring give them, poise does not.
//!
//! The amplitude itself can vary round the dial (friction differs by
//! position), and through the watch's isochronism that moves the rate in
//! the same once-round pattern. With a measured isochronism slope it is
//! taken off before the heavy spot is fitted; with none, a once-round
//! amplitude wave clear of zero by 2 SE is named as a caveat.

use crate::dsp;
use crate::figure_text::{base_digits, figure_with_pm, fixed_text};
use serde::Serialize;

/// The fewest upright positions a wave is fitted from: it has three
/// unknowns.
pub const MIN_POSITIONS: usize = 3;

/// The amplitude at which poise does not move the rate, degrees: the first
/// zero of J1, 3.8317 rad.
pub const NULL_AMPLITUDE_DEG: f64 = 219.5;

/// The positions measured before a pane shows the wave: three fix it,
/// and five leave two to check its shape (Hugh's spec, 2026-11-12).
pub const MIN_SHOWN: usize = 5;

/// What a pane says until [`MIN_SHOWN`] positions are in.
pub fn pending_text() -> String {
    format!("Fitted once {MIN_SHOWN} positions are measured")
}

/// What a pane says with no fit: on a take that is done (`done`, opened
/// again and not being recorded) with one to fewer than [`MIN_SHOWN`] of
/// its `measured` upright positions, what it lacks, as no more will come
/// (Hugh on #1622); else [`pending_text`], as for a take with no stops at
/// all, where there is nothing to explain.
pub fn unfitted_text(measured: usize, done: bool) -> String {
    if done && (1..MIN_SHOWN).contains(&measured) {
        format!("Needs {MIN_SHOWN} upright positions to fit; this take has {measured}")
    } else {
        pending_text()
    }
}

/// The share of each amplitude added either side of its 2 SE before asking
/// whether it reaches [`NULL_AMPLITUDE_DEG`]. Amplitude scales with the
/// lift angle, and a catalogue lift can be several percent out: the one
/// measured (DD3285) moved 7.3%, 55° to 59°. The math ruling of
/// 2026-11-12; a later change may drop it for a measured lift.
pub const NULL_BAND_SHARE: f64 = 0.08;

/// How many standard errors clear of zero a once-round amplitude wave must
/// be to be named, and how far either side of an amplitude the null band
/// reaches: the guide's 2 SE ([`crate::confidence::CLEAR_SE`]).
pub use crate::confidence::CLEAR_SE;
/// The two-sided 95% normal quantile, for the angle's interval.
const Z95: f64 = 1.96;

/// Whether a size stands clear of zero by [`CLEAR_SE`] of its ±: the
/// gate before any direction is named. A size with no ± never does.
pub fn clear(size: &Term) -> bool {
    size.value > 0.0 && crate::confidence::clear_of_zero(size.value, size.se)
}

/// One upright position's figures for the fits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// The angle of the hour mark at the top, degrees ([`hour_code`]).
    pub angle_deg: f64,
    pub rate_s_per_day: f64,
    pub rate_se: Option<f64>,
    pub amplitude_deg: Option<f64>,
    pub amplitude_se: Option<f64>,
}

/// A watch's isochronism: how its rate moves with amplitude, s/d per
/// degree, with its ± (core's isochronism from a run-down).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Slope {
    pub s_per_day_per_deg: f64,
    pub se: f64,
}

/// A fitted figure and its standard error.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Term {
    pub value: f64,
    pub se: Option<f64>,
}

/// A once-round wave: its mean, its size each way from the mean, and the
/// angle of its peak, degrees 0 to 360.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Wave {
    pub mean: Term,
    pub size: Term,
    pub peak_deg: Term,
}

/// Why the heavy spot is not inferred.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "why", rename_all = "snake_case")]
pub enum Withheld {
    /// A position's amplitude is within reach of 219.5°, where poise
    /// barely moves the rate: the first such, its angle and amplitude.
    NullBand { angle_deg: f64, amplitude_deg: f64 },
    /// A position has no amplitude.
    NoAmplitude,
    /// The amplitude-weighted fit cannot be solved.
    NoFit,
    /// The positions have no ±, so the size cannot be told from noise.
    NoSe,
    /// The poise error's size is within [`CLEAR_SE`] of zero.
    WithinNoise,
}

/// What limits the heavy spot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Caveat {
    /// The amplitude itself varies once round the dial, clear of zero by
    /// [`CLEAR_SE`]: through isochronism part of the rate wave may come
    /// from that. Its size each way, degrees.
    AmplitudeWave { size_deg: f64, se_deg: f64 },
    /// This isochronism was taken off the rates before the heavy spot was
    /// fitted. `amplitude_se` is false when a position's amplitude had no
    /// ±, so the slope times it could not be carried into that rate's ±.
    IsochronismRemoved { slope: Slope, amplitude_se: bool },
}

/// The two fits over the upright positions.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PoiseFit {
    pub positions: usize,
    /// The measured rate wave, s/d and degrees: a fact. Its peak is named
    /// in words only when its size is [`clear`] of the noise.
    pub wave: Wave,
    /// Whether the wave's size is [`clear`] of zero by 2 SE, so its
    /// fastest hour is named: for agents reading `wave.peak_deg`.
    pub wave_clear: bool,
    /// The twice-round terms beside it, s/d, where the positions leave
    /// room: cos 2α from four, sin 2α too from five.
    pub shape_cos2_s_per_day: Option<Term>,
    pub shape_sin2_s_per_day: Option<Term>,
    /// The inferred poise error: its size U (s/d at a small swing) and the
    /// hour at the top (degrees) when the heavy spot is lowest at rest.
    pub heavy_spot: Option<Wave>,
    pub withheld: Option<Withheld>,
    pub caveats: Vec<Caveat>,
    /// The amplitude's own once-round wave, degrees, where every position
    /// has one.
    pub amplitude_wave: Option<Wave>,
    pub amplitude_min_deg: Option<f64>,
    pub amplitude_max_deg: Option<f64>,
    /// The fit in words ([`PoiseFit::sentence`]) and in one line
    /// ([`PoiseFit::short`]).
    pub text: String,
    pub short_text: String,
    /// The heavy spot's line ([`PoiseFit::heavy_line`]).
    pub heavy_text: String,
}

/// f(θ) = 2·J1(θ)/θ of an amplitude in degrees: how much of a small
/// swing's poise effect is left at that amplitude.
pub fn amplitude_factor(amplitude_deg: f64) -> f64 {
    factor_series(amplitude_deg.to_radians()).0
}

/// f′(θ), per degree of amplitude: how fast [`amplitude_factor`] moves,
/// which carries an amplitude's noise into the heavy-spot fit.
pub fn amplitude_factor_slope(amplitude_deg: f64) -> f64 {
    factor_series(amplitude_deg.to_radians()).1 * std::f64::consts::PI / 180.0
}

/// 2·J1(x)/x = Σ (−1)^k (x/2)^(2k) / (k!(k+1)!) and its derivative in x,
/// Σ k·c_k·q^(k−1)·(−x/2) with q = −x²/4; the terms shrink fast enough
/// over any amplitude a balance reaches.
fn factor_series(x: f64) -> (f64, f64) {
    let q = -(x * x) / 4.0;
    let (mut c, mut q_pow) = (1.0, 1.0);
    let (mut sum, mut slope) = (1.0, 0.0);
    for k in 1..60 {
        c /= k as f64 * (k + 1) as f64;
        slope += k as f64 * c * q_pow * (-x / 2.0);
        q_pow *= q;
        let term = c * q_pow;
        sum += term;
        if term.abs() < 1e-17 && k > 1 {
            break;
        }
    }
    (sum, slope)
}

/// Whether an amplitude, with its ±, may sit where poise barely moves the
/// rate: [θ − 2·SE, θ + 2·SE] widened by [`NULL_BAND_SHARE`] of θ holds
/// [`NULL_AMPLITUDE_DEG`].
pub fn in_null_band(amplitude_deg: f64, se: Option<f64>) -> bool {
    let se = se.filter(|s| s.is_finite()).unwrap_or(0.0).abs();
    let reach = CLEAR_SE * se + NULL_BAND_SHARE * amplitude_deg.abs();
    (amplitude_deg - NULL_AMPLITUDE_DEG).abs() <= reach
}

/// The amplitudes [`in_null_band`] holds when they have no ±, degrees:
/// about 203 to 239.
pub fn null_band_deg() -> (f64, f64) {
    (
        NULL_AMPLITUDE_DEG / (1.0 + NULL_BAND_SHARE),
        NULL_AMPLITUDE_DEG / (1.0 - NULL_BAND_SHARE),
    )
}

/// What the band near [`NULL_AMPLITUDE_DEG`] means for a poise test, for
/// a pane's Info panel: why the heavy spot is not stated in it, and the
/// amplitudes outside it.
pub fn null_band_note() -> String {
    let (lo, hi) = null_band_deg();
    let d = base_digits::AMPLITUDE;
    format!(
        "Poise moves the rate least at {}° amplitude, and the other way above it, so the \
         heavy spot is not stated where any position may be between {}° and {}°. A run let \
         down below {}°, or fully wound above {}°, can show it.",
        fixed_text(NULL_AMPLITUDE_DEG, 1),
        fixed_text(lo, d),
        fixed_text(hi, d),
        fixed_text(lo, d),
        fixed_text(hi, d),
    )
}

/// The amplitude so far, for a test's banner before its first position
/// is in, with whether it sits in the band ([`in_null_band`]).
pub fn amplitude_now_text(amplitude_deg: f64, se: Option<f64>) -> String {
    let a = figure_with_pm(amplitude_deg, se, base_digits::AMPLITUDE, false);
    if in_null_band(amplitude_deg, se) {
        format!(
            "Amplitude now {a}°: near {}°, where poise barely moves the rate",
            fixed_text(NULL_AMPLITUDE_DEG, 1)
        )
    } else {
        format!("Amplitude now {a}°")
    }
}

/// An upright position's [`Point`] from its reading, with the ±s core
/// gives it. None lying flat, or with no rate.
pub fn point(position: crate::session::Position, m: &crate::session::Measurement) -> Option<Point> {
    let c = m.confidence;
    Some(Point {
        angle_deg: position.top_deg()?,
        rate_s_per_day: m.rate_s_per_day?,
        rate_se: c.and_then(|c| c.rate.se),
        amplitude_deg: m.amplitude_deg,
        amplitude_se: c.and_then(|c| c.amplitude.se),
    })
}

/// The fit a test shows over its upright readings ([`fit`]), once
/// [`MIN_SHOWN`] of them are in; before that, None, and a pane says
/// [`pending_text`] in its place. The app's Dynamic Poise pane and the
/// CLI's and MCP's session report read this one (#1473, #1474).
pub fn shown_fit<'a>(
    readings: impl IntoIterator<Item = (crate::session::Position, &'a crate::session::Measurement)>,
) -> Option<PoiseFit> {
    let points: Vec<Point> = readings
        .into_iter()
        .filter_map(|(p, m)| point(p, m))
        .collect();
    if points.len() < MIN_SHOWN {
        return None;
    }
    fit(&points, None)
}

/// The poise fit of one state of wind in a session ([`by_state`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StateFit {
    /// Hours after full wind, 0 at full wind; `None` where the wind was
    /// not given, as the session report's states name them.
    pub wind_h: Option<f64>,
    pub fit: PoiseFit,
}

/// The [`shown_fit`] of each state of wind whose upright readings give
/// one, fullest first and wind not given last, the states grouped as the
/// session report's are (hours to a tenth): what a session's report says
/// of poise, in the CLI and MCP as in the app (#1473, #1474).
pub fn by_state(readings: &[crate::session::Reading]) -> Vec<StateFit> {
    use crate::session::{wind_key, wind_keys};
    wind_keys(readings)
        .into_iter()
        .filter_map(|k| {
            let at = readings.iter().filter(|r| wind_key(r.wind_h) == k);
            let fit = shown_fit(at.map(|r| (r.position, &r.measurement)))?;
            Some(StateFit {
                wind_h: k.map(|k| k as f64 / 10.0),
                fit,
            })
        })
        .collect()
}

/// Witschi's code of the hour nearest `deg` on the half hour: "12H" to
/// "11H", and "1H30" and so on for the 45° steps between.
pub fn hour_code(deg: f64) -> &'static str {
    let half_hours = ((deg.rem_euclid(360.0) / 15.0).round() as usize) % 24;
    crate::session::HOUR_CODES[half_hours]
}

/// Both fits over `points`, one per upright position, taking `slope` off
/// before the heavy spot where the watch's isochronism is known. None with
/// fewer than [`MIN_POSITIONS`] or angles that cannot fix a wave.
pub fn fit(points: &[Point], slope: Option<Slope>) -> Option<PoiseFit> {
    let pts: Vec<Point> = points
        .iter()
        .copied()
        .filter(|p| p.angle_deg.is_finite() && p.rate_s_per_day.is_finite())
        .collect();
    if pts.len() < MIN_POSITIONS {
        return None;
    }
    let rates: Vec<f64> = pts.iter().map(|p| p.rate_s_per_day).collect();
    let rate_ses = all_se(pts.iter().map(|p| p.rate_se));
    let ones = vec![1.0; pts.len()];
    let wave = wave_fit(&pts, &ones, &rates, rate_ses.as_deref(), None)?;
    let (shape_cos2_s_per_day, shape_sin2_s_per_day) = shape_fit(&pts, &rates, rate_ses.as_deref());
    let amps: Option<Vec<f64>> = pts
        .iter()
        .map(|p| p.amplitude_deg.filter(|a| a.is_finite()))
        .collect();
    let amplitude_wave = amps.as_ref().and_then(|a| {
        let ses = all_se(pts.iter().map(|p| p.amplitude_se));
        wave_fit(&pts, &ones, a, ses.as_deref(), None)
    });
    let mut caveats = Vec::new();
    // The amplitude's own wave aliases into the measured wave too, so it is
    // named whether or not the heavy spot is stated.
    if let (None, Some(w)) = (slope, amplitude_wave) {
        if let (true, Some(se)) = (clear(&w.size), w.size.se) {
            caveats.push(Caveat::AmplitudeWave {
                size_deg: w.size.value,
                se_deg: se,
            });
        }
    }
    let (heavy_spot, withheld) = match &amps {
        None => (None, Some(Withheld::NoAmplitude)),
        Some(a) => {
            let ses = rate_ses.as_deref();
            heavy_fit(&pts, a, ses, slope, &mut caveats)
        }
    };
    let flat = amps.iter().flatten().copied();
    let mut out = PoiseFit {
        positions: pts.len(),
        wave_clear: clear(&wave.size),
        wave,
        shape_cos2_s_per_day,
        shape_sin2_s_per_day,
        heavy_spot,
        withheld,
        caveats,
        amplitude_wave,
        amplitude_min_deg: flat.clone().reduce(f64::min),
        amplitude_max_deg: flat.reduce(f64::max),
        text: String::new(),
        short_text: String::new(),
        heavy_text: String::new(),
    };
    out.text = out.sentence();
    out.short_text = out.short();
    out.heavy_text = out.heavy_line();
    Some(out)
}

/// The heavy spot, or why it is withheld; any caveat on it goes into
/// `caveats`.
fn heavy_fit(
    pts: &[Point],
    amps: &[f64],
    rate_ses: Option<&[f64]>,
    slope: Option<Slope>,
    caveats: &mut Vec<Caveat>,
) -> (Option<Wave>, Option<Withheld>) {
    if let Some((p, &a)) = pts
        .iter()
        .zip(amps)
        .find(|(p, &a)| in_null_band(a, p.amplitude_se))
    {
        let why = Withheld::NullBand {
            angle_deg: p.angle_deg,
            amplitude_deg: a,
        };
        return (None, Some(why));
    }
    let mut rates: Vec<f64> = pts.iter().map(|p| p.rate_s_per_day).collect();
    let mean = amps.iter().sum::<f64>() / amps.len() as f64;
    let lever: Vec<f64> = amps.iter().map(|a| a - mean).collect();
    if let Some(s) = slope {
        for (r, d) in rates.iter_mut().zip(&lever) {
            *r -= s.s_per_day_per_deg * d;
        }
    }
    // Taking s·(θ_i − mean θ) off puts each amplitude's own noise into its
    // rate: s·SE_θi, separate at each position, so it widens that rate's ±.
    let carried: Option<Vec<f64>> = match (slope, rate_ses) {
        (Some(s), Some(r)) => pts
            .iter()
            .zip(r)
            .map(|(p, se)| p.amplitude_se.map(|a| se.hypot(s.s_per_day_per_deg * a)))
            .collect(),
        _ => None,
    };
    if let Some(s) = slope {
        caveats.push(Caveat::IsochronismRemoved {
            slope: s,
            amplitude_se: carried.is_some(),
        });
    }
    let ses = carried.as_deref().or(rate_ses);
    let f: Vec<f64> = amps.iter().map(|&a| amplitude_factor(a)).collect();
    let common = slope.map(|s| (lever.as_slice(), s.se));
    let first = wave_fit(pts, &f, &rates, ses, common);
    // f(θ̂) carries the amplitude's noise into the regressor: Û·f′(θ_i)·
    // SE_θi·cos(α_i − ψ̂) at each position, separately. Plugged in from
    // the first fit, then fitted once more.
    let widened: Option<Vec<f64>> = match (&first, ses) {
        (Some(w), Some(r)) => pts
            .iter()
            .zip(amps)
            .zip(r)
            .map(|((p, &a), se)| {
                let off = (p.angle_deg - w.peak_deg.value).to_radians().cos();
                let e = w.size.value * amplitude_factor_slope(a) * off;
                p.amplitude_se.map(|sa| se.hypot(e * sa))
            })
            .collect(),
        _ => None,
    };
    let fitted = match widened.as_deref() {
        Some(r) => wave_fit(pts, &f, &rates, Some(r), common),
        None => first,
    };
    match fitted {
        Some(w) if w.size.se.is_none() => (None, Some(Withheld::NoSe)),
        Some(w) if !clear(&w.size) => (None, Some(Withheld::WithinNoise)),
        Some(w) => (Some(w), None),
        None => (None, Some(Withheld::NoFit)),
    }
}

/// Every standard error, or none if any is missing or not positive.
fn all_se(ses: impl Iterator<Item = Option<f64>>) -> Option<Vec<f64>> {
    ses.map(|s| s.filter(|s| s.is_finite() && *s > 0.0))
        .collect()
}

/// y = m + scale_i·(a·cos α_i + b·sin α_i), weighted by 1/se² where every
/// point has one: the wave's mean, size √(a² + b²) and peak angle.
/// `common` is an error shared by every point, σ·d_i (a slope's ± times
/// each point's lever d): σ²·g·gᵀ is added to the covariance, g the fit's
/// response to d, and the weights stay the points' own.
fn wave_fit(
    pts: &[Point],
    scale: &[f64],
    y: &[f64],
    ses: Option<&[f64]>,
    common: Option<(&[f64], f64)>,
) -> Option<Wave> {
    let rows: Vec<Vec<f64>> = pts
        .iter()
        .zip(scale)
        .map(|(p, k)| {
            let t = p.angle_deg.to_radians();
            vec![1.0, k * t.cos(), k * t.sin()]
        })
        .collect();
    let (beta, mut cov) = wls(&rows, y, ses)?;
    if let (Some(c), Some((d, sigma))) = (cov.as_mut(), common) {
        let (g, _) = wls(&rows, d, ses)?;
        for (i, row) in c.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v += sigma * sigma * g[i] * g[j];
            }
        }
    }
    let (a, b) = (beta[1], beta[2]);
    let size = a.hypot(b);
    let (size_se, peak_se) = match &cov {
        Some(c) if size > 0.0 => {
            let quad = |g: [f64; 2]| {
                (g[0] * g[0] * c[1][1] + 2.0 * g[0] * g[1] * c[1][2] + g[1] * g[1] * c[2][2])
                    .max(0.0)
                    .sqrt()
            };
            // The angle by test inversion: under a true direction the
            // estimate's part across it is N(0, σ⊥²) whatever the size, so
            // the 95% half-width is asin(1.96·σ⊥/size); shown as one SE.
            let across = quad([-b / size, a / size]);
            let half = (Z95 * across / size).min(1.0).asin();
            (
                Some(quad([a / size, b / size])),
                Some(half.to_degrees() / Z95),
            )
        }
        _ => (None, None),
    };
    Some(Wave {
        mean: Term {
            value: beta[0],
            se: cov.as_ref().map(|c| c[0][0].max(0.0).sqrt()),
        },
        size: Term {
            value: size,
            se: size_se,
        },
        peak_deg: Term {
            value: b.atan2(a).to_degrees().rem_euclid(360.0),
            se: peak_se,
        },
    })
}

/// The twice-round terms, fitted beside the once-round wave: cos 2α where
/// the positions leave room for a fourth unknown, sin 2α where they leave
/// room for one more, each only where it is not zero at every position
/// (sin 2α is, at 90° steps).
fn shape_fit(pts: &[Point], y: &[f64], ses: Option<&[f64]>) -> (Option<Term>, Option<Term>) {
    let n = pts.len();
    let col = |f: fn(f64) -> f64| -> Vec<f64> {
        pts.iter()
            .map(|p| f(2.0 * p.angle_deg.to_radians()))
            .collect()
    };
    let readable = |c: &[f64]| c.iter().map(|x| x * x).sum::<f64>() > 1e-9 * n as f64;
    let (c2, s2) = (col(f64::cos), col(f64::sin));
    let use_c2 = n > MIN_POSITIONS && readable(&c2);
    let use_s2 = n > MIN_POSITIONS + usize::from(use_c2) && readable(&s2);
    if !use_c2 && !use_s2 {
        return (None, None);
    }
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            let t = pts[i].angle_deg.to_radians();
            let mut r = vec![1.0, t.cos(), t.sin()];
            r.extend(use_c2.then_some(c2[i]));
            r.extend(use_s2.then_some(s2[i]));
            r
        })
        .collect();
    let Some((beta, cov)) = wls(&rows, y, ses) else {
        return (None, None);
    };
    let term = |k: usize| Term {
        value: beta[k],
        se: cov.as_ref().map(|c| c[k][k].max(0.0).sqrt()),
    };
    let c2_term = use_c2.then(|| term(3));
    let s2_term = use_s2.then(|| term(3 + usize::from(use_c2)));
    (c2_term, s2_term)
}

type Matrix = Vec<Vec<f64>>;

/// Least squares of `y` on `rows` by core's one solve
/// ([`dsp::least_squares_cov`]): weighted by 1/se² with `ses`, with the
/// covariance (XᵀWX)⁻¹; unweighted and with no covariance without. Rows
/// of 3, 4 or 5 columns, as the wave and its twice-round check take.
fn wls(rows: &[Vec<f64>], y: &[f64], ses: Option<&[f64]>) -> Option<(Vec<f64>, Option<Matrix>)> {
    let k = rows.first()?.len();
    if rows.len() < k {
        return None;
    }
    let w: Vec<f64> = (0..rows.len())
        .map(|i| ses.map_or(1.0, |s| 1.0 / (s[i] * s[i])))
        .collect();
    let (beta, cov) = match k {
        3 => fixed::<3>(rows, y, &w)?,
        4 => fixed::<4>(rows, y, &w)?,
        5 => fixed::<5>(rows, y, &w)?,
        _ => return None,
    };
    Some((beta, ses.map(|_| cov)))
}

/// [`wls`] at `N` columns. A pivot under 1e-10 of the summed weights is
/// taken as singular: every column is a cosine, a sine or 1.
fn fixed<const N: usize>(rows: &[Vec<f64>], y: &[f64], w: &[f64]) -> Option<(Vec<f64>, Matrix)> {
    let tiny = 1e-10 * w.iter().sum::<f64>();
    let it = rows
        .iter()
        .zip(y)
        .zip(w)
        .map(|((r, &y), &w)| (std::array::from_fn(|i| r[i]), y, w));
    let (x, cov) = dsp::least_squares_cov::<N>(it, tiny)?;
    Some((x.to_vec(), cov.iter().map(|r| r.to_vec()).collect()))
}

impl PoiseFit {
    /// The fit in words: the measured wave, its twice-round check and the
    /// amplitude, then the heavy spot or why it is not given, and any
    /// caveat on it.
    pub fn sentence(&self) -> String {
        self.sentence_with(true)
    }

    /// [`PoiseFit::sentence`] for beside [`null_band_note`], as a pane's
    /// Info panel shows them: without its sentence on 219.5°, which the
    /// note gives, so each fact is said once.
    pub fn sentence_beside_note(&self) -> String {
        self.sentence_with(false)
    }

    /// The sentence, with its closing word on 219.5° where the heavy spot
    /// is not stated when `explain`.
    fn sentence_with(&self, explain: bool) -> String {
        let rate = |t: &Term, signed| figure_with_pm(t.value, t.se, base_digits::RATE, signed);
        let w = &self.wave;
        let mut s = format!(
            "Once-round wave over {} upright positions: {} s/d each way from the mean",
            self.positions,
            rate(&w.size, false),
        );
        s.push_str(&match (clear(&w.size), w.size.se.is_some()) {
            (true, _) => format!(
                ", fastest at {}° (nearest {} at the top).",
                figure_with_pm(w.peak_deg.value, w.peak_deg.se, 0, false),
                hour_code(w.peak_deg.value),
            ),
            (false, true) => ", not clear of the noise, so no fastest hour.".into(),
            (false, false) => ", with no ±, so no fastest hour.".into(),
        });
        let shape: Vec<String> = [
            ("cos 2α", &self.shape_cos2_s_per_day),
            ("sin 2α", &self.shape_sin2_s_per_day),
        ]
        .iter()
        .filter_map(|(name, t)| t.as_ref().map(|t| format!("{name} {} s/d", rate(t, true))))
        .collect();
        if !shape.is_empty() {
            s.push_str(&format!(" Twice-round terms: {}.", shape.join(", ")));
        }
        // In the band the heavy spot's line opens with the range.
        let band = matches!(self.withheld, Some(Withheld::NullBand { .. }));
        if let Some(range) = self.amplitude_range().filter(|_| !band) {
            s.push_str(&format!(" Amplitude {range}°."));
        }
        s.push(' ');
        s.push_str(&self.heavy_line());
        if self.heavy_spot.is_some() {
            s.push_str(
                " The heavy spot is from the fit at each position's own amplitude, so it rests \
                 on the lift angle.",
            );
        } else if explain {
            s.push_str(&format!(
                " Poise moves the rate least at {}° amplitude, and the other way above it.",
                fixed_text(NULL_AMPLITUDE_DEG, 1)
            ));
        }
        s
    }

    /// The second line: the heavy spot, or why it is not stated, then
    /// any caveat on it. Inferred, where the title line is measured.
    pub fn heavy_line(&self) -> String {
        let mut s = self.heavy_spot_line();
        let caveats = self.caveat_line();
        if !caveats.is_empty() {
            s.push(' ');
            s.push_str(&caveats);
        }
        s
    }

    /// The heavy spot, or why it is not stated, alone: the
    /// [`PoiseFit::heavy_line`] without its caveats.
    pub fn heavy_spot_line(&self) -> String {
        match (&self.heavy_spot, self.withheld) {
            (Some(h), _) => {
                let (hour, deg) = spot_at(h);
                format!("Heavy spot hangs lowest with {hour} at the top ({deg}°).")
            }
            (None, Some(Withheld::NullBand { .. })) => format!(
                "At {}°, poise barely moves the rate. Heavy spot not stated.",
                self.amplitude_range().unwrap_or_default()
            ),
            (None, Some(Withheld::NoAmplitude)) => {
                "Heavy spot not stated: a position has no amplitude.".into()
            }
            (None, Some(Withheld::WithinNoise)) => {
                "Heavy spot not stated: its wave is not clear of the noise.".into()
            }
            (None, Some(Withheld::NoSe)) => "Heavy spot not stated: no ± to test it by.".into(),
            (None, _) => "Heavy spot not stated.".into(),
        }
    }

    /// The heavy spot as the pane's facts box gives it, a name and a
    /// value: ("Heavy spot", "6H at the top, 180 ± 12°"), or "Not stated"
    /// where it is withheld; why, and any caveat, are in
    /// [`PoiseFit::sentence`] (Hugh on #1537).
    pub fn heavy_spot_row(&self) -> (String, String) {
        let value = match &self.heavy_spot {
            Some(h) => {
                let (hour, deg) = spot_at(h);
                format!("{hour} at the top, {deg}°")
            }
            None => "Not stated".into(),
        };
        ("Heavy spot".into(), value)
    }

    /// The caveats on the heavy spot in words, one after another, or
    /// nothing.
    pub fn caveat_line(&self) -> String {
        let words: Vec<String> = self.caveats.iter().map(caveat_text).collect();
        words.join(" ")
    }

    /// The fit in one line: the wave's size, the hour nearest its peak,
    /// and the amplitude it was read at.
    pub fn short(&self) -> String {
        let size = &self.wave.size;
        let mut s = format!(
            "Once-round wave: {} s/d each way",
            figure_with_pm(size.value, size.se, base_digits::RATE, false),
        );
        s.push_str(&match (clear(size), size.se.is_some()) {
            (true, _) => format!(
                ", fastest near {} at the top",
                hour_code(self.wave.peak_deg.value)
            ),
            (false, true) => ", not clear of the noise".into(),
            (false, false) => ", no ±".into(),
        });
        if let Some(range) = self.amplitude_range() {
            s.push_str(&format!(" · at {range}°"));
        }
        s
    }

    /// The amplitude range as text, "159–186", or one figure when both
    /// ends round alike.
    fn amplitude_range(&self) -> Option<String> {
        let d = base_digits::AMPLITUDE;
        let lo = fixed_text(self.amplitude_min_deg?, d);
        let hi = fixed_text(self.amplitude_max_deg?, d);
        Some(if lo == hi { lo } else { format!("{lo}–{hi}") })
    }

    /// The measured wave's fastest point, (angle 0–360°, rate s/d): where
    /// a picture draws its fastest line to.
    pub fn fastest(&self) -> (f64, f64) {
        let w = &self.wave;
        (
            w.peak_deg.value.rem_euclid(360.0),
            w.mean.value + w.size.value,
        )
    }

    /// The measured wave's slowest point, (angle 0–360°, rate s/d): half
    /// way round from [`PoiseFit::fastest`], where a picture's labels sit
    /// clear of the wave.
    pub fn slowest(&self) -> (f64, f64) {
        let w = &self.wave;
        (
            (w.peak_deg.value + 180.0).rem_euclid(360.0),
            w.mean.value - w.size.value,
        )
    }

    /// The measured wave at `steps` + 1 evenly spaced angles from 0° to
    /// 360°, (angle, rate s/d): the one source of the drawn curve, on a
    /// line or round a dial.
    pub fn curve(&self, steps: usize) -> Vec<(f64, f64)> {
        let steps = steps.max(1);
        let w = &self.wave;
        (0..=steps)
            .map(|i| {
                let deg = 360.0 * i as f64 / steps as f64;
                let r = w.mean.value + w.size.value * (deg - w.peak_deg.value).to_radians().cos();
                (deg, r)
            })
            .collect()
    }
}

/// Where the heavy spot `h` hangs lowest: the hour at the top, and its
/// bearing in degrees with its ±, as [`PoiseFit::heavy_spot_line`] and
/// [`PoiseFit::heavy_spot_row`] both write them.
fn spot_at(h: &Wave) -> (&'static str, String) {
    (
        hour_code(h.peak_deg.value),
        figure_with_pm(h.peak_deg.value, h.peak_deg.se, 0, false),
    )
}

fn caveat_text(c: &Caveat) -> String {
    match c {
        Caveat::AmplitudeWave { size_deg, se_deg } => format!(
            "Amplitude also changes once round, {}° each way, so part of the wave may be \
             isochronism.",
            figure_with_pm(*size_deg, Some(*se_deg), base_digits::AMPLITUDE, false)
        ),
        Caveat::IsochronismRemoved {
            slope,
            amplitude_se,
        } => format!(
            "Corrected for isochronism, {} s/d per degree{}.",
            figure_with_pm(
                slope.s_per_day_per_deg,
                Some(slope.se),
                base_digits::SLOPE,
                true
            ),
            if *amplitude_se {
                ""
            } else {
                "; an amplitude has no ±, so its share of the correction's ± is not carried"
            }
        ),
    }
}

#[cfg(test)]
#[path = "poise_tests.rs"]
mod tests;
