//! The Steadiness pane: how the rate, the amplitude and the beat error
//! each change over the session, from the same tests as
//! `timegrapher series` (core `steadiness`): what was detected, its
//! statistics and the views behind them.
//!
//! The tests need the readings in time order over minutes to hours, so
//! they run over the whole session in the background, again every minute
//! while it grows, and the pane shows the latest answer.

use crate::marks::{self, Rank};
use crate::theme;
use eframe::egui::{self, Color32, RichText, Vec2};
use egui_plot::{
    GridInput, GridMark, HLine, Line, LineStyle, Plot, PlotPoint, Points, Polygon, Text, VLine,
};
use serde::{Deserialize, Serialize};
use timegrapher_core::library::length_text;
use timegrapher_core::live::LiveAnalyzer;
use timegrapher_core::longrun;
use timegrapher_core::longterm::{Fold, Grid, LongConfig, Search};
use timegrapher_core::period_ticks::{period_tick_label, period_ticks};
use timegrapher_core::periodicity::{default_wheels, reference_lines, Wheel};
use timegrapher_core::steadiness::{self, Config, Report, SeriesCheck, SeriesKind};
use timegrapher_core::steady_views::{self, FoldFrom};
use timegrapher_core::stream::BeatLog;

pub use timegrapher_core::joined::{join, joined_duration, MIN_S};

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

/// The breaks a reading of `log` is cut at: those the take lists
/// (`listed`; none for a session heard here), with every jump its beats
/// show as a break of no length. Core's `take::breaks` does the same for
/// a take it opens, and also renumbers the beats across them; a live log
/// is renumbered by the live engine as it goes, so only the cuts are
/// added here.
#[expect(
    clippy::disallowed_methods,
    reason = "a live log: core take::breaks would renumber beats the live engine already numbered"
)]
pub fn reading_breaks(
    listed: Vec<timegrapher_core::breaks::Break>,
    log: &BeatLog,
) -> Vec<timegrapher_core::breaks::Break> {
    timegrapher_core::breaks::with_jumps(listed, &log.jumps)
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
        // The sides an opened take named by the beat count alone, so its
        // readings carry core's caveat; `sided` only feeds that list.
        sided: Vec::new(),
        by_count: live.by_count().to_vec(),
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

/// Run the tests over a session. Slow for long sessions (a 2 h take takes
/// some seconds), so it is run off the window's thread.
/// The tests over `log`, naming cycles after `wheels` (a picked
/// calibre's train) or, without them, the wheels most calibres at the beat
/// rate share, with both usual escape wheels (15 and 20 teeth).
#[cfg(test)]
pub fn compute(log: &BeatLog, wheels: Option<Vec<Wheel>>) -> Steadiness {
    compute_with(
        log,
        &reading_breaks(Vec::new(), log),
        wheels,
        None,
        &std::sync::atomic::AtomicU8::new(0),
    )
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
/// [`stage_text`]) so the panes can say what the tests are doing. A take
/// whose own clock corrects its rate (`clock`: its clock log, or its USB
/// lines) is read on it throughout, its readings drawn as the cards and
/// `timegrapher series` give them. `breaks` are the cuts the readings
/// stop at, with the time each gap held ([`reading_breaks`]), as
/// `series` passes core's.
pub fn compute_with(
    log: &BeatLog,
    breaks: &[timegrapher_core::breaks::Break],
    wheels: Option<Vec<Wheel>>,
    clock: Option<&timegrapher_core::clock::ClockFit>,
    stage: &std::sync::atomic::AtomicU8,
) -> Steadiness {
    use std::sync::atomic::Ordering::Relaxed;
    stage.store(1, Relaxed);
    let lc = LongConfig {
        wheels: wheels.unwrap_or_else(|| default_wheels(log.bph)),
        ..Default::default()
    };
    let cfg = Config::default();
    let long = longrun::analyse_with_breaks(log, clock, breaks, &lc);
    stage.store(2, Relaxed);
    let report = steadiness::check(log, clock, &long, &lc, &cfg);
    stage.store(3, Relaxed);
    // The tests' own grids: a rate reading over a dropout or a jump is left
    // out, and the beat error is given as sizes when its sign turns over
    // across a jump, as the tests beside them and the take's figures give it.
    let grids = match clock {
        // On the take's own clock, as the tests read it.
        Some(_) => steady_views::check_grids(log, &long, cfg.rate_reading_s),
        None => steady_views::grids_with_breaks(log, cfg.rate_reading_s, breaks),
    };
    let searches = steady_views::searches(&grids, &long, &lc);
    let mut fold_from = [FoldFrom::Own, FoldFrom::Own, FoldFrom::Own];
    let folds = steady_views::folds(&grids, &report, &lc);
    let folds = std::array::from_fn(|i| {
        let (fold, from) = folds[i].clone()?;
        fold_from[i] = from;
        Some(fold)
    });
    let period_span = steady_views::period_span(&searches);
    // With the same clock the long-run analysis above was given.
    let maps = timegrapher_core::timefreq::for_take(log, clock, &long, &lc);
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
        length_text(s)
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

/// Grid lines for an axis in dB: every 10 dB, at 0 dB and below.
fn db_grid(input: GridInput) -> Vec<GridMark> {
    let (lo, hi) = input.bounds;
    (0..)
        .map(|k| -10.0 * k as f64)
        .take_while(|v| *v >= lo)
        .filter(|v| *v <= hi)
        .map(|value| GridMark {
            value,
            step_size: 10.0,
        })
        .collect()
}

/// A level in dB as an axis or a key gives it: "0 dB", "−10 dB".
pub fn db_label(v: f64) -> String {
    format!("{:.0} dB", v + 0.0).replace('-', "−")
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
            // With too few readings the row already says so in place of
            // its plot: the panel doesn't say it twice.
            if c.autocorrelation.is_some() {
                ui.add(text(&c.headline, pal.text));
            }
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

/// The two levels a series switches between, dashed across its readings:
/// only where every two-level test holds, never for a fit the tests
/// turned down.
fn state_levels(t: Option<&timegrapher_core::twostate::TwoState>) -> Option<[f64; 2]> {
    t.filter(|t| t.two_levels).map(|t| [t.low, t.high])
}

/// One series' title and plot. Its findings and statistics are in the
/// Info panel.
fn row(ui: &mut egui::Ui, s: &Steadiness, i: usize, c: &SeriesCheck, view: View, opts: Opts) {
    let db = opts.periods_db;
    let pal = theme::pal(ui);
    // A map's key names its series ("Timing Swing (ms)"), so the map
    // takes the title's line for its picture.
    if !view.is_map() {
        ui.label(theme::caption(title(c.series)).color(pal.text_secondary));
    }
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
                .x_axis_formatter(clear_of_edge(plot_w, length_text))
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
                if let Some(levels) = state_levels(c.two_state.as_ref()) {
                    for v in levels {
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
            // In dB the scores are drawn below the tallest, 0 dB, down to
            // a thousandth of it, −30 dB (a score of 0 sits on that floor):
            // a score is a power, so ten times its log.
            let tallest = pts.iter().map(|p| p[1]).fold(search.threshold, f64::max);
            let floor = tallest * 1e-3;
            let y_of = move |v: f64| {
                if db {
                    10.0 * (v.max(floor) / tallest).log10()
                } else {
                    v
                }
            };
            let pts: Vec<[f64; 2]> = pts.iter().map(|q| [q[0], y_of(q[1])]).collect();
            let (y_lo, top) = if db {
                (-30.0, 0.5)
            } else {
                (0.0, tallest * 1.03)
            };
            let threshold = y_of(search.threshold);
            let lines = wheel_lines(marked(s, opts), (x_lo, x_hi));
            let names = WheelNames::reserve(ui, (i, view), opts.wheels);
            // The period found, its dot and note; the note is hover only on
            // a short plot, where the names are inside it on its top.
            let found = c.period.as_ref().map(|per| {
                let x = per.period_s.log10();
                let y = pts
                    .iter()
                    .min_by(|a, b| (a[0] - x).abs().total_cmp(&(b[0] - x).abs()))
                    .map_or(0.0, |q| q[1]);
                let mut t = secs(per.period_s);
                if let Some(w) = &per.wheel {
                    t += &format!(", {}", marks::title_case(w));
                }
                ([x, y], t)
            });
            let short = note_short(ui, &names, (i, view));
            let top = if found.is_some() && !short {
                let hi = pts.iter().map(|q| q[1]).fold(y_lo, f64::max);
                marks::room_above(y_lo, hi, top, ui.available_height() - AXIS_H)
            } else {
                top
            };
            let fill = ui.visuals().extreme_bg_color.gamma_multiply(0.8);
            let mut plot = base
                .height(ui.available_height().max(40.0))
                .x_grid_spacer(period_grid(ui.available_width()))
                .x_axis_formatter(clear_of_edge(plot_w, period_label))
                .default_x_bounds(x_lo, x_hi)
                .default_y_bounds(y_lo, top);
            plot = if db {
                plot.y_grid_spacer(db_grid)
                    .y_axis_formatter(|m, _| db_label(m.value))
            } else {
                plot.y_grid_spacer(|g| theme::even_grid(g, 24.0, &[]))
                    .y_axis_formatter(|m, _| plain(m.value, m.step_size))
            };
            let resp = plot.show(ui, |p| {
                wheel_marks(p, &lines, pal);
                p.hline(
                    HLine::new("threshold", threshold)
                        .color(pal.text_secondary)
                        .style(dashed)
                        .width(1.0_f32),
                );
                // Over the lines, under the trace.
                if let Some(n) = &names {
                    n.inside(p, &lines, &pts, fill);
                }
                p.line(Line::new("score", pts.clone()).color(colour).width(1.5_f32));
                if let Some((at, _)) = &found {
                    p.points(Points::new("peak", vec![*at]).color(colour).radius(3.5_f32));
                }
            });
            if let Some(n) = &names {
                n.draw(ui, &resp.transform, &lines, &pts);
            }
            if let Some((at, t)) = &found {
                let id = ui.id().with(("point note", i, view));
                marks::beside(ui, &resp.transform, *at, t, &pts, &[], fill, short, id);
            }
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
                        t += &format!(", {}", marks::title_case(w));
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
                    marks::title_case(w).to_lowercase(),
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
            let short = note_short(ui, &None, (i, view));
            let fill = ui.visuals().extreme_bg_color.gamma_multiply(0.8);
            let ljung = format!("Ljung–Box p = {} ", steadiness::p_text(a.p_value));
            let resp = base
                .x_grid_spacer(|g| theme::even_grid(g, 80.0, &theme::TIME_STEPS))
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
                    p.line(Line::new("r", pts.clone()).color(colour).width(1.5_f32));
                    if let (Some(l), Some(r)) = (a.repeat_lag_s, a.repeat_r) {
                        p.points(
                            Points::new("repeat", vec![[l, r]])
                                .color(colour)
                                .radius(3.5_f32),
                        );
                    }
                    p.text(
                        Text::new("ljung box", PlotPoint::new(x_hi, 1.35), ljung.clone())
                            .color(pal.text_secondary)
                            .anchor(egui::Align2::RIGHT_TOP),
                    );
                });
            if let (Some(l), Some(r)) = (a.repeat_lag_s, a.repeat_r) {
                // The Ljung–Box note's place, kept clear of the repeat's.
                let t = &resp.transform;
                let font = egui::TextStyle::Small.resolve(ui.style());
                let w = ui.fonts_mut(|f| f.layout_no_wrap(ljung, font, pal.text_secondary));
                let corner = t.position_from_point(&PlotPoint::new(x_hi, 1.35));
                let ljung =
                    egui::Rect::from_min_size(corner - egui::vec2(w.size().x, 0.0), w.size());
                let note = format!("repeats at {}", secs(l));
                let id = ui.id().with(("point note", i, view));
                marks::beside(ui, t, [l, r], &note, &pts, &[ljung], fill, short, id);
            }
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
                .x_axis_formatter(clear_of_edge(plot_w, length_text))
                .y_grid_spacer(|g| theme::even_grid(g, 24.0, &[]))
                .y_axis_formatter(|m, _| plain(m.value, m.step_size))
                .default_y_bounds(-m, m)
                .show(ui, |p| {
                    p.hline(HLine::new("zero", 0.0).color(weak).width(1.0_f32));
                    for b in [cu.band, -cu.band] {
                        p.hline(
                            HLine::new("95%", b)
                                .color(pal.text_secondary)
                                .style(dashed)
                                .width(1.0_f32),
                        );
                    }
                    p.line(Line::new("cusum", pts.clone()).color(colour).width(1.5_f32));
                    if cu.shift {
                        let y = cu.at;
                        p.points(
                            Points::new("corner", vec![[cu.at_s, y]])
                                .color(colour)
                                .radius(3.5_f32),
                        );
                        mark_label(
                            p,
                            [cu.at_s, y],
                            format!("level changes near {}", length_text(cu.at_s)),
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
                opts.maps_db,
                marked(s, opts),
                length_text,
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
            let top = y_hi + (y_hi - y_lo).max(0.5) * 0.05;
            let unit = match sp.unit {
                _ if as_rate => " s/d",
                "deg" => "°",
                _ => " ms",
            };
            let size =
                move |v: f64| format!("{}{unit}", timegrapher_core::timefreq::size_number(v));
            let peak = tallest_peak(&pts);
            let lines = wheel_lines(marked(s, opts), (x_lo, x_hi));
            let names = WheelNames::reserve(ui, (i, view), opts.wheels);
            let fill = ui.visuals().extreme_bg_color.gamma_multiply(0.8);
            let tops = pts.clone();
            // The tallest peak's dot and note; the note is hover only on a
            // short plot, where the names are inside it on its top.
            let tallest = peak.map(|peak| {
                (
                    peak,
                    format!(
                        "tallest peak at {}: {}{}",
                        secs(10f64.powf(peak[0])),
                        size(10f64.powf(peak[1])),
                        if as_rate { " peak to peak" } else { "" }
                    ),
                )
            });
            let short = note_short(ui, &names, (i, view));
            let top = if tallest.is_some() && !short {
                let hi = pts.iter().map(|q| q[1]).fold(y_lo, f64::max);
                marks::room_above(y_lo, hi, top, ui.available_height() - AXIS_H)
            } else {
                top
            };
            let resp = base
                .height(ui.available_height().max(40.0))
                .x_grid_spacer(period_grid(ui.available_width()))
                .x_axis_formatter(clear_of_edge(plot_w, period_label))
                .y_grid_spacer(log_grid)
                .y_axis_formatter(move |m, r| log_label(m, r, size))
                .default_x_bounds(x_lo, x_hi)
                .default_y_bounds(y_lo, top)
                .show(ui, |p| {
                    wheel_marks(p, &lines, pal);
                    if let Some(n) = &names {
                        n.inside(p, &lines, &tops, fill);
                    }
                    p.line(Line::new("spectrum", pts).color(colour).width(1.5_f32));
                    if let Some((at, _)) = &tallest {
                        p.points(
                            Points::new("tallest", vec![*at])
                                .color(colour)
                                .radius(3.5_f32),
                        );
                    }
                });
            if let Some(n) = &names {
                n.draw(ui, &resp.transform, &lines, &tops);
            }
            if let Some((at, t)) = &tallest {
                let id = ui.id().with(("point note", i, view));
                marks::beside(ui, &resp.transform, *at, t, &tops, &[], fill, short, id);
            }
        }
    }
}

/// How the views draw what they can draw two ways.
#[derive(Clone, Copy, Default)]
pub struct Opts {
    /// Periods: the scores in dB below the tallest.
    pub periods_db: bool,
    /// FFT: rate as the beat timing's spectrum in ms, not its swing in
    /// the rate readings.
    pub fft_timing: bool,
    /// STFT and wavelet maps: the sizes' colours in dB.
    pub maps_db: bool,
    /// The calibre's wheels marked on the period axis, or the reference
    /// periods when no wheel is known.
    pub wheels: bool,
    /// The Info panel, beside the plots: one setting for every view.
    pub info: bool,
}

/// What Wheel Periods does, for its hint.
pub const WHEELS_HINT: &str = "Mark the period each of the calibre's wheels turns in, in \
     a grey dotted line named above the plot, and half of it fainter, where a wheel that is off centre \
     shows twice a turn. With no calibre picked, or none of its wheels known, one minute and \
     one hour are marked in grey instead: periods alone, named after no wheel.";

/// The wheels to mark: `None` when the marks are off.
fn marked(s: &Steadiness, opts: Opts) -> Option<&[Wheel]> {
    opts.wheels.then_some(&s.wheels[..])
}

/// What a line on the period axis marks: a wheel's turn, half of it (a
/// wheel off centre shows twice a turn), or a reference period.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Turn {
    Wheel,
    Twice,
    Reference,
}

/// A line on the period axis (log10 seconds), its name, its short name
/// (for the narrow lane beside a map) and what its hover says.
pub(crate) struct WheelLine {
    pub x: f64,
    pub turn: Turn,
    pub name: String,
    pub short: String,
    pub hover: String,
}

impl WheelLine {
    /// A wheel's name is strong, a ×2 or a reference faint.
    pub fn rank(&self) -> Rank {
        if self.turn == Turn::Wheel {
            Rank::Strong
        } else {
            Rank::Faint
        }
    }

    /// Its line's colour: the wheels' grey, a ×2 fainter, a reference in
    /// the tertiary grey.
    pub fn ink(&self, pal: &theme::Palette) -> Color32 {
        match self.turn {
            Turn::Wheel => pal.wheel,
            Turn::Twice => pal.wheel.gamma_multiply(0.6),
            Turn::Reference => pal.text_tertiary,
        }
    }
}

/// The lines to mark on a period axis spanning `span`: each wheel's turn
/// and half of it ("×2", where a wheel that is off centre shows twice a
/// turn, unless another wheel turns there); with no wheels, the
/// reference periods; with `None` (the marks off), nothing.
pub(crate) fn wheel_lines(wheels: Option<&[Wheel]>, (x_lo, x_hi): (f64, f64)) -> Vec<WheelLine> {
    let inside = |x: f64| x > x_lo && x < x_hi;
    let Some(wheels) = wheels else {
        return Vec::new();
    };
    if wheels.is_empty() {
        return reference_lines()
            .into_iter()
            .map(|r| WheelLine {
                x: r.period_s.log10(),
                turn: Turn::Reference,
                name: r.label.to_string(),
                short: r.label.to_string(),
                hover: r.label.to_string(),
            })
            .filter(|l| inside(l.x))
            .collect();
    }
    let mut out = Vec::new();
    for w in wheels {
        let title = marks::title_case(&w.name);
        let short = marks::wheel_short(&w.name);
        out.push(WheelLine {
            x: w.period_s.log10(),
            turn: Turn::Wheel,
            name: title.clone(),
            short: short.clone(),
            hover: format!("{title}: {}", secs(w.period_s)),
        });
        let half = w.period_s / 2.0;
        if !wheels
            .iter()
            .any(|o| (o.period_s / half - 1.0).abs() < 0.03)
        {
            out.push(WheelLine {
                x: half.log10(),
                turn: Turn::Twice,
                name: format!("{title} ×2"),
                short: format!("{short} ×2"),
                hover: format!("{title} ×2: {}", secs(half)),
            });
        }
    }
    out.retain(|l| inside(l.x));
    out
}

/// The lines in the plot, under its trace: a wheel's dotted, a ×2's
/// looser and fainter, a reference's solid. Their names are drawn by
/// [`wheel_names`].
fn wheel_marks(p: &mut egui_plot::PlotUi<'_>, lines: &[WheelLine], pal: &theme::Palette) {
    for l in lines {
        let v = VLine::new(l.name.clone(), l.x).color(l.ink(pal));
        p.vline(match l.turn {
            Turn::Wheel => v.style(LineStyle::dotted_dense()).width(2.0_f32),
            Turn::Twice => v.style(LineStyle::dotted_loose()).width(1.25_f32),
            Turn::Reference => v.width(1.25_f32),
        });
    }
}

/// The room a plot's x axis takes under it, points: near enough to tell
/// its frame's height from the height it is given.
const AXIS_H: f32 = 20.0;

/// Whether a plot's point note is hover only: on a short plot, as the
/// wheels' `names` judged it where they are on, else by the plot's own
/// height inside its x axis. Call before the plot is laid out.
fn note_short(ui: &egui::Ui, names: &Option<WheelNames>, id: impl std::hash::Hash) -> bool {
    names.as_ref().map_or_else(
        || {
            let h = ui.available_height() - AXIS_H;
            marks::short(ui, ui.id().with(("note short", id)), h)
        },
        |n| n.short,
    )
}

/// Where the wheels' names go on a period plot, by the rules every named
/// line follows ([`marks`]): in a lane of two rows above the plot, or
/// inside along its top where the plot is short.
struct WheelNames {
    id: egui::Id,
    short: bool,
}

impl WheelNames {
    /// Room for the names above a plot, kept whenever the marks are on so
    /// the plot never jumps; on a plot that would be short with it, none,
    /// the names going inside. Call before the plot is laid out.
    fn reserve(ui: &mut egui::Ui, id: impl std::hash::Hash, on: bool) -> Option<Self> {
        if !on {
            return None;
        }
        let id = ui.id().with(("wheel names", id));
        let room = marks::LANE + marks::LANE_GAP;
        let short = marks::short(ui, id.with("short"), ui.available_height() - room);
        if !short {
            ui.add_space(room);
        }
        Some(Self { id, short })
    }

    /// The names inside a short plot, as an item of it: added after the
    /// lines and before the trace (`tops`).
    fn inside(
        &self,
        p: &mut egui_plot::PlotUi<'_>,
        lines: &[WheelLine],
        tops: &[[f64; 2]],
        fill: Color32,
    ) {
        if self.short {
            p.add(marks::InsideNames {
                base: egui_plot::PlotItemBase::new("wheel names".into()),
                names: names_of(lines),
                tops: tops.to_vec(),
                fill,
            });
        }
    }

    /// The names in the lane, or the hovers of those inside, once the plot
    /// (`t`) is drawn.
    fn draw(
        &self,
        ui: &egui::Ui,
        t: &egui_plot::PlotTransform,
        lines: &[WheelLine],
        tops: &[[f64; 2]],
    ) {
        let frame = *t.frame();
        if self.short {
            let placed = marks::inside_placed(ui, t, &names_of(lines), tops);
            let hover_of = |name: &str| {
                lines
                    .iter()
                    .find(|l| l.name == name)
                    .map_or_else(String::new, |l| l.hover.clone())
            };
            marks::inside_hovers(ui, frame, &placed, &hover_of, self.id);
        } else {
            let pal = theme::pal(ui);
            let named: Vec<marks::Named> = lines
                .iter()
                .map(|l| marks::Named {
                    x: t.position_from_point_x(l.x),
                    name: l.name.clone(),
                    rank: l.rank(),
                    ink: l.ink(pal),
                    hover: l.hover.clone(),
                })
                .collect();
            marks::lane_above(ui, frame, &named, self.id);
        }
    }
}

/// Each line's place, name and rank, for [`marks`].
fn names_of(lines: &[WheelLine]) -> Vec<(f64, String, Rank)> {
    lines
        .iter()
        .map(|l| (l.x, l.name.clone(), l.rank()))
        .collect()
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
#[path = "steady_tests.rs"]
mod tests;
