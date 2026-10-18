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
    GridInput, GridMark, HLine, Line, LineStyle, Plot, PlotPoint, Points, Text, VLine,
};
use serde::{Deserialize, Serialize};
use timegrapher_core::live::LiveAnalyzer;
use timegrapher_core::longrun;
use timegrapher_core::longterm::{Fold, Grid, LongConfig, Search};
use timegrapher_core::periodicity::{default_wheels, Wheel};
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
}

impl View {
    #[cfg(test)]
    const ALL: [View; 7] = [
        View::Readings,
        View::Periods,
        View::Cycle,
        View::Autocorrelation,
        View::Allan,
        View::Cusum,
        View::Fft,
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
                 a peak above the dashed line is a real cycle, labelled with the wheel whose \
                 turn it matches"
            }
            View::Cycle => {
                "The readings folded at the cycle the tests found: every cycle's readings as \
                 dots, and their median shape as the line"
            }
            View::Autocorrelation => {
                "How much each reading resembles the one a lag later: independent readings \
                 stay inside the dashed band; a rise back to a peak means they repeat"
            }
            View::Allan => {
                "How much the rate's average scatters when averaged over longer and longer \
                 times; lower is steadier. On or below the dashed line, the rate only jitters \
                 from moment to moment; above it towards the right, it wanders or drifts, and a \
                 cycle puts a bump near half its period. Both axes are logarithmic"
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
    /// Seconds of the session the tests covered.
    pub upto_s: f64,
}

/// The session's beats and amplitude windows so far, as a recording's
/// analysis would give them.
pub fn log_of(live: &LiveAnalyzer) -> Option<BeatLog> {
    Some(BeatLog {
        sample_rate: live.sample_rate(),
        duration_s: live.duration_s(),
        bph: live.bph()?,
        lift_deg: live.config().analysis.amplitude.lift_deg,
        beats: live.beats().to_vec(),
        amplitude_windows: live.amplitude_windows().to_vec(),
        clipped_beats: live.clipped_beats().to_vec(),
        // Not counted live; the tests don't use it.
        clipped_samples: 0,
        // Imported jumps and current position/settling boundaries are all
        // known cuts, not extra elapsed time or newly detected jumps.
        jumps: live.breaks().to_vec(),
    })
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
    Steadiness {
        report,
        grids,
        searches,
        folds,
        fold_from,
        period_span,
        wheels: lc.wheels,
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
    let mut parts = vec![format!("{} readings", c.readings)];
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
        parts.push(format!("Ljung–Box p {:.3}", a.p_value));
    }
    if let Some(t) = &c.trend {
        parts.push(format!("trend {} {u}/h", signed(t.per_hour)));
    }
    if let Some(p) = &c.period {
        parts.push(format!(
            "period {:.0} s, peak to peak {} {u}",
            p.period_s,
            n(p.peak_to_peak)
        ));
    }
    parts.join(" · ")
}

fn series_colour(k: SeriesKind, pal: &theme::Palette) -> Color32 {
    match k {
        SeriesKind::Rate => pal.trace_rate,
        SeriesKind::Amplitude => pal.trace_amplitude,
        SeriesKind::BeatError => pal.trace_beat_error,
    }
}

fn title(k: SeriesKind) -> &'static str {
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

/// The switches for which series the pane shows, for its toolbar. At
/// least one stays on.
pub fn series_switches(ui: &mut egui::Ui, shown: &mut [bool; 3]) {
    for (i, k) in SERIES.into_iter().enumerate() {
        let others = shown.iter().enumerate().any(|(j, &on)| j != i && on);
        let mut on = shown[i];
        if theme::toggle(ui, title(k), &mut on)
            .on_hover_text(format!(
                "Show the {} series. Turning the others off gives this one the whole pane.",
                title(k).to_lowercase()
            ))
            .changed()
            && (on || others)
        {
            shown[i] = on;
        }
    }
}

/// Draw the pane: a note on what was tested, then a row for each series
/// shown with what was detected, its statistics and the chosen view behind
/// it.
pub fn draw(
    ui: &mut egui::Ui,
    s: Option<&Steadiness>,
    status: &str,
    view: View,
    shown: &[bool; 3],
    opts: Opts,
) {
    let pal = theme::pal(ui);
    if !status.is_empty() {
        ui.label(RichText::new(status).small().color(pal.text_secondary));
    }
    let Some(s) = s else {
        return;
    };
    // What the view shows, in a line; all of it on hover.
    let hint = format!("{}: {}.", view.label(), view.hint());
    ui.add(egui::Label::new(RichText::new(&hint).small().color(pal.text_tertiary)).truncate())
        .on_hover_text(hint);
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
                        row(ui, s, i, c, view, opts, None);
                    });
                    if k + 1 < n {
                        ui.add_space(gap);
                    }
                }
            });
    } else {
        // Wide and short: side by side.
        let col_w = fits(n, w);
        // Every headline in the same room, so the plots line up.
        let rows = series
            .iter()
            .map(|(_, c)| headline(ui, &c.headline, col_w, HEADLINE_ROWS).rows.len())
            .max()
            .unwrap_or(1);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for &(i, c) in &series {
                ui.allocate_ui(Vec2::new(col_w, h), |ui| {
                    ui.set_width(col_w);
                    ui.set_min_height(h);
                    ui.vertical(|ui| row(ui, s, i, c, view, opts, Some(rows)));
                });
            }
        });
    }
}

/// The most lines a headline takes before it ends in "…"; the whole of it
/// is on hover.
const HEADLINE_ROWS: usize = 8;

/// A headline laid out `width` wide on up to `max_rows` lines.
fn headline(
    ui: &egui::Ui,
    text: &str,
    width: f32,
    max_rows: usize,
) -> std::sync::Arc<egui::Galley> {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(font, theme::pal(ui).text_secondary),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: width,
        max_rows,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    ui.fonts_mut(|f| f.layout_job(job))
}

/// One series' headline, statistics and plot. Side by side, `rows` gives
/// every headline the same room so the plots line up; one above another,
/// each takes the lines it needs.
fn row(
    ui: &mut egui::Ui,
    s: &Steadiness,
    i: usize,
    c: &SeriesCheck,
    view: View,
    opts: Opts,
    rows: Option<usize>,
) {
    let log_y = opts.periods_log;
    let pal = theme::pal(ui);
    ui.label(theme::caption(title(c.series)).color(pal.text_secondary));
    // What was detected, in full unless it is very long; all of it on
    // hover, with the statistics behind it.
    let max_rows = rows.unwrap_or(HEADLINE_ROWS).max(1);
    let galley = headline(ui, &c.headline, ui.available_width(), max_rows);
    let short_by = rows.map_or(0, |r| r.saturating_sub(galley.rows.len().max(1)));
    let line_h = galley.size().y / galley.rows.len().max(1) as f32;
    let stats = stats_line(c);
    ui.add(egui::Label::new(galley))
        .on_hover_text(format!("{}\n\n{stats}", c.headline));
    // Side by side, the same room for each, so the plots line up.
    if short_by > 0 {
        ui.add_space(line_h * short_by as f32);
    }
    // Too few readings: no tests ran, so there is nothing to draw.
    if c.autocorrelation.is_none() {
        return;
    }
    ui.add(egui::Label::new(RichText::new(&stats).small().color(pal.text_tertiary)).truncate())
        .on_hover_text(&stats);
    let k = c.series;
    let colour = series_colour(k, pal);
    let weak = pal.text_tertiary;
    let id = ("steadiness", i, view);
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
                .x_axis_formatter(|m, _| strip::fmt_time(m.value))
                .y_grid_spacer(|g| theme::even_grid(g, 24.0, &[]))
                .y_axis_formatter(move |m, _| value_text(k, m.value, m.step_size));
            if let Some((lo, hi)) = range {
                plot = plot.default_y_bounds(lo, hi);
            }
            plot.show(ui, |p| {
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
                .x_grid_spacer(log_grid)
                .x_axis_formatter(|m, r| log_label(m, r, secs))
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
                wheel_marks(p, &s.wheels, (x_lo, x_hi), label_y, pal);
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
                return note(ui, "No cycle found: nothing repeats in these readings.");
            };
            let bins = f.profile.len();
            let x = |b: usize| (b as f64 + 0.5) / bins as f64 * f.period_s;
            let dots: Vec<[f64; 2]> = f
                .raster
                .iter()
                .flat_map(|r| {
                    r.iter()
                        .enumerate()
                        .filter(|(_, v)| v.is_finite())
                        .map(|(b, v)| [x(b), *v])
                        .collect::<Vec<_>>()
                })
                .collect();
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
                .x_axis_formatter(|m, _| lag_text(m.value))
                .y_grid_spacer(|g| theme::even_grid(g, 24.0, &[]))
                .y_axis_formatter(move |m, _| value_text(k, m.value, m.step_size))
                .default_x_bounds(0.0, f.period_s);
            if let Some((lo, hi)) = range {
                plot = plot.default_y_bounds(lo, hi);
            }
            let label = match &s.fold_from[i] {
                FoldFrom::Own => {
                    let mut t = format!("{} cycle", secs(f.period_s));
                    if let Some(w) = c
                        .cycle
                        .as_ref()
                        .and_then(|c| c.wheel.as_ref())
                        .or(c.period.as_ref().and_then(|p| p.wheel.as_ref()))
                    {
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
            plot.show(ui, |p| {
                p.points(Points::new("cycles", dots).color(weak).radius(1.2_f32));
                p.line(Line::new("shape", shape).color(colour).width(2.0_f32));
                let b = p.plot_bounds();
                p.text(
                    Text::new(
                        "cycle label",
                        PlotPoint::new(b.max()[0], b.min()[1]),
                        format!("{label} "),
                    )
                    .color(pal.text_secondary)
                    .anchor(egui::Align2::RIGHT_BOTTOM),
                );
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
                .x_axis_formatter(|m, _| lag_text(m.value))
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
            base.x_grid_spacer(log_grid)
                .x_axis_formatter(|m, r| log_label(m, r, secs))
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
                .x_axis_formatter(|m, _| strip::fmt_time(m.value))
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
            let as_rate = sp.series == "rate" && sp.unit == "ms" && !opts.fft_timing;
            let reading_s = Config::default().rate_reading_s;
            let pts: Vec<[f64; 2]> = sp
                .period_s
                .iter()
                .zip(&sp.amplitude)
                .filter(|(&p, _)| !as_rate || p >= 2.0 * reading_s)
                .map(|(&p, &a)| {
                    (
                        p,
                        if as_rate {
                            reading_swing(a, p, reading_s)
                        } else {
                            a
                        },
                    )
                })
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
            let peak = pts
                .iter()
                .copied()
                .max_by(|a, b| a[1].total_cmp(&b[1]))
                .unwrap_or([0.0, 0.0]);
            base.x_grid_spacer(log_grid)
                .x_axis_formatter(|m, r| log_label(m, r, secs))
                .y_grid_spacer(log_grid)
                .y_axis_formatter(move |m, r| log_label(m, r, size))
                .default_x_bounds(x_lo, x_hi)
                .default_y_bounds(y_lo, top)
                .show(ui, |p| {
                    wheel_marks(p, &s.wheels, (x_lo, x_hi), top, pal);
                    p.line(Line::new("spectrum", pts).color(colour).width(1.5_f32));
                    p.points(
                        Points::new("tallest", vec![peak])
                            .color(colour)
                            .radius(3.5_f32),
                    );
                    mark_label(
                        p,
                        peak,
                        format!(
                            "tallest at {}: {}{}",
                            secs(10f64.powf(peak[0])),
                            size(10f64.powf(peak[1])),
                            if as_rate { " peak to peak" } else { "" }
                        ),
                        pal.text,
                        false,
                    );
                });
        }
    }
}

/// The peak-to-peak swing, s/d, that a beat-timing sine `amp_ms` in size
/// with period `period_s` makes in rate readings each averaged over
/// `reading_s`: the timing's slope, `2π·amp/period` ms per second, is
/// 86.4 s/d for each ms per second, and averaging over a reading keeps
/// `sin(x)/x` of it, `x = π·reading/period`.
fn reading_swing(amp_ms: f64, period_s: f64, reading_s: f64) -> f64 {
    let x = std::f64::consts::PI * reading_s / period_s;
    let kept = if x == 0.0 { 1.0 } else { (x.sin() / x).abs() };
    2.0 * amp_ms * std::f64::consts::TAU / period_s * 86.4 * kept
}

/// How the views draw what they can draw two ways.
#[derive(Clone, Copy, Default)]
pub struct Opts {
    /// Periods: the scores on a log scale.
    pub periods_log: bool,
    /// FFT: rate as the beat timing's spectrum in ms, not its swing in
    /// the rate readings.
    pub fft_timing: bool,
}

/// The wheels' turns on a log10 period axis spanning `span`: a bold line
/// and name at each wheel's period, and a fainter dashed one at half of
/// it ("×2"), where a wheel that is off centre shows twice a turn. Names
/// sit at height `label_y`.
fn wheel_marks(
    p: &mut egui_plot::PlotUi<'_>,
    wheels: &[Wheel],
    (x_lo, x_hi): (f64, f64),
    label_y: f64,
    pal: &theme::Palette,
) {
    let inside = |x: f64| x > x_lo && x < x_hi;
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
                    .color(pal.text_secondary)
                    .width(2.0_f32),
            );
            p.text(
                Text::new("wheel", PlotPoint::new(x, label_y), wheel_title(&w.name))
                    .color(pal.text)
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
                    .color(pal.text_tertiary)
                    .style(LineStyle::Dashed { length: 4.0 })
                    .width(1.25_f32),
            );
            p.text(
                Text::new(
                    "wheel ×2",
                    PlotPoint::new(x2, label_y),
                    format!("{} ×2", wheel_title(&w.name)),
                )
                .color(pal.text_secondary)
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
    use super::*;
    use timegrapher_core::live::LiveConfig;
    use timegrapher_core::synth::{generate, SynthConfig};

    #[test]
    fn a_long_headline_is_shown_whole() {
        // A headline ending on a line faster than the readings, at the
        // Steadiness tab's width in a 1280-point window.
        let text = "Rate repeats every 90 s, 1.1 s/d peak to peak, 0.4 s/d \
            above the scatter. The rate's level also moves 0.8 s/d over the \
            take. The beat timing also repeats every 5.0 s, faster than the \
            10 s readings show: Escape Wheel, 0.12 ms peak to peak, in the \
            beat timing itself.";
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut rows = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let g = headline(ui, text, 415.0, HEADLINE_ROWS);
                let shown: String = g.rows.iter().map(|r| r.text()).collect();
                assert!(!shown.contains('…'), "{shown}");
                rows = Some(g.rows.len());
            });
        });
        let rows = rows.unwrap();
        assert!(rows > 3 && rows < HEADLINE_ROWS, "{rows} rows");
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
        let cuts = longrun::cuts(None, &breaks);
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
    fn a_timing_wave_is_drawn_as_the_swing_in_rate_readings() {
        // 0.157 ms of timing at 46 s is 3.7 s/d peak to peak in the rate
        // itself; a 10 s reading keeps sin(x)/x of it, x = π·10/46.
        let x = std::f64::consts::PI * 10.0 / 46.0;
        let swing = reading_swing(0.157, 46.0, 10.0);
        assert!((swing - 3.70 * x.sin() / x).abs() < 0.02, "{swing}");
        // A wave as long as a reading averages away.
        assert!(reading_swing(1.0, 10.0, 10.0) < 1e-9);
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
