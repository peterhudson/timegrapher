//! The Charts pane: a chart for each reading shown, sharing the pane's time axis.

use super::*;
use crate::strength::{self, Grid, Strength};
use timegrapher_core::figure_text::{base_digits, figure, true_minus};

impl TimegrapherApp {
    /// The Charts pane: a chart for each reading shown, one above the
    /// other, sharing the pane's height and its time axis.
    pub(super) fn charts_pane(&mut self, ui: &mut egui::Ui) {
        let shown = self.chart_series;
        self.note_need(ui, Pane::Charts, STACKED_MIN_H);
        let (mut moved, mut over, mut newest) = (false, None, false);
        self.charts_drawn = stacked(ui, shown, (STACKED_MIN_H, false), |ui, k, _| {
            let (m, hover, at_newest) = self.chart(ui, k);
            moved |= m;
            over = over.or(hover);
            newest |= at_newest;
        });
        self.charts_fit = !moved;
        // Their lines end a reading apart, so one at its newest is enough.
        self.charts_follow = newest;
        // Every chart's lane reads the pointer's time, those above the
        // one under it a frame late.
        if over != self.charts_over {
            self.charts_over = over;
            ui.ctx().request_repaint();
        }
    }

    /// The pane's one way back to the whole session, in its toolbar as
    /// the Paper Strip's Fit is: on while the charts show it all, turned
    /// off by a pan or a zoom (Walter's walk of #1057: a button on every
    /// chart wrapped over its title and lines, #1063).
    pub(super) fn charts_fit_switch(&mut self, ui: &mut egui::Ui) {
        let mut fit = self.charts_fit;
        let hint = if self.running() {
            "Show the whole session on the charts and keep following new readings. Scrolling, \
             dragging or zooming a chart's time turns it off; double-clicking a chart turns it \
             on."
        } else {
            "Show the whole session on the charts. Scrolling, dragging or zooming a chart's \
             time turns it off; double-clicking a chart turns it on."
        };
        let pal = theme::pal(ui);
        if theme::tool_switch(ui, Icon::Fit, pal.accent, "Fit", &mut fit, hint) && fit {
            self.charts_to_live = true;
            ui.ctx().request_repaint();
        }
    }

    /// One reading's chart: whether it was moved off the whole session,
    /// the time under the pointer on it, and whether, so moved, it ends at
    /// its newest reading.
    fn chart(&mut self, ui: &mut egui::Ui, reading: SeriesKind) -> (bool, Option<f64>, bool) {
        let pal = theme::pal(ui);
        // A lane over the plot for the chart's name and its lines' key,
        // so neither sits on the lines (Walter's walk of #1057, #1063).
        let font = egui::TextStyle::Small.resolve(ui.style());
        let (lane, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), font.size + 6.0),
            egui::Sense::hover(),
        );
        let average = self.average_s;
        let (series, unit, decimals) = chart_lines(reading, &self.trend, average, pal);
        let (mut plot, grid) = chart_plot(reading, theme::time_gutter(ui.ctx()));
        // One time axis for every chart in the pane: the take's, from its
        // start to its end or now, not each chart's own first and last
        // reading, so their times line up (Hugh, #1252).
        let take = self.take_span();
        if let Some((t0, t1)) = take {
            plot = plot.include_x(t0).include_x(t1);
        }
        // Ending where the strip above does, left of its scales, so their
        // times line up.
        if let Some(w) = theme::time_width(ui.ctx(), ui.cursor().left()) {
            plot = plot.width(w.clamp(64.0, ui.available_width()));
        }
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
        let follow = self.charts_follow;
        let resp = plot.show(ui, |p| {
            grid.draw(p, pal);
            draw_chart(p, series, take, (to_live, follow), (y_range, rescale))
        });
        let (click, hover, moved, at_newest) = resp.inner;
        // The moment shown and the pointer's time, painted over the plot
        // rather than added to it: a plot line counts towards the automatic
        // bounds, so one under the pointer near the edge widened the chart
        // a little more every frame.
        let frame = *resp.transform.frame();
        let painter = ui.painter_at(frame);
        let t = resp.transform;
        paint_y_labels(ui, &t, unit);
        // The lane above the plot: which reading this is, each line's
        // value at the pointer's time over any chart, and its key.
        let lane = egui::Rect::from_min_max(
            egui::pos2(frame.left(), lane.top()),
            egui::pos2(frame.right(), lane.bottom()),
        );
        let at = hover.or(self.charts_over);
        let values = at.map_or_else(Vec::new, |x| {
            lane_values(&lookup, (x, &self.trend), (reading, unit, decimals), pal)
        });
        let only = steady::SERIES.map(|k| k == reading);
        paint_lane(
            ui,
            lane,
            (steady::title(reading), colour, font),
            (hover.map(length_text), &values),
            &chart_key(only, pal),
        );
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
        // The moment shown as a call-out, the pointer's time at axis
        // strength on every chart (Hugh, #1063).
        let lines = [(marker, Strength::CallOut), (cursor, Strength::Axis)];
        for (x, strength) in lines {
            if let Some(x) = x {
                strength::time_line(&painter, &resp.transform, x, strength, pal);
            }
        }
        if let Some(at) = click {
            if at.x > 0.0 {
                self.strip_fit = false;
                self.strip_end = Some(at.x);
            }
        }
        (moved, hover, at_newest)
    }
}

/// A click on a chart, the time under the pointer, whether it was moved
/// off the whole session, and whether, so moved, it ends at the newest
/// reading.
type ChartInput = (Option<egui_plot::PlotPoint>, Option<f64>, bool, bool);

/// The lines of one reading's chart over the session, each in its reading's
/// colour, with the unit and decimals its values show with.
fn chart_lines(
    reading: SeriesKind,
    trend: &[TrendPoint],
    average: f64,
    pal: &theme::Palette,
) -> (Vec<ChartLine>, &'static str, usize) {
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
    match reading {
        SeriesKind::Rate => (
            vec![ChartLine {
                name: format!(
                    "Rate in seconds per day, {} average",
                    fields::duration(average)
                ),
                pts: pick(|p| p.rate_s_per_day),
                se: |p| p.rate_error_s_per_day,
                color: pal.trace_rate,
                style: solid,
                key: pal.trace_rate,
                under: None,
                main: true,
            }],
            " s/d",
            base_digits::RATE,
        ),
        SeriesKind::Amplitude => (
            vec![
                ChartLine {
                    name: "Average Amplitude".into(),
                    pts: pick(|p| p.amplitude_deg),
                    se: |p| p.amplitude_error_deg,
                    color: pal.trace_amplitude,
                    style: solid,
                    key: pal.trace_amplitude,
                    under: None,
                    main: true,
                },
                ChartLine {
                    name: "Amplitude from Tick".into(),
                    pts: pick(|p| p.amplitude_even_deg),
                    se: |p| p.amplitude_error_deg,
                    color: pal.trace_amplitude,
                    style: LineStyle::Dashed { length: 6.0 },
                    key: pal.tick,
                    under: Some(pal.tick),
                    main: false,
                },
                ChartLine {
                    name: "Amplitude from Tock".into(),
                    pts: pick(|p| p.amplitude_odd_deg),
                    se: |p| p.amplitude_error_deg,
                    color: pal.trace_amplitude,
                    style: LineStyle::Dashed { length: 6.0 },
                    key: pal.tock,
                    under: Some(pal.tock),
                    main: false,
                },
            ],
            "°",
            base_digits::AMPLITUDE,
        ),
        SeriesKind::BeatError => (
            vec![
                ChartLine {
                    name: "From the Unlock".into(),
                    pts: pick(|p| p.beat_error_unlock_ms.map(f64::abs)),
                    se: |p| p.beat_error_unlock_error_ms,
                    color: pal.trace_beat_error,
                    style: solid,
                    key: pal.trace_beat_error,
                    under: None,
                    main: true,
                },
                ChartLine {
                    name: "From the Drop".into(),
                    pts: pick(|p| p.beat_error_ms.map(f64::abs)),
                    se: |_| None,
                    color: pal.trace_beat_error.gamma_multiply(0.6),
                    style: LineStyle::dashed_dense(),
                    key: pal.trace_beat_error.gamma_multiply(0.6),
                    under: None,
                    main: false,
                },
            ],
            " ms",
            base_digits::BEAT_ERROR,
        ),
    }
}

/// A reading's chart, linked in time to the others: the wheel or a drag
/// pans time, Cmd or Ctrl with the wheel or a pinch zooms it, and the
/// scale fits the lines on its own. Its grid, with a zero line on the
/// rate's (0 s/d).
fn chart_plot(reading: SeriesKind, gutter: f32) -> (Plot<'static>, Grid) {
    // Short charts still get a label at the top and the bottom: the
    // labels are painted below, in small text, so they fit closer than
    // the plot's own.
    let mut grid = Grid::new(
        |g| theme::even_grid(g, 80.0, &theme::TIME_STEPS),
        theme::short_y_grid,
    );
    if reading == SeriesKind::Rate {
        grid = grid.zero_y();
    }
    let plot = grid
        .on(Plot::new(steady::title(reading))
            // As short as its share of the pane, rather than spilling out.
            .min_size(Vec2::new(64.0, 24.0))
            // No room either side of the take's time: its start sits where
            // the strip's does, at the gutter's edge (Hugh, #1252).
            .set_margin_fraction(Vec2::new(0.0, 0.05))
            .link_axis("trend", [true, false])
            .link_cursor("trend", [true, false])
            .allow_scroll(false)
            // The wheel and a drag pan time and Cmd/Ctrl with the wheel
            // zooms it (below), kept to the take. The scale fits the lines
            // on its own, so it neither zooms nor pans: a zoom would only
            // be put back.
            .allow_zoom(false)
            .allow_drag(false)
            .allow_axis_zoom_drag(false)
            .allow_boxed_zoom(false))
        .custom_x_axes(vec![egui_plot::AxisHints::new_x()
            .formatter(|m, r| chart_time_label(m.value, m.step_size, r))])
        .y_axis_min_width(gutter)
        .y_axis_formatter(|_, _| String::new())
        // The values at the pointer are shown for every line at once
        // (below), instead of only when the pointer is on a line.
        .show_x(false)
        .show_y(false);
    (plot, grid)
}

/// Draw a chart's lines, broken where there is no reading, and return a
/// click, the time under the pointer and whether the chart was moved off
/// the whole session.
fn draw_chart(
    p: &mut egui_plot::PlotUi<'_>,
    series: Vec<ChartLine>,
    take: Option<(f64, f64)>,
    (to_live, follow): (bool, bool),
    (y_range, rescale): (Option<(f64, f64)>, bool),
) -> ChartInput {
    if to_live {
        p.set_auto_bounds([true, true]);
    }
    if let (Some((lo, hi)), true) = (y_range, rescale) {
        p.set_plot_bounds_y(lo..=hi);
    }
    let panned = pan_and_zoom(p, take);
    // Panned to the newest reading, a chart follows new ones at the
    // length it shows, as the strip does; Fit is the whole session (Hugh,
    // #1070).
    let b = p.plot_bounds();
    let (lo, hi) = (b.min()[0], b.max()[0]);
    let mut at_newest = false;
    if let (Some((_, newest)), false) = (take, to_live || p.auto_bounds().x) {
        if follow && !panned && hi < newest {
            p.set_plot_bounds_x(newest - (hi - lo)..=newest);
        }
        // Judged from the bounds a pan has settled into, the frame after.
        at_newest = !panned && (follow || hi >= newest);
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
    (click, hover.map(|h| h.x), moved, moved && at_newest)
}

/// Pan and zoom a chart's time by hand, kept to the take. The wheel, a
/// swipe either way or a drag pans; Cmd (Mac) or Ctrl with the wheel, or
/// a pinch, zooms about the pointer. Alt and the wheel zooms the strip's
/// width; the charts' scale fits their lines, so here it does nothing.
/// Zoomed out to the whole take, the chart shows the whole session again.
/// Whether it moved the chart.
fn pan_and_zoom(p: &mut egui_plot::PlotUi<'_>, take: Option<(f64, f64)>) -> bool {
    let r = p.response();
    let dragged = r.dragged_by(egui::PointerButton::Primary);
    if !r.hovered() && !dragged {
        return false;
    }
    let drag = if dragged { r.drag_delta().x } else { 0.0 };
    let px = r.rect.width().max(1.0);
    if dragged {
        p.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let (s, zoom, alt) = p
        .ctx()
        .input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.alt));
    let shift = drag + if alt { 0.0 } else { s.x + s.y };
    if shift == 0.0 && zoom == 1.0 {
        return false;
    }
    let b = p.plot_bounds();
    let (mut lo, mut hi) = (b.min()[0], b.max()[0]);
    if zoom != 1.0 {
        let at = p.pointer_coordinate().map_or((lo + hi) / 2.0, |c| c.x);
        lo = at + (lo - at) / f64::from(zoom);
        hi = at + (hi - at) / f64::from(zoom);
    }
    let dt = -f64::from(shift / px) * (hi - lo);
    let Some(take) = take else {
        return false;
    };
    match keep_in(lo + dt, hi + dt, take) {
        // Held at an end, a turn further moves nothing.
        Some(to) if to == (b.min()[0], b.max()[0]) => false,
        Some((lo, hi)) => {
            p.set_plot_bounds_x(lo..=hi);
            true
        }
        None => {
            let y = p.auto_bounds().y;
            p.set_auto_bounds([true, y]);
            true
        }
    }
}

/// The time range `lo..hi` moved, not stretched, to lie within the take
/// `t0..t1`, or None when it holds the whole take: the whole session.
fn keep_in(lo: f64, hi: f64, (t0, t1): (f64, f64)) -> Option<(f64, f64)> {
    let w = hi - lo;
    if w >= t1 - t0 {
        None
    } else if lo < t0 {
        Some((t0, t0 + w))
    } else if hi > t1 {
        Some((t1 - w, t1))
    } else {
        Some((lo, hi))
    }
}

/// A line's value at `x` as the values box writes it: the point there at
/// the figure's place (`base`), as its card showed it (#1063, #1470).
pub(super) fn line_value(
    pts: &[[f64; 2]],
    se: fn(&TrendPoint) -> Option<f64>,
    (x, trend): (f64, &[TrendPoint]),
    base: usize,
    signed: bool,
) -> Option<String> {
    let [t, v] = pts[nearest_at(pts, x)?];
    let at = trend.partition_point(|p| p.t < t);
    let se = trend.get(at).and_then(se);
    Some(true_minus(&figure(v, se, base, signed)))
}

/// Each of `lines`' values at `x` for a chart's lane, written as its
/// card writes the reading, in the line's colour.
fn lane_values(
    lines: &[ChartLine],
    (x, trend): (f64, &[TrendPoint]),
    (reading, unit, base): (SeriesKind, &str, usize),
    pal: &theme::Palette,
) -> Vec<LaneValue> {
    let signed = reading == SeriesKind::Rate;
    lines
        .iter()
        .filter_map(|l| {
            let v = line_value(&l.pts, l.se, (x, trend), base, signed)?;
            Some((format!("{v}{unit}"), l.color, l.under, pal.axis_ink(l.key)))
        })
        .collect()
}

/// A line's value in a chart's lane: its text, the line's colour and the
/// colour under its dashes for its swatch, and the colour to write it in.
type LaneValue = (String, Color32, Option<Color32>, Color32);

/// A chart's lane, above its plot (Hugh, #1063): the reading's name at
/// the left in its colour; then, while the pointer is over a chart, the
/// time (on the chart under it only) and each line's value there beside
/// its swatch; then the key at the right, left out before any value is
/// where the lane is too narrow for both. Each value has a slot as wide
/// as its digits as zeros, so the values beside it don't jitter.
fn paint_lane(
    ui: &egui::Ui,
    lane: egui::Rect,
    (title, colour, font): (&str, Color32, egui::FontId),
    (time, values): (Option<String>, &[LaneValue]),
    key: &[(&str, Color32, Option<Color32>)],
) {
    let pal = theme::pal(ui);
    let p = ui.painter();
    let (top, y) = (lane.top() + 3.0, lane.top() + 3.0 + font.size / 2.0);
    let mut x = lane.left();
    let write = |x: &mut f32, text: String, ink: Color32, slot: bool| {
        *x += lane_text(p, (*x, top), &text, (&font, ink), slot);
    };
    write(&mut x, title.into(), colour, false);
    x += LANE_AFTER_TITLE;
    if let Some(t) = time {
        write(&mut x, t, pal.text, true);
        x += LANE_GAP;
    }
    for (text, color, under, ink) in values {
        x += paint_swatch(p, egui::pos2(x, y), *color, *under) + 4.0;
        write(&mut x, text.clone(), *ink, true);
        x += LANE_GAP;
    }
    paint_key(p, lane, key, (ui, x), pal);
}

/// The gap after a lane's title, before its time and values.
pub const LANE_AFTER_TITLE: f32 = 16.0;
/// The gap after each of a lane's time and values.
pub const LANE_GAP: f32 = 12.0;

/// How wide a lane's `text` is: when `slot`, at least as wide as its
/// digits written as zeros, so a figure beside it doesn't jitter as the
/// digits change under the pointer (#1063).
pub fn lane_width(p: &egui::Painter, text: &str, font: &egui::FontId, slot: bool) -> f32 {
    let w = |t: String| p.layout_no_wrap(t, font.clone(), Color32::WHITE).size().x;
    let own = w(text.to_string());
    if slot {
        let zeros: String = text
            .chars()
            .map(|c| if c.is_ascii_digit() { '0' } else { c })
            .collect();
        own.max(w(zeros))
    } else {
        own
    }
}

/// A lane's `text` with its top left at `at`, in `ink`: the width it
/// takes ([`lane_width`]).
pub fn lane_text(
    p: &egui::Painter,
    at: (f32, f32),
    text: &str,
    (font, ink): (&egui::FontId, Color32),
    slot: bool,
) -> f32 {
    let g = p.layout_no_wrap(text.to_string(), font.clone(), ink);
    p.galley(egui::pos2(at.0, at.1), g, ink);
    lane_width(p, text, font, slot)
}

/// The names in a chart's key and their colours, when it has more than one
/// line.
/// A second colour is drawn as a dash of the two, as the line is.
pub(super) fn chart_key(
    shown: [bool; 3],
    pal: &theme::Palette,
) -> Vec<(&'static str, Color32, Option<Color32>)> {
    let mut key = Vec::new();
    if shown[1] {
        key.extend([
            ("Average Amplitude", pal.trace_amplitude, None),
            ("From Tick", pal.trace_amplitude, Some(pal.tick)),
            ("From Tock", pal.trace_amplitude, Some(pal.tock)),
        ]);
    }
    if shown[2] {
        key.extend([
            ("From the Unlock", pal.trace_beat_error, None),
            (
                "From the Drop",
                pal.trace_beat_error.gamma_multiply(0.6),
                None,
            ),
        ]);
    }
    key
}

/// A chart's key, right to left from its top right corner: each name
/// after its line's [`paint_swatch`]. Left out where it would reach left
/// of `clear_of`, so what is painted there stays whole (#1063); whether
/// it was painted.
pub fn paint_key(
    painter: &egui::Painter,
    frame: egui::Rect,
    key: &[(&str, Color32, Option<Color32>)],
    (ui, clear_of): (&egui::Ui, f32),
    pal: &theme::Palette,
) -> bool {
    if key.is_empty() {
        return true;
    }
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galleys: Vec<_> = key
        .iter()
        .map(|&(name, _, _)| painter.layout_no_wrap(name.into(), font.clone(), pal.text_secondary))
        .collect();
    let (gap, space) = (5.0, 12.0);
    let width: f32 = key
        .iter()
        .zip(&galleys)
        .map(|(&(_, _, under), g)| swatch_width(under) + gap + g.size().x)
        .sum::<f32>()
        + space * (key.len() as f32 - 1.0);
    let h = galleys.iter().map(|g| g.size().y).fold(0.0, f32::max);
    let right = frame.right() - 6.0;
    let left = right - width - 4.0;
    if left < clear_of {
        return false;
    }
    let top = frame.top() + 3.0;
    let y = top + h / 2.0;
    // On a backing of the card's colour, so the lines under it don't
    // muddle it.
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(left, top - 1.0),
            egui::pos2(right + 2.0, top + h + 1.0),
        ),
        3.0,
        pal.card,
    );
    let mut x = right - width;
    for (&(_, color, under), g) in key.iter().zip(galleys) {
        x += paint_swatch(painter, egui::pos2(x, y), color, under) + gap;
        let w = g.size().x;
        painter.galley(egui::pos2(x, top), g, pal.text_secondary);
        x += w + space;
    }
    true
}

/// How wide a line's [`paint_swatch`] is.
fn swatch_width(under: Option<Color32>) -> f32 {
    if under.is_some() {
        14.0
    } else {
        6.0
    }
}

/// A line's mark from its left middle `at`, as its key and its values
/// show it: a dot, or a short dash for a line drawn dashed in `color`
/// over `under`. Its width.
fn paint_swatch(
    painter: &egui::Painter,
    at: egui::Pos2,
    color: Color32,
    under: Option<Color32>,
) -> f32 {
    let (x, y) = (at.x, at.y);
    match under {
        Some(u) => {
            painter.hline(x..=x + 14.0, y, egui::Stroke::new(2.0_f32, u));
            for i in 0..3 {
                let a = x + i as f32 * 5.0;
                painter.hline(a..=a + 3.0, y, egui::Stroke::new(2.0_f32, color));
            }
        }
        None => {
            painter.circle_filled(egui::pos2(x + 3.0, y), 3.0, color);
        }
    }
    swatch_width(under)
}
