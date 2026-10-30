//! The Distributions pane: how each reading's values spread over the take.

use super::*;

impl TimegrapherApp {
    pub(super) fn histogram_settings(&mut self, ui: &mut egui::Ui) {
        theme::tool_choice(
            ui,
            "Show",
            "The values on probability paper, or their cumulative share",
            &mut self.hist_cumulative,
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
            &mut self.hist_readings,
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

    /// The 2-second stretches of amplitude and beat error, leaving out any
    /// that overlap a masked stretch or reach across a break, as the
    /// readings do.
    pub(super) fn unmasked_windows(&self) -> Vec<&timegrapher_core::amplitude::AmplitudeWindow> {
        let breaks = self.live.as_ref().map_or(&[][..], |l| l.breaks());
        self.live
            .as_ref()
            .map_or(&[][..], |l| l.amplitude_windows())
            .iter()
            .filter(|w| !breaks.iter().any(|&b| w.start_s < b && b < w.end_s))
            .filter(|w| {
                !self
                    .masked
                    .iter()
                    .any(|&(a, b)| w.start_s < b && w.end_s > a)
            })
            .collect()
    }

    /// The Distributions pane: a plot for each reading shown, one above
    /// the other, sharing the pane's height.
    pub(super) fn distributions_pane(&mut self, ui: &mut egui::Ui) {
        let shown = self.dist_series;
        self.note_need(ui, Pane::Distributions, DIST_MIN_H);
        stacked(ui, shown, DIST_MIN_H, |ui, k| self.histogram(ui, k));
    }

    /// How often each value of a reading came up, over the session or the
    /// strip's length: bars, or the cumulative share.
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    pub(super) fn histogram(&mut self, ui: &mut egui::Ui, reading: SeriesKind) {
        use timegrapher_core::histogram as hist;
        let pal = theme::pal(ui);
        let short = !self.hist_readings;
        if reading == SeriesKind::Rate && short {
            match self.live.as_ref() {
                Some(live) => self.rate_2s.refresh(live, &self.masked),
                None => self.rate_2s = RateTwoS::default(),
            }
        }
        // Only the position the watch is in now: values from another
        // position are a different measurement.
        let inside = |t: f64| self.in_position(t);
        let windows = self.unmasked_windows();
        // One reading per averaging time: a new reading comes every half
        // second, and the ones between share most of their beats, so counting
        // them all would stack up repeats and look surer than it is.
        let every = self.average_s;
        let trend = &self.trend;
        let readings = |f: fn(&TrendPoint) -> Option<f64>| -> Vec<f64> {
            let mut last = f64::NEG_INFINITY;
            trend
                .iter()
                .filter(|p| inside(p.t))
                .filter_map(|p| Some((p.t, f(p)?)))
                .filter(|&(t, _)| {
                    let keep = t >= last + every - 1e-6;
                    if keep {
                        last = t;
                    }
                    keep
                })
                .map(|(_, v)| v)
                .collect()
        };
        use timegrapher_core::distribution::{self as dist, AMPLITUDE, BEAT_ERROR, RATE};
        let (values, kind, color, label): (Vec<f64>, dist::Kind, _, &'static str) = match reading {
            SeriesKind::Rate if short => (
                self.rate_2s
                    .values
                    .iter()
                    .filter(|&&(a, _)| inside(a + RateTwoS::STEP_S) && self.in_position(a + 1e-6))
                    .map(|&(_, v)| v)
                    .collect(),
                RATE,
                pal.trace_rate,
                "2 s",
            ),
            SeriesKind::Rate => (readings(|p| p.rate), RATE, pal.trace_rate, "readings"),
            SeriesKind::Amplitude if short => (
                windows
                    .iter()
                    .filter(|w| inside(w.end_s) && self.in_position(w.start_s + 1e-6))
                    .filter_map(|w| w.mean())
                    .collect(),
                AMPLITUDE,
                pal.trace_amplitude,
                "2 s",
            ),
            SeriesKind::Amplitude => (
                readings(|p| p.amplitude),
                AMPLITUDE,
                pal.trace_amplitude,
                "readings",
            ),
            SeriesKind::BeatError if short => (
                windows
                    .iter()
                    .filter(|w| inside(w.end_s) && self.in_position(w.start_s + 1e-6))
                    .filter_map(|w| {
                        core_session::beat_error_signed(w.beat_error_unlock_ms, w.beat_error_ms)
                    })
                    .collect(),
                BEAT_ERROR,
                pal.trace_beat_error,
                "2 s",
            ),
            SeriesKind::BeatError => (
                readings(TrendPoint::beat_error),
                BEAT_ERROR,
                pal.trace_beat_error,
                "readings",
            ),
        };
        let step = if label == "2 s" {
            dist::STRETCH_S
        } else {
            self.average_s
        };
        let Some(d) = dist::distribution(&values, kind, label, step) else {
            ui.label(
                RichText::new(format!(
                    "{}: not enough readings yet",
                    steady::title(reading)
                ))
                .small()
                .color(pal.text_secondary),
            );
            return;
        };
        let only = (!self.session.stretches().is_empty()).then(|| self.position.name());
        let line = format!(
            "{}: {}",
            steady::title(reading),
            d.line(kind, " in the session", only)
        );
        ui.add(egui::Label::new(RichText::new(&line).small().color(pal.text_secondary)).truncate())
            .on_hover_text(line.clone());
        let (x_lo, x_hi) = (d.edges[0], d.edges[d.edges.len() - 1]);
        let unit_owned = kind.suffix.to_string();
        let cumulative = self.hist_cumulative;
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
            .x_grid_spacer(|g| theme::even_grid(g, 80.0, &[]))
            .x_axis_formatter(move |m, _| {
                let d = if m.step_size >= 1.0 {
                    0
                } else if m.step_size >= 0.1 {
                    1
                } else {
                    2
                };
                // No "-0" from rounding.
                let v = if m.value.abs() < m.step_size * 1e-6 {
                    0.0
                } else {
                    m.value
                };
                format!("{:.*}{unit_owned}", d, v)
            })
            .y_axis_min_width(44.0)
            .default_x_bounds(x_lo, x_hi);
        if cumulative {
            // The share of values at or below each value, as a step line.
            let mut pts: Vec<[f64; 2]> = Vec::new();
            for [x, q] in hist::ecdf(&values) {
                if x < x_lo || x > x_hi {
                    continue;
                }
                if let Some(&[_, prev]) = pts.last() {
                    pts.push([x, prev]);
                }
                pts.push([x, q * 100.0]);
            }
            plot.y_grid_spacer(|g| theme::even_grid(g, 30.0, &[]))
                .y_axis_formatter(|m, _| {
                    if m.value < -1e-9 || m.value > 100.0 + 1e-9 {
                        String::new()
                    } else {
                        format!("{}%", m.value.abs())
                    }
                })
                .default_y_bounds(0.0, 100.0)
                .show(ui, |p| {
                    p.line(
                        Line::new("share at or below", pts)
                            .color(color)
                            .width(1.8_f32),
                    );
                });
            return;
        }
        // Probability paper: the share at or below each value on a scale of
        // standard deviations, labelled in percent.
        let pts: Vec<[f64; 2]> = hist::probability_plot(&values)
            .into_iter()
            .filter(|p| p[0] >= x_lo && p[0] <= x_hi)
            .collect();
        let z_top = hist::normal_quantile(1.0 - 0.5 / values.len().max(2) as f64).min(3.3) + 0.25;
        let line = d.normal_line.map(|l| {
            vec![
                [l.centre - l.spread * z_top, -z_top],
                [l.centre + l.spread * z_top, z_top],
            ]
        });
        plot.y_grid_spacer(move |g| {
            // From 50% outwards, leaving out a share whose label would crowd
            // the one before it.
            let px = 8.0 / g.base_step_size;
            let mut kept: Vec<f64> = Vec::new();
            for &(pc, _) in &PROBABILITY_MARKS[5..] {
                let z = hist::normal_quantile(pc / 100.0);
                if z <= z_top && kept.last().is_none_or(|&k| (z - k) * px >= 16.0) {
                    kept.push(z);
                }
            }
            kept.iter()
                .flat_map(|&z| if z == 0.0 { vec![z] } else { vec![z, -z] })
                .map(|value| GridMark {
                    value,
                    step_size: 1.0,
                })
                .collect()
        })
        .y_axis_formatter(|m, _| {
            PROBABILITY_MARKS
                .iter()
                .find(|&&(pc, _)| (hist::normal_quantile(pc / 100.0) - m.value).abs() < 1e-6)
                .map_or(String::new(), |&(_, label)| label.to_string())
        })
        .default_y_bounds(-z_top, z_top)
        .show(ui, |p| {
            if let Some(l) = line {
                p.line(
                    Line::new("one steady state", l)
                        .color(pal.text_tertiary)
                        .style(LineStyle::dashed_loose())
                        .width(1.0_f32),
                );
            }
            p.points(
                Points::new("values", pts)
                    .color(color)
                    .radius(1.6_f32)
                    .filled(true),
            );
        });
    }
}
