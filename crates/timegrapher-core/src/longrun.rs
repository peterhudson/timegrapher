//! A long run from beat log to report: calibrated rate and amplitude over
//! time, and the periodic changes in each.

use crate::beats::Beat;
use crate::breaks::Break;
use crate::clean::Cuts;
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
    /// unlock (median of the amplitude windows in the slice), ms; sizes,
    /// given as positive, when the sign turns over across a break
    /// ([`beat_error_turns`]), as the take's figures are.
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
    /// Conditions detected: `sides_by_count` when Tick and Tock were named
    /// by the beat count across some of `breaks`, not told apart by their
    /// sound.
    pub caveats: Vec<crate::measure::Caveat>,
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

/// Grid step for the timing series: fine enough to search the escape
/// wheel, coarser on longer runs to keep the work bounded. The period
/// search needs eight steps a turn, so the shortest period searched is
/// 2 s up to 12 hours and 4 s up to 48 hours: there it reaches a 5 s
/// escape wheel (20 teeth at 28,800 bph, 15 at 21,600) but not a 3.75 s
/// one (15 teeth at 28,800 bph).
pub fn timing_step(span_s: f64) -> f64 {
    if span_s <= 12.0 * 3600.0 {
        0.25
    } else if span_s <= 48.0 * 3600.0 {
        0.5
    } else {
        4.0
    }
}

/// Audio seconds to true seconds for a take, as [`Cuts::true_time`] has
/// it.
pub fn true_time<'a>(clock: Option<&'a ClockFit>, breaks: &[Break]) -> impl Fn(f64) -> f64 + 'a {
    let cuts = Cuts::new(clock, breaks, &[]);
    move |t| cuts.true_time(t)
}

/// Whether the beat error's sign turns over across the take's breaks
/// ([`timing::sign_turns`] on its whole stretches), so its figures are
/// given as sizes.
pub fn beat_error_turns(log: &BeatLog, clock: Option<&ClockFit>, breaks: &[Break]) -> bool {
    if breaks.is_empty() {
        return false;
    }
    let cuts = Cuts::new(clock, breaks, &[]);
    timing::sign_turns(&cuts.beats(&log.beats), log.bph, &cuts.piece_cuts())
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
    let holes = Cuts::new(clock, breaks, &[]);
    let map = |t: f64| holes.true_time(t);
    let cuts = holes.piece_cuts();
    let beats = holes.beats(&log.beats);
    let duration = map(log.duration_s);
    let mut overall = timing::fit_pieces(&beats, log.bph, &cuts);
    let turned = timing::sign_turns(&beats, log.bph, &cuts);
    let size = |v: f64| if turned { v.abs() } else { v };
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
    // A slice that is not whole (reaching into a gap, or with 2 s or more
    // without beats) holds part of its span only and reads nothing.
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
        let whole = holes.whole(&beats, s, e, amp_w);
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
            beat_error_ms: w.map(|f| size(f.beat_error_ms)),
            beat_error_unlock_ms: timing::median_signed(unlocks.into_iter(), turned),
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
    let expected = crate::beats::expected_beats(listened, log.bph);
    LongReport {
        duration_s: duration,
        sample_rate: log.sample_rate,
        bph: log.bph,
        lift_deg: log.lift_deg,
        beats_found: beats.len(),
        clean_fraction: (bw.len() as f64 / expected.max(1.0)).min(1.0),
        clock: clock.cloned(),
        breaks: breaks.to_vec(),
        caveats: (!log.by_count.is_empty())
            .then_some(crate::measure::Caveat::SidesByCount {
                count: log.by_count.len(),
            })
            .into_iter()
            .chain((turned && overall.is_some()).then_some(crate::measure::Caveat::BeatErrorAsSize))
            .chain(clock.and_then(ClockFit::sources_disagree).map(Into::into))
            .collect(),
        overall,
        beat_error_unlock_ms: timing::median_signed(uv.into_iter(), turned),
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
    fn a_reading_says_when_usb_lines_and_a_good_log_disagree() {
        // A minute of a watch, on a take whose clock log measures the card
        // 20 ppm slow, entry by entry exactly: USB lines 100 ppm off that
        // put the caveat, with both figures, on the session reading and the
        // long run (and so `series`); lines within 5 ppm put none.
        use crate::measure::Caveat;
        // An entry every 30 s over an hour.
        let pairs: Vec<(f64, f64)> = (0..=120)
            .map(|i| {
                let a = i as f64 * 30.0;
                (a, 1.79e9 + a * (1.0 + 20e-6))
            })
            .collect();
        let a = crate::synth::generate(
            &crate::synth::SynthConfig {
                duration_s: 60.0,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        );
        let log = crate::stream::analyze_samples(
            &a.samples,
            a.sample_rate,
            &crate::stream::StreamConfig {
                analysis: crate::analysis::AnalysisConfig {
                    bph: Some(28800),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let caveats = |usb: f64| {
            let mut fit = ClockFit::for_rates(&pairs).unwrap();
            fit.rate_from_usb(usb);
            let m = crate::session::measure_with_breaks(&log, Some(&fit), &[], 0.0, log.duration_s);
            let long = analyse(&log, Some(&fit), &Default::default());
            (m.caveats, long.caveats)
        };
        let disagree = |c: &[Caveat]| {
            c.iter().find_map(|c| match *c {
                Caveat::ClockSourcesDisagree {
                    usb_ppm,
                    log_ppm,
                    log_ppm_sd,
                } => Some((usb_ppm, log_ppm, log_ppm_sd)),
                _ => None,
            })
        };
        let (reading, long) = caveats(120.0);
        for c in [&reading, &long] {
            let (usb, log_ppm, sd) = disagree(c).unwrap_or_else(|| panic!("{c:?}"));
            assert_eq!(usb, 120.0);
            assert!((log_ppm - 20.0).abs() < 0.01, "{log_ppm}");
            assert!(sd < 0.01, "{sd}");
        }
        let json = serde_json::to_value(&reading).unwrap();
        assert!(
            json.as_array()
                .unwrap()
                .iter()
                .any(|c| c["code"] == "clock_sources_disagree" && c["usb_ppm"] == 120.0),
            "{json}"
        );
        let (reading, long) = caveats(23.0);
        assert_eq!(disagree(&reading), None, "{reading:?}");
        assert_eq!(disagree(&long), None, "{long:?}");
    }

    #[test]
    fn a_gap_runs_to_the_clock_fits_own_break() {
        // Lines every 10 s, at a block's last frame, but none at the 30 s
        // pause at 305 s of audio (a log from before the recorder wrote
        // one there): the first after it comes with the first 0.1 s block,
        // so the clock fit's break is at 305.1 s.
        let k = 1.0 + 20e-6;
        let mut pairs: Vec<(f64, f64)> = (0..=30)
            .map(|i| (i as f64 * 10.0, 1.79e9 + i as f64 * 10.0 * k))
            .collect();
        pairs.extend((0..30).map(|i| {
            let t = 305.1 + i as f64 * 10.0;
            (t, 1.79e9 + t * k + 30.0)
        }));
        let clock = ClockFit::new(&pairs).unwrap();
        assert_eq!(clock.breaks.len(), 1, "{:?}", clock.breaks);
        assert!((clock.breaks[0].audio_s - 305.1).abs() < 1e-9);
        let breaks = [Break {
            audio_s: 305.0,
            gap_s: 30.0,
        }];
        let map = true_time(Some(&clock), &breaks);
        let holes = Cuts::new(Some(&clock), &breaks, &[]);
        let gaps: Vec<(f64, f64)> = holes.holes().iter().map(|h| (h.start_s, h.end_s)).collect();
        let (a, b) = gaps[0];
        // From the last sound before the pause to the clock's resume, so
        // the 0.1 s of sound mapped before the jump lies in the gap.
        assert!((a - map(305.0 - 1e-6)).abs() < 1e-6, "{gaps:?}");
        assert!((b - map(305.1)).abs() < 1e-6, "{gaps:?}");
        assert!((b - a - 30.1).abs() < 0.01, "{gaps:?}");
        // The old middle cut fell 15 s before the pause, in the sound.
        let mid = map(305.0) - 0.5 * 30.0 - 1e-6;
        assert!(mid < a - 10.0, "{mid} {a}");
        // A reading ending at the pause is whole; one taking in the ragged
        // 0.1 s after it is not; one starting at the resume is whole.
        assert!(holes.clear(a - 10.0, a));
        assert!(!holes.clear(map(305.05), map(305.05) + 10.0));
        assert!(holes.clear(b, b + 10.0));
        // With the clock's break moved to the listed one, the gap ends at
        // the resume and the sound after it maps after the gap.
        let mut placed = clock.clone();
        placed.place_breaks(&breaks);
        assert!((placed.breaks[0].audio_s - 305.0).abs() < 1e-9);
        let on = Cuts::new(Some(&placed), &breaks, &[]).holes()[0];
        let on = (on.start_s, on.end_s);
        let map_on = true_time(Some(&placed), &breaks);
        assert!(
            (on.0 - a).abs() < 1e-6 && (on.1 - map_on(305.0)).abs() < 1e-9,
            "{on:?}"
        );
        assert!(map_on(305.05) > on.1);
        // A jump of no known length is a point.
        let jump = [Break {
            audio_s: 100.0,
            gap_s: 0.0,
        }];
        let p = Cuts::new(None, &jump, &[]).holes()[0];
        assert_eq!(p.start_s, p.end_s);
    }

    #[test]
    fn a_run_over_12_hours_reaches_a_5_s_escape_wheel() {
        // 13 h at 21,600 bph with a 0.5 ms timing ripple every 5 s, the
        // 15-tooth escape wheel's turn: the 0.5 s grid's 4 s floor lets
        // the search reach it, where a 1 s grid stopped at 8 s.
        let duration = 13.0 * 3600.0;
        assert_eq!(timing_step(duration), 0.5);
        let beats: Vec<Beat> = (0..(duration * 6.0) as i64)
            .map(|index| {
                let t = index as f64 / 6.0;
                Beat {
                    time: t + 0.0005 * (std::f64::consts::TAU * t / 5.0).sin(),
                    index,
                    quality: 1.0,
                }
            })
            .collect();
        let log = BeatLog {
            sample_rate: 48000,
            duration_s: duration,
            bph: 21600,
            lift_deg: 52.0,
            beats,
            amplitude_windows: vec![],
            clipped_beats: vec![],
            clipped_samples: 0,
            jumps: vec![],
            sided: vec![],
            by_count: vec![],
            snippets: Vec::new(),
        };
        let r = analyse(&log, None, &LongConfig::default());
        let c = r
            .timing
            .components
            .iter()
            .find(|c| (c.period_s - 5.0).abs() < 0.01)
            .unwrap_or_else(|| panic!("{:?}", r.timing.components));
        assert!((c.peak_to_peak - 0.001).abs() < 0.0002, "{c:?}");
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
        let cuts = Cuts::new(None, &breaks, &[]).piece_cuts();
        assert_eq!(cuts, vec![map(100.0) - 1e-6]);
        let ours = timing::fit_pieces(&beats, 18000, &cuts).unwrap();
        let single = timing::fit_pieces(&beats, 18000, &[100.0 - 1e-6]).unwrap();
        assert!((ours.rate_s_per_day - single.rate_s_per_day).abs() < 1e-9);
        assert!((ours.rate_s_per_day + 0.864).abs() < 0.01, "{ours:?}");
    }

    #[test]
    fn a_beat_error_that_turns_over_reads_as_sizes() {
        // Tick 0.3 ms late throughout, but the beats after a break at
        // 200 s are numbered one on, as when the count crossed it wrong:
        // the sign turns over there, so the slices and the series give
        // sizes, and the take says the sides were named by the count.
        let make = |flip: bool| {
            let beats: Vec<Beat> = (0..2000)
                .map(|i| {
                    let t = i as f64 * 0.2 + 0.05 + if i % 2 == 0 { 0.0003 } else { 0.0 };
                    Beat {
                        time: t,
                        index: if flip && t > 200.0 { i + 1 } else { i },
                        quality: 1.0,
                    }
                })
                .collect();
            let amplitude_windows = (0..200)
                .map(|k| {
                    let start_s = k as f64 * 2.0;
                    crate::amplitude::AmplitudeWindow {
                        start_s,
                        end_s: start_s + 2.0,
                        even_deg: Some(250.0),
                        odd_deg: Some(250.0),
                        beat_error_ms: None,
                        beat_error_unlock_ms: Some(if flip && start_s >= 200.0 {
                            -0.3
                        } else {
                            0.3
                        }),
                        even_unlock_ms: None,
                        odd_unlock_ms: None,
                        even_drop_ms: None,
                        odd_drop_ms: None,
                    }
                })
                .collect();
            BeatLog {
                sample_rate: 48000,
                duration_s: 400.0,
                bph: 18000,
                lift_deg: 52.0,
                beats,
                amplitude_windows,
                clipped_beats: vec![],
                clipped_samples: 0,
                jumps: vec![],
                sided: vec![],
                by_count: if flip { vec![200.0] } else { vec![] },
                snippets: Vec::new(),
            }
        };
        let breaks = [Break {
            audio_s: 200.0,
            gap_s: 0.0,
        }];
        for flip in [false, true] {
            let log = make(flip);
            assert_eq!(beat_error_turns(&log, None, &breaks), flip);
            let r = analyse_with_breaks(&log, None, &breaks, &LongConfig::default());
            let slice_be: Vec<f64> = r.slices.iter().filter_map(|s| s.beat_error_ms).collect();
            let slice_unlock: Vec<f64> = r
                .slices
                .iter()
                .filter_map(|s| s.beat_error_unlock_ms)
                .collect();
            assert!(slice_be.len() > 30 && slice_unlock.len() > 30);
            // One sign throughout: the take's own, or flipped, sizes.
            let s0 = slice_be[0].signum();
            assert!(slice_be.iter().all(|&v| v.signum() == s0), "{slice_be:?}");
            if flip {
                assert_eq!(s0, 1.0);
                assert!(slice_unlock.iter().all(|&v| (v - 0.3).abs() < 1e-9));
                assert_eq!(
                    r.caveats,
                    vec![
                        crate::measure::Caveat::SidesByCount { count: 1 },
                        crate::measure::Caveat::BeatErrorAsSize
                    ]
                );
            } else {
                assert!(r.caveats.is_empty());
            }
            // `series` carries the long run's caveats.
            let lc = LongConfig::default();
            let st = crate::steadiness::check(
                &log,
                None,
                &r,
                &lc,
                &crate::steadiness::Config::default(),
            );
            assert_eq!(st.caveats, r.caveats);
            let [_, be] = crate::steadiness::window_grids(&log, None, &breaks);
            let finite: Vec<f64> = be.y.iter().copied().filter(|v| v.is_finite()).collect();
            assert_eq!(finite.len(), 200);
            assert!(finite.iter().all(|&v| (v - 0.3).abs() < 1e-9), "{flip}");
        }
        // With no watch heard there is no beat error to give as a size.
        let mut quiet = make(true);
        quiet.beats.clear();
        let r = analyse_with_breaks(&quiet, None, &breaks, &LongConfig::default());
        assert!(r.overall.is_none());
        assert!(!r.caveats.contains(&crate::measure::Caveat::BeatErrorAsSize));
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
        let cuts = Cuts::new(Some(&clock), &breaks, &[]).piece_cuts();
        assert_eq!(cuts.len(), 4, "{cuts:?}");
        let all = timing::fit_pieces(&beats, 18000, &cuts).unwrap();
        let first = timing::fit(&beats[..1500], 18000).unwrap();
        let last = timing::fit(&beats[1531..], 18000).unwrap();
        assert_eq!(all.beats_used, first.beats_used + last.beats_used);
    }
}
