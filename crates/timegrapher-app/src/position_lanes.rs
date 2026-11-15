//! The Positions Test's charts (#1729, Hugh's layout, Peter's approval of
//! 14 Nov): core's position line for each measure, stacked, each on its
//! own value axis. The value runs along the bottom; each position is a
//! dot on a lane of its own, named in the position's teal, the lanes in
//! two groups, Horizontal then Vertical. The three averages are grey
//! dashed lines named above the plot by the rule every named line in the
//! app follows. On a test after full wind, each position's full-wind
//! figure is a hollow dot with an arrow to this test's, and a key says
//! which is which. No position is ever joined to another (Norm's chart
//! rule). The hovered position is ringed in every chart, and its figure
//! is in each chart's title row.

use crate::marks::{self, Named, Rank};
use crate::steady::series_colour;
use crate::strength::{self, Grid, Strength};
use crate::theme;
use eframe::egui;
use timegrapher_core::position_line::{test_lines, PositionLine};
use timegrapher_core::session::{PositionGroup, SourcedFigures};

/// The height of each chart's title row, points.
const TITLE_H: f32 = 18.0;
/// The least height of a lane, points; lanes grow to fill the pane up to
/// [`LANE_MAX_H`].
const LANE_H: f32 = 15.0;
const LANE_MAX_H: f32 = 24.0;
/// The gap between the Horizontal and the Vertical lanes, in lanes.
const GROUP_GAP: f32 = 0.8;
/// The height under each chart for its values, points.
const AXIS_H: f32 = 20.0;
/// The gap between one chart's axis and the next one's title row, points.
const CHART_GAP: f32 = 16.0;
/// A dot's radius, and the hover's ring round it, points.
const DOT_R: f32 = 4.5;
const RING_R: f32 = 7.5;
/// The gap between the group names, the codes and the plot, points.
const NAME_GAP: f32 = 10.0;
/// The room right of the plot for the last tick's half label and the
/// unit after it, points.
const UNIT_W: f32 = 52.0;

/// Where the pane keeps the hovered position, from the table or a chart,
/// for the next frame to draw.
pub fn hover_id() -> egui::Id {
    egui::Id::new("positions hover")
}

/// Where a table puts the position whose row is under the pointer this
/// frame.
pub fn table_hover_id() -> egui::Id {
    egui::Id::new("positions table hover")
}

/// Where the pane keeps its tables' widest width, for the next frame.
fn table_w_id() -> egui::Id {
    egui::Id::new("positions tables width")
}

/// A table of the pane `w` wide: the pane's body is as wide as its widest
/// table, so the charts beside it never cut one off.
pub fn note_table_w(ui: &egui::Ui, w: f32) {
    ui.data_mut(|d| {
        let was: f32 = d.get_temp(table_w_id()).unwrap_or_default();
        d.insert_temp(table_w_id(), was.max(w));
    });
}

/// A table's position rows: the hovered position's row outlined (#1729),
/// and the row under the pointer kept for the pane. `codes` are each
/// row's position, `tops` each row's top and then the foot of the last,
/// across `x`, `gap` between rows.
pub fn table_rows(
    ui: &egui::Ui,
    (x, gap): (egui::Rangef, f32),
    codes: &[&'static str],
    tops: &[f32],
) {
    let pal = theme::pal(ui);
    let half = gap / 2.0;
    let hover: Option<&'static str> = ui.data(|d| d.get_temp(hover_id())).flatten();
    let rows: Vec<(&'static str, egui::Rect)> = codes
        .iter()
        .zip(tops.windows(2))
        .map(|(&c, w)| {
            let y = egui::Rangef::new(w[0] - half, w[1] - half);
            (c, egui::Rect::from_x_y_ranges(x.expand(4.0), y))
        })
        .collect();
    if let Some((_, r)) = rows.iter().find(|(c, _)| Some(*c) == hover) {
        ui.painter().rect_stroke(
            *r,
            egui::CornerRadius::same(2),
            egui::Stroke::new(1.0_f32, pal.text_secondary),
            egui::StrokeKind::Inside,
        );
    }
    let Some(pointer) = ui.ctx().pointer_hover_pos() else {
        return;
    };
    if !ui.clip_rect().contains(pointer) {
        return;
    }
    if let Some((c, _)) = rows.iter().find(|(_, r)| r.contains(pointer)) {
        ui.data_mut(|d| d.insert_temp(table_hover_id(), Some(*c)));
    }
}

/// The pane's `body` (its tables and notes) in the scroll `area`, with
/// the charts of `lines` beside it, or above it where the pane is too
/// narrow for both (#1729); the position hovered in either marked in
/// both. Gives what the body gave.
pub fn beside<R>(
    ui: &mut egui::Ui,
    area: egui::ScrollArea,
    lines: &[PositionLine],
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let refs: Vec<&PositionLine> = lines.iter().collect();
    let hover: Option<&'static str> = ui.data(|d| d.get_temp(hover_id())).flatten();
    ui.data_mut(|d| d.insert_temp(table_hover_id(), None::<&'static str>));
    let room = ui.available_rect_before_wrap();
    // As wide as last frame's widest table, then measured again.
    let widest: f32 = ui.data(|d| d.get_temp(table_w_id())).unwrap_or_default();
    ui.data_mut(|d| d.insert_temp(table_w_id(), 0.0_f32));
    let body_w = crate::guided::TABLE_W.max(widest) + ui.spacing().scroll.allocated_width();
    let gap = 2.0 * theme::GAP;
    let (out, at) = if refs.is_empty() {
        (area.show(ui, body).inner, None)
    } else if room.width() - body_w - gap >= crate::reserve::CHARTS_MIN_W {
        // The table at its own width, scrolling alone; the charts the
        // pane's height, or their least in a column that scrolls.
        let split = room.left() + body_w;
        let left = egui::Rect::from_x_y_ranges(room.left()..=split, room.y_range());
        let column = egui::Rect::from_x_y_ranges(split + gap..=room.right(), room.y_range());
        let out = ui
            .scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
                area.show(ui, body).inner
            })
            .inner;
        let high = column.height().max(read_h(&refs));
        let at = ui
            .scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("positions charts scroll")
                    .auto_shrink([false, false])
                    .max_height(column.height())
                    .show(ui, |ui| {
                        let bar = if high > column.height() {
                            ui.spacing().scroll.allocated_width()
                        } else {
                            0.0
                        };
                        let size = egui::vec2(ui.available_width() - bar, high);
                        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                        stack(ui, rect, &refs, hover)
                    })
                    .inner
            })
            .inner;
        ui.allocate_rect(room, egui::Sense::hover());
        (out, at)
    } else {
        area.show(ui, |ui| {
            let size = egui::vec2(ui.available_width(), read_h(&refs));
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let at = stack(ui, rect, &refs, hover);
            ui.add_space(theme::GAP);
            (body(ui), at)
        })
        .inner
    };
    let table: Option<&'static str> = ui.data(|d| d.get_temp(table_hover_id())).flatten();
    let now = table.or(at);
    if now != hover {
        ui.data_mut(|d| d.insert_temp(hover_id(), now));
        ui.ctx().request_repaint();
    }
    out
}

/// The position lines the pane draws, one for each measure `shown` that
/// has a figure: core's [`test_lines`] for this test's `figures` at
/// `wind_h`, the 45° stops in `more`, and the full-wind test's `full`.
pub fn lines(
    figures: &[SourcedFigures],
    more: &[SourcedFigures],
    (wind_h, full): (Option<f64>, Option<&[SourcedFigures]>),
    shown: [bool; 3],
) -> Vec<PositionLine> {
    let all = test_lines(wind_h, figures, more, full);
    [all.rate, all.amplitude, all.beat_error]
        .into_iter()
        .zip(shown)
        .filter(|(l, on)| *on && l.axis.is_some())
        .map(|(l, _)| l)
        .collect()
}

/// Each lane's place down the plot, in lanes from the top: the
/// Horizontal lanes, a gap, then the Vertical ones.
fn lane_ys(line: &PositionLine) -> Vec<f64> {
    let mut y = 0.0;
    let mut last = None;
    line.lanes
        .iter()
        .map(|l| {
            if last.is_some_and(|g| g != l.group) {
                y += f64::from(GROUP_GAP);
            }
            last = Some(l.group);
            let at = y;
            y += 1.0;
            at
        })
        .collect()
}

/// A chart's height in lanes, its half lane above and below included.
fn lanes_tall(line: &PositionLine) -> f64 {
    lane_ys(line).last().map_or(1.0, |y| y + 1.0)
}

/// The height of a chart's parts other than its lanes, points: its title
/// row, the names over its plot and its values under it.
fn chrome_h() -> f32 {
    TITLE_H + marks::ROW_H + marks::LANE_GAP + AXIS_H
}

/// The height `lines` need to read well, stacked.
pub fn read_h(lines: &[&PositionLine]) -> f32 {
    let n = lines.len() as f32;
    lines
        .iter()
        .map(|l| chrome_h() + lanes_tall(l) as f32 * LANE_H)
        .sum::<f32>()
        + (n - 1.0).max(0.0) * CHART_GAP
}

/// The group names' and the codes' column widths, the same for every
/// chart so their plots line up.
fn name_widths(ui: &egui::Ui, lines: &[&PositionLine]) -> (f32, f32) {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let wide = |t: &str, f: &egui::FontId| {
        ui.fonts_mut(|fo| {
            fo.layout_no_wrap(t.to_string(), f.clone(), egui::Color32::WHITE)
                .size()
                .x
        })
    };
    let groups = [PositionGroup::Horizontal, PositionGroup::Vertical]
        .iter()
        .map(|g| wide(g.name(), &body))
        .fold(0.0, f32::max);
    let codes = lines
        .iter()
        .flat_map(|l| &l.lanes)
        .map(|l| wide(l.position, &small))
        .fold(0.0, f32::max);
    (groups, codes)
}

/// The charts in `rect`, stacked, the position `hover` ringed in each.
/// Gives the position under the pointer, if any.
pub fn stack(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    lines: &[&PositionLine],
    hover: Option<&str>,
) -> Option<&'static str> {
    let (groups_w, codes_w) = name_widths(ui, lines);
    let gutter = groups_w + NAME_GAP + codes_w + NAME_GAP / 2.0;
    // The lanes grow to share the height, up to a limit, so a short test
    // doesn't stretch thin across a tall pane.
    let tall: f64 = lines.iter().map(|l| lanes_tall(l)).sum();
    let n = lines.len() as f32;
    let spare = rect.height() - n * chrome_h() - (n - 1.0).max(0.0) * CHART_GAP;
    let lane_h = (spare / tall as f32).clamp(LANE_H, LANE_MAX_H);
    let mut top = rect.top();
    let mut under = None;
    for (i, line) in lines.iter().enumerate() {
        let h = chrome_h() + lanes_tall(line) as f32 * lane_h;
        let area = egui::Rect::from_x_y_ranges(rect.x_range(), top..=top + h);
        let at = one(ui, area, line, (gutter, groups_w, lane_h), hover, i == 0);
        under = under.or(at);
        top += h + CHART_GAP;
    }
    under
}

/// One chart in `area`: its title row, the averages' names, the lanes
/// and their names, and the values under it. The `first` chart names the
/// hovered position in its title row, and holds the key. Gives the
/// position under the pointer.
fn one(
    ui: &mut egui::Ui,
    area: egui::Rect,
    line: &PositionLine,
    (gutter, groups_w, lane_h): (f32, f32, f32),
    hover: Option<&str>,
    first: bool,
) -> Option<&'static str> {
    let [lo, hi, step] = line.axis?;
    let pal = theme::pal(ui);
    let ink = series_colour(line.series, pal);
    let title = egui::Rect::from_x_y_ranges(area.x_range(), area.top()..=area.top() + TITLE_H);
    let plot_top = title.bottom() + marks::ROW_H + marks::LANE_GAP;
    let tall = lanes_tall(line);
    let plot_rect = egui::Rect::from_x_y_ranges(
        area.left() + gutter..=area.right() - UNIT_W,
        plot_top..=plot_top + tall as f32 * lane_h + AXIS_H,
    );
    let ys = lane_ys(line);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(plot_rect));
    let resp = plot(&mut child, line, (lo, hi, step), tall);
    let t = resp.transform;
    let frame = *t.frame();
    let at = |v: f64, y: f64| t.position_from_point(&egui_plot::PlotPoint::new(v, -y));
    let painter = ui.painter_at(frame.expand(RING_R + 1.0));
    for (l, &y) in line.lanes.iter().zip(&ys) {
        let Some(v) = l.value else { continue };
        let now = at(v, y);
        if let Some(w) = l.full_wind {
            let was = at(w, y);
            arrow(&painter, was, now, ink);
            painter.circle(was, DOT_R - 0.75, pal.card, egui::Stroke::new(1.5_f32, ink));
        }
        painter.circle_filled(now, DOT_R, ink);
        if hover == Some(l.position) {
            painter.circle_stroke(now, RING_R, egui::Stroke::new(1.5_f32, pal.text));
        }
    }
    lane_names(ui, frame, line, &ys, (groups_w, lane_h, area.left()));
    let named: Vec<Named> = line
        .averages
        .iter()
        .map(|a| Named {
            x: at(a.value, 0.0).x,
            name: a.name.to_string(),
            rank: Rank::Faint,
            ink: strength::colour(Strength::CallOut, pal),
            hover: a.hover.clone(),
        })
        .collect();
    marks::lane_above_stack(
        ui,
        frame,
        &named,
        ui.id().with(("position averages", line.title)),
    );
    title_row(ui, title, line, hover, first);
    // The unit at the value axis's right end (Hugh's rule 7), clear of
    // the last tick's label, which is centred on the end.
    let signed = line.series == timegrapher_core::steadiness::SeriesKind::Rate;
    let last = crate::hour_steps::tick_label(signed, hi, step);
    let half = ui
        .painter()
        .layout_no_wrap(last, egui::TextStyle::Body.resolve(ui.style()), pal.text)
        .size()
        .x
        / 2.0;
    ui.painter().text(
        egui::pos2(frame.right() + half + 6.0, frame.bottom() + AXIS_H / 2.0),
        egui::Align2::LEFT_CENTER,
        line.unit,
        egui::TextStyle::Small.resolve(ui.style()),
        pal.text_secondary,
    );
    // The position under the pointer: the lane nearest it, over the plot
    // or its names.
    let pointer = ui.ctx().pointer_hover_pos()?;
    let lanes = egui::Rect::from_x_y_ranges(area.left()..=frame.right(), frame.y_range());
    if !lanes.contains(pointer) || !ui.clip_rect().contains(pointer) {
        return None;
    }
    let y = -t.value_from_position(pointer).y;
    line.lanes
        .iter()
        .zip(&ys)
        .min_by(|a, b| (a.1 - y).abs().total_cmp(&(b.1 - y).abs()))
        .map(|(l, _)| l.position)
}

/// The plot: the value axis along the bottom and the averages' dashed
/// lines; the dots are painted over it.
fn plot(
    ui: &mut egui::Ui,
    line: &PositionLine,
    (lo, hi, step): (f64, f64, f64),
    tall: f64,
) -> egui_plot::PlotResponse<()> {
    use egui_plot::{Plot, VLine};
    let pal = theme::pal(ui);
    let signed = line.series == timegrapher_core::steadiness::SeriesKind::Rate;
    // A grid line along each lane, as the mock draws them.
    let lanes: Vec<egui_plot::GridMark> = lane_ys(line)
        .into_iter()
        .map(|y| egui_plot::GridMark {
            value: -y,
            step_size: 1.0,
        })
        .collect();
    let grid = Grid::new(
        move |g| crate::hour_steps::value_marks(g, Some([lo, hi, step]), &[80.0, 60.0]),
        move |_| lanes.clone(),
    );
    let plot = grid
        .on(Plot::new(("position line", line.title)))
        .height(ui.available_height())
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .show_x(false)
        .show_y(false)
        .show_axes([true, false])
        .default_x_bounds(lo, hi)
        .default_y_bounds(-tall + 0.5, 0.5)
        .x_axis_formatter(move |m, _| crate::hour_steps::tick_label(signed, m.value, m.step_size));
    plot.show(ui, |p| {
        grid.draw(p, pal);
        for a in &line.averages {
            p.vline(
                VLine::new(a.name, a.value)
                    .color(strength::colour(Strength::CallOut, pal))
                    .width(strength::CALL_OUT_W)
                    .style(strength::dashed()),
            );
        }
    })
}

/// An arrow from `from` to the dot at `to`, ending at the dot's edge.
fn arrow(painter: &egui::Painter, from: egui::Pos2, to: egui::Pos2, ink: egui::Color32) {
    let d = to - from;
    let len = d.length();
    if len < 2.0 * DOT_R + 6.0 {
        return;
    }
    let dir = d / len;
    let tip = to - dir * (DOT_R + 1.0);
    let start = from + dir * DOT_R;
    let side = egui::vec2(-dir.y, dir.x) * 3.5;
    painter.line_segment([start, tip - dir * 5.0], egui::Stroke::new(1.0_f32, ink));
    painter.add(egui::Shape::convex_polygon(
        vec![tip, tip - dir * 6.0 + side, tip - dir * 6.0 - side],
        ink,
        egui::Stroke::NONE,
    ));
}

/// Each lane's code in the position's teal beside the plot, and each
/// group's name at its first lane, from `left`.
fn lane_names(
    ui: &egui::Ui,
    frame: egui::Rect,
    line: &PositionLine,
    ys: &[f64],
    (groups_w, lane_h, left): (f32, f32, f32),
) {
    let pal = theme::pal(ui);
    let painter = ui.painter();
    let top = frame.top() + lane_h / 2.0;
    let mut last = None;
    for (l, &y) in line.lanes.iter().zip(ys) {
        let at = top + y as f32 * lane_h;
        painter.text(
            egui::pos2(left + groups_w + NAME_GAP, at),
            egui::Align2::LEFT_CENTER,
            l.position,
            egui::TextStyle::Small.resolve(ui.style()),
            pal.positions,
        );
        if last != Some(l.group) {
            painter.text(
                egui::pos2(left, at),
                egui::Align2::LEFT_CENTER,
                l.group.name(),
                egui::TextStyle::Body.resolve(ui.style()),
                pal.text_secondary,
            );
        }
        last = Some(l.group);
    }
}

/// A chart's title row in `band`: its name over the lanes' names,
/// the hovered position's figure after it in its colour (named in the
/// `first` chart), and in the first chart the key at the right where
/// there is a comparison.
fn title_row(
    ui: &egui::Ui,
    band: egui::Rect,
    line: &PositionLine,
    hover: Option<&str>,
    first: bool,
) {
    let pal = theme::pal(ui);
    let ink = series_colour(line.series, pal);
    let painter = ui.painter().with_clip_rect(band);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let y = band.center().y;
    let name = painter.text(
        egui::pos2(band.left(), y),
        egui::Align2::LEFT_CENTER,
        line.title,
        font.clone(),
        pal.text_secondary,
    );
    if let Some(l) = hover.and_then(|h| line.lanes.iter().find(|l| l.position == h)) {
        // The name once, in the first chart's title (Hugh on #1785).
        let words = if first { &l.hover } else { &l.figures };
        painter.text(
            egui::pos2(name.right() + 8.0, y),
            egui::Align2::LEFT_CENTER,
            words,
            font.clone(),
            ink,
        );
    }
    let Some([solid, hollow]) = line.key.filter(|_| first) else {
        return;
    };
    let small = egui::TextStyle::Small.resolve(ui.style());
    let mut x = band.right();
    for (words, filled) in [(hollow, false), (solid, true)] {
        let r = painter.text(
            egui::pos2(x, y),
            egui::Align2::RIGHT_CENTER,
            words,
            small.clone(),
            pal.text_secondary,
        );
        let dot = egui::pos2(r.left() - DOT_R - 4.0, y);
        if filled {
            painter.circle_filled(dot, DOT_R, ink);
        } else {
            painter.circle(dot, DOT_R - 0.75, pal.card, egui::Stroke::new(1.5_f32, ink));
        }
        x = dot.x - DOT_R - 16.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use timegrapher_core::session::{Position, StateIndices};
    use timegrapher_core::steadiness::SeriesKind;

    fn state(rates: &[(Position, f64)]) -> StateIndices {
        let figures: Vec<_> = rates
            .iter()
            .map(|&(p, r)| (p, Some(r), Some(300.0), Some(0.1), None))
            .collect();
        timegrapher_core::session::sourced_indices_over(None, &figures, &Position::TEN)
    }

    /// The lanes go Horizontal then Vertical, with a gap between the
    /// groups, a lane apart within each.
    #[test]
    fn the_groups_have_a_gap_between_them() {
        let s = state(&[
            (Position::CH, 1.0),
            (Position::CB, 2.0),
            (Position::H3, 3.0),
            (Position::H6, 4.0),
        ]);
        let line = timegrapher_core::position_line::position_line(&s, None, SeriesKind::Rate);
        let ys = lane_ys(&line);
        let gap = f64::from(GROUP_GAP);
        assert_eq!(ys, vec![0.0, 1.0, 2.0 + gap, 3.0 + gap]);
        assert_eq!(lanes_tall(&line), 4.0 + gap);
    }
}

#[cfg(test)]
#[path = "position_lanes_tests.rs"]
mod pane_tests;
