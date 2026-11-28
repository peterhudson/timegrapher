//! The Distributions pane: how each reading's values spread over the take.

use super::result_keys::DistRead;
use super::*;
use crate::fact_box::FactBox;
use crate::strength::{self, Grid, Strength};

/// Core's readings for the Distributions pane (#1144): the rate's, and the
/// amplitude and the beat error averaged into them, with the beat error's
/// source ([`timegrapher_core::steadiness::reading_grids`]).
pub(super) type DistReadings = (
    [timegrapher_core::longterm::Grid; 3],
    core_session::BeatErrorSelection,
);

/// The width kept for the shares up the side of a distribution's plot.
const SIDE_AXIS_W: f32 = 44.0;

/// What a distribution's plot rests on (#2171): the reading and how it
/// is shown, the take's [`Revision`], the position counted, Average
/// Over, core's readings and how far they reach, how many 2 s rates and
/// amplitude windows and which breaks the take has, and its changes of
/// position and settlings, which a reading's position is tested on. Any
/// change works the plot out again; while listening, each new 2 s
/// stretch does. The breaks and positions are kept whole: listening adds
/// a break (a jump) with no [`Revision`] moved.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PlotKey {
    reading: SeriesKind,
    short: bool,
    cumulative: bool,
    rev: Revision,
    position: Position,
    average: u64,
    readings: Option<(DistKey, u64)>,
    rates: usize,
    windows: usize,
    breaks: Vec<f64>,
    stretches: Vec<(f64, Position)>,
    settles: Vec<(f64, f64)>,
}

/// A distribution's plot as worked out for its [`PlotKey`]: core's
/// distribution of the values, how many there were, the beat error's
/// source, and the points drawn, on the scale's span.
#[derive(Debug)]
pub(super) struct DistPlot {
    key: PlotKey,
    n: usize,
    kind: timegrapher_core::distribution::Kind,
    source: Option<core_session::BeatErrorSelection>,
    d: Option<timegrapher_core::distribution::Distribution>,
    pts: Vec<[f64; 2]>,
}

/// What the Distributions pane keeps between frames: the 2 s rates, and
/// each reading's plot as last worked out.
#[derive(Debug, Default)]
pub(super) struct DistState {
    pub(super) rate_2s: RateTwoS,
    plots: [Option<std::sync::Arc<DistPlot>>; 3],
}

/// A reading's colour on the charts, the strip and here.
fn series_color(reading: SeriesKind, pal: &theme::Palette) -> Color32 {
    match reading {
        SeriesKind::Rate => pal.trace_rate,
        SeriesKind::Amplitude => pal.trace_amplitude,
        SeriesKind::BeatError => pal.trace_beat_error,
    }
}

/// `pts` as drawn on `t`'s frame over the plot's own bounds (`x` and
/// `y`, its default bounds, which it keeps: no zoom or drag), less any
/// within half a point of the one kept before it, the last always kept.
/// A long take's tens of thousands of values lie that close along the
/// curve, and drawing each made every frame crawl (#2171); what shows is
/// the same. The bounds are the plot's own rather than `t`'s, which on
/// the frame the values change are still the last frame's.
fn on_screen(
    pts: &[[f64; 2]],
    t: &egui_plot::PlotTransform,
    (x, y): ([f64; 2], [f64; 2]),
) -> Vec<[f64; 2]> {
    let bounds = egui_plot::PlotBounds::from_min_max([x[0], y[0]], [x[1], y[1]]);
    let t = egui_plot::PlotTransform::new(*t.frame(), bounds, egui::Vec2b::new(false, false));
    let at = |p: &[f64; 2]| t.position_from_point(&egui_plot::PlotPoint::new(p[0], p[1]));
    let mut kept: Vec<[f64; 2]> = Vec::new();
    let mut last: Option<egui::Pos2> = None;
    for (i, p) in pts.iter().enumerate() {
        let here = at(p);
        let near = last.is_some_and(|l| (here - l).length() < ON_SCREEN_PT);
        if !near || i + 1 == pts.len() {
            kept.push(*p);
            last = Some(here);
        }
    }
    kept
}

/// How close two values' points are drawn before only the first is, pt.
const ON_SCREEN_PT: f32 = 0.5;

impl TimegrapherApp {
    pub(super) fn histogram_settings(&mut self, ui: &mut egui::Ui) {
        theme::tool_choice(
            ui,
            "Show",
            "The values on probability paper, or their cumulative share",
            &mut self.bench.panes_view.hist_cumulative,
            &[
                (
                    false,
                    Icon::ProbLine,
                    "Probability",
                    "Each value against its rank on a scale of standard deviations: \
                             one steady state falls on the dashed straight line, two states \
                             draw two lines joined by a bend",
                ),
                (
                    true,
                    Icon::SCurve,
                    "Cumulative",
                    "The share of values at or below each value: one state rises in \
                             one steep stretch, two states rise twice with a flatter stretch \
                             between",
                ),
            ],
        );
        theme::tool_choice(
            ui,
            "Values",
            "What the distributions count",
            &mut self.bench.panes_view.hist_readings,
            &[
                (
                    true,
                    Icon::Bucket,
                    "Readings",
                    "The readings, each averaged over Average Over, as on the charts \
                             and the strip, one per Average Over so that no two share beats: \
                             states lasting longer than that stand out clearly",
                ),
                (
                    false,
                    Icon::TwoSeconds,
                    "2 s",
                    "Each 2 seconds of beats, the finest the app measures: catches \
                             quicker changes, with more scatter",
                ),
            ],
        );
    }

    /// The runs in the position the watch is in now, joined
    /// ([`Session::current_runs`]): the 2 s values read them; the readings
    /// and the steadiness tests read only their heard parts
    /// ([`Self::heard_runs`]).
    pub(super) fn current_runs(&self) -> timegrapher_core::take_context::Stretches<Position> {
        self.session.current_runs(self.bench.take.position)
    }

    /// The 2-second stretches of amplitude and beat error in the position
    /// the watch is in now, settled: core's one rule for which windows a
    /// stretch takes ([`core_session::stretch_windows`], #636), so none
    /// overlaps a masked stretch or reaches across a break, as the
    /// readings do.
    pub(super) fn position_windows(&self) -> Vec<&timegrapher_core::amplitude::AmplitudeWindow> {
        let Some(live) = self.bench.recording.live.as_ref() else {
            return Vec::new();
        };
        let cuts = live.cuts();
        let runs = self.current_runs();
        // None reaching into a hole in a run, as a mask leaves none (#1156).
        runs.read
            .iter()
            .flat_map(|&(a, b)| {
                core_session::stretch_windows(&cuts, live.amplitude_windows(), a, b)
            })
            .filter(|w| !core_session::window_in_mask(&runs.holes, w))
            .collect()
    }

    /// The Distributions pane: a plot for each reading shown, one above
    /// the other, sharing the pane's height.
    pub(super) fn distributions_pane(&mut self, ui: &mut egui::Ui) {
        if self.bench.panes_view.hist_readings {
            self.update_dist_readings(Some(ui.ctx()));
        }
        let shown = self.bench.panes_view.dist_series;
        self.note_need(ui, Pane::Distributions, DIST_MIN_H);
        stacked(ui, shown, (DIST_MIN_H, true), |ui, k, no_room| {
            self.histogram_with(ui, k, no_room)
        });
    }

    /// How often each value of a reading came up, over the session or the
    /// strip's length: bars, or the cumulative share. With room for it,
    /// as the tests draw it.
    #[cfg(test)]
    pub(super) fn histogram(&mut self, ui: &mut egui::Ui, reading: SeriesKind) {
        self.histogram_with(ui, reading, None);
    }

    /// A reading's distribution, with what the pane leaves out for want
    /// of room and what to do (`no_room`) in a grey chip under its title.
    fn histogram_with(
        &mut self,
        ui: &mut egui::Ui,
        reading: SeriesKind,
        no_room: Option<&(String, String)>,
    ) {
        let pal = theme::pal(ui);
        let short = !self.bench.panes_view.hist_readings;
        if reading == SeriesKind::Rate && short {
            match self.bench.recording.live.as_ref() {
                Some(live) => self
                    .bench
                    .panes_view
                    .dist
                    .rate_2s
                    .refresh(live, &self.bench.take.masked),
                None => self.bench.panes_view.dist.rate_2s = RateTwoS::default(),
            }
        }
        let worked = self.dist_plot(reading, short);
        let color = series_color(reading, pal);
        // The title line: the series, and the one position counted when
        // the take has positions, as the toolbar doesn't name it (#1528).
        let only = (!self.session.stretches().is_empty()).then(|| self.bench.take.position.name());
        let title = match only {
            Some(p) => format!("{} \u{b7} {p} only", steady::title(reading)),
            None => steady::title(reading).to_string(),
        };
        ui.label(theme::caption(&title).color(pal.text_secondary));
        if let Some((what, remedy)) = no_room {
            let short = no_room_short(what);
            theme::chip(
                ui,
                &short,
                &short,
                std::slice::from_ref(remedy),
                ui.available_width(),
            );
        }
        // The beat error's source in a grey chip under it, core's caveats
        // in the chip's hover (#1528).
        if let Some(src) = worked.source.as_ref().and_then(|s| s.beat_error_source) {
            let label = timegrapher_core::session::beat_error_source_label(Some(src));
            let caveats: Vec<String> = worked
                .source
                .iter()
                .flat_map(|s| s.caveats())
                .filter_map(|c| c.long(None))
                .collect();
            theme::chip(ui, label, label, &caveats, ui.available_width());
        }
        let Some(d) = worked.d.as_ref() else {
            ui.label(
                RichText::new("Not enough readings yet")
                    .small()
                    .color(pal.text_secondary),
            );
            return;
        };
        let rows = d.fact_rows(worked.kind);
        let shed = timegrapher_core::distribution::Distribution::fact_shed(&rows);
        let facts = facts_box(ui, steady::title(reading), rows, shed);
        let (x_lo, x_hi) = (d.edges[0], d.edges[d.edges.len() - 1]);
        let unit_owned = worked.kind.suffix.to_string();
        let plot_w = f64::from(ui.available_width() - SIDE_AXIS_W);
        let plot = Plot::new(format!("{} Distribution", steady::title(reading)))
            // As short as its share of the pane, rather than spilling out.
            .min_size(Vec2::new(64.0, 24.0))
            .allow_scroll(false)
            .allow_zoom(false)
            .allow_drag(false)
            .allow_axis_zoom_drag(false)
            .allow_boxed_zoom(false)
            .allow_double_click_reset(false)
            .show_x(false)
            .show_y(false)
            .x_axis_formatter(move |m, r| {
                let v = timegrapher_core::figure_text::true_minus(
                    &timegrapher_core::chart_scale::axis_number(m.value, m.step_size),
                );
                theme::clear_of_ends(format!("{v}{unit_owned}"), m.value, r, plot_w)
            })
            .y_axis_min_width(SIDE_AXIS_W)
            .default_x_bounds(x_lo, x_hi);
        if worked.key.cumulative {
            return cumulative_plot(ui, plot, (&worked.pts, [x_lo, x_hi]), color, facts);
        }
        probability_paper(ui, plot, (&worked, [x_lo, x_hi]), color, facts);
    }

    /// What `reading`'s plot is worked out from ([`PlotKey`]).
    fn plot_key(&self, reading: SeriesKind, short: bool) -> PlotKey {
        let live = self.bench.recording.live.as_ref();
        PlotKey {
            reading,
            short,
            cumulative: self.bench.panes_view.hist_cumulative,
            rev: self.bench.take.rev,
            position: self.bench.take.position,
            average: self.bench.take.average_s.to_bits(),
            readings: self
                .bench
                .results
                .dist_readings
                .as_ref()
                .map(|d| (d.key, d.upto_s.to_bits())),
            rates: self.bench.panes_view.dist.rate_2s.values.len(),
            windows: live.map_or(0, |l| l.amplitude_windows().len()),
            breaks: live.map_or_else(Vec::new, |l| l.breaks().to_vec()),
            stretches: self.session.stretches(),
            settles: self.session.settles(),
        }
    }

    /// `reading`'s plot, worked out again only when what it rests on
    /// changes ([`PlotKey`]): a long take's every 2 s stretch, collected,
    /// sorted and placed afresh each frame, made the app crawl (#2171).
    fn dist_plot(&mut self, reading: SeriesKind, short: bool) -> std::sync::Arc<DistPlot> {
        let key = self.plot_key(reading, short);
        let i = reading as usize;
        if let Some(p) = self.bench.panes_view.dist.plots[i]
            .as_ref()
            .filter(|p| p.key == key)
        {
            return p.clone();
        }
        let worked = std::sync::Arc::new(self.work_out_plot(key));
        self.bench.panes_view.dist.plots[i] = Some(worked.clone());
        worked
    }

    /// A distribution's plot, worked out from the values `key` names.
    fn work_out_plot(&self, key: PlotKey) -> DistPlot {
        use timegrapher_core::{distribution as dist, histogram as hist};
        let (values, kind, _, label, source) =
            self.hist_values(key.reading, key.short, &theme::DARK);
        let step = if label == "2 s" {
            dist::STRETCH_S
        } else {
            self.bench.take.average_s
        };
        let d = dist::distribution(&values, kind, label, step);
        let pts = d.as_ref().map_or_else(Vec::new, |d| {
            let (x_lo, x_hi) = (d.edges[0], d.edges[d.edges.len() - 1]);
            let inside = |x: f64| x >= x_lo && x <= x_hi;
            if key.cumulative {
                // The share of values at or below each value, as a step
                // line.
                let mut pts: Vec<[f64; 2]> = Vec::new();
                for [x, q] in hist::ecdf(&values).into_iter().filter(|p| inside(p[0])) {
                    if let Some(&[_, prev]) = pts.last() {
                        pts.push([x, prev]);
                    }
                    pts.push([x, q * 100.0]);
                }
                pts
            } else {
                hist::probability_plot(&values)
                    .into_iter()
                    .filter(|p| inside(p[0]))
                    .collect()
            }
        });
        DistPlot {
            key,
            n: values.len(),
            kind,
            source,
            d,
            pts,
        }
    }

    /// Keeps core's readings for the pane ([`DistReadings`]) up to date
    /// with the take, the position on show and Average Over: read over what
    /// the steadiness tests read ([`Self::steady_read`] of the position's
    /// [`Self::heard_runs`]), each stretch no reading hears a watch in left
    /// out and cut at both ends, in the background with `ctx`, the last
    /// ones showing until the next are ready, or here and now without.
    /// While listening they are read again once a new reading can have
    /// come. What is heard rests on the readings at the Average Over, so
    /// none is read on readings not yet read again (#2480).
    pub(super) fn update_dist_readings(&mut self, ctx: Option<&egui::Context>) {
        let key = DistKey {
            rev: self.bench.take.rev,
            position: self.bench.take.position,
            average: self.bench.take.average_s.to_bits(),
        };
        if self
            .bench
            .results
            .dist_job
            .as_ref()
            .is_some_and(|j| j.key != key)
        {
            self.bench.results.dist_job = None;
        }
        if self
            .bench
            .results
            .dist_readings
            .as_ref()
            .is_some_and(|d| d.key != key)
        {
            self.bench.results.dist_readings = None;
        }
        if let Some(j) = &self.bench.results.dist_job {
            match j.got.try_recv() {
                Ok(got) => {
                    self.bench.results.dist_readings = Some(DistRead {
                        key: j.key,
                        upto_s: j.upto_s,
                        got,
                    });
                    self.bench.results.dist_job = None;
                }
                Err(TryRecvError::Disconnected) => self.bench.results.dist_job = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        let runs = self.heard_runs();
        let clock = Self::rate_clock(&self.bench.take.take_clock).cloned();
        let joined = !self.session.stretches().is_empty();
        let Some(live) = self.bench.recording.live.as_ref() else {
            return;
        };
        let dur = steady::joined_duration(
            &runs.read,
            live.duration_s(),
            clock.as_ref().filter(|_| joined),
        );
        let due = self
            .bench
            .results
            .dist_readings
            .as_ref()
            .is_none_or(|d| dur - d.upto_s >= self.bench.take.average_s);
        if self.bench.results.dist_job.is_some() || !due || !self.trend_current() {
            return;
        }
        let Some((log, breaks)) = self.steady_read(&runs, clock.as_ref()) else {
            return;
        };
        let clock = clock.filter(|_| !joined); // A joined log is on it.
        let r = self.bench.take.average_s;
        let read =
            move || timegrapher_core::steadiness::reading_grids(&log, clock.as_ref(), &breaks, r);
        match ctx {
            Some(ctx) => {
                let (tx, rx) = std::sync::mpsc::channel();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(read());
                    ctx.request_repaint();
                });
                self.bench.results.dist_job = Some(DistRead {
                    key,
                    upto_s: dur,
                    got: rx,
                });
            }
            None => {
                self.bench.results.dist_readings = Some(DistRead {
                    key,
                    upto_s: dur,
                    got: read(),
                })
            }
        }
    }

    /// The values of `reading` a distribution counts, in the position the
    /// watch is in now: each 2 s stretch (`short`) or one reading per
    /// averaging time. With them, the scale they are on, their colour and
    /// what each value is.
    pub(super) fn hist_values(
        &self,
        reading: SeriesKind,
        short: bool,
        pal: &theme::Palette,
    ) -> (
        Vec<f64>,
        timegrapher_core::distribution::Kind,
        Color32,
        &'static str,
        Option<core_session::BeatErrorSelection>,
    ) {
        // Only the position the watch is in now: values from another
        // position are a different measurement.
        let inside = self.session.in_position_test(self.bench.take.position);
        let color = series_color(reading, pal);
        // The readings, one per Average Over so that no two share beats:
        // core's, as `timegrapher series` counts them (#1144).
        let core = self.bench.results.dist_readings.as_ref().map(|d| &d.got.0);
        let readings = |i: usize| -> Vec<f64> {
            core.map_or_else(Vec::new, |g| {
                g[i].y.iter().copied().filter(|v| v.is_finite()).collect()
            })
        };
        use timegrapher_core::distribution::{AMPLITUDE, BEAT_ERROR, RATE};
        match reading {
            SeriesKind::Rate if short => (
                self.bench
                    .panes_view
                    .dist
                    .rate_2s
                    .values
                    .iter()
                    .filter(|&&(a, _)| inside(a + RateTwoS::STEP_S) && inside(a + 1e-6))
                    .map(|&(_, v)| v)
                    .collect(),
                RATE,
                color,
                "2 s",
                None,
            ),
            SeriesKind::Rate => (readings(0), RATE, pal.trace_rate, "readings", None),
            SeriesKind::Amplitude if short => (
                self.position_windows()
                    .iter()
                    .filter_map(|w| w.mean())
                    .collect(),
                AMPLITUDE,
                color,
                "2 s",
                None,
            ),
            SeriesKind::Amplitude => (readings(1), AMPLITUDE, color, "readings", None),
            SeriesKind::BeatError => {
                let selected = self.hist_beat_errors(short);
                let values = if short {
                    selected.values.iter().map(|(_, v)| *v).collect()
                } else {
                    readings(2)
                };
                (
                    values,
                    BEAT_ERROR,
                    color,
                    if short { "2 s" } else { "readings" },
                    Some(selected),
                )
            }
        }
    }
    fn hist_beat_errors(&self, short: bool) -> core_session::BeatErrorSelection {
        if short {
            // Read for the take's escapement (#2313): never the drop on a
            // co-axial.
            let escapement = self
                .bench
                .recording
                .live
                .as_ref()
                .map(|l| l.config().analysis.amplitude.escapement)
                .unwrap_or_default();
            return core_session::select_window_beat_errors(self.position_windows(), escapement);
        }
        // The source core chose over the windows the readings are made
        // of, with its caveats.
        self.bench.results.dist_readings.as_ref().map_or_else(
            || core_session::select_beat_errors(std::iter::empty()),
            |d| d.got.1.clone(),
        )
    }
}

/// A distribution as the share of values at or below each value, as a step
/// line (`pts`, worked out in [`TimegrapherApp::work_out_plot`]).
fn cumulative_plot(
    ui: &mut egui::Ui,
    plot: Plot<'_>,
    (pts, x): (&[[f64; 2]], [f64; 2]),
    color: Color32,
    mut facts: FactBox,
) {
    let pal = theme::pal(ui);
    let grid = Grid::new(value_grid, |g| theme::even_grid(g, 30.0, &[]));
    let r = grid
        .on(plot)
        .y_axis_formatter(|m, _| {
            if m.value < -1e-9 || m.value > 100.0 + 1e-9 {
                String::new()
            } else {
                format!("{}%", m.value.abs())
            }
        })
        .default_y_bounds(0.0, 100.0)
        .show(ui, |p| {
            grid.draw(p, pal);
            let pts = on_screen(pts, p.transform(), (x, [0.0, 100.0]));
            facts.plot_h = p.transform().frame().height();
            facts.tops = pts.clone();
            p.add(facts.clone());
            p.line(
                Line::new("share at or below", pts)
                    .color(color)
                    .width(1.8_f32),
            );
        });
    shed_hover(&r, &facts);
}

/// A distribution on probability paper: the share at or below each value
/// on a scale of standard deviations, labelled in percent, with the line
/// one steady state would follow.
fn probability_paper(
    ui: &mut egui::Ui,
    plot: Plot<'_>,
    (worked, x): (&DistPlot, [f64; 2]),
    color: Color32,
    mut facts: FactBox,
) {
    use timegrapher_core::histogram as hist;
    let pal = theme::pal(ui);
    let normal_line = worked.d.as_ref().and_then(|d| d.normal_line);
    let z_top = hist::normal_quantile(1.0 - 0.5 / worked.n.max(2) as f64).min(3.3) + 0.25;
    let line = normal_line.map(|l| {
        vec![
            [l.centre - l.spread * z_top, -z_top],
            [l.centre + l.spread * z_top, z_top],
        ]
    });
    let grid = Grid::new(value_grid, move |g: egui_plot::GridInput| {
        probability_grid(8.0 / g.base_step_size, z_top)
    });
    let r = grid
        .on(plot)
        .y_axis_formatter(|m, _| {
            PROBABILITY_MARKS
                .iter()
                .find(|&&(pc, _)| (hist::normal_quantile(pc / 100.0) - m.value).abs() < 1e-6)
                .map_or(String::new(), |&(_, label)| label.to_string())
        })
        .default_y_bounds(-z_top, z_top)
        .show(ui, |p| {
            grid.draw(p, pal);
            let pts = on_screen(&worked.pts, p.transform(), (x, [-z_top, z_top]));
            facts.plot_h = p.transform().frame().height();
            facts.tops = pts.clone();
            p.add(facts.clone());
            // The line one steady state would follow: a call-out, dashed.
            if let Some(l) = line {
                let s = strength::stroke(Strength::CallOut, pal);
                p.line(
                    Line::new("one steady state", l)
                        .color(s.color)
                        .style(LineStyle::dashed_loose())
                        .width(s.width),
                );
            }
            p.points(
                Points::new("values", pts)
                    .color(color)
                    .radius(1.6_f32)
                    .filled(true),
            );
        });
    shed_hover(&r, &facts);
}

/// The rows a short plot's facts box leaves out, in a hover over the
/// box: every row, so nothing it sheds is lost (Hugh, #1528).
fn shed_hover(r: &egui_plot::PlotResponse<()>, facts: &FactBox) {
    let (shown, rect) = facts.layout(&r.transform);
    if shown.len() == facts.rows.len() || shown.is_empty() {
        return;
    }
    let over = r.response.hover_pos().is_some_and(|p| rect.contains(p));
    if over {
        let words: Vec<String> = facts
            .rows
            .iter()
            .map(|(n, v)| format!("{n}: {v}"))
            .collect();
        r.response.clone().on_hover_text(words.join("\n"));
    }
}

/// A series' facts box in the top left of its plot, which a rising
/// curve leaves clear: core's rows ([`Distribution::fact_rows`],
/// #1528), moving to the right or shedding rows in core's order
/// ([`Distribution::fact_shed`]) where the values run under it.
///
/// [`Distribution::fact_shed`]: timegrapher_core::distribution::Distribution::fact_shed
///
/// [`Distribution::fact_rows`]: timegrapher_core::distribution::Distribution::fact_rows
fn facts_box(ui: &egui::Ui, name: &str, rows: Vec<(String, String)>, shed: Vec<usize>) -> FactBox {
    FactBox::plain(ui, "distribution facts", name, rows, shed)
}

/// Probability paper's share marks, `px` points to one standard
/// deviation, up to `z_top`: from 50% outwards, leaving out a share
/// whose label would crowd the one before it. Where that leaves out 1%
/// and 99%, those and 50% alone (Hugh, #1528), or 50% alone where they
/// don't fit. The marks kept are [`LABEL_GAP`] apart at least.
fn probability_grid(px: f64, z_top: f64) -> Vec<GridMark> {
    use timegrapher_core::histogram as hist;
    let z = |pc: f64| hist::normal_quantile(pc / 100.0);
    let mut kept: Vec<f64> = Vec::new();
    for &(pc, _) in &PROBABILITY_MARKS[5..] {
        let at = z(pc);
        if at <= z_top
            && kept
                .last()
                .is_none_or(|&k| (at - k) * px >= f64::from(LABEL_GAP))
        {
            kept.push(at);
        }
    }
    let z99 = z(99.0);
    if z99 <= z_top && !kept.contains(&z99) {
        kept = if z99 * px >= f64::from(LABEL_GAP) {
            vec![0.0, z99]
        } else {
            vec![0.0]
        };
    }
    // egui_plot labels a mark only where its step spans 20 pt or more,
    // faint up to 30: the plot's whole height, as the gaps are kept here.
    let step_size = 2.0 * z_top;
    kept.iter()
        .flat_map(|&z| if z == 0.0 { vec![z] } else { vec![z, -z] })
        .map(|value| GridMark { value, step_size })
        .collect()
}

/// The least room between two share labels up probability paper,
/// points: more than egui_plot's least for a y label, 20.
const LABEL_GAP: f32 = 22.0;

/// A distribution's value axis: every grid line and label alike, at
/// least 80 points apart.
fn value_grid(g: egui_plot::GridInput) -> Vec<GridMark> {
    theme::even_grid(g, 80.0, &[])
}

/// The rate over each 2 seconds of beats, on a grid from the first beat,
/// worked out on as beats arrive: (start, s/d). Whatever it was worked
/// out from is kept, so it starts again when that changes.
#[derive(Debug, Default)]
pub(super) struct RateTwoS {
    first: Option<f64>,
    beats: usize,
    bph: Option<u32>,
    breaks: Vec<f64>,
    masked: Vec<(f64, f64)>,
    /// The next stretch on the grid to work out.
    next: usize,
    pub(super) values: Vec<(f64, f64)>,
}

impl RateTwoS {
    /// Seconds of beats in each value, as the amplitude's stretches.
    pub(super) const STEP_S: f64 = timegrapher_core::distribution::STRETCH_S;

    /// Bring the values up to `live`'s settled beats. None reaches across a
    /// break (a jump or a new position) or into a masked stretch, as no
    /// reading does.
    pub(super) fn refresh(&mut self, live: &LiveAnalyzer, masked: &[(f64, f64)]) {
        let beats = live.beats();
        let first = beats.first().map(|b| b.time);
        if self.first != first
            || self.beats > beats.len()
            || self.bph != live.bph()
            || self.breaks != live.breaks()
            || self.masked != masked
        {
            *self = RateTwoS {
                first,
                bph: live.bph(),
                breaks: live.breaks().to_vec(),
                masked: masked.to_vec(),
                ..Default::default()
            };
        }
        self.beats = beats.len();
        let (Some(t0), Some(bph)) = (first, self.bph) else {
            return;
        };
        self.next = timegrapher_core::distribution::rates_2s(
            beats,
            bph,
            t0,
            self.next,
            &self.breaks,
            &self.masked,
            &mut self.values,
        );
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;

    /// Up a short plot, the share labels thin to 1%, 50% and 99%, each
    /// labelled; a tall one keeps the shares between (Hugh, #1528).
    #[test]
    fn probability_labels_thin_to_the_ends_and_the_middle() {
        use timegrapher_core::histogram::normal_quantile;
        let z99 = normal_quantile(0.99);
        let values = |m: Vec<GridMark>| m.iter().map(|g| g.value).collect::<Vec<_>>();
        assert_eq!(values(probability_grid(14.0, 3.5)), vec![0.0, z99, -z99]);
        // Too short even for 1% and 99%: 50% alone.
        assert_eq!(values(probability_grid(8.0, 3.5)), vec![0.0]);
        let tall = values(probability_grid(60.0, 3.5));
        assert!(
            tall.contains(&normal_quantile(0.9)) && tall.contains(&z99),
            "{tall:?}"
        );
        // Every mark kept is labelled: its step spans the plot.
        assert!(probability_grid(14.0, 3.5)
            .iter()
            .all(|g| g.step_size * 14.0 >= 30.0));
    }

    /// A box short of room flows its rows into two columns, then sheds
    /// core's rows in core's order, never the median or the values off
    /// the scale (Hugh, #1528).
    #[test]
    fn a_short_box_flows_into_two_columns_then_sheds() {
        let v: Vec<f64> = (0..400)
            .map(|i| f64::from(i % 50) * 0.1 + if i % 97 == 0 { 40.0 } else { 0.0 })
            .collect();
        let d = timegrapher_core::distribution::distribution(
            &v,
            timegrapher_core::distribution::RATE,
            "readings",
            10.0,
        )
        .unwrap();
        let rows = d.fact_rows(timegrapher_core::distribution::RATE);
        assert_eq!(rows.len(), 4, "{rows:?}");
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let shed = timegrapher_core::distribution::Distribution::fact_shed(&rows);
                let b = facts_box(ui, "Rate", rows.clone(), shed);
                let at = |h: f32| {
                    egui_plot::PlotTransform::new(
                        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(600.0, h)),
                        egui_plot::PlotBounds::from_min_max([0.0, 0.0], [1.0, 1.0]),
                        egui::Vec2b::new(false, false),
                    )
                };
                let names =
                    |k: Vec<usize>| k.iter().map(|&i| rows[i].0.clone()).collect::<Vec<_>>();
                // Room for four rows: one column.
                let tall = at(100.0);
                assert_eq!(b.shown(&tall).len(), 4);
                let one = b.rect(*tall.frame(), 4);
                // Room for three: all four flow into two columns of two,
                // twice as wide, half as tall.
                let short = at(72.0);
                assert_eq!(b.shown(&short).len(), 4);
                let two = b.rect(*short.frame(), 4);
                assert!(
                    two.width() > 1.9 * (one.width() - 2.0 * crate::fact_box::FACT_PAD),
                    "{one:?} {two:?}"
                );
                assert!(two.height() < one.height(), "{one:?} {two:?}");
                // Room for one: two columns of one keep the median and the
                // values off the scale.
                assert_eq!(names(b.shown(&at(40.0))), ["Median", "Off the scale"]);
            });
        });
    }

    /// Rate, Amplitude and Beat Error alike, at one plot height with the
    /// values running across the plot under where one column of four rows
    /// would reach: each flows its four rows into two columns, clear of
    /// the values, before it sheds any (Walter and Hugh, #1535).
    #[test]
    fn every_series_flows_before_it_sheds() {
        use timegrapher_core::distribution::{self as dist, Distribution};
        let v: Vec<f64> = (0..400)
            .map(|i| f64::from(i % 50) * 0.1 + if i % 97 == 0 { 40.0 } else { 0.0 })
            .collect();
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                // Room for every row in one column, as at 1280 by 1300.
                let t = egui_plot::PlotTransform::new(
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 100.0)),
                    egui_plot::PlotBounds::from_min_max([0.0, 0.0], [1.0, 1.0]),
                    egui::Vec2b::new(false, false),
                );
                // Values across the whole width, 66 points down: under one
                // column's foot (4 + 2 x 8 + 4 x 16 = 84), below two's (52).
                let y = 1.0 - 66.0 / 100.0;
                let tops: Vec<[f64; 2]> = (0..=100).map(|i| [f64::from(i) / 100.0, y]).collect();
                for kind in [dist::RATE, dist::AMPLITUDE, dist::BEAT_ERROR] {
                    let d = dist::distribution(&v, kind, "readings", 10.0).unwrap();
                    let rows = d.fact_rows(kind);
                    assert_eq!(rows.len(), 4, "{}: {rows:?}", kind.series);
                    let shed = Distribution::fact_shed(&rows);
                    let mut b = facts_box(ui, kind.series, rows, shed);
                    b.tops = tops.clone();
                    let (shown, r) = b.layout(&t);
                    assert_eq!(shown.len(), 4, "{}: {shown:?}", kind.series);
                    assert!(r.height() < 60.0, "{}: one column {r:?}", kind.series);
                    assert!(!b.covers(&t, r, 0.0), "{}: {r:?}", kind.series);
                }
            });
        });
    }

    /// Three plots stacked keep the last one's axis inside the pane: each
    /// gap's layout spacing is counted (Walter's walk of #1535).
    #[test]
    fn the_last_plots_axis_stays_inside_the_pane() {
        let mut app = super::super::tests::synthetic(300.0, 5.0);
        app.bench.panes_view.dist_series = [true; 3];
        app.update_dist_readings(None);
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        for h in [400.0, 520.0, 700.0] {
            let mut bottom = (0.0, 0.0);
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, h),
                    )),
                    ..Default::default()
                };
                let mut inner = 0.0;
                let out = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        inner = ui.max_rect().bottom();
                        app.distributions_pane(ui);
                    });
                });
                let lowest = out
                    .shapes
                    .iter()
                    .filter(|c| matches!(c.shape, egui::Shape::Text(_)))
                    .map(|c| c.shape.visual_bounding_rect().bottom())
                    .fold(0.0, f32::max);
                bottom = (lowest, inner);
            }
            assert!(bottom.0 <= bottom.1 + 0.5, "{h}: {bottom:?}");
        }
    }

    /// A pane too short for every plot names what it leaves out in a grey
    /// chip under the first plot's title, not a line under the plots
    /// (Hugh, #1528).
    #[test]
    fn no_room_is_a_chip_under_the_first_title() {
        let mut app = super::super::tests::synthetic(122.0, 5.0);
        app.bench.panes_view.dist_series = [true; 3];
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut texts = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 200.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.distributions_pane(ui));
            });
            texts = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos.y)),
                    _ => None,
                })
                .collect();
        }
        let y = |w: &str| texts.iter().find(|t| t.0 == w).map(|t| t.1);
        let chip = "Amplitude and Beat Error not shown: no room";
        assert!(y(chip).is_some(), "{texts:?}");
        assert!(y("Rate") < y(chip), "{texts:?}");
        assert!(
            !texts.iter().any(|t| t.0.contains("Make the pane taller")),
            "{texts:?}"
        );
    }

    /// The readings the pane counts are core's (#1144): one per Average
    /// Over, read again when Average Over changes, with the beat error's
    /// source as core chose it over the windows they are made of.
    /// Listening runs on 2 s past the last whole reading: live settles a
    /// 2 s amplitude cell only once a beat's guard of sound follows it,
    /// and a reading needs every cell on the take's one tiling (#636), so
    /// at 120 s the last reading's amplitude is still to come.
    #[test]
    fn readings_are_cores_one_per_average_over() {
        let mut app = super::super::tests::synthetic(122.0, 5.0);
        app.bench.take.average_s = 10.0;
        app.update_dist_readings(None);
        let count = |app: &TimegrapherApp, k| app.hist_values(k, false, &theme::DARK).0;
        let rates = count(&app, SeriesKind::Rate);
        assert!(
            (9..=12).contains(&rates.len()),
            "{} rate readings",
            rates.len()
        );
        assert!(rates.iter().all(|r| (r - 5.0).abs() < 1.0), "{rates:?}");
        let amps = count(&app, SeriesKind::Amplitude);
        assert_eq!(amps.len(), rates.len());
        assert!(amps.iter().all(|a| (a - 280.0).abs() < 10.0), "{amps:?}");
        let (bes, .., selected) = app.hist_values(SeriesKind::BeatError, false, &theme::DARK);
        assert_eq!(bes.len(), rates.len());
        let core = app.bench.results.dist_readings.as_ref().unwrap();
        assert_eq!(
            selected.unwrap().beat_error_source,
            core.got.1.beat_error_source
        );
        app.bench.take.average_s = 20.0;
        app.update_dist_readings(None);
        assert!(
            app.bench.results.dist_readings.is_none(),
            "read on the readings at the old Average Over"
        );
        app.refresh_trend();
        app.update_dist_readings(None);
        let longer = count(&app, SeriesKind::Rate);
        assert!(
            (4..=6).contains(&longer.len()),
            "{} rate readings at 20 s",
            longer.len()
        );
    }

    /// A stretch no reading hears a watch in (the stand's noise, here a
    /// watch's beats the readings say are not one) is left out of the
    /// readings and cut at both ends, as the steadiness tests leave it
    /// out: no reading counts a beat of it (#2480).
    #[test]
    fn the_readings_leave_out_what_no_reading_hears() {
        let mut app = super::super::tests::synthetic(400.0, 5.0);
        app.bench.take.average_s = 10.0;
        for p in &mut app.bench.results.trend {
            if (150.0..=250.0).contains(&p.t) {
                p.heard = false;
            }
        }
        let end = app.bench.recording.live.as_ref().unwrap().duration_s();
        let unheard = timegrapher_core::live::unheard_spans(&app.bench.results.trend, 10.0, end);
        assert_eq!(unheard.len(), 1, "{unheard:?}");
        let (u, v) = unheard[0];
        app.update_dist_readings(None);
        let grids = &app.bench.results.dist_readings.as_ref().unwrap().got.0;
        let rate = &grids[0];
        let mut kept = 0;
        for (i, y) in rate.y.iter().enumerate() {
            let from = rate.t0 + i as f64 * rate.step;
            if y.is_finite() {
                kept += 1;
                assert!(
                    from + rate.step <= u + 1e-6 || from >= v - 1e-6,
                    "a reading over {from}..{} reaches into {u}..{v}",
                    from + rate.step
                );
            }
        }
        assert!(kept >= 20, "{kept} readings");
    }
}

#[cfg(test)]
mod plot_cache_tests {
    use super::*;

    /// One frame of `reading`'s distribution on an 800 × 400 panel: the
    /// circles it painted.
    fn circles(app: &mut TimegrapherApp, ctx: &egui::Context, reading: SeriesKind) -> usize {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(800.0, 400.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.histogram(ui, reading));
        });
        out.shapes
            .iter()
            .filter(|s| matches!(s.shape, egui::Shape::Circle(_)))
            .count()
    }

    /// A long take's 2 s rates, 40,000 of them, as a 22 h take has: what
    /// the plot draws is thinned to the screen, and a frame with nothing
    /// new works nothing out again (#2171). Each was done afresh every
    /// frame, 40,000 circles and a sort each time.
    #[test]
    fn a_long_takes_plot_is_worked_out_once_and_drawn_at_screen_size() {
        let mut app = super::super::tests::synthetic(30.0, 12.0);
        app.bench.panes_view.hist_readings = false;
        let ctx = super::super::tests::themed();
        let _ = circles(&mut app, &ctx, SeriesKind::Rate);
        let first = app.bench.panes_view.dist.rate_2s.values.len();
        assert!(first > 5, "{first}");
        // 40,000 more 2 s rates spread like a real watch's.
        let more = (0..40_000).map(|i| {
            let i = f64::from(i);
            (
                30.0 + 2.0 * i,
                12.0 + 8.0 * (i * 0.7).sin() + 3.0 * (i * 0.13).cos(),
            )
        });
        app.bench.panes_view.dist.rate_2s.values.extend(more);
        let drawn = circles(&mut app, &ctx, SeriesKind::Rate);
        let plot = app.bench.panes_view.dist.plots[SeriesKind::Rate as usize]
            .clone()
            .unwrap();
        assert_eq!(plot.n, first + 40_000);
        assert!(plot.pts.len() > 39_000, "{}", plot.pts.len());
        // The plot is about 740 × 350 pt: a curve across it needs no
        // more than a point every half a point along its length, even
        // on the frame the values change.
        assert!((100..4_400).contains(&drawn), "{drawn} circles");
        // Nothing new: the same plot, not worked out again.
        let _ = circles(&mut app, &ctx, SeriesKind::Rate);
        let again = app.bench.panes_view.dist.plots[SeriesKind::Rate as usize]
            .clone()
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(&plot, &again));
        // A new stretch, the other view, another Average Over, another
        // position, or a new mask: worked out again.
        app.bench
            .panes_view
            .dist
            .rate_2s
            .values
            .push((80_100.0, 12.0));
        let changes: [fn(&mut TimegrapherApp); 5] = [
            |_| {},
            |a| a.bench.panes_view.hist_cumulative = true,
            |a| a.bench.take.average_s = 20.0,
            |a| a.bench.take.position = Position::DialDown,
            |a| a.bench.take.rev.masks += 1,
        ];
        let mut was = again;
        for change in changes {
            change(&mut app);
            let _ = circles(&mut app, &ctx, SeriesKind::Rate);
            let now = app.bench.panes_view.dist.plots[SeriesKind::Rate as usize]
                .clone()
                .unwrap();
            assert!(!std::sync::Arc::ptr_eq(&was, &now));
            was = now;
        }
        assert_eq!(was.n, first + 40_001);
    }

    /// The Amplitude plot is worked out again when listening brings a
    /// new 2 s window, and for a break listening adds, a move or a
    /// settling, none of which moves the take's [`Revision`] (#2171).
    #[test]
    fn the_amplitude_plot_follows_new_windows_breaks_moves_and_settling() {
        use timegrapher_core::synth::{generate, SynthConfig};
        // One watch heard for 40 s, the last 10 s held back.
        let audio = generate(
            &SynthConfig {
                duration_s: 40.0,
                rate_s_per_day: 12.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let (first, rest) = audio.samples.split_at(48_000 * 30);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.reset_session(48_000, "syn".into());
        let push = |app: &mut TimegrapherApp, s: &[f32]| {
            let live = app.bench.recording.live.as_mut().unwrap();
            for b in s.chunks(960) {
                live.push(b);
            }
        };
        push(&mut app, first);
        app.bench.panes_view.hist_readings = false;
        let ctx = super::super::tests::themed();
        let amp = SeriesKind::Amplitude as usize;
        let plot = |app: &mut TimegrapherApp| {
            let _ = circles(app, &ctx, SeriesKind::Amplitude);
            app.bench.panes_view.dist.plots[amp].clone().unwrap()
        };
        let mut was = plot(&mut app);
        assert!(was.n > 5, "{}", was.n);
        let rev = app.bench.take.rev;
        // The last 10 s heard: new windows and no break, worked out again.
        push(&mut app, rest);
        assert!(app
            .bench
            .recording
            .live
            .as_ref()
            .unwrap()
            .breaks()
            .is_empty());
        let changes: [fn(&mut TimegrapherApp); 4] = [
            |_| {},
            |a| {
                a.bench
                    .recording
                    .live
                    .as_mut()
                    .unwrap()
                    .set_breaks(vec![15.0])
            },
            |a| {
                let stretches = [(0.0, Position::DialUp), (20.0, Position::DialDown)];
                a.session.load(&stretches, &[]);
            },
            |a| {
                let stretches = [(0.0, Position::DialUp), (20.0, Position::DialDown)];
                a.session.load(&stretches, &[(20.0, 25.0)]);
            },
        ];
        for change in changes {
            change(&mut app);
            let now = plot(&mut app);
            assert!(!std::sync::Arc::ptr_eq(&was, &now));
            was = now;
        }
        assert_eq!(
            app.bench.take.rev, rev,
            "each change above moved no revision"
        );
    }

    /// With Values on Readings, the plot is worked out again when core's
    /// readings come in from the background (#2171).
    #[test]
    fn the_readings_plot_follows_cores_readings_as_they_come() {
        let mut app = super::super::tests::synthetic(122.0, 5.0);
        app.bench.take.average_s = 10.0;
        app.bench.panes_view.hist_readings = true;
        let ctx = super::super::tests::themed();
        let rate = SeriesKind::Rate as usize;
        let _ = circles(&mut app, &ctx, SeriesKind::Rate);
        let before = app.bench.panes_view.dist.plots[rate].clone().unwrap();
        assert_eq!(before.n, 0, "no readings read yet");
        app.update_dist_readings(None);
        let _ = circles(&mut app, &ctx, SeriesKind::Rate);
        let after = app.bench.panes_view.dist.plots[rate].clone().unwrap();
        assert!(after.n >= 9, "{} readings", after.n);
    }

    /// Thinning keeps both ends and leaves every value's point within
    /// half a point of one drawn, so what shows is the same.
    #[test]
    fn thinning_leaves_every_point_within_half_a_point_of_one_drawn() {
        let t = egui_plot::PlotTransform::new(
            egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(600.0, 300.0)),
            egui_plot::PlotBounds::from_min_max([0.0, -3.5], [100.0, 3.5]),
            egui::Vec2b::new(false, false),
        );
        let pts: Vec<[f64; 2]> = (0..20_000)
            .map(|i| {
                let q = (f64::from(i) + 0.5) / 20_000.0;
                let z = timegrapher_core::histogram::normal_quantile(q);
                [50.0 + 12.0 * z, z]
            })
            .collect();
        let kept = on_screen(&pts, &t, ([0.0, 100.0], [-3.5, 3.5]));
        assert_eq!(kept.first(), pts.first());
        assert_eq!(kept.last(), pts.last());
        assert!(kept.len() < 3_000, "{}", kept.len());
        let at = |p: &[f64; 2]| t.position_from_point(&egui_plot::PlotPoint::new(p[0], p[1]));
        for p in &pts {
            let near = kept
                .iter()
                .map(|k| (at(k) - at(p)).length())
                .fold(f32::INFINITY, f32::min);
            assert!(near < 0.5, "{p:?} is {near} pt from a drawn point");
        }
        // A last value within half a point of the one drawn before it is
        // still drawn: the curve reaches its end.
        let close = [[50.0, 0.0], [50.01, 0.001], [50.02, 0.002]];
        let kept = on_screen(&close, &t, ([0.0, 100.0], [-3.5, 3.5]));
        assert_eq!(kept, vec![close[0], close[2]]);
    }
}
