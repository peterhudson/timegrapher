//! The Isochronism pane: how the rate moves with the amplitude as the
//! mainspring lets down, from the same steadiness report as the
//! Steadiness pane (core `steadiness`, as `timegrapher series` gives it).
//! Nothing is computed here: the curve comes ready made with the report.

use crate::marks;
use crate::theme;
use eframe::egui::{self, RichText};
use egui_plot::{Line, Plot, Points, VLine};
use timegrapher_core::steadiness::IsochronismCurve;

/// Room for the vertical axis labels, as on the Steadiness pane.
const GUTTER: f32 = 64.0;

/// Draw the pane: the rate by amplitude.
pub fn ui(ui: &mut egui::Ui, curve: Option<&IsochronismCurve>) {
    let pal = theme::pal(ui);
    match curve {
        Some(c) => {
            ui.label(RichText::new(&c.headline).small().color(pal.text_secondary));
            ui.label(RichText::new(caption(c)).small().color(pal.text_tertiary));
            let h = ui.available_height().max(120.0);
            chart(ui, c, h);
        }
        None => note(
            ui,
            "No isochronism curve yet: the amplitude has not moved enough. It needs at least \
             three 2° steps of amplitude with 10 readings in each, which a run-down over some \
             hours gives.",
        ),
    }
}

/// The line under the headline naming what the chart draws.
fn caption(c: &IsochronismCurve) -> String {
    // The slope and its amplitudes are the headline's: said once.
    let mut s = format!(
        "Dots: the median rate in each {:.0}° of amplitude, with bars over its middle half · \
         Line: the curve through them",
        c.bin_deg
    );
    if let Some(t) = c.turn_deg {
        s += &format!(" · Turn: where the curve turns, {t:.0}°");
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
    format!("{v:+.d$} s/d")
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
    // The turn is named in a lane of one row above the plot, as every
    // named line is (see `marks`).
    let lane = if c.turn_deg.is_some() {
        marks::ROW_H + marks::LANE_GAP
    } else {
        0.0
    };
    ui.add_space(lane);
    let h = (h - lane).max(40.0);
    let resp = Plot::new("isochronism")
        .allow_scroll(false)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_axis_zoom_drag(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .show_x(false)
        .show_y(false)
        .y_axis_min_width(GUTTER)
        .height(h)
        .x_grid_spacer(|g| theme::even_grid(g, 80.0, &[]))
        .x_axis_formatter(|m, r| {
            theme::clear_of_right_edge(format!("{:.0}°", m.value), m.value, r, 0.03)
        })
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
    if let Some(t) = c.turn_deg {
        let turn = marks::Named {
            x: resp.transform.position_from_point_x(t),
            name: "Turn".into(),
            rank: marks::Rank::Strong,
            ink: pal.text_secondary,
            hover: format!("Turn: {t:.0}°"),
        };
        marks::lane_above_one(ui, *resp.transform.frame(), &[turn], ui.id().with("turn"));
    }
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
    use timegrapher_core::steadiness::AmplitudeBin;

    /// A run-down drawn by hand: flat above 250°, falling below.
    fn run_down() -> IsochronismCurve {
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
        IsochronismCurve {
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
        }
    }

    #[test]
    fn draws_a_curve_and_says_why_when_it_is_missing() {
        let curve = run_down();
        assert!(caption(&curve).contains("where the curve turns, 257°"));
        // The slope and its amplitudes only in the headline.
        assert!(
            !caption(&curve).contains("per degree"),
            "{}",
            caption(&curve)
        );
        assert!((curve_at(&curve, 250.0) - 16.8).abs() < 1e-12);
        assert_eq!(rate_label(-0.0001, 0.5), "+0.0 s/d");

        let ctx = egui::Context::default();
        theme::install(&ctx);
        for c in [Some(&curve), None] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 500.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |u| super::ui(u, c));
            });
            assert!(!out.shapes.is_empty());
        }
    }

    /// The turn is named "Turn" above the plot, over its line, and the
    /// rate labels stay below it.
    #[test]
    fn the_turn_is_named_above_the_plot() {
        let curve = run_down();
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700.0, 500.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |u| super::ui(u, Some(&curve)));
        });
        let texts: Vec<(String, egui::Rect)> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((
                    t.galley.text().to_string(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                )),
                _ => None,
            })
            .collect();
        let turn = texts.iter().find(|t| t.0 == "Turn").expect("Turn is named");
        let rates: Vec<_> = texts.iter().filter(|t| t.0.ends_with(" s/d")).collect();
        assert!(!rates.is_empty(), "{texts:?}");
        assert!(
            rates.iter().all(|r| r.1.top() >= turn.1.bottom()),
            "{texts:?}"
        );
        // Over its line: 256.7° sits right of the middle of 236° to 264°.
        let x = turn.1.center().x;
        assert!(x > 350.0 && x < 700.0, "{turn:?}");
    }
}
