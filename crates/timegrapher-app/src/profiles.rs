//! The tick and tock sound pane: the typical sound of the ticks and of the
//! tocks, with the band most beats fall in and the marks the engine read
//! the amplitude and beat error from.

use crate::strip::side_colors;
use crate::theme;
use eframe::egui::{self, Color32};
use egui_plot::{HLine, Line, LineStyle, Plot, Polygon};
use timegrapher_core::profile::TickProfile;

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

/// The facts on each plot, a row each, top to bottom.
const FACT_NAMES: [&str; 5] = ["Beats", "Unlock", "Drop", "Unlock to Drop", "Amplitude"];

/// Below this height a plot's facts keep only their last two rows, Unlock
/// to Drop and Amplitude; the rest are in the hover.
const SHORT_PLOT: f32 = 100.0;
/// The least height a side takes stacked: its name and a plot of 40 pt.
const STACKED_MIN: f32 = 64.0;

/// A side's facts before it has beats.
const NO_FACTS: &str = "Not enough beats yet";

/// One side's facts with their units, in [`FACT_NAMES`]' order.
pub fn facts(p: &TickProfile) -> [String; 5] {
    let ms = |v: Option<f64>| {
        v.map_or("–".to_string(), |v| {
            format!("{v:+.2} ms").replace('-', "−")
        })
    };
    [
        p.beats.to_string(),
        ms(p.unlock_ms),
        ms(p.drop_ms),
        match (p.unlock_ms, p.drop_ms) {
            (Some(u), Some(d)) => format!("{:.2} ms", d - u),
            _ => "–".to_string(),
        },
        p.amplitude_deg
            .map_or("out of range".to_string(), |a| format!("{a:.0}°")),
    ]
}

/// Which of a side's five facts its box shows, top to bottom: on a plot
/// `plot_h` tall with room for `fit_frame` rows inside it, all five, else
/// (as on a short plot) two; and never more than `fit_floor`, the rows
/// that fit above the noise floor, shedding Drop, then Unlock, then
/// Beats, then Unlock to Drop, so Amplitude goes last. The hover has all
/// five.
fn keep_rows(plot_h: f32, fit_frame: usize, fit_floor: usize) -> Vec<usize> {
    let by_height = if plot_h >= SHORT_PLOT && fit_frame >= 5 {
        5
    } else {
        fit_frame.min(2)
    };
    let mut keep = [4, 3, 0, 1, 2][..by_height.min(fit_floor)].to_vec();
    keep.sort_unstable();
    keep
}

/// A side's facts in the plot's top-left corner, as a plot item so they
/// sit over the grid and under the band and the trace: a spike that runs
/// into them still shows. The same box at the same place on both plots,
/// so Tick and Tock compare row against row.
#[derive(Clone)]
struct FactBox {
    base: egui_plot::PlotItemBase,
    /// The five facts, or the one line saying there are none yet.
    rows: Vec<(String, String)>,
    plot_h: f32,
    /// The noise floor's level: the box stays above it.
    floor: Option<f64>,
    /// The values' column, right-aligned, as wide as the widest value on
    /// either side; and the names' column likewise.
    name_w: f32,
    value_w: f32,
    /// The same for the two rows a short plot keeps.
    short_name_w: f32,
    short_value_w: f32,
    fill: Color32,
    name_color: Color32,
    value_color: Color32,
}

/// The box's distance from the plot's corner, points.
const FACT_INSET: f32 = 4.0;
/// Room inside the box around its rows, and each row's height, points.
const FACT_PAD: f32 = 8.0;
const FACT_ROW: f32 = 16.0;
/// The room around the rows in a short plot's box, and each row's height.
const FACT_PAD_SHORT: f32 = 3.0;
const FACT_ROW_SHORT: f32 = 14.0;
/// Between the names' column and the values'.
const FACT_GAP: f32 = 16.0;
/// The facts' type size.
const FACT_SIZE: f32 = 12.0;

impl FactBox {
    /// Where the box goes in a plot whose frame is `frame`, `rows` tall.
    /// Fewer than five rows of facts make a short box, as narrow as its
    /// rows and tight around them.
    fn rect(&self, frame: egui::Rect, rows: usize) -> egui::Rect {
        let short = rows < 5 && self.rows.len() == 5;
        let (name_w, value_w) = if short {
            (self.short_name_w, self.short_value_w)
        } else {
            (self.name_w, self.value_w)
        };
        let pad = Self::pad(rows, self.rows.len());
        egui::Rect::from_min_size(
            frame.min + egui::vec2(FACT_INSET, FACT_INSET),
            egui::vec2(
                name_w + FACT_GAP + value_w + 2.0 * pad,
                rows as f32 * Self::row_h(rows, self.rows.len()) + 2.0 * pad,
            ),
        )
    }

    /// The room around `rows` of a box holding `of` facts.
    fn pad(rows: usize, of: usize) -> f32 {
        if rows < 5 && of == 5 {
            FACT_PAD_SHORT
        } else {
            FACT_PAD
        }
    }

    /// Each row's height in a box of `rows` holding `of` facts.
    fn row_h(rows: usize, of: usize) -> f32 {
        if rows < 5 && of == 5 {
            FACT_ROW_SHORT
        } else {
            FACT_ROW
        }
    }

    /// The rows shown in the plot as `t` draws it: inside the plot and
    /// above its noise floor by 6 pt.
    fn shown(&self, t: &egui_plot::PlotTransform) -> Vec<usize> {
        let frame = *t.frame();
        let top = frame.top() + FACT_INSET;
        let room = |bottom: f32, pad: f32, row: f32| {
            ((bottom - top - 2.0 * pad) / row).floor().max(0.0) as usize
        };
        // A short plot's box is tight around its rows.
        let full =
            self.plot_h >= SHORT_PLOT && room(frame.bottom() - FACT_INSET, FACT_PAD, FACT_ROW) >= 5;
        let (pad, row) = if full || self.rows.len() != 5 {
            (FACT_PAD, FACT_ROW)
        } else {
            (FACT_PAD_SHORT, FACT_ROW_SHORT)
        };
        let fit_frame = room(frame.bottom() - FACT_INSET, pad, row);
        // Above the noise floor where there is room; never no facts at all
        // where the plot holds two rows.
        let fit_floor = self
            .floor
            .map_or(fit_frame, |y| {
                room(t.position_from_point_y(y) - 6.0, pad, row)
            })
            .max(fit_frame.min(2))
            .min(fit_frame);
        match self.rows.len() {
            5 => keep_rows(self.plot_h, fit_frame, fit_floor),
            1 if fit_floor >= 1 => vec![0],
            _ => Vec::new(),
        }
    }

    /// Where the box ends, from the plot's left edge, as `t` draws it:
    /// nothing names itself over it.
    fn right(&self, t: &egui_plot::PlotTransform) -> f32 {
        let shown = self.shown(t).len();
        if shown == 0 {
            0.0
        } else {
            FACT_INSET + self.rect(egui::Rect::ZERO, shown).width()
        }
    }
}

impl egui_plot::PlotItem for FactBox {
    fn shapes(
        &self,
        ui: &egui::Ui,
        transform: &egui_plot::PlotTransform,
        shapes: &mut Vec<egui::Shape>,
    ) {
        let shown = self.shown(transform);
        if shown.is_empty() {
            return;
        }
        let r = self.rect(*transform.frame(), shown.len());
        let pad = Self::pad(shown.len(), self.rows.len());
        let row = Self::row_h(shown.len(), self.rows.len());
        shapes.push(egui::Shape::rect_filled(r, 6.0, self.fill));
        let font = egui::FontId::proportional(FACT_SIZE);
        for (i, (name, value)) in shown.iter().map(|&k| &self.rows[k]).enumerate() {
            let y = r.top() + pad + (i as f32 + 0.5) * row;
            ui.fonts_mut(|f| {
                let g = f.layout_no_wrap(name.clone(), font.clone(), self.name_color);
                let at = egui::pos2(r.left() + pad, y - g.size().y / 2.0);
                shapes.push(egui::Shape::galley(at, g, self.name_color));
                let g = f.layout_no_wrap(value.clone(), font.clone(), self.value_color);
                let at = egui::pos2(r.right() - pad - g.size().x, y - g.size().y / 2.0);
                shapes.push(egui::Shape::galley(at, g, self.value_color));
            });
        }
    }

    fn initialize(&mut self, _x_range: std::ops::RangeInclusive<f64>) {}

    fn color(&self) -> Color32 {
        self.value_color
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

/// The widest of `texts` in the facts' type, points.
fn widest<'a>(ui: &egui::Ui, texts: impl Iterator<Item = &'a str>) -> f32 {
    texts
        .map(|t| {
            ui.fonts_mut(|f| {
                f.layout_no_wrap(
                    t.to_owned(),
                    egui::FontId::proportional(FACT_SIZE),
                    Color32::WHITE,
                )
                .size()
                .x
            })
        })
        .fold(0.0, f32::max)
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
    // Each side's facts, and the columns' widths over both sides, so the
    // two boxes match.
    let facts: Vec<Option<[String; 5]>> = profiles.iter().map(|p| p.as_ref().map(facts)).collect();
    let name_w = widest(ui, FACT_NAMES.iter().copied());
    let value_w = widest(
        ui,
        facts
            .iter()
            .flatten()
            .flat_map(|f| f.iter().map(String::as_str)),
    );
    // The two rows a short plot keeps, Unlock to Drop and Amplitude.
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
    let fact_box = |k: usize, plot_h: f32| {
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
            plot_h,
            floor: profiles[k]
                .as_ref()
                .map(|p| scaled(&[p.floor], top, scale)[0]),
            name_w: if facts[k].is_some() { name_w } else { none_w },
            value_w: if facts[k].is_some() { value_w } else { 0.0 },
            short_name_w,
            short_value_w,
            rows,
            fill: box_fill,
            name_color: pal.text_secondary,
            value_color: pal.text,
        }
    };
    // `axis`: the time axis under this side; stacked, Tock's serves both,
    // as their time runs together.
    let side = |ui: &mut egui::Ui, k: usize, axis: bool| {
        // The lanes keep their own gaps: no spacing between them and the plot.
        ui.spacing_mut().item_spacing.y = 0.0;
        let p = &profiles[k];
        let dx = shift[k];
        let name = ["Tick", "Tock"][k];
        let c = colors[k];
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
        let short_id = ui.id().with(("profile short", k));
        let was_short = ui.data(|d| d.get_temp::<bool>(short_id)).unwrap_or(false);
        let short = outside_h
            < if was_short {
                SHORT_PLOT + HYSTERESIS
            } else {
                SHORT_PLOT - HYSTERESIS
            };
        ui.data_mut(|d| d.insert_temp(short_id, short));
        let (top_lane, bottom_lane) = if short {
            (ROW_H + 2.0, axis_h + 4.0)
        } else {
            (top_lane, bottom_lane)
        };
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
        let h = (ui.available_height() - 20.0).max(20.0) as f64;
        let plot_h = (ui.available_height() - bottom_lane).max(24.0);
        let facts_here = fact_box(k, plot_h);
        let empty = p.is_none();
        let plot = Plot::new(("profile", k))
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
            .x_grid_spacer(|g| theme::even_grid(g, 80.0, &[]))
            .y_grid_spacer(|g| theme::even_grid(g, 30.0, &[]))
            .y_axis_min_width(52.0)
            .y_axis_formatter(move |m, r| {
                // Before there are beats there is no level to mark.
                if empty || (m.value - r.start()) / (r.end() - r.start()) * h < 9.0 {
                    return String::new();
                }
                match scale {
                    Scale::Decibels => format!("{:.0} dB", m.value + 0.0),
                    Scale::Linear => level_label(m.value, m.step_size),
                }
            });
        // Before there are beats, a typical stretch of time rather than none.
        let plot = if p.is_none() {
            plot.default_x_bounds(-20.0, 10.0)
        } else {
            plot
        };
        let mut med = Vec::new();
        // A short plot's edge names, drawn in it.
        let mut edge_names = p.as_ref().filter(|_| short && opt.edges).map(|p| {
            let level = scaled(&p.median, top, scale);
            edge_marks(p)
                .into_iter()
                .map(|(l, v)| {
                    let i = ((v - p.t0_ms) / p.step_ms).round();
                    let y = (i >= 0.0).then(|| level.get(i as usize).copied()).flatten();
                    (l, v + dx, name_width(ui, l), y)
                })
                .collect::<Vec<_>>()
        });
        let resp = plot.show(ui, |pl| {
            pl.set_plot_bounds_y(y_lo..=y_hi);
            // Over the grid, under the band and the trace.
            pl.add(facts_here.clone());
            let Some(p) = p else { return };
            if let Some(names) = edge_names.take() {
                pl.add(EdgeNames {
                    base: egui_plot::PlotItemBase::new(format!("{name} edge names")),
                    names,
                    facts: facts_here.clone(),
                    fill: box_fill,
                    ink: pal.text,
                    lines: [pal.unlock, pal.drop, pal.peak],
                });
            }
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
                        "middle 80% of beats",
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
            let floor = scaled(&[p.floor], top, scale)[0];
            pl.hline(
                HLine::new("noise floor", floor)
                    .color(pal.text_tertiary)
                    .style(LineStyle::dashed_dense()),
            );
            med = m;
        });
        // A box that keeps fewer rows: the hover has all five.
        let shown = facts_here.shown(&resp.transform);
        if let (true, Some(f)) = (shown.len() < FACT_NAMES.len(), &facts[k]) {
            // Where no row fits, the corner a two-row box would take.
            let r = facts_here.rect(*resp.transform.frame(), shown.len().max(2));
            if resp.response.hover_pos().is_some_and(|at| r.contains(at)) {
                let all: Vec<String> = FACT_NAMES
                    .iter()
                    .zip(f)
                    .map(|(n, v)| format!("{n}: {v}"))
                    .collect();
                resp.response.clone().on_hover_text(all.join("\n"));
            }
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
            let inside = short.then(|| facts_here.right(&resp.transform));
            label_marks(ui, &resp.transform, p, &med, opt, dx, k, inside);
        }
    };
    // Stacked, each side's share; too short for two plots one above the
    // other, they go side by side, each the pane's height.
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

/// The rectangle of an edge's name `w` wide at `left`, inside a short
/// plot along its top.
fn inside_name_rect(frame: egui::Rect, left: f32, w: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(left - 3.0, frame.top() + FACT_INSET),
        egui::vec2(w + 6.0, ROW_H),
    )
}

/// Where the edges' names go inside a short plot, one row along its top:
/// `names` are each one's label, line x and width, left to right. Each is
/// centred on its line within the plot; one that would touch the facts
/// box (ending at `box_right`) starts at its line instead, right of it,
/// or isn't named if its line is under the box; one that would come within a word space of the name before it isn't
/// named (its line's hover names it). Each name's left edge, or none.
pub fn inside_places(
    names: &[(&str, f32, f32)],
    left: f32,
    right: f32,
    box_right: f32,
) -> Vec<Option<f32>> {
    let mut end = f32::NEG_INFINITY;
    names
        .iter()
        .map(|&(_, x, w)| {
            let mut at = (x - w / 2.0).clamp(left + 4.0, (right - 4.0 - w).max(left + 4.0));
            if at - 3.0 < box_right + 4.0 {
                at = x + 4.0;
                // Its line under the facts box: unnamed rather than over
                // the facts (its line's hover names it).
                if at - 3.0 < box_right + 4.0 {
                    return None;
                }
            }
            if at - 3.0 < end + NAME_GAP {
                return None;
            }
            end = at + w + 3.0;
            Some(at)
        })
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
        let at = inside_places(
            &named,
            frame.left(),
            frame.right(),
            frame.left() + self.facts.right(transform),
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
                    let r = inside_name_rect(frame, a, *w);
                    names.push((r, a, *label));
                    r.bottom()
                }
                None => frame.top(),
            };
            if stop > from {
                shapes.push(egui::Shape::line_segment(
                    [egui::pos2(*x, from), egui::pos2(*x, stop)],
                    egui::Stroke::new(1.2_f32, ink),
                ));
            }
        }
        for (r, a, label) in names {
            shapes.push(egui::Shape::rect_filled(r, 3.0, self.fill));
            let g = ui.fonts_mut(|f| {
                f.layout_no_wrap(label.to_string(), theme::semibold(11.5), self.ink)
            });
            let p = egui::pos2(a, r.center().y - g.size().y / 2.0);
            shapes.push(egui::Shape::galley(p, g, self.ink));
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

/// One row of names in a lane, points tall.
const ROW_H: f32 = 15.0;
/// A lane of names: two rows.
const LANE: f32 = 2.0 * ROW_H;
/// The short gap between the plot and its nearest row of names, so each
/// line or leader shows outside the plot before it reaches its name.
const LANE_GAP: f32 = 6.0;
/// The time axis's labels under the sounds' lane.
const AXIS_H: f32 = 16.0;
/// The room kept between two names in a row: a word space and more, so
/// two names side by side never read as one phrase.
const NAME_GAP: f32 = 12.0;
/// How far an overlap must start or end before a name changes row, so
/// names don't hop between frames as the averaged sound drifts.
const HYSTERESIS: f32 = 2.0;

/// How many rows of names a side's lane took last frame (`up` for the
/// edges' above the plot): one, or two where names crowded.
fn lane_rows(ui: &egui::Ui, side: usize, up: bool) -> u8 {
    let rows: Vec<u8> = ui
        .data(|d| d.get_temp(lane_id(ui, side, up)))
        .unwrap_or_default();
    1 + rows.into_iter().max().unwrap_or(0).min(1)
}

/// Where a lane keeps the rows its names took, for the next frame.
fn lane_id(ui: &egui::Ui, side: usize, up: bool) -> egui::Id {
    ui.id().with(("profile lane", side, up))
}

/// Where each name in a lane goes: `names` are each one's centre (its
/// line's or ring's x) and width, left to right; `was` is the row each was
/// in last frame. Each name is centred on its mark, kept within
/// `left..right`, and goes one row further out where it would touch the
/// one before it; where the second row is taken too, it slides right of
/// the name there (drawn with an angled leader). Returns each name's left
/// edge and row.
pub fn place_names(names: &[(f32, f32)], left: f32, right: f32, was: &[u8]) -> Vec<(f32, u8)> {
    let mut ends = [f32::NEG_INFINITY; 2];
    let mut out = Vec::with_capacity(names.len());
    for (i, &(x, w)) in names.iter().enumerate() {
        let at = (x - w / 2.0).clamp(left, (right - w).max(left));
        let before = was.get(i).copied().unwrap_or(0);
        // The overlap with the name before in the first row, the gap
        // between names included: a name goes out once it overlaps by
        // the hysteresis, and comes back once it clears by as much.
        let overlap = ends[0] + NAME_GAP - at;
        let out_row = if before == 1 {
            overlap > -HYSTERESIS
        } else {
            overlap > HYSTERESIS
        };
        let (at, row) = if !out_row {
            (at, 0)
        } else {
            (at.max(ends[1] + NAME_GAP).min((right - w).max(left)), 1)
        };
        ends[row as usize] = at + w;
        out.push((at, row));
    }
    out
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
    let p = 10f64.powf(want.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * p)
        .find(|&s| s >= want * (1.0 - 1e-9))
        .unwrap_or(10.0 * p);
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
                format!("{} ms", v + 0.0).replace('-', "−"),
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
    med: &[f64],
    opt: Options,
    dx: f64,
    side: usize,
    inside: Option<f32>,
) {
    let frame = *t.frame();
    let pal = theme::pal(ui);
    let painter = ui.painter_at(frame.expand2(egui::vec2(0.0, LANE + LANE_GAP + 2.0)));
    let edge_ink = |label: &str| match label {
        "Unlock" => pal.unlock,
        "Drop" => pal.drop,
        _ => pal.peak,
    };
    // The trace's height at a time, on the screen.
    let trace_y = |v: f64| -> Option<f32> {
        let i = ((v - p.t0_ms) / p.step_ms).round();
        (i >= 0.0)
            .then(|| med.get(i as usize).copied())
            .flatten()
            .map(|y| t.position_from_point_y(y))
    };
    // Where the trace is marked: an edge's bar, a sound's ring, both drawn
    // over the trace, smaller on a short plot so they don't fill it, and
    // left off where they would cross an edge's name drawn in the plot.
    let r = (frame.height() / 8.0).clamp(2.5, 5.0);
    let names_in = std::cell::RefCell::new(Vec::<egui::Rect>::new());
    let clear = |at: egui::Pos2| {
        let mark = egui::Rect::from_center_size(at, egui::vec2(2.0 * r + 2.0, 2.0 * r + 2.0));
        !names_in.borrow().iter().any(|n| n.intersects(mark))
    };
    let edge_bar = |at: egui::Pos2| {
        if clear(at) {
            painter.line_segment(
                [at - egui::vec2(r - 1.0, 0.0), at + egui::vec2(r - 1.0, 0.0)],
                egui::Stroke::new(2.0_f32, pal.text),
            );
        }
    };
    let sound_ring = |at: egui::Pos2| {
        if clear(at) {
            painter.circle_stroke(at, r, egui::Stroke::new(1.5_f32, pal.text_secondary));
        }
    };
    // A lane laid out and drawn: `marks` are each name, its time and its
    // hover's words; `up` for the edges above the plot.
    let lane = |marks: Vec<(&str, f64)>, up: bool| {
        let font = if up {
            theme::semibold(11.5)
        } else {
            egui::FontId::proportional(11.5)
        };
        let ink = if up { pal.text } else { pal.text_secondary };
        let mut marks: Vec<(f32, &str, f64)> = marks
            .into_iter()
            .map(|(l, v)| (t.position_from_point_x(v + dx), l, v))
            .filter(|(x, _, _)| *x >= frame.left() && *x <= frame.right())
            .collect();
        marks.sort_by(|a, b| a.0.total_cmp(&b.0));
        let galleys: Vec<_> = marks
            .iter()
            .map(|(_, l, _)| {
                let mut job = egui::text::LayoutJob::single_section(
                    l.to_string(),
                    egui::TextFormat {
                        font_id: font.clone(),
                        color: ink,
                        italics: !up,
                        ..Default::default()
                    },
                );
                job.wrap.max_rows = 1;
                ui.fonts_mut(|f| f.layout_job(job))
            })
            .collect();
        // A short plot: the edges' names are drawn in it, under the trace
        // (see `EdgeNames`), and the sounds' are their rings' hovers.
        if let Some(box_right) = inside {
            let hover = |hit: egui::Rect, label: &str, v: f64| {
                ui.interact(
                    hit,
                    ui.id().with(("mark", side, label)),
                    egui::Sense::hover(),
                )
                .on_hover_text(format!(
                    "{label}: {}",
                    format!("{v:+.2} ms").replace('-', "−")
                ));
            };
            if up {
                let named: Vec<(&str, f32, f32)> = marks
                    .iter()
                    .zip(&galleys)
                    .map(|((x, l, _), g)| (*l, *x, g.size().x))
                    .collect();
                let at = inside_places(
                    &named,
                    frame.left(),
                    frame.right(),
                    frame.left() + box_right,
                );
                names_in
                    .borrow_mut()
                    .extend(named.iter().zip(&at).filter_map(|((l, _, _), a)| {
                        a.map(|a| inside_name_rect(frame, a, named_w(&named, l)))
                    }));
                for ((x, label, v), at) in marks.iter().zip(at) {
                    if let Some(y) = trace_y(*v + 0.0) {
                        edge_bar(egui::pos2(*x, y));
                    }
                    let line = egui::Rect::from_x_y_ranges(
                        x - 3.0..=x + 3.0,
                        frame.top()..=frame.bottom(),
                    );
                    let hit = at.map_or(line, |a| {
                        line.union(inside_name_rect(frame, a, named_w(&named, label)))
                    });
                    hover(hit, label, *v);
                }
            } else {
                for (x, label, v) in &marks {
                    if let Some(y) = trace_y(*v + 0.0) {
                        sound_ring(egui::pos2(*x, y));
                        hover(
                            egui::Rect::from_center_size(egui::pos2(*x, y), egui::vec2(10.0, 10.0)),
                            label,
                            *v,
                        );
                    }
                }
            }
            return;
        }
        let id = lane_id(ui, side, up);
        let was: Vec<u8> = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        let names: Vec<(f32, f32)> = marks
            .iter()
            .zip(&galleys)
            .map(|((x, _, _), g)| (*x, g.size().x))
            .collect();
        let placed = place_names(&names, frame.left(), frame.right(), &was);
        ui.data_mut(|d| d.insert_temp(id, placed.iter().map(|p| p.1).collect::<Vec<u8>>()));
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
            let on = trace_y(*v + 0.0);
            let centre = name.center().x;
            if up {
                // Short of the trace, and of a sound's ring at the same spot.
                let stop = on.map_or(frame.bottom(), |y| (y - r - 2.0).max(frame.top()));
                // Straight down from its name, or from where the name had to
                // slide to, with a short angled leader back to its line.
                let mut pts = vec![egui::pos2(centre, end_y)];
                if (centre - x).abs() > 1.0 {
                    pts.push(egui::pos2(x, frame.top() - LANE_GAP / 2.0));
                }
                pts.push(egui::pos2(x, stop));
                painter.add(egui::Shape::line(
                    pts,
                    egui::Stroke::new(1.2_f32, edge_ink(label)),
                ));
                if let Some(y) = on {
                    edge_bar(egui::pos2(x, y));
                }
            } else if let Some(y) = on {
                let mut pts = vec![
                    egui::pos2(x, y + r),
                    egui::pos2(x, frame.bottom() + LANE_GAP / 2.0),
                ];
                pts.push(egui::pos2(centre, end_y));
                painter.extend(egui::Shape::dashed_line(
                    &pts,
                    egui::Stroke::new(1.0_f32, pal.sound),
                    2.0,
                    2.0,
                ));
                sound_ring(egui::pos2(x, y));
            }
            names.push((name, g));
            let hit = name.union(egui::Rect::from_x_y_ranges(
                x - 3.0..=x + 3.0,
                frame.top()..=frame.bottom(),
            ));
            ui.interact(
                hit,
                ui.id().with(("mark", side, *label)),
                egui::Sense::hover(),
            )
            .on_hover_text(format!(
                "{label}: {}",
                format!("{v:+.2} ms").replace('-', "−")
            ));
        }
        for (name, g) in names {
            painter.rect_filled(name.expand(2.0), 2.0, ui.visuals().panel_fill);
            painter.galley(name.min, g, ink);
        }
    };
    if opt.edges {
        lane(edge_marks(p), true);
    }
    if opt.sounds {
        lane(
            [
                ("Unlock Sound", p.sound1_ms),
                ("Impulse Sound", p.sound2_ms),
                ("Drop Sound", p.sound3_ms),
            ]
            .into_iter()
            .filter_map(|(l, v)| v.map(|v| (l, v)))
            .collect(),
            false,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Names in a lane: centred on their marks, out a row where they would
    /// touch, slid along where the second row is taken, inside the plot,
    /// and kept in their row while an overlap is within the hysteresis.
    #[test]
    fn names_that_crowd_move_out_a_row() {
        // Apart: all in the first row, centred.
        let p = place_names(&[(100.0, 40.0), (300.0, 40.0)], 0.0, 500.0, &[]);
        assert_eq!(p, vec![(80.0, 0), (280.0, 0)]);
        // Peak would touch Drop: out a row, still centred on its line.
        let p = place_names(&[(100.0, 40.0), (130.0, 40.0)], 0.0, 500.0, &[]);
        assert_eq!(p, vec![(80.0, 0), (110.0, 1)]);
        // Three together: the third slides right of the second's name.
        let p = place_names(
            &[(100.0, 40.0), (110.0, 40.0), (120.0, 40.0)],
            0.0,
            500.0,
            &[],
        );
        assert_eq!(p[2].1, 1);
        assert!(p[2].0 >= p[1].0 + 40.0 + NAME_GAP - 1e-3, "{p:?}");
        // Kept inside the plot's width.
        let p = place_names(&[(5.0, 40.0), (495.0, 40.0)], 0.0, 500.0, &[]);
        assert_eq!(p, vec![(0.0, 0), (460.0, 0)]);
        // A word space apart is too close: two names side by side would
        // read as one phrase.
        let p = place_names(&[(100.0, 40.0), (147.0, 40.0)], 0.0, 500.0, &[]);
        assert_eq!(p[1].1, 1, "{p:?}");
        // Within the hysteresis a name stays in the row it was in: here
        // the gap is 1 pt more than NAME_GAP.
        let near = [(100.0, 40.0), (141.0 + NAME_GAP, 40.0)];
        assert_eq!(place_names(&near, 0.0, 500.0, &[0, 0])[1].1, 0);
        assert_eq!(place_names(&near, 0.0, 500.0, &[0, 1])[1].1, 1);
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
        assert_eq!(
            facts(&p),
            ["40", "−7.64 ms", "−0.67 ms", "6.97 ms", "286°"].map(String::from)
        );
        assert_eq!(FACT_NAMES[3], "Unlock to Drop");
        // A short plot keeps Unlock to Drop and Amplitude.
        let names = |k: Vec<usize>| k.into_iter().map(|i| FACT_NAMES[i]).collect::<Vec<_>>();
        // A short plot keeps Unlock to Drop and Amplitude.
        assert_eq!(
            names(keep_rows(80.0, 3, 3)),
            ["Unlock to Drop", "Amplitude"]
        );
        assert_eq!(keep_rows(150.0, 6, 6), [0, 1, 2, 3, 4]);
        // Two or five: a tall plot without room for five keeps two.
        assert_eq!(keep_rows(110.0, 4, 4), [3, 4]);
        // Never taller than the plot: one row, then none.
        assert_eq!(names(keep_rows(40.0, 1, 1)), ["Amplitude"]);
        assert_eq!(keep_rows(30.0, 0, 0), Vec::<usize>::new());
        // Above the noise floor: Drop goes first, then Unlock, then Beats.
        assert_eq!(
            names(keep_rows(150.0, 6, 4)),
            ["Beats", "Unlock", "Unlock to Drop", "Amplitude"]
        );
        assert_eq!(
            names(keep_rows(150.0, 6, 3)),
            ["Beats", "Unlock to Drop", "Amplitude"]
        );
    }

    /// The facts box sits in the plot's top-left corner, as tall as its
    /// rows and as wide as both columns, so it is the same on both plots.
    #[test]
    fn the_facts_box_sits_in_the_plots_corner() {
        let b = FactBox {
            base: egui_plot::PlotItemBase::new("Tick facts".into()),
            rows: vec![(String::new(), String::new()); 5],
            plot_h: 150.0,
            floor: None,
            name_w: 90.0,
            value_w: 60.0,
            short_name_w: 80.0,
            short_value_w: 40.0,
            fill: Color32::WHITE,
            name_color: Color32::GRAY,
            value_color: Color32::BLACK,
        };
        let frame = egui::Rect::from_min_size(egui::pos2(50.0, 30.0), egui::vec2(800.0, 150.0));
        let r = b.rect(frame, 5);
        assert_eq!(r.min, egui::pos2(54.0, 34.0));
        assert_eq!(r.width(), 90.0 + FACT_GAP + 60.0 + 2.0 * FACT_PAD);
        assert_eq!(r.height(), 5.0 * FACT_ROW + 2.0 * FACT_PAD);
        // A short plot's two rows: as narrow as they are, tight around them.
        let r = b.rect(frame, 2);
        assert_eq!(r.width(), 80.0 + FACT_GAP + 40.0 + 2.0 * FACT_PAD_SHORT);
        assert_eq!(r.height(), 2.0 * FACT_ROW_SHORT + 2.0 * FACT_PAD_SHORT);
        // A plot 44 pt tall holds the two rows, even over a high noise
        // floor; never none.
        let short = FactBox {
            plot_h: 44.0,
            floor: Some(0.9),
            ..b.clone()
        };
        let frame = egui::Rect::from_min_size(egui::pos2(50.0, 30.0), egui::vec2(300.0, 44.0));
        let t = egui_plot::PlotTransform::new(
            frame,
            egui_plot::PlotBounds::from_min_max([0.0, 0.0], [1.0, 1.0]),
            egui::Vec2b::new(false, false),
        );
        assert_eq!(short.shown(&t), vec![3, 4]);
        // It adds nothing to the plot's bounds.
        assert!(!egui_plot::PlotItem::bounds(&b).is_valid());
    }

    /// Inside a short plot the edges' names are one row: centred on their
    /// lines; Unlock right of its line where the facts box has the corner;
    /// Peak unnamed where it would crowd Drop (its line's hover names it).
    #[test]
    fn a_short_plots_edge_names_keep_one_row() {
        let at = inside_places(
            &[
                ("Unlock", 400.0, 40.0),
                ("Drop", 700.0, 30.0),
                ("Peak", 900.0, 30.0),
            ],
            0.0,
            1000.0,
            200.0,
        );
        assert_eq!(at, vec![Some(380.0), Some(685.0), Some(885.0)]);
        // Unlock under the facts box: it starts at its line, right of it.
        let at = inside_places(&[("Unlock", 210.0, 40.0)], 0.0, 1000.0, 200.0);
        assert_eq!(at, vec![Some(214.0)]);
        // Its line under the box: unnamed, the facts left clear.
        let at = inside_places(&[("Unlock", 150.0, 40.0)], 0.0, 1000.0, 200.0);
        assert_eq!(at, vec![None]);
        // Peak 15 pt after Drop: not named.
        let at = inside_places(
            &[("Drop", 700.0, 30.0), ("Peak", 715.0, 30.0)],
            0.0,
            1000.0,
            0.0,
        );
        assert_eq!(at, vec![Some(685.0), None]);
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
