//! The Charts pane: a chart for each reading shown, sharing the pane's time axis.

use super::*;

impl TimegrapherApp {
    /// The Charts pane: a chart for each reading shown, one above the
    /// other, sharing the pane's height and its time axis.
    pub(super) fn charts_pane(&mut self, ui: &mut egui::Ui) {
        let shown = self.chart_series;
        self.note_need(ui, Pane::Charts, STACKED_MIN_H);
        self.charts_drawn = stacked(ui, shown, STACKED_MIN_H, |ui, k| self.chart(ui, k));
    }

    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn chart(&mut self, ui: &mut egui::Ui, reading: SeriesKind) {
        let pal = theme::pal(ui);
        let weak = pal.text_tertiary;
        let average = self.average_s;
        let trend = &self.trend;
        let pick = |f: fn(&TrendPoint) -> Option<f64>| -> Vec<[f64; 2]> {
            trend
                .iter()
                .filter_map(|p| f(p).map(|v| [p.t, v]))
                .collect()
        };
        // Each line in its reading's colour, as on the strip and in the
        // distributions. Tick's amplitude dashes violet over blue and Tock's
        // violet over deep orange, the key drawn the same; beat error from the
        // drop is a fainter dashed amber.
        let solid = LineStyle::Solid;
        let (series, unit, decimals): (Vec<ChartLine>, &'static str, usize) = match reading {
            SeriesKind::Rate => (
                vec![ChartLine {
                    name: format!(
                        "Rate in seconds per day, {} average",
                        fields::duration(average)
                    ),
                    pts: pick(|p| p.rate),
                    color: pal.trace_rate,
                    style: solid,
                    key: pal.trace_rate,
                    under: None,
                    main: true,
                }],
                " s/day",
                1,
            ),
            SeriesKind::Amplitude => (
                vec![
                    ChartLine {
                        name: "Average Amplitude".into(),
                        pts: pick(|p| p.amplitude),
                        color: pal.trace_amplitude,
                        style: solid,
                        key: pal.trace_amplitude,
                        under: None,
                        main: true,
                    },
                    ChartLine {
                        name: "Amplitude from Tick".into(),
                        pts: pick(|p| p.amplitude_a),
                        color: pal.trace_amplitude,
                        style: LineStyle::Dashed { length: 6.0 },
                        key: pal.tick,
                        under: Some(pal.tick),
                        main: false,
                    },
                    ChartLine {
                        name: "Amplitude from Tock".into(),
                        pts: pick(|p| p.amplitude_b),
                        color: pal.trace_amplitude,
                        style: LineStyle::Dashed { length: 6.0 },
                        key: pal.tock,
                        under: Some(pal.tock),
                        main: false,
                    },
                ],
                "°",
                0,
            ),
            SeriesKind::BeatError => (
                vec![
                    ChartLine {
                        name: "From the Unlock".into(),
                        pts: pick(|p| p.beat_error_unlock.map(f64::abs)),
                        color: pal.trace_beat_error,
                        style: solid,
                        key: pal.trace_beat_error,
                        under: None,
                        main: true,
                    },
                    ChartLine {
                        name: "From the Drop".into(),
                        pts: pick(|p| p.beat_error_drop.map(f64::abs)),
                        color: pal.trace_beat_error.gamma_multiply(0.6),
                        style: LineStyle::dashed_dense(),
                        key: pal.trace_beat_error.gamma_multiply(0.6),
                        under: None,
                        main: false,
                    },
                ],
                " ms",
                2,
            ),
        };
        let mut plot = Plot::new(steady::title(reading))
            // As short as its share of the pane, rather than spilling out.
            .min_size(Vec2::new(64.0, 24.0))
            .link_axis("trend", [true, false])
            .link_cursor("trend", [true, false])
            .allow_scroll(false)
            .allow_zoom([false, true])
            // The wheel zooms and a drag pans; the axes stay put.
            .allow_axis_zoom_drag(false)
            .allow_boxed_zoom(false)
            .x_grid_spacer(|g| theme::even_grid(g, 80.0, &theme::TIME_STEPS))
            .custom_x_axes(vec![egui_plot::AxisHints::new_x()
                .formatter(|m, r| chart_time_label(m.value, m.step_size, r))])
            // Short charts still get a label at the top and the bottom:
            // the labels are painted below, in small text, so they fit
            // closer than the plot's own.
            .y_grid_spacer(theme::short_y_grid)
            .y_axis_min_width(GUTTER)
            .y_axis_formatter(|_, _| String::new())
            // The values at the pointer are shown for every line at once
            // (below), instead of only when the pointer is on a line.
            .show_x(false)
            .show_y(false);
        // Scale to hold every point of the lines: the whole take.
        let all: Vec<[f64; 2]> = series.iter().flat_map(|s| s.pts.iter().copied()).collect();
        let y_range = data_range(&all);
        if let Some((lo, hi)) = y_range {
            plot = plot.default_y_bounds(lo, hi);
        }
        let marker = self.strip_end.is_some().then(|| self.strip_end_s());
        let lookup = series.clone();
        let colour = series.first().map_or(pal.text_secondary, |l| l.key);
        let to_live = self.charts_to_live_now;
        // A new averaging time spreads the values wider or narrower, so the
        // scale follows it, keeping the time shown.
        let rescale = self
            .chart_average_drawn
            .insert(steady::title(reading), average)
            .is_some_and(|was| was != average);
        let resp = plot.show(ui, |p| {
            if to_live {
                p.set_auto_bounds([true, true]);
            }
            if let (Some((lo, hi)), true) = (y_range, rescale) {
                p.set_plot_bounds_y(lo..=hi);
            }
            if p.response().hovered() {
                let s = p.ctx().input(|i| i.smooth_scroll_delta);
                let wheel = s.x + s.y;
                if wheel != 0.0 {
                    p.zoom_bounds_around_hovered(Vec2::new((wheel * 0.003).exp(), 1.0));
                }
            }
            for l in series {
                let w = if l.main { 1.8_f32 } else { 1.2_f32 };
                // Broken where there is no reading (a masked stretch, a
                // pause, beats not found), not drawn straight across.
                for seg in segments(&l.pts) {
                    if let Some(c) = l.under {
                        p.line(
                            Line::new(format!("{} gaps", l.name), seg.clone())
                                .color(c)
                                .width(w),
                        );
                    }
                    p.line(
                        Line::new(l.name.clone(), seg)
                            .color(l.color)
                            .style(l.style)
                            .width(w),
                    );
                }
            }
            let (hovered, clicked) = {
                let r = p.response();
                (r.hovered(), r.clicked() && !r.double_clicked())
            };
            let hover = p.pointer_coordinate().filter(|_| hovered);
            let click = clicked.then(|| p.pointer_coordinate()).flatten();
            // Panned or zoomed away from the whole session.
            let moved = !to_live && !p.auto_bounds().x;
            (click, hover.map(|h| h.x), moved)
        });
        let (click, hover, moved) = resp.inner;
        // The moment shown and the pointer's time, painted over the plot
        // rather than added to it: a plot line counts towards the automatic
        // bounds, so one under the pointer near the edge widened the chart
        // a little more every frame.
        let frame = *resp.transform.frame();
        let painter = ui.painter_at(frame);
        let t = resp.transform;
        paint_y_labels(ui, &t, unit);
        // Which reading this chart is, in its colour, at its top left.
        painter.text(
            frame.left_top() + Vec2::new(6.0, 3.0),
            egui::Align2::LEFT_TOP,
            steady::title(reading),
            egui::TextStyle::Small.resolve(ui.style()),
            colour,
        );
        // Its lines' key at its top right, beside the lines it names.
        let only = steady::SERIES.map(|k| k == reading);
        paint_key(&painter, frame, &chart_key(only, pal), ui, pal);
        let at = |x| egui::pos2(t.position_from_point_x(x), frame.top());
        let test = self.in_test();
        if test {
            strip::paint_phases(
                &painter,
                frame,
                &self.strip_settles(),
                &self.guided_measured,
                at,
                true,
                ui.visuals(),
            );
        }
        strip::paint_bands(
            &painter,
            frame,
            &self.position_bands(),
            at,
            true,
            true,
            !test,
            ui.visuals(),
        );
        if hover.is_some() {
            self.cursor_next = hover;
        }
        let cursor = hover.or(self.cursor_t);
        for x in [marker, cursor].into_iter().flatten() {
            let px = resp.transform.position_from_point_x(x);
            if px >= frame.left() && px <= frame.right() {
                painter.vline(px, frame.y_range(), egui::Stroke::new(1.0_f32, weak));
            }
        }
        let running = self.running();
        if moved {
            // Say how to get back, on the chart itself.
            let at = resp.response.rect.left_top() + Vec2::new(60.0, 4.0);
            let label = if running {
                "Follow Live"
            } else {
                "Show Whole Session"
            };
            let b = ui.put(
                egui::Rect::from_min_size(at, Vec2::new(150.0, 24.0)),
                theme::primary(ui, label).min_size(Vec2::new(0.0, 24.0)),
            );
            if b.on_hover_text(if running {
                "Back to the whole session, following new beats as they come in. \
                 Double-clicking a chart does the same."
            } else {
                "Back to the whole session. Double-clicking a chart does the same."
            })
            .clicked()
            {
                self.charts_to_live = true;
                ui.ctx().request_repaint();
            }
        }
        if let Some(x) = hover {
            resp.response.on_hover_ui_at_pointer(|ui| {
                ui.set_max_width(300.0);
                ui.label(RichText::new(length_text(x)).strong());
                for ChartLine { name, pts, key, .. } in &lookup {
                    let v = value_at(pts, x);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("■").color(*key));
                        ui.label(match v {
                            Some(v) => format!("{name}: {v:.decimals$}{unit}"),
                            None => format!("{name}: none"),
                        });
                    });
                }
                ui.label(
                    RichText::new(if running {
                        "Wheel zooms time, Ctrl+wheel the scale, drag pans, \
                         click shows that moment on the strip, \
                         double-click follows live again."
                    } else {
                        "Wheel zooms time, Ctrl+wheel the scale, drag pans, \
                         click shows that moment on the strip, \
                         double-click shows the whole session."
                    })
                    .small()
                    .weak(),
                );
            });
        }
        if let Some(at) = click {
            if at.x > 0.0 {
                self.strip_fit = false;
                self.strip_end = Some(at.x);
            }
        }
    }
}
