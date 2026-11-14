//! The Steadiness pane: how the rate, the amplitude and the beat error
//! each change over the session, from the same tests as
//! `timegrapher series` (core `steadiness`): what was detected, its
//! statistics and the views behind them.
//!
//! The tests need the readings in time order over minutes to hours, so
//! they run over the whole session in the background, again every minute
//! while it grows, and the pane shows the latest answer.

use crate::marks::{self, Rank};
use crate::strength::{self, Strength, DATA_W};
use crate::theme;
use eframe::egui::{self, Color32, RichText, Vec2};
use egui_plot::{
    Bar, BarChart, GridInput, GridMark, HLine, Line, LineStyle, Plot, PlotPoint, Points, Polygon,
    Text, VLine,
};
use serde::{Deserialize, Serialize};
use timegrapher_core::figure_text::{figure_minus, true_minus};
use timegrapher_core::library::length_text;
use timegrapher_core::live::LiveAnalyzer;
use timegrapher_core::longrun;
use timegrapher_core::longterm::{Fold, Grid, LongConfig, Search};
use timegrapher_core::period_ticks::{self, period_tick_label, period_ticks};
use timegrapher_core::periodicity::{default_wheels, reference_lines, Wheel};
use timegrapher_core::steadiness::{self, Config, Report, SeriesCheck, SeriesKind};
use timegrapher_core::steady_views::{self, FoldFrom};
use timegrapher_core::stream::BeatLog;

pub use timegrapher_core::joined::{join, joined_duration, MIN_S};

/// How much the session grows before the tests run again, seconds.
pub const RERUN_S: f64 = 60.0;
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

/// A switch in a Steadiness pane's toolbar: each pane keeps its own,
/// and a pane not switched yet takes `unset`, the one value every pane
/// shared before each kept its own.
#[derive(Debug, Clone, PartialEq)]
pub struct PerPane {
    pub set: std::collections::BTreeMap<View, bool>,
    pub unset: bool,
}

impl PerPane {
    pub fn new(set: Vec<(View, bool)>, unset: bool) -> Self {
        PerPane {
            set: set.into_iter().collect(),
            unset,
        }
    }

    /// Whether the switch is on in `view`'s pane.
    pub fn on(&self, view: View) -> bool {
        self.set.get(&view).copied().unwrap_or(self.unset)
    }

    /// Switch `view`'s pane to `on`. Only a change is noted, so the panes
    /// never switched keep the default.
    pub fn set(&mut self, view: View, on: bool) {
        if on != self.on(view) {
            self.set.insert(view, on);
        }
    }

    /// The panes switched, as settings keep them.
    pub fn saved(&self) -> Vec<(View, bool)> {
        self.set.iter().map(|(v, on)| (*v, *on)).collect()
    }
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
                "The running sum of each reading's distance from the mean, scaled by their \
                 long-run spread: readings with no change of level, correlated or not, stay \
                 inside the dashed lines about 95 times in 100; a step in the level bends it \
                 into a V or a tent. The corner is marked only where the headline names \
                 a level change"
            }
            View::Fft => {
                "The plain amplitude spectrum of each series, like Witschi's FFT: the size \
                 estimated at each period for complete data, the rate's as the swing peak to peak it \
                 makes in 10 s rate readings (or the beat timing's, in ms, with Timing), \
                 with the wheels' periods marked and twice a turn more loosely dotted. Both axes are \
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
    /// The calibre's meshes, marked beside its wheels; set by whoever
    /// knows the calibre, after the tests run.
    pub meshes: Vec<timegrapher_core::train::MeshLine>,
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
    log_masked(live, live.masked())
}

/// [`log_of`], with only the stretches `masked` left out of it: a long
/// take's run reads its log with the masks by hand alone, so a settling
/// culled from its hours never moves where it starts (#1233).
pub fn log_masked(live: &LiveAnalyzer, masked: &[(f64, f64)]) -> Option<BeatLog> {
    log_masked_at(live, masked, live.bph())
}

/// [`log_of`] at `bph`, when the session's rate is no longer locked: a
/// Power Reserve Test reads its hours at the rate it ran at, though the
/// silence after the watch stopped let an automatic rate go (#1394).
pub fn log_of_at(live: &LiveAnalyzer, bph: Option<u32>) -> Option<BeatLog> {
    log_masked_at(live, live.masked(), bph)
}

fn log_masked_at(live: &LiveAnalyzer, masked: &[(f64, f64)], bph: Option<u32>) -> Option<BeatLog> {
    let mut log = BeatLog {
        sample_rate: live.sample_rate(),
        duration_s: live.duration_s(),
        bph: bph?,
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
        // The sound live still keeps, so the whole take judges a stretch
        // in timing's doubt zone by its beat sound as session does.
        snippets: live.snippets(),
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
        meshes: Vec::new(),
        maps,
        upto_s: log.duration_s,
    }
}

/// The statistics behind a series' headline, on one line, as core writes
/// it for every screen ([`SeriesCheck::stats_line`], with a true minus).
/// Read fresh, not from `screen_stats`, so a check changed after it was
/// made (as tests do) says what it holds.
fn stats_line(c: &SeriesCheck) -> String {
    figure_minus(&c.stats_line())
}

pub(crate) fn series_colour(k: SeriesKind, pal: &theme::Palette) -> Color32 {
    match k {
        SeriesKind::Rate => pal.trace_rate,
        SeriesKind::Amplitude => pal.trace_amplitude,
        SeriesKind::BeatError => pal.trace_beat_error,
    }
}

/// A reading's name, as its series, chart, distribution and switch show it.
pub fn title(k: SeriesKind) -> &'static str {
    k.title()
}

/// A value in a series' units for an axis or a note.
fn value_text(k: SeriesKind, v: f64, step: f64) -> String {
    let v = true_minus(&timegrapher_core::chart_scale::axis_number(v, step));
    match k {
        SeriesKind::Rate => format!("{v} s/d"),
        SeriesKind::Amplitude => format!("{v}°"),
        SeriesKind::BeatError => format!("{v} ms"),
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
    let room = (width / period_ticks::MARK_ROOM_PX as f32).max(2.0) as usize;
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

/// Grid lines for a time axis, about 80 points apart at round times.
pub fn time_grid(input: GridInput) -> Vec<GridMark> {
    theme::even_grid(input, 80.0, &theme::TIME_STEPS)
}

/// Grid lines for a value axis, about 24 points apart.
fn even_grid(input: GridInput) -> Vec<GridMark> {
    theme::even_grid(input, 24.0, &[])
}

/// The grid of a series' readings over time: rate and beat error have a
/// zero line (0 s/d, 0 ms), amplitude none.
fn value_grid(k: SeriesKind) -> strength::Grid {
    let grid = strength::Grid::new(time_grid, even_grid);
    if k == SeriesKind::Amplitude {
        grid
    } else {
        grid.zero_y()
    }
}

/// How strongly the cycle plot's boxes are drawn: the shape's colour at
/// this opacity, so the shape stays on top.
const BOX_OPACITY: f32 = 0.6;

/// A level in dB as an axis or a key gives it: "0 dB", "−10 dB".
pub fn db_label(v: f64) -> String {
    true_minus(&format!("{:.0} dB", v + 0.0))
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
    // Where Info over the rows starts, in a pane too narrow for it beside
    // them: the readouts stop there (#1580).
    let covered = theme::info_over_left(ui.available_rect_before_wrap(), opts.info);
    theme::with_info(
        ui,
        opts.info,
        egui::Id::new(("steady info", view)),
        |ui, h| info_panel(ui, s, &series, status, view, h),
        |ui| rows(ui, s, &series, (view, opts), covered),
    );
}

/// The Info panel: what the view shows, then a block for each series
/// shown under its coloured title (its findings and statistics in full),
/// then what was tested. It scrolls, and its words can be selected. On
/// FFT, each series' block also says how much of the grid its spectrum
/// was read from (#1528), kept off the plot.
fn info_panel(
    ui: &mut egui::Ui,
    s: &Steadiness,
    series: &[(usize, &SeriesCheck)],
    status: &str,
    view: View,
    max_h: f32,
) {
    let pal = theme::pal(ui);
    // Each view's panel its own.
    theme::info_scroll(ui, egui::Id::new(("steady info", view)), max_h, |ui| {
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
                ui.add(text(&c.screen_headline, pal.text));
            }
            ui.add(text(&stats_line(c), pal.text_secondary));
            if let Some(support) = (view == View::Fft)
                .then(|| fft_support(s, c.series))
                .flatten()
            {
                ui.add(text(&support, pal.text_tertiary));
            }
        }
        if !status.is_empty() {
            ui.add_space(8.0);
            ui.add(text(status, pal.text_tertiary));
        }
    });
}

/// Each series' row with the view's plot: one above another, filling the
/// pane, or side by side when it is wide and short. `covered` is where
/// Info over the rows starts, if it is over them.
fn rows(
    ui: &mut egui::Ui,
    s: &Steadiness,
    series: &[(usize, &SeriesCheck)],
    (view, opts): (View, Opts),
    covered: Option<f32>,
) {
    let n = series.len().max(1);
    let gap = 10.0;
    let (w, h) = (ui.available_width(), ui.available_height());
    // The time under the pointer on any row's Readings Over Time: every
    // row reads it, those above the one under it a frame late, as the
    // Charts' lanes do (#1277).
    let over_id = egui::Id::new(READINGS_OVER);
    let was: Option<f64> = ui.ctx().data(|d| d.get_temp(over_id)).flatten();
    let mut over = None;
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
                        over = over.or(row(ui, (s, i, c), view, opts, (was, covered)));
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
                    let drawn_row =
                        ui.vertical(|ui| row(ui, (s, i, c), view, opts, (was, covered)));
                    over = over.or(drawn_row.inner);
                });
            }
        });
    }
    if over != was {
        ui.ctx().data_mut(|d| d.insert_temp(over_id, over));
        ui.ctx().request_repaint();
    }
}

/// Where [`rows`] keeps the time under the pointer on Readings Over Time.
const READINGS_OVER: &str = "steadiness readings over";

/// An x-axis label made by `text`, left off within 18 pt of the plot's
/// right edge, where it would run past the plot into what is beside it.
fn clear_of_edge(
    plot_w: f64,
    text: impl Fn(f64) -> String,
) -> impl Fn(egui_plot::GridMark, &std::ops::RangeInclusive<f64>) -> String {
    move |m, r| {
        if theme::near_an_end(m.value, r, f64::NEG_INFINITY, 18.0 / plot_w) {
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

/// The levels drawn across a series' readings, one line per stretch:
/// only where the words name them (core's `Changes::named`, #1112). A
/// split the headline turns down draws nothing; the stats line says why.
fn level_lines(c: &SeriesCheck) -> Vec<[[f64; 2]; 2]> {
    c.changes
        .as_ref()
        .filter(|ch| ch.named && ch.segments.len() > 1)
        .map_or_else(Vec::new, |ch| {
            ch.segments
                .iter()
                .map(|g| [[g.start_s, g.mean], [g.end_s, g.mean]])
                .collect()
        })
}

/// One series' title and plot, and the time under the pointer on its
/// Readings Over Time; `over` is that time on any row, and `covered`
/// where Info over the rows starts. Its findings and statistics are in
/// the Info panel.
fn row(
    ui: &mut egui::Ui,
    (s, i, c): (&Steadiness, usize, &SeriesCheck),
    view: View,
    opts: Opts,
    (over, covered): (Option<f64>, Option<f32>),
) -> Option<f64> {
    let pal = theme::pal(ui);
    // A map's key names its series ("Timing Swing (ms)"), so the map
    // takes the title's line for its picture.
    let title_rect = if view.is_map() {
        egui::Rect::NOTHING
    } else {
        ui.label(theme::caption(title(c.series)).color(pal.text_secondary))
            .rect
    };
    // Too few readings: no tests ran, so there is nothing to draw; the
    // headline says how many it needs.
    if c.autocorrelation.is_none() {
        note(ui, &c.screen_headline);
        return None;
    }
    let k = c.series;
    let colour = series_colour(k, pal);
    let id = ("steadiness", i, view);
    // The plot's width inside its value axis, for the time labels at the
    // right edge.
    let plot_w = f64::from(ui.available_width() - 64.0);
    let base = Plot::new(id)
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
    let dashed = strength::dashed();
    let r = Row {
        s,
        i,
        c,
        view,
        opts,
        k,
        colour,
        id,
        plot_w,
        base,
        dashed,
        pal,
    };
    match view {
        View::Readings => return readings_plot(ui, r, (title_rect, over, covered)),
        View::Periods => periods_plot(ui, r),
        View::Cycle => cycle_plot(ui, r),
        View::Autocorrelation => autocorrelation_plot(ui, r),
        View::Allan => allan_plot(ui, r),
        View::Cusum => cusum_plot(ui, r),
        View::Stft | View::Wavelet => map_plot(ui, r),
        View::Fft => fft_plot(ui, r),
    }
    None
}

/// What every view of a series' row draws with.
struct Row<'a> {
    s: &'a Steadiness,
    i: usize,
    c: &'a SeriesCheck,
    view: View,
    opts: Opts,
    k: SeriesKind,
    colour: Color32,
    id: (&'static str, usize, View),
    plot_w: f64,
    base: Plot<'static>,
    dashed: LineStyle,
    pal: &'static theme::Palette,
}

/// The readings over the session, with the level changes and the two
/// levels; the time under the pointer on it. While the pointer is over
/// any row's, the time there and the reading drawn nearest it are read
/// out beside the title (`title_rect`), as the Charts' lanes do (#1277).
fn readings_plot(
    ui: &mut egui::Ui,
    r: Row<'_>,
    (title_rect, over, covered): (egui::Rect, Option<f64>, Option<f32>),
) -> Option<f64> {
    let Row {
        s,
        i,
        c,
        k,
        colour,
        plot_w,
        base,
        dashed,
        pal,
        ..
    } = r;
    let g = &s.grids[i];
    let drawn = steady_views::thin_readings(g);
    let pts: Vec<[f64; 2]> = drawn.iter().map(|d| [d.t, d.mean]).collect();
    let near = pts.clone();
    // A long series is drawn in chunks, each the band from its lowest
    // reading to its highest, on the scale of the readings themselves: a
    // band reaching past it is clipped at the plot's edge (#1034).
    let banded = steady_views::banded(&drawn);
    let range = steady_views::readings_range(g);
    let grid = value_grid(k);
    // A time pane: the shared gutter and the take's whole time, from its
    // start to its end, so the strip's and the charts' times line up over
    // it (Hugh, #1252). Each value label is noted in the gutter, which is
    // as wide as the widest the time panes draw.
    let gutter = theme::time_gutter(ui.ctx());
    let ctx = ui.ctx().clone();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let end = readings_end(s, r.opts);
    let plot_w = plot_w + 64.0 - f64::from(gutter);
    let mut plot = grid
        .on(base)
        .y_axis_min_width(gutter)
        .include_x(0.0)
        .include_x(end)
        .set_margin_fraction(egui::Vec2::new(0.0, 0.05))
        .x_axis_formatter(clear_of_edge(plot_w, length_text))
        .y_axis_formatter(move |m, _| {
            let text = value_text(k, m.value, m.step_size);
            let w = ctx.fonts_mut(|f| {
                f.layout_no_wrap(text.clone(), font.clone(), Color32::WHITE)
                    .size()
                    .x
            });
            // egui_plot's 4 pt either side of a value label.
            theme::note_gutter_label(
                &ctx,
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w + 8.0, 1.0)),
            );
            text
        });
    plot = time_axis(plot, ui, r.opts);
    if let Some((lo, hi)) = range {
        plot = plot.default_y_bounds(lo, hi);
    }
    // What a dot is, and how many are drawn, as Charts' key.
    let read: usize = drawn.iter().map(|d| d.count).sum();
    let key = steady_views::readings_key(g.step, drawn.len(), read, banded);
    let width = steady_views::chunk_s(g);
    let resp = plot.show(ui, |p| {
        grid.draw(p, pal);
        if banded {
            let bars = drawn
                .iter()
                .map(|d| {
                    Bar::new(d.t, d.hi - d.lo)
                        .base_offset(d.lo)
                        .width(width)
                        .fill(colour.gamma_multiply(BOX_OPACITY))
                        .stroke(egui::Stroke::NONE)
                })
                .collect();
            p.bar_chart(BarChart::new("bands", bars));
        }
        p.points(Points::new("readings", pts).color(colour).radius(1.4_f32));
        for [a, b] in level_lines(c) {
            p.line(
                Line::new("level", vec![a, b])
                    .color(pal.text)
                    .width(1.8_f32),
            );
        }
        if let Some(levels) = state_levels(c.two_state.as_ref()) {
            for v in levels {
                p.hline(call_out(HLine::new("state", v), dashed, pal));
            }
        }
        p.pointer_coordinate().map(|at| at.x)
    });
    let frame = *resp.transform.frame();
    crate::app::paint_key(
        ui.painter(),
        frame,
        &[(key.as_str(), colour, None)],
        (ui, f32::NEG_INFINITY),
        pal,
    );
    let hover = resp.inner.filter(|_| resp.response.hovered());
    if let Some(x) = hover.or(over) {
        let painter = ui.painter_at(*resp.transform.frame());
        strength::time_line(&painter, &resp.transform, x, Strength::Axis, pal);
        let value = crate::app::nearest_at(&near, x).map(|j| drawn[j].readout_parts(k));
        let time = hover.map(|x| readout_time(x, r.opts.joined_in));
        let edge = (title_rect, readout_right(ui.max_rect().right(), covered));
        paint_readout(ui, edge, (time, value.unwrap_or_default()), colour);
    }
    hover
}

/// The time under the pointer as Readings Over Time's readout writes
/// it: the time on its axis, and while a position's stretches are joined
/// end to end, whose time it is, "4:12 in DD", as the axis title says
/// ("Time in DD", Hugh on #1256 and #1277).
fn readout_time(x: f64, joined_in: Option<&str>) -> String {
    let t = length_text(x);
    joined_in.map_or_else(|| t.clone(), |code| format!("{t} in {code}"))
}

/// Readings Over Time's readout on its title's line, after the title:
/// the time (on the row under the pointer only), then the reading drawn
/// nearest it in the series' colour, as a Charts lane writes them, each
/// in a slot as wide as its digits as zeros (`app::lane_width`). Nothing
/// runs past `right` (the row's right edge, or Info's left edge over
/// it): the longest of [`readout_choices`] that fits is drawn, so a
/// narrow pane never shows half a figure and keeps the value longest
/// (Hugh on #1277, #1580).
fn paint_readout(
    ui: &egui::Ui,
    (title_rect, right): (egui::Rect, f32),
    (time, value): (Option<String>, Vec<String>),
    colour: Color32,
) {
    if !title_rect.is_positive() {
        return;
    }
    let pal = theme::pal(ui);
    let font = egui::TextStyle::Small.resolve(ui.style());
    let p = ui.painter();
    let top = title_rect.center().y - font.size / 2.0;
    let left = title_rect.right() + crate::app::LANE_AFTER_TITLE;
    let parts = |(time, value): (Option<String>, Option<String>)| {
        [(time, pal.text), (value, pal.axis_ink(colour))]
            .into_iter()
            .filter_map(|(t, ink)| t.map(|t| (true_minus(&t), ink)))
            .collect::<Vec<_>>()
    };
    let width = |parts: &[(String, Color32)]| {
        parts
            .iter()
            .map(|(t, _)| crate::app::lane_width(p, t, &font, true) + crate::app::LANE_GAP)
            .sum::<f32>()
            - crate::app::LANE_GAP
    };
    let Some(fits) = readout_choices(time, &value)
        .into_iter()
        .map(parts)
        .find(|parts| left + width(parts) <= right)
    else {
        return;
    };
    let mut x = left;
    for (text, ink) in fits {
        x += crate::app::lane_text(p, (x, top), &text, (&font, ink), true) + crate::app::LANE_GAP;
    }
}

/// What the readout can show, longest first: the time and the whole
/// reading, then the reading losing its last part ([`timegrapher_core::steady_views::Drawn::readout_parts`]:
/// the range, then "mean of 8") until only its value is left, then the
/// value alone without the time. The value is dropped only when it
/// doesn't fit on its own (Hugh on #1580).
fn readout_choices(
    time: Option<String>,
    value: &[String],
) -> Vec<(Option<String>, Option<String>)> {
    if value.is_empty() {
        return vec![(time, None)];
    }
    let mut choices: Vec<_> = (1..=value.len())
        .rev()
        .map(|n| (time.clone(), Some(value[..n].join(", "))))
        .collect();
    if time.is_some() {
        choices.push((None, Some(value[0].clone())));
    }
    choices
}

/// Where a row's readout must end: the row's right edge `row_right`, or
/// the left edge of Info over the rows when it starts sooner (#1580).
fn readout_right(row_right: f32, covered: Option<f32>) -> f32 {
    covered.map_or(row_right, |left| row_right.min(left))
}

/// Readings Over Time's time axis against the strip's: ending where the
/// strip above does only on the take's own clock. One position's
/// stretches joined end to end are on another, which no width lines up
/// with the strip's (Walter on #1256), so the axis says whose time it is
/// instead (Hugh).
fn time_axis(mut plot: Plot<'static>, ui: &egui::Ui, opts: Opts) -> Plot<'static> {
    if let Some(w) = readings_width(ui.ctx(), ui.cursor().left(), opts) {
        plot = plot.width(w.clamp(64.0, ui.available_width()));
    }
    if let Some(code) = opts.joined_in {
        plot = plot.x_axis_label(format!("Time in {code}"));
    }
    plot
}

/// Where Readings Over Time's time ends: the take's end, the strip's and
/// the charts' (Walter on #1256); else, for a position's stretches joined
/// end to end, what the tests read of them, and never short of the last
/// window.
pub fn readings_end(s: &Steadiness, opts: Opts) -> f64 {
    opts.take_span.map_or_else(
        || {
            s.grids
                .iter()
                .map(|g| g.t0 + g.y.len() as f64 * g.step)
                .fold(s.upto_s, f64::max)
        },
        |(_, t1)| t1,
    )
}

/// The width that ends Readings Over Time's time area where the strip's
/// ends, for a plot whose gutter starts at `left`: only on the take's own
/// clock, never for a position's stretches joined end to end.
pub fn readings_width(ctx: &egui::Context, left: f32, opts: Opts) -> Option<f32> {
    theme::time_width(ctx, left).filter(|_| opts.take_span.is_some())
}

/// The period found: its dot on the scores, and its note.
fn period_found(c: &SeriesCheck, pts: &[[f64; 2]]) -> Option<([f64; 2], String)> {
    c.period.as_ref().map(|per| {
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
    })
}

/// The period search's scores, with the threshold, the wheels' periods and the period found.
fn periods_plot(ui: &mut egui::Ui, r: Row<'_>) {
    let db = r.opts.periods_db;
    let Row {
        s,
        i,
        c,
        view,
        opts,
        colour,
        plot_w,
        base,
        dashed,
        pal,
        ..
    } = r;
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
    let found = period_found(c, &pts);
    let short = note_short(ui, &names, (i, view));
    let top = if found.is_some() && !short {
        let hi = pts.iter().map(|q| q[1]).fold(y_lo, f64::max);
        marks::room_above(y_lo, hi, top, ui.available_height() - AXIS_H)
    } else {
        top
    };
    let fill = ui.visuals().extreme_bg_color.gamma_multiply(0.8);
    let periods = period_grid(ui.available_width());
    let grid = if db {
        strength::Grid::new(periods, db_grid)
    } else {
        strength::Grid::new(periods, even_grid)
    };
    let plot = grid
        .on(base)
        .height(ui.available_height().max(40.0))
        .x_axis_formatter(clear_of_edge(plot_w, period_label))
        .default_x_bounds(x_lo, x_hi)
        .default_y_bounds(y_lo, top);
    let plot = if db {
        plot.y_axis_formatter(|m, _| db_label(m.value))
    } else {
        plot.y_axis_formatter(|m, _| plain(m.value, m.step_size))
    };
    let resp = plot.show(ui, |p| {
        grid.draw(p, pal);
        wheel_marks(p, &lines, pal);
        p.hline(call_out(HLine::new("threshold", threshold), dashed, pal));
        // Over the lines, under the trace.
        if let Some(n) = &names {
            n.inside(p, &lines, &pts, fill);
        }
        p.line(Line::new("score", pts.clone()).color(colour).width(DATA_W));
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

/// The note above a cycle plot: what it is folded at, and how much the
/// median line and the boxes swing.
fn cycle_label(
    s: &Steadiness,
    i: usize,
    c: &SeriesCheck,
    f: &Fold,
    boxes: &[Option<[f64; 3]>],
) -> String {
    let k = c.series;
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
    match steady_views::fold_spread(f, boxes) {
        Some((swing, middle)) => format!(
            "{label}\nThe median line swings {}; the boxes' median height is {}",
            value_text(k, swing, swing.abs().min(middle.abs()) / 10.0),
            value_text(k, middle, swing.abs().min(middle.abs()) / 10.0),
        ),
        None => label,
    }
}

/// The readings folded at a cycle, banded by when each came, with the boxes and the shape.
fn cycle_plot(ui: &mut egui::Ui, r: Row<'_>) {
    let Row {
        s,
        i,
        c,
        k,
        colour,
        plot_w,
        base,
        pal,
        ..
    } = r;
    let Some(f) = &s.folds[i] else {
        return note(ui, "No cycle found to fold these readings at.");
    };
    let bins = f.profile.len();
    let x = |b: usize| (b as f64 + 0.5) / bins as f64 * f.period_s;
    // The dots go in bands by when their cycle came, first to
    // last, so slow wander shows as a run of colour, on the time
    // map lifted into the theme so every dot stays seen (Hugh's M3).
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
    let boxes = steady_views::fold_columns(f);
    let box_w = 0.3 * f.period_s / bins as f64;
    let shape: Vec<[f64; 2]> = f
        .profile
        .iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .map(|(b, v)| [x(b), *v])
        .collect();
    let range = steady_views::fold_range(f);
    let grid = value_grid(k);
    let mut plot = grid
        .on(base)
        .x_axis_formatter(clear_of_edge(plot_w, lag_text))
        .y_axis_formatter(move |m, _| value_text(k, m.value, m.step_size))
        .default_x_bounds(0.0, f.period_s);
    if let Some((lo, hi)) = range {
        plot = plot.default_y_bounds(lo, hi);
    }
    let label = cycle_label(s, i, c, f, &boxes);
    // Above the plot, wrapped to its width, so it never sits on the
    // axis or runs past the plot's edges.
    ui.add(egui::Label::new(RichText::new(label).small().color(pal.text_secondary)).wrap());
    let plot = plot.height(ui.available_height().max(40.0));
    // The boxes are drawn from the readings, so they are data: the
    // shape's colour, lighter, so the shape stays on top.
    let box_c = colour.gamma_multiply(BOX_OPACITY);
    let dark = ui.visuals().dark_mode;
    let time_colour = |t: f32| crate::tfmap::time_colour(dark, t);
    plot.show(ui, |p| {
        grid.draw(p, pal);
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
                    .color(time_colour(t))
                    .radius(1.4_f32),
            );
        }
        for (b, q) in boxes.iter().enumerate() {
            let Some([q1, _, q3]) = *q else { continue };
            let (l, r) = (x(b) - box_w, x(b) + box_w);
            p.polygon(
                Polygon::new("middle half", vec![[l, q1], [r, q1], [r, q3], [l, q3]])
                    .fill_color(Color32::TRANSPARENT)
                    .stroke(egui::Stroke::new(DATA_W, box_c)),
            );
        }
        p.line(Line::new("shape", shape).color(colour).width(2.0_f32));
    });
}

/// The autocorrelation by lag, with its band and where it repeats.
fn autocorrelation_plot(ui: &mut egui::Ui, r: Row<'_>) {
    let Row {
        i,
        c,
        view,
        colour,
        plot_w,
        base,
        dashed,
        pal,
        ..
    } = r;
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
    let grid = strength::Grid::new(time_grid, even_grid).zero_y();
    let resp = grid
        .on(base)
        .x_axis_formatter(clear_of_edge(plot_w, lag_text))
        .y_axis_formatter(|m, _| plain(m.value, m.step_size))
        .default_x_bounds(0.0, x_hi)
        // Room above the curve for the Ljung–Box note.
        .default_y_bounds(lo - 0.05, 1.35)
        .show(ui, |p| {
            grid.draw(p, pal);
            for b in [a.band, -a.band] {
                p.hline(call_out(HLine::new("band", b), dashed, pal));
            }
            p.line(Line::new("r", pts.clone()).color(colour).width(DATA_W));
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
        let ljung = egui::Rect::from_min_size(corner - egui::vec2(w.size().x, 0.0), w.size());
        let note = format!("repeats at {}", secs(l));
        let id = ui.id().with(("point note", i, view));
        marks::beside(ui, t, [l, r], &note, &pts, &[ljung], fill, short, id);
    }
}

/// Where the Allan plot names its dashed "if the readings were
/// independent" line: at its last point, and nowhere when it has one
/// point, since a line with no length has nothing to name and its caption
/// would sit on the least scatter's label (#1560).
fn independent_caption_at(white: &[[f64; 2]]) -> Option<[f64; 2]> {
    white.last().copied().filter(|_| white.len() > 1)
}

/// The Allan deviation by averaging time, against independent readings.
fn allan_plot(ui: &mut egui::Ui, r: Row<'_>) {
    let Row {
        c,
        k,
        colour,
        plot_w,
        base,
        dashed,
        pal,
        ..
    } = r;
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
    let grid = strength::Grid::new(period_grid(ui.available_width()), log_grid);
    grid.on(base)
        .x_axis_formatter(clear_of_edge(plot_w, period_label))
        .y_axis_formatter(move |m, r| log_label(m, r, |v| unit_sig(k, v)))
        .show(ui, |p| {
            grid.draw(p, pal);
            let last = independent_caption_at(&white);
            p.line(
                Line::new("independent", white)
                    .color(pal.callout)
                    .style(dashed)
                    .width(strength::CALL_OUT_W),
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
                    .width(DATA_W),
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
                        format!("least scatter at {}: {}", secs(t), k.fmt_pm(d)),
                        pal.text,
                        false,
                    );
                }
            }
        });
}

/// The CUSUM over the session, with its band and where the level changes.
fn cusum_plot(ui: &mut egui::Ui, r: Row<'_>) {
    let Row {
        c,
        colour,
        plot_w,
        base,
        dashed,
        pal,
        ..
    } = r;
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
    let grid = strength::Grid::new(time_grid, even_grid).zero_y();
    grid.on(base)
        .x_axis_formatter(clear_of_edge(plot_w, length_text))
        .y_axis_formatter(|m, _| plain(m.value, m.step_size))
        .default_y_bounds(-m, m)
        .show(ui, |p| {
            grid.draw(p, pal);
            for b in [cu.band, -cu.band] {
                p.hline(call_out(HLine::new("95%", b), dashed, pal));
            }
            p.line(Line::new("cusum", pts.clone()).color(colour).width(DATA_W));
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

/// The STFT or wavelet map of the series.
fn map_plot(ui: &mut egui::Ui, r: Row<'_>) {
    let Row {
        s,
        view,
        opts,
        k,
        id,
        base,
        ..
    } = r;
    let name = series_name(k);
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

/// Series `k`'s spectrum in the report, when it has one: what the FFT
/// plot draws and its Info panel's line describes.
fn spectrum(
    s: &Steadiness,
    k: SeriesKind,
) -> Option<&timegrapher_core::spectrum::AmplitudeSpectrum> {
    let name = series_name(k);
    s.report
        .spectra
        .iter()
        .find(|x| x.series == name && !x.period_s.is_empty())
}

/// How much of the grid series `k`'s spectrum was read from, as core
/// words it (`SpectrumSupport::description`): for the FFT Info panel.
fn fft_support(s: &Steadiness, k: SeriesKind) -> Option<String> {
    spectrum(s, k).map(|sp| sp.support.description())
}

/// A series' name in the report's spectra and maps.
fn series_name(k: SeriesKind) -> &'static str {
    match k {
        SeriesKind::Rate => "rate",
        SeriesKind::Amplitude => "amplitude",
        _ => "beat_error",
    }
}

/// Periods and sizes as log points, leaving out any that can't be.
fn log_points(periods: &[f64], sizes: &[f64]) -> Vec<[f64; 2]> {
    log_runs(periods, sizes).concat()
}

/// [`log_points`] in runs: a size core withheld (NaN, #1638) ends one, so
/// the line leaves a gap there and never bridges it.
fn log_runs(periods: &[f64], sizes: &[f64]) -> Vec<Vec<[f64; 2]>> {
    let mut runs: Vec<Vec<[f64; 2]>> = vec![Vec::new()];
    for (&p, &a) in periods.iter().zip(sizes) {
        if a.is_nan() {
            runs.push(Vec::new());
        } else if p > 0.0 && a > 0.0 && a.is_finite() {
            runs.last_mut().expect("a run").push([p.log10(), a.log10()]);
        }
    }
    runs.retain(|r| !r.is_empty());
    runs
}

/// The lowest and highest of the points on one axis (0 across, 1 up).
fn span(pts: &[[f64; 2]], axis: usize) -> (f64, f64) {
    pts.iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p[axis]), hi.max(p[axis]))
        })
}

/// The series' spectrum, with the wheels' periods and its tallest peak.
fn fft_plot(ui: &mut egui::Ui, r: Row<'_>) {
    let Row {
        s,
        i,
        view,
        opts,
        k,
        colour,
        plot_w,
        base,
        pal,
        ..
    } = r;
    let Some(sp) = spectrum(s, k) else {
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
    let pts = log_points(periods, sizes);
    if pts.len() < 2 {
        return note(ui, "The spectrum needs a longer session.");
    }
    let ((x_lo, x_hi), (y_lo, y_hi)) = (span(&pts, 0), span(&pts, 1));
    let top = y_hi + (y_hi - y_lo).max(0.5) * 0.05;
    let (gap, unit) = if as_rate {
        (" ", "s/d")
    } else {
        steadiness::unit_after(sp.unit)
    };
    let size = move |v: f64| format!("{}{gap}{unit}", timegrapher_core::timefreq::size_number(v));
    // A peak is read within a run, never across a withheld gap.
    let peak = log_runs(periods, sizes)
        .iter()
        .filter_map(|r| tallest_peak(r.as_slice()))
        .max_by(|a, b| a[1].total_cmp(&b[1]));
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
    let grid = strength::Grid::new(period_grid(ui.available_width()), log_grid);
    let resp = grid
        .on(base)
        .height(ui.available_height().max(40.0))
        .x_axis_formatter(clear_of_edge(plot_w, period_label))
        .y_axis_formatter(move |m, r| log_label(m, r, size))
        .default_x_bounds(x_lo, x_hi)
        .default_y_bounds(y_lo, top)
        .show(ui, |p| {
            grid.draw(p, pal);
            wheel_marks(p, &lines, pal);
            if let Some(n) = &names {
                n.inside(p, &lines, &tops, fill);
            }
            for run in log_runs(periods, sizes) {
                p.line(Line::new("spectrum", run).color(colour).width(DATA_W));
            }
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
    /// The Info panel, beside the plots: each pane opens its own.
    pub info: bool,
    /// The take's span when the readings are on its own clock, as the
    /// strip's and the charts' are, not one position's stretches joined
    /// end to end: Readings Over Time then fits it, as they do (#1252).
    pub take_span: Option<(f64, f64)>,
    /// The short name of the position whose stretches are joined end to
    /// end, when they are: Readings Over Time's time is then time in it,
    /// and its axis says so ("Time in DD", Hugh on #1256).
    pub joined_in: Option<&'static str>,
}

/// What Wheel Periods does, for its hint.
pub const WHEELS_HINT: &str = "Mark the period each of the calibre's wheels turns in, in \
     a grey dotted line named above the plot, and half of it more loosely dotted, where a \
     wheel that is off centre shows twice a turn. Where the calibre's tooth and leaf counts \
     give them, each pinion's mesh is marked too, faintly dashed: the time one tooth of the \
     wheel driving it takes to engage, drive and leave, where amplitude can step. With no calibre picked, or none of its wheels known, one minute and \
     one hour are marked in grey instead: periods alone, named after no wheel.";

/// What the period axis marks: the calibre's wheels, and its meshes
/// ([`timegrapher_core::calibre_store::Catalog::meshes`]).
#[derive(Clone, Copy)]
pub(crate) struct Marks<'a> {
    pub wheels: &'a [Wheel],
    pub meshes: &'a [timegrapher_core::train::MeshLine],
}

/// The wheels and meshes to mark: `None` when the marks are off.
fn marked(s: &Steadiness, opts: Opts) -> Option<Marks<'_>> {
    opts.wheels.then_some(Marks {
        wheels: &s.wheels,
        meshes: &s.meshes,
    })
}

/// What a line on the period axis marks: a wheel's turn, half of it (a
/// wheel off centre shows twice a turn), a pinion's mesh (one tooth of
/// the wheel driving it engaging, driving and leaving), or a reference
/// period.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Turn {
    Wheel,
    Twice,
    Mesh,
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

    /// Its line's colour: every one is a call-out, so the call-out grey;
    /// its pattern tells a wheel, a ×2 and a reference apart.
    pub fn ink(&self, pal: &theme::Palette) -> Color32 {
        strength::colour(Strength::CallOut, pal)
    }

    /// Its line's width: a wheel's a full call-out, a ×2 or a reference
    /// a fainter one.
    pub fn width(&self) -> f32 {
        if self.turn == Turn::Wheel {
            strength::CALL_OUT_W
        } else {
            strength::FAINT_CALL_OUT_W
        }
    }
}

/// The lines to mark on a period axis spanning `span`: each wheel's turn
/// and half of it ("×2", where a wheel that is off centre shows twice a
/// turn, unless another wheel turns there), then each mesh where no wheel
/// or ×2 already marks its period; with no wheels, the reference
/// periods; with `None` (the marks off), nothing.
pub(crate) fn wheel_lines(shown: Option<Marks<'_>>, (x_lo, x_hi): (f64, f64)) -> Vec<WheelLine> {
    let inside = |x: f64| x > x_lo && x < x_hi;
    let Some(Marks { wheels, meshes }) = shown else {
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
        if !wheels.iter().any(|o| one_mark(o.period_s, half)) {
            out.push(WheelLine {
                x: half.log10(),
                turn: Turn::Twice,
                name: format!("{title} ×2"),
                short: format!("{short} ×2"),
                hover: format!("{title} ×2: {}", secs(half)),
            });
        }
    }
    for m in meshes {
        if out.iter().any(|l| one_mark(10f64.powf(l.x), m.period_s)) {
            continue;
        }
        let title = marks::title_case(&m.name);
        out.push(WheelLine {
            x: m.period_s.log10(),
            turn: Turn::Mesh,
            name: format!("{title} Mesh"),
            short: marks::mesh_short(&m.name),
            hover: format!("{title} Mesh: {}.", m.about),
        });
    }
    out.retain(|l| inside(l.x));
    out
}

/// Whether two periods are near enough (3%) to share one line on a period
/// axis, which keeps the first one marked there.
fn one_mark(a_s: f64, b_s: f64) -> bool {
    (a_s / b_s - 1.0).abs() < 0.03
}

/// The lines in the plot, under its trace, all call-outs: a wheel's
/// dotted, a ×2's looser, a mesh's loosely dashed, a reference period's solid (a named mark). Their names are drawn by
/// [`wheel_names`].
fn wheel_marks(p: &mut egui_plot::PlotUi<'_>, lines: &[WheelLine], pal: &theme::Palette) {
    for l in lines {
        let v = VLine::new(l.name.clone(), l.x)
            .color(l.ink(pal))
            .width(l.width());
        p.vline(match l.turn {
            Turn::Wheel => v.style(LineStyle::dotted_dense()),
            Turn::Twice => v.style(LineStyle::dotted_loose()),
            Turn::Mesh => v.style(LineStyle::dashed_loose()),
            Turn::Reference => v,
        });
    }
}

/// A reference or fitted line across a plot (a threshold, a band, a
/// state level): a call-out, dashed.
fn call_out(h: HLine, dashed: LineStyle, pal: &theme::Palette) -> HLine {
    let s = strength::stroke(Strength::CallOut, pal);
    h.color(s.color).width(s.width).style(dashed)
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
    true_minus(&timegrapher_core::chart_scale::axis_number(v, step))
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

fn note(ui: &mut egui::Ui, text: &str) {
    let pal = theme::pal(ui);
    ui.centered_and_justified(|ui| {
        ui.label(RichText::new(text).small().color(pal.text_tertiary));
    });
}

#[cfg(test)]
#[path = "steady_tests.rs"]
mod tests;
