//! Names on the lines a plot marks (a Profile's edges, a calibre's
//! wheels): where each name goes and how it is drawn, by one set of rules
//! for every plot. Names sit in a lane outside the plot, on the side the
//! lines come from, each centred on its line; a name that would touch
//! another goes a row out, sliding along with a leader where that row is
//! taken. On a short plot they go inside, along its top. Strong names (a
//! wheel, an edge) are placed first, faint ones (a wheel's ×2, a reference
//! period) after. This is layout, not a value worked out from the sound,
//! so it lives in the app.

use crate::theme;
use eframe::egui::{self, Color32};
use std::sync::Arc;

/// One row of names in a lane, points tall.
pub const ROW_H: f32 = 15.0;
/// A lane of names: two rows.
pub const LANE: f32 = 2.0 * ROW_H;
/// The short gap between the plot and its nearest row of names, so each
/// line or leader shows outside the plot before it reaches its name.
pub const LANE_GAP: f32 = 6.0;
/// The room kept between two names in a row: a word space and more, so
/// two names side by side never read as one phrase.
pub const NAME_GAP: f32 = 12.0;
/// How far an overlap must start or end before a name changes row, or a
/// plot's height passes a limit before its names move, so they don't hop
/// between frames.
pub const HYSTERESIS: f32 = 2.0;
/// A plot shorter than this takes its names inside, along its top.
pub const SHORT_PLOT: f32 = 100.0;
/// Inside a short plot, the row of names' gap below the plot's top.
pub const INSIDE_TOP: f32 = 4.0;
/// The names' type size.
const NAME_SIZE: f32 = 11.5;

/// How strongly a line is named: a wheel or an edge is strong, a wheel's
/// ×2 or a reference period faint. Strong names are placed first, and are
/// semibold in the text colour; faint ones regular in the secondary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rank {
    Strong,
    Faint,
}

/// The order names are placed in: strong first, then faint, each left
/// to right. `names` are each one's line x, width and rank.
fn order(names: &[(f32, f32, Rank)]) -> Vec<usize> {
    let mut o: Vec<usize> = (0..names.len()).collect();
    o.sort_by(|&a, &b| {
        (names[a].2 == Rank::Faint)
            .cmp(&(names[b].2 == Rank::Faint))
            .then(names[a].0.total_cmp(&names[b].0))
    });
    o
}

/// Where each name in a lane goes: `names` are each one's centre (its
/// line's x), width and rank; `was` is the row each was in last frame.
/// Each name is centred on its line, kept within `left..right`, and goes
/// one row further out where it would come within [`NAME_GAP`] of a name
/// already in the first row; where the second row is taken there too, it
/// slides right of the name there (drawn with an angled leader). Returns
/// each name's left edge and row, in the order given.
pub fn place(names: &[(f32, f32, Rank)], left: f32, right: f32, was: &[u8]) -> Vec<(f32, u8)> {
    let mut rows: [Vec<(f32, f32)>; 2] = Default::default();
    let mut out = vec![(0.0, 0); names.len()];
    for i in order(names) {
        let (x, w, _) = names[i];
        let last = (right - w).max(left);
        let at = (x - w / 2.0).clamp(left, last);
        // The overlap with a name in the first row, the gap between names
        // included: a name goes out once it overlaps by the hysteresis,
        // and comes back once it clears by as much.
        let overlap = rows[0]
            .iter()
            .map(|&(a, b)| (b + NAME_GAP - at).min(at + w + NAME_GAP - a))
            .fold(f32::NEG_INFINITY, f32::max);
        let out_row = if was.get(i).copied().unwrap_or(0) == 1 {
            overlap > -HYSTERESIS
        } else {
            overlap > HYSTERESIS
        };
        let (at, row) = if out_row {
            let mut a = at;
            while let Some(&(_, e)) = rows[1]
                .iter()
                .find(|&&(s, e)| a < e + NAME_GAP && a + w + NAME_GAP > s)
            {
                a = e + NAME_GAP;
            }
            (a.min(last), 1)
        } else {
            (at, 0)
        };
        rows[row as usize].push((at, at + w));
        out[i] = (at, row);
    }
    out
}

/// Where the names go inside a short plot, one row along its top: `names`
/// are each one's line x, width and rank. Each is centred on its line
/// within the plot; one that would touch the facts box (the span of x
/// `facts`) starts or ends at its line instead, on the side away from the
/// box, or isn't named if its line is under the box; one that would come
/// within [`NAME_GAP`] of a name already placed isn't named (its line's
/// hover names it). Where the trace crosses its row there (`clear` says
/// whether a span of x is clear of it), it ends at its line on the left,
/// else starts at it on the right, whichever is clear; with neither, it
/// isn't named rather than sit on the trace. A name beside its line never
/// covers another line, where it would read as that line's name. Each
/// name's left edge, or none, in the order given.
pub fn inside(
    names: &[(f32, f32, Rank)],
    left: f32,
    right: f32,
    facts: (f32, f32),
    clear: &dyn Fn(f32, f32) -> bool,
) -> Vec<Option<f32>> {
    let mut placed: Vec<(f32, f32)> = Vec::new();
    let near = |placed: &[(f32, f32)], a: f32, w: f32| {
        placed
            .iter()
            .any(|&(s, e)| a - 3.0 < e + NAME_GAP && a + w + 3.0 + NAME_GAP > s)
    };
    let on_facts = |a: f32, w: f32| a + w + 3.0 > facts.0 - 4.0 && a - 3.0 < facts.1 + 4.0;
    // Over another line, its box hiding it.
    let on_line = |a: f32, w: f32, own: f32| {
        names
            .iter()
            .any(|&(x, _, _)| x != own && x > a - 5.0 && x < a + w + 5.0)
    };
    let mut out = vec![None; names.len()];
    for i in order(names) {
        let (x, w, _) = names[i];
        let mut at = (x - w / 2.0).clamp(left + 4.0, (right - 4.0 - w).max(left + 4.0));
        if on_facts(at, w) {
            // Beside its line, away from the box; its line under the box,
            // unnamed rather than over the facts (its line's hover names
            // it).
            match [x + 4.0, x - w - 4.0].into_iter().find(|&a| {
                !on_facts(a, w)
                    && !on_line(a, w, x)
                    && a - 3.0 >= left + 1.0
                    && a + w + 3.0 <= right - 1.0
            }) {
                Some(a) => at = a,
                None => continue,
            }
        }
        if !clear(at - 3.0, at + w + 3.0) {
            let fits = |a: f32| {
                a - 3.0 >= left + 1.0
                    && !near(&placed, a, w)
                    && a + w + 3.0 <= right - 1.0
                    && !on_facts(a, w)
                    && !on_line(a, w, x)
                    && clear(a - 3.0, a + w + 3.0)
            };
            match [x - w - 5.0, x + 5.0].into_iter().find(|&a| fits(a)) {
                Some(a) => at = a,
                None => continue,
            }
        }
        if near(&placed, at, w) {
            continue;
        }
        placed.push((at - 3.0, at + w + 3.0));
        out[i] = Some(at);
    }
    out
}

/// Whether a plot `h` tall is short, so its names go inside it: under
/// [`SHORT_PLOT`], with [`HYSTERESIS`] either way so it doesn't flip as
/// the pane is dragged. `id` keeps what it was last frame.
pub fn short(ui: &egui::Ui, id: egui::Id, h: f32) -> bool {
    let was = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let short = h < if was {
        SHORT_PLOT + HYSTERESIS
    } else {
        SHORT_PLOT - HYSTERESIS
    };
    ui.data_mut(|d| d.insert_temp(id, short));
    short
}

/// The rows a lane's names took last frame, kept under `id`.
pub fn rows_were(ui: &egui::Ui, id: egui::Id) -> Vec<u8> {
    ui.data(|d| d.get_temp(id)).unwrap_or_default()
}

/// Keep the rows a lane's names took, for the next frame.
pub fn keep_rows(ui: &egui::Ui, id: egui::Id, placed: &[(f32, u8)]) {
    ui.data_mut(|d| d.insert_temp(id, placed.iter().map(|p| p.1).collect::<Vec<u8>>()));
}

/// A name laid out in its rank's type: strong semibold in the text
/// colour, faint regular in the secondary; `italic` for a Profile's
/// sounds.
pub fn galley(ui: &egui::Ui, name: &str, rank: Rank, italic: bool) -> Arc<egui::Galley> {
    let pal = theme::pal(ui);
    let (font_id, color) = match rank {
        Rank::Strong => (theme::semibold(NAME_SIZE), pal.text),
        Rank::Faint => (egui::FontId::proportional(NAME_SIZE), pal.text_secondary),
    };
    let mut job = egui::text::LayoutJob::single_section(
        name.to_string(),
        egui::TextFormat {
            font_id,
            color,
            italics: italic,
            ..Default::default()
        },
    );
    job.wrap.max_rows = 1;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// The name `g` in a lane, at `rect`, on a box of the panel's colour, so
/// no other line runs through it.
pub fn paint_lane_name(
    ui: &egui::Ui,
    painter: &egui::Painter,
    rect: egui::Rect,
    g: Arc<egui::Galley>,
) {
    painter.rect_filled(rect.expand(2.0), 2.0, ui.visuals().panel_fill);
    painter.galley(rect.min, g, Color32::PLACEHOLDER);
}

/// The rectangle of a name `w` wide at `left`, inside a short plot along
/// its top.
pub fn inside_rect(frame: egui::Rect, left: f32, w: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(left - 3.0, frame.top() + INSIDE_TOP),
        egui::vec2(w + 6.0, ROW_H),
    )
}

/// The shapes of a name `g` inside a short plot, its left edge at `left`
/// in `rect`: its box of `fill` (80% of the plot's background, so the grid
/// shows faintly) and the name.
pub fn inside_name(
    rect: egui::Rect,
    left: f32,
    g: Arc<egui::Galley>,
    fill: Color32,
) -> [egui::Shape; 2] {
    let p = egui::pos2(left, rect.center().y - g.size().y / 2.0);
    [
        egui::Shape::rect_filled(rect, 3.0, fill),
        egui::Shape::galley(p, g, Color32::PLACEHOLDER),
    ]
}

/// Whether the trace (`tops`, plot coordinates) stays below the row of
/// names inside a short plot from screen x `a` to `b`.
pub fn row_clear(t: &egui_plot::PlotTransform, tops: &[[f64; 2]], a: f32, b: f32) -> bool {
    let row_foot = t.frame().top() + INSIDE_TOP + ROW_H;
    tops.iter()
        .map(|p| (t.position_from_point_x(p[0]), t.position_from_point_y(p[1])))
        .filter(|(x, _)| (a..=b).contains(x))
        .all(|(_, y)| y > row_foot + 1.0)
}

/// A line and its name as one hover target, saying `text`.
pub fn hover(ui: &egui::Ui, id: egui::Id, hit: egui::Rect, text: impl Into<egui::WidgetText>) {
    ui.interact(hit, id, egui::Sense::hover())
        .on_hover_text(text);
}

/// A line down a plot, named: where it runs (screen x), its name, rank and
/// colour, and what its hover says.
pub struct Named {
    pub x: f32,
    pub name: String,
    pub rank: Rank,
    pub ink: Color32,
    pub hover: String,
}

/// Lines down a plot (`frame`), named in a lane of two rows above it, as
/// [`place`] places them: each name joined to its line by a short stroke
/// of the line's colour through the gap above the plot, with an angled
/// leader where it had to slide. The lines themselves are the plot's,
/// drawn under its trace. `id` keeps the rows the names took.
pub fn lane_above(ui: &egui::Ui, frame: egui::Rect, lines: &[Named], id: egui::Id) {
    let lines: Vec<&Named> = lines
        .iter()
        .filter(|l| l.x >= frame.left() && l.x <= frame.right())
        .collect();
    let galleys: Vec<_> = lines
        .iter()
        .map(|l| galley(ui, &l.name, l.rank, false))
        .collect();
    let names: Vec<(f32, f32, Rank)> = lines
        .iter()
        .zip(&galleys)
        .map(|(l, g)| (l.x, g.size().x, l.rank))
        .collect();
    let placed = place(&names, frame.left(), frame.right(), &rows_were(ui, id));
    keep_rows(ui, id, &placed);
    let painter = ui.painter_at(frame.expand2(egui::vec2(0.0, LANE + LANE_GAP + 2.0)));
    let mut boxes = Vec::new();
    for ((l, g), &(left, row)) in lines.iter().zip(galleys).zip(&placed) {
        let off = LANE_GAP + row as f32 * ROW_H;
        let rect = egui::Rect::from_min_size(
            egui::pos2(left, frame.top() - off - ROW_H),
            egui::vec2(g.size().x, ROW_H),
        );
        let centre = rect.center().x;
        let mut pts = vec![egui::pos2(centre, frame.top() - off - 1.0)];
        if (centre - l.x).abs() > 1.0 {
            pts.push(egui::pos2(l.x, frame.top() - LANE_GAP / 2.0));
        }
        pts.push(egui::pos2(l.x, frame.top()));
        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0_f32, l.ink)));
        let hit = rect.union(egui::Rect::from_x_y_ranges(
            l.x - 3.0..=l.x + 3.0,
            frame.top()..=frame.bottom(),
        ));
        hover(ui, id.with(("line", &l.name)), hit, &l.hover);
        boxes.push((rect, g));
    }
    for (rect, g) in boxes {
        paint_lane_name(ui, &painter, rect, g);
    }
}

/// Names inside a short plot along its top, as a plot item so they sit
/// over the plot's lines and under its trace, each on a box: `names` are
/// each one's line (plot x), name and rank; `tops` is the trace, which a
/// name keeps clear of. The lines are the plot's own.
pub struct InsideNames {
    pub base: egui_plot::PlotItemBase,
    pub names: Vec<(f64, String, Rank)>,
    pub tops: Vec<[f64; 2]>,
    pub fill: Color32,
}

impl InsideNames {
    /// Each name's line (screen x), name, rank and laid-out text, those
    /// inside the plot, and where [`inside`] puts each.
    fn placed(
        &self,
        ui: &egui::Ui,
        t: &egui_plot::PlotTransform,
    ) -> Vec<(f32, String, Option<f32>, Arc<egui::Galley>)> {
        inside_placed(ui, t, &self.names, &self.tops)
    }
}

/// Where the names inside a short plot go (see [`InsideNames`]): each
/// one inside the plot, its line's screen x, name, left edge or none, and
/// laid-out text.
pub fn inside_placed(
    ui: &egui::Ui,
    t: &egui_plot::PlotTransform,
    names: &[(f64, String, Rank)],
    tops: &[[f64; 2]],
) -> Vec<(f32, String, Option<f32>, Arc<egui::Galley>)> {
    let frame = *t.frame();
    let named: Vec<(f32, &String, Rank, Arc<egui::Galley>)> = names
        .iter()
        .map(|(v, n, r)| (t.position_from_point_x(*v), n, *r, galley(ui, n, *r, false)))
        .filter(|n| n.0 >= frame.left() && n.0 <= frame.right())
        .collect();
    let spans: Vec<(f32, f32, Rank)> = named.iter().map(|n| (n.0, n.3.size().x, n.2)).collect();
    let at = inside(
        &spans,
        frame.left(),
        frame.right(),
        (f32::NEG_INFINITY, f32::NEG_INFINITY),
        &|a, b| row_clear(t, tops, a, b),
    );
    named
        .into_iter()
        .zip(at)
        .map(|((x, n, _, g), a)| (x, n.clone(), a, g))
        .collect()
}

/// The hovers of lines named inside a short plot (`placed` as
/// [`inside_placed`] gives them): each line, and its name where it has
/// one, says `hover` for its name.
pub fn inside_hovers(
    ui: &egui::Ui,
    frame: egui::Rect,
    placed: &[(f32, String, Option<f32>, Arc<egui::Galley>)],
    hover_of: &dyn Fn(&str) -> String,
    id: egui::Id,
) {
    for (x, name, at, g) in placed {
        let line = egui::Rect::from_x_y_ranges(x - 3.0..=x + 3.0, frame.top()..=frame.bottom());
        let hit = at.map_or(line, |a| line.union(inside_rect(frame, a, g.size().x)));
        hover(ui, id.with(("line", name)), hit, hover_of(name));
    }
}

impl egui_plot::PlotItem for InsideNames {
    fn shapes(
        &self,
        ui: &egui::Ui,
        transform: &egui_plot::PlotTransform,
        shapes: &mut Vec<egui::Shape>,
    ) {
        let frame = *transform.frame();
        for (_, _, at, g) in self.placed(ui, transform) {
            if let Some(a) = at {
                shapes.extend(inside_name(
                    inside_rect(frame, a, g.size().x),
                    a,
                    g,
                    self.fill,
                ));
            }
        }
    }

    fn initialize(&mut self, _x_range: std::ops::RangeInclusive<f64>) {}

    fn color(&self) -> Color32 {
        Color32::TRANSPARENT
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

#[cfg(test)]
mod tests {
    use super::*;

    const S: Rank = Rank::Strong;
    const F: Rank = Rank::Faint;

    /// Names in a lane: centred on their marks, out a row where they would
    /// touch, slid along where the second row is taken, inside the plot,
    /// and kept in their row while an overlap is within the hysteresis.
    #[test]
    fn names_that_crowd_move_out_a_row() {
        // Apart: all in the first row, centred.
        let p = place(&[(100.0, 40.0, S), (300.0, 40.0, S)], 0.0, 500.0, &[]);
        assert_eq!(p, vec![(80.0, 0), (280.0, 0)]);
        // Peak would touch Drop: out a row, still centred on its line.
        let p = place(&[(100.0, 40.0, S), (130.0, 40.0, S)], 0.0, 500.0, &[]);
        assert_eq!(p, vec![(80.0, 0), (110.0, 1)]);
        // Three together: the third slides right of the second's name.
        let p = place(
            &[(100.0, 40.0, S), (110.0, 40.0, S), (120.0, 40.0, S)],
            0.0,
            500.0,
            &[],
        );
        assert_eq!(p[2].1, 1);
        assert!(p[2].0 >= p[1].0 + 40.0 + NAME_GAP - 1e-3, "{p:?}");
        // Kept inside the plot's width.
        let p = place(&[(5.0, 40.0, S), (495.0, 40.0, S)], 0.0, 500.0, &[]);
        assert_eq!(p, vec![(0.0, 0), (460.0, 0)]);
        // A word space apart is too close: two names side by side would
        // read as one phrase.
        let p = place(&[(100.0, 40.0, S), (147.0, 40.0, S)], 0.0, 500.0, &[]);
        assert_eq!(p[1].1, 1, "{p:?}");
        // Within the hysteresis a name stays in the row it was in: here
        // the gap is 1 pt more than NAME_GAP.
        let near = [(100.0, 40.0, S), (141.0 + NAME_GAP, 40.0, S)];
        assert_eq!(place(&near, 0.0, 500.0, &[0, 0])[1].1, 0);
        assert_eq!(place(&near, 0.0, 500.0, &[0, 1])[1].1, 1);
    }

    /// A strong name keeps the row nearest the plot: a faint one left of
    /// it that would touch it goes a row out, though it comes first left
    /// to right. Two faint names apart from the strong ones stay in.
    #[test]
    fn strong_names_are_placed_first() {
        // Third Wheel ×2 at 180, Third Wheel at 220: the wheel's name keeps
        // the first row, centred; the ×2's goes out, centred too.
        let p = place(&[(180.0, 60.0, F), (220.0, 60.0, S)], 0.0, 500.0, &[]);
        assert_eq!(p, vec![(150.0, 1), (190.0, 0)]);
        // All strong, the left one would have kept the row.
        let p = place(&[(180.0, 60.0, S), (220.0, 60.0, S)], 0.0, 500.0, &[]);
        assert_eq!(p, vec![(150.0, 0), (190.0, 1)]);
        // Far apart, every name in the first row.
        let p = place(
            &[(50.0, 40.0, F), (200.0, 40.0, S), (400.0, 40.0, F)],
            0.0,
            500.0,
            &[],
        );
        assert!(p.iter().all(|q| q.1 == 0), "{p:?}");
        // A faint name out a row slides past a name already there.
        let p = place(
            &[(100.0, 40.0, S), (110.0, 40.0, S), (120.0, 40.0, F)],
            0.0,
            500.0,
            &[],
        );
        assert_eq!(p[1], (90.0, 1));
        assert_eq!(p[2], (142.0, 1));
    }

    /// Inside a short plot the names are one row: centred on their lines;
    /// Unlock right of its line where the facts box has the corner; Peak
    /// unnamed where it would crowd Drop (its line's hover names it).
    #[test]
    fn a_short_plots_edge_names_keep_one_row() {
        let at = inside(
            &[(400.0, 40.0, S), (700.0, 30.0, S), (900.0, 30.0, S)],
            0.0,
            1000.0,
            (0.0, 200.0),
            &|_, _| true,
        );
        assert_eq!(at, vec![Some(380.0), Some(685.0), Some(885.0)]);
        // Unlock under the facts box: it starts at its line, right of it.
        let at = inside(&[(210.0, 40.0, S)], 0.0, 1000.0, (0.0, 200.0), &|_, _| true);
        assert_eq!(at, vec![Some(214.0)]);
        // Its line under the box: unnamed, the facts left clear.
        let at = inside(&[(150.0, 40.0, S)], 0.0, 1000.0, (0.0, 200.0), &|_, _| true);
        assert_eq!(at, vec![None]);
        // Peak 15 pt after Drop: not named.
        let at = inside(
            &[(700.0, 30.0, S), (715.0, 30.0, S)],
            0.0,
            1000.0,
            (0.0, 0.0),
            &|_, _| true,
        );
        assert_eq!(at, vec![Some(685.0), None]);
    }

    /// Where the trace crosses a name's row at its line, the name ends at
    /// the line on the left if that is clear, else starts at it on the
    /// right; crossed on both sides, it isn't named (its line's hover
    /// names it) rather than sit on the trace. Beside a facts box in the
    /// right corner, it goes left of its line.
    #[test]
    fn a_short_plots_name_takes_a_clear_side() {
        let names = [(700.0, 30.0, S)];
        // The trace in the row from `lo` to `hi`.
        let crossing = |lo: f32, hi: f32| move |a: f32, b: f32| b < lo || a > hi;
        let at = inside(&names, 0.0, 1000.0, (0.0, 0.0), &crossing(660.0, 700.0));
        assert_eq!(at, vec![Some(705.0)], "right of its line");
        let at = inside(&names, 0.0, 1000.0, (0.0, 0.0), &crossing(700.0, 760.0));
        assert_eq!(at, vec![Some(665.0)], "left of its line");
        let at = inside(&names, 0.0, 1000.0, (0.0, 0.0), &|_, _| false);
        assert_eq!(
            at,
            vec![None],
            "crossed both sides: unnamed, not on the trace"
        );
        // The facts box in the right corner: a name it would touch ends at
        // its line, left of it, away from the box.
        let at = inside(
            &[(790.0, 30.0, S)],
            0.0,
            1000.0,
            (800.0, 1000.0),
            &|_, _| true,
        );
        assert_eq!(at, vec![Some(756.0)]);
        // Drop crossed on both sides is unnamed; Peak, 20 pt after it and
        // crossed at its line and on its right, isn't named left of its
        // line over Drop's, where it would read as Drop's name.
        let at = inside(
            &[(700.0, 30.0, S), (720.0, 20.0, S)],
            0.0,
            1000.0,
            (0.0, 0.0),
            &|a, b| b < 684.0 || (a > 690.0 && b < 722.0) || a > 736.0,
        );
        assert_eq!(at, vec![None, None]);
        // 60 pt after it, clear of Drop's line, it is.
        let at = inside(
            &[(700.0, 30.0, S), (760.0, 20.0, S)],
            0.0,
            1000.0,
            (0.0, 0.0),
            &|a, b| b < 684.0 || (a > 690.0 && b < 705.0) || (a > 712.0 && b < 760.0) || a > 775.0,
        );
        assert_eq!(at, vec![None, Some(735.0)]);
    }

    /// Inside a short plot strong names are placed first: a faint name
    /// that would crowd a strong one is the one left unnamed, though it
    /// comes first left to right.
    #[test]
    fn inside_a_short_plot_the_strong_name_keeps_its_place() {
        let none = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        let at = inside(
            &[(200.0, 60.0, F), (240.0, 60.0, S)],
            0.0,
            1000.0,
            none,
            &|_, _| true,
        );
        assert_eq!(at, vec![None, Some(210.0)]);
        let at = inside(
            &[(100.0, 60.0, F), (240.0, 60.0, S)],
            0.0,
            1000.0,
            none,
            &|_, _| true,
        );
        assert_eq!(at, vec![Some(70.0), Some(210.0)]);
    }
}
