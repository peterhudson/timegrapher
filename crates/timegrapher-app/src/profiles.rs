//! The tick and tock sound pane: the typical sound of the ticks and of the
//! tocks, with the band most beats fall in and the marks the engine read
//! the amplitude and beat error from.

use crate::strip::side_colors;
use crate::theme;
use eframe::egui::{self, Color32, RichText};
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

/// The facts table's columns after the side's name, each heading with its
/// unit.
const FACT_HEADS: [&str; 5] = [
    "Beats",
    "Unlock (ms)",
    "Drop (ms)",
    "Unlock to Drop (ms)",
    "Amplitude",
];

/// One side's row of the facts table, in [`FACT_HEADS`]' order.
pub fn facts(p: &TickProfile) -> [String; 5] {
    let ms = |v: Option<f64>| v.map_or("–".to_string(), |v| format!("{v:+.2}").replace('-', "−"));
    [
        p.beats.to_string(),
        ms(p.unlock_ms),
        ms(p.drop_ms),
        match (p.unlock_ms, p.drop_ms) {
            (Some(u), Some(d)) => format!("{:.2}", d - u),
            _ => "–".to_string(),
        },
        p.amplitude_deg
            .map_or("out of range".to_string(), |a| format!("{a:.0}°")),
    ]
}

/// The facts above the plots: one row per side, so Tick and Tock compare
/// down each column.
fn facts_table(ui: &mut egui::Ui, profiles: &[Option<TickProfile>; 2], colors: [Color32; 2]) {
    let pal = theme::pal(ui);
    let small = egui::FontId::proportional(11.0);
    // Each figure right-aligned under its heading, as wide as the heading
    // or the widest figure.
    let rows: Vec<Option<[String; 5]>> = profiles.iter().map(|p| p.as_ref().map(facts)).collect();
    let widths: Vec<f32> = FACT_HEADS
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let head = ui.fonts_mut(|f| {
                f.layout_no_wrap(h.to_string(), small.clone(), Color32::WHITE)
                    .size()
                    .x
            });
            rows.iter()
                .flatten()
                .map(|r| {
                    ui.fonts_mut(|f| {
                        f.layout_no_wrap(
                            r[i].clone(),
                            egui::FontId::proportional(12.5),
                            Color32::WHITE,
                        )
                        .size()
                        .x
                    })
                })
                .fold(head, f32::max)
        })
        .collect();
    let cell = |ui: &mut egui::Ui, w: f32, text: RichText| {
        ui.allocate_ui_with_layout(
            egui::vec2(w, 16.0),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_min_width(w);
                ui.label(text)
            },
        );
    };
    let name_w = 60.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 18.0;
        ui.add_space(name_w);
        for (h, &w) in FACT_HEADS.iter().zip(&widths) {
            cell(
                ui,
                w,
                RichText::new(*h)
                    .font(small.clone())
                    .color(pal.text_secondary),
            );
        }
    });
    for (k, row) in rows.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            ui.allocate_ui_with_layout(
                egui::vec2(name_w, 16.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(name_w);
                    ui.spacing_mut().item_spacing.x = 6.0;
                    theme::dot(ui, colors[k]);
                    ui.label(RichText::new(["Tick", "Tock"][k]).font(theme::semibold(12.5)));
                },
            );
            match row {
                Some(r) => {
                    for (v, &w) in r.iter().zip(&widths) {
                        cell(ui, w, RichText::new(v).size(12.5));
                    }
                }
                None => {
                    ui.label(RichText::new("Not enough beats yet").color(pal.text_secondary));
                }
            }
        });
    }
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
) {
    let shift = offsets(opt, drop_beat_error_ms);
    let pal = theme::pal(ui);
    if let Some(n) = note {
        ui.label(RichText::new(n).small().color(pal.text_secondary));
    }
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
    facts_table(ui, profiles, colors);
    ui.add_space(4.0);
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
        let plot = Plot::new(("profile", k))
            .height((ui.available_height() - bottom_lane).max(40.0))
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
        let resp = plot.show(ui, |pl| {
            pl.set_plot_bounds_y(y_lo..=y_hi);
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
    fn the_facts_are_one_row_per_side() {
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
            ["40", "−7.64", "−0.67", "6.97", "286°"].map(String::from)
        );
        assert_eq!(FACT_HEADS[3], "Unlock to Drop (ms)");
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
