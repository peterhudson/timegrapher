//! A test pane's charts over hours, one per measure, stacked on one hour
//! axis with one hover line through them all (#1729, Hugh's layout,
//! Peter's approval of 14 Nov). Each hour's mean is a level step across
//! its own span, its ± a band where it has one, and a line breaks at
//! every hour not shown (Norm's ruling on #1729). An hour grey in the
//! pane's table is grey here too. Each chart's title row gives its value
//! at the hovered hour, in its colour, and the unit sits over its axis,
//! the ticks bare numbers. Only the bottom chart names the hours, and
//! under the hovered one says which it is.

use crate::steady::series_colour;
use crate::strength::{self, Grid};
use crate::theme;
use eframe::egui;
use timegrapher_core::chart_scale::axis_number;
use timegrapher_core::steadiness::SeriesKind;

/// One hour's figure as drawn: its value, its words for the hover (its
/// figure with its ± and unit, from core), its band, and whether it is
/// grey, as its table cell is.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub v: f64,
    pub hover: String,
    /// Its ± band, low and high, from core (a beat error's floored at 0).
    pub band: Option<(f64, f64)>,
    pub grey: bool,
}

/// One chart: its measure, a step for each hour (none where the hour's
/// figure isn't shown), and how its ticks are written.
pub struct Chart {
    pub kind: SeriesKind,
    pub steps: Vec<Option<Step>>,
    pub unit: &'static str,
    /// A sign written on its ticks: a rate.
    pub signed: bool,
    /// Its value axis, [lo, hi, step], core's for the run
    /// ([`timegrapher_core::power_reserve::chart_axis`]): its ticks fall
    /// on each step, and zero, where it lies inside, is drawn across.
    pub axis: Option<[f64; 3]>,
}

/// The hours' spans, `[from, to)` in hours since the run began, one for
/// each step of every chart; the last may be a part hour.
pub type Spans = [(f64, f64)];

/// The height of each chart's title row, points.
const TITLE_H: f32 = 18.0;

/// The height under the bottom chart for its hours, points.
const AXIS_H: f32 = 20.0;

/// The height of the line over each chart's value axis that names its
/// unit, points.
const UNIT_H: f32 = 14.0;

/// The gap between one chart's plot and the next one's title row, clear
/// of the lowest tick's label, points.
const CHART_GAP: f32 = 10.0;

/// The least height a chart takes, its title row and unit line included,
/// points (Hugh on #1761: at least ~90 pt each), with half a tick label
/// more so the top tick's label clears the unit (Walter on #1761) and the
/// plot keeps its height.
pub const CHART_READ_H: f32 = 98.0;

/// The steps, in hours, the hour axis goes up in.
const HOUR_STEPS: [f64; 7] = [1.0, 2.0, 3.0, 6.0, 12.0, 24.0, 48.0];

/// The least width of the charts' value axis, points.
const GUTTER: f32 = 48.0;

/// The hour axis's marks: none before the run, in the margin left of 0 h.
fn hour_marks(g: egui_plot::GridInput) -> Vec<egui_plot::GridMark> {
    // 80 points apart at least: egui_plot names hours only from 60 apart,
    // and fades them in up to 80.
    let mut marks = theme::even_grid(g, 80.0, &HOUR_STEPS);
    // Adding 0 turns the grid's -0 into 0, which is kept.
    marks.retain(|m| !(m.value + 0.0).is_sign_negative());
    marks
}

/// The height `n` charts need to read well, the hours under them
/// included.
pub fn read_h(n: usize) -> f32 {
    n as f32 * CHART_READ_H + n.saturating_sub(1) as f32 * CHART_GAP + AXIS_H
}

/// The hour of `spans` at `x` hours, if any.
fn hour_at(spans: &Spans, x: f64) -> Option<usize> {
    spans.iter().position(|&(a, b)| a <= x && x < b)
}

/// One width of value axis for every chart, so their plots start at the
/// same x: the least gutter, or the widest tick label any draws.
fn shared_gutter(ui: &egui::Ui, charts: &[&Chart]) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut w = GUTTER;
    for c in charts {
        for m in c
            .axis
            .map(|[lo, hi, step]| step_marks(lo, hi, step))
            .unwrap_or_default()
        {
            let t = tick_text(c, m.value, m.step_size);
            let wide = ui.fonts_mut(|f| {
                f.layout_no_wrap(t, font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            });
            w = w.max(wide + 8.0);
        }
    }
    w
}

/// A tick's label: bare, a rate's signed so fast or slow reads at a
/// glance (Hugh on #1761), a minus a true one.
fn tick_text(c: &Chart, v: f64, step: f64) -> String {
    tick_label(c.signed, v, step)
}

/// A value tick's label: bare, `signed` for a rate, a minus a true one;
/// the Positions Test's charts write theirs the same, by core's one rule
/// (#1779).
pub(crate) fn tick_label(signed: bool, v: f64, step: f64) -> String {
    timegrapher_core::chart_scale::axis_tick(v, step, signed)
}

/// A chart's value marks: core's axis's own step, or a rounder step above
/// it where that crowds the plot, labels at least the first of `px`
/// points apart, else the next: an hour chart's 24 down to 21 on a short
/// plot (egui_plot names no mark closer than 20), a position line's
/// along its bottom wider, for its longer labels.
pub(crate) fn value_marks(
    g: egui_plot::GridInput,
    axis: Option<[f64; 3]>,
    px: &[f64],
) -> Vec<egui_plot::GridMark> {
    let Some([_, _, step]) = axis else {
        return theme::short_y_grid(g);
    };
    // Core's step, then the round steps (1, 2 and 5 times a power of ten)
    // above it, each the round step at least twice the last, up to the
    // plot's span.
    let span = g.bounds.1 - g.bounds.0;
    let ladder: Vec<f64> = std::iter::successors(Some(step), |&s| {
        Some(timegrapher_core::chart_scale::round_step(s + s))
    })
    .take_while(|&s| s == step || s <= span)
    .collect();
    let at = |px: f64| {
        let again = egui_plot::GridInput {
            bounds: g.bounds,
            base_step_size: g.base_step_size,
        };
        theme::even_grid(again, px, &ladder)
    };
    px.iter()
        .map(|&p| at(p))
        .find(|m| m.len() >= 2)
        .unwrap_or_else(|| at(px[0]))
}

/// The marks of an axis from `lo` to `hi` at each `step`.
fn step_marks(lo: f64, hi: f64, step: f64) -> Vec<egui_plot::GridMark> {
    let n = ((hi - lo) / step).round() as i64;
    std::iter::successors(Some(0_i64), |i| Some(i + 1))
        .take_while(|&i| i <= n)
        .map(|i| egui_plot::GridMark {
            value: lo + i as f64 * step + 0.0,
            step_size: step,
        })
        .collect()
}

/// The charts in `rect`, stacked, sharing its height, over `spans`
/// (the hours of every chart's steps), the hour `hover` marked by one
/// line through them all, its value in each title row and `axis_words`
/// for it under the hours. Gives the hour under the pointer, if any.
pub fn stack(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    charts: &[&Chart],
    spans: &Spans,
    hover: Option<usize>,
    axis_words: impl Fn(usize) -> String,
) -> Option<usize> {
    if charts.is_empty() {
        return None;
    }
    let end = spans.iter().map(|s| s.1).fold(1.0, f64::max);
    // A 5% margin each side, as egui's own, so 0 h clears the value axis.
    let x_range = (-end * 0.05, end * 1.05);
    let gutter = shared_gutter(ui, charts);
    // Never shorter than reads: what doesn't fit runs past the rect's
    // foot, where it scrolls.
    let n = charts.len() as f32;
    let each = ((rect.height() - AXIS_H - (n - 1.0) * CHART_GAP) / n).max(CHART_READ_H);
    // The hour labels the hovered hour's words would cover are left out,
    // so no label shows half hidden behind them.
    let words = hover.map(&axis_words);
    let frame = egui::Rect::from_x_y_ranges(rect.left() + gutter..=rect.right(), rect.y_range());
    let hide = hover
        .zip(words.as_deref())
        .map(|(h, w)| covered_hours(ui, frame, x_range, spans[h], w, end));
    let mut under = None;
    let mut bottom_frame = None;
    for (i, c) in charts.iter().enumerate() {
        let last = i + 1 == charts.len();
        let top = rect.top() + i as f32 * (each + CHART_GAP);
        let band = egui::Rect::from_x_y_ranges(rect.x_range(), top..=top + TITLE_H);
        let unit =
            egui::Rect::from_x_y_ranges(rect.x_range(), band.bottom()..=band.bottom() + UNIT_H);
        // The top tick's label is centred on the plot's top edge: half a
        // label's height clear of the unit line (Walter on #1761).
        let clear = ui.text_style_height(&egui::TextStyle::Body) / 2.0;
        let plot_h = each - TITLE_H - UNIT_H - clear + if last { AXIS_H } else { 0.0 };
        let plot_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left(), unit.bottom() + clear),
            egui::vec2(rect.width(), plot_h),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(plot_rect));
        let (frame, at) = one(&mut child, c, spans, (x_range, gutter, last), (hover, hide));
        under = under.or(at);
        title_row(ui, band, frame, c, hover);
        unit_line(ui, unit, frame, c);
        if last {
            bottom_frame = Some(frame);
        }
    }
    if let (Some(h), Some(w), Some(frame)) = (hover, words.as_deref(), bottom_frame) {
        hover_words(ui, frame, x_range, spans[h], w);
    }
    under
}

/// One chart in `ui`: its bands, its steps, and the hover line; the hours
/// named under it when it is the `last`. Gives its plot's frame and the
/// hour under the pointer.
fn one(
    ui: &mut egui::Ui,
    c: &Chart,
    spans: &Spans,
    ((x_lo, x_hi), gutter, last): ((f64, f64), f32, bool),
    (hover, hide): (Option<usize>, Option<(f64, f64)>),
) -> (egui::Rect, Option<usize>) {
    use egui_plot::{Line, Plot, Polygon, VLine};
    let pal = theme::pal(ui);
    let colour = series_colour(c.kind, pal);
    let axis = c.axis;
    let grid = Grid::new(hour_marks, move |g| value_marks(g, axis, &[24.0, 21.0]));
    // Zero across where it lies inside the axis (Hugh on #1761).
    let grid = match c.axis {
        Some([lo, hi, _]) if (lo..=hi).contains(&0.0) => grid.zero_y(),
        _ => grid,
    };
    let mut plot = grid
        .on(Plot::new(("hour steps", c.kind.title())))
        .height(ui.available_height())
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .show_x(false)
        .show_y(false)
        .default_x_bounds(x_lo, x_hi)
        .y_axis_min_width(gutter)
        .show_axes([last, true])
        .x_axis_formatter(move |m, _| match hide {
            Some((a, b)) if (a..=b).contains(&m.value) => String::new(),
            _ => format!("{} h", axis_number(m.value, m.step_size)),
        })
        .y_axis_formatter(|m, _| tick_text(c, m.value, m.step_size));
    if let Some([lo, hi, _]) = c.axis {
        plot = plot.default_y_bounds(lo, hi);
    }
    let resp = plot.show(ui, |p| {
        grid.draw(p, pal);
        for (s, &(a, b)) in c.steps.iter().zip(spans) {
            let Some(Step {
                band: Some((lo, hi)),
                grey,
                ..
            }) = *s
            else {
                continue;
            };
            let ink = if grey { pal.text_tertiary } else { colour };
            p.polygon(
                Polygon::new("±", vec![[a, lo], [b, lo], [b, hi], [a, hi]])
                    .fill_color(ink.gamma_multiply(strength::BAND_FILL))
                    .stroke(egui::Stroke::NONE),
            );
        }
        for (pts, grey) in runs(&c.steps, spans) {
            let ink = if grey { pal.text_tertiary } else { colour };
            p.line(
                Line::new(c.kind.title(), pts)
                    .color(ink)
                    .width(strength::DATA_W),
            );
        }
        if let Some(&(a, b)) = hover.and_then(|h| spans.get(h)) {
            p.vline(
                VLine::new("hover", (a + b) / 2.0)
                    .color(pal.text_secondary)
                    .width(strength::FAINT_CALL_OUT_W),
            );
        }
    });
    let at = resp
        .response
        .hover_pos()
        .and_then(|pos| hour_at(spans, resp.transform.value_from_position(pos).x));
    (*resp.transform.frame(), at)
}

/// The steps as lines: one for each run of hours shown side by side with
/// the same ink, each hour level across its span and a rise where the next
/// begins; a run starts from the hour before's level where that hour is
/// shown, so the line is whole, and breaks where an hour isn't.
fn runs(steps: &[Option<Step>], spans: &Spans) -> Vec<(Vec<[f64; 2]>, bool)> {
    let mut out: Vec<(Vec<[f64; 2]>, bool)> = Vec::new();
    let mut prev: Option<(f64, f64)> = None;
    for (s, &(a, b)) in steps.iter().zip(spans) {
        let Some(s) = s else {
            prev = None;
            continue;
        };
        // Joined where the last hour ends as this one begins: a whole
        // hour's end is the next hour exactly.
        let joined = prev.filter(|&(_, end)| end >= a);
        match out.last_mut() {
            Some((pts, grey)) if joined.is_some() && *grey == s.grey => {
                pts.push([a, s.v]);
                pts.push([b, s.v]);
            }
            _ => {
                let mut pts = Vec::new();
                if let Some((v, _)) = joined {
                    pts.push([a, v]);
                }
                pts.push([a, s.v]);
                pts.push([b, s.v]);
                out.push((pts, s.grey));
            }
        }
        prev = Some((s.v, b));
    }
    out
}

/// A chart's title row in `band`: its name alone over the plot's left
/// edge, and its value at the hovered hour after it in its colour.
fn title_row(ui: &egui::Ui, band: egui::Rect, frame: egui::Rect, c: &Chart, hover: Option<usize>) {
    let pal = theme::pal(ui);
    let painter = ui.painter().with_clip_rect(band);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let y = band.center().y;
    let name = painter.text(
        egui::pos2(frame.left(), y),
        egui::Align2::LEFT_CENTER,
        c.kind.title(),
        font.clone(),
        pal.text_secondary,
    );
    if let Some(h) = hover {
        let words = match c.steps.get(h).cloned().flatten() {
            // Degrees close up to the figure, as everywhere: "328°".
            // The pane's own words for it, from core (#1776).
            Some(s) => s.hover.clone(),
            None => "–".to_string(),
        };
        let ink = match c.steps.get(h).cloned().flatten() {
            Some(s) if !s.grey => series_colour(c.kind, pal),
            _ => pal.text_secondary,
        };
        painter.text(
            egui::pos2(name.right() + 8.0, y),
            egui::Align2::LEFT_CENTER,
            words,
            font,
            ink,
        );
    }
}

/// A chart's unit on its own line in `line`, over its value axis, which
/// ends at `frame`'s left (Hugh on #1761: the ticks bare).
fn unit_line(ui: &egui::Ui, line: egui::Rect, frame: egui::Rect, c: &Chart) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    ui.painter().with_clip_rect(line).text(
        egui::pos2(frame.left() - 6.0, line.center().y),
        egui::Align2::RIGHT_CENTER,
        c.unit,
        font,
        theme::pal(ui).text_secondary,
    );
}

/// The hovered hour named under the bottom chart's `frame`, over its hour
/// labels, centred on the hover line.
fn hover_words(
    ui: &egui::Ui,
    frame: egui::Rect,
    x_range: (f64, f64),
    span: (f64, f64),
    words: &str,
) {
    let pal = theme::pal(ui);
    let galley = body_galley(ui, words);
    let size = galley.size();
    let left = words_left(frame, x_range, span, size.x);
    let at = egui::pos2(left, frame.bottom() + 3.0);
    let back = egui::Rect::from_min_size(at, size).expand(2.0);
    ui.painter().rect_filled(back, 2.0, pal.card);
    ui.painter().galley(at, galley, pal.text);
}

/// `words` laid out in the body font, as the hours under the charts are.
fn body_galley(ui: &egui::Ui, words: &str) -> std::sync::Arc<egui::Galley> {
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.painter()
        .layout_no_wrap(words.to_string(), font, theme::pal(ui).text)
}

/// Where words `width` wide start under `frame`: centred on the hour
/// `(a, b)`, kept inside the plot's width.
fn words_left(frame: egui::Rect, (x_lo, x_hi): (f64, f64), (a, b): (f64, f64), width: f32) -> f32 {
    let t = ((a + b) / 2.0 - x_lo) / (x_hi - x_lo);
    let x = frame.left() + frame.width() * t as f32;
    (x - width / 2.0)
        .max(frame.left())
        .min(frame.right() - width)
}

/// The hours whose labels the hovered hour's `words` would cover under
/// `frame`: a label's own width (the widest, at `end`) either side of
/// them, so a label left beside them keeps half a label's gap.
fn covered_hours(
    ui: &egui::Ui,
    frame: egui::Rect,
    (x_lo, x_hi): (f64, f64),
    span: (f64, f64),
    words: &str,
    end: f64,
) -> (f64, f64) {
    let width = body_galley(ui, words).size().x;
    let label = body_galley(ui, &format!("{end:.0} h")).size().x;
    let left = words_left(frame, (x_lo, x_hi), span, width);
    let per = (x_hi - x_lo) / f64::from(frame.width());
    let hour = |px: f32| x_lo + f64::from(px - frame.left()) * per;
    (hour(left - label), hour(left + width + label))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(v: f64, grey: bool) -> Option<Step> {
        Some(Step {
            v,
            hover: String::new(),
            band: None,
            grey,
        })
    }

    /// Hours side by side make one line, level across each and rising
    /// between; an hour not shown breaks it, and grey starts a new run
    /// from the level before.
    #[test]
    fn steps_join_level_and_break_at_gaps() {
        let spans = [(0.0, 1.0), (1.0, 2.0), (2.0, 3.0), (3.0, 4.0), (4.0, 4.5)];
        let steps = [
            step(1.0, false),
            step(2.0, false),
            None,
            step(3.0, false),
            step(4.0, true),
        ];
        let r = runs(&steps, &spans);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].0, vec![[0.0, 1.0], [1.0, 1.0], [1.0, 2.0], [2.0, 2.0]]);
        assert_eq!(r[1].0, vec![[3.0, 3.0], [4.0, 3.0]]);
        assert_eq!(r[2], (vec![[4.0, 3.0], [4.0, 4.0], [4.5, 4.0]], true));
    }

    /// A long run's margin left of 0 h, wider than an hour step, marks no
    /// hour before the run.
    #[test]
    fn no_hour_is_marked_before_the_run() {
        let marks = hour_marks(egui_plot::GridInput {
            bounds: (-2.3, 48.8),
            base_step_size: 0.2,
        });
        assert!(!marks.is_empty());
        assert!(marks.iter().all(|m| m.value >= 0.0), "{marks:?}");
        assert_eq!(marks[0].value, 0.0);
    }

    /// The hour at a time: its own span, the part hour's included.
    #[test]
    fn the_hour_at_a_time_is_the_one_whose_span_holds_it() {
        let spans = [(0.0, 1.0), (1.0, 1.4)];
        assert_eq!(hour_at(&spans, 0.5), Some(0));
        assert_eq!(hour_at(&spans, 1.2), Some(1));
        assert_eq!(hour_at(&spans, 1.5), None);
        assert_eq!(hour_at(&spans, -0.1), None);
    }
}
