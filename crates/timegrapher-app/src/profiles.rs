//! The tick and tock sound pane: the typical sound of the ticks and of the
//! tocks, with the band most beats fall in and the marks the engine read
//! the amplitude and beat error from.

use crate::fact_box::{around_plate, widest, FactBox};
use crate::marks::{self, Rank, LANE, LANE_GAP, ROW_H};
use crate::strength::{self, Grid, Strength};
use crate::strip::side_colors;
use crate::theme;
use eframe::egui::{self, Color32};
use egui_plot::{HLine, Line, LineStyle, Plot, Polygon};
use timegrapher_core::figure_text::true_minus;
// The facts on each plot, a row each, top to bottom, and a time's words:
// core's (#510).
pub use timegrapher_core::profile::ms_text;
use timegrapher_core::profile::{facts, TickProfile, FACT_NAMES};

/// How the sound's level is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Scale {
    /// The envelope as it is.
    Linear,
    /// Decibels below the loudest point, so quiet sounds such as the
    /// unlock show as clearly as the drop.
    Decibels,
}

/// Lowest level drawn on the decibel scale.
const FLOOR_DB: f64 = -50.0;

/// One side's level at each point, scaled for drawing.
fn scaled(v: &[f32], top: f32, scale: Scale) -> Vec<f64> {
    v.iter()
        .map(|&x| match scale {
            Scale::Linear => x as f64,
            Scale::Decibels => {
                (20.0 * (x.max(1e-12) as f64 / top.max(1e-12) as f64).log10()).max(FLOOR_DB)
            }
        })
        .collect()
}

/// A linear level's axis label, with as many decimals as its grid step
/// needs, so close marks never read alike.
fn level_label(value: f64, step: f64) -> String {
    let places = if step > 0.0 {
        (-step.log10() - 1e-9).ceil().clamp(0.0, 6.0) as usize
    } else {
        2
    };
    format!("{:.*}", places, value + 0.0)
}

/// Below this height a plot's facts keep only their last row, Unlock to
/// Drop; the rest are in the hover. Its names go inside
/// it below the same height.
const SHORT_PLOT: f32 = marks::SHORT_PLOT;
/// The least height a side takes stacked: its name and a plot of 40 pt.
const STACKED_MIN: f32 = 64.0;

/// A side's facts before it has beats.
const NO_FACTS: &str = "Not enough beats yet";

/// Which of a side's four facts its box shows, top to bottom: on a plot
/// `plot_h` tall with room for `fit_frame` rows inside it, all four, else
/// (as on a short plot) Unlock to Drop alone. The same in dB as in linear,
/// so nothing moves with the scale. The hover has all four.
fn keep_rows(plot_h: f32, fit_frame: usize) -> Vec<usize> {
    let by_height = if plot_h >= SHORT_PLOT && fit_frame >= 4 {
        4
    } else {
        fit_frame.min(1)
    };
    let mut keep = [3, 0, 1, 2][..by_height].to_vec();
    keep.sort_unstable();
    keep
}

/// How the pane is drawn: set in its toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Options {
    pub scale: Scale,
    /// Tick beside tock rather than above it.
    pub side_by_side: bool,
    /// The unlock, drop and drop peak edges amplitude and beat error are
    /// read from.
    pub edges: bool,
    /// Where the three sounds rise.
    pub sounds: bool,
    /// Both sides on one clock, each moved half the beat error from where
    /// a watch in beat would put it, rather than each from its own drop.
    pub shared_clock: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            scale: Scale::Linear,
            side_by_side: false,
            edges: true,
            sounds: true,
            shared_clock: true,
        }
    }
}

/// How far each side is drawn from its own drop, ms. On the shared clock
/// the tick (even beats) sits half the beat error from the time a watch in
/// beat would drop, and the tock half the other way, so the drops stand a
/// beat error apart and the unlocks an unlock beat error apart.
pub fn offsets(opt: Options, drop_beat_error_ms: Option<f64>) -> [f64; 2] {
    match (opt.shared_clock, drop_beat_error_ms) {
        (true, Some(be)) if be.is_finite() => [be / 2.0, -be / 2.0],
        _ => [0.0, 0.0],
    }
}

/// Draw both sides, tick above or beside tock, on one scale so their
/// loudness compares. `drop_beat_error_ms` places them on the shared clock.
pub fn draw(
    ui: &mut egui::Ui,
    profiles: &[Option<TickProfile>; 2],
    opt: Options,
    drop_beat_error_ms: Option<f64>,
    note: Option<&str>,
    band: &str,
) {
    let shift = offsets(opt, drop_beat_error_ms);
    let pal = theme::pal(ui);
    let colors = side_colors(ui.visuals().dark_mode);
    let top = profiles
        .iter()
        .flatten()
        .flat_map(|p| p.p90.iter().copied())
        .fold(0.0f32, f32::max);
    let scale = opt.scale;
    let (y_lo, y_hi) = match scale {
        Scale::Linear => (0.0, (top as f64 * 1.08).max(1e-6)),
        Scale::Decibels => (FLOOR_DB, 3.0),
    };
    let facts: Vec<Option<[String; 4]>> = profiles.iter().map(|p| p.as_ref().map(facts)).collect();
    let boxes = fact_boxes(ui, &facts);
    // `axis`: the time axis under this side; stacked, Tock's serves both,
    // as their time runs together.
    let side = |ui: &mut egui::Ui, k: usize, axis: bool| {
        // The lanes keep their own gaps: no spacing between them and the plot.
        ui.spacing_mut().item_spacing.y = 0.0;
        let p = &profiles[k];
        let dx = shift[k];
        let name = ["Tick", "Tock"][k];
        let c = colors[k];
        let (short, top_lane, bottom_lane) = lanes(ui, k, opt, axis);
        side_name(ui, top_lane, name, c);
        let h = (ui.available_height() - 20.0).max(20.0) as f64;
        let plot_h = (ui.available_height() - bottom_lane).max(24.0);
        let mut facts_here = FactBox {
            plot_h,
            ..boxes[k].clone()
        };
        let empty = p.is_none();
        let grid = Grid::new(
            |g| theme::even_grid(g, 80.0, &[]),
            |g| theme::even_grid(g, 30.0, &[]),
        );
        let plot = profile_plot(&grid, k, plot_h, h, empty, (y_lo, y_hi), scale);
        let mut med = Vec::new();
        // The trace at each time: where an edge's name inside a short plot
        // finds a clear spot.
        let tops: Vec<[f64; 2]> = p.as_ref().map_or_else(Vec::new, |p| {
            scaled(&p.median, top, scale)
                .iter()
                .enumerate()
                .map(|(i, v)| [p.t0_ms + dx + i as f64 * p.step_ms, *v])
                .collect()
        });
        if short {
            facts_here.tops = tops.clone();
        }
        // A short plot's edge names, drawn in it.
        let mut edge_names = p
            .as_ref()
            .filter(|_| short && opt.edges)
            .map(|p| short_edge_names(ui, p, top, scale, dx));
        let resp = plot.show(ui, |pl| {
            pl.set_plot_bounds_y(y_lo..=y_hi);
            grid.draw(pl, pal);
            // The noise floor, a call-out under the facts box, which keeps
            // its rows over it in either scale.
            if let Some(p) = p {
                let s = strength::stroke(Strength::CallOut, pal);
                pl.hline(
                    HLine::new("noise floor", scaled(&[p.floor], top, scale)[0])
                        .color(s.color)
                        .width(s.width)
                        .style(LineStyle::dashed_dense()),
                );
            }
            // Over the grid, under the band and the trace.
            pl.add(facts_here.clone());
            let Some(p) = p else { return };
            if let Some(names) = edge_names.take() {
                pl.add(EdgeNames {
                    base: egui_plot::PlotItemBase::new(format!("{name} edge names")),
                    names,
                    tops: tops.clone(),
                    facts: facts_here.clone(),
                    fill: facts_here.fill,
                    ink: pal.text,
                    lines: [pal.unlock, pal.drop, pal.peak],
                });
            }
            med = paint_trace(pl, p, dx, top, scale, (c, band));
        });
        if let Some(f) = &facts[k] {
            facts_hover(&resp, &facts_here, f, note);
        }
        let (foot, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), bottom_lane),
            egui::Sense::hover(),
        );
        if axis {
            time_axis(ui, &resp.transform, foot);
        }
        // Each plot's height and foot, for the layout's tests.
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new(("profile plot", k)),
                (*resp.transform.frame(), foot),
            )
        });
        if let Some(p) = p {
            let facts = (short, &facts_here);
            label_marks(ui, &resp.transform, p, (&med, &tops), opt, dx, (k, facts));
        }
    };
    arrange(ui, opt, side);
}

/// Stacked, each side's share; too short for two plots one above the
/// other, they go side by side, each the pane's height.
fn arrange(ui: &mut egui::Ui, opt: Options, side: impl Fn(&mut egui::Ui, usize, bool)) {
    let gap = 6.0;
    let stacked_h = (ui.available_height() - gap - AXIS_H - 2.0) / 2.0;
    if opt.side_by_side || stacked_h < STACKED_MIN {
        ui.columns(2, |cols| {
            for (k, ui) in cols.iter_mut().enumerate() {
                side(ui, k, true);
            }
        });
    } else {
        // Tick above Tock, the one time axis under Tock: Tock's share is
        // the axis taller, so the two plots are the same height.
        ui.spacing_mut().item_spacing.y = 0.0;
        let w = ui.available_width();
        let h = stacked_h;
        for k in 0..2 {
            let h = if k == 1 { h + AXIS_H + 2.0 } else { h };
            ui.allocate_ui(egui::vec2(w, h), |ui| {
                ui.set_height(h);
                side(ui, k, k == 1)
            });
            if k == 0 {
                ui.add_space(gap);
            }
        }
    }
}

/// Each side's facts box, its height set where it is drawn: the columns'
/// widths are taken over both sides, so the two boxes match.
fn fact_boxes(ui: &egui::Ui, facts: &[Option<[String; 4]>]) -> [FactBox; 2] {
    let pal = theme::pal(ui);
    let name_w = widest(ui, FACT_NAMES.iter().copied());
    let value_w = widest(
        ui,
        facts
            .iter()
            .flatten()
            .flat_map(|f| f.iter().map(String::as_str)),
    );
    // The row a short plot keeps, Unlock to Drop.
    let short_name_w = widest(ui, FACT_NAMES[3..].iter().copied());
    let short_value_w = widest(
        ui,
        facts
            .iter()
            .flatten()
            .flat_map(|f| f[3..].iter().map(String::as_str)),
    );
    let none_w = widest(ui, [NO_FACTS].into_iter());
    let box_fill = ui.visuals().extreme_bg_color.gamma_multiply(0.8);
    let (here, ctx) = (ui.id(), ui.ctx().clone());
    let fact_box = |k: usize| {
        let held = here.with(("facts box", k));
        let rows = match &facts[k] {
            Some(f) => FACT_NAMES
                .iter()
                .zip(f)
                .map(|(n, v)| (n.to_string(), v.clone()))
                .collect(),
            None => vec![(NO_FACTS.to_string(), String::new())],
        };
        FactBox {
            base: egui_plot::PlotItemBase::new(format!("{} facts", ["Tick", "Tock"][k])),
            plot_h: 0.0,
            tops: Vec::new(),
            held,
            was: ctx.data(|d| d.get_temp(held)),
            name_w: if facts[k].is_some() { name_w } else { none_w },
            value_w: if facts[k].is_some() { value_w } else { 0.0 },
            short_name_w,
            short_value_w,
            rows,
            fill: box_fill,
            name_color: pal.text_secondary,
            value_color: pal.text,
            keep: facts[k]
                .is_some()
                .then_some(keep_rows as fn(f32, usize) -> Vec<usize>),
            shed: Vec::new(),
            flow: false,
        }
    };
    [fact_box(0), fact_box(1)]
}

/// Whether a side's plot is short, and the heights of the lanes above and
/// below it. `axis`: the time axis goes under this side.
fn lanes(ui: &egui::Ui, k: usize, opt: Options, axis: bool) -> (bool, f32, f32) {
    // The edges' names in a lane above the plot, the sounds' in a lane
    // below it, then the time axis: none of them inside the plot.
    // Each lane as many rows as its names took last frame, so the plot
    // keeps what a second row isn't using.
    let rows = |up: bool| lane_rows(ui, k, up) as f32 * ROW_H;
    let axis_h = if axis { AXIS_H + 2.0 } else { 0.0 };
    let top_lane = if opt.edges { rows(true) } else { ROW_H } + LANE_GAP;
    let bottom_lane = if opt.sounds {
        rows(false) + LANE_GAP
    } else {
        0.0
    } + axis_h;
    // A plot that would be short with its names outside takes them in:
    // the edges' along its top, the sounds' to their rings' hovers.
    let outside_h = ui.available_height() - top_lane - bottom_lane;
    let short = marks::short(ui, ui.id().with(("profile short", k)), outside_h);
    let (top_lane, bottom_lane) = if short {
        (ROW_H + 2.0, axis_h + 4.0)
    } else {
        (top_lane, bottom_lane)
    };
    (short, top_lane, bottom_lane)
}

/// A side's plot on `grid`, fixed in place: `h` its height for the level
/// labels, which `empty` leaves off.
fn profile_plot(
    grid: &Grid,
    k: usize,
    plot_h: f32,
    h: f64,
    empty: bool,
    (y_lo, y_hi): (f64, f64),
    scale: Scale,
) -> Plot<'static> {
    let plot = grid
        .on(Plot::new(("profile", k)))
        .height(plot_h)
        .link_axis("profile", [true, false])
        // No crosshair or value box: the marks and the facts above say
        // what matters.
        .show_x(false)
        .show_y(false)
        .allow_scroll(false)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_axis_zoom_drag(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .default_y_bounds(y_lo, y_hi)
        // The time axis is drawn under the sounds' names, below.
        .show_axes([false, true])
        .y_axis_min_width(52.0)
        .y_axis_formatter(move |m, r| {
            // Before there are beats there is no level to mark.
            if empty || (m.value - r.start()) / (r.end() - r.start()) * h < 9.0 {
                return String::new();
            }
            match scale {
                Scale::Decibels => crate::steady::db_label(m.value),
                Scale::Linear => level_label(m.value, m.step_size),
            }
        });
    // Before there are beats, a typical stretch of time rather than none.
    if empty {
        plot.default_x_bounds(-20.0, 10.0)
    } else {
        plot
    }
}

/// A side's name, in its colour, in the lane above its plot.
fn side_name(ui: &mut egui::Ui, top_lane: f32, name: &str, c: Color32) {
    let (head, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), top_lane),
        egui::Sense::hover(),
    );
    ui.painter().text(
        egui::pos2(head.left(), head.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        name,
        theme::semibold(12.5),
        c,
    );
}

/// The edges a short plot names inside it: each name, its time `dx`
/// along, its width, and the trace's level there.
fn short_edge_names(
    ui: &egui::Ui,
    p: &TickProfile,
    top: f32,
    scale: Scale,
    dx: f64,
) -> Vec<(&'static str, f64, f32, Option<f64>)> {
    let level = scaled(&p.median, top, scale);
    edge_marks(p)
        .into_iter()
        .map(|(l, v)| {
            let i = ((v - p.t0_ms) / p.step_ms).round();
            let y = (i >= 0.0).then(|| level.get(i as usize).copied()).flatten();
            (l, v + dx, name_width(ui, l), y)
        })
        .collect()
}

/// What a profile's band is: the middle 80% of its beats.
pub const BAND_BEATS: &str = "middle 80% of beats";
/// What an averaged profile's band is ([`timegrapher_core::profile::profile_at`]).
pub const BAND_AVERAGED: &str = "range of the 2 s bands";

/// A side's typical sound in colour `c` and its band (named `band`),
/// `dx` along: the typical sound's levels, as plotted.
fn paint_trace(
    pl: &mut egui_plot::PlotUi,
    p: &TickProfile,
    dx: f64,
    top: f32,
    scale: Scale,
    (c, band_name): (Color32, &str),
) -> Vec<f64> {
    let x = |i: usize| p.t0_ms + dx + i as f64 * p.step_ms;
    let (m, lo, hi) = (
        scaled(&p.median, top, scale),
        scaled(&p.p10, top, scale),
        scaled(&p.p90, top, scale),
    );
    // The band as thin quads: plots fill only convex shapes.
    let band = c.gamma_multiply(0.25);
    for i in 1..m.len().min(lo.len()).min(hi.len()) {
        pl.polygon(
            Polygon::new(
                band_name,
                vec![
                    [x(i - 1), lo[i - 1]],
                    [x(i), lo[i]],
                    [x(i), hi[i]],
                    [x(i - 1), hi[i - 1]],
                ],
            )
            .fill_color(band)
            .stroke(egui::Stroke::NONE),
        );
    }
    pl.line(
        Line::new(
            "typical sound",
            m.iter()
                .enumerate()
                .map(|(i, &v)| [x(i), v])
                .collect::<Vec<_>>(),
        )
        .color(c)
        .width(1.8_f32),
    );
    m
}

/// A box that keeps fewer rows: the hover has all four. With a `note` on
/// what the profiles are (a moment's sound kept coarsely), the hover leads
/// with it, however many rows the box keeps.
fn facts_hover(
    resp: &egui_plot::PlotResponse<()>,
    facts_here: &FactBox,
    f: &[String; 4],
    note: Option<&str>,
) {
    let (shown, r) = facts_here.layout(&resp.transform);
    if shown.len() < FACT_NAMES.len() || note.is_some() {
        // Where no row is drawn, the corner a one-row box would take.
        let r = if shown.is_empty() {
            facts_here.corners(&resp.transform, 1, 1).0
        } else {
            r
        };
        if resp.response.hover_pos().is_some_and(|at| r.contains(at)) {
            let all: Vec<String> = note
                .map(str::to_string)
                .into_iter()
                .chain(FACT_NAMES.iter().zip(f).map(|(n, v)| format!("{n}: {v}")))
                .collect();
            resp.response.clone().on_hover_text(all.join("\n"));
        }
    }
}

/// The edges a side names, with their times: Unlock, Drop and Peak, those
/// it has.
fn edge_marks(p: &TickProfile) -> Vec<(&'static str, f64)> {
    [
        ("Unlock", p.unlock_ms),
        ("Drop", p.drop_ms),
        ("Peak", p.peak_ms),
    ]
    .into_iter()
    .filter_map(|(l, v)| v.map(|v| (l, v)))
    .collect()
}

/// An edge's name's width in its type, points.
fn name_width(ui: &egui::Ui, label: &str) -> f32 {
    ui.fonts_mut(|f| {
        f.layout_no_wrap(label.to_owned(), theme::semibold(11.5), Color32::WHITE)
            .size()
            .x
    })
}

fn named_w(named: &[(&str, f32, f32)], label: &str) -> f32 {
    named.iter().find(|n| n.0 == label).map_or(0.0, |n| n.2)
}

/// The edges' names, each its line's x and width, for [`marks`]: every
/// edge is named strong.
fn strong(named: &[(&str, f32, f32)]) -> Vec<(f32, f32, Rank)> {
    named
        .iter()
        .map(|&(_, x, w)| (x, w, Rank::Strong))
        .collect()
}

/// The edges' names inside a short plot, as a plot item so they sit over
/// the grid and under the band and the trace: each on the plot's
/// background, its line from it down to just short of the trace.
struct EdgeNames {
    base: egui_plot::PlotItemBase,
    /// Each edge's label, time as drawn, name width and the trace's level
    /// there.
    names: Vec<(&'static str, f64, f32, Option<f64>)>,
    /// The trace at each time, plot coordinates.
    tops: Vec<[f64; 2]>,
    /// The facts box, which keeps its corner.
    facts: FactBox,
    fill: Color32,
    ink: Color32,
    /// Unlock's, Drop's and Peak's lines.
    lines: [Color32; 3],
}

impl egui_plot::PlotItem for EdgeNames {
    fn shapes(
        &self,
        ui: &egui::Ui,
        transform: &egui_plot::PlotTransform,
        shapes: &mut Vec<egui::Shape>,
    ) {
        let frame = *transform.frame();
        let named: Vec<(&str, f32, f32)> = self
            .names
            .iter()
            .map(|&(l, v, w, _)| (l, transform.position_from_point_x(v), w))
            .filter(|n| n.1 >= frame.left() && n.1 <= frame.right())
            .collect();
        let at = marks::inside(
            &strong(&named),
            frame.left(),
            frame.right(),
            self.facts.span(transform),
            &|a, b| marks::row_clear(transform, &self.tops, a, b),
        );
        // The names go on after every line, on their boxes, so no other
        // edge's line runs through one.
        let mut names = Vec::new();
        for ((label, x, w), at) in named.iter().zip(at) {
            let level = self.names.iter().find(|n| n.0 == *label).and_then(|n| n.3);
            let stop = level.map_or(frame.bottom(), |y| {
                (transform.position_from_point_y(y) - 4.0).max(frame.top())
            });
            let ink = match *label {
                "Unlock" => self.lines[0],
                "Drop" => self.lines[1],
                _ => self.lines[2],
            };
            let from = match at {
                Some(a) => {
                    let r = marks::inside_rect(frame, a, *w);
                    names.push((r, a, *label));
                    r.bottom()
                }
                None => frame.top(),
            };
            // Stopped at the facts box's plate, and on below it (#1633).
            for (a, b) in around_plate(self.facts.plate(transform), *x, from, stop) {
                shapes.push(egui::Shape::line_segment(
                    [egui::pos2(*x, a), egui::pos2(*x, b)],
                    egui::Stroke::new(1.2_f32, ink),
                ));
            }
        }
        for (r, a, label) in names {
            let g = marks::galley(ui, label, Rank::Strong, false);
            shapes.extend(marks::inside_name(r, a, g, self.fill));
        }
    }

    fn initialize(&mut self, _x_range: std::ops::RangeInclusive<f64>) {}

    fn color(&self) -> Color32 {
        self.ink
    }

    fn geometry(&self) -> egui_plot::PlotGeometry<'_> {
        egui_plot::PlotGeometry::None
    }

    fn bounds(&self) -> egui_plot::PlotBounds {
        egui_plot::PlotBounds::NOTHING
    }

    fn base(&self) -> &egui_plot::PlotItemBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut egui_plot::PlotItemBase {
        &mut self.base
    }
}

/// The time axis's labels under the sounds' lane.
const AXIS_H: f32 = 16.0;

/// How many rows of names a side's lane took last frame (`up` for the
/// edges' above the plot): one, or two where names crowded.
fn lane_rows(ui: &egui::Ui, side: usize, up: bool) -> u8 {
    let rows = marks::rows_were(ui, lane_id(ui, side, up));
    1 + rows.into_iter().max().unwrap_or(0).min(1)
}

/// Where a lane keeps the rows its names took, for the next frame.
fn lane_id(ui: &egui::Ui, side: usize, up: bool) -> egui::Id {
    ui.id().with(("profile lane", side, up))
}

/// The time axis under a side, in ms, its labels where the plot's
/// grid lines are.
fn time_axis(ui: &egui::Ui, t: &egui_plot::PlotTransform, foot: egui::Rect) {
    let frame = *t.frame();
    let pal = theme::pal(ui);
    let b = t.bounds();
    let (lo, hi) = (b.min()[0], b.max()[0]);
    if hi <= lo || hi.is_nan() || lo.is_nan() || frame.width() <= 0.0 {
        return;
    }
    let want = (hi - lo) / frame.width() as f64 * 80.0;
    let step = timegrapher_core::chart_scale::round_step(want);
    let painter = ui.painter_at(foot.expand2(egui::vec2(40.0, 0.0)));
    let mut k = (lo / step).ceil();
    while k * step <= hi {
        let v = k * step;
        let x = t.position_from_point_x(v);
        // No label hanging past the right edge.
        if frame.right() - x >= 24.0 {
            painter.text(
                egui::pos2(x, foot.bottom()),
                egui::Align2::CENTER_BOTTOM,
                true_minus(&format!("{} ms", v + 0.0)),
                egui::FontId::proportional(11.0),
                pal.text_secondary,
            );
        }
        k += 1.0;
    }
}

/// Name the marks outside the plot: each edge (Unlock, Drop, Peak) a solid
/// grey line coming down from its name above the plot, stopping just short
/// of the trace, and a short solid bar across the trace where it meets it;
/// each sound a hollow ring on the trace where it has risen halfway, with a
/// dotted leader straight down past the plot to its name below. Solid and
/// bold for an edge, hollow, dotted and italic for a sound; where an edge
/// and a sound meet the trace together, the bar sits inside the ring. A
/// name hides any other mark's line passing behind it.
#[allow(clippy::too_many_arguments)]
fn label_marks(
    ui: &egui::Ui,
    t: &egui_plot::PlotTransform,
    p: &TickProfile,
    (med, tops): (&[f64], &[[f64; 2]]),
    opt: Options,
    dx: f64,
    (side, (short, facts)): (usize, (bool, &FactBox)),
) {
    // The span a short plot's names keep off, and the plate a line stops
    // at (#1633).
    let inside = short.then(|| facts.span(t));
    let plate = facts.plate(t);
    let frame = *t.frame();
    let m = Marker {
        ui,
        t,
        p,
        med,
        tops,
        dx,
        side,
        frame,
        pal: theme::pal(ui),
        painter: ui.painter_at(frame.expand2(egui::vec2(0.0, LANE + LANE_GAP + 2.0))),
        // Where the trace is marked: an edge's bar, a sound's ring, both
        // drawn over the trace, smaller on a short plot so they don't fill
        // it, and left off where they would cross an edge's name drawn in
        // the plot.
        r: (frame.height() / 8.0).clamp(2.5, 5.0),
        names_in: std::cell::RefCell::new(Vec::new()),
        plate,
    };
    if opt.edges {
        m.lane(edge_marks(p), true, inside);
    }
    if opt.sounds {
        m.lane(
            [
                ("Unlock Sound", p.sound1_ms),
                ("Impulse Sound", p.sound2_ms),
                ("Drop Sound", p.sound3_ms),
            ]
            .into_iter()
            .filter_map(|(l, v)| v.map(|v| (l, v)))
            .collect(),
            false,
            inside,
        );
    }
}

/// What `label_marks` draws with: a side's plot, its trace, and the edges'
/// names already drawn inside it.
struct Marker<'a> {
    ui: &'a egui::Ui,
    t: &'a egui_plot::PlotTransform,
    p: &'a TickProfile,
    med: &'a [f64],
    tops: &'a [[f64; 2]],
    dx: f64,
    side: usize,
    frame: egui::Rect,
    pal: &'static theme::Palette,
    painter: egui::Painter,
    /// A mark's radius on the trace.
    r: f32,
    names_in: std::cell::RefCell<Vec<egui::Rect>>,
    /// The facts box's plate, which a line stops at (#1633).
    plate: Option<egui::Rect>,
}

/// A mark in a lane: where it is across the plot, its name and its time.
type LaneMark<'s> = (f32, &'s str, f64);

impl Marker<'_> {
    fn edge_ink(&self, label: &str) -> Color32 {
        match label {
            "Unlock" => self.pal.unlock,
            "Drop" => self.pal.drop,
            _ => self.pal.peak,
        }
    }

    /// The trace's height at a time, on the screen.
    fn trace_y(&self, v: f64) -> Option<f32> {
        let p = self.p;
        let i = ((v - p.t0_ms) / p.step_ms).round();
        (i >= 0.0)
            .then(|| self.med.get(i as usize).copied())
            .flatten()
            .map(|y| self.t.position_from_point_y(y))
    }

    fn clear(&self, at: egui::Pos2) -> bool {
        let r = self.r;
        let mark = egui::Rect::from_center_size(at, egui::vec2(2.0 * r + 2.0, 2.0 * r + 2.0));
        !self.names_in.borrow().iter().any(|n| n.intersects(mark))
    }

    fn edge_bar(&self, at: egui::Pos2) {
        if self.clear(at) {
            let r = self.r;
            self.painter.line_segment(
                [at - egui::vec2(r - 1.0, 0.0), at + egui::vec2(r - 1.0, 0.0)],
                egui::Stroke::new(2.0_f32, self.pal.text),
            );
        }
    }

    fn sound_ring(&self, at: egui::Pos2) {
        if self.clear(at) {
            self.painter.circle_stroke(
                at,
                self.r,
                egui::Stroke::new(1.5_f32, self.pal.text_secondary),
            );
        }
    }

    fn hover(&self, hit: egui::Rect, label: &str, v: f64) {
        marks::hover(
            self.ui,
            self.ui.id().with(("mark", self.side, label)),
            hit,
            format!("{label}: {}", ms_text(v)),
        );
    }

    /// A lane laid out and drawn: `marks` are each name and its time; `up`
    /// for the edges above the plot.
    fn lane(&self, marks: Vec<(&str, f64)>, up: bool, inside: Option<(f32, f32)>) {
        let frame = self.frame;
        let mut marks: Vec<LaneMark> = marks
            .into_iter()
            .map(|(l, v)| (self.t.position_from_point_x(v + self.dx), l, v))
            .filter(|(x, _, _)| *x >= frame.left() && *x <= frame.right())
            .collect();
        marks.sort_by(|a, b| a.0.total_cmp(&b.0));
        // The edges' names semibold, the sounds' regular, italic and
        // secondary.
        let rank = if up { Rank::Strong } else { Rank::Faint };
        let galleys: Vec<_> = marks
            .iter()
            .map(|(_, l, _)| marks::galley(self.ui, l, rank, !up))
            .collect();
        // A short plot: the edges' names are drawn in it, under the trace
        // (see `EdgeNames`), and the sounds' are their rings' hovers.
        match inside {
            Some(facts) if up => self.edges_inside(&marks, &galleys, facts),
            Some(_) => self.sounds_inside(&marks),
            None => self.lane_outside(&marks, galleys, up),
        }
    }

    fn edges_inside(
        &self,
        marks: &[LaneMark],
        galleys: &[std::sync::Arc<egui::Galley>],
        facts: (f32, f32),
    ) {
        let frame = self.frame;
        let named: Vec<(&str, f32, f32)> = marks
            .iter()
            .zip(galleys)
            .map(|((x, l, _), g)| (*l, *x, g.size().x))
            .collect();
        let at = marks::inside(
            &strong(&named),
            frame.left(),
            frame.right(),
            facts,
            &|a, b| marks::row_clear(self.t, self.tops, a, b),
        );
        self.names_in
            .borrow_mut()
            .extend(named.iter().zip(&at).filter_map(|((l, _, _), a)| {
                a.map(|a| marks::inside_rect(frame, a, named_w(&named, l)))
            }));
        for ((x, label, v), at) in marks.iter().zip(at) {
            if let Some(y) = self.trace_y(*v + 0.0) {
                self.edge_bar(egui::pos2(*x, y));
            }
            let line = egui::Rect::from_x_y_ranges(x - 3.0..=x + 3.0, frame.top()..=frame.bottom());
            let hit = at.map_or(line, |a| {
                line.union(marks::inside_rect(frame, a, named_w(&named, label)))
            });
            self.hover(hit, label, *v);
        }
    }

    fn sounds_inside(&self, marks: &[LaneMark]) {
        for (x, label, v) in marks {
            if let Some(y) = self.trace_y(*v + 0.0) {
                self.sound_ring(egui::pos2(*x, y));
                self.hover(
                    egui::Rect::from_center_size(egui::pos2(*x, y), egui::vec2(10.0, 10.0)),
                    label,
                    *v,
                );
            }
        }
    }

    fn lane_outside(
        &self,
        marks: &[LaneMark],
        galleys: Vec<std::sync::Arc<egui::Galley>>,
        up: bool,
    ) {
        let (ui, frame) = (self.ui, self.frame);
        let id = lane_id(ui, self.side, up);
        let names: Vec<(f32, f32, Rank)> = marks
            .iter()
            .zip(&galleys)
            .map(|((x, _, _), g)| (*x, g.size().x, Rank::Strong))
            .collect();
        let placed = marks::place(
            &names,
            frame.left(),
            frame.right(),
            &marks::rows_were(ui, id),
        );
        marks::keep_rows(ui, id, &placed);
        // The names go on last, each on a box of the panel's colour, so no
        // other mark's line runs through one.
        let mut names = Vec::new();
        for (((x, label, v), g), (left, row)) in marks.iter().zip(galleys).zip(&placed) {
            let x = *x;
            let w = g.size().x;
            let row_off = LANE_GAP + *row as f32 * ROW_H;
            let (name_y, end_y) = if up {
                (frame.top() - row_off - ROW_H, frame.top() - row_off - 1.0)
            } else {
                (frame.bottom() + row_off, frame.bottom() + row_off + 1.0)
            };
            let name = egui::Rect::from_min_size(egui::pos2(*left, name_y), egui::vec2(w, ROW_H));
            let on = self.trace_y(*v + 0.0);
            let centre = name.center().x;
            if up {
                self.edge_line(label, x, centre, end_y, on);
            } else if let Some(y) = on {
                self.sound_leader(x, y, centre, end_y);
            }
            names.push((name, g));
            let hit = name.union(egui::Rect::from_x_y_ranges(
                x - 3.0..=x + 3.0,
                frame.top()..=frame.bottom(),
            ));
            self.hover(hit, label, *v);
        }
        for (name, g) in names {
            marks::paint_lane_name(ui, &self.painter, name, g);
        }
    }

    /// An edge's line down from its name at `centre` to the trace at `x`,
    /// and its bar there.
    fn edge_line(&self, label: &str, x: f32, centre: f32, end_y: f32, on: Option<f32>) {
        let frame = self.frame;
        // Short of the trace, and of a sound's ring at the same spot.
        let stop = on.map_or(frame.bottom(), |y| (y - self.r - 2.0).max(frame.top()));
        // Straight down from its name, or from where the name had to
        // slide to, with a short angled leader back to its line.
        let stroke = egui::Stroke::new(1.2_f32, self.edge_ink(label));
        let mut down = end_y;
        if (centre - x).abs() > 1.0 {
            down = frame.top() - LANE_GAP / 2.0;
            self.painter
                .line_segment([egui::pos2(centre, end_y), egui::pos2(x, down)], stroke);
        }
        // Stopped at the facts box's plate, and on below it.
        for (a, b) in around_plate(self.plate, x, down, stop) {
            self.painter
                .line_segment([egui::pos2(x, a), egui::pos2(x, b)], stroke);
        }
        if let Some(y) = on {
            self.edge_bar(egui::pos2(x, y));
        }
    }

    /// A sound's ring on the trace at (`x`, `y`) and its dotted leader down
    /// to its name at `centre`.
    fn sound_leader(&self, x: f32, y: f32, centre: f32, end_y: f32) {
        let pts = vec![
            egui::pos2(x, y + self.r),
            egui::pos2(x, self.frame.bottom() + LANE_GAP / 2.0),
            egui::pos2(centre, end_y),
        ];
        self.painter.extend(egui::Shape::dashed_line(
            &pts,
            egui::Stroke::new(1.0_f32, self.pal.sound),
            2.0,
            2.0,
        ));
        self.sound_ring(egui::pos2(x, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact_box::*;

    /// The level axis takes its decimals from its grid step: small levels
    /// on a tall plot read 0.002, 0.004, not 0.00 at every mark.
    #[test]
    fn level_marks_read_apart_at_any_step() {
        assert_eq!(level_label(0.004, 0.002), "0.004");
        assert_eq!(level_label(0.0, 0.002), "0.000");
        assert_eq!(level_label(0.25, 0.05), "0.25");
        assert_eq!(level_label(0.3, 0.1), "0.3");
        assert_eq!(level_label(2.0, 1.0), "2");
        assert_eq!(level_label(-0.0, 0.01), "0.00");
        let marks: Vec<String> = (0..6)
            .map(|i| level_label(i as f64 * 0.0005, 0.0005))
            .collect();
        let mut seen = marks.clone();
        seen.dedup();
        assert_eq!(seen, marks);
    }

    #[test]
    fn the_facts_are_a_row_each_with_their_units() {
        let p = TickProfile {
            side: timegrapher_core::profile::Side::A,
            beats: 40,
            t0_ms: -20.0,
            step_ms: 0.1,
            median: Vec::new(),
            p10: Vec::new(),
            p90: Vec::new(),
            floor: 0.0,
            unlock_ms: Some(-7.64),
            drop_ms: Some(-0.67),
            peak_ms: None,
            amplitude_deg: Some(286.4),
            sound1_ms: None,
            sound2_ms: None,
            sound3_ms: None,
        };
        // The box leaves the amplitude to the Amplitude card, which reads
        // the same moment with its ± (Norm, #1068 item 6).
        assert_eq!(
            facts(&p),
            ["40", "−7.64 ms", "−0.67 ms", "6.97 ms"].map(String::from)
        );
        assert!(!FACT_NAMES.contains(&"Amplitude"));
        assert_eq!(FACT_NAMES[3], "Unlock to Drop");
        let names = |k: Vec<usize>| k.into_iter().map(|i| FACT_NAMES[i]).collect::<Vec<_>>();
        // A short plot keeps Unlock to Drop.
        assert_eq!(names(keep_rows(80.0, 3)), ["Unlock to Drop"]);
        assert_eq!(keep_rows(150.0, 6), [0, 1, 2, 3]);
        // One or four: a tall plot without room for four keeps one.
        assert_eq!(keep_rows(110.0, 3), [3]);
        // Never taller than the plot: one row, then none.
        assert_eq!(names(keep_rows(40.0, 1)), ["Unlock to Drop"]);
        assert_eq!(keep_rows(30.0, 0), Vec::<usize>::new());
        // The noise floor sheds nothing: the box is the same in dB.
        assert_eq!(keep_rows(150.0, 6).len(), 4);
    }

    /// The facts box sits in the plot's top-left corner, as tall as its
    /// rows and as wide as both columns, so it is the same on both plots.
    #[test]
    fn the_facts_box_sits_in_the_plots_corner() {
        let b = FactBox {
            base: egui_plot::PlotItemBase::new("Tick facts".into()),
            rows: vec![(String::new(), String::new()); 4],
            plot_h: 150.0,
            tops: Vec::new(),
            held: egui::Id::new("held"),
            was: None,
            name_w: 90.0,
            value_w: 60.0,
            short_name_w: 80.0,
            short_value_w: 40.0,
            fill: Color32::WHITE,
            name_color: Color32::GRAY,
            value_color: Color32::BLACK,
            keep: Some(keep_rows),
            shed: Vec::new(),
            flow: false,
        };
        let frame = egui::Rect::from_min_size(egui::pos2(50.0, 30.0), egui::vec2(800.0, 150.0));
        let r = b.rect(frame, 4);
        assert_eq!(r.min, egui::pos2(54.0, 34.0));
        assert_eq!(r.width(), 90.0 + FACT_GAP + 60.0 + 2.0 * FACT_PAD);
        assert_eq!(r.height(), 4.0 * FACT_ROW + 2.0 * FACT_PAD);
        // A short plot's one row: as narrow as it is, tight around it.
        let r = b.rect(frame, 1);
        assert_eq!(r.width(), 80.0 + FACT_GAP + 40.0 + 2.0 * FACT_PAD_SHORT);
        assert_eq!(r.height(), FACT_ROW_SHORT + 2.0 * FACT_PAD_SHORT);
        // A plot 44 pt tall holds the row; never none.
        let short = FactBox {
            plot_h: 44.0,
            ..b.clone()
        };
        let frame = egui::Rect::from_min_size(egui::pos2(50.0, 30.0), egui::vec2(300.0, 44.0));
        let t = egui_plot::PlotTransform::new(
            frame,
            egui_plot::PlotBounds::from_min_max([0.0, 0.0], [1.0, 1.0]),
            egui::Vec2b::new(false, false),
        );
        assert_eq!(short.shown(&t), vec![3]);
        // Over a trace that rises under the left corner and is low on the
        // right, the box takes the top right corner.
        let rising = FactBox {
            tops: (0..=10)
                .map(|i| [i as f64 / 10.0, if i < 5 { 0.95 } else { 0.05 }])
                .collect(),
            ..short.clone()
        };
        let (rows, r) = rising.layout(&t);
        assert_eq!(rows, vec![3]);
        assert!(
            (r.right() - (frame.right() - FACT_INSET)).abs() < 1e-3,
            "{r:?}"
        );
        let (lo, hi) = rising.span(&t);
        assert!(
            lo > frame.left() + 100.0 && hi <= frame.right(),
            "{lo} {hi}"
        );
        // Under a trace high on both sides, it goes to a bottom corner the
        // trace leaves clear, its row kept (Hugh, #1626).
        let high = FactBox {
            tops: (0..=10).map(|i| [i as f64 / 10.0, 0.95]).collect(),
            ..short.clone()
        };
        let (rows, r) = high.layout(&t);
        assert_eq!(rows, vec![3]);
        assert!(
            (r.bottom() - (frame.bottom() - FACT_INSET)).abs() < 1e-3,
            "{r:?}"
        );
        // A trace across the whole plot, high and at mid height, covers
        // every corner: the box sits top left anyway, never dropped.
        let everywhere = FactBox {
            tops: (0..=40)
                .map(|i| [i as f64 / 40.0, if i % 2 == 0 { 0.95 } else { 0.3 }])
                .collect(),
            ..short.clone()
        };
        let (rows, r) = everywhere.layout(&t);
        assert_eq!(rows, vec![3]);
        assert_eq!(r.min, frame.min + egui::vec2(FACT_INSET, FACT_INSET));
        assert_eq!(everywhere.span(&t), (frame.left(), r.right()));
        // A tall box over a trace high under the left corner, and on the
        // right low enough for one row but not two: it sheds rows,
        // keeping Unlock to Drop, in the right corner.
        let peak = FactBox {
            tops: (0..=10)
                .map(|i| [i as f64 / 10.0, if i < 5 { 0.95 } else { 0.78 }])
                .collect(),
            ..b.clone()
        };
        let tall = egui::Rect::from_min_size(egui::pos2(50.0, 30.0), egui::vec2(800.0, 150.0));
        let tt = egui_plot::PlotTransform::new(
            tall,
            egui_plot::PlotBounds::from_min_max([0.0, 0.0], [1.0, 1.0]),
            egui::Vec2b::new(false, false),
        );
        assert_eq!(peak.shown(&tt), vec![0, 1, 2, 3]);
        let (rows, r) = peak.layout(&tt);
        assert_eq!(rows, vec![3]);
        assert!(
            (r.right() - (tall.right() - FACT_INSET)).abs() < 1e-3,
            "{r:?}"
        );
        // It adds nothing to the plot's bounds.
        assert!(!egui_plot::PlotItem::bounds(&b).is_valid());
    }

    /// An edge's line stops at the facts box's plate and goes on below
    /// it, so it never touches the box's text; a line beside the plate,
    /// or none, runs whole (Hugh, #1633).
    #[test]
    fn a_line_stops_at_the_facts_plate() {
        let plate = egui::Rect::from_min_max(egui::pos2(54.0, 34.0), egui::pos2(220.0, 110.0));
        assert_eq!(around_plate(None, 215.0, 0.0, 200.0), vec![(0.0, 200.0)]);
        assert_eq!(
            around_plate(Some(plate), 215.0, 0.0, 200.0),
            vec![(0.0, 34.0), (110.0, 200.0)]
        );
        // Stopping short of the trace inside the plate: only above it.
        assert_eq!(
            around_plate(Some(plate), 215.0, 0.0, 80.0),
            vec![(0.0, 34.0)]
        );
        // From inside the plate down: only below it.
        assert_eq!(
            around_plate(Some(plate), 215.0, 50.0, 200.0),
            vec![(110.0, 200.0)]
        );
        assert_eq!(
            around_plate(Some(plate), 221.0, 0.0, 200.0),
            vec![(0.0, 200.0)]
        );
    }

    /// A trace on the edge of the box's rows doesn't make it flick from
    /// frame to frame: another layout than the one last drawn must clear
    /// the trace by `FACT_HOLD` more.
    #[test]
    fn a_facts_box_holds_its_layout_over_a_trace_on_the_edge() {
        let frame = egui::Rect::from_min_size(egui::pos2(50.0, 30.0), egui::vec2(300.0, 44.0));
        let t = egui_plot::PlotTransform::new(
            frame,
            egui_plot::PlotBounds::from_min_max([0.0, 0.0], [1.0, 1.0]),
            egui::Vec2b::new(false, false),
        );
        // The row ends 54 pt down; the trace runs at 59, clear of it by
        // 5 pt: more than the 2 pt any layout needs, less than another
        // layout's 2 pt and `FACT_HOLD`.
        let y = (frame.bottom() - 59.0) as f64 / 44.0;
        let b = FactBox {
            base: egui_plot::PlotItemBase::new("Tick facts".into()),
            rows: vec![(String::new(), String::new()); 4],
            plot_h: 44.0,
            tops: (0..=10).map(|i| [i as f64 / 10.0, y]).collect(),
            held: egui::Id::new("held"),
            was: None,
            name_w: 90.0,
            value_w: 60.0,
            short_name_w: 80.0,
            short_value_w: 40.0,
            fill: Color32::WHITE,
            name_color: Color32::GRAY,
            value_color: Color32::BLACK,
            keep: Some(keep_rows),
            shed: Vec::new(),
            flow: false,
        };
        assert_eq!(b.rect(frame, 1).bottom(), 54.0);
        // First drawn, or drawn in the left corner last frame: the left.
        let left = |b: &FactBox| b.layout(&t).1.left() < frame.center().x;
        assert_eq!(b.layout(&t).0, vec![3]);
        assert!(left(&b));
        let was_left = FactBox {
            was: Some((1, Corner::TopLeft)),
            ..b.clone()
        };
        assert!(left(&was_left));
        // Drawn in the right corner last frame, it stays there until the
        // trace clears the left by the hold as well, or lies at the
        // plot's foot.
        let was_right = FactBox {
            was: Some((1, Corner::TopRight)),
            ..b.clone()
        };
        assert_eq!(was_right.layout(&t).0, vec![3]);
        assert!(!left(&was_right));
        let clear = FactBox {
            tops: (0..=10).map(|i| [i as f64 / 10.0, 0.0]).collect(),
            ..was_right.clone()
        };
        assert!(left(&clear));
    }

    #[test]
    fn decibels_below_the_top() {
        let v = scaled(&[1.0, 0.1, 0.0], 1.0, Scale::Decibels);
        assert!((v[0] - 0.0).abs() < 1e-9);
        assert!((v[1] + 20.0).abs() < 1e-6);
        assert_eq!(v[2], FLOOR_DB);
        assert_eq!(scaled(&[0.5], 1.0, Scale::Linear), vec![0.5]);
    }

    #[test]
    fn the_shared_clock_puts_the_drops_a_beat_error_apart() {
        let opt = Options::default();
        let [tick, tock] = offsets(opt, Some(-0.97));
        assert!((tick - tock + 0.97).abs() < 1e-12);
        assert_eq!(offsets(opt, None), [0.0, 0.0]);
        let own = Options {
            shared_clock: false,
            ..opt
        };
        assert_eq!(offsets(own, Some(-0.97)), [0.0, 0.0]);
    }
}
