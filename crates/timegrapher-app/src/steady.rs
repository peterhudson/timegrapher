//! The Steadiness pane: whether the rate, the amplitude and the beat error
//! each hold steady over the session, from the same tests as
//! `timegrapher series` (core `steadiness`), with the views behind each
//! verdict.
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
use timegrapher_core::longterm::{self, Fold, Grid, LongConfig, Search};
use timegrapher_core::periodicity::{standard_wheels, Wheel};
use timegrapher_core::steadiness::{self, Config, Report, SeriesCheck, SeriesKind, Verdict};
use timegrapher_core::stream::BeatLog;
use timegrapher_core::timing;

/// Seconds of beats the tests need before they can say anything: 30 rate
/// readings of 10 s.
pub const MIN_S: f64 = 300.0;
/// How much the session grows before the tests run again, seconds.
pub const RERUN_S: f64 = 60.0;
/// Escape wheel teeth for naming the escape wheel's period, as `series`
/// assumes unless told otherwise.
const ESCAPE_TEETH: u32 = 15;
/// Most readings drawn on the Readings view per series.
const MAX_DRAWN: usize = 3000;
/// Shortest row, and narrowest column, in which a series' plot stays
/// readable, points.
const MIN_ROW: f32 = 120.0;
const MIN_COLUMN: f32 = 260.0;

/// What the plot under each verdict shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum View {
    #[default]
    Readings,
    Periods,
    Cycle,
    Autocorrelation,
    Allan,
    Cusum,
}

impl View {
    const ALL: [View; 6] = [
        View::Readings,
        View::Periods,
        View::Cycle,
        View::Autocorrelation,
        View::Allan,
        View::Cusum,
    ];

    fn label(self) -> &'static str {
        match self {
            View::Readings => "Readings",
            View::Periods => "Periods",
            View::Cycle => "Cycle",
            View::Autocorrelation => "Autocorrelation",
            View::Allan => "Allan Deviation",
            View::Cusum => "CUSUM",
        }
    }

    fn hint(self) -> &'static str {
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
                "The scatter of averages over each averaging time, against the dashed line \
                 independent readings would follow: wander and drift lift the curve at long \
                 times, a cycle puts a bump near half its period. Both axes are logarithmic"
            }
            View::Cusum => {
                "The running sum of each reading's distance from the mean: independent \
                 readings stay inside the dashed lines; a step in the level bends it into a \
                 V or a tent, with the corner at the step"
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
    /// Each series folded at the cycle it was found to have, if any.
    folds: [Option<Fold>; 3],
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
    })
}

/// Run the tests over a session. Slow for long sessions (a 2 h take takes
/// some seconds), so it is run off the window's thread.
/// The tests over `log`, naming cycles after `wheels` (a picked
/// calibre's train) or, without them, the wheels most calibres at the beat
/// rate share.
pub fn compute(log: &BeatLog, wheels: Option<Vec<Wheel>>) -> Steadiness {
    let lc = LongConfig {
        wheels: wheels.unwrap_or_else(|| standard_wheels(log.bph, ESCAPE_TEETH)),
        ..Default::default()
    };
    let cfg = Config::default();
    let long = longrun::analyse(log, None, &lc);
    let report = steadiness::check(log, None, &long, &lc, &cfg);
    let grids = grids(log, cfg.rate_reading_s);
    let readings = |g: &Grid| g.y.iter().filter(|v| v.is_finite()).count();
    let be_search = (readings(&grids[2]) >= 30).then(|| {
        let mut g = grids[2].clone();
        g.t0 = 0.0;
        longterm::analyse(&g, &lc).search
    });
    let searches = [
        Some(long.timing.search.clone()),
        Some(long.amplitude.search.clone()),
        be_search,
    ];
    let folds = std::array::from_fn(|i| {
        let s = report.series.get(i)?;
        let g = &grids[i];
        let period = s
            .cycle
            .as_ref()
            .map(|c| c.period_s)
            .or(s.period.as_ref().map(|p| p.period_s))?;
        (period >= 3.0 * g.step && g.span() >= 2.0 * period).then(|| {
            let bins = ((period / g.step).round() as usize).clamp(6, 40);
            longterm::fold(g, period, bins)
        })
    });
    let logs = searches
        .iter()
        .flatten()
        .flat_map(|x| x.period_s.iter())
        .filter(|p| **p > 0.0)
        .map(|p| p.log10());
    let period_span = logs
        .fold(None, |r: Option<(f64, f64)>, x| {
            Some(r.map_or((x, x), |(a, b)| (a.min(x), b.max(x))))
        })
        .filter(|(a, b)| a < b);
    Steadiness {
        report,
        grids,
        searches,
        folds,
        period_span,
        wheels: lc.wheels,
        upto_s: log.duration_s,
    }
}

/// The readings the tests are made on: 10 s rate readings, and the 2 s
/// amplitude windows for the amplitude and the beat error from the unlock.
fn grids(log: &BeatLog, rate_s: f64) -> [Grid; 3] {
    let wins = timing::windows(&log.beats, log.bph, rate_s, rate_s);
    let rate = match (wins.first(), wins.last()) {
        (Some(w0), Some(last)) => {
            let n = ((last.start_s - w0.start_s) / rate_s).round() as usize + 1;
            let mut y = vec![f64::NAN; n];
            for w in &wins {
                let i = ((w.start_s - w0.start_s) / rate_s).round() as usize;
                if i < n {
                    y[i] = w.fit.rate_s_per_day;
                }
            }
            Grid {
                t0: w0.start_s,
                step: rate_s,
                y,
            }
        }
        _ => Grid {
            t0: 0.0,
            step: rate_s,
            y: Vec::new(),
        },
    };
    let aw = &log.amplitude_windows;
    let step = aw.first().map_or(2.0, |w| w.end_s - w.start_s);
    let grid_of = |f: &dyn Fn(&timegrapher_core::amplitude::AmplitudeWindow) -> Option<f64>| {
        let (Some(w0), Some(last)) = (aw.first(), aw.last()) else {
            return Grid {
                t0: 0.0,
                step,
                y: Vec::new(),
            };
        };
        let n = ((last.start_s - w0.start_s) / step).round() as usize + 1;
        let mut y = vec![f64::NAN; n];
        for w in aw {
            let i = ((w.start_s - w0.start_s) / step).round() as usize;
            if i < n {
                y[i] = f(w).unwrap_or(f64::NAN);
            }
        }
        Grid {
            t0: w0.start_s,
            step,
            y,
        }
    };
    [
        rate,
        grid_of(&|w| w.mean()),
        grid_of(&|w| w.beat_error_unlock_ms),
    ]
}

/// The verdict as a name in Title Case, what it means, and its colour.
fn verdict_text(v: Verdict, pal: &theme::Palette) -> (&'static str, &'static str, Color32) {
    match v {
        Verdict::Steady => (
            "Steady",
            "Independent readings about one level: what a healthy watch on a quiet bench gives",
            pal.good,
        ),
        Verdict::Periodic => (
            "Periodic",
            "The readings repeat on a cycle, named after the wheel whose turn it matches: \
             something in the train changes once a turn",
            pal.warn,
        ),
        Verdict::TwoStates => (
            "Two States",
            "The readings switch between two levels, as when a part rubs some of the time",
            pal.warn,
        ),
        Verdict::Measurement => (
            "Measurement",
            "Two levels that come from the unlock mark hopping between two edges of the \
             sound, not from the watch: a problem with the measurement, so don't read it as \
             a fault",
            pal.text_secondary,
        ),
        Verdict::ShiftingMean => (
            "Shifting Mean",
            "The level steps once or more and stays at the new level, as after a knock or \
             when the watch was moved",
            pal.warn,
        ),
        Verdict::Drifting => (
            "Drifting",
            "The level slides or curves slowly, as the amplitude does when the mainspring \
             runs down",
            pal.warn,
        ),
        Verdict::Wandering => (
            "Wandering",
            "Each reading remembers the last ones, with no cycle, steps or trend to explain \
             it: slow, irregular wander",
            pal.warn,
        ),
        Verdict::TooShort => (
            "Too Short",
            "Too few readings to say; the tests need about 5 minutes of beats",
            pal.text_tertiary,
        ),
    }
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

/// Draw the pane: the view picker and a note on what was tested, then a
/// row for each series with its verdict, its one-line reason and the
/// chosen view behind it.
pub fn draw(ui: &mut egui::Ui, s: Option<&Steadiness>, status: &str, view: &mut View) -> bool {
    let pal = theme::pal(ui);
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        let choices: Vec<(View, &str, &str)> = View::ALL
            .iter()
            .map(|v| (*v, v.label(), v.hint()))
            .collect();
        changed = theme::segmented(ui, view, &choices);
        ui.add_space(8.0);
        ui.label(RichText::new(status).small().color(pal.text_secondary));
    });
    let Some(s) = s else {
        return changed;
    };
    let n = s.report.series.len().max(1);
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
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (i, c) in s.report.series.iter().enumerate() {
                    ui.allocate_ui(Vec2::new(ui.available_width(), row_h), |ui| {
                        ui.set_min_height(row_h);
                        row(ui, s, i, c, *view);
                    });
                    if i + 1 < n {
                        ui.add_space(gap);
                    }
                }
            });
    } else {
        // Wide and short: side by side.
        let col_w = fits(n, w);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (i, c) in s.report.series.iter().enumerate() {
                ui.allocate_ui(Vec2::new(col_w, h), |ui| {
                    ui.set_width(col_w);
                    ui.set_min_height(h);
                    ui.vertical(|ui| row(ui, s, i, c, *view));
                });
            }
        });
    }
    changed
}

fn row(ui: &mut egui::Ui, s: &Steadiness, i: usize, c: &SeriesCheck, view: View) {
    let pal = theme::pal(ui);
    let (word, meaning, colour) = verdict_text(c.verdict, pal);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(theme::caption(title(c.series)).color(pal.text_secondary));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            theme::dot(ui, colour);
            ui.label(
                RichText::new(word)
                    .font(theme::semibold(12.5))
                    .color(pal.text),
            );
        })
        .response
        .on_hover_text(meaning);
    });
    // The reason, on up to two lines; all of it on hover.
    let font = egui::TextStyle::Small.resolve(ui.style());
    let mut job = egui::text::LayoutJob::single_section(
        c.headline.clone(),
        egui::TextFormat::simple(font, pal.text_secondary),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: ui.available_width(),
        max_rows: 2,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let one_line = galley.rows.len() < 2;
    let line_h = galley.size().y;
    ui.add(egui::Label::new(galley))
        .on_hover_text(c.headline.clone());
    // Always two lines' room, so the plots line up.
    if one_line {
        ui.add_space(line_h);
    }
    if c.verdict == Verdict::TooShort {
        return;
    }
    let k = c.series;
    let colour = series_colour(k, pal);
    let weak = pal.text_tertiary;
    let id = ("steadiness", i, view);
    let base = Plot::new(id)
        .allow_scroll(false)
        .allow_zoom(false)
        .allow_drag(false)
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
                if matches!(c.verdict, Verdict::TwoStates | Verdict::Measurement) {
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
            let top = pts.iter().map(|p| p[1]).fold(search.threshold, f64::max) * 1.15;
            base.x_grid_spacer(log_grid)
                .x_axis_formatter(|m, r| log_label(m, r, secs))
                .y_grid_spacer(|g| theme::even_grid(g, 24.0, &[]))
                .y_axis_formatter(|m, _| format!("{:.0}", m.value))
                .default_x_bounds(x_lo, x_hi)
                .default_y_bounds(0.0, top)
                .show(ui, |p| {
                    for w in &s.wheels {
                        let x = w.period_s.log10();
                        if x > x_lo && x < x_hi {
                            p.vline(VLine::new(w.name.clone(), x).color(weak).width(1.0_f32));
                            p.text(
                                Text::new(
                                    "wheel",
                                    PlotPoint::new(x, top * 0.97),
                                    wheel_title(&w.name),
                                )
                                .color(pal.text_secondary)
                                .anchor(egui::Align2::LEFT_TOP),
                            );
                        }
                    }
                    p.hline(
                        HLine::new("threshold", search.threshold)
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
            let label = {
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
                    p.line(
                        Line::new("independent", white)
                            .color(pal.text_secondary)
                            .style(dashed)
                            .width(1.0_f32),
                    );
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
    }
}

/// A note beside a marked point, on whichever side keeps it inside the
/// plot, above or below the point.
fn mark_label(
    p: &mut egui_plot::PlotUi<'_>,
    at: [f64; 2],
    text: String,
    color: Color32,
    above: bool,
) {
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
/// lines only when less than two decades show.
fn log_label(
    m: GridMark,
    range: &std::ops::RangeInclusive<f64>,
    f: impl Fn(f64) -> String,
) -> String {
    let span = range.end() - range.start();
    if m.step_size >= 1.0 || span < 2.0 {
        f(10f64.powf(m.value))
    } else {
        String::new()
    }
}

/// The 1st to 99th percentile of the points' values, padded a little.
fn spread(pts: &[[f64; 2]]) -> Option<(f64, f64)> {
    if pts.len() < 2 {
        return None;
    }
    let mut v: Vec<f64> = pts.iter().map(|p| p[1]).collect();
    v.sort_by(f64::total_cmp);
    let q = |f: f64| v[((v.len() - 1) as f64 * f).round() as usize];
    let (lo, hi) = (q(0.01), q(0.99));
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
        assert_eq!(rate.verdict, Verdict::Periodic, "{}", rate.headline);
        assert!(s.folds[0].is_some());

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
        for view in View::ALL {
            let mut v = view;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 700.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw(ui, Some(&s), "test", &mut v);
                });
            });
            assert!(!out.shapes.is_empty());
        }
    }
}
