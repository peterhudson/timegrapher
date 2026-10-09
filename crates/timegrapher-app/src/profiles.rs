//! The tick and tock sound pane: the typical sound of the ticks and of the
//! tocks, with the band most beats fall in and the marks the engine read
//! the amplitude and beat error from.

use crate::strip::side_colors;
use crate::theme;
use eframe::egui::{self, Color32, RichText};
use egui_plot::{HLine, Line, LineStyle, Plot, Polygon, VLine};
use timegrapher_core::profile::TickProfile;

/// How the sound's level is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// A line for the profile's summary under its title.
pub fn summary(p: &TickProfile) -> String {
    let ms = |v: Option<f64>| v.map_or("not found".to_string(), |v| format!("{v:+.2} ms"));
    let mut s = format!(
        "{} beats · unlock {} · drop {}",
        p.beats,
        ms(p.unlock_ms),
        ms(p.drop_ms)
    );
    if let (Some(u), Some(d)) = (p.unlock_ms, p.drop_ms) {
        s += &format!(" · unlock to drop {:.2} ms", d - u);
    }
    match p.amplitude_deg {
        Some(a) => s += &format!(" · amplitude {a:.0}°"),
        None => s += " · amplitude out of range",
    }
    s
}

/// Draw both sides, tick above tock, sharing their axes.
pub fn draw(
    ui: &mut egui::Ui,
    profiles: &[Option<TickProfile>; 2],
    scale: &mut Scale,
    note: Option<&str>,
) {
    let pal = theme::pal(ui);
    ui.horizontal(|ui| {
        theme::segmented(
            ui,
            scale,
            &[
                (
                    Scale::Linear,
                    "Linear",
                    "The sound as the engine measures it",
                ),
                (
                    Scale::Decibels,
                    "dB",
                    "The level below the loudest point of each side, which makes the \
                     quiet unlock easier to see",
                ),
            ],
        );
        // The marks are named on the plots; this says what the line styles
        // mean.
        if let Some(n) = note {
            ui.label(RichText::new(n).small().color(pal.text_secondary));
        }
        let key = "solid: the edges amplitude and beat error are read from · dashed: \
                   the three sounds · flat dashed: noise floor";
        ui.add(egui::Label::new(RichText::new(key).small().weak()).truncate())
            .on_hover_text(key);
    });
    let colors = side_colors(ui.visuals().dark_mode);
    // Both sides on one scale, so their loudness compares.
    let top = profiles
        .iter()
        .flatten()
        .flat_map(|p| p.p90.iter().copied())
        .fold(0.0f32, f32::max);
    let h = ((ui.available_height() - 12.0) / 2.0).max(60.0);
    let scale = *scale;
    for (k, p) in profiles.iter().enumerate() {
        let name = ["Tick", "Tock"][k];
        let c = colors[k];
        ui.horizontal(|ui| {
            theme::dot(ui, c);
            ui.label(RichText::new(name).font(theme::semibold(12.5)).color(c));
            let s = match p {
                Some(p) => summary(p),
                None => "not enough beats yet".into(),
            };
            ui.add(
                egui::Label::new(RichText::new(&s).small().color(pal.text_secondary)).truncate(),
            )
            .on_hover_text(s);
        });
        let plot = Plot::new(("profile", k))
            .height(h - 24.0)
            .link_axis("profile", [true, true])
            // No crosshair or value box: the marks and the summary above say
            // what matters.
            .show_x(false)
            .show_y(false)
            .allow_scroll(false)
            .x_axis_formatter(|m, _| format!("{} ms", m.value))
            .y_axis_min_width(52.0)
            .y_axis_formatter(move |m, _| match scale {
                Scale::Decibels => format!("{:.0} dB", m.value),
                Scale::Linear => format!("{:.2}", m.value),
            })
            .include_y(if scale == Scale::Decibels {
                FLOOR_DB
            } else {
                0.0
            });
        let resp = plot.show(ui, |pl| {
            let Some(p) = p else { return };
            let x = |i: usize| p.t0_ms + i as f64 * p.step_ms;
            let (med, lo, hi) = (
                scaled(&p.median, top, scale),
                scaled(&p.p10, top, scale),
                scaled(&p.p90, top, scale),
            );
            // The band as thin quads: plots fill only convex shapes.
            let band = c.gamma_multiply(0.25);
            for i in 1..med.len().min(lo.len()).min(hi.len()) {
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
                    med.iter()
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
            let marks = [
                ("unlock", p.unlock_ms, pal.unlock, 1.5_f32),
                ("drop", p.drop_ms, pal.drop, 1.5),
                ("drop peak", p.peak_ms, pal.peak, 1.0),
            ];
            for (label, at, col, w) in marks {
                if let Some(t) = at {
                    pl.vline(VLine::new(label, t).color(col).width(w));
                }
            }
            for (label, at) in [
                ("sound 1", p.sound1_ms),
                ("sound 2", p.sound2_ms),
                ("sound 3", p.sound3_ms),
            ] {
                if let Some(t) = at {
                    pl.vline(
                        VLine::new(label, t)
                            .color(pal.text_tertiary)
                            .style(LineStyle::dashed_loose()),
                    );
                }
            }
        });
        if let Some(p) = p {
            label_marks(ui, &resp.transform, p);
        }
    }
}

/// Name each mark on the plot itself, along the top: the engine's edges
/// first, then the three sounds, each label just right of its line and
/// moved down a row where it would run into another.
fn label_marks(ui: &egui::Ui, t: &egui_plot::PlotTransform, p: &TickProfile) {
    let frame = *t.frame();
    let painter = ui.painter_at(frame);
    let font = egui::FontId::proportional(11.0);
    let pal = theme::pal(ui);
    let grey = pal.text_secondary;
    let mut ends: Vec<f32> = Vec::new();
    for group in [
        [
            ("unlock", p.unlock_ms, pal.unlock),
            ("drop", p.drop_ms, pal.drop),
            ("peak", p.peak_ms, pal.peak),
        ],
        [
            ("1 unlock", p.sound1_ms, grey),
            ("2 impulse", p.sound2_ms, grey),
            ("3 drop", p.sound3_ms, grey),
        ],
    ] {
        let mut marks: Vec<(f32, &str, Color32)> = group
            .into_iter()
            .filter_map(|(l, at, c)| at.map(|v| (t.position_from_point_x(v), l, c)))
            .filter(|(x, _, _)| *x >= frame.left() && *x <= frame.right())
            .collect();
        marks.sort_by(|a, b| a.0.total_cmp(&b.0));
        // The sounds start on the row below the edges' lowest.
        let first = ends.len();
        for (x, label, c) in marks {
            let galley = painter.layout_no_wrap(label.to_string(), font.clone(), c);
            let row = (first..ends.len())
                .find(|&r| ends[r] + 4.0 < x)
                .unwrap_or(ends.len());
            let end = x + 3.0 + galley.size().x;
            if row == ends.len() {
                ends.push(end);
            } else {
                ends[row] = end;
            }
            let y = frame.top() + 2.0 + row as f32 * (galley.size().y + 1.0);
            painter.galley(egui::pos2(x + 3.0, y), galley, c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibels_below_the_top() {
        let v = scaled(&[1.0, 0.1, 0.0], 1.0, Scale::Decibels);
        assert!((v[0] - 0.0).abs() < 1e-9);
        assert!((v[1] + 20.0).abs() < 1e-6);
        assert_eq!(v[2], FLOOR_DB);
        assert_eq!(scaled(&[0.5], 1.0, Scale::Linear), vec![0.5]);
    }
}
