//! The Steadiness pane: how the rate, the amplitude and the beat error
//! each change over the session, from the same tests as
//! `timegrapher series` (core `steadiness`): what was detected, its
//! statistics and the views behind them.
//!
//! The tests need the readings in time order over minutes to hours, so
//! they run over the whole session in the background, again every minute
//! while it grows, and the pane shows the latest answer.

use crate::strip;
use crate::theme;
use eframe::egui::{self, Color32, RichText, Vec2};
use egui_plot::{
    GridInput, GridMark, HLine, Line, LineStyle, Plot, PlotPoint, Points, Polygon, Text, VLine,
};
use serde::{Deserialize, Serialize};
use timegrapher_core::live::LiveAnalyzer;
use timegrapher_core::longrun;
use timegrapher_core::longterm::{Fold, Grid, LongConfig, Search};
use timegrapher_core::period_ticks::{period_tick_label, period_ticks};
use timegrapher_core::periodicity::{default_wheels, reference_lines, Wheel};
use timegrapher_core::steadiness::{self, Config, Report, SeriesCheck, SeriesKind};
use timegrapher_core::steady_views::{self, FoldFrom};
use timegrapher_core::stream::BeatLog;

/// Seconds of beats the tests need before they can say anything: 30 rate
/// readings of 10 s.
pub const MIN_S: f64 = 300.0;
/// How much the session grows before the tests run again, seconds.
pub const RERUN_S: f64 = 60.0;
/// Most readings drawn on the Readings view per series.
const MAX_DRAWN: usize = 3000;
/// Shortest row, and narrowest column, in which a series' plot stays
/// readable, points.
const MIN_ROW: f32 = 120.0;
const MIN_COLUMN: f32 = 260.0;

/// What a Steadiness pane draws under each series: each view is a pane of
/// its own.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum View {
    #[default]
    Readings,
    Periods,
    Cycle,
    Autocorrelation,
    Allan,
    Cusum,
    Fft,
    Stft,
    Wavelet,
}

impl View {
    /// The STFT and Wavelet Maps: one map at a time, the choice shared.
    pub fn is_map(self) -> bool {
        matches!(self, View::Stft | View::Wavelet)
    }

    #[cfg(test)]
    const ALL: [View; 9] = [
        View::Readings,
        View::Periods,
        View::Cycle,
        View::Autocorrelation,
        View::Allan,
        View::Cusum,
        View::Fft,
        View::Stft,
        View::Wavelet,
    ];

    pub fn label(self) -> &'static str {
        match self {
            View::Readings => "Readings",
            View::Periods => "Periods",
            View::Cycle => "Cycle",
            View::Autocorrelation => "Autocorrelation",
            View::Allan => "Allan Deviation",
            View::Cusum => "CUSUM",
            View::Fft => "FFT",
            View::Stft => "STFT Map",
            View::Wavelet => "Wavelet Map",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            View::Readings => {
                "Every reading in time order, with the stretches of constant level the \
                 change finder split them into, and the two levels when there are two states"
            }
            View::Periods => {
                "How strongly the readings repeat at each period, from the period search: \
                 the horizontal dashed line is the score a period needs for the search to list it, and \
                 the strongest line it lists is labelled with its period and with the wheel \
                 whose turn it matches, when it matches one; the headline names any cycle \
                 the tests found"
            }
            View::Cycle => {
                "The readings folded at the cycle the tests found: every cycle's readings as \
                 dots, coloured by when the cycle came (purple first, yellow-green last), a box \
                 round the middle half of each phase's readings, and their median shape as \
                 the line; the note gives the line's swing beside a box's median height"
            }
            View::Autocorrelation => {
                "How much each reading resembles the one a lag later: independent readings \
                 stay inside the dashed band; a rise back to a peak means they repeat"
            }
            View::Allan => {
                "How much the rate's average scatters when averaged over longer and longer \
                 times; lower is steadier. On or below the dashed line, the rate only jitters \
                 from moment to moment; above it towards the right, it wanders or drifts, and a \
                 cycle puts a bump near 0.37 of its period. Both axes are logarithmic"
            }
            View::Cusum => {
                "The running sum of each reading's distance from the mean: independent \
                 readings stay inside the dashed lines; a step in the level bends it into a \
                 V or a tent, with the corner at the step"
            }
            View::Fft => {
                "The plain amplitude spectrum of each series, like Witschi's FFT: the size \
                 estimated at each period for complete data, the rate's as the swing peak to peak it \
                 makes in 10 s rate readings (or the beat timing's, in ms, with Timing), \
                 with the wheels' periods marked and twice a turn dashed. Both axes are \
                 logarithmic"
            }
            View::Stft => {
                "Each series' size at each period (up, on a log scale) and time of the \
                 session (across), from the amplitude spectrum of each 20 minutes of \
                 readings, stepped 5 minutes: every period is read over the same window, so \
                 this map gives a line's size and shows a line that comes and goes. The \
                 rate's map is of the beat timing, in ms. Empty where a window has a \
                 reading missing"
            }
            View::Wavelet => {
                "Each series' size at each period (up, on a log scale) and time of the \
                 session (across), from a synchrosqueezed wavelet transform: short periods \
                 read over short times and long ones over long, each moved to the period its \
                 own phase turns at, so a line shows sharply where and when it is; under \
                 noise one row reads under the line's size, so read its size from the STFT \
                 Map. Each unbroken stretch is read alone, and its ends, which a period's \
                 wavelet can't reach, are left empty"
            }
        }
    }
}

/// The tests' answer over a session, with what the views draw.
pub struct Steadiness {
    pub report: Report,
    /// Rate, amplitude and beat error readings on their grids.
    grids: [Grid; 3],
    /// The period search behind each series.
    searches: [Option<Search>; 3],
    /// Each series folded at the cycle it was found to have, else at
    /// another series' cycle or a wheel's turn, so its Cycle view still
    /// shows whether that repeats in it.
    folds: [Option<Fold>; 3],
    /// Where each fold's period came from.
    fold_from: [FoldFrom; 3],
    wheels: Vec<Wheel>,
    /// The periods every series' search covers between them, log10 seconds,
    /// so the three Periods plots share one time axis.
    period_span: Option<(f64, f64)>,
    /// The STFT and wavelet maps of each series (core `timefreq`).
    maps: Vec<timegrapher_core::timefreq::TimeFrequency>,
    /// Seconds of the session the tests covered.
    pub upto_s: f64,
}

/// The session's beats and amplitude windows so far, as a recording's
/// analysis would give them.
pub fn log_of(live: &LiveAnalyzer) -> Option<BeatLog> {
    let masked = live.masked();
    let mut log = BeatLog {
        sample_rate: live.sample_rate(),
        duration_s: live.duration_s(),
        bph: live.bph()?,
        lift_deg: live.config().analysis.amplitude.lift_deg,
        beats: live.beats().to_vec(),
        amplitude_windows: live.amplitude_windows().to_vec(),
        clipped_beats: live.clipped_beats().to_vec(),
        // Not counted live; the tests don't use it.
        clipped_samples: 0,
        // Imported jumps, current position/settling boundaries and both
        // ends of each masked stretch are all known cuts, not extra
        // elapsed time or newly detected jumps.
        jumps: live.breaks().to_vec(),
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    // Stretches masked by hand are left out, cut at both ends: no reading
    // reaches across one, and none takes in a ragged edge of beats beside
    // one (a 10 s reading over the end of a mask would have only the beats
    // after it).
    timegrapher_core::session::mask_log(&mut log, masked);
    log.jumps.extend(
        timegrapher_core::clean::mask_breaks(masked)
            .iter()
            .map(|b| b.audio_s),
    );
    log.jumps.sort_by(f64::total_cmp);
    log.jumps.dedup();
    Some(log)
}

/// Room left between two stretches in one position when they are joined
/// for the tests, seconds: longer than a gap the readings fit across.
pub const JOIN_S: f64 = 5.0;

/// The length of the stretches `spans` (start, end; the last may run on)
/// once joined end to end, up to `end_s`, seconds.
pub fn joined_duration(spans: &[(f64, f64)], end_s: f64) -> f64 {
    let lens: Vec<f64> = spans
        .iter()
        .map(|&(a, b)| b.min(end_s) - a.max(0.0))
        .filter(|&l| l > 0.0)
        .collect();
    lens.iter().sum::<f64>() + JOIN_S * lens.len().saturating_sub(1) as f64
}

/// The part of `log` inside `spans`, in time order, with the time between
/// them taken out: one position's beats over a session that moved the
/// watch, joined so the tests see them as one run, with a short gap at each
/// join that no reading fits across.
pub fn join(log: &BeatLog, spans: &[(f64, f64)]) -> BeatLog {
    let beat = 3600.0 / log.bph as f64;
    let mut out = BeatLog {
        sample_rate: log.sample_rate,
        duration_s: 0.0,
        bph: log.bph,
        lift_deg: log.lift_deg,
        beats: Vec::new(),
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: log.clipped_samples,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    // The last kept span's end and last beat (time, number), joined.
    let mut prev: Option<(f64, f64, i64)> = None;
    for &(a, b) in spans {
        let b = b.min(log.duration_s);
        let lo = log.beats.partition_point(|x| x.time < a);
        let hi = log.beats.partition_point(|x| x.time < b);
        let Some(first) = log.beats.get(lo).filter(|_| lo < hi) else {
            continue;
        };
        // Taken off this span's times and beat numbers: it starts JOIN_S
        // after the last one ended, its beats numbered on by the beats that
        // would fit between.
        let (shift_t, shift_i) = match prev {
            None => (a.max(0.0), first.index),
            Some((end, last_t, last_i)) => {
                let shift_t = a - (end + JOIN_S);
                let n = ((first.time - shift_t - last_t) / beat).round().max(1.0) as i64;
                (shift_t, first.index - (last_i + n))
            }
        };
        if prev.is_some() {
            out.jumps.push(a - shift_t);
        }
        out.jumps.extend(
            log.jumps
                .iter()
                .filter(|&&t| t >= a && t < b)
                .map(|t| t - shift_t),
        );
        out.beats.extend(
            log.beats[lo..hi]
                .iter()
                .map(|x| timegrapher_core::beats::Beat {
                    time: x.time - shift_t,
                    index: x.index - shift_i,
                    ..*x
                }),
        );
        out.amplitude_windows.extend(
            log.amplitude_windows
                .iter()
                .filter(|w| w.start_s >= a && w.end_s <= b)
                .map(|w| {
                    let mut w = w.clone();
                    w.start_s -= shift_t;
                    w.end_s -= shift_t;
                    // Beat indices changed sides; keep the existing side
                    // measurements and signed errors on those same beats.
                    if shift_i % 2 != 0 {
                        w.swap_sides();
                    }
                    w
                }),
        );
        out.clipped_beats.extend(
            log.clipped_beats
                .iter()
                .filter(|&&t| t >= a && t < b)
                .map(|t| t - shift_t),
        );
        let last = out.beats.last().unwrap();
        out.duration_s = b - shift_t;
        prev = Some((out.duration_s, last.time, last.index));
    }
    out.jumps.sort_by(f64::total_cmp);
    out.jumps.dedup();
    out
}

/// Run the tests over a session. Slow for long sessions (a 2 h take takes
/// some seconds), so it is run off the window's thread.
/// The tests over `log`, naming cycles after `wheels` (a picked
/// calibre's train) or, without them, the wheels most calibres at the beat
/// rate share, with both usual escape wheels (15 and 20 teeth).
#[cfg(test)]
pub fn compute(log: &BeatLog, wheels: Option<Vec<Wheel>>) -> Steadiness {
    compute_with(log, wheels, &std::sync::atomic::AtomicU8::new(0))
}

/// What a run of [`compute_with`] is doing, from the step it last stored.
pub fn stage_text(stage: u8) -> &'static str {
    match stage {
        0 | 1 => "Searching the readings for cycles",
        2 => "Testing the rate, amplitude and beat error",
        _ => "Drawing the results",
    }
}

/// [`compute`], storing each step as it starts in `stage` (see
/// [`stage_text`]) so the panes can say what the tests are doing.
pub fn compute_with(
    log: &BeatLog,
    wheels: Option<Vec<Wheel>>,
    stage: &std::sync::atomic::AtomicU8,
) -> Steadiness {
    use std::sync::atomic::Ordering::Relaxed;
    stage.store(1, Relaxed);
    let lc = LongConfig {
        wheels: wheels.unwrap_or_else(|| default_wheels(log.bph)),
        ..Default::default()
    };
    let cfg = Config::default();
    // These beats are already indexed on their derived timeline. Zero-gap
    // cuts split fits without adding time or applying side-alignment heuristics.
    let breaks: Vec<_> = log
        .jumps
        .iter()
        .map(|&audio_s| timegrapher_core::breaks::Break {
            audio_s,
            gap_s: 0.0,
        })
        .collect();
    let long = longrun::analyse_with_breaks(log, None, &breaks, &lc);
    stage.store(2, Relaxed);
    let report = steadiness::check(log, None, &long, &lc, &cfg);
    stage.store(3, Relaxed);
    // The tests' own grids: a rate reading over a dropout or a jump is left
    // out, and the beat error is given as sizes when its sign turns over
    // across a jump, as the tests beside them and the take's figures give it.
    let grids = steady_views::grids_with_breaks(log, cfg.rate_reading_s, &breaks);
    let searches = steady_views::searches(&grids, &long, &lc);
    let mut fold_from = [FoldFrom::Own, FoldFrom::Own, FoldFrom::Own];
    let folds = steady_views::folds(&grids, &report, &lc);
    let folds = std::array::from_fn(|i| {
        let (fold, from) = folds[i].clone()?;
        fold_from[i] = from;
        Some(fold)
    });
    let period_span = steady_views::period_span(&searches);
    // With the same (no) clock the long-run analysis above was given.
    let maps = timegrapher_core::timefreq::for_take(log, None, &long, &lc);
    Steadiness {
        report,
        grids,
        searches,
        folds,
        fold_from,
        period_span,
        wheels: lc.wheels,
        maps,
        upto_s: log.duration_s,
    }
}

/// The statistics behind a series' headline, on one line: readings,
/// median, spread, scatter from one reading to the next, lag-1
/// correlation and Ljung–Box p, and the trend, period and cycle when
/// found.
fn stats_line(c: &SeriesCheck) -> String {
    let u = c.unit;
    // A true minus sign, as in the readings.
    let n = |v: f64| format!("{v:.2}").replace('-', "−");
    let signed = |v: f64| format!("{v:+.2}").replace('-', "−");
    let mut parts = vec![format!("Tested on {}", counted(c.readings, c.step_s))];
    if c.outliers > 0 {
        parts.push(format!(
            "{} left out as more than 6 σ from their 5-minute level",
            c.outliers
        ));
    }
    if let Some(m) = c.median {
        parts.push(format!("median {} {u}", n(m)));
    }
    if let Some(sd) = c.sd {
        parts.push(format!("SD {} {u}", n(sd)));
    }
    if let Some(st) = c.short_term_sd {
        parts.push(format!("±{} {u} reading to reading", n(st)));
    }
    if let Some(a) = &c.autocorrelation {
        let lag1 = a.lag_s.iter().zip(&a.r).find(|(l, _)| **l > 0.0);
        if let Some((_, r1)) = lag1 {
            parts.push(format!("lag-1 r {}", n(*r1)));
        }
        parts.push(format!("Ljung–Box p {}", steadiness::p_text(a.p_value)));
    }
    if let Some(t) = &c.trend {
        parts.push(format!("trend {} {u}/h", signed(t.per_hour)));
    }
    if let Some(p) = &c.period {
        parts.push(format!("period {:.0} s", p.period_s));
    }
    parts.join(" · ")
}

/// What a count of readings `step_s` long counts: "179 10 s readings";
/// readings shorter than 10 s are windows, "880 2 s windows".
fn counted(n: usize, step_s: f64) -> String {
    format!("{n} {}", kind_of(step_s))
}

/// Readings `step_s` long: "10 s readings"; shorter than 10 s, "2 s
/// windows".
fn kind_of(step_s: f64) -> String {
    let what = if step_s >= 10.0 - 1e-9 {
        "readings"
    } else {
        "windows"
    };
    format!("{} {what}", secs(step_s))
}

fn series_colour(k: SeriesKind, pal: &theme::Palette) -> Color32 {
    match k {
        SeriesKind::Rate => pal.trace_rate,
        SeriesKind::Amplitude => pal.trace_amplitude,
        SeriesKind::BeatError => pal.trace_beat_error,
    }
}

/// A reading's name, as its series, chart, distribution and switch show it.
pub fn title(k: SeriesKind) -> &'static str {
    match k {
        SeriesKind::Rate => "Rate",
        SeriesKind::Amplitude => "Amplitude",
        SeriesKind::BeatError => "Beat Error",
    }
}

/// A value in a series' units for an axis or a note.
fn value_text(k: SeriesKind, v: f64, step: f64) -> String {
    let d = if step >= 1.0 {
        0
    } else if step >= 0.1 {
        1
    } else if step >= 0.01 {
        2
    } else {
        3
    };
    let v = if v.abs() < step * 1e-6 { 0.0 } else { v };
    match k {
        SeriesKind::Rate => format!("{v:.d$} s/d"),
        SeriesKind::Amplitude => format!("{v:.d$}°"),
        SeriesKind::BeatError => format!("{v:.d$} ms"),
    }
}

/// The tallest point that stands above both its neighbours, so a
/// spectrum still climbing at either end of its axis (the rate's slow
/// wander building up in the timing) is not labelled as a peak.
fn tallest_peak(pts: &[[f64; 2]]) -> Option<[f64; 2]> {
    let mut by_x = pts.to_vec();
    by_x.sort_by(|a, b| a[0].total_cmp(&b[0]));
    by_x.windows(3)
        .filter(|w| w[1][1] > w[0][1] && w[1][1] > w[2][1])
        .map(|w| w[1])
        .max_by(|a, b| a[1].total_cmp(&b[1]))
}

/// Seconds as a short time: "45 s", "12 min", "2.5 h".
fn secs(s: f64) -> String {
    if s < 120.0 {
        format!("{s:.0} s")
    } else if s < 7200.0 {
        format!("{:.0} min", s / 60.0)
    } else {
        format!("{:.1} h", s / 3600.0)
    }
}

/// A time on a linear axis: "30 s" under a minute, then "1:30", so ticks
/// every 30 s don't round to the same whole minute.
fn lag_text(s: f64) -> String {
    if s.abs() < 59.5 {
        format!("{s:.0} s")
    } else {
        strip::fmt_time(s)
    }
}

/// A wheel's name in Title Case.
/// The wheel to name beside a fold at `period_s`: that of the finding
/// (period, wheel) folded at, never one of another period's.
fn fold_wheel<'a>(found: &[(f64, Option<&'a str>)], period_s: f64) -> Option<&'a str> {
    found
        .iter()
        .filter(|(p, _)| (p - period_s).abs() <= 0.01 * period_s)
        .find_map(|(_, w)| *w)
}

fn wheel_title(name: &str) -> String {
    name.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A number to about three significant figures, for a logarithmic axis.
fn sig(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return "0".into();
    }
    let d = (2 - v.abs().log10().floor() as i32).max(0) as usize;
    let s = format!("{v:.d$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// Grid lines for a period axis drawn in log10 seconds, `width` points
/// wide: at the times core's `period_ticks` names, as many as have room.
pub fn period_grid(width: f32) -> impl Fn(GridInput) -> Vec<GridMark> {
    let room = (width / 64.0).max(2.0) as usize;
    move |input| {
        let (lo, hi) = input.bounds;
        period_ticks(10f64.powf(lo), 10f64.powf(hi), room)
            .into_iter()
            .map(|t| GridMark {
                value: t.period_s.log10(),
                step_size: [1.0, 0.5, 0.25][usize::from(t.level.min(2))],
            })
            .collect()
    }
}

/// The label of a period axis' mark at `log10_s`, as core names it.
pub fn period_label(log10_s: f64) -> String {
    period_tick_label(10f64.powf(log10_s))
        .unwrap_or_default()
        .to_string()
}

/// Grid lines for an axis drawn in log10 of its values: every decade, and
/// 2 and 5 within each when fewer than three decades show.
fn log_grid(input: GridInput) -> Vec<GridMark> {
    let (lo, hi) = input.bounds;
    let fine = hi - lo < 3.0;
    let mut marks = Vec::new();
    for k in (lo.floor() as i32 - 1)..=(hi.ceil() as i32) {
        for (m, step) in [(1.0f64, 1.0), (2.0, 0.3), (5.0, 0.3)] {
            if !fine && m != 1.0 {
                continue;
            }
            let v = k as f64 + m.log10();
            if v >= lo && v <= hi {
                marks.push(GridMark {
                    value: v,
                    step_size: step,
                });
            }
        }
    }
    marks
}

/// The series the pane can show, in the order the switches list them.
pub const SERIES: [SeriesKind; 3] = [
    SeriesKind::Rate,
    SeriesKind::Amplitude,
    SeriesKind::BeatError,
];

/// The icon for each reading: a hare for the rate, a swing for the
/// amplitude and a seesaw for the beat error, wherever the app uses one.
pub fn series_icon(k: SeriesKind) -> theme::Icon {
    match k {
        SeriesKind::Rate => theme::Icon::Rate,
        SeriesKind::Amplitude => theme::Icon::Amplitude,
        SeriesKind::BeatError => theme::Icon::BeatError,
    }
}

/// The switches for which series the pane shows, for its toolbar: the
/// readings' icons, each tinted in its colour while shown. At least one
/// stays on.
pub fn series_switches(ui: &mut egui::Ui, shown: &mut [bool; 3]) {
    let pal = theme::pal(ui);
    let hint = |k| {
        format!(
            "Show the {} series. Turning the others off gives this one the whole pane.",
            title(k).to_lowercase()
        )
    };
    for k in SERIES {
        theme::note_icon(ui, series_icon(k), title(k));
    }
    theme::bar_item(ui, "Series", |ui| {
        for (i, k) in SERIES.into_iter().enumerate() {
            let flip = if theme::in_toolbar(ui) {
                theme::icon_toggle(ui, series_icon(k), shown[i], series_colour(k, pal))
                    .on_hover_text(format!("{}: {}", title(k), hint(k)))
                    .clicked()
            } else {
                // Under "…" or in a list: a switch for each, in words.
                let mut on = shown[i];
                theme::switch_row(ui, title(k), &mut on, &hint(k))
            };
            if flip {
                flip_series(shown, i);
            }
        }
    });
}

/// One series of `shown`: the first one on, else Rate.
pub fn one_series(shown: [bool; 3]) -> [bool; 3] {
    let i = shown.iter().position(|&on| on).unwrap_or(0);
    let mut one = [false; 3];
    one[i] = true;
    one
}

/// The two map panes' choice of the one map shown: the same three icons
/// as elsewhere, joined, one picked at a time, so the map has the pane.
pub fn series_choice(ui: &mut egui::Ui, shown: &mut [bool; 3]) {
    *shown = one_series(*shown);
    let pal = theme::pal(ui);
    let hint = |k| {
        format!(
            "Show the {} map. One map is shown at a time, so it has room.",
            title(k)
        )
    };
    for k in SERIES {
        theme::note_icon(ui, series_icon(k), title(k));
    }
    theme::bar_item(ui, "Series", |ui| {
        if theme::in_toolbar(ui) {
            egui::Frame::new()
                .fill(pal.control)
                .corner_radius(egui::CornerRadius::same(theme::CONTROL_RADIUS + 1))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (i, k) in SERIES.into_iter().enumerate() {
                        if theme::icon_toggle(ui, series_icon(k), shown[i], series_colour(k, pal))
                            .on_hover_text(format!("{}: {}", title(k), hint(k)))
                            .clicked()
                        {
                            *shown = [false; 3];
                            shown[i] = true;
                        }
                    }
                });
        } else {
            let mut i = shown.iter().position(|&on| on).unwrap_or(0);
            let words: Vec<(usize, &str, &str)> = SERIES
                .into_iter()
                .enumerate()
                .map(|(i, k)| (i, title(k), ""))
                .collect();
            theme::row(
                ui,
                "Map",
                Some("One map is shown at a time, so it has room."),
                |ui| theme::segmented(ui, &mut i, &words),
            );
            *shown = [false; 3];
            shown[i] = true;
        }
    });
}

/// Turn series `i` on or off, unless it is the last one on.
pub fn flip_series(shown: &mut [bool; 3], i: usize) {
    let others = shown.iter().enumerate().any(|(j, &on)| j != i && on);
    if !shown[i] || others {
        shown[i] = !shown[i];
    }
}

/// Narrower than this, the Info panel opens over the plots.
const INFO_BESIDE_W: f32 = 600.0;
/// The Info panel's width, or a third of the pane if that is less.
const INFO_W: f32 = 300.0;

/// Draw the pane: a row for each series shown with the chosen view, and,
/// when `opts.info`, the Info panel at the right with what the view shows,
/// each series' findings and statistics in full, and what was tested.
/// Before there is an answer, `status` says what the tests wait for.
pub fn draw(
    ui: &mut egui::Ui,
    s: Option<&Steadiness>,
    status: &str,
    view: View,
    shown: &[bool; 3],
    opts: Opts,
) {
    let pal = theme::pal(ui);
    let Some(s) = s else {
        if !status.is_empty() {
            ui.label(RichText::new(status).small().color(pal.text_secondary));
        }
        return;
    };
    let series: Vec<(usize, &SeriesCheck)> = s
        .report
        .series
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            SERIES
                .iter()
                .position(|&k| k == c.series)
                .is_none_or(|j| shown[j])
        })
        .collect();
    let full = ui.available_rect_before_wrap();
    let beside = full.width() >= INFO_BESIDE_W;
    if opts.info && beside {
        let w = INFO_W.min(full.width() / 3.0);
        let gap = 12.0;
        let plots = full.with_max_x(full.right() - w - gap);
        // Only the part of the pane on screen, so the panel scrolls rather
        // than running off the bottom.
        let panel = full.with_min_x(full.right() - w).intersect(ui.clip_rect());
        ui.painter().vline(
            panel.left() - gap / 2.0,
            panel.y_range(),
            egui::Stroke::new(1.0_f32, pal.separator),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| {
            info_panel(ui, &series, status, view, panel.height());
        });
        ui.scope_builder(egui::UiBuilder::new().max_rect(plots), |ui| {
            rows(ui, s, &series, view, opts);
        });
        ui.allocate_rect(full, egui::Sense::hover());
    } else {
        rows(ui, s, &series, view, opts);
        if opts.info {
            // Over the plots, from the pane's top right corner.
            let w = INFO_W.min(full.width() - 16.0).max(120.0);
            let seen = full.intersect(ui.clip_rect());
            egui::Area::new(ui.id().with("steady info over"))
                .order(egui::Order::Foreground)
                .fixed_pos(egui::pos2(full.right() - w - 4.0, seen.top() + 4.0))
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_width(w - 16.0);
                        let h = (seen.height() - 24.0).max(80.0);
                        ui.set_max_height(h);
                        info_panel(ui, &series, status, view, h);
                    });
                });
        }
    }
}

/// The Info panel: what the view shows, then a block for each series
/// shown under its coloured title (its findings and statistics in full),
/// then what was tested. It scrolls, and its words can be selected.
fn info_panel(
    ui: &mut egui::Ui,
    series: &[(usize, &SeriesCheck)],
    status: &str,
    view: View,
    max_h: f32,
) {
    let pal = theme::pal(ui);
    // Each view's panel its own, opening at its top: from the top again
    // when it was not drawn the pass before.
    let id = egui::Id::new(("steady info", view));
    let pass = ui.ctx().cumulative_pass_nr();
    let last = ui.ctx().data_mut(|d| {
        let last = d.get_temp::<u64>(id);
        d.insert_temp(id, pass);
        last
    });
    let mut area = egui::ScrollArea::vertical()
        .id_salt(id)
        // No taller than `max_h`, the room on screen, with its bar always
        // showing so what is below the edge is not missed.
        .max_height(max_h)
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible);
    if last.is_none_or(|l| l + 1 < pass) {
        area = area.vertical_scroll_offset(0.0);
    }
    area.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        let text = |t: &str, c: Color32| {
            egui::Label::new(RichText::new(t).small().color(c))
                .wrap()
                .selectable(true)
        };
        ui.label(theme::caption(view.label()).color(pal.text_secondary));
        ui.add(text(&format!("{}.", view.hint()), pal.text_secondary));
        for &(_, c) in series {
            ui.add_space(8.0);
            ui.label(theme::caption(title(c.series)).color(pal.ink(series_colour(c.series, pal))));
            ui.add(text(&c.headline, pal.text));
            ui.add(text(&stats_line(c), pal.text_secondary));
        }
        if !status.is_empty() {
            ui.add_space(8.0);
            ui.add(text(status, pal.text_tertiary));
        }
    });
}

/// Each series' row with the view's plot: one above another, filling the
/// pane, or side by side when it is wide and short.
fn rows(
    ui: &mut egui::Ui,
    s: &Steadiness,
    series: &[(usize, &SeriesCheck)],
    view: View,
    opts: Opts,
) {
    let n = series.len().max(1);
    let gap = 10.0;
    let (w, h) = (ui.available_width(), ui.available_height());
    // Each gap also takes the layout's spacing either side of it.
    let step_gap = gap + 2.0 * ui.spacing().item_spacing.y;
    let fits = |count: usize, room: f32| (room - step_gap * (count as f32 - 1.0)) / count as f32;
    if fits(n, h) - 4.0 >= MIN_ROW || fits(n, w) < MIN_COLUMN {
        // One above another, filling the pane, or scrolling when it is too
        // short for every row to be readable.
        // A little short of the room, so the last axis isn't clipped.
        let row_h = (fits(n, h) - 4.0).max(MIN_ROW);
        // The bar always shows, so a row below the pane's edge is not
        // missed.
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
                for (k, &(i, c)) in series.iter().enumerate() {
                    ui.allocate_ui(Vec2::new(ui.available_width(), row_h), |ui| {
                        ui.set_min_height(row_h);
                        row(ui, s, i, c, view, opts);
                    });
                    if k + 1 < n {
                        ui.add_space(gap);
                    }
                }
            });
    } else {
        // Wide and short: side by side.
        let col_w = fits(n, w);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for &(i, c) in series {
                ui.allocate_ui(Vec2::new(col_w, h), |ui| {
                    ui.set_width(col_w);
                    ui.set_min_height(h);
                    ui.vertical(|ui| row(ui, s, i, c, view, opts));
                });
            }
        });
    }
}

/// An x-axis label made by `text`, left off within 18 pt of the plot's
/// right edge, where it would run past the plot into what is beside it.
fn clear_of_edge(
    plot_w: f64,
    text: impl Fn(f64) -> String,
) -> impl Fn(egui_plot::GridMark, &std::ops::RangeInclusive<f64>) -> String {
    move |m, r| {
        let span = (r.end() - r.start()).max(f64::EPSILON);
        if (r.end() - m.value) / span * plot_w < 18.0 {
            String::new()
        } else {
            text(m.value)
        }
    }
}

/// One series' title and plot. Its findings and statistics are in the
/// Info panel.
fn row(ui: &mut egui::Ui, s: &Steadiness, i: usize, c: &SeriesCheck, view: View, opts: Opts) {
    let log_y = opts.periods_log;
    let pal = theme::pal(ui);
    ui.label(theme::caption(title(c.series)).color(pal.text_secondary));
    // Too few readings: no tests ran, so there is nothing to draw; the
    // headline says how many it needs.
    if c.autocorrelation.is_none() {
        return note(ui, &c.headline);
    }
    let k = c.series;
    let colour = series_colour(k, pal);
    let weak = pal.text_tertiary;
    let id = ("steadiness", i, view);
    // The plot's width inside its value axis, for the time labels at the
    // right edge.
    let plot_w = f64::from(ui.available_width() - 64.0);
    let base = Plot::new(id)
        // Grid lines reach full strength only this far apart, so at the
        // usual spacing they are faint.
        .grid_spacing(8.0..=1200.0)
        .allow_scroll(false)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_axis_zoom_drag(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .show_x(false)
        .show_y(false)
        .y_axis_min_width(64.0)
        .height(ui.available_height().max(40.0));
    let dashed = LineStyle::Dashed { length: 6.0 };
    match view {
        View::Readings => {
            let g = &s.grids[i];
            let n = g.y.len();
            let stride = n.div_ceil(MAX_DRAWN).max(1);
            let pts: Vec<[f64; 2]> = (0..n)
                .step_by(stride)
                .filter(|&j| g.y[j].is_finite())
                .map(|j| [g.time(j), g.y[j]])
                .collect();
            let range = spread(&pts);
            let mut plot = base
                .x_grid_spacer(|g| theme::even_grid(g, 80.0, &theme::TIME_STEPS))
                .x_axis_formatter(clear_of_edge(plot_w, strip::fmt_time))
                .y_grid_spacer(|g| theme::even_grid(g, 24.0, &[]))
                .y_axis_formatter(move |m, _| value_text(k, m.value, m.step_size));
            if let Some((lo, hi)) = range {
                plot = plot.default_y_bounds(lo, hi);
            }
            // What a dot is, and how many are drawn, as Charts' key.
            let key = format!("{} ({})", kind_of(g.step), pts.len());
            let resp = plot.show(ui, |p| {
                p.points(Points::new("readings", pts).color(colour).radius(1.4_f32));
                if let Some(ch) = &c.changes {
                    if ch.segments.len() > 1 {
                        for seg in &ch.segments {
                            p.line(
                                Line::new(
                                    "level",
                                    vec![[seg.start_s, seg.mean], [seg.end_s, seg.mean]],
                                )
                                .color(pal.text)
                                .width(1.8_f32),
                            );
                        }
                    }
                }
                if let Some(ts) = &c.two_state {
                    for v in [ts.low, ts.high] {
                        p.hline(
                            HLine::new("state", v)
                                .color(pal.text)
                                .style(dashed)
                                .width(1.0_f32),
                        );
                    }
                }
            });
            crate::app::paint_key(
                ui.painter(),
                resp.transform.frame().to_owned(),
                &[(key.as_str(), colour, None)],
                ui,
                pal,
            );
        }
        View::Periods => {
            let Some(search) = s.searches[i].as_ref().filter(|x| !x.period_s.is_empty()) else {
                return note(ui, "The period search needs a longer session.");
            };
            let pts: Vec<[f64; 2]> = search
                .period_s
                .iter()
                .zip(&search.score)
                .filter(|(p, v)| **p > 0.0 && v.is_finite())
                .map(|(p, v)| [p.log10(), *v])
                .collect();
            // The search runs from long periods to short.
            let (x_lo, x_hi) = s.period_span.unwrap_or((
                pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
                pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max),
            ));
            if x_lo.partial_cmp(&x_hi) != Some(std::cmp::Ordering::Less) {
                return note(ui, "The period search needs a longer session.");
            }
            // On a log scale the scores are drawn as log10, down to a
            // thousandth of the tallest (a score of 0 sits on that floor).
            let tallest = pts.iter().map(|p| p[1]).fold(search.threshold, f64::max);
            let floor = tallest * 1e-3;
            let y_of = move |v: f64| if log_y { v.max(floor).log10() } else { v };
            let pts: Vec<[f64; 2]> = pts.iter().map(|q| [q[0], y_of(q[1])]).collect();
            let (y_lo, top) = if log_y {
                // Room above the tallest for the wheels' names.
                (floor.log10(), tallest.log10() + 0.45)
            } else {
                (0.0, tallest * 1.15)
            };
            let threshold = y_of(search.threshold);
            let mut plot = base
                .x_grid_spacer(period_grid(ui.available_width()))
                .x_axis_formatter(clear_of_edge(plot_w, period_label))
                .default_x_bounds(x_lo, x_hi)
                .default_y_bounds(y_lo, top);
            plot = if log_y {
                plot.y_grid_spacer(log_grid)
                    .y_axis_formatter(|m, r| log_label(m, r, sig))
            } else {
                plot.y_grid_spacer(|g| theme::even_grid(g, 24.0, &[]))
                    .y_axis_formatter(|m, _| plain(m.value, m.step_size))
            };
            // The names stay clear of the tallest peak's label.
            let label_y = if log_y { top } else { top * 0.97 };
            plot.show(ui, |p| {
                wheel_marks(p, marked(s, opts), (x_lo, x_hi), label_y, pal);
                p.hline(
                    HLine::new("threshold", threshold)
                        .color(pal.text_secondary)
                        .style(dashed)
                        .width(1.0_f32),
                );
                p.line(Line::new("score", pts.clone()).color(colour).width(1.5_f32));
                if let Some(per) = &c.period {
                    let x = per.period_s.log10();
                    let y = pts
                        .iter()
                        .min_by(|a, b| (a[0] - x).abs().total_cmp(&(b[0] - x).abs()))
                        .map_or(0.0, |q| q[1]);
                    p.points(
                        Points::new("peak", vec![[x, y]])
                            .color(colour)
                            .radius(3.5_f32),
                    );
                    let mut t = secs(per.period_s);
                    if let Some(w) = &per.wheel {
                        t += &format!(", {}", wheel_title(w));
                    }
                    mark_label(p, [x, y], t, pal.text, false);
                }
            });
        }
        View::Cycle => {
            let Some(f) = &s.folds[i] else {
                return note(ui, "No cycle found to fold these readings at.");
            };
            let bins = f.profile.len();
            let x = |b: usize| (b as f64 + 0.5) / bins as f64 * f.period_s;
            // The dots go in bands by when their cycle came, first to
            // last, so slow wander shows as a run of colour. The map's
            // ends are left off so both themes' cards keep every dot seen.
            let rows = f.raster.len();
            let bands = rows.clamp(1, TIME_BANDS);
            let mut by_time: Vec<Vec<[f64; 2]>> = vec![Vec::new(); bands];
            for (row, r) in f.raster.iter().enumerate() {
                let band = row * bands / rows.max(1);
                by_time[band].extend(
                    r.iter()
                        .enumerate()
                        .filter(|(_, v)| v.is_finite())
                        .map(|(b, v)| [x(b), *v]),
                );
            }
            let dots: Vec<[f64; 2]> = by_time.iter().flatten().copied().collect();
            let boxes = steady_views::fold_columns(f);
            let box_w = 0.3 * f.period_s / bins as f64;
            let shape: Vec<[f64; 2]> = f
                .profile
                .iter()
                .enumerate()
                .filter(|(_, v)| v.is_finite())
                .map(|(b, v)| [x(b), *v])
                .collect();
            let range = spread(&dots);
            let mut plot = base
                .x_grid_spacer(|g| theme::even_grid(g, 80.0, &theme::TIME_STEPS))
                .x_axis_formatter(clear_of_edge(plot_w, lag_text))
                .y_grid_spacer(|g| theme::even_grid(g, 24.0, &[]))
                .y_axis_formatter(move |m, _| value_text(k, m.value, m.step_size))
                .default_x_bounds(0.0, f.period_s);
            if let Some((lo, hi)) = range {
                plot = plot.default_y_bounds(lo, hi);
            }
            let label = match &s.fold_from[i] {
                FoldFrom::Own => {
                    let mut t = format!("{} cycle", secs(f.period_s));
                    let found = [
                        c.cycle.as_ref().map(|c| (c.period_s, c.wheel.as_deref())),
                        c.period.as_ref().map(|p| (p.period_s, p.wheel.as_deref())),
                    ];
                    let found: Vec<_> = found.into_iter().flatten().collect();
                    if let Some(w) = fold_wheel(&found, f.period_s) {
                        t += &format!(", {}", wheel_title(w));
                    }
                    t + &format!(", {} cycles folded", f.raster.len())
                }
                FoldFrom::Series(j) => format!(
                    "Folded at the {}'s {} cycle, {} cycles",
                    s.report
                        .series
                        .get(*j)
                        .map_or("other", |o| title(o.series))
                        .to_lowercase(),
                    secs(f.period_s),
                    f.raster.len()
                ),
                FoldFrom::Wheel(w) => format!(
                    "Folded at the {}'s {} turn, {} cycles",
                    wheel_title(w).to_lowercase(),
                    secs(f.period_s),
                    f.raster.len()
                ),
            };
            let label = match steady_views::fold_spread(f, &boxes) {
                Some((swing, middle)) => format!(
                    "{label}\nThe median line swings {}; the boxes' median height is {}",
                    value_text(k, swing, swing.abs().min(middle.abs()) / 10.0),
                    value_text(k, middle, swing.abs().min(middle.abs()) / 10.0),
                ),
                None => label,
            };
            // Above the plot, wrapped to its width, so it never sits on the
            // axis or runs past the plot's edges.
            ui.add(egui::Label::new(RichText::new(label).small().color(pal.text_secondary)).wrap());
            let plot = plot.height(ui.available_height().max(40.0));
            plot.show(ui, |p| {
                for (band, pts) in by_time.into_iter().enumerate() {
                    if pts.is_empty() {
                        continue;
                    }
                    let t = if bands > 1 {
                        band as f32 / (bands - 1) as f32
                    } else {
                        0.5
                    };
                    p.points(
                        Points::new("cycles", pts)
                            .color(crate::tfmap::viridis(0.15 + 0.75 * t))
                            .radius(1.4_f32),
                    );
                }
                for (b, q) in boxes.iter().enumerate() {
                    let Some([q1, _, q3]) = *q else { continue };
                    let (l, r) = (x(b) - box_w, x(b) + box_w);
                    p.polygon(
                        Polygon::new("middle half", vec![[l, q1], [r, q1], [r, q3], [l, q3]])
                            .fill_color(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::new(1.0_f32, weak)),
                    );
                }
                p.line(Line::new("shape", shape).color(colour).width(2.0_f32));
            });
        }
        View::Autocorrelation => {
            let Some(a) = &c.autocorrelation else {
                return note(ui, "Too few readings for the autocorrelation.");
            };
            let pts: Vec<[f64; 2]> = a
                .lag_s
                .iter()
                .zip(&a.r)
                .filter(|(_, r)| r.is_finite())
                .map(|(l, r)| [*l, *r])
                .collect();
            let x_hi = pts.last().map_or(1.0, |p| p[0]);
            let lo = pts.iter().map(|p| p[1]).fold(-a.band, f64::min).min(-0.2);
            base.x_grid_spacer(|g| theme::even_grid(g, 80.0, &theme::TIME_STEPS))
                .x_axis_formatter(clear_of_edge(plot_w, lag_text))
                .y_grid_spacer(|g| theme::even_grid(g, 24.0, &[]))
                .y_axis_formatter(|m, _| plain(m.value, m.step_size))
                .default_x_bounds(0.0, x_hi)
                // Room above the curve for the Ljung–Box note.
                .default_y_bounds(lo - 0.05, 1.35)
                .show(ui, |p| {
                    p.hline(HLine::new("zero", 0.0).color(weak).width(1.0_f32));
                    for b in [a.band, -a.band] {
                        p.hline(
                            HLine::new("band", b)
                                .color(pal.text_secondary)
                                .style(dashed)
                                .width(1.0_f32),
                        );
                    }
                    p.line(Line::new("r", pts).color(colour).width(1.5_f32));
                    if let (Some(l), Some(r)) = (a.repeat_lag_s, a.repeat_r) {
                        p.points(
                            Points::new("repeat", vec![[l, r]])
                                .color(colour)
                                .radius(3.5_f32),
                        );
                        mark_label(
                            p,
                            [l, r],
                            format!("repeats at {}", secs(l)),
                            pal.text,
                            false,
                        );
                    }
                    p.text(
                        Text::new(
                            "ljung box",
                            PlotPoint::new(x_hi, 1.35),
                            format!("Ljung–Box p = {} ", steadiness::p_text(a.p_value)),
                        )
                        .color(pal.text_secondary)
                        .anchor(egui::Align2::RIGHT_TOP),
                    );
                });
        }
        View::Allan => {
            let Some(a) = &c.allan else {
                return note(ui, "Too few readings for the Allan deviation.");
            };
            let both = |v: &[f64]| -> Vec<[f64; 2]> {
                a.tau_s
                    .iter()
                    .zip(v)
                    .filter(|(t, d)| **t > 0.0 && **d > 0.0 && d.is_finite())
                    .map(|(t, d)| [t.log10(), d.log10()])
                    .collect()
            };
            let dev = both(&a.deviation);
            let white = both(&a.white);
            base.x_grid_spacer(period_grid(ui.available_width()))
                .x_axis_formatter(clear_of_edge(plot_w, period_label))
                .y_grid_spacer(log_grid)
                .y_axis_formatter(move |m, r| log_label(m, r, |v| unit_sig(k, v)))
                .show(ui, |p| {
                    let last = white.last().copied();
                    p.line(
                        Line::new("independent", white)
                            .color(pal.text_secondary)
                            .style(dashed)
                            .width(1.0_f32),
                    );
                    // Name the dashed line where it ends.
                    if let Some(at) = last {
                        mark_label(
                            p,
                            at,
                            "if the readings were independent".into(),
                            pal.text_secondary,
                            false,
                        );
                    }
                    p.line(
                        Line::new("deviation", dev.clone())
                            .color(colour)
                            .width(1.5_f32),
                    );
                    p.points(
                        Points::new("deviation points", dev)
                            .color(colour)
                            .radius(2.0_f32),
                    );
                    if let (Some(t), Some(d)) = (a.best_tau_s, a.best_deviation) {
                        if t > 0.0 && d > 0.0 {
                            let at = [t.log10(), d.log10()];
                            mark_label(
                                p,
                                at,
                                format!("least scatter at {}: {}", secs(t), unit_sig(k, d)),
                                pal.text,
                                false,
                            );
                        }
                    }
                });
        }
        View::Cusum => {
            let Some(cu) = &c.cusum else {
                return note(ui, "Too few readings for the CUSUM.");
            };
            let pts: Vec<[f64; 2]> = cu
                .t_s
                .iter()
                .zip(&cu.s)
                .filter(|(_, v)| v.is_finite())
                .map(|(t, v)| [*t, *v])
                .collect();
            let m = pts.iter().map(|p| p[1].abs()).fold(1.6, f64::max) * 1.1;
            base.x_grid_spacer(|g| theme::even_grid(g, 80.0, &theme::TIME_STEPS))
                .x_axis_formatter(clear_of_edge(plot_w, strip::fmt_time))
                .y_grid_spacer(|g| theme::even_grid(g, 24.0, &[]))
                .y_axis_formatter(|m, _| plain(m.value, m.step_size))
                .default_y_bounds(-m, m)
                .show(ui, |p| {
                    p.hline(HLine::new("zero", 0.0).color(weak).width(1.0_f32));
                    for b in [1.36, -1.36] {
                        p.hline(
                            HLine::new("95%", b)
                                .color(pal.text_secondary)
                                .style(dashed)
                                .width(1.0_f32),
                        );
                    }
                    p.line(Line::new("cusum", pts.clone()).color(colour).width(1.5_f32));
                    if cu.max > 1.36 {
                        let y = pts
                            .iter()
                            .min_by(|a, b| {
                                (a[0] - cu.at_s).abs().total_cmp(&(b[0] - cu.at_s).abs())
                            })
                            .map_or(0.0, |q| q[1]);
                        p.points(
                            Points::new("corner", vec![[cu.at_s, y]])
                                .color(colour)
                                .radius(3.5_f32),
                        );
                        mark_label(
                            p,
                            [cu.at_s, y],
                            format!("level changes near {}", strip::fmt_time(cu.at_s)),
                            pal.text,
                            y >= 0.0,
                        );
                    }
                });
        }
        View::Stft | View::Wavelet => {
            let name = match k {
                SeriesKind::Rate => "rate",
                SeriesKind::Amplitude => "amplitude",
                _ => "beat_error",
            };
            let method = if view == View::Stft { "stft" } else { "sst" };
            let Some(m) = s
                .maps
                .iter()
                .find(|m| m.series == name && m.method == method)
            else {
                return note(
                    ui,
                    if view == View::Stft {
                        "The STFT Map needs 20 minutes of readings with none missing."
                    } else {
                        "The wavelet map needs a longer stretch of unbroken readings."
                    },
                );
            };
            crate::tfmap::draw(
                ui,
                base,
                egui::Id::new(id),
                m,
                s.upto_s,
                opts.maps_log,
                marked(s, opts),
                strip::fmt_time,
                secs,
            );
        }
        View::Fft => {
            let name = match k {
                SeriesKind::Rate => "rate",
                SeriesKind::Amplitude => "amplitude",
                _ => "beat_error",
            };
            let Some(sp) = s
                .report
                .spectra
                .iter()
                .find(|x| x.series == name && !x.period_s.is_empty())
            else {
                return note(ui, "The spectrum needs a longer session.");
            };
            // Rate's spectrum is of the beat timing, in ms: drawn unless
            // asked as the swing it makes in the rate readings, peak to
            // peak in s/d, over the periods those readings can show.
            let swing = sp.reading_swing.as_ref().filter(|_| !opts.fft_timing);
            let as_rate = swing.is_some();
            let (periods, sizes) = match swing {
                Some(s) => (&s.period_s, &s.swing_s_per_day),
                None => (&sp.period_s, &sp.amplitude),
            };
            let pts: Vec<[f64; 2]> = periods
                .iter()
                .zip(sizes)
                .map(|(&p, &a)| (p, a))
                .filter(|(p, a)| *p > 0.0 && *a > 0.0 && a.is_finite())
                .map(|(p, a)| [p.log10(), a.log10()])
                .collect();
            if pts.len() < 2 {
                return note(ui, "The spectrum needs a longer session.");
            }
            ui.label(
                RichText::new(sp.support.description())
                    .small()
                    .color(pal.text_tertiary),
            );
            let base = base.height(ui.available_height().max(40.0));
            let (x_lo, x_hi) = pts
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                    (lo.min(p[0]), hi.max(p[0]))
                });
            let (y_lo, y_hi) = pts
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                    (lo.min(p[1]), hi.max(p[1]))
                });
            // Room above the tallest line for the wheels' names.
            let top = y_hi + (y_hi - y_lo).max(0.5) * 0.25;
            let unit = match sp.unit {
                _ if as_rate => " s/d",
                "deg" => "°",
                _ => " ms",
            };
            let size = move |v: f64| {
                let d = if v >= 10.0 {
                    0
                } else if v >= 1.0 {
                    1
                } else if v >= 0.1 {
                    2
                } else {
                    3
                };
                format!("{v:.d$}{unit}")
            };
            let peak = tallest_peak(&pts);
            base.x_grid_spacer(period_grid(ui.available_width()))
                .x_axis_formatter(clear_of_edge(plot_w, period_label))
                .y_grid_spacer(log_grid)
                .y_axis_formatter(move |m, r| log_label(m, r, size))
                .default_x_bounds(x_lo, x_hi)
                .default_y_bounds(y_lo, top)
                .show(ui, |p| {
                    wheel_marks(p, marked(s, opts), (x_lo, x_hi), top, pal);
                    p.line(Line::new("spectrum", pts).color(colour).width(1.5_f32));
                    if let Some(peak) = peak {
                        p.points(
                            Points::new("tallest", vec![peak])
                                .color(colour)
                                .radius(3.5_f32),
                        );
                        mark_label(
                            p,
                            peak,
                            format!(
                                "tallest peak at {}: {}{}",
                                secs(10f64.powf(peak[0])),
                                size(10f64.powf(peak[1])),
                                if as_rate { " peak to peak" } else { "" }
                            ),
                            pal.text,
                            false,
                        );
                    }
                });
        }
    }
}

/// How the views draw what they can draw two ways.
#[derive(Clone, Copy, Default)]
pub struct Opts {
    /// Periods: the scores on a log scale.
    pub periods_log: bool,
    /// FFT: rate as the beat timing's spectrum in ms, not its swing in
    /// the rate readings.
    pub fft_timing: bool,
    /// STFT and wavelet maps: the sizes' colours on a log scale.
    pub maps_log: bool,
    /// The calibre's wheels marked on the period axis, or the reference
    /// periods when no wheel is known.
    pub wheels: bool,
    /// The Info panel, beside the plots: one setting for every view.
    pub info: bool,
}

/// What Wheel Periods does, for its hint.
pub const WHEELS_HINT: &str = "Mark the period each of the calibre's wheels turns in, in \
     a grey dotted line named beside it, and half of it fainter, where a wheel that is off centre \
     shows twice a turn. With no calibre picked, or none of its wheels known, one minute and \
     one hour are marked in grey instead: periods alone, named after no wheel.";

/// The wheels to mark: `None` when the marks are off.
fn marked(s: &Steadiness, opts: Opts) -> Option<&[Wheel]> {
    opts.wheels.then_some(&s.wheels[..])
}

/// The wheels' turns on a log10 period axis spanning `span`, in the
/// wheels' own colour: a bold line and name at each wheel's period, and a
/// fainter dashed one at half of it ("×2"), where a wheel that is off
/// centre shows twice a turn. Names sit at height `label_y`. With no
/// wheels, the reference periods in grey; with `None`, nothing.
fn wheel_marks(
    p: &mut egui_plot::PlotUi<'_>,
    wheels: Option<&[Wheel]>,
    (x_lo, x_hi): (f64, f64),
    label_y: f64,
    pal: &theme::Palette,
) {
    let inside = |x: f64| x > x_lo && x < x_hi;
    let Some(wheels) = wheels else {
        return;
    };
    if wheels.is_empty() {
        for r in reference_lines() {
            let x = r.period_s.log10();
            if inside(x) {
                p.vline(
                    VLine::new(r.label, x)
                        .color(pal.text_tertiary)
                        .width(1.25_f32),
                );
                p.text(
                    Text::new("reference", PlotPoint::new(x, label_y), r.label)
                        .color(pal.text_secondary)
                        .anchor(egui::Align2::LEFT_TOP),
                );
            }
        }
        return;
    }
    for (k, w) in wheels.iter().enumerate() {
        // Two candidates of one name (the escape wheel at 15 and at 20
        // teeth): the shorter one is labelled on its left, so the two
        // names don't overlap.
        let shorter_twin = wheels
            .iter()
            .enumerate()
            .any(|(j, o)| j != k && o.name == w.name && o.period_s > w.period_s);
        let x = w.period_s.log10();
        if inside(x) {
            p.vline(
                VLine::new(w.name.clone(), x)
                    .color(pal.wheel)
                    .style(LineStyle::dotted_dense())
                    .width(2.0_f32),
            );
            p.text(
                Text::new("wheel", PlotPoint::new(x, label_y), wheel_title(&w.name))
                    .color(pal.wheel)
                    .anchor(if shorter_twin {
                        egui::Align2::RIGHT_TOP
                    } else {
                        egui::Align2::LEFT_TOP
                    }),
            );
        }
        // Twice a turn, unless another wheel turns there.
        let half = w.period_s / 2.0;
        let x2 = half.log10();
        let taken = wheels
            .iter()
            .any(|o| (o.period_s / half - 1.0).abs() < 0.03);
        if inside(x2) && !taken && !shorter_twin {
            p.vline(
                VLine::new(format!("{} ×2", w.name), x2)
                    .color(pal.wheel.gamma_multiply(0.6))
                    .style(LineStyle::dotted_loose())
                    .width(1.25_f32),
            );
            p.text(
                Text::new(
                    "wheel ×2",
                    PlotPoint::new(x2, label_y),
                    format!("{} ×2", wheel_title(&w.name)),
                )
                .color(pal.wheel.gamma_multiply(0.8))
                .anchor(egui::Align2::LEFT_TOP),
            );
        }
    }
}

/// A note beside a marked point, on whichever side keeps it inside the
/// plot: left or right of it, and above it when `above` asks and there is
/// room there, else below (and the other way round).
fn mark_label(
    p: &mut egui_plot::PlotUi<'_>,
    at: [f64; 2],
    text: String,
    color: Color32,
    above: bool,
) {
    // A line of small text and a little air, points.
    const ROOM: f32 = 18.0;
    let frame = *p.transform().frame();
    let y = p.screen_from_plot(PlotPoint::new(at[0], at[1])).y;
    let above = if above {
        y - frame.top() >= ROOM || frame.bottom() - y < ROOM
    } else {
        frame.bottom() - y < ROOM && y - frame.top() >= ROOM
    };
    let b = p.plot_bounds();
    let right = at[0] > (b.min()[0] + b.max()[0]) / 2.0;
    let h = if right {
        egui::Align::Max
    } else {
        egui::Align::Min
    };
    let v = if above {
        egui::Align::Max
    } else {
        egui::Align::Min
    };
    let text = if right {
        format!("{text}  ")
    } else {
        format!("  {text}")
    };
    p.text(
        Text::new("mark", PlotPoint::new(at[0], at[1]), text)
            .color(color)
            .anchor(egui::Align2([h, v])),
    );
}

/// A plain number for a linear axis, with no "-0.0".
fn plain(v: f64, step: f64) -> String {
    let d = if step >= 1.0 {
        0
    } else if step >= 0.1 {
        1
    } else {
        2
    };
    let v = if v.abs() < step * 1e-6 { 0.0 } else { v };
    format!("{v:.d$}")
}

/// A number in a series' units to about three figures.
fn unit_sig(k: SeriesKind, v: f64) -> String {
    match k {
        SeriesKind::Rate => format!("{} s/d", sig(v)),
        SeriesKind::Amplitude => format!("{}°", sig(v)),
        SeriesKind::BeatError => format!("{} ms", sig(v)),
    }
}

/// The label for a grid line on a log10 axis: decades always, the 2 and 5
/// lines only when less than three decades show.
fn log_label(
    m: GridMark,
    range: &std::ops::RangeInclusive<f64>,
    f: impl Fn(f64) -> String,
) -> String {
    let span = range.end() - range.start();
    if m.step_size >= 1.0 || span < 3.0 {
        f(10f64.powf(m.value))
    } else {
        String::new()
    }
}

/// How many colours the Cycle view's dots take, first cycle to last.
const TIME_BANDS: usize = 12;

/// The 1st to 99th percentile of the points' values, padded a little,
/// leaving out lone wild values (a knock, a dropped beat) so that two of
/// them don't flatten the rest: those beyond three interquartile ranges
/// of the middle half, unless they are more than 3% of the values (then
/// they are part of what the watch does, as in the steadiness test).
fn spread(pts: &[[f64; 2]]) -> Option<(f64, f64)> {
    if pts.len() < 2 {
        return None;
    }
    let mut v: Vec<f64> = pts.iter().map(|p| p[1]).collect();
    v.sort_by(f64::total_cmp);
    let q = |v: &[f64], f: f64| v[((v.len() - 1) as f64 * f).round() as usize];
    let (q1, q3) = (q(&v, 0.25), q(&v, 0.75));
    let fence = 3.0 * (q3 - q1);
    let kept: Vec<f64> = v
        .iter()
        .copied()
        .filter(|&x| x >= q1 - fence && x <= q3 + fence)
        .collect();
    if kept.len() >= 2 && (v.len() - kept.len()) as f64 <= 0.03 * v.len() as f64 {
        v = kept;
    }
    let (lo, hi) = (q(&v, 0.01), q(&v, 0.99));
    let pad = ((hi - lo) * 0.12).max(1e-3);
    Some((lo - pad, hi + pad))
}

fn note(ui: &mut egui::Ui, text: &str) {
    let pal = theme::pal(ui);
    ui.centered_and_justified(|ui| {
        ui.label(RichText::new(text).small().color(pal.text_tertiary));
    });
}

#[cfg(test)]
mod tests {
    /// A fold names a wheel only when the finding folded at is that
    /// wheel's: a 46 s fold doesn't borrow the escape wheel of the beat
    /// timing's 5.0 s line.
    #[test]
    fn a_fold_names_only_its_own_findings_wheel() {
        let escape = Some("escape wheel");
        assert_eq!(fold_wheel(&[(46.0, None), (5.0, escape)], 46.0), None);
        assert_eq!(fold_wheel(&[(5.0, escape)], 5.0), escape);
        assert_eq!(fold_wheel(&[(46.0, None), (46.2, escape)], 46.0), escape);
        assert_eq!(fold_wheel(&[], 46.0), None);
    }

    #[test]
    fn a_spectrum_climbing_at_its_end_labels_its_own_peak_not_the_end() {
        // log10 period against log10 size: a hump at 60 s, then a climb
        // to the longest period that ends higher than the hump.
        let pts = [[1.0, 0.0], [1.78, 1.0], [2.2, 0.4], [3.0, 1.5], [3.3, 2.0]];
        assert_eq!(super::tallest_peak(&pts), Some([1.78, 1.0]));
        assert_eq!(
            super::tallest_peak(&[[1.0, 0.0], [2.0, 1.0], [3.0, 2.0]]),
            None
        );
    }

    #[test]
    fn the_cycle_columns_give_their_quartiles_and_the_line_its_swing() {
        let nan = f64::NAN;
        let f = super::Fold {
            period_s: 60.0,
            profile: vec![1.0, -1.0, nan],
            counts: vec![5, 5, 1],
            raster: vec![
                vec![0.0, -3.0, 9.0],
                vec![1.0, -2.0, nan],
                vec![2.0, -1.0, nan],
                vec![3.0, 0.0, nan],
                vec![4.0, 1.0, nan],
            ],
            t0: 0.0,
        };
        let c = steady_views::fold_columns(&f);
        assert_eq!(c[0], Some([1.0, 2.0, 3.0]));
        assert_eq!(c[1], Some([-2.0, -1.0, 0.0]));
        assert_eq!(c[2], None);
        assert_eq!(steady_views::fold_spread(&f, &c), Some((2.0, 2.0)));
    }

    use super::*;

    /// A period axis is marked at times a watchmaker reads, in their own
    /// units, not at 100 s and "17 min".
    #[test]
    fn a_period_axis_is_marked_in_watchmakers_times() {
        let marks = period_grid(900.0)(GridInput {
            bounds: (4f64.log10(), 60_000f64.log10()),
            base_step_size: 0.001,
        });
        let labels: Vec<String> = marks.iter().map(|m| period_label(m.value)).collect();
        assert!(labels.contains(&"1 min".to_string()), "{labels:?}");
        assert!(labels.contains(&"1 h".to_string()), "{labels:?}");
        assert!(
            labels.iter().all(|l| !l.is_empty() && !l.contains("17")),
            "{labels:?}"
        );
        // Narrow, fewer marks, the main ones kept.
        let few = period_grid(200.0)(GridInput {
            bounds: (4f64.log10(), 60_000f64.log10()),
            base_step_size: 0.001,
        });
        assert!(few.len() < marks.len() && few.len() >= 3);
        assert_eq!(period_label(1000f64.log10()), "");
    }
    use timegrapher_core::beats::Beat;
    use timegrapher_core::live::LiveConfig;
    use timegrapher_core::synth::{generate, SynthConfig};

    /// The plots carry only their titles and a key saying what a dot is;
    /// the Info panel holds the view's description, each series' finding
    /// whole and its statistics, which say what they counted, and what was
    /// tested: beside the plots in a wide pane, over them in a narrow one.
    #[test]
    fn the_info_panel_holds_the_findings_and_the_plots_say_what_a_dot_is() {
        let cfg = SynthConfig {
            duration_s: 600.0,
            rate_s_per_day: 5.0,
            ..Default::default()
        };
        let audio = generate(
            &cfg,
            |_| 270.0,
            |t| 20.0 * (std::f64::consts::TAU * t / 60.0).sin(),
        );
        let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
        for b in audio.samples.chunks(4800) {
            live.push(b);
        }
        let s = compute(&log_of(&live).expect("beat rate"), None);
        let rate = s.report.series[0].headline.clone();
        let ctx = egui::Context::default();
        theme::install(&ctx);
        fn texts(shape: &egui::Shape, into: &mut Vec<(String, f32)>) {
            match shape {
                egui::Shape::Text(t) => into.push((t.galley.text().to_string(), t.pos.x)),
                egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, into)),
                _ => {}
            }
        }
        let drawn = |w: f32, info: bool| {
            let mut all = Vec::new();
            for _ in 0..2 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(w, 700.0),
                    )),
                    ..Default::default()
                };
                let out = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let opts = Opts {
                            info,
                            ..Default::default()
                        };
                        draw(
                            ui,
                            Some(&s),
                            "Tested over 10:00 of beats",
                            View::Readings,
                            &[true, true, false],
                            opts,
                        );
                    });
                });
                all.clear();
                out.shapes.iter().for_each(|c| texts(&c.shape, &mut all));
            }
            all
        };
        let has = |all: &[(String, f32)], t: &str| all.iter().any(|(s, _)| s.contains(t));
        let closed = drawn(900.0, false);
        assert!(has(&closed, "10 s readings ("), "{closed:?}");
        assert!(has(&closed, "2 s windows ("), "{closed:?}");
        for gone in [
            rate.as_str(),
            "Tested on",
            "Tested over",
            "Every reading in time order",
        ] {
            assert!(!has(&closed, gone), "{gone}");
        }
        let open = drawn(900.0, true);
        for t in [
            rate.as_str(),
            "Tested on ",
            "Tested over 10:00 of beats",
            "Every reading in time order",
            "Readings",
        ] {
            assert!(has(&open, t), "{t}: {open:?}");
        }
        // Beside the plots: the panel's words start in its third.
        let x = open
            .iter()
            .find(|(s, _)| s.contains("Tested on "))
            .unwrap()
            .1;
        assert!(x >= 900.0 - INFO_W - 16.0, "{x}");
        assert!(!has(&open, "…"), "{open:?}");
        // Narrow: over the plots, the same words.
        let narrow = drawn(500.0, true);
        assert!(has(&narrow, rate.as_str()) && has(&narrow, "Tested on "));
    }

    /// In a short pane the Info panel scrolls inside it rather than running
    /// off its bottom, and Cycle's note sits above each plot, inside its
    /// column, clear of the panel.
    #[test]
    fn a_short_panes_info_scrolls_and_the_cycle_note_stays_in_its_column() {
        let cfg = SynthConfig {
            duration_s: 600.0,
            rate_s_per_day: 5.0,
            ..Default::default()
        };
        let audio = generate(
            &cfg,
            |_| 270.0,
            |t| 20.0 * (std::f64::consts::TAU * t / 60.0).sin(),
        );
        let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
        for b in audio.samples.chunks(4800) {
            live.push(b);
        }
        let s = compute(&log_of(&live).expect("beat rate"), None);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let (w, h) = (1440.0, 300.0);
        let mut out = None;
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h))),
                ..Default::default()
            };
            out = Some(ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let opts = Opts {
                        info: true,
                        ..Default::default()
                    };
                    draw(
                        ui,
                        Some(&s),
                        "Tested over 10:00 of beats",
                        View::Cycle,
                        &[true, true, true],
                        opts,
                    );
                });
            }));
        }
        let out = out.unwrap();
        let mut texts = Vec::new();
        for c in &out.shapes {
            if let egui::Shape::Text(t) = &c.shape {
                let r = t.galley.rect.translate(t.pos.to_vec2());
                texts.push((t.galley.text().to_string(), r, c.clip_rect));
            }
        }
        // The pane's inner edge is 8 pt in from the window's.
        let panel_left = w - 8.0 - INFO_W;
        // The panel's words are clipped at the pane's foot: below it they
        // scroll.
        let panel: Vec<_> = texts.iter().filter(|t| t.1.left() >= panel_left).collect();
        assert!(
            panel.iter().any(|t| t.0.contains("Tested on ")),
            "{texts:?}"
        );
        assert!(panel.iter().all(|t| t.2.bottom() <= h), "{panel:?}");
        let notes: Vec<_> = texts.iter().filter(|t| t.0.contains("cycles")).collect();
        assert!(!notes.is_empty(), "{texts:?}");
        for n in notes {
            assert!(n.1.left() >= 0.0 && n.1.right() < panel_left, "{n:?}");
        }
        // No axis label runs past the line before the panel.
        let line = panel_left - 12.0 / 2.0;
        for t in texts.iter().filter(|t| t.1.left() < panel_left) {
            assert!(t.1.right() <= line, "{t:?}");
        }
        // The plots take the room left under their notes.
        let axis: Vec<_> = texts.iter().filter(|t| t.0.ends_with(" s/d")).collect();
        assert!(!axis.is_empty(), "{texts:?}");
        assert!(axis.iter().all(|t| t.1.bottom() <= h), "{axis:?}");
    }

    /// Each view's Info panel opens at its top, with its heading in view:
    /// one scrolled down and closed, or another view's, does not open
    /// scrolled.
    #[test]
    fn the_info_panel_opens_at_its_top() {
        let cfg = SynthConfig {
            duration_s: 600.0,
            rate_s_per_day: 5.0,
            ..Default::default()
        };
        let audio = generate(
            &cfg,
            |_| 270.0,
            |t| 20.0 * (std::f64::consts::TAU * t / 60.0).sin(),
        );
        let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
        for b in audio.samples.chunks(4800) {
            live.push(b);
        }
        let s = compute(&log_of(&live).expect("beat rate"), None);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let (w, h) = (1440.0, 300.0);
        let panel_left = w - 8.0 - INFO_W;
        let at = egui::pos2(w - INFO_W / 2.0, h / 2.0);
        // One pass: the panel's heading, if in view, with `wheel` scrolling
        // the panel, or no pane at all.
        let pass = |view: Option<View>, wheel: bool| {
            let mut events = vec![egui::Event::PointerMoved(at)];
            if wheel {
                events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, -400.0),
                    modifiers: Default::default(),
                });
            }
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h))),
                events,
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(view) = view {
                        let opts = Opts {
                            info: true,
                            ..Default::default()
                        };
                        draw(ui, Some(&s), "", view, &[true, true, true], opts);
                    }
                });
            });
            let label = view.map(View::label).unwrap_or_default();
            out.shapes.iter().any(|c| match &c.shape {
                egui::Shape::Text(t) => {
                    let r = t.galley.rect.translate(t.pos.to_vec2());
                    t.galley.text() == label
                        && r.left() >= panel_left
                        && r.top() >= c.clip_rect.top()
                }
                _ => false,
            })
        };
        assert!(pass(Some(View::Cycle), false), "opens at its top");
        for _ in 0..20 {
            pass(Some(View::Cycle), true);
        }
        assert!(
            !pass(Some(View::Cycle), false),
            "scrolled, the heading is out of view"
        );
        // Another view's panel opens at its top.
        assert!(pass(Some(View::Readings), false));
        // Closed and opened again: at its top.
        pass(None, false);
        assert!(pass(Some(View::Cycle), false));
    }

    #[test]
    fn fft_renderer_paints_input_availability_in_both_rate_displays() {
        // Constructed beat/grid fixtures only: this tests presentation, not
        // acquisition, audio detection or a new spectral estimator.
        let mut state = compute(&context_log(), None);
        let c = steadiness::check_series(
            SeriesKind::Rate,
            Grid {
                t0: 0.0,
                step: 10.0,
                y: (0..40).map(|i| (i as f64).sin()).collect(),
            },
            None,
            None,
            None,
            &[],
            &Config::default(),
        );
        assert!(
            c.autocorrelation.is_some(),
            "exercise the actual FFT renderer"
        );
        state.report.series[0] = c;
        let ctx = egui::Context::default();
        theme::install(&ctx);
        for missing in [false, true] {
            let grid = Grid {
                t0: 0.0,
                step: 1.0,
                y: (0..256)
                    .map(|i| {
                        if missing && i % 2 != 0 {
                            f64::NAN
                        } else {
                            (std::f64::consts::TAU * i as f64 / 64.0).sin()
                        }
                    })
                    .collect(),
            };
            state.report.spectra =
                vec![
                    timegrapher_core::spectrum::amplitude_spectrum(&grid, "rate", "ms", 1.0, 600)
                        .unwrap(),
                ];
            for fft_timing in [false, true] {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(900.0, 700.0),
                    )),
                    ..Default::default()
                };
                let out = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        draw(
                            ui,
                            Some(&state),
                            "FFT support fixture",
                            View::Fft,
                            &[true, false, false],
                            Opts {
                                fft_timing,
                                ..Default::default()
                            },
                        );
                    });
                });
                let text = out
                    .shapes
                    .iter()
                    .filter_map(|c| match &c.shape {
                        egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(
                    text.contains(if missing {
                        "128 of 256 grid samples available"
                    } else {
                        "256 of 256 grid samples available"
                    }),
                    "{text}"
                );
                assert_eq!(text.contains("values are not corrected for gaps"), missing);
            }
        }
    }

    /// Wheel Periods marks the calibre's wheels in their own colour, named
    /// beside their lines, or one minute and one hour when no wheel is
    /// known; off, it marks neither.
    #[test]
    fn wheel_periods_marks_the_wheels_in_their_colour_or_the_reference_periods() {
        let mut state = compute(
            &context_log(),
            Some(vec![Wheel {
                name: "fourth wheel".into(),
                period_s: 60.0,
            }]),
        );
        state.report.series[0] = steadiness::check_series(
            SeriesKind::Rate,
            Grid {
                t0: 0.0,
                step: 10.0,
                y: (0..40).map(|i| (i as f64).sin()).collect(),
            },
            None,
            None,
            None,
            &[],
            &Config::default(),
        );
        let grid = Grid {
            t0: 0.0,
            step: 1.0,
            y: (0..256)
                .map(|i| (std::f64::consts::TAU * i as f64 / 64.0).sin())
                .collect(),
        };
        state.report.spectra =
            vec![
                timegrapher_core::spectrum::amplitude_spectrum(&grid, "rate", "ms", 1.0, 600)
                    .unwrap(),
            ];
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let wheel = theme::palette(ctx.style().visuals.dark_mode).wheel;
        let drawn = |state: &Steadiness, wheels: bool| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 700.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw(
                        ui,
                        Some(state),
                        "",
                        View::Fft,
                        &[true, false, false],
                        Opts {
                            wheels,
                            ..Default::default()
                        },
                    );
                });
            });
            let texts: Vec<(String, Color32)> = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.fallback_color)),
                    _ => None,
                })
                .collect();
            (texts, format!("{:?}", out.shapes))
        };
        let named = |texts: &[(String, Color32)], name: &str| {
            texts.iter().filter(|(t, _)| t == name).count()
        };
        // The wheel's grey is the faint text colour, which the plot uses
        // too: the wheel's line and name add to what is drawn in it.
        let in_wheel = |shapes: &str| shapes.matches(&format!("{wheel:?}")).count();
        let (on, shapes_on) = drawn(&state, true);
        assert_eq!(named(&on, "Fourth Wheel"), 1, "{on:?}");
        let (off, shapes_off) = drawn(&state, false);
        assert_eq!(named(&off, "Fourth Wheel"), 0, "{off:?}");
        assert!(
            in_wheel(&shapes_on) > in_wheel(&shapes_off),
            "the wheel's line and name in its colour"
        );
        // No wheel known: one minute marked, beside the axis's own "1 min".
        state.wheels.clear();
        let (on, _) = drawn(&state, true);
        let (off, _) = drawn(&state, false);
        assert_eq!(named(&on, "1 min"), named(&off, "1 min") + 1, "{on:?}");
    }

    // Known beat records only: these controls do not run the detector.
    fn context_log() -> BeatLog {
        BeatLog {
            sample_rate: 48000,
            duration_s: 120.0,
            bph: 28800,
            lift_deg: 52.0,
            beats: (0..960)
                .map(|index| timegrapher_core::beats::Beat {
                    time: index as f64 * 0.125 + if index >= 480 { 0.03 } else { 0.0 },
                    index,
                    quality: if index % 13 == 0 { 0.2 } else { 1.0 },
                })
                .collect(),
            amplitude_windows: Vec::new(),
            clipped_beats: vec![1.0, 12.0, 41.0, 61.0, 80.0],
            clipped_samples: 17,
            jumps: vec![60.03],
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        }
    }

    #[test]
    fn derived_log_keeps_known_cuts_and_beat_quality() {
        let log = context_log();
        let original: Vec<_> = log
            .beats
            .iter()
            .map(|b| (b.time, b.index, b.quality))
            .collect();
        let mut live = LiveAnalyzer::from_log(log, LiveConfig::default());
        live.set_breaks(vec![40.0, 70.0]);
        let derived = log_of(&live).unwrap();
        assert_eq!(derived.jumps, [40.0, 60.03, 70.0]);
        assert_eq!(
            derived
                .beats
                .iter()
                .map(|b| (b.time, b.index, b.quality))
                .collect::<Vec<_>>(),
            original
        );
        assert_eq!(derived.clipped_beats, live.clipped_beats());
        assert_eq!(derived.duration_s, live.duration_s());
        assert_eq!(derived.clipped_samples, 0, "not counted live, unchanged");
    }

    /// Both ends of a masked stretch are cuts, so no 10-second reading
    /// downstream reaches across one or takes in a ragged edge beside one.
    #[test]
    fn a_masked_stretch_is_cut_at_both_ends() {
        let mut live = LiveAnalyzer::from_log(context_log(), LiveConfig::default());
        live.set_masked(&[(20.0, 33.5)]);
        let derived = log_of(&live).unwrap();
        assert_eq!(derived.jumps, [20.0, 33.5, 60.03]);
        assert!(derived
            .beats
            .iter()
            .all(|b| !(20.0..33.5).contains(&b.time)));
    }

    #[test]
    fn derived_join_transports_cuts_and_existing_sample_context() {
        let mut log = context_log();
        log.jumps = vec![5.0, 10.0, 15.0, 20.0, 30.0, 40.0, 60.03, 80.0, 110.0];
        let spans = [(10.0, 20.0), (30.01, 30.02), (40.0, 80.0)];
        let joined = join(&log, &spans);
        // The empty middle span adds neither a join nor a cut. The second
        // retained span starts at15 (ten seconds plus the existing5s gap).
        assert_eq!(joined.jumps, [0.0, 5.0, 15.0, 35.03]);
        assert_eq!(joined.duration_s, 55.0);
        assert_eq!(joined.clipped_beats, [2.0, 16.0, 36.0]);
        assert_eq!(joined.clipped_samples, 17, "existing whole-log counter");
        let kept: Vec<_> = log
            .beats
            .iter()
            .filter(|b| (10.0..20.0).contains(&b.time) || (40.0..80.0).contains(&b.time))
            .collect();
        assert_eq!(joined.beats.len(), kept.len());
        for (out, input) in joined.beats.iter().zip(kept) {
            assert_eq!(out.quality, input.quality);
            assert_eq!(
                out.time,
                input.time - if input.time < 20.0 { 10.0 } else { 25.0 }
            );
        }
        assert!(joined.beats.windows(2).all(|b| b[0].index < b[1].index));
    }

    #[test]
    fn derived_join_keeps_amplitude_sides_on_the_renumbered_beats() {
        use timegrapher_core::amplitude::AmplitudeWindow;
        for (start, second_start, expected_swaps) in
            [(0.0, 40.0, [false, false]), (0.125, 40.125, [true, true])]
        {
            let end = start + 10.0;
            let mut log = context_log();
            let window = |a, b| AmplitudeWindow {
                start_s: a,
                end_s: b,
                even_deg: Some(260.0),
                odd_deg: Some(280.0),
                beat_error_ms: Some(0.4),
                beat_error_unlock_ms: Some(-0.2),
                even_unlock_ms: Some(-3.0),
                odd_unlock_ms: Some(-4.0),
                even_drop_ms: Some(0.1),
                odd_drop_ms: Some(0.5),
            };
            log.amplitude_windows = vec![
                window(start - 1.0, start + 1.0),
                window(start + 2.0, start + 4.0),
                window(end - 1.0, end + 1.0),
                window(42.0, 44.0),
            ];
            let joined = join(&log, &[(start, end), (second_start, second_start + 10.0)]);
            assert_eq!(
                joined.amplitude_windows.len(),
                2,
                "only fully contained windows"
            );
            for (((a, b), w), expected_swap) in [(start, end), (second_start, second_start + 10.0)]
                .into_iter()
                .zip(&joined.amplitude_windows)
                .zip(expected_swaps)
            {
                let input_first = log.beats.iter().find(|x| x.time >= a).unwrap();
                let offset = if a == start {
                    start
                } else {
                    second_start - 15.0
                };
                let output_first = joined
                    .beats
                    .iter()
                    .find(|x| x.time == input_first.time - offset)
                    .unwrap();
                let swapped = (input_first.index - output_first.index) % 2 != 0;
                assert_eq!(
                    swapped, expected_swap,
                    "explicit first/later parity coverage"
                );
                assert_eq!(w.even_deg, Some(if expected_swap { 280.0 } else { 260.0 }));
                assert_eq!(
                    w.beat_error_ms,
                    Some(if expected_swap { -0.4 } else { 0.4 })
                );
                let mut expected = log
                    .amplitude_windows
                    .iter()
                    .find(|w| w.start_s >= a && w.end_s <= b)
                    .unwrap()
                    .clone();
                expected.start_s -= offset;
                expected.end_s -= offset;
                if swapped {
                    expected.swap_sides();
                }
                assert_eq!(
                    serde_json::to_value(w).unwrap(),
                    serde_json::to_value(&expected).unwrap()
                );
                assert_eq!(w.mean(), Some(270.0));
            }
        }
    }

    /// A rate reading over a dropout with no listed break is left out of
    /// the views, as the tests leave it out.
    #[test]
    fn views_leave_out_a_rate_reading_over_a_dropout() {
        let period = 0.125 / (1.0 + 10.0 / 86400.0);
        let beats: Vec<Beat> = (0..1600)
            .map(|index| Beat {
                index,
                time: index as f64 * period,
                quality: 1.0,
            })
            // Nothing heard over the 2 s stretch from 34 s, inside the 10 s
            // reading from 30 s.
            .filter(|b| !(33.9..36.1).contains(&b.time))
            .collect();
        let log = BeatLog {
            sample_rate: 48000,
            duration_s: 200.0,
            bph: 28800,
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
        let at = |g: &Grid, t: f64| g.y[((t - g.t0) / g.step).floor() as usize];
        let raw = steady_views::grids_with_breaks(&log, 10.0, &[]);
        assert!(at(&raw[0], 34.5).is_nan(), "the core grids leave it out");
        let s = compute(&log, None);
        assert!(at(&s.grids[0], 34.5).is_nan());
        assert!(at(&s.grids[0], 54.5).is_finite());
    }

    #[test]
    fn derived_compute_uses_existing_piecewise_analysis() {
        let mut log = context_log();
        // Unequal rates ensure whole-history weighting cannot be replaced by
        // a latest-piece fit. Keep the existing phase step and beat qualities.
        for b in &mut log.beats {
            let rate = if b.index < 480 { 10.0 } else { -20.0 };
            let local = if b.index < 480 {
                b.index
            } else {
                b.index - 480
            };
            b.time = if b.index < 480 { 0.0 } else { 60.03 }
                + local as f64 * 0.125 / (1.0 + rate / 86400.0);
        }
        let lc = LongConfig {
            wheels: default_wheels(log.bph),
            ..Default::default()
        };
        let breaks = [timegrapher_core::breaks::Break {
            audio_s: 60.03,
            gap_s: 0.0,
        }];
        let long = longrun::analyse_with_breaks(&log, None, &breaks, &lc);
        let expected = steadiness::check(&log, None, &long, &lc, &Config::default());
        let actual = compute(&log, None);
        assert_eq!(
            serde_json::to_value(&actual.report).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        let cuts = timegrapher_core::clean::Cuts::new(None, &breaks, &[]).piece_cuts();
        let whole = timegrapher_core::timing::fit_pieces(&log.beats, log.bph, &cuts).unwrap();
        assert!((whole.rate_s_per_day + 5.0).abs() < 0.1);
        assert_eq!(
            long.overall.as_ref().unwrap().rate_s_per_day,
            whole.rate_s_per_day
        );
        assert!(
            (whole.rate_s_per_day + 20.0).abs() > 10.0,
            "not latest piece"
        );
        assert_eq!(
            actual.grids[0].y.len(),
            steady_views::grids_with_breaks(&log, 10.0, &breaks)[0]
                .y
                .len()
        );
    }

    #[test]
    fn the_beat_error_view_gives_sizes_where_its_sign_turns() {
        use timegrapher_core::amplitude::AmplitudeWindow;
        // `after` ms of beat error after the jump at 60.03 s, 0.6 ms before.
        let log_with = |after: f64| {
            let mut log = context_log();
            for b in &mut log.beats {
                let be = if b.index < 480 { 0.6 } else { after };
                let half = be / 2000.0;
                b.time += if b.index % 2 == 1 { half } else { -half };
            }
            log.amplitude_windows = (0..60)
                .map(|i| {
                    let a = i as f64 * 2.0;
                    AmplitudeWindow {
                        start_s: a,
                        end_s: a + 2.0,
                        even_deg: Some(270.0),
                        odd_deg: Some(270.0),
                        beat_error_ms: Some(if a < 60.0 { 0.6 } else { after }),
                        beat_error_unlock_ms: Some(if a < 60.0 { 0.6 } else { after }),
                        even_unlock_ms: None,
                        odd_unlock_ms: None,
                        even_drop_ms: None,
                        odd_drop_ms: None,
                    }
                })
                .collect();
            log
        };
        let finite = |s: &Steadiness| -> Vec<f64> {
            s.grids[2]
                .y
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .collect()
        };
        let turned = compute(&log_with(-0.4), None);
        let be = finite(&turned);
        assert!(be.iter().all(|&v| v > 0.0), "{be:?}");
        assert!(be.iter().any(|&v| (v - 0.4).abs() < 1e-9), "{be:?}");
        // Where the beats' sign agrees either side, the readings keep theirs.
        let mut agree = log_with(0.4);
        for w in &mut agree.amplitude_windows {
            w.beat_error_unlock_ms = w.beat_error_unlock_ms.map(|v| -v);
        }
        let both_slow = compute(&agree, None);
        assert!(finite(&both_slow).iter().any(|&v| v < 0.0));
    }

    /// A watch whose rate swings once a minute (a fourth wheel fault) is
    /// found periodic, and every view draws.
    #[test]
    fn a_once_a_minute_swing_reads_periodic_and_every_view_draws() {
        let cfg = SynthConfig {
            duration_s: 600.0,
            rate_s_per_day: 5.0,
            ..Default::default()
        };
        let audio = generate(
            &cfg,
            |_| 270.0,
            |t| 20.0 * (std::f64::consts::TAU * t / 60.0).sin(),
        );
        let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
        for b in audio.samples.chunks(4800) {
            live.push(b);
        }
        let log = log_of(&live).expect("beat rate");
        let s = compute(&log, None);
        let rate = &s.report.series[0];
        assert!(rate.cycle.is_some(), "{}", rate.headline);
        assert!(s.folds[0].is_some());
        assert_eq!(s.fold_from[0], FoldFrom::Own);
        // The steady amplitude still has a Cycle chart, folded at the
        // rate's cycle (or a wheel's turn if the rate's didn't fit).
        assert!(s.folds[1].is_some());
        assert_ne!(s.fold_from[1], FoldFrom::Own);

        // A picked calibre's train names the cycle instead.
        let named = compute(
            &log,
            Some(vec![Wheel {
                name: "seconds wheel".into(),
                period_s: 60.0,
            }]),
        );
        let wheel = named.report.series[0]
            .cycle
            .as_ref()
            .and_then(|c| c.wheel.clone())
            .or_else(|| {
                named.report.series[0]
                    .period
                    .as_ref()
                    .and_then(|p| p.wheel.clone())
            });
        assert_eq!(wheel.as_deref(), Some("seconds wheel"));

        let ctx = egui::Context::default();
        theme::install(&ctx);
        for v in View::ALL {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 700.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut shown = [true, false, true];
                    series_switches(ui, &mut shown);
                    draw(ui, Some(&s), "test", v, &shown, Opts::default());
                });
            });
            assert!(!out.shapes.is_empty());
        }

        // The map panes draw a map the core gives, with its key, and
        // say what they need when it gives none.
        let drawn = |s: &Steadiness, v: View| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 700.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw(
                        ui,
                        Some(s),
                        "maps",
                        v,
                        &[true, false, false],
                        Opts::default(),
                    );
                });
            });
            fn texts(shape: &egui::Shape, into: &mut String) {
                match shape {
                    egui::Shape::Text(t) => into.push_str(&format!("{}\n", t.galley.text())),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, into)),
                    _ => {}
                }
            }
            let mut all = String::new();
            out.shapes.iter().for_each(|c| texts(&c.shape, &mut all));
            all
        };
        let mut s = s;
        s.maps.clear();
        assert!(drawn(&s, View::Stft).contains("The STFT Map needs"));
        let nan = f64::NAN;
        for method in ["stft", "sst"] {
            s.maps.push(timegrapher_core::timefreq::TimeFrequency {
                series: "rate",
                method,
                unit: "ms",
                step_s: 10.0,
                window_s: None,
                hop_s: Some(300.0),
                period_s: vec![600.0, 120.0, 30.0],
                t_s: vec![600.0, 900.0],
                values: vec![vec![0.1, 0.4, nan], vec![0.2, 0.3, 0.1]],
            });
        }
        for v in [View::Stft, View::Wavelet] {
            let t = drawn(&s, v);
            assert!(t.contains("not read") && !t.contains("needs"), "{v:?}: {t}");
        }
    }

    /// Two wild readings among a hundred steady ones don't set the scale.
    #[test]
    fn a_few_wild_readings_leave_the_scale_alone() {
        let mut pts: Vec<[f64; 2]> = (0..116)
            .map(|i| [i as f64, 10.0 + (i % 7) as f64 * 0.5])
            .collect();
        pts[60][1] = 1200.0;
        pts[62][1] = 1140.0;
        let (lo, hi) = spread(&pts).unwrap();
        assert!(lo > 8.0 && hi < 16.0, "{lo} to {hi}");
        // Two states, a third of the readings each way, keep both.
        let two: Vec<[f64; 2]> = (0..90)
            .map(|i| [i as f64, if i % 3 == 0 { 30.0 } else { 10.0 }])
            .collect();
        let (lo, hi) = spread(&two).unwrap();
        assert!(lo < 10.0 && hi > 30.0, "{lo} to {hi}");
    }
}
