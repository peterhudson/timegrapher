//! The Isochronism pane: how the rate moves with the amplitude as the
//! mainspring lets down, and the 0 h / 24 h comparison, from the same
//! steadiness report as the Steadiness pane (core `steadiness`, as
//! `timegrapher series` gives them). Nothing is computed here: the curve
//! and the comparison come ready made with the report.

use crate::theme;
use eframe::egui::{self, RichText};
use egui_plot::{Line, Plot, Points, VLine};
use timegrapher_core::steadiness::{IsochronismCurve, WindDown};

/// Room for the 0 h / 24 h table under the chart, points.
const TABLE_H: f32 = 118.0;
/// Room for the vertical axis labels, as on the Steadiness pane.
const GUTTER: f32 = 64.0;

/// Draw the pane: the rate by amplitude, and the 0 h / 24 h table.
pub fn ui(ui: &mut egui::Ui, curve: Option<&IsochronismCurve>, wind: Option<&WindDown>) {
    let pal = theme::pal(ui);
    match curve {
        Some(c) => {
            ui.label(RichText::new(&c.headline).small().color(pal.text_secondary));
            ui.label(RichText::new(caption(c)).small().color(pal.text_tertiary));
            let h = (ui.available_height() - TABLE_H).max(120.0);
            chart(ui, c, h);
        }
        None => {
            let h = (ui.available_height() - TABLE_H).max(60.0);
            ui.allocate_ui(egui::vec2(ui.available_width(), h), |ui| {
                note(
                    ui,
                    "No isochronism curve yet: the amplitude has not moved enough. It needs at \
                     least three 2° steps of amplitude with 10 readings in each, which a run-down \
                     over some hours gives.",
                );
            });
        }
    }
    ui.add_space(8.0);
    table(ui, wind);
}

/// The line under the headline naming what the chart draws.
fn caption(c: &IsochronismCurve) -> String {
    let mut s = format!(
        "Dots: the median rate in each {:.0}° of amplitude, with bars over its middle half · \
         Line: the curve through them · {:+.2} s/day per degree over {:.0}° to {:.0}°",
        c.bin_deg, c.s_per_day_per_deg, c.amplitude_from_deg, c.amplitude_to_deg
    );
    if let Some(t) = c.turn_deg {
        s += &format!(" · Upright line: where the curve turns, {t:.0}°");
    }
    s
}

/// The rate at amplitude `a` on the quadratic.
fn curve_at(c: &IsochronismCurve, a: f64) -> f64 {
    let x = a - c.curve_centre_deg;
    c.curve[0] + c.curve[1] * x + c.curve[2] * x * x
}

/// A rate on the axis: as few decimals as the grid step needs, no "-0".
fn rate_label(v: f64, step: f64) -> String {
    let d = if step >= 1.0 {
        0
    } else if step >= 0.1 {
        1
    } else {
        2
    };
    // What rounds to zero prints as zero, not "-0".
    let scale = 10f64.powi(d as i32);
    let v = (v * scale).round() / scale + 0.0;
    format!("{v:+.d$} s/day")
}

fn chart(ui: &mut egui::Ui, c: &IsochronismCurve, h: f32) {
    let pal = theme::pal(ui);
    let colour = pal.trace_rate;
    let (Some(first), Some(last)) = (c.bins.first(), c.bins.last()) else {
        return;
    };
    let (a_lo, a_hi) = (first.amplitude_deg, last.amplitude_deg);
    let curve: Vec<[f64; 2]> = (0..=60)
        .map(|i| {
            let a = a_lo + (a_hi - a_lo) * i as f64 / 60.0;
            [a, curve_at(c, a)]
        })
        .collect();
    // Scale to the bars and the curve, padded by 15% as the charts are.
    let (lo, hi) = c
        .bins
        .iter()
        .flat_map(|b| [b.rate_q25, b.rate_q75])
        .chain(curve.iter().map(|p| p[1]))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
            (l.min(v), h.max(v))
        });
    let pad = ((hi - lo) * 0.15).max(0.05);
    let a_pad = ((a_hi - a_lo) * 0.05).max(c.bin_deg);
    Plot::new("isochronism")
        .allow_scroll(false)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .show_x(false)
        .show_y(false)
        .y_axis_min_width(GUTTER)
        .height(h)
        .x_grid_spacer(|g| theme::even_grid(g, 80.0, &[]))
        .x_axis_formatter(|m, _| format!("{:.0}°", m.value))
        .y_grid_spacer(|g| theme::even_grid(g, 24.0, &[]))
        .y_axis_formatter(|m, _| rate_label(m.value, m.step_size))
        .default_x_bounds(a_lo - a_pad, a_hi + a_pad)
        .default_y_bounds(lo - pad, hi + pad)
        .show(ui, |p| {
            for b in &c.bins {
                p.line(
                    Line::new(
                        "middle half",
                        vec![[b.amplitude_deg, b.rate_q25], [b.amplitude_deg, b.rate_q75]],
                    )
                    .color(colour)
                    .width(1.2_f32),
                );
            }
            p.line(Line::new("curve", curve).color(colour).width(1.2_f32));
            p.points(
                Points::new(
                    "median",
                    c.bins
                        .iter()
                        .map(|b| [b.amplitude_deg, b.rate_s_per_day])
                        .collect::<Vec<_>>(),
                )
                .color(colour)
                .radius(3.0_f32),
            );
            if let Some(t) = c.turn_deg {
                p.vline(
                    VLine::new("turn", t)
                        .color(pal.text_secondary)
                        .width(1.0_f32),
                );
            }
        });
}

/// The 0 h / 24 h table: the start of the take against 24 h after full
/// wind, or against its last half hour.
fn table(ui: &mut egui::Ui, wind: Option<&WindDown>) {
    let pal = theme::pal(ui);
    ui.label(theme::caption("0 h / 24 h").color(pal.text_secondary));
    let Some(w) = wind else {
        ui.label(
            RichText::new(
                "The take is too short: the comparison needs at least 2 hours, and 24 hours \
                 for the full test.",
            )
            .small()
            .color(pal.text_secondary),
        );
        return;
    };
    let hours = |s: &timegrapher_core::steadiness::Stretch| {
        format!("{:.1}–{:.1} h", s.from_wind_h, s.to_wind_h)
    };
    let end = if w.at_24h {
        "At 24 h"
    } else {
        "Last Half Hour"
    };
    let dash = || "–".to_string();
    let rate = |v: Option<f64>| v.map_or_else(dash, |v| format!("{v:+.1} s/day"));
    let amp = |v: Option<f64>| v.map_or_else(dash, |v| format!("{v:.0}°"));
    let amp_change = |v: Option<f64>| v.map_or_else(dash, |v| format!("{v:+.0}°"));
    let be = |v: Option<f64>| v.map_or_else(dash, |v| format!("{v:.2} ms"));
    let be_change = |v: Option<f64>| v.map_or_else(dash, |v| format!("{v:+.2} ms"));
    let rows = [
        (
            "Rate",
            pal.trace_rate,
            rate(w.start.rate_s_per_day),
            rate(w.end.rate_s_per_day),
            rate(w.rate_change_s_per_day),
        ),
        (
            "Amplitude",
            pal.trace_amplitude,
            amp(w.start.amplitude_deg),
            amp(w.end.amplitude_deg),
            amp_change(w.amplitude_change_deg),
        ),
        (
            "Beat Error",
            pal.trace_beat_error,
            be(w.start.beat_error_ms),
            be(w.end.beat_error_ms),
            be_change(w.beat_error_change_ms),
        ),
    ];
    let small = |t: &str| RichText::new(t).small().color(pal.text_secondary);
    egui::Grid::new("wind_down")
        .num_columns(4)
        .spacing([18.0, 3.0])
        .min_col_width(70.0)
        .show(ui, |ui| {
            ui.label("");
            ui.label(small(&format!("Start ({})", hours(&w.start))));
            ui.label(small(&format!("{end} ({})", hours(&w.end))));
            ui.label(small("Change"));
            ui.end_row();
            for (name, colour, a, b, d) in rows {
                ui.label(RichText::new(name).color(colour));
                ui.label(a);
                ui.label(b);
                ui.label(d);
                ui.end_row();
            }
        });
    ui.label(RichText::new(&w.headline).small().color(pal.text_tertiary));
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
    use timegrapher_core::steadiness::{AmplitudeBin, Stretch};

    fn stretch(from: f64, rate: f64, amp: f64, be: f64) -> Stretch {
        Stretch {
            from_wind_h: from,
            to_wind_h: from + 0.5,
            readings: 180,
            rate_s_per_day: Some(rate),
            amplitude_deg: Some(amp),
            beat_error_ms: Some(be),
        }
    }

    /// A run-down drawn by hand: flat above 250°, falling below.
    #[test]
    fn draws_a_curve_and_the_table_and_says_why_when_either_is_missing() {
        let bins: Vec<AmplitudeBin> = (0..8)
            .map(|i| {
                let a = 236.0 + 4.0 * i as f64;
                let r = 17.0 - 0.02 * (252.0 - a).max(0.0).powi(2);
                AmplitudeBin {
                    amplitude_deg: a,
                    readings: 200,
                    rate_s_per_day: r,
                    rate_q25: r - 0.5,
                    rate_q75: r + 0.5,
                    from_wind_h: 15.0 - 2.0 * i as f64,
                    to_wind_h: 16.0 - 2.0 * i as f64,
                }
            })
            .collect();
        let curve = IsochronismCurve {
            step_s: 10.0,
            bin_deg: 2.0,
            bins,
            amplitude_from_deg: 235.0,
            amplitude_to_deg: 265.0,
            s_per_day_per_deg: 0.4,
            rate_change_s_per_day: 12.0,
            curve_centre_deg: 250.0,
            curve: [16.8, 0.2, -0.015],
            curve_explained: 0.95,
            turn_deg: Some(256.7),
            headline: "The rate falls as the amplitude does.".into(),
        };
        let wind = WindDown {
            start: stretch(0.0, 17.3, 262.0, 0.21),
            end: stretch(15.3, 7.4, 241.0, 0.33),
            hours_apart: 15.3,
            at_24h: false,
            rate_change_s_per_day: Some(-9.9),
            amplitude_change_deg: Some(-21.0),
            beat_error_change_ms: Some(0.12),
            headline: "From the start to the last half hour, the rate fell 9.9 s/day.".into(),
        };
        assert!(caption(&curve).contains("where the curve turns, 257°"));
        assert!((curve_at(&curve, 250.0) - 16.8).abs() < 1e-12);
        assert_eq!(rate_label(-0.0001, 0.5), "+0.0 s/day");

        let ctx = egui::Context::default();
        theme::install(&ctx);
        for (c, w) in [
            (Some(&curve), Some(&wind)),
            (None, Some(&wind)),
            (Some(&curve), None),
            (None, None),
        ] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 500.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |u| super::ui(u, c, w));
            });
            assert!(!out.shapes.is_empty());
        }
    }
}
