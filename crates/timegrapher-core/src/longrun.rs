//! A long run from beat log to report: calibrated rate and amplitude over
//! time, and the periodic changes in each.

use crate::beats::Beat;
use crate::breaks::Break;
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
    /// Peak-to-peak of the folded timing, ms.
    pub timing_swing_ms: f64,
}

/// Below this period (three 10 s rate readings) a rate reading cannot follow
/// a cycle, and the slope of the folded timing turns a fraction of a
/// millisecond into hundreds of s/d. Such a cycle, an escape wheel's tooth
/// pattern for one, is stated as a timing swing in ms instead.
pub const TIMING_UNIT_BELOW_S: f64 = 30.0;

impl RateComponent {
    /// The size to show people, with its unit: the timing swing in ms for a
    /// cycle shorter than [`TIMING_UNIT_BELOW_S`], the rate swing in s/d
    /// otherwise.
    pub fn size(&self) -> (f64, &'static str) {
        if self.component.period_s < TIMING_UNIT_BELOW_S {
            (self.timing_swing_ms, "ms")
        } else {
            (self.rate_swing_s_per_day, "s/d")
        }
    }
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
    /// Pauses and resumes in the take; times here run on with each gap in.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub breaks: Vec<Break>,
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

/// Audio seconds to true seconds for a take: through the clock fit, and
/// with the gaps of `breaks` in. A clock fit from a log with the pauses in
/// already puts the gaps back; any other clock (none, or one steady
/// correction) gets them from the breaks.
pub fn true_time<'a>(clock: Option<&'a ClockFit>, breaks: &'a [Break]) -> impl Fn(f64) -> f64 + 'a {
    let clock_has_gaps = clock.is_some_and(|c| !c.breaks.is_empty());
    move |t| {
        let base = clock.map_or(t, |c| c.map(t));
        if clock_has_gaps {
            base
        } else {
            // A negative gap (the first sound after a pause stamped
            // before the last one before it) would step true time back and
            // overlap the two sides, so it counts as none.
            base + breaks
                .iter()
                .filter(|b| b.audio_s <= t)
                .map(|b| b.gap_s.max(0.0))
                .sum::<f64>()
        }
    }
}

/// Each break's place on [`true_time`]: halfway through its gap. A fit
/// or a window must not reach across one.
pub fn cuts(clock: Option<&ClockFit>, breaks: &[Break]) -> Vec<f64> {
    let map = true_time(clock, breaks);
    breaks
        .iter()
        .map(|b| map(b.audio_s) - 0.5 * b.gap_s - 1e-6)
        .collect()
}

/// Each break's gap on [`true_time`], from the last sound before the
/// pause to the first after it. A reading reaching into one from either
/// side holds only part of its span, so it is left out (see
/// [`touches_gap`]).
///
/// A clock fit with the pauses in, unless its breaks were moved to the
/// listed ones ([`ClockFit::place_breaks`], as `long` and `series` do),
/// places its own break at its first log line after the resume, up to a
/// block after the break's frame, so the gap ends at that break where one
/// is within [`crate::breaks::JUMP_MATCH_S`] after; the sound between the
/// two lies in the gap. A break of zero or negative length is a point.
pub fn gaps(clock: Option<&ClockFit>, breaks: &[Break]) -> Vec<(f64, f64)> {
    let map = true_time(clock, breaks);
    let clock_breaks = clock.map_or(&[][..], |c| &c.breaks[..]);
    breaks
        .iter()
        .map(|b| {
            if b.gap_s <= 0.0 {
                let p = map(b.audio_s);
                return (p, p);
            }
            let start = map(b.audio_s - 1e-6);
            let resume = clock_breaks
                .iter()
                .map(|c| c.audio_s)
                .find(|&c| c >= b.audio_s && c - b.audio_s <= crate::breaks::JUMP_MATCH_S)
                .unwrap_or(b.audio_s);
            (start, map(resume).max(start))
        })
        .collect()
}

/// Where per-stretch fits cut, on true time: each stretch ends where its
/// gap starts and the next starts where it ends, so sound inside a gap
/// (the first block after a resume, before the clock log's break) belongs
/// to neither. A point gap is one cut.
pub fn piece_cuts(gaps: &[(f64, f64)]) -> Vec<f64> {
    let mut cuts: Vec<f64> = gaps
        .iter()
        .flat_map(|&(a, b)| {
            if b > a + 2e-6 {
                vec![a - 1e-6, b - 1e-6]
            } else {
                vec![a - 1e-6]
            }
        })
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts
}

/// Whether a stretch from `start` to `end` on true time reaches into, or
/// across, any of `gaps`.
pub fn touches_gap(gaps: &[(f64, f64)], start: f64, end: f64) -> bool {
    gaps.iter()
        .any(|&(a, b)| start < b - 1e-6 && end > a + 1e-6)
}

pub fn analyse(log: &BeatLog, clock: Option<&ClockFit>, cfg: &LongConfig) -> LongReport {
    analyse_with_breaks(log, clock, &[], cfg)
}

/// [`analyse`] for a take with breaks, whose beats have been renumbered
/// with [`crate::breaks::renumber`]. Times run on true time with the gaps in (from
/// the clock fit, else from the breaks), so a cycle search sees each gap as
/// a missing stretch; rate and beat error are fitted per stretch, and no
/// slice spans a break.
pub fn analyse_with_breaks(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[Break],
    cfg: &LongConfig,
) -> LongReport {
    let map = true_time(clock, breaks);
    let gaps = gaps(clock, breaks);
    let cuts = piece_cuts(&gaps);
    let beats: Vec<Beat> = log
        .beats
        .iter()
        .map(|b| Beat {
            time: map(b.time),
            ..*b
        })
        .collect();
    let duration = map(log.duration_s);
    let mut overall = timing::fit_pieces(&beats, log.bph, &cuts);
    let residuals =
        timing::residuals_pieces(&beats, &cuts).unwrap_or_else(|| vec![f64::NAN; beats.len()]);

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
                timing_swing_ms: c.peak_to_peak * 1000.0,
                rate_shape,
            }
        })
        .collect();

    // Slices for the overview: about 400 over the run, 10 s to 10 min each.
    // A slice reaching into a gap holds part of its span only and reads
    // nothing.
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
        let whole = !touches_gap(&gaps, s, e);
        let w = if whole {
            timing::fit(&beats[b0..bi], log.bph)
        } else {
            None
        };
        if let Some(f) = w {
            jitters.push(f.jitter_us);
        }
        let mut amps = Vec::new();
        while ai < at.len() && at[ai] < e {
            if at[ai] >= s && whole {
                amps.push(av[ai]);
            }
            ai += 1;
        }
        let mut unlocks = Vec::new();
        while ui < ut.len() && ut[ui] < e {
            if ut[ui] >= s && whole {
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
    let listened = duration - breaks.iter().map(|b| b.gap_s.max(0.0)).sum::<f64>();
    let expected = listened * log.bph as f64 / 3600.0;
    LongReport {
        duration_s: duration,
        sample_rate: log.sample_rate,
        bph: log.bph,
        lift_deg: log.lift_deg,
        beats_found: beats.len(),
        clean_fraction: (bw.len() as f64 / expected.max(1.0)).min(1.0),
        clock: clock.cloned(),
        breaks: breaks.to_vec(),
        overall,
        beat_error_unlock_ms: timing::median_signed(
            uv.into_iter(),
            timing::sign_turns(&beats, log.bph, &cuts),
        ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gap_runs_to_the_clock_fits_own_break() {
        // Lines as the recorder writes them: every 10 s, at a block's last
        // frame. The first after a 30 s pause at 300 s of audio comes with
        // the first 0.1 s block, so the clock fit's break is at 300.1 s.
        let k = 1.0 + 20e-6;
        let mut pairs: Vec<(f64, f64)> = (0..=30)
            .map(|i| (i as f64 * 10.0, 1.79e9 + i as f64 * 10.0 * k))
            .collect();
        pairs.extend((0..30).map(|i| {
            let t = 300.1 + i as f64 * 10.0;
            (t, 1.79e9 + t * k + 30.0)
        }));
        let clock = ClockFit::new(&pairs).unwrap();
        assert_eq!(clock.breaks.len(), 1, "{:?}", clock.breaks);
        assert!((clock.breaks[0].audio_s - 300.1).abs() < 1e-9);
        let breaks = [Break {
            audio_s: 300.0,
            gap_s: 30.0,
        }];
        let map = true_time(Some(&clock), &breaks);
        let gaps = gaps(Some(&clock), &breaks);
        let (a, b) = gaps[0];
        // From the last sound before the pause to the clock's resume, so
        // the 0.1 s of sound mapped before the jump lies in the gap.
        assert!((a - map(300.0 - 1e-6)).abs() < 1e-6, "{gaps:?}");
        assert!((b - map(300.1)).abs() < 1e-6, "{gaps:?}");
        assert!((b - a - 30.1).abs() < 0.01, "{gaps:?}");
        // The old middle cut fell 15 s before the pause, in the sound.
        let mid = cuts(Some(&clock), &breaks)[0];
        assert!(mid < a - 10.0, "{mid} {a}");
        // A reading ending at the pause is whole; one taking in the ragged
        // 0.1 s after it is not; one starting at the resume is whole.
        assert!(!touches_gap(&gaps, a - 10.0, a));
        assert!(touches_gap(&gaps, map(300.05), map(300.05) + 10.0));
        assert!(!touches_gap(&gaps, b, b + 10.0));
        // With the clock's break moved to the listed one, the gap ends at
        // the resume and the sound after it maps after the gap.
        let mut placed = clock.clone();
        placed.place_breaks(&breaks);
        assert!((placed.breaks[0].audio_s - 300.0).abs() < 1e-9);
        let on = super::gaps(Some(&placed), &breaks)[0];
        let map_on = true_time(Some(&placed), &breaks);
        assert!(
            (on.0 - a).abs() < 1e-6 && (on.1 - map_on(300.0)).abs() < 1e-9,
            "{on:?}"
        );
        assert!(map_on(300.05) > on.1);
        // A jump of no known length is a point.
        let jump = [Break {
            audio_s: 100.0,
            gap_s: 0.0,
        }];
        let p = super::gaps(None, &jump)[0];
        assert_eq!(p.0, p.1);
    }

    #[test]
    fn a_negative_gap_counts_as_none() {
        // Beats every 0.2 s straight through a jump at 100 s listed with
        // a gap of -0.2 s: true time must not step back, so the rate is
        // the one fitted with the single cut at the jump.
        let breaks = [Break {
            audio_s: 100.0,
            gap_s: -0.2,
        }];
        let map = true_time(None, &breaks);
        assert_eq!(map(100.0), 100.0);
        assert_eq!(map(150.0), 150.0);
        let beats: Vec<Beat> = (0..1000)
            .map(|index| Beat {
                time: map(index as f64 * 0.2 * (1.0 + 1e-5)),
                index,
                quality: 1.0,
            })
            .collect();
        let gaps = super::gaps(None, &breaks);
        let cuts = piece_cuts(&gaps);
        assert_eq!(cuts, vec![map(100.0) - 1e-6]);
        let ours = timing::fit_pieces(&beats, 18000, &cuts).unwrap();
        let single = timing::fit_pieces(&beats, 18000, &[100.0 - 1e-6]).unwrap();
        assert!((ours.rate_s_per_day - single.rate_s_per_day).abs() < 1e-9);
        assert!((ours.rate_s_per_day + 0.864).abs() < 0.01, "{ours:?}");
    }

    #[test]
    fn sound_inside_a_gap_opens_no_stretch() {
        // Recorder-shaped lines with two 30 s pauses, at 300 s and 306 s
        // of audio, each first line after a resume one 0.1 s block late.
        // A beat lands in each of those blocks; the 5.9 s between the
        // pauses is under the 10 s floor, so only the first and last
        // stretches count.
        let k = 1.0 + 20e-6;
        let line = |t: f64, gap: f64| (t, 1.79e9 + t * k + gap);
        let mut pairs: Vec<(f64, f64)> = (0..=30).map(|i| line(i as f64 * 10.0, 0.0)).collect();
        pairs.push(line(300.1, 30.0));
        pairs.extend((0..30).map(|i| line(306.1 + i as f64 * 10.0, 60.0)));
        let clock = ClockFit::new(&pairs).unwrap();
        assert_eq!(clock.breaks.len(), 2, "{:?}", clock.breaks);
        let breaks = [
            Break {
                audio_s: 300.0,
                gap_s: 30.0,
            },
            Break {
                audio_s: 306.0,
                gap_s: 30.0,
            },
        ];
        let map = true_time(Some(&clock), &breaks);
        let beats: Vec<Beat> = (0..3000)
            .map(|index| Beat {
                time: map(index as f64 * 0.2 + 0.05),
                index,
                quality: 1.0,
            })
            .collect();
        let cuts = piece_cuts(&super::gaps(Some(&clock), &breaks));
        assert_eq!(cuts.len(), 4, "{cuts:?}");
        let all = timing::fit_pieces(&beats, 18000, &cuts).unwrap();
        let first = timing::fit(&beats[..1500], 18000).unwrap();
        let last = timing::fit(&beats[1531..], 18000).unwrap();
        assert_eq!(all.beats_used, first.beats_used + last.beats_used);
    }
}
