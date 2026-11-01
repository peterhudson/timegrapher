//! Two-state finder: does a series (amplitude or rate) sit at two levels
//! and switch between them, or is it one level with noise and slow wander?
//!
//! The samples are first reduced to block medians (10 s by default, or
//! as long as the samples when they are longer), so that one bad window
//! cannot make a state, and a running median over ±5 minutes is taken
//! off, so that the slow fall of amplitude as the mainspring runs down
//! does not read as two levels. The blocks are then fitted with one
//! Gaussian and with a mixture of two of equal width.
//!
//! Two levels are reported only when all of these hold:
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
//! Beside the levels, the finder records facts a reader weighs:
//! - whether the low state comes back on a fixed period (the state
//!   sequence correlates with itself at one lag at 0.5 or more), as the
//!   once-a-minute dip of a fourth wheel does;
//! - for amplitude, whether one side, Tick or Tock, carries the split
//!   alone, and whether the beat error from the unlock jumps between the
//!   levels while the beat error from the drop stays put.
//!
//! Each block keeps its place in the take, so a block that is missing
//! (masked, in a gap, too thin) is time that passed: switches, stays and
//! lags are not counted across it as if the blocks beside it touched.
//!
//! What the levels mean for the watch is not decided here.

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
    /// Amplitude only: where each side's unlock edge sat, ms from the
    /// beat time.
    pub tick_unlock_ms: Option<f64>,
    pub tock_unlock_ms: Option<f64>,
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

/// What the finder saw.
#[derive(Debug, Clone, Serialize)]
pub struct TwoState {
    /// Enough blocks to fit (at least `min_blocks`); false, nothing below
    /// the block count was looked for.
    #[serde(skip)]
    pub fitted: bool,
    /// The level sequence repeats at one lag (`regularity` at least
    /// `min_regularity`).
    #[serde(skip)]
    pub repeats: bool,
    /// All the two-level tests hold: ΔBIC, separation, the smaller
    /// state's share, the switches and the spread over the take (see the
    /// module notes for the figures).
    pub two_levels: bool,
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
    /// from the unlock and from the drop, ms (signed).
    pub beat_error_unlock_ms: Option<(f64, f64)>,
    pub beat_error_drop_ms: Option<(f64, f64)>,
    /// Amplitude only: median unlock edge of each side in the low and the
    /// high state, ms from the beat time.
    pub tick_unlock_ms: Option<(f64, f64)>,
    pub tock_unlock_ms: Option<(f64, f64)>,
    /// Amplitude only: whether one side's unlock edges fall in two
    /// clusters that the other side's edges do not follow, the direct
    /// sign of the edge finder hopping between the onset and the shoulder
    /// after it. A change of the balance's swing moves both sides' edges.
    pub tick_edge_split_alone: Option<bool>,
    pub tock_edge_split_alone: Option<bool>,
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
    /// Which `block_s` stretch of the take the block is, from its start:
    /// blocks that are missing (masked, in a gap, too thin) leave holes
    /// in these numbers.
    k: i64,
    value: f64,
    tick: Option<f64>,
    tock: Option<f64>,
    unlock: Option<f64>,
    drop: Option<f64>,
    tick_edge: Option<f64>,
    tock_edge: Option<f64>,
}

fn blocks(samples: &[Sample], p: &Params) -> (Vec<Block>, f64) {
    let mut vals: Vec<f64> = samples.iter().map(|s| s.value).collect();
    if vals.is_empty() {
        return (Vec::new(), p.block_s);
    }
    let med = median(&mut vals);
    let mut dev: Vec<f64> = samples.iter().map(|s| (s.value - med).abs()).collect();
    let mad = 1.4826 * median(&mut dev);
    // Windows more than 5 MAD from the median are stray and dropped,
    // unless those on one side hold at least the smaller state's least
    // share of the windows: then they are a level, which can sit that far
    // out when it holds under half the time (the MAD is then the spread
    // of the other level alone), and are kept.
    let far = |s: &Sample, side: f64| {
        mad > 0.0 && (s.value - med).abs() >= 5.0 * mad && (s.value - med) * side > 0.0
    };
    let finite = samples.iter().filter(|s| s.value.is_finite()).count();
    let is_level = |side: f64| {
        let n = samples.iter().filter(|s| far(s, side)).count();
        n > 0 && n as f64 >= p.min_share * finite as f64
    };
    let (keep_low, keep_high) = (is_level(-1.0), is_level(1.0));
    let keep = |s: &&Sample| {
        s.value.is_finite() && (!far(s, -1.0) || keep_low) && (!far(s, 1.0) || keep_high)
    };
    let kept: Vec<&Sample> = samples.iter().filter(keep).collect();
    let len = samples
        .iter()
        .map(|s| s.end_s - s.start_s)
        .fold(f64::NAN, f64::min)
        .max(1e-3);
    // Samples longer than a block (rate readings of more than 10 s, as
    // `series --reading` gives) are a block each: the blocks are as long
    // as the samples, and side by side when the samples are.
    let block_s = if len > p.block_s + 1e-6 {
        len
    } else {
        p.block_s
    };
    let need = ((0.6 * block_s / len).floor() as usize).max(1);
    let mut out = Vec::new();
    let mut i = 0;
    while i < kept.len() {
        let k = (kept[i].start_s / block_s).floor();
        let mut j = i;
        while j < kept.len() && (kept[j].start_s / block_s).floor() == k {
            j += 1;
        }
        let b = &kept[i..j];
        if b.len() >= need {
            out.push(Block {
                k: k as i64,
                value: median_of(b.iter().map(|s| s.value)).unwrap(),
                tick: median_of(b.iter().filter_map(|s| s.tick)),
                tock: median_of(b.iter().filter_map(|s| s.tock)),
                unlock: median_of(b.iter().filter_map(|s| s.beat_error_unlock_ms)),
                drop: median_of(b.iter().filter_map(|s| s.beat_error_drop_ms)),
                tick_edge: median_of(b.iter().filter_map(|s| s.tick_unlock_ms)),
                tock_edge: median_of(b.iter().filter_map(|s| s.tock_unlock_ms)),
            });
        }
        i = j;
    }
    (out, block_s)
}

/// Fit of two Gaussians of one width: means, width, weights, log
/// likelihood and each point's probability of the second.
///
/// The fit is started from several splits of the sorted blocks (the 20th
/// and 80th percentiles, and each tail's 5th percentile against the
/// median), and the one with the highest likelihood is kept: a clean
/// level held 10-15% of the time has no block between the 20th and 80th
/// percentiles, so a fit started there alone can settle with both levels
/// in the other.
fn mixture(y: &[f64]) -> ([f64; 2], f64, f64, Vec<f64>) {
    let mut s: Vec<f64> = y.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let q = |f: f64| s[((s.len() - 1) as f64 * f).round() as usize];
    [(0.2, 0.8), (0.05, 0.5), (0.5, 0.95)]
        .iter()
        .map(|&(a, b)| mixture_from(y, [q(a), q(b)]))
        .reduce(|best, f| if f.2 > best.2 { f } else { best })
        .unwrap()
}

/// [`mixture`] from one start, the two levels at `mu`.
fn mixture_from(y: &[f64], mut mu: [f64; 2]) -> ([f64; 2], f64, f64, Vec<f64>) {
    let n = y.len() as f64;
    let mean = y.iter().sum::<f64>() / n;
    let mut sd = ((y.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt() / 2.0).max(1e-6);
    let mut w = [0.5, 0.5];
    let mut r = vec![0.5; y.len()];
    // Each point's share of the second level. Every round but the last
    // takes it from one exponential, the two levels' ratio, and skips the
    // log likelihood, which only the last round's E step returns; that
    // last round, and a point far from both levels, take both
    // exponentials as before. The two exponentials and the
    // logarithm of every round were 61% of the steady-watch test (#804);
    // on 400 scenario takes the pane's every finding, headline and level
    // is the same, and only `separation` and `delta_bic` move, by under
    // 1e-10 of their size.
    let e_step = |mu: [f64; 2], sd: f64, w: [f64; 2], r: &mut [f64], last: bool| {
        let both = |v: f64| {
            let a = w[0] * (-0.5 * ((v - mu[0]) / sd).powi(2)).exp();
            let b = w[1] * (-0.5 * ((v - mu[1]) / sd).powi(2)).exp();
            let t = (a + b).max(1e-300);
            (b / t, t)
        };
        if !last {
            let k = w[0] / w[1];
            for (ri, &v) in r.iter_mut().zip(y) {
                let (z0, z1) = ((v - mu[0]) / sd, (v - mu[1]) / sd);
                // Near both levels' floor (exp(-300) is 5e-131) the two
                // exponentials are kept, so the floor acts as before.
                *ri = if z0 * z0 < 600.0 || z1 * z1 < 600.0 {
                    1.0 / (1.0 + k * (0.5 * (z1 * z1 - z0 * z0)).exp())
                } else {
                    both(v).0
                };
            }
            return 0.0;
        }
        let mut ll = 0.0;
        for (ri, &v) in r.iter_mut().zip(y) {
            let (share, t) = both(v);
            ll += (t / (sd * (2.0 * std::f64::consts::PI).sqrt())).ln();
            *ri = share;
        }
        ll
    };
    let mut ll = 0.0;
    for round in 0..300 {
        ll = e_step(mu, sd, w, &mut r, round == 299);
        let r1: f64 = r.iter().sum();
        let r0 = n - r1;
        if r0 < 1e-9 || r1 < 1e-9 {
            if round < 299 {
                ll = e_step(mu, sd, w, &mut r, true);
            }
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
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
pub fn find(samples: &[Sample], p: &Params) -> TwoState {
    let (b, block_s) = blocks(samples, p);
    let n = b.len();
    let mut out = TwoState {
        fitted: false,
        repeats: false,
        two_levels: false,
        blocks: n,
        block_s,
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
        tick_unlock_ms: None,
        tock_unlock_ms: None,
        tick_edge_split_alone: None,
        tock_edge_split_alone: None,
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
    // Take the slow wander off, keeping the series' own median. The
    // running median, the switches, the stays and the lags all go by each
    // block's place in the take, so a hole in the blocks is time that
    // passed, not two blocks side by side.
    let k: Vec<i64> = b.iter().map(|b| b.k).collect();
    let h = (p.detrend_s / block_s).round() as i64;
    let mid = median(&mut x.clone());
    let y: Vec<f64> = (0..n)
        .map(|i| {
            let lo = k.partition_point(|&j| j < k[i] - h);
            let hi = k.partition_point(|&j| j <= k[i] + h);
            let mut w = x[lo..hi].to_vec();
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
    // Next to each other in the take: a change of state across a hole is
    // not a timed switch.
    let next = |i: usize| k[i] == k[i - 1] + 1;
    out.switches = (1..n)
        .filter(|&i| next(i) && high[i] != high[i - 1])
        .count();
    out.switch_every_s = n as f64 * block_s / out.switches.max(1) as f64;
    out.spread = (0..3).all(|k| {
        let part = &high[k * n / 3..(k + 1) * n / 3];
        let h = part.iter().filter(|&&s| s).count() as f64 / part.len() as f64;
        h.min(1.0 - h) >= 0.05
    });
    let mut runs: (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    let mut len = 1;
    for i in 1..=n {
        if i == n || high[i] != high[i - 1] || !next(i) {
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
            v.iter().sum::<usize>() as f64 / v.len() as f64 * block_s
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
    out.tick_unlock_ms = pair(&|b| b.tick_edge);
    out.tock_unlock_ms = pair(&|b| b.tock_edge);

    // Regularity: the first clear peak of the state sequence's
    // autocorrelation, from two blocks up to a third of the take. Each lag
    // is a time apart: only blocks that far apart in the take are paired,
    // so a lag that holes leave few pairs for scores low.
    let m = 1.0 - out.low_share;
    let z: Vec<f64> = high.iter().map(|&s| f64::from(u8::from(s)) - m).collect();
    let z0: f64 = z.iter().map(|v| v * v).sum();
    if z0 > 0.0 {
        let span = (k[n - 1] - k[0] + 1) as usize;
        let ac: Vec<f64> = (0..=(span / 3).min(60) as i64)
            .map(|l| {
                (0..n)
                    .filter_map(|i| k.binary_search(&(k[i] + l)).ok().map(|j| z[i] * z[j]))
                    .sum::<f64>()
                    / z0
            })
            .collect();
        let peak = (2..ac.len().saturating_sub(1))
            .find(|&l| ac[l] >= 0.25 && ac[l] >= ac[l - 1] && ac[l] >= ac[l + 1]);
        if let Some(l) = peak {
            out.period_s = Some(l as f64 * block_s);
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
    let edges: Vec<(f64, f64)> = samples
        .iter()
        .filter_map(|s| s.tick_unlock_ms.zip(s.tock_unlock_ms))
        .filter(|q| q.0.is_finite() && q.1.is_finite())
        .collect();
    let swapped: Vec<(f64, f64)> = edges.iter().map(|q| (q.1, q.0)).collect();
    out.tick_edge_split_alone = split_alone(&edges, p);
    out.tock_edge_split_alone = split_alone(&swapped, p);
    if let (Some(t), Some(k)) = (out.tick_change, out.tock_change) {
        let (a, b) = (t.abs().min(k.abs()), t.abs().max(k.abs()));
        out.one_sided = t * k < 0.0
            || a < p.min_side_share * b
            || out.tick_split_alone == Some(true)
            || out.tock_split_alone == Some(true)
            || out.tick_edge_split_alone == Some(true)
            || out.tock_edge_split_alone == Some(true);
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
    out.fitted = true;
    out.two_levels = two;
    out.repeats = out.regularity >= p.min_regularity;
    out
}

impl TwoState {
    /// Two levels that only one side carries, or that the unlock beat
    /// error jumps with while the drop's stays put.
    pub fn one_side_or_unlock(&self) -> bool {
        self.two_levels && (self.one_sided || self.unlock_jump)
    }

    /// Two levels, both sides moving, whose low level comes back on a
    /// fixed period.
    pub fn repeating_levels(&self) -> bool {
        self.two_levels && !self.one_side_or_unlock() && self.repeats
    }

    /// Two levels, both sides moving, switched between with no fixed period.
    pub fn switching_levels(&self) -> bool {
        self.two_levels && !self.one_side_or_unlock() && !self.repeats
    }
}

/// The side whose windows split in two while the other's do not follow.
fn lone_split(r: &TwoState) -> Option<&'static str> {
    if r.tick_edge_split_alone == Some(true) || r.tick_split_alone == Some(true) {
        Some("Tick")
    } else if r.tock_edge_split_alone == Some(true) || r.tock_split_alone == Some(true) {
        Some("Tock")
    } else {
        None
    }
}

/// One plain phrase for a report, e.g. "two amplitude levels, 293° and
/// 312°, switching about every 40 s (ΔBIC 42, separation 3.1)". `series`
/// is "amplitude" or "rate".
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
    if !r.fitted {
        return format!(
            "{} blocks of {:.0} s, too few to look for two {series} levels",
            r.blocks, r.block_s
        );
    }
    if !r.two_levels {
        return match lone_split(r) {
            Some(side) => {
                format!("one {series} level; the {side} windows fall in two clusters")
            }
            None => format!("one {series} level"),
        };
    }
    let mut t = format!(
        "two {series} levels, {} and {}{unit}, switching about every {} (ΔBIC {:.0}, separation {:.1})",
        v(r.low),
        v(r.high),
        every(r.switch_every_s),
        r.delta_bic,
        r.separation
    );
    if r.one_sided {
        let tick = r.tick_change.unwrap_or(0.0).abs();
        let tock = r.tock_change.unwrap_or(0.0).abs();
        let side = lone_split(r).unwrap_or(if tick > tock { "Tick" } else { "Tock" });
        t += &format!("; {side} carries the change alone");
    } else if r.unlock_jump {
        t += "; the beat error from the unlock jumps with it, the one from the drop does not";
    } else if r.repeats {
        t += &format!(
            "; the low level comes back about every {}",
            every(r.period_s.unwrap_or(r.switch_every_s))
        );
    }
    t
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

    /// The fit before #804, written out plainly: each round's shares and
    /// likelihood from both levels' densities, kept as the reference the
    /// faster E step must match.
    fn reference_mixture(y: &[f64], start: [f64; 2]) -> ([f64; 2], f64, f64, Vec<f64>) {
        let count = y.len() as f64;
        let avg = y.iter().sum::<f64>() / count;
        let spread = y.iter().map(|v| (v - avg) * (v - avg)).sum::<f64>() / count;
        let (mut lv, mut width, mut wt) = (start, (spread.sqrt() / 2.0).max(1e-6), [0.5, 0.5]);
        let density = |x: f64, m: f64, s: f64| (-0.5 * ((x - m) / s).powi(2)).exp();
        let root = (2.0 * std::f64::consts::PI).sqrt();
        let mut shares = vec![0.5; y.len()];
        let mut like = 0.0;
        for _ in 0..300 {
            let mut sum_ll = 0.0;
            for (k, x) in y.iter().enumerate() {
                let lo = wt[0] * density(*x, lv[0], width);
                let hi = wt[1] * density(*x, lv[1], width);
                let tot = (lo + hi).max(1e-300);
                sum_ll += (tot / (width * root)).ln();
                shares[k] = hi / tot;
            }
            like = sum_ll;
            let high: f64 = shares.iter().sum();
            let low = count - high;
            if low.min(high) < 1e-9 {
                break;
            }
            wt = [low / count, high / count];
            let (mut m0, mut m1) = (0.0, 0.0);
            for (x, p) in y.iter().zip(&shares) {
                m0 += x * (1.0 - p);
                m1 += x * p;
            }
            lv = [m0 / low, m1 / high];
            let mut v = 0.0;
            for (x, p) in y.iter().zip(&shares) {
                v += (1.0 - p) * (x - lv[0]).powi(2) + p * (x - lv[1]).powi(2);
            }
            width = (v / count).sqrt().max(1e-6);
        }
        (lv, width, like, shares)
    }

    /// The fit's one-exponential rounds (#804) give the levels, width,
    /// likelihood and shares of the plain two-exponential fit: on a 70/30
    /// split, on a 90/10 split with one reading far from both levels, and
    /// from a start whose second level is empty at once, where the
    /// likelihood must come from a full E step.
    #[test]
    fn the_fast_fit_matches_the_plain_fit() {
        let mut seed = 11;
        let split: Vec<f64> = (0..600)
            .map(|i| if i % 10 < 7 { 0.0 } else { 4.0 } + noise(&mut seed))
            .collect();
        let mut far: Vec<f64> = (0..500)
            .map(|i| if i % 10 < 9 { 10.0 } else { 15.0 } + 0.8 * noise(&mut seed))
            .collect();
        far[250] = 200.0;
        let one: Vec<f64> = (0..300).map(|_| noise(&mut seed)).collect();
        let close = |a: f64, b: f64, what: &str, case: &str| {
            assert!(
                (a - b).abs() <= 1e-9 * a.abs().max(1.0),
                "{case}: {what} {a} against {b}"
            );
        };
        for (case, y, start) in [
            ("70/30", &split, [0.5, 3.5]),
            ("90/10 with a far reading", &far, [10.5, 14.0]),
            ("an empty second level", &one, [0.0, 1000.0]),
        ] {
            let (mu, sd, ll, r) = mixture_from(y, start);
            let (mu0, sd0, ll0, r0) = reference_mixture(y, start);
            close(mu[0], mu0[0], "low level", case);
            close(mu[1], mu0[1], "high level", case);
            close(sd, sd0, "width", case);
            close(ll, ll0, "log likelihood", case);
            for (a, b) in r.iter().zip(&r0) {
                close(*a, *b, "share", case);
            }
            assert!(ll < 0.0, "{case}: {ll}");
        }
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
                    // A bigger swing brings both unlock edges closer.
                    tick_unlock_ms: Some(-7.0 + (v - 300.0) * 0.02),
                    tock_unlock_ms: Some(-7.0 + (v - 300.0) * 0.02),
                }
            })
            .collect()
    }

    #[test]
    fn steady_with_a_slow_fall_is_one_level() {
        // 30 minutes falling 15 degrees as the mainspring runs down.
        let s = series(900, |_, t| 300.0 - 15.0 * t / 1800.0);
        let r = find(&s, &Params::default());
        assert!(r.fitted && !r.two_levels, "{r:?}");
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
            if k.is_multiple_of(2) {
                312.0
            } else {
                293.0
            }
        });
        let r = find(&s, &Params::default());
        assert!(r.switching_levels(), "{r:?}");
        assert!((r.low - 293.0).abs() < 2.0 && (r.high - 312.0).abs() < 2.0);
        assert!(r.switches >= 20);
    }

    #[test]
    fn a_far_level_held_under_half_the_time_is_kept() {
        // 2 s amplitude windows over 30 minutes: 36 s at 280°, then 24 s at
        // 213°, over and over, with a scatter of about 3°. The low level
        // holds 40% of the time and sits more than five median absolute
        // deviations below the median, which sits in the high level; it is
        // a level, not stray windows, so it is fitted and not dropped.
        let mut seed = 12345u64;
        let mut noise = || {
            (0..12)
                .map(|_| {
                    seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    (seed >> 11) as f64 / (1u64 << 53) as f64
                })
                .sum::<f64>()
                - 6.0
        };
        let samples: Vec<Sample> = (0..900)
            .map(|i| {
                let t = i as f64 * 2.0;
                let v = if t % 60.0 < 36.0 { 280.0 } else { 213.0 };
                Sample {
                    start_s: t,
                    end_s: t + 2.0,
                    value: v + 3.0 * noise(),
                    ..Default::default()
                }
            })
            .collect();
        let r = find(&samples, &Params::default());
        assert!(r.two_levels, "{r:?}");
        assert!((r.low - 213.0).abs() < 2.0, "{r:?}");
        assert!((r.high - 280.0).abs() < 2.0, "{r:?}");
        // Of each minute's six blocks, the two wholly at 213° are low; the
        // one that is 4 s low and 6 s high has its median high.
        assert!((r.low_share - 1.0 / 3.0).abs() < 0.02, "{r:?}");
        assert_eq!(r.period_s, Some(60.0), "{r:?}");
    }

    #[test]
    fn a_clean_minority_state_is_found_whatever_split_the_fit_starts_from() {
        // 2 s amplitude windows over 30 minutes: 280° with a dip to 265° for
        // 10 s every 80 s (12.5% of the time), and a scatter of about 0.5°.
        // A fit started from the 20th and 80th percentiles starts both
        // levels inside 280°; started from other splits as well, it finds
        // the dip.
        let mut seed = 4242u64;
        let mut noise = || {
            (0..12)
                .map(|_| {
                    seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    (seed >> 11) as f64 / (1u64 << 53) as f64
                })
                .sum::<f64>()
                - 6.0
        };
        let samples: Vec<Sample> = (0..900)
            .map(|i| {
                let t = i as f64 * 2.0;
                let v = if t % 80.0 < 10.0 { 265.0 } else { 280.0 };
                Sample {
                    start_s: t,
                    end_s: t + 2.0,
                    value: v + 0.5 * noise(),
                    ..Default::default()
                }
            })
            .collect();
        let r = find(&samples, &Params::default());
        assert!(r.two_levels, "{r:?}");
        assert!((r.low - 265.0).abs() < 1.0, "{r:?}");
        assert!((r.high - 280.0).abs() < 1.0, "{r:?}");
        assert!((r.low_share - 0.125).abs() < 0.02, "{r:?}");
    }

    #[test]
    fn stray_windows_far_out_are_still_dropped() {
        // One steady level of 280° with a scatter of about 3°, and a knock
        // every two and a half minutes: five 2 s windows in a row reading
        // 130°, a whole 10 s block, 7% of the windows in all. They are
        // fewer than the smaller state's least share, so they are stray and
        // dropped: no block sits at 130°.
        let mut seed = 777u64;
        let mut noise = || {
            (0..12)
                .map(|_| {
                    seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    (seed >> 11) as f64 / (1u64 << 53) as f64
                })
                .sum::<f64>()
                - 6.0
        };
        let samples: Vec<Sample> = (0..900)
            .map(|i| {
                let t = i as f64 * 2.0;
                let v = if t % 150.0 < 10.0 { 130.0 } else { 280.0 };
                Sample {
                    start_s: t,
                    end_s: t + 2.0,
                    value: v + 3.0 * noise(),
                    ..Default::default()
                }
            })
            .collect();
        let r = find(&samples, &Params::default());
        assert!(!r.two_levels, "{r:?}");
        assert!(r.low > 270.0, "{r:?}");
    }

    #[test]
    fn holes_in_the_blocks_are_time_that_passed() {
        // Low, low, high, high, one 10 s block each: a 40 s pattern. The
        // same 120 blocks heard 30 s apart, with 20 s holes between them,
        // are a 120 s pattern in the take, with no two blocks side by side.
        let block = |i: usize, every: f64| {
            let v = if i % 4 < 2 { 280.0 } else { 310.0 };
            let v = v + 0.1 * ((i * 7) % 5) as f64;
            Sample {
                start_s: i as f64 * every,
                end_s: i as f64 * every + 10.0,
                value: v,
                ..Default::default()
            }
        };
        let dense: Vec<Sample> = (0..120).map(|i| block(i, 10.0)).collect();
        let r = find(&dense, &Params::default());
        assert_eq!(r.period_s, Some(40.0), "{r:?}");
        assert!((r.dwell_low_s - 20.0).abs() < 2.0, "{r:?}");
        let sparse: Vec<Sample> = (0..120).map(|i| block(i, 30.0)).collect();
        let r = find(&sparse, &Params::default());
        assert_eq!(r.period_s, Some(120.0), "{r:?}");
        assert_eq!(r.switches, 0, "{r:?}");
        assert!((r.dwell_low_s - 10.0).abs() < 1e-9, "{r:?}");
        assert!(!r.two_levels, "{r:?}");
    }

    #[test]
    fn readings_longer_than_a_block_sit_side_by_side() {
        // The irregular switching above, read as 30 s rate readings back
        // to back, as `series --reading 30` gives: each reading is a
        // block, and the blocks touch.
        let dwell = [60.0, 90.0, 40.0, 130.0, 70.0, 50.0, 110.0, 80.0];
        let edges: Vec<f64> = dwell
            .iter()
            .cycle()
            .scan(0.0, |t, d| {
                *t += d;
                Some(*t)
            })
            .take(200)
            .collect();
        let mut seed = 7;
        let s: Vec<Sample> = (0..450)
            .map(|j| {
                let t = j as f64 * 30.0;
                let k = edges.iter().filter(|&&e| e <= t + 15.0).count();
                let v = if k.is_multiple_of(2) { 312.0 } else { 293.0 };
                Sample {
                    start_s: t,
                    end_s: t + 30.0,
                    value: v + 3.0 * noise(&mut seed),
                    ..Default::default()
                }
            })
            .collect();
        let r = find(&s, &Params::default());
        assert_eq!(r.block_s, 30.0, "{r:?}");
        assert!(r.two_levels && r.switches >= 20, "{r:?}");
    }

    #[test]
    fn a_dip_once_a_minute_is_regular() {
        let s = series(900, |_, t| if t % 60.0 < 20.0 { 232.0 } else { 245.0 });
        let r = find(&s, &Params::default());
        assert!(r.repeating_levels(), "{r:?}");
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
            if k.is_multiple_of(2) {
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
        assert!(r.one_side_or_unlock(), "{r:?}");
    }

    #[test]
    fn a_single_step_is_not_switching() {
        let s = series(900, |_, t| if t < 1200.0 { 300.0 } else { 280.0 });
        let r = find(&s, &Params::default());
        assert!(!r.switching_levels(), "{r:?}");
    }

    #[test]
    fn a_hopping_unlock_edge_on_one_side_is_flagged() {
        // Steady at 305; one Tick window in five reads 320 as the edge
        // lands on the shoulder; the Tock does not follow.
        let mut s = series(900, |_, _| 305.0);
        for (i, x) in s.iter_mut().enumerate() {
            let tock = x.value - 1.0;
            let hop = i % 5 == 2;
            let tick = if hop { x.value + 16.0 } else { x.value + 1.0 };
            x.tick = Some(tick);
            x.tock = Some(tock);
            // The Tick edge lands 0.35 ms late on the shoulder.
            x.tick_unlock_ms = x.tock_unlock_ms.map(|e| if hop { e + 0.35 } else { e });
        }
        let r = find(&s, &Params::default());
        assert_eq!(r.tick_split_alone, Some(true), "{r:?}");
        assert_eq!(r.tock_split_alone, Some(false));
        assert_eq!(r.tick_edge_split_alone, Some(true));
        assert_eq!(r.tock_edge_split_alone, Some(false));
        assert!(!r.switching_levels());
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
        assert_eq!(r.tick_edge_split_alone, Some(false));
        assert_eq!(r.tock_edge_split_alone, Some(false));
    }

    #[test]
    fn short_takes_are_not_judged() {
        let s = series(20, |_, _| 300.0);
        assert!(!find(&s, &Params::default()).fitted);
    }
}
