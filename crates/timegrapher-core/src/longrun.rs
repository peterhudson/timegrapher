//! A long run from beat log to report: calibrated rate and amplitude over
//! time, and the periodic changes in each.

use crate::beats::Beat;
use crate::breaks::Break;
use crate::clean::Cuts;
use crate::clock::ClockFit;
use crate::dsp::median;
use crate::dsp::percentile;
use crate::longterm::{self, grid_median, Component, Grid, LongConfig, SeriesReport};
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
    /// The escapement the run was read for (#2313), given only for a
    /// co-axial: its slices give no drop beat error, nor does
    /// [`LongReport::drop_beat_error_ms`].
    #[serde(skip_serializing_if = "crate::escapement::Escapement::is_swiss_lever")]
    pub escapement: crate::escapement::Escapement,
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
    /// The take's whole reading's timing scatter, µs, where it finds no
    /// regular beat ([`LongReport::no_rate_text`]).
    #[serde(skip)]
    pub no_watch_jitter_us: Option<f64>,
}

/// Grid step for the timing series: fine enough to search the escape
/// wheel, coarser on longer runs to keep the work bounded. The period
/// search needs eight steps a turn, so the grid alone allows 2 s up to
/// 12 hours, 4 s up to 80 hours (a full Power Reserve test, the memory
/// budget's run) and 32 s past that. The shortest period searched is the
/// larger of that and `Config::min_period_s` (3 s): 3 s up to 12 hours,
/// then 4 s, which reaches a 5 s escape wheel (20 teeth at 28,800 bph, 15
/// at 21,600) but not a 3.75 s one (15 teeth at 28,800 bph), then 32 s.
/// The report states the shortest period each search tried (#1897).
pub fn timing_step(span_s: f64) -> f64 {
    if span_s <= 12.0 * 3600.0 {
        0.25
    } else if span_s <= 80.0 * 3600.0 {
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
    analyse_read(log, clock, clock, breaks, cfg)
}

/// [`analyse_with_breaks`], its times put on `map` and its report saying
/// it was read on `read_on`. Where the take's whole reading finds no
/// regular beat ([`whole_reading`]), the report gives no figures
/// ([`LongReport::read_no_watch`], #2355).
fn analyse_read(
    log: &BeatLog,
    map: Option<&ClockFit>,
    read_on: Option<&ClockFit>,
    breaks: &[Break],
    cfg: &LongConfig,
) -> LongReport {
    let mut r = analyse_figures(log, map, read_on, breaks, cfg);
    let whole = whole_reading(log, map, breaks);
    if !whole.watch_heard {
        r.read_no_watch(crate::measure::no_watch_reason_in(&whole.caveats).unwrap_or_default());
        r.no_watch_jitter_us = whole.jitter_us;
    }
    r
}

/// A take's long-run report as `long` and `series` read it (#2355,
/// #2404): [`analyse_with_breaks`], or for a log whose times were put on
/// `clock` already (`on_clock`, [`crate::joined::join`]) that clock named
/// but not put on again, its lock then
/// judged with the beat rate set, `bph_set` (none on Auto), as the
/// take's whole reading judges it ([`LongReport::judge_lock`]).
pub fn read_take(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[Break],
    on_clock: bool,
    bph_set: Option<u32>,
    cfg: &LongConfig,
) -> LongReport {
    let map = clock.filter(|_| !on_clock);
    let mut r = analyse_read(log, map, clock, breaks, cfg);
    r.judge_lock(bph_set, log, map);
    r
}

/// The take's whole reading, `log` from its start to its end on `map`
/// with its `breaks` ([`crate::session::measure_with_breaks`]): the
/// reading whose verdicts the long-run report takes, so it says what
/// `session` and `analyze` say of the same sound (#2355).
fn whole_reading(
    log: &BeatLog,
    map: Option<&ClockFit>,
    breaks: &[Break],
) -> crate::session::Measurement {
    crate::session::measure_with_breaks(log, map, breaks, 0.0, log.duration_s)
}

/// [`analyse_read`]'s figures, before the take's whole reading is
/// judged.
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn analyse_figures(
    log: &BeatLog,
    map: Option<&ClockFit>,
    read_on: Option<&ClockFit>,
    breaks: &[Break],
    cfg: &LongConfig,
) -> LongReport {
    // Counted on audio time: a log read in stretches has its pauses on the
    // clock already, and its caller counts them ([`crate::joined::pauses_unseen`]).
    let unseen = map.map_or(0, |c| c.unseen(breaks));
    let holes = Cuts::new(map, breaks, &[]);
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
        .filter(|(b, r)| b.is_good() && r.is_finite())
        .map(|(b, &r)| (watch(b), r))
        .unzip();
    let mut timing_grid = grid_median(&bw, &br, 0.0, step, n);
    // Where each piece's timing starts again from its own fit, on the
    // grid ([`longterm::piece_starts`], the one rule). The grid is on the
    // watch's clock, a beat count, so a cut is placed at the first beat
    // after it, not mapped through the true-time clock; and the bin it
    // falls in holds both pieces' residuals, so it reads nothing.
    let cuts_on_watch: Vec<f64> = cuts
        .iter()
        .filter_map(|&c| beats.get(beats.partition_point(|b| b.time < c)))
        .map(watch)
        .collect();
    for &c in &cuts_on_watch {
        let at = c / step;
        if at > at.floor() {
            if let Some(v) = timing_grid.y.get_mut(at.floor() as usize) {
                *v = f64::NAN;
            }
        }
    }
    let piece_starts = longterm::piece_starts(&timing_grid, &cuts_on_watch);
    let amp_w = log
        .amplitude_windows
        .first()
        .map_or(2.0, |w| w.end_s - w.start_s);
    let astep = step.max(amp_w);
    // The windows a reading may use, each at its middle on true time: one
    // that reaches into a break's gap is left out, as Steadiness and every
    // reading leave it (#636).
    let windows = holes.amplitude_windows(&log.amplitude_windows);
    let (at, av): (Vec<f64>, Vec<f64>) = windows
        .iter()
        .filter_map(|w| Some((map((w.start_s + w.end_s) / 2.0), w.mean()?)))
        .unzip();
    let (ut, uv): (Vec<f64>, Vec<f64>) = windows
        .iter()
        .filter_map(|w| Some((map((w.start_s + w.end_s) / 2.0), w.beat_error_unlock_ms?)))
        .unzip();
    let aw: Vec<f64> = at.iter().map(|&t| to_watch(t)).collect();
    let amp_grid = grid_median(&aw, &av, 0.0, astep, (duration / astep).floor() as usize);

    let amp_cuts: Vec<f64> = cuts.iter().map(|&c| to_watch(c)).collect();
    let amp_starts = longterm::piece_starts(&amp_grid, &amp_cuts);
    // The two searches read nothing of each other: on two cores (#1839).
    let (amplitude, timing) = crate::cores::both(
        || longterm::analyse_pieces(&amp_grid, &amp_starts, cfg),
        || longterm::analyse_timing_pieces(&timing_grid, &piece_starts, cfg),
    );
    let rate_components = timing
        .components
        .iter()
        .map(|c| RateComponent {
            component: c.clone(),
            rate_swing_s_per_day: longterm::rate_swing(&c.shape, c.period_s),
            timing_swing_ms: c.peak_to_peak * 1000.0,
            rate_shape: longterm::rate_profile(&c.shape, c.period_s),
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
            beat_error_ms: log
                .escapement
                .drop_beat_error(w.map(|f| size(f.beat_error_ms))),
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
    let listened = duration - holes.breaks().iter().map(|b| b.gap_s).sum::<f64>();
    let expected = crate::beats::expected_beats(listened, log.bph);
    let beat_error_unlock_ms = timing::median_signed(uv.into_iter(), turned);
    // A co-axial's beat error is the unlock's alone, or none, said (#2313).
    let unlock = beat_error_unlock_ms.map(|_| crate::session::BeatErrorSource::Unlock);
    let coaxial = log.escapement.beat_error_caveat(
        unlock,
        crate::measure::DropWhy::UnlockNotFound,
        overall.is_some(),
    );
    LongReport {
        duration_s: duration,
        sample_rate: log.sample_rate,
        bph: log.bph,
        lift_deg: log.lift_deg,
        beats_found: beats.len(),
        clean_fraction: (bw.len() as f64 / expected.max(1.0)).min(1.0),
        clock: read_on.cloned(),
        breaks: breaks.to_vec(),
        caveats: (!log.by_count.is_empty())
            .then_some(crate::measure::Caveat::SidesByCount {
                count: log.by_count.len(),
            })
            .into_iter()
            .chain((turned && overall.is_some()).then_some(crate::measure::Caveat::BeatErrorAsSize))
            .chain(read_on.and_then(ClockFit::sources_disagree).map(Into::into))
            .chain((unseen > 0).then_some(crate::measure::Caveat::PausesUnseen { count: unseen }))
            .chain(log.escapement.amplitude_caveat(!av.is_empty()))
            .chain(coaxial)
            .collect(),
        overall,
        beat_error_unlock_ms,
        escapement: log.escapement,
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
        no_watch_jitter_us: None,
    }
}

impl LongReport {
    /// The report of `log`'s beats judged as `session` judges the take's
    /// whole reading ([`crate::session::Measurement::judge_lock`], #2404):
    /// on Auto (`bph_set` none), where the lock is off every standard
    /// rate ([`crate::analysis::lock_off_standard`]), no rate in s/d is
    /// given ([`Self::read_off_standard`]), and the caveat
    /// `off_standard_rate` is the whole reading's, read on `map`. The
    /// verdict is the lock's, never made again here. A run with no
    /// regular beat takes none: its whole reading has no rate to read off.
    pub fn judge_lock(&mut self, bph_set: Option<u32>, log: &BeatLog, map: Option<&ClockFit>) {
        if !crate::analysis::lock_off_standard(bph_set, log) {
            return;
        }
        let mut whole = whole_reading(log, map, &self.breaks);
        whole.judge_lock(bph_set, log);
        if let Some(o) = crate::measure::off_standard(&whole.caveats) {
            self.read_off_standard(o);
        }
    }

    /// A run whose lock is off every standard rate (`o`, Norm on #2223):
    /// no rate in s/d over the run, in a slice, or in a rate cycle's
    /// swing, and the caveat `off_standard_rate` says why
    /// ([`crate::measure::off_standard_withheld`]). The overall fit stays:
    /// it is the fit at the report's beat rate, and the drop beat error
    /// is read from it; [`Self::rate_s_per_day`] gives none.
    fn read_off_standard(&mut self, o: crate::timing::OffStandard) {
        for s in &mut self.slices {
            s.rate_s_per_day = None;
        }
        (self.rate_p05, self.rate_p95) = (None, None);
        self.rate_components.clear();
        crate::measure::off_standard_withheld(o, None, &mut self.caveats);
    }

    /// A run in which the take's whole reading finds no regular beat
    /// (`reason`, [`crate::measure::no_watch_reason_of`], #2355): no
    /// figures, as no reading gives any, so no fit, slice, median,
    /// spread or cycle; the caveats that hang on a figure go with it,
    /// and `no_watch` says why.
    fn read_no_watch(&mut self, reason: crate::measure::NoWatchReason) {
        use crate::measure::Caveat;
        self.overall = None;
        self.beat_error_unlock_ms = None;
        (self.rate_p05, self.rate_p95) = (None, None);
        self.amplitude_deg = None;
        (self.amplitude_p05, self.amplitude_p95) = (None, None);
        for s in &mut self.slices {
            s.rate_s_per_day = None;
            s.beat_error_ms = None;
            s.beat_error_unlock_ms = None;
            s.amplitude_deg = None;
        }
        for series in [&mut self.timing, &mut self.amplitude] {
            for g in [&mut series.grid, &mut series.detrended] {
                g.y.fill(f64::NAN);
            }
            series.components.clear();
        }
        self.rate_components.clear();
        self.caveats.retain(|c| {
            !matches!(
                c,
                Caveat::BeatErrorAsSize | Caveat::CoaxialBeatError { .. } | Caveat::CoaxialEstimate
            )
        });
        crate::measure::add(&mut self.caveats, Caveat::NoWatch { reason });
    }

    /// Why no regular beat was heard over the run, where none was
    /// ([`Self::read_no_watch`]).
    pub fn no_watch(&self) -> Option<crate::measure::NoWatchReason> {
        crate::measure::no_watch_reason_in(&self.caveats)
    }

    /// Whether the run gives no rate in s/d: no regular beat heard, or a
    /// lock off every standard rate on Auto ([`Self::judge_lock`]).
    pub fn rate_withheld(&self) -> bool {
        self.no_watch().is_some() || crate::measure::off_standard(&self.caveats).is_some()
    }

    /// What the report says in its rate's place where it gives none in
    /// s/d, in core's words: no regular beat found, as every surface says
    /// it ([`crate::measure::no_watch_text`], with `bph_set` the beat
    /// rate set) and that there are no readings; or why a lock off every
    /// standard rate gives no rate (`off_standard_rate`). None where the
    /// rate is given.
    pub fn no_rate_text(&self, bph_set: Option<u32>) -> Option<String> {
        if let Some(reason) = self.no_watch() {
            let none = crate::measure::no_watch_text(reason, self.no_watch_jitter_us, bph_set);
            return Some(format!("{none}. {}", crate::session::NO_READINGS));
        }
        crate::measure::off_standard(&self.caveats)
            .and_then(|o| crate::measure::Caveat::OffStandardRate(o).long(None))
    }

    /// Whether the report gives none of `kind`'s readings: the rate's
    /// where it gives no rate in s/d ([`Self::rate_withheld`]), and every
    /// series' where no regular beat was heard ([`Self::no_watch`]). The
    /// one rule for every grid of the take's series read beside the
    /// report ([`Self::judged`]).
    pub fn withholds(&self, kind: crate::steadiness::SeriesKind) -> bool {
        use crate::steadiness::SeriesKind;
        match kind {
            SeriesKind::Rate => self.rate_withheld(),
            SeriesKind::Amplitude | SeriesKind::BeatError => self.no_watch().is_some(),
        }
    }

    /// A grid of `kind`'s readings as the report's verdict leaves it
    /// ([`Self::withholds`]): its readings left out where they are
    /// withheld, its tiling kept, so every test and view laid on it reads
    /// too few.
    pub fn judge(&self, kind: crate::steadiness::SeriesKind, g: &mut Grid) {
        if self.withholds(kind) {
            g.y.fill(f64::NAN);
        }
    }

    /// [`Self::judge`], of a grid given and returned.
    pub fn judged(&self, kind: crate::steadiness::SeriesKind, mut g: Grid) -> Grid {
        self.judge(kind, &mut g);
        g
    }

    /// The rate's, the amplitude's and the beat error's grids, in that
    /// order, each [`Self::judged`].
    pub fn judged_grids(&self, [rate, amp, be]: [Grid; 3]) -> [Grid; 3] {
        use crate::steadiness::SeriesKind;
        [
            self.judged(SeriesKind::Rate, rate),
            self.judged(SeriesKind::Amplitude, amp),
            self.judged(SeriesKind::BeatError, be),
        ]
    }

    /// The run's rate in s/d, the overall fit's: none where it is
    /// withheld ([`Self::rate_withheld`]).
    pub fn rate_s_per_day(&self) -> Option<f64> {
        self.overall
            .filter(|_| !self.rate_withheld())
            .map(|f| f.rate_s_per_day)
    }

    /// The run's whole fit as a document gives it
    /// ([`crate::escapement::Escapement::overall_json`]), its rate null
    /// where it is withheld, as `analyze`'s `overall` is.
    pub fn overall_json(&self) -> serde_json::Value {
        let mut v = self.escapement.overall_json(self.overall.as_ref());
        if let Some(o) = v.as_object_mut().filter(|_| self.rate_withheld()) {
            o.insert("rate_s_per_day".into(), serde_json::Value::Null);
        }
        v
    }

    /// The run's beat error from the drop, ms, signed: the overall fit's,
    /// none on a co-axial ([`crate::escapement::Escapement::drop_beat_error`]).
    pub fn drop_beat_error_ms(&self) -> Option<f64> {
        self.escapement
            .drop_beat_error(self.overall.map(|f| f.beat_error_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic take's beat log, read on Auto (`bph` none) or at a set
    /// rate, as `long` reads a sound file: at amplitude `amp(t)`, its rate
    /// moved by `rate(t)` s/d.
    fn read_log_of(
        cfg: &crate::synth::SynthConfig,
        bph: Option<u32>,
        amp: &dyn Fn(f64) -> f64,
        rate: &dyn Fn(f64) -> f64,
    ) -> BeatLog {
        let a = crate::synth::generate(cfg, amp, rate);
        crate::stream::analyze_samples(
            &a.samples,
            a.sample_rate,
            &crate::stream::StreamConfig {
                analysis: crate::analysis::AnalysisConfig {
                    bph,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
    }

    /// The audit's probe (#2404): 17,570 bph at +5 s/d, its rate swinging
    /// 50 s/d every 15 s, so the run has a rate cycle. On Auto its lock
    /// is off every standard rate, so the long-run report gives no rate in
    /// s/d anywhere (overall, slices, spread, rate cycles, `series`' rate
    /// readings), and says why with the take's whole reading's own
    /// caveat; its beat error and amplitude stay. With the rate set, it
    /// reads.
    #[test]
    fn a_lock_off_every_standard_rate_gives_no_rate_and_a_set_one_reads() {
        let cfg = crate::synth::SynthConfig {
            duration_s: 120.0,
            bph: 17570,
            rate_s_per_day: 5.0,
            ..Default::default()
        };
        let swing = |t: f64| 50.0 * (t * std::f64::consts::TAU / 15.0).sin();
        let lc = LongConfig::default();
        let log = read_log_of(&cfg, None, &|_| 270.0, &swing);
        // Before the verdict, the figures each blanked field would give.
        let figures = analyse_figures(&log, None, None, &[], &lc);
        assert!(!figures.rate_components.is_empty());
        let r = read_take(&log, None, &[], false, None, &lc);
        assert!(
            r.rate_withheld() && r.no_watch().is_none(),
            "{:?}",
            r.caveats
        );
        assert_eq!(r.rate_s_per_day(), None);
        assert!(r.overall_json()["rate_s_per_day"].is_null());
        assert!(r.overall.is_some() && r.drop_beat_error_ms().is_some());
        assert!(r.amplitude_deg.is_some());
        assert_eq!((r.rate_p05, r.rate_p95), (None, None));
        assert!(r.rate_components.is_empty());
        assert!(!r.slices.is_empty());
        assert!(r.slices.iter().all(|s| s.rate_s_per_day.is_none()));
        assert!(r.slices.iter().all(|s| s.amplitude_deg.is_some()));
        // The take's whole reading's verdict and figures, as `session`
        // reads them, and the sentence `analyze` gives.
        let mut whole = crate::session::measure(&log, None, 0.0, log.duration_s);
        whole.judge_lock(None, &log);
        let o = crate::measure::off_standard(&r.caveats).expect("the caveat");
        assert_eq!(Some(o), crate::measure::off_standard(&whole.caveats));
        assert!(r
            .no_rate_text(None)
            .is_some_and(|t| t.starts_with("No rate in s/d")));
        // `series` reads no rate either; the amplitude still reads.
        let check = crate::steadiness::Config::default();
        let st = crate::steadiness::check(&log, None, &r, &lc, &check);
        assert_eq!(st.series[0].readings, 0);
        assert_eq!(st.series[0].median, None);
        assert!(st.series[1].readings > 0);
        assert!(st.isochronism.is_none() && st.walk.is_none());
        assert!(st.distributions.overall.iter().all(|d| d.series != "rate"));

        // Set at 17,570: the rate reads, and no caveat says otherwise.
        let log = read_log_of(&cfg, Some(17570), &|_| 270.0, &swing);
        let r = read_take(&log, None, &[], false, Some(17570), &lc);
        assert!(!r.rate_withheld(), "{:?}", r.caveats);
        let rate = r.rate_s_per_day().expect("a rate");
        assert!((rate - 5.0).abs() < 0.5, "{rate}");
        assert!(r.slices.iter().all(|s| s.rate_s_per_day.is_some()));
        assert!(r.no_rate_text(Some(17570)).is_none());
        let st = crate::steadiness::check(&log, None, &r, &lc, &check);
        assert!(st.series[0].readings > 0);
    }

    /// A named wheel's rate line off every standard rate on Auto (Norm 33
    /// on #2487): 4,000 s at 17,570 bph, its rate swinging 30 s/d every
    /// 40 s, with a 40 s wheel named. Set at 17,570 the line and the
    /// rate's spectrum give their sizes in s/d; on Auto neither does, the
    /// line keeping its timing in ms.
    #[test]
    fn a_wheels_rate_line_gives_no_s_per_day_off_every_standard_rate() {
        let cfg = crate::synth::SynthConfig {
            duration_s: 4000.0,
            bph: 17570,
            rate_s_per_day: 5.0,
            ..Default::default()
        };
        let swing = |t: f64| 30.0 * (t * std::f64::consts::TAU / 40.0).sin();
        let lc = LongConfig {
            wheels: vec![crate::periodicity::Wheel {
                name: "test wheel".into(),
                period_s: 40.0,
            }],
            ..Default::default()
        };
        let check = crate::steadiness::Config::default();
        for bph_set in [Some(17570), None] {
            let log = read_log_of(&cfg, bph_set, &|_| 270.0, &swing);
            let r = read_take(&log, None, &[], false, bph_set, &lc);
            assert_eq!(r.rate_withheld(), bph_set.is_none());
            let lines = crate::wheel_lines::lines_for_take(&log, None, &r, &lc, false);
            let rate = lines
                .iter()
                .find(|l| l.series == "rate")
                .expect("the rate line");
            assert!(!rate.bins.is_empty());
            let st = crate::steadiness::check(&log, None, &r, &lc, &check);
            let spectrum = st
                .spectra
                .iter()
                .find(|s| s.series == "rate")
                .expect("its spectrum");
            if bph_set.is_some() {
                assert!(rate
                    .bins
                    .iter()
                    .all(|b| b.size_s_per_day.is_some() && b.se_s_per_day.is_some()));
                assert!(spectrum.reading_swing.is_some());
            } else {
                assert!(rate
                    .bins
                    .iter()
                    .all(|b| b.size_s_per_day.is_none() && b.se_s_per_day.is_none()));
                assert!(rate.bins.iter().all(|b| b.size > 0.0));
                assert!(spectrum.reading_swing.is_none());
            }
        }
    }

    /// No regular beat (#2355): noise alone (45 s, every sound of the watch
    /// at gain 0), and a watch whose beats scatter far past 2% of a beat
    /// (17,570 bph, each beat's rate moved by up to 3000 s/d, its
    /// amplitude swinging 20° every 20 s), read for a Swiss lever and for
    /// a co-axial. Each take's whole reading finds no regular beat, as
    /// `session` and `analyze` find, so the long-run report gives no
    /// figures (no fit, slice, median, spread, grid or cycle), no caveat
    /// that hangs on one, no off-standard verdict, and `no_watch` with
    /// the take's reason; `series` reads nothing either. The scattered
    /// watch gives each of those figures before the verdict.
    #[test]
    fn no_regular_beat_gives_no_figures() {
        use crate::escapement::Escapement;
        let mut silent = crate::synth::SynthConfig {
            duration_s: 45.0,
            ..Default::default()
        };
        for s in &mut silent.sounds {
            s.gain = 0.0;
        }
        let scattered = crate::synth::SynthConfig {
            duration_s: 120.0,
            bph: 17570,
            rate_s_per_day: 5.0,
            ..Default::default()
        };
        // A rate from -3000 to +3000 s/d, new every millisecond of the take.
        let jump = |t: f64| {
            let x = ((t * 1000.0).floor() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 11;
            3000.0 * ((x as f64 / (1u64 << 53) as f64) * 2.0 - 1.0)
        };
        let swing = |t: f64| 270.0 + 20.0 * (t * std::f64::consts::TAU / 20.0).sin();
        let read = |cfg: &crate::synth::SynthConfig, e: Escapement, take: bool| {
            let a = if take {
                crate::synth::generate(cfg, swing, jump)
            } else {
                crate::synth::generate(cfg, |_| 270.0, |_| 0.0)
            };
            let mut sc = crate::stream::StreamConfig::default();
            sc.analysis.amplitude.escapement = e;
            crate::stream::analyze_samples(&a.samples, a.sample_rate, &sc)
        };
        let lc = LongConfig::default();
        for (log, figures) in [
            (read(&silent, Escapement::SwissLever, false), false),
            (read(&scattered, Escapement::SwissLever, true), true),
            (read(&scattered, Escapement::CoAxial, true), true),
        ] {
            assert!(!log.beats.is_empty(), "the beat finder reads beats");
            let whole = crate::session::measure(&log, None, 0.0, log.duration_s);
            assert!(!whole.watch_heard);
            let before = analyse_figures(&log, None, None, &[], &lc);
            if figures {
                figures_before_the_verdict(&before, &log);
            }
            for r in [
                analyse(&log, None, &lc),
                read_take(&log, None, &[], false, None, &lc),
            ] {
                no_figures(&r, &log, &whole, &lc);
            }
        }
    }

    /// What a take with no regular beat, `log`, gives before the verdict
    /// blanks it: each figure [`no_figures`] checks is blanked.
    fn figures_before_the_verdict(before: &LongReport, log: &BeatLog) {
        use crate::measure::Caveat;
        let live = |s: &SeriesReport| s.grid.y.iter().any(|v| v.is_finite());
        assert!(before.overall.is_some() && before.beat_error_unlock_ms.is_some());
        assert!(before.amplitude_deg.is_some() && before.amplitude_p05.is_some());
        assert!(before.amplitude_p95.is_some());
        assert!(before.slices.iter().any(|s| s.amplitude_deg.is_some()));
        assert!(before
            .slices
            .iter()
            .any(|s| s.beat_error_unlock_ms.is_some()));
        assert!(live(&before.timing) && live(&before.amplitude));
        assert!(!before.amplitude.components.is_empty());
        assert!(crate::analysis::lock_off_standard(None, log));
        if log.escapement.is_co_axial() {
            assert!(before.caveats.contains(&Caveat::CoaxialEstimate));
            assert!(before
                .caveats
                .iter()
                .any(|c| matches!(c, Caveat::CoaxialBeatError { .. })));
        }
    }

    /// A report of `log`, whose whole reading `whole` finds no regular
    /// beat, with no figures, and `series` reading none (`steadiness::check` read
    /// with `lc`).
    fn no_figures(
        r: &LongReport,
        log: &BeatLog,
        whole: &crate::session::Measurement,
        lc: &LongConfig,
    ) {
        use crate::measure::Caveat;
        assert_eq!(
            r.no_watch(),
            crate::measure::no_watch_reason_in(&whole.caveats)
        );
        assert!(r.no_watch().is_some() && r.rate_withheld());
        assert_eq!(crate::measure::off_standard(&r.caveats), None);
        assert!(r.overall.is_none() && r.drop_beat_error_ms().is_none());
        assert_eq!(r.beat_error_unlock_ms, None);
        assert_eq!(
            (r.amplitude_deg, r.rate_p05, r.rate_p95),
            (None, None, None)
        );
        assert_eq!((r.amplitude_p05, r.amplitude_p95), (None, None));
        assert!(r.slices.iter().all(|s| s.rate_s_per_day.is_none()
            && s.beat_error_ms.is_none()
            && s.beat_error_unlock_ms.is_none()
            && s.amplitude_deg.is_none()));
        assert!(r.rate_components.is_empty());
        assert!(r.timing.components.is_empty() && r.amplitude.components.is_empty());
        for g in [
            &r.timing.grid,
            &r.timing.detrended,
            &r.amplitude.grid,
            &r.amplitude.detrended,
        ] {
            assert!(g.y.iter().all(|v| v.is_nan()));
        }
        assert!(!r.caveats.iter().any(|c| matches!(
            c,
            Caveat::BeatErrorAsSize | Caveat::CoaxialEstimate | Caveat::CoaxialBeatError { .. }
        )));
        let text = r.no_rate_text(None).expect("the sentence");
        assert!(
            text.starts_with("No regular beat found")
                && text.ends_with(crate::session::NO_READINGS),
            "{text}"
        );
        let st = crate::steadiness::check(log, None, r, lc, &Default::default());
        assert!(st.series.iter().all(|s| s.readings == 0), "{:?}", st.series);
        // No two-state samples either: none of the rate's windows,
        // nor of the amplitude's.
        for s in &st.series[..2] {
            assert_eq!(
                s.two_state.as_ref().map_or(0, |t| t.blocks),
                0,
                "{:?}",
                s.series
            );
        }
        assert!(st.beat_error_source.is_none() && st.wind_down.is_none());
        assert!(!st
            .caveats
            .iter()
            .any(|c| matches!(c, Caveat::BeatErrorFromDrop { .. })));
        // Every grid read beside the report takes its verdict.
        let grids = crate::steady_views::check_grids(log, None, r, 10.0);
        assert!(grids.iter().all(|g| g.y.iter().all(|v| v.is_nan())));
    }

    #[test]
    fn a_reading_says_when_usb_lines_and_a_good_log_disagree() {
        // A minute of a watch, on a take whose clock log measures the card
        // 20 ppm slow, entry by entry exactly: USB lines 100 ppm off that
        // put the caveat, with both figures, on the session reading and the
        // long run (and so `series`); lines within 3 ppm put none.
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
                    ..
                } => Some((usb_ppm, log_ppm, log_ppm_sd)),
                _ => None,
            })
        };
        let (reading, long) = caveats(-120.0);
        for c in [&reading, &long] {
            let (usb, log_ppm, sd) = disagree(c).unwrap_or_else(|| panic!("{c:?}"));
            assert_eq!(usb, -120.0);
            assert!((log_ppm + 20.0).abs() < 0.01, "{log_ppm}");
            assert!(sd < 0.01, "{sd}");
        }
        let json = serde_json::to_value(&reading).unwrap();
        assert!(
            json.as_array()
                .unwrap()
                .iter()
                .any(|c| c["code"] == "clock_sources_disagree" && c["usb_ppm"] == -120.0),
            "{json}"
        );
        let (reading, long) = caveats(-23.0);
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

    /// Only good beats count as clean, and only they go in the timing
    /// series: a bad beat 3 ms off moves neither.
    #[test]
    fn bad_beats_are_neither_clean_nor_timed() {
        let duration = 600.0;
        let beats: Vec<Beat> = (0..(duration * 8.0) as i64)
            .map(|index| {
                let bad = index % 3 != 0;
                Beat {
                    time: index as f64 / 8.0 + if bad { 0.003 } else { 0.0 },
                    index,
                    quality: if bad { 0.0 } else { 1.0 },
                }
            })
            .collect();
        let log = BeatLog {
            lock_template_kept: false,
            sample_rate: 48000,
            duration_s: duration,
            bph: 28800,
            lift_deg: 52.0,
            escapement: Default::default(),
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
        assert!(
            (r.clean_fraction - 1.0 / 3.0).abs() < 0.01,
            "{}",
            r.clean_fraction
        );
        let worst = r
            .timing
            .grid
            .y
            .iter()
            .filter(|v| v.is_finite())
            .fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(worst < 1e-4, "{worst}");
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
            lock_template_kept: false,
            sample_rate: 48000,
            duration_s: duration,
            bph: 21600,
            lift_deg: 52.0,
            escapement: Default::default(),
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

    /// A window that reaches across a break is culled from the long run's
    /// amplitude, never kept, as Steadiness and every reading leave it
    /// out (#636): the window over a 30 s pause at 201 s, read 999°, moves
    /// no cell of the grid.
    #[test]
    fn a_window_across_a_break_is_left_out_of_the_amplitude() {
        let beats: Vec<Beat> = (0..2000)
            .map(|i| Beat {
                time: i as f64 * 0.2 + 0.05,
                index: i,
                quality: 1.0,
            })
            .collect();
        let amplitude_windows = (0..200)
            .map(|k| {
                let start_s = k as f64 * 2.0;
                let deg = if start_s == 200.0 { 999.0 } else { 250.0 };
                crate::amplitude::AmplitudeWindow {
                    start_s,
                    end_s: start_s + 2.0,
                    even_deg: Some(deg),
                    odd_deg: Some(deg),
                    beat_error_ms: None,
                    beat_error_unlock_ms: Some(0.3),
                    even_unlock_ms: None,
                    odd_unlock_ms: None,
                    even_drop_ms: None,
                    odd_drop_ms: None,
                    extra: None,
                    sound_1: [f32::NAN; 2],
                }
            })
            .collect();
        let log = BeatLog {
            lock_template_kept: false,
            sample_rate: 48000,
            duration_s: 400.0,
            bph: 18000,
            lift_deg: 52.0,
            escapement: Default::default(),
            beats,
            amplitude_windows,
            clipped_beats: vec![],
            clipped_samples: 0,
            jumps: vec![],
            sided: vec![],
            by_count: vec![],
            snippets: Vec::new(),
        };
        let breaks = [Break {
            audio_s: 201.0,
            gap_s: 30.0,
        }];
        let r = analyse_with_breaks(&log, None, &breaks, &LongConfig::default());
        let read: Vec<f64> = r
            .amplitude
            .grid
            .y
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .collect();
        assert!(read.len() > 150, "{}", read.len());
        assert!(read.iter().all(|&v| v == 250.0), "{read:?}");
        let sliced: Vec<f64> = r.slices.iter().filter_map(|s| s.amplitude_deg).collect();
        assert!(sliced.iter().all(|&v| v == 250.0), "{sliced:?}");
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
                    let be = if flip && start_s >= 200.0 { -0.3 } else { 0.3 };
                    crate::amplitude::AmplitudeWindow {
                        start_s,
                        end_s: start_s + 2.0,
                        even_deg: Some(250.0),
                        odd_deg: Some(250.0),
                        beat_error_ms: None,
                        beat_error_unlock_ms: Some(be),
                        even_unlock_ms: None,
                        odd_unlock_ms: None,
                        even_drop_ms: None,
                        odd_drop_ms: None,
                        extra: None,
                        sound_1: [f32::NAN; 2],
                    }
                })
                .collect();
            BeatLog {
                lock_template_kept: false,
                sample_rate: 48000,
                duration_s: 400.0,
                bph: 18000,
                lift_deg: 52.0,
                escapement: Default::default(),
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

    #[test]
    fn the_timing_grid_coarsens_at_12_and_80_hours() {
        // The step behind the period search's 2 s and 4 s floors: 0.25 s
        // up to 12 hours, 0.5 s up to 80, then 4 s.
        let h = 3600.0;
        for (span, step) in [
            (11.9, 0.25),
            (12.0, 0.25),
            (12.1, 0.5),
            (48.1, 0.5),
            (80.0, 0.5),
            (80.1, 4.0),
        ] {
            assert_eq!(timing_step(span * h), step, "{span} h");
        }
    }

    #[test]
    fn a_cycle_under_30_s_is_sized_as_a_timing_swing() {
        // Below three 10 s rate readings a cycle is told in ms, its rate
        // swing being the slope of a fraction of a millisecond.
        let at = |period_s: f64| RateComponent {
            component: Component {
                period_s,
                resolution_s: 0.1,
                significance: 3.0,
                beyond_simulated: false,
                harmonics: 1,
                explained: 0.5,
                peak_to_peak: 0.0004,
                wheel: None,
                nearest_wheel: None,
                shape: Vec::new(),
                fold: longterm::Fold {
                    period_s,
                    profile: Vec::new(),
                    counts: Vec::new(),
                    raster: Vec::new(),
                    t0: 0.0,
                },
                harmonic_of: None,
            },
            rate_shape: Vec::new(),
            rate_swing_s_per_day: 40.0,
            timing_swing_ms: 0.4,
        };
        for (period, size) in [
            (5.0, (0.4, "ms")),
            (29.9, (0.4, "ms")),
            (30.0, (40.0, "s/d")),
            (60.0, (40.0, "s/d")),
        ] {
            assert_eq!(at(period).size(), size, "{period} s");
        }
    }
}

#[cfg(test)]
#[path = "longrun_verdict_tests.rs"]
mod verdict_tests;
