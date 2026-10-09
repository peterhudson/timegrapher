//! A long run from beat log to report: calibrated rate and amplitude over
//! time, and the periodic changes in each.

use crate::beats::Beat;
use crate::clock::ClockFit;
use crate::dsp::median;
use crate::longterm::{self, grid_median, Component, LongConfig, SeriesReport};
use crate::stream::BeatLog;
use crate::timing::{self, TimingFit};
use serde::Serialize;

/// Rate and amplitude over one stretch of the run.
#[derive(Debug, Clone, Serialize)]
pub struct Slice {
    pub start_s: f64,
    pub end_s: f64,
    pub rate_s_per_day: Option<f64>,
    /// Signed beat error from the drop (the slice's fit) and from the
    /// unlock (median of the amplitude windows in the slice), ms.
    pub beat_error_ms: Option<f64>,
    pub beat_error_unlock_ms: Option<f64>,
    pub amplitude_deg: Option<f64>,
}

/// A component of the timing, with its size expressed as rate.
#[derive(Debug, Clone, Serialize)]
pub struct RateComponent {
    #[serde(flatten)]
    pub component: Component,
    /// The rate over one cycle, s/d (from the slope of the folded timing).
    pub rate_shape: Vec<f64>,
    /// Peak-to-peak of `rate_shape`, s/d.
    pub rate_swing_s_per_day: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LongReport {
    pub duration_s: f64,
    pub sample_rate: u32,
    pub bph: u32,
    pub lift_deg: f64,
    pub beats_found: usize,
    /// Share of the expected beats found with a clean match, 0..1.
    pub clean_fraction: f64,
    pub clock: Option<ClockFit>,
    /// The whole run fitted at once (calibrated when `clock` is set); its
    /// jitter is the median over the slices.
    pub overall: Option<TimingFit>,
    /// Signed beat error from the unlock, median of the amplitude windows, ms.
    pub beat_error_unlock_ms: Option<f64>,
    pub rate_p05: Option<f64>,
    pub rate_p95: Option<f64>,
    pub amplitude_deg: Option<f64>,
    pub amplitude_p05: Option<f64>,
    pub amplitude_p95: Option<f64>,
    /// Length of each slice in `slices`, seconds.
    pub slice_s: f64,
    pub slices: Vec<Slice>,
    /// Periodic changes in timing (offset in seconds) and in amplitude (degrees).
    pub timing: SeriesReport,
    pub amplitude: SeriesReport,
    pub rate_components: Vec<RateComponent>,
}

fn percentile(v: &[f64], q: f64) -> Option<f64> {
    let mut w: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if w.is_empty() {
        return None;
    }
    w.sort_by(|a, b| a.total_cmp(b));
    Some(w[((w.len() - 1) as f64 * q).round() as usize])
}

/// Grid step for the timing series: fine enough to see the escape wheel
/// on runs up to 12 hours, coarser beyond to keep the work bounded.
pub fn timing_step(span_s: f64) -> f64 {
    if span_s <= 12.0 * 3600.0 {
        0.25
    } else if span_s <= 48.0 * 3600.0 {
        1.0
    } else {
        4.0
    }
}

pub fn analyse(log: &BeatLog, clock: Option<&ClockFit>, cfg: &LongConfig) -> LongReport {
    let map = |t: f64| clock.map_or(t, |c| c.map(t));
    let beats: Vec<Beat> = log
        .beats
        .iter()
        .map(|b| Beat {
            time: map(b.time),
            ..*b
        })
        .collect();
    let duration = map(log.duration_s);
    let mut overall = timing::fit(&beats, log.bph);
    let residuals = timing::residuals(&beats).unwrap_or_else(|| vec![f64::NAN; beats.len()]);

    // Uniform series for the period search, on the watch's own clock: a
    // beat's watch time is its beat number times the nominal beat period.
    // A wheel turns once per fixed number of beats (the fourth wheel every
    // 480 at 28,800 bph), so on this clock its period is exact and its
    // phase doesn't wander when the rate does.
    let nominal = 3600.0 / log.bph as f64;
    let (i0, t0) = beats.first().map_or((0, 0.0), |b| (b.index, b.time));
    let watch = |b: &Beat| t0 + (b.index - i0) as f64 * nominal;
    let to_watch = |t: f64| -> f64 {
        let k = beats.partition_point(|b| b.time < t);
        match (k.checked_sub(1).and_then(|i| beats.get(i)), beats.get(k)) {
            (Some(a), Some(b)) => {
                let f = (t - a.time) / (b.time - a.time).max(1e-9);
                watch(a) + f * (watch(b) - watch(a))
            }
            (Some(a), None) => watch(a) + (t - a.time),
            (None, Some(b)) => watch(b) - (b.time - t),
            (None, None) => t,
        }
    };
    let step = timing_step(duration);
    let n = (duration / step).floor() as usize;
    let (bw, br): (Vec<f64>, Vec<f64>) = beats
        .iter()
        .zip(&residuals)
        .filter(|(b, r)| b.quality > 0.4 && r.is_finite())
        .map(|(b, &r)| (watch(b), r))
        .unzip();
    let timing_grid = grid_median(&bw, &br, 0.0, step, n);
    let amp_w = log
        .amplitude_windows
        .first()
        .map_or(2.0, |w| w.end_s - w.start_s);
    let astep = step.max(amp_w);
    let (at, av): (Vec<f64>, Vec<f64>) = log
        .amplitude_windows
        .iter()
        .filter_map(|w| Some((map((w.start_s + w.end_s) / 2.0), w.mean()?)))
        .unzip();
    let (ut, uv): (Vec<f64>, Vec<f64>) = log
        .amplitude_windows
        .iter()
        .filter_map(|w| Some((map((w.start_s + w.end_s) / 2.0), w.beat_error_unlock_ms?)))
        .unzip();
    let aw: Vec<f64> = at.iter().map(|&t| to_watch(t)).collect();
    let amp_grid = grid_median(&aw, &av, 0.0, astep, (duration / astep).floor() as usize);

    let timing = longterm::analyse(&timing_grid, cfg);
    let amplitude = longterm::analyse(&amp_grid, cfg);
    let rate_components = timing
        .components
        .iter()
        .map(|c| {
            let rate_shape = longterm::rate_profile(&c.shape, c.period_s);
            let lo = rate_shape.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = rate_shape.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            RateComponent {
                component: c.clone(),
                rate_swing_s_per_day: hi - lo,
                rate_shape,
            }
        })
        .collect();

    // Slices for the overview: about 400 over the run, 10 s to 10 min each.
    let slice_s = (duration / 400.0).clamp(10.0, 600.0).round();
    let mut slices = Vec::new();
    let mut jitters = Vec::new();
    let mut s = 0.0;
    let (mut bi, mut ai, mut ui) = (0, 0, 0);
    while s + slice_s <= duration + 1e-9 {
        let e = s + slice_s;
        let b0 = bi;
        while bi < beats.len() && beats[bi].time < e {
            bi += 1;
        }
        let w = timing::fit(&beats[b0..bi], log.bph);
        if let Some(f) = w {
            jitters.push(f.jitter_us);
        }
        let mut amps = Vec::new();
        while ai < at.len() && at[ai] < e {
            if at[ai] >= s {
                amps.push(av[ai]);
            }
            ai += 1;
        }
        let mut unlocks = Vec::new();
        while ui < ut.len() && ut[ui] < e {
            if ut[ui] >= s {
                unlocks.push(uv[ui]);
            }
            ui += 1;
        }
        slices.push(Slice {
            start_s: s,
            end_s: e,
            rate_s_per_day: w.map(|f| f.rate_s_per_day),
            beat_error_ms: w.map(|f| f.beat_error_ms),
            beat_error_unlock_ms: (!unlocks.is_empty()).then(|| median(&mut unlocks)),
            amplitude_deg: (!amps.is_empty()).then(|| median(&mut amps)),
        });
        s = e;
    }
    // Jitter is beat-to-beat scatter: over a whole run the fit's residuals
    // are dominated by the rate wandering, so take the median over slices.
    if let Some(f) = overall.as_mut() {
        if !jitters.is_empty() {
            f.jitter_us = median(&mut jitters);
        }
    }
    let rates: Vec<f64> = slices.iter().filter_map(|s| s.rate_s_per_day).collect();
    let amps: Vec<f64> = slices.iter().filter_map(|s| s.amplitude_deg).collect();
    let expected = duration * log.bph as f64 / 3600.0;
    LongReport {
        duration_s: duration,
        sample_rate: log.sample_rate,
        bph: log.bph,
        lift_deg: log.lift_deg,
        beats_found: beats.len(),
        clean_fraction: (bw.len() as f64 / expected.max(1.0)).min(1.0),
        clock: clock.cloned(),
        overall,
        beat_error_unlock_ms: {
            let mut u = uv;
            (!u.is_empty()).then(|| median(&mut u))
        },
        rate_p05: percentile(&rates, 0.05),
        rate_p95: percentile(&rates, 0.95),
        amplitude_deg: {
            let mut a = av.clone();
            (!a.is_empty()).then(|| median(&mut a))
        },
        amplitude_p05: percentile(&amps, 0.05),
        amplitude_p95: percentile(&amps, 0.95),
        slice_s,
        slices,
        timing,
        amplitude,
        rate_components,
    }
}
