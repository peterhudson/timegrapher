//! Two-state finder: does a series (amplitude or rate) sit at two levels
//! and switch between them, or is it one level with noise and slow wander?
//!
//! The samples are first reduced to block medians (10 s by default), so
//! that one bad window cannot make a state, and a running median over
//! ±5 minutes is taken off, so that the slow fall of amplitude as the
//! mainspring runs down does not read as two levels. The blocks are then
//! fitted with one Gaussian and with a mixture of two of equal width.
//!
//! Two states are called only when all of these hold:
//! - the mixture wins by BIC by at least 10 (Kass and Raftery's "very
//!   strong" evidence);
//! - the two levels are at least two widths apart (Ashman's D ≥ 2, the
//!   usual bar for two separate humps rather than one wide one);
//! - the smaller state holds at least 10% of the time;
//! - the series goes back and forth at least four times, and both
//!   states turn up in each third of the take (at least 5% of its
//!   blocks), so a step or a drift the running median left is not called
//!   switching.
//!
//! Two kinds of two-level series are then told apart from a watch with
//! two states:
//! - **regular**: the low state comes back on a fixed period (the state
//!   sequence correlates with itself at one lag at 0.5 or more), as the
//!   once-a-minute dip of a fourth wheel does. That is a cycle, which the
//!   cycle finder already reports with its period and wheel.
//! - **measurement**: for amplitude, a real change of the balance's swing
//!   moves Tick and Tock together. A split that one side carries alone, or
//!   one that comes with a jump in the beat error from the unlock while
//!   the beat error from the drop stays put, points at the edge finder
//!   hopping between two marks, not at the watch.

use serde::Serialize;

/// One reading of the series: a 2 s amplitude window or a 10 s rate
/// reading.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sample {
    /// Start and end, seconds.
    pub start_s: f64,
    pub end_s: f64,
    pub value: f64,
    /// Amplitude only: the Tick and Tock sides, and the window's beat
    /// error from the unlock and from the drop, ms.
    pub tick: Option<f64>,
    pub tock: Option<f64>,
    pub beat_error_unlock_ms: Option<f64>,
    pub beat_error_drop_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Params {
    pub block_s: f64,
    /// Half-width of the running median taken off before fitting, s.
    pub detrend_s: f64,
    pub min_delta_bic: f64,
    pub min_separation: f64,
    pub min_share: f64,
    pub min_switches: usize,
    /// Autocorrelation of the state sequence that makes it regular.
    pub min_regularity: f64,
    /// Below this many blocks nothing is called.
    pub min_blocks: usize,
    /// A side carrying less than this share of the other side's change is
    /// not moving with it.
    pub min_side_share: f64,
    /// A change of beat error from the unlock between the states, ms, at
    /// least three times the change from the drop, flags the unlock edge.
    pub unlock_jump_ms: f64,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            block_s: 10.0,
            detrend_s: 300.0,
            min_delta_bic: 10.0,
            min_separation: 2.0,
            min_share: 0.1,
            min_switches: 4,
            min_regularity: 0.5,
            min_blocks: 30,
            min_side_share: 0.4,
            unlock_jump_ms: 0.1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// One level: noise and wander only.
    OneLevel,
    /// Two levels the watch switches between.
    TwoStates,
    /// Two levels, but the low one comes back on a fixed period: a cycle.
    Regular,
    /// Two levels that look like the measurement, not the watch.
    Measurement,
    /// Too little data to say.
    TooShort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// What the finder saw, whatever the verdict.
#[derive(Debug, Clone, Serialize)]
pub struct TwoState {
    pub verdict: Verdict,
    pub confidence: Confidence,
    /// Blocks fitted, and their length, s.
    pub blocks: usize,
    pub block_s: f64,
    /// Median of the blocks in the low and the high state (the series'
    /// own units, before the running median is taken off).
    pub low: f64,
    pub high: f64,
    /// Share of the blocks in the low state.
    pub low_share: f64,
    /// Distance between the fitted levels in fitted widths (Ashman's D).
    pub separation: f64,
    /// BIC of one level minus BIC of two: positive favours two.
    pub delta_bic: f64,
    pub switches: usize,
    /// Mean stay in each state, s.
    pub dwell_low_s: f64,
    pub dwell_high_s: f64,
    /// Mean time between switches, s.
    pub switch_every_s: f64,
    /// Both states turn up in every third of the take.
    pub spread: bool,
    /// The lag at which the state sequence best repeats, and how well.
    pub period_s: Option<f64>,
    pub regularity: f64,
    /// Amplitude only: high minus low on the Tick and the Tock side.
    pub tick_change: Option<f64>,
    pub tock_change: Option<f64>,
    /// Amplitude only: median beat error in the low and the high state,
    /// from the unlock and from the drop, ms (signed). The windows do not
    /// carry the unlock time itself; a jump here is its trace.
    pub beat_error_unlock_ms: Option<(f64, f64)>,
    pub beat_error_drop_ms: Option<(f64, f64)>,
    /// Amplitude only: whether the Tick (or the Tock) 2 s windows fall in
    /// two clusters that the other side's windows do not follow. The
    /// unlock edge hopping between the onset and the shoulder after it
    /// splits one side's windows alone; a change of the balance's swing
    /// moves both.
    pub tick_split_alone: Option<bool>,
    pub tock_split_alone: Option<bool>,
    /// One side carries the split alone.
    pub one_sided: bool,
    /// The beat error from the unlock jumps between the states while the
    /// one from the drop does not.
    pub unlock_jump: bool,
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn median_of(v: impl Iterator<Item = f64>) -> Option<f64> {
    let mut w: Vec<f64> = v.filter(|x| x.is_finite()).collect();
    (!w.is_empty()).then(|| median(&mut w))
}

struct Block {
    value: f64,
    tick: Option<f64>,
    tock: Option<f64>,
    unlock: Option<f64>,
    drop: Option<f64>,
}

fn blocks(samples: &[Sample], p: &Params) -> Vec<Block> {
    let mut vals: Vec<f64> = samples.iter().map(|s| s.value).collect();
    if vals.is_empty() {
        return Vec::new();
    }
    let med = median(&mut vals);
    let mut dev: Vec<f64> = samples.iter().map(|s| (s.value - med).abs()).collect();
    let mad = 1.4826 * median(&mut dev);
    let keep =
        |s: &&Sample| s.value.is_finite() && (mad == 0.0 || (s.value - med).abs() < 5.0 * mad);
    let kept: Vec<&Sample> = samples.iter().filter(keep).collect();
    let len = samples
        .iter()
        .map(|s| s.end_s - s.start_s)
        .fold(f64::NAN, f64::min)
        .max(1e-3);
    let need = ((0.6 * p.block_s / len).floor() as usize).max(1);
    let mut out = Vec::new();
    let mut i = 0;
    while i < kept.len() {
        let k = (kept[i].start_s / p.block_s).floor();
        let mut j = i;
        while j < kept.len() && (kept[j].start_s / p.block_s).floor() == k {
            j += 1;
        }
        let b = &kept[i..j];
        if b.len() >= need {
            out.push(Block {
                value: median_of(b.iter().map(|s| s.value)).unwrap(),
                tick: median_of(b.iter().filter_map(|s| s.tick)),
                tock: median_of(b.iter().filter_map(|s| s.tock)),
                unlock: median_of(b.iter().filter_map(|s| s.beat_error_unlock_ms)),
                drop: median_of(b.iter().filter_map(|s| s.beat_error_drop_ms)),
            });
        }
        i = j;
    }
    out
}

/// Fit of two Gaussians of one width: means, width, weights, log
/// likelihood and each point's probability of the second.
fn mixture(y: &[f64]) -> ([f64; 2], f64, f64, Vec<f64>) {
    let n = y.len() as f64;
    let mut s: Vec<f64> = y.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let q = |f: f64| s[((s.len() - 1) as f64 * f).round() as usize];
    let mut mu = [q(0.2), q(0.8)];
    let mean = y.iter().sum::<f64>() / n;
    let mut sd = ((y.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt() / 2.0).max(1e-6);
    let mut w = [0.5, 0.5];
    let mut r = vec![0.5; y.len()];
    let mut ll = 0.0;
    for _ in 0..300 {
        ll = 0.0;
        for (i, &v) in y.iter().enumerate() {
            let a = w[0] * (-0.5 * ((v - mu[0]) / sd).powi(2)).exp();
            let b = w[1] * (-0.5 * ((v - mu[1]) / sd).powi(2)).exp();
            let t = (a + b).max(1e-300);
            ll += (t / (sd * (2.0 * std::f64::consts::PI).sqrt())).ln();
            r[i] = b / t;
        }
        let r1: f64 = r.iter().sum();
        let r0 = n - r1;
        if r0 < 1e-9 || r1 < 1e-9 {
            break;
        }
        w = [r0 / n, r1 / n];
        mu = [
            y.iter().zip(&r).map(|(v, p)| v * (1.0 - p)).sum::<f64>() / r0,
            y.iter().zip(&r).map(|(v, p)| v * p).sum::<f64>() / r1,
        ];
        let var = y
            .iter()
            .zip(&r)
            .map(|(v, p)| (1.0 - p) * (v - mu[0]).powi(2) + p * (v - mu[1]).powi(2))
            .sum::<f64>()
            / n;
        sd = var.sqrt().max(1e-6);
    }
    (mu, sd, ll, r)
}

/// Whether one side's 2 s windows fall in two clusters (outliers
/// dropped; the same BIC, separation and share bars as the blocks) while
/// the other side does not follow: sorted by the first side's cluster,
/// the other side's windows move less than `min_side_share` of the
/// first side's change. `None` when there are too few windows.
fn split_alone(pairs: &[(f64, f64)], p: &Params) -> Option<bool> {
    if pairs.len() < 3 * p.min_blocks {
        return None;
    }
    let mut x: Vec<f64> = pairs.iter().map(|q| q.0).collect();
    let med = median(&mut x.clone());
    let mad = 1.4826 * median(&mut x.iter().map(|v| (v - med).abs()).collect::<Vec<_>>());
    let kept: Vec<(f64, f64)> = pairs
        .iter()
        .copied()
        .filter(|q| mad == 0.0 || (q.0 - med).abs() < 5.0 * mad)
        .collect();
    x = kept.iter().map(|q| q.0).collect();
    let n = x.len() as f64;
    let mean = x.iter().sum::<f64>() / n;
    let var1 = (x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).max(1e-12);
    let ll1 = -0.5 * n * ((2.0 * std::f64::consts::PI * var1).ln() + 1.0);
    let (mu, sd, ll2, r) = mixture(&x);
    let share = r.iter().sum::<f64>() / n;
    let split = 2.0 * (ll2 - ll1) - 2.0 * n.ln() >= p.min_delta_bic
        && (mu[1] - mu[0]).abs() / sd >= p.min_separation
        && share.min(1.0 - share) >= p.min_share;
    if !split {
        return Some(false);
    }
    let other = |second: bool| {
        median_of(
            kept.iter()
                .zip(&r)
                .filter(|(_, &q)| (q > 0.5) == second)
                .map(|(v, _)| v.1),
        )
    };
    let follows = match (other(false), other(true)) {
        (Some(o0), Some(o1)) => (o1 - o0) / (mu[1] - mu[0]),
        _ => 0.0,
    };
    Some(follows < p.min_side_share)
}

/// Look for two states in `samples`, in time order.
pub fn find(samples: &[Sample], p: &Params) -> TwoState {
    let b = blocks(samples, p);
    let n = b.len();
    let mut out = TwoState {
        verdict: Verdict::TooShort,
        confidence: Confidence::Low,
        blocks: n,
        block_s: p.block_s,
        low: f64::NAN,
        high: f64::NAN,
        low_share: 0.0,
        separation: 0.0,
        delta_bic: 0.0,
        switches: 0,
        dwell_low_s: 0.0,
        dwell_high_s: 0.0,
        switch_every_s: 0.0,
        spread: false,
        period_s: None,
        regularity: 0.0,
        tick_change: None,
        tock_change: None,
        beat_error_unlock_ms: None,
        beat_error_drop_ms: None,
        tick_split_alone: None,
        tock_split_alone: None,
        one_sided: false,
        unlock_jump: false,
    };
    let x: Vec<f64> = b.iter().map(|b| b.value).collect();
    if n < p.min_blocks {
        out.low = median_of(x.iter().copied()).unwrap_or(f64::NAN);
        out.high = out.low;
        return out;
    }
    // Take the slow wander off, keeping the series' own median.
    let h = (p.detrend_s / p.block_s).round() as usize;
    let mid = median(&mut x.clone());
    let y: Vec<f64> = (0..n)
        .map(|i| {
            let mut w = x[i.saturating_sub(h)..(i + h + 1).min(n)].to_vec();
            x[i] - median(&mut w) + mid
        })
        .collect();
    let mean = y.iter().sum::<f64>() / n as f64;
    let var1 = (y.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64).max(1e-12);
    let ll1 = -0.5 * n as f64 * ((2.0 * std::f64::consts::PI * var1).ln() + 1.0);
    let (mu, sd, ll2, r) = mixture(&y);
    // Two more parameters: a second mean and a weight.
    out.delta_bic = 2.0 * (ll2 - ll1) - 2.0 * (n as f64).ln();
    out.separation = (mu[1] - mu[0]).abs() / sd;
    let hi = usize::from(mu[1] > mu[0]);
    let high: Vec<bool> = r
        .iter()
        .map(|&q| if hi == 1 { q > 0.5 } else { q < 0.5 })
        .collect();
    let n_high = high.iter().filter(|&&s| s).count();
    out.low_share = 1.0 - n_high as f64 / n as f64;
    out.switches = high.windows(2).filter(|w| w[0] != w[1]).count();
    out.switch_every_s = n as f64 * p.block_s / out.switches.max(1) as f64;
    out.spread = (0..3).all(|k| {
        let part = &high[k * n / 3..(k + 1) * n / 3];
        let h = part.iter().filter(|&&s| s).count() as f64 / part.len() as f64;
        h.min(1.0 - h) >= 0.05
    });
    let mut runs: (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    let mut len = 1;
    for i in 1..=n {
        if i == n || high[i] != high[i - 1] {
            if high[i - 1] {
                runs.1.push(len);
            } else {
                runs.0.push(len);
            }
            len = 1;
        } else {
            len += 1;
        }
    }
    let mean_run = |v: &[usize]| {
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<usize>() as f64 / v.len() as f64 * p.block_s
        }
    };
    out.dwell_low_s = mean_run(&runs.0);
    out.dwell_high_s = mean_run(&runs.1);
    let pick = |want: bool, f: &dyn Fn(&Block) -> Option<f64>| {
        median_of(
            b.iter()
                .zip(&high)
                .filter(|(_, &s)| s == want)
                .filter_map(|(b, _)| f(b)),
        )
    };
    let pair = |f: &dyn Fn(&Block) -> Option<f64>| pick(false, f).zip(pick(true, f));
    if let Some((l, h)) = pair(&|b| Some(b.value)) {
        out.low = l;
        out.high = h;
    } else {
        out.low = mid;
        out.high = mid;
    }
    out.tick_change = pair(&|b| b.tick).map(|(l, h)| h - l);
    out.tock_change = pair(&|b| b.tock).map(|(l, h)| h - l);
    out.beat_error_unlock_ms = pair(&|b| b.unlock);
    out.beat_error_drop_ms = pair(&|b| b.drop);

    // Regularity: the first clear peak of the state sequence's
    // autocorrelation, from two blocks up to a third of the take.
    let m = 1.0 - out.low_share;
    let z: Vec<f64> = high.iter().map(|&s| f64::from(u8::from(s)) - m).collect();
    let z0: f64 = z.iter().map(|v| v * v).sum();
    if z0 > 0.0 {
        let ac: Vec<f64> = (0..=(n / 3).min(60))
            .map(|l| z[l..].iter().zip(&z).map(|(a, b)| a * b).sum::<f64>() / z0)
            .collect();
        let peak = (2..ac.len().saturating_sub(1))
            .find(|&l| ac[l] >= 0.25 && ac[l] >= ac[l - 1] && ac[l] >= ac[l + 1]);
        if let Some(l) = peak {
            out.period_s = Some(l as f64 * p.block_s);
            out.regularity = ac[l];
        }
    }

    let sides: Vec<(f64, f64)> = samples
        .iter()
        .filter_map(|s| s.tick.zip(s.tock))
        .filter(|q| q.0.is_finite() && q.1.is_finite())
        .collect();
    let swapped: Vec<(f64, f64)> = sides.iter().map(|q| (q.1, q.0)).collect();
    out.tick_split_alone = split_alone(&sides, p);
    out.tock_split_alone = split_alone(&swapped, p);
    if let (Some(t), Some(k)) = (out.tick_change, out.tock_change) {
        let (a, b) = (t.abs().min(k.abs()), t.abs().max(k.abs()));
        out.one_sided = t * k < 0.0
            || a < p.min_side_share * b
            || out.tick_split_alone == Some(true)
            || out.tock_split_alone == Some(true);
    }
    if let (Some(u), Some(d)) = (out.beat_error_unlock_ms, out.beat_error_drop_ms) {
        let (du, dd) = ((u.1 - u.0).abs(), (d.1 - d.0).abs());
        out.unlock_jump = du >= p.unlock_jump_ms && du > 3.0 * dd;
    }

    let two = out.delta_bic >= p.min_delta_bic
        && out.separation >= p.min_separation
        && out.low_share.min(1.0 - out.low_share) >= p.min_share
        && out.switches >= p.min_switches
        && out.spread;
    out.verdict = if !two {
        Verdict::OneLevel
    } else if out.one_sided || out.unlock_jump {
        Verdict::Measurement
    } else if out.regularity >= p.min_regularity {
        Verdict::Regular
    } else {
        Verdict::TwoStates
    };
    out.confidence = match out.verdict {
        Verdict::OneLevel if out.delta_bic < 0.0 || out.separation < 1.5 => Confidence::High,
        Verdict::OneLevel => Confidence::Medium,
        Verdict::TooShort => Confidence::Low,
        _ if out.separation >= 3.0 && out.delta_bic >= 30.0 && out.switches >= 10 => {
            Confidence::High
        }
        _ => Confidence::Medium,
    };
    out
}

/// The side whose windows split in two while the other's do not follow.
fn lone_split(r: &TwoState) -> Option<&'static str> {
    if r.tick_split_alone == Some(true) {
        Some("Tick")
    } else if r.tock_split_alone == Some(true) {
        Some("Tock")
    } else {
        None
    }
}

/// One plain phrase for a report, e.g. "two amplitude states, 293° and
/// 312°, switching about every 40 s". `series` is "amplitude" or
/// "rate".
pub fn describe(r: &TwoState, series: &str) -> String {
    let v = |x: f64| {
        if series == "rate" {
            format!("{x:+.1}")
        } else {
            format!("{x:.0}°")
        }
    };
    let unit = if series == "rate" { " sec/day" } else { "" };
    let every = |s: f64| {
        if s >= 120.0 {
            format!("{:.0} minutes", s / 60.0)
        } else {
            format!("{:.0} s", (s / 5.0).round() * 5.0)
        }
    };
    match r.verdict {
        Verdict::TooShort => format!("too short to look for {series} states"),
        Verdict::OneLevel => match lone_split(r) {
            Some(side) => format!(
                "one {series} level, though the {side} windows fall in two clusters (likely the unlock mark hopping, not the watch)"
            ),
            None => format!("one {series} level"),
        },
        Verdict::TwoStates => format!(
            "two {series} states, {} and {}{unit}, switching about every {} ({} confidence)",
            v(r.low),
            v(r.high),
            every(r.switch_every_s),
            match r.confidence {
                Confidence::High => "high",
                Confidence::Medium => "medium",
                Confidence::Low => "low",
            }
        ),
        Verdict::Regular => format!(
            "{series} drops from {} to {}{unit} about every {}, a regular cycle rather than two states",
            v(r.high),
            v(r.low),
            every(r.period_s.unwrap_or(r.switch_every_s))
        ),
        Verdict::Measurement => {
            let why = if r.one_sided {
                let tick = r.tick_change.unwrap_or(0.0).abs();
                let tock = r.tock_change.unwrap_or(0.0).abs();
                let side = lone_split(r).unwrap_or(if tick > tock { "Tick" } else { "Tock" });
                format!("{side} carries the change alone")
            } else {
                "the beat error from the unlock jumps with it".to_string()
            };
            format!(
                "two {series} levels, {} and {}{unit}, but {why}: likely the measurement, not the watch",
                v(r.low),
                v(r.high)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny deterministic noise source.
    fn noise(seed: &mut u64) -> f64 {
        let mut s = 0.0;
        for _ in 0..12 {
            *seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            s += (*seed >> 11) as f64 / (1u64 << 53) as f64;
        }
        s - 6.0
    }

    fn series(n: usize, f: impl Fn(usize, f64) -> f64) -> Vec<Sample> {
        let mut seed = 7;
        (0..n)
            .map(|i| {
                let t = i as f64 * 2.0;
                let v = f(i, t) + 3.0 * noise(&mut seed);
                Sample {
                    start_s: t,
                    end_s: t + 2.0,
                    value: v,
                    tick: Some(v + 1.0),
                    tock: Some(v - 1.0),
                    beat_error_unlock_ms: Some(0.2),
                    beat_error_drop_ms: Some(0.2),
                }
            })
            .collect()
    }

    #[test]
    fn steady_with_a_slow_fall_is_one_level() {
        // 30 minutes falling 15 degrees as the mainspring runs down.
        let s = series(900, |_, t| 300.0 - 15.0 * t / 1800.0);
        let r = find(&s, &Params::default());
        assert_eq!(r.verdict, Verdict::OneLevel, "{r:?}");
    }

    #[test]
    fn irregular_switching_is_two_states() {
        // Dwell times that do not repeat: 40 to 130 s.
        let dwell = [60.0, 90.0, 40.0, 130.0, 70.0, 50.0, 110.0, 80.0];
        let edges: Vec<f64> = dwell
            .iter()
            .cycle()
            .scan(0.0, |t, d| {
                *t += d;
                Some(*t)
            })
            .take(40)
            .collect();
        let s = series(900, |_, t| {
            let k = edges.iter().filter(|&&e| e <= t).count();
            if k % 2 == 0 {
                312.0
            } else {
                293.0
            }
        });
        let r = find(&s, &Params::default());
        assert_eq!(r.verdict, Verdict::TwoStates, "{r:?}");
        assert!((r.low - 293.0).abs() < 2.0 && (r.high - 312.0).abs() < 2.0);
        assert!(r.switches >= 20);
    }

    #[test]
    fn a_dip_once_a_minute_is_regular() {
        let s = series(900, |_, t| if t % 60.0 < 20.0 { 232.0 } else { 245.0 });
        let r = find(&s, &Params::default());
        assert_eq!(r.verdict, Verdict::Regular, "{r:?}");
        assert_eq!(r.period_s, Some(60.0));
    }

    #[test]
    fn a_split_on_one_side_only_is_the_measurement() {
        let dwell = [60.0, 90.0, 40.0, 130.0, 70.0, 50.0, 110.0, 80.0];
        let edges: Vec<f64> = dwell
            .iter()
            .cycle()
            .scan(0.0, |t, d| {
                *t += d;
                Some(*t)
            })
            .take(40)
            .collect();
        let mut s = series(900, |_, t| {
            let k = edges.iter().filter(|&&e| e <= t).count();
            if k % 2 == 0 {
                300.0
            } else {
                280.0
            }
        });
        // Tick stays put; Tock carries the whole 40 degree split.
        for x in &mut s {
            x.tick = Some(300.0);
            x.tock = Some(2.0 * x.value - 300.0);
        }
        let r = find(&s, &Params::default());
        assert!(r.one_sided);
        assert_eq!(r.verdict, Verdict::Measurement, "{r:?}");
    }

    #[test]
    fn a_single_step_is_not_switching() {
        let s = series(900, |_, t| if t < 1200.0 { 300.0 } else { 280.0 });
        let r = find(&s, &Params::default());
        assert_ne!(r.verdict, Verdict::TwoStates, "{r:?}");
    }

    #[test]
    fn a_hopping_unlock_edge_on_one_side_is_flagged() {
        // Steady at 305; one Tick window in five reads 320 as the edge
        // lands on the shoulder; the Tock does not follow.
        let mut s = series(900, |_, _| 305.0);
        for (i, x) in s.iter_mut().enumerate() {
            let tock = x.value - 1.0;
            let tick = if i % 5 == 2 {
                x.value + 16.0
            } else {
                x.value + 1.0
            };
            x.tick = Some(tick);
            x.tock = Some(tock);
        }
        let r = find(&s, &Params::default());
        assert_eq!(r.tick_split_alone, Some(true), "{r:?}");
        assert_eq!(r.tock_split_alone, Some(false));
        assert_ne!(r.verdict, Verdict::TwoStates);
        assert!(describe(&r, "amplitude").contains("Tick windows"));
    }

    #[test]
    fn both_sides_moving_is_not_flagged() {
        let s = series(
            900,
            |_, t| if (t / 50.0).sin() > 0.6 { 290.0 } else { 305.0 },
        );
        let r = find(&s, &Params::default());
        assert_eq!(r.tick_split_alone, Some(false), "{r:?}");
        assert_eq!(r.tock_split_alone, Some(false));
    }

    #[test]
    fn short_takes_are_not_judged() {
        let s = series(20, |_, _| 300.0);
        assert_eq!(find(&s, &Params::default()).verdict, Verdict::TooShort);
    }
}
