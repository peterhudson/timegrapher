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

/// The facts on each plot, a row each, top to bottom.
const FACT_NAMES: [&str; 5] = ["Beats", "Unlock", "Drop", "Unlock to Drop", "Amplitude"];

/// Below this height a plot's facts keep only their last two rows, Unlock
/// to Drop and Amplitude; the rest are in the hover.
const SHORT_PLOT: f32 = 100.0;

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

/// The rows a plot's facts show: all five, or on a short plot the last
/// two.
fn fact_rows(short: bool) -> std::ops::Range<usize> {
    if short {
        3..5
    } else {
        0..5
    }
}

/// A side's facts in the plot's top-left corner, as a plot item so they
/// sit over the grid and under the band and the trace: a spike that runs
/// into them still shows. The same box at the same place on both plots,
/// so Tick and Tock compare row against row.
struct FactBox {
    base: egui_plot::PlotItemBase,
    rows: Vec<(String, String)>,
    /// The values' column, right-aligned, as wide as the widest value on
    /// either side; and the names' column likewise.
    name_w: f32,
    value_w: f32,
    fill: Color32,
    name_color: Color32,
    value_color: Color32,
}

/// Room inside the box around its rows, and each row's height, points.
const FACT_PAD: f32 = 8.0;
const FACT_ROW: f32 = 16.0;
/// Between the names' column and the values'.
const FACT_GAP: f32 = 16.0;
/// The facts' type size.
const FACT_SIZE: f32 = 12.0;

impl FactBox {
    /// Where the box goes in a plot whose frame is `frame`.
    fn rect(&self, frame: egui::Rect) -> egui::Rect {
        egui::Rect::from_min_size(
            frame.min + egui::vec2(4.0, 4.0),
            egui::vec2(
                self.name_w + FACT_GAP + self.value_w + 2.0 * FACT_PAD,
                self.rows.len() as f32 * FACT_ROW + 2.0 * FACT_PAD,
            ),
        )
    }
}

impl egui_plot::PlotItem for FactBox {
    fn shapes(
        &self,
        ui: &egui::Ui,
        transform: &egui_plot::PlotTransform,
        shapes: &mut Vec<egui::Shape>,
    ) {
        let r = self.rect(*transform.frame());
        shapes.push(egui::Shape::rect_filled(r, 6.0, self.fill));
        let font = egui::FontId::proportional(FACT_SIZE);
        for (i, (name, value)) in self.rows.iter().enumerate() {
            let y = r.top() + FACT_PAD + (i as f32 + 0.5) * FACT_ROW;
            ui.fonts_mut(|f| {
                let g = f.layout_no_wrap(name.clone(), font.clone(), self.name_color);
                let at = egui::pos2(r.left() + FACT_PAD, y - g.size().y / 2.0);
                shapes.push(egui::Shape::galley(at, g, self.name_color));
                let g = f.layout_no_wrap(value.clone(), font.clone(), self.value_color);
                let at = egui::pos2(r.right() - FACT_PAD - g.size().x, y - g.size().y / 2.0);
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
    let none_w = widest(ui, [NO_FACTS].into_iter());
    let box_fill = ui.visuals().extreme_bg_color.gamma_multiply(0.8);
    let fact_box = |k: usize, short: bool| {
        let rows = match &facts[k] {
            Some(f) => fact_rows(short)
                .map(|i| (FACT_NAMES[i].to_string(), f[i].clone()))
                .collect(),
            None => vec![(NO_FACTS.to_string(), String::new())],
        };
        FactBox {
            base: egui_plot::PlotItemBase::new(format!("{} facts", ["Tick", "Tock"][k])),
            name_w: if facts[k].is_some() { name_w } else { none_w },
            value_w: if facts[k].is_some() { value_w } else { 0.0 },
            rows,
            fill: box_fill,
            name_color: pal.text_secondary,
            value_color: pal.text,
        }
    };
    let side = |ui: &mut egui::Ui, k: usize| {
        let p = &profiles[k];
        let dx = shift[k];
        let name = ["Tick", "Tock"][k];
        let c = colors[k];
        // The edges' names in a lane above the plot, the sounds' in a lane
        // below it, then the time axis: none of them inside the plot.
        let top_lane = if opt.edges { LANE } else { 0.0 } + LANE_GAP;
        let bottom_lane = if opt.sounds { LANE } else { 0.0 } + LANE_GAP + AXIS_H;
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
        let plot_h = (ui.available_height() - bottom_lane).max(40.0);
        let short = plot_h < SHORT_PLOT;
        let facts_here = fact_box(k, short);
        let facts_rect = |frame| facts_here.rect(frame);
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
                if (m.value - r.start()) / (r.end() - r.start()) * h < 9.0 {
                    return String::new();
                }
                match scale {
                    Scale::Decibels => format!("{:.0} dB", m.value + 0.0),
                    Scale::Linear => format!("{:.2}", m.value + 0.0),
                }
            });
        // Before there are beats, a typical stretch of time rather than none.
        let plot = if p.is_none() {
            plot.default_x_bounds(-20.0, 10.0)
        } else {
            plot
        };
        let mut med = Vec::new();
        let mut drawn_box = None;
        let resp = plot.show(ui, |pl| {
            pl.set_plot_bounds_y(y_lo..=y_hi);
            // Over the grid, under the band and the trace.
            drawn_box = Some(facts_rect(*pl.transform().frame()));
            pl.add(fact_box(k, short));
            let Some(p) = p else { return };
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
        // A short plot's facts keep two rows; the hover has all five.
        if let (true, Some(f), Some(r)) = (short, &facts[k], drawn_box) {
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
        time_axis(ui, &resp.transform, foot);
        if let Some(p) = p {
            label_marks(ui, &resp.transform, p, &med, opt, dx, k);
        }
    };
    if opt.side_by_side {
        ui.columns(2, |cols| {
            for (k, ui) in cols.iter_mut().enumerate() {
                side(ui, k);
            }
        });
    } else {
        // Each side in its own half, so both keep a whole time axis.
        let gap = 6.0;
        let w = ui.available_width();
        let h = ((ui.available_height() - gap) / 2.0).max(60.0);
        for k in 0..2 {
            ui.allocate_ui(egui::vec2(w, h), |ui| {
                ui.set_height(h);
                side(ui, k)
            });
            if k == 0 {
                ui.add_space(gap);
            }
        }
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

/// Where each name in a lane goes: `names` are each one's centre (its
/// line's or dot's x) and width, left to right; `was` is the row each was
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
/// of the trace; each sound a dot on the trace where it has risen halfway,
/// with a dotted leader straight down past the plot to its name below.
/// A line from the top is an edge, a dot on the curve is a sound.
fn label_marks(
    ui: &egui::Ui,
    t: &egui_plot::PlotTransform,
    p: &TickProfile,
    med: &[f64],
    opt: Options,
    dx: f64,
    side: usize,
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
        let id = ui.id().with(("profile lane", side, up));
        let was: Vec<u8> = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        let names: Vec<(f32, f32)> = marks
            .iter()
            .zip(&galleys)
            .map(|((x, _, _), g)| (*x, g.size().x))
            .collect();
        let placed = place_names(&names, frame.left(), frame.right(), &was);
        ui.data_mut(|d| d.insert_temp(id, placed.iter().map(|p| p.1).collect::<Vec<u8>>()));
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
                let stop = on.map_or(frame.bottom(), |y| (y - 4.0).max(frame.top()));
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
            } else if let Some(y) = on {
                let mut pts = vec![
                    egui::pos2(x, y + 3.0),
                    egui::pos2(x, frame.bottom() + LANE_GAP / 2.0),
                ];
                pts.push(egui::pos2(centre, end_y));
                painter.extend(egui::Shape::dashed_line(
                    &pts,
                    egui::Stroke::new(1.0_f32, pal.sound),
                    2.0,
                    2.0,
                ));
                painter.circle_filled(egui::pos2(x, y), 3.0, pal.text);
            }
            painter.galley(name.min, g, ink);
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
    };
    if opt.edges {
        lane(
            [
                ("Unlock", p.unlock_ms),
                ("Drop", p.drop_ms),
                ("Peak", p.peak_ms),
            ]
            .into_iter()
            .filter_map(|(l, v)| v.map(|v| (l, v)))
            .collect(),
            true,
        );
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
        let short: Vec<&str> = fact_rows(true).map(|i| FACT_NAMES[i]).collect();
        assert_eq!(short, ["Unlock to Drop", "Amplitude"]);
        assert_eq!(fact_rows(false), 0..5);
    }

    /// The facts box sits in the plot's top-left corner, as tall as its
    /// rows and as wide as both columns, so it is the same on both plots.
    #[test]
    fn the_facts_box_sits_in_the_plots_corner() {
        let b = FactBox {
            base: egui_plot::PlotItemBase::new("Tick facts".into()),
            rows: vec![(String::new(), String::new()); 5],
            name_w: 90.0,
            value_w: 60.0,
            fill: Color32::WHITE,
            name_color: Color32::GRAY,
            value_color: Color32::BLACK,
        };
        let frame = egui::Rect::from_min_size(egui::pos2(50.0, 30.0), egui::vec2(800.0, 150.0));
        let r = b.rect(frame);
        assert_eq!(r.min, egui::pos2(54.0, 34.0));
        assert_eq!(r.width(), 90.0 + FACT_GAP + 60.0 + 2.0 * FACT_PAD);
        assert_eq!(r.height(), 5.0 * FACT_ROW + 2.0 * FACT_PAD);
        // It adds nothing to the plot's bounds.
        assert!(!egui_plot::PlotItem::bounds(&b).is_valid());
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
