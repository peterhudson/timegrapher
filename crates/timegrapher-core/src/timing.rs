//! Rate and beat error from beat times.
//!
//! Beat `k` is modelled as `t = t0 + k*P + s*e/2`, where `P` is the beat
//! period, `s` is +1 for even beats and -1 for odd ones, and `e` is the
//! beat error. Fitting all three at once gives the rate from tick and toc
//! together while cancelling beat error, as Witschi recommends.

use crate::beats::Beat;
use crate::dsp::robust_sd;
use serde::Serialize;

/// Where a paper strip is anchored: a beat number and the time it is
/// drawn at the centre line. The app's strip and `record`'s live strip
/// both use it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub index: i64,
    pub time: f64,
}

impl Anchor {
    /// Centre the strip on the last `n` beats (tick and tock together).
    pub fn centre_on(beats: &[Beat], period_s: f64, n: usize) -> Option<Anchor> {
        let last = beats.last()?;
        let recent = &beats[beats.len().saturating_sub(n)..];
        let t = recent
            .iter()
            .map(|b| b.time - (b.index - last.index) as f64 * period_s)
            .sum::<f64>()
            / recent.len() as f64;
        Some(Anchor {
            index: last.index,
            time: t,
        })
    }

    /// How early the beat came, in ms, against this anchor and a beat
    /// period of `period_s`. Positive means early (the watch gains).
    pub fn lead_ms(&self, b: &Beat, period_s: f64) -> f64 {
        let expected = self.time + (b.index - self.index) as f64 * period_s;
        (expected - b.time) * 1000.0
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TimingFit {
    /// Daily rate in seconds per day; positive means the watch gains.
    pub rate_s_per_day: f64,
    /// Signed beat error in milliseconds (even beats late is positive).
    pub beat_error_ms: f64,
    /// Fitted beat period, seconds.
    pub period_s: f64,
    /// Robust standard deviation of the residuals, microseconds.
    pub jitter_us: f64,
    pub beats_used: usize,
}

fn side(index: i64) -> f64 {
    if index.rem_euclid(2) == 0 {
        0.5
    } else {
        -0.5
    }
}

/// Least-squares fit of `t = a + b*k + c*s` over the selected beats.
fn solve(beats: &[&Beat]) -> Option<[f64; 3]> {
    if beats.len() < 6 {
        return None;
    }
    // Centre k and t for numerical stability.
    let k0 = beats[0].index as f64;
    let t0 = beats[0].time;
    let mut ata = [[0.0f64; 3]; 3];
    let mut atb = [0.0f64; 3];
    for b in beats {
        let row = [1.0, b.index as f64 - k0, side(b.index)];
        let y = b.time - t0;
        for i in 0..3 {
            atb[i] += row[i] * y;
            for j in 0..3 {
                ata[i][j] += row[i] * row[j];
            }
        }
    }
    let x = solve3(ata, atb)?;
    Some([x[0] + t0 - x[1] * k0, x[1], x[2]])
}

#[allow(clippy::needless_range_loop)]
fn solve3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    for c in 0..3 {
        let p = (c..3).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[p][c].abs() < 1e-300 {
            return None;
        }
        a.swap(c, p);
        b.swap(c, p);
        for r in 0..3 {
            if r != c {
                let f = a[r][c] / a[c][c];
                for k in 0..3 {
                    a[r][k] -= f * a[c][k];
                }
                b[r] -= f * b[c];
            }
        }
    }
    Some([b[0] / a[0][0], b[1] / a[1][1], b[2] / a[2][2]])
}

/// Robust fit: beats with low template correlation are left out, then
/// beats more than 5 robust SDs from the fit, and the fit is repeated.
fn fit_coef(beats: &[Beat]) -> Option<([f64; 3], Vec<&Beat>)> {
    let mut sel: Vec<&Beat> = beats.iter().filter(|b| b.is_good()).collect();
    let mut coef = solve(&sel)?;
    for _ in 0..2 {
        let res: Vec<f64> = sel.iter().map(|b| residual(b, &coef)).collect();
        let sd = robust_sd(&res).max(1e-6);
        sel = sel
            .iter()
            .zip(&res)
            .filter(|(_, r)| r.abs() < 5.0 * sd)
            .map(|(b, _)| *b)
            .collect();
        coef = solve(&sel)?;
    }
    Some((coef, sel))
}

/// Fit rate and beat error to a run of beats.
pub fn fit(beats: &[Beat], bph: u32) -> Option<TimingFit> {
    let nominal = 3600.0 / bph as f64;
    let (coef, sel) = fit_coef(beats)?;
    let res: Vec<f64> = sel.iter().map(|b| residual(b, &coef)).collect();
    Some(TimingFit {
        rate_s_per_day: (nominal / coef[1] - 1.0) * 86400.0,
        beat_error_ms: coef[2] * 1000.0,
        period_s: coef[1],
        jitter_us: robust_sd(&res) * 1e6,
        beats_used: sel.len(),
    })
}

/// Signed beat error alone, ms, for runs too short to need the rate.
pub fn beat_error_ms(beats: &[Beat]) -> Option<f64> {
    fit_coef(beats).map(|(c, _)| c[2] * 1000.0)
}

fn residual(b: &Beat, c: &[f64; 3]) -> f64 {
    b.time - (c[0] + c[1] * b.index as f64 + c[2] * side(b.index))
}

/// Residual of every beat against one fit for the whole recording:
/// how far each beat is from where a constant rate would put it, seconds.
pub fn residuals(beats: &[Beat]) -> Option<Vec<f64>> {
    let (coef, _) = fit_coef(beats)?;
    Some(beats.iter().map(|b| residual(b, &coef)).collect())
}

/// The stretches of `beats` (sorted by time) between `cuts`, times on the
/// beats' own axis.
fn pieces<'a>(beats: &'a [Beat], cuts: &[f64]) -> Vec<&'a [Beat]> {
    let mut out = Vec::new();
    let mut lo = 0;
    for &c in cuts {
        let hi = lo + beats[lo..].partition_point(|b| b.time < c);
        out.push(&beats[lo..hi]);
        lo = hi;
    }
    out.push(&beats[lo..]);
    out
}

/// The shortest stretch between cuts that [`fit_pieces`] and
/// [`residuals_pieces`] read, seconds: the length of the 10 s bins the
/// readings rest on. A shorter stretch is the ragged edge of a mask or a
/// gap and is left out rather than fitted, since a few seconds of beats
/// give a rate several s/d out.
pub const MIN_PIECE_S: f64 = 10.0;

/// Whether a stretch spans [`MIN_PIECE_S`] from its first beat to its last,
/// so a stretch exactly 10 s long between cuts, whose beats span a beat
/// less, is left out too.
fn whole(p: &[Beat]) -> bool {
    match (p.first(), p.last()) {
        (Some(a), Some(b)) => b.time - a.time >= MIN_PIECE_S - 1e-9,
        _ => false,
    }
}

/// [`fit`] for a take with breaks: each stretch between `cuts` is fitted on
/// its own, since the phase (and after a long gap the rate) does not carry
/// across a break, and the fits are combined weighted by the beats each
/// used, so the rate is the average over the time the watch was heard.
/// The beat error is combined as [`signed_across`] says. Stretches
/// shorter than [`MIN_PIECE_S`] are left out. With no cuts this is
/// [`fit`].
pub fn fit_pieces(beats: &[Beat], bph: u32, cuts: &[f64]) -> Option<TimingFit> {
    if cuts.is_empty() {
        return fit(beats, bph);
    }
    let fits: Vec<TimingFit> = pieces(beats, cuts)
        .into_iter()
        .filter(|p| whole(p))
        .filter_map(|p| fit(p, bph))
        .collect();
    let n: usize = fits.iter().map(|f| f.beats_used).sum();
    if n == 0 {
        return None;
    }
    let mean = |g: &dyn Fn(&TimingFit) -> f64| {
        fits.iter().map(|f| g(f) * f.beats_used as f64).sum::<f64>() / n as f64
    };
    let rate = mean(&|f| f.rate_s_per_day);
    Some(TimingFit {
        rate_s_per_day: rate,
        beat_error_ms: signed_across(
            &fits
                .iter()
                .map(|f| (f.beat_error_ms, f.beats_used as f64))
                .collect::<Vec<_>>(),
        )?,
        period_s: 3600.0 / bph as f64 / (1.0 + rate / 86400.0),
        jitter_us: mean(&|f| f.jitter_us),
        beats_used: n,
    })
}

/// A signed figure (a beat error) over stretches, each `(value, weight)`:
/// their weighted mean when the stretches agree in sign, else the weighted
/// mean of their sizes, given as positive, so a sign that turns over across
/// a break does not cancel the size the watch has either side.
pub fn signed_across(parts: &[(f64, f64)]) -> Option<f64> {
    let w: f64 = parts.iter().map(|p| p.1).sum();
    if w.is_nan() || w <= 0.0 {
        return None;
    }
    let mixed = parts.iter().any(|p| p.0 > 0.0) && parts.iter().any(|p| p.0 < 0.0);
    let v = |x: f64| if mixed { x.abs() } else { x };
    Some(parts.iter().map(|p| v(p.0) * p.1).sum::<f64>() / w)
}

/// Whether the beat error turns over across `cuts`: the whole stretches
/// that [`fit_pieces`] fits differ in the sign of their beat error, so it
/// combines them as sizes. Other beat-error figures over the same beats
/// follow this one decision ([`median_signed`]), so they keep one sign.
pub fn sign_turns(beats: &[Beat], bph: u32, cuts: &[f64]) -> bool {
    let be: Vec<f64> = pieces(beats, cuts)
        .into_iter()
        .filter(|p| whole(p))
        .filter_map(|p| fit(p, bph))
        .map(|f| f.beat_error_ms)
        .collect();
    be.iter().any(|&v| v > 0.0) && be.iter().any(|&v| v < 0.0)
}

/// The median of signed figures (a beat error's 2 s windows): of their
/// sizes, given as positive, where the sign turns over ([`sign_turns`]),
/// else of the values.
pub fn median_signed(values: impl Iterator<Item = f64>, turned: bool) -> Option<f64> {
    let mut v: Vec<f64> = values
        .filter(|x| x.is_finite())
        .map(|x| if turned { x.abs() } else { x })
        .collect();
    (!v.is_empty()).then(|| crate::dsp::median(&mut v))
}

/// [`residuals`] for a take with breaks: each stretch against its own fit,
/// and NaN for a stretch shorter than [`MIN_PIECE_S`]. With no cuts this
/// is [`residuals`].
pub fn residuals_pieces(beats: &[Beat], cuts: &[f64]) -> Option<Vec<f64>> {
    if cuts.is_empty() {
        return residuals(beats);
    }
    let mut out = Vec::with_capacity(beats.len());
    for p in pieces(beats, cuts) {
        match whole(p).then(|| residuals(p)).flatten() {
            Some(r) => out.extend(r),
            None => out.extend(vec![f64::NAN; p.len()]),
        }
    }
    out.iter().any(|r| r.is_finite()).then_some(out)
}

/// Rate, beat error and jitter in consecutive windows of `window_s`.
#[derive(Debug, Clone, Serialize)]
pub struct WindowFit {
    pub start_s: f64,
    pub end_s: f64,
    pub fit: TimingFit,
}

pub fn windows(beats: &[Beat], bph: u32, window_s: f64, step_s: f64) -> Vec<WindowFit> {
    let mut out = Vec::new();
    let Some(last) = beats.last() else { return out };
    let mut start = beats[0].time;
    let mut lo = 0usize;
    while start + window_s <= last.time + 1e-9 {
        while lo < beats.len() && beats[lo].time < start {
            lo += 1;
        }
        let mut hi = lo;
        while hi < beats.len() && beats[hi].time < start + window_s {
            hi += 1;
        }
        if let Some(fit) = fit(&beats[lo..hi], bph) {
            out.push(WindowFit {
                start_s: start,
                end_s: start + window_s,
                fit,
            });
        }
        start += step_s;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_that_turn_over_combine_as_sizes() {
        assert_eq!(signed_across(&[(-0.6, 1.0), (0.6, 1.0)]), Some(0.6));
        assert!((signed_across(&[(-0.6, 3.0), (-0.2, 1.0)]).unwrap() + 0.5).abs() < 1e-12);
        assert_eq!(signed_across(&[(0.4, 1.0), (0.0, 1.0)]), Some(0.2));
        assert_eq!(signed_across(&[]), None);
        assert_eq!(
            median_signed([-0.6, -0.5, 0.6].into_iter(), true),
            Some(0.6)
        );
        assert_eq!(
            median_signed([-0.6, -0.5, 0.6].into_iter(), false),
            Some(-0.5)
        );
        assert_eq!(median_signed([f64::NAN].into_iter(), false), None);
        let flip = |mut b: Vec<Beat>, at: f64| {
            b.iter_mut()
                .filter(|x| x.time >= at)
                .for_each(|x| x.index += 1);
            b
        };
        let both = flip(take(5.5, &[(0.0, 12.0), (20.0, 32.0)]), 19.0);
        assert!(sign_turns(&both, 28800, &[19.0]));
        assert!(!sign_turns(&both, 28800, &[]));
        let k = both.partition_point(|b| b.time < 19.0);
        assert!(!sign_turns(&both[..k], 28800, &[19.0]));
        assert!(!sign_turns(
            &take(5.5, &[(0.0, 12.0), (20.0, 32.0)]),
            28800,
            &[19.0]
        ));
        let scrap = flip(take(5.5, &[(0.0, 40.0), (50.0, 52.0)]), 45.0);
        assert!(!sign_turns(&scrap, 28800, &[45.0]));
    }

    /// Beats at `rate` s/d with 0.3 ms beat error and a little jitter, on
    /// the stretches given, numbered on one grid.
    fn take(rate: f64, spans: &[(f64, f64)]) -> Vec<Beat> {
        let p = 0.125 / (1.0 + rate / 86400.0);
        let mut seed = 7u64;
        let mut noise = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 2e-4
        };
        (0..(spans.last().unwrap().1 / p) as i64)
            .map(|k| Beat {
                index: k,
                time: k as f64 * p + side(k) * 3e-4 + noise(),
                quality: 1.0,
            })
            .filter(|b| spans.iter().any(|&(a, e)| b.time >= a && b.time < e))
            .collect()
    }

    #[test]
    fn a_ragged_piece_does_not_pull_the_rate() {
        // Masks at 60-90 s, 180-182 s and 183.5-190 s leave a 60 s piece
        // and a 1.5 s piece between them at 182-183.5 s.
        let beats = take(
            5.5,
            &[(0.0, 60.0), (90.0, 180.0), (182.0, 183.5), (190.0, 600.0)],
        );
        let cuts = [60.0, 180.0, 183.5 + 1e-3];
        let mut skewed = beats.clone();
        // The short piece's few beats, slewed as after a knock.
        for b in skewed
            .iter_mut()
            .filter(|b| b.time >= 182.0 && b.time < 183.5)
        {
            b.time += (b.time - 182.0) * 2e-4;
        }
        let f = fit_pieces(&skewed, 28800, &cuts).unwrap();
        assert!(
            (f.rate_s_per_day - 5.5).abs() < 0.005,
            "{}",
            f.rate_s_per_day
        );
        assert!((f.beat_error_ms - 0.3).abs() < 0.01, "{}", f.beat_error_ms);
        let r = residuals_pieces(&skewed, &cuts).unwrap();
        let short = skewed
            .iter()
            .zip(&r)
            .filter(|(b, _)| b.time >= 182.0 && b.time < 183.5);
        assert!(short.clone().count() > 6 && short.clone().all(|(_, r)| r.is_nan()));
        assert!(skewed
            .iter()
            .zip(&r)
            .filter(|(b, _)| b.time > 190.0)
            .all(|(_, r)| r.is_finite()));
    }

    #[test]
    fn a_short_whole_piece_counts_by_its_beats() {
        // A 12 s piece, its time shortened by 2 s/d so it reads faster than
        // the 400 s piece, is kept and counts by its beats.
        let beats = take(5.5, &[(0.0, 400.0), (420.0, 432.0)]);
        let mut b2 = beats.clone();
        for b in b2.iter_mut().filter(|b| b.time >= 420.0) {
            b.time -= (b.time - 420.0) * 2.0 / 86400.0;
        }
        let f = fit_pieces(&b2, 28800, &[410.0]).unwrap();
        let k = b2.partition_point(|b| b.time < 410.0);
        let (a, c) = (fit(&b2[..k], 28800).unwrap(), fit(&b2[k..], 28800).unwrap());
        let n = (a.beats_used + c.beats_used) as f64;
        let mean =
            (a.rate_s_per_day * a.beats_used as f64 + c.rate_s_per_day * c.beats_used as f64) / n;
        assert!(
            (f.rate_s_per_day - mean).abs() < 1e-9,
            "{} {}",
            f.rate_s_per_day,
            mean
        );
        assert!(
            (f.rate_s_per_day - a.rate_s_per_day).abs() > 0.04,
            "{} {}",
            f.rate_s_per_day,
            a.rate_s_per_day
        );
        assert_eq!(
            fit_pieces(&b2[..10], 28800, &[410.0]).map(|f| f.beats_used),
            None
        );
    }
}
