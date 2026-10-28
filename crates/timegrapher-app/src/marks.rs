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

/// Where the names in a lane of one row go: `names` are each one's centre
/// (its line's x), width and rank. Each is centred on its line within
/// `left..right`; one that would come within [`NAME_GAP`] of a name
/// already placed slides along past it (drawn with an angled leader), to
/// the right where there is room, else to the left. Each name's left
/// edge, in the order given.
pub fn place_row(names: &[(f32, f32, Rank)], left: f32, right: f32) -> Vec<f32> {
    let mut taken: Vec<(f32, f32)> = Vec::new();
    let mut out = vec![0.0; names.len()];
    for i in order(names) {
        let (x, w, _) = names[i];
        let last = (right - w).max(left);
        let centred = (x - w / 2.0).clamp(left, last);
        let hit = |a: f32, taken: &[(f32, f32)]| {
            taken
                .iter()
                .find(|&&(s, e)| a < e + NAME_GAP && a + w + NAME_GAP > s)
                .copied()
        };
        let mut at = centred;
        while let Some((_, e)) = hit(at, &taken) {
            at = e + NAME_GAP;
        }
        if at > last {
            at = centred;
            while let Some((s, _)) = hit(at, &taken) {
                at = s - NAME_GAP - w;
            }
            at = at.max(left);
        }
        taken.push((at, at + w));
        out[i] = at;
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

/// The name `g` in a lane, at `rect`, as plain text: under it, a patch
/// of the pane's own colour hides any leader that would cross it, and
/// shows nothing else.
pub fn paint_lane_name(
    ui: &egui::Ui,
    painter: &egui::Painter,
    rect: egui::Rect,
    g: Arc<egui::Galley>,
) {
    painter.rect_filled(rect.expand(2.0), 2.0, theme::pal(ui).card);
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
    lane(ui, frame, lines, id, false);
}

/// As [`lane_above`], in a lane of one row: a name that would touch
/// another slides along the row ([`place_row`]). For a plot that names
/// only a few lines, such as the Scope's two edges.
pub fn lane_above_one(ui: &egui::Ui, frame: egui::Rect, lines: &[Named], id: egui::Id) {
    lane(ui, frame, lines, id, true);
}

fn lane(ui: &egui::Ui, frame: egui::Rect, lines: &[Named], id: egui::Id, one: bool) {
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
    // Within the part of the plot on screen, the patch under each name
    // included, so no name is cut at an edge.
    let seen = frame
        .intersect(ui.clip_rect())
        .shrink2(egui::vec2(2.0, 0.0));
    let placed = if one {
        place_row(&names, seen.left(), seen.right())
            .into_iter()
            .map(|a| (a, 0))
            .collect()
    } else {
        let placed = place(&names, seen.left(), seen.right(), &rows_were(ui, id));
        keep_rows(ui, id, &placed);
        placed
    };
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

/// "escape wheel" as "Escape Wheel": a wheel's name as the app shows it.
pub fn title_case(name: &str) -> String {
    name.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A wheel's short name, for the narrow lane beside a map: "fourth wheel"
/// as "4th Wheel". A name with no short form is shown whole.
pub fn wheel_short(name: &str) -> String {
    match name {
        "centre wheel" => "2nd Wheel".into(),
        "third wheel" => "3rd Wheel".into(),
        "fourth wheel" => "4th Wheel".into(),
        "escape wheel" => "Esc Wheel".into(),
        "seconds wheel" => "Sec Wheel".into(),
        "intermediate wheel" => "Int Wheel".into(),
        _ => title_case(name),
    }
}

/// The length of the tick that joins a line across a map to its name.
const TICK: f32 = 4.0;
/// From the map's edge to its names: the tick, then the room a leader
/// takes to reach a name that slid.
const SIDE_GAP: f32 = TICK + 8.0;

/// Where names level with lines across a plot go, in a column beside it:
/// `ys` are the lines' screen y, and each name is [`ROW_H`] tall. Each is
/// centred on its line; names that would touch slide apart, each run of
/// them spread evenly about its lines' middle, and the column is kept
/// within `top..bottom`. Each name's centre, in the order given.
pub fn stack(ys: &[f32], top: f32, bottom: f32) -> Vec<f32> {
    let mut o: Vec<usize> = (0..ys.len()).collect();
    o.sort_by(|&a, &b| ys[a].total_cmp(&ys[b]));
    // Runs of names that touch, each as its first name and its size; a
    // run is centred on its lines' middle, and joins the run above where
    // they would touch.
    // The centre of the first name in a run: its lines' middle less half
    // the run, kept within the column.
    let first = |(s, n): (usize, usize)| {
        let mid = o[s..s + n].iter().map(|&i| ys[i]).sum::<f32>() / n as f32;
        (mid - (n as f32 - 1.0) * ROW_H / 2.0)
            .min(bottom - ROW_H / 2.0 - (n as f32 - 1.0) * ROW_H)
            .max(top + ROW_H / 2.0)
    };
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for k in 0..o.len() {
        let mut run = (k, 1);
        while let Some(&above) = runs.last() {
            let end = first(above) + above.1 as f32 * ROW_H;
            if first(run) < end {
                runs.pop();
                run = (above.0, above.1 + run.1);
            } else {
                break;
            }
        }
        runs.push(run);
    }
    let mut out = vec![0.0; ys.len()];
    for &run in &runs {
        let y0 = first(run);
        for (j, &i) in o[run.0..run.0 + run.1].iter().enumerate() {
            out[i] = y0 + j as f32 * ROW_H;
        }
    }
    out
}

/// A line across a plot, named: where it runs (screen y), its short
/// name, rank and colour, and what its hover says.
pub struct Across {
    pub y: f32,
    pub name: String,
    pub rank: Rank,
    pub ink: Color32,
    pub hover: String,
}

/// The room the names of `lines` take beside a plot: the tick and the
/// widest name. Kept the same whatever stretch the plot shows, so the plot
/// doesn't change width as its lines come and go.
pub fn side_width(ui: &egui::Ui, names: &[(String, Rank)]) -> f32 {
    let widest = names
        .iter()
        .map(|(n, r)| galley(ui, n, *r, false).size().x)
        .fold(0.0, f32::max);
    if widest > 0.0 {
        SIDE_GAP + widest + 4.0
    } else {
        0.0
    }
}

/// Lines across a plot (`frame`), named in a column right of it, as
/// [`stack`] places them: each name joined to its line by a short tick of
/// the line's colour, with an angled leader where it had to slide.
/// Nothing is drawn over the plot itself. `id` keys the hovers, each the
/// line and its name together. Whether the pointer is on one, so the plot
/// can leave out its own hover there.
pub fn lane_right(ui: &egui::Ui, frame: egui::Rect, lines: &[Across], id: egui::Id) -> bool {
    let lines: Vec<&Across> = lines
        .iter()
        .filter(|l| l.y >= frame.top() && l.y <= frame.bottom())
        .collect();
    let ys: Vec<f32> = lines.iter().map(|l| l.y).collect();
    let mut on = false;
    let at = stack(&ys, frame.top() - ROW_H / 2.0, frame.bottom() + ROW_H / 2.0);
    let right = frame.right();
    let painter = ui.painter_at(egui::Rect::from_min_max(
        egui::pos2(right, frame.top() - ROW_H),
        egui::pos2(ui.max_rect().right(), frame.bottom() + ROW_H),
    ));
    for (l, &y) in lines.iter().zip(&at) {
        let mut pts = vec![egui::pos2(right, l.y), egui::pos2(right + TICK, l.y)];
        pts.push(egui::pos2(right + SIDE_GAP - 2.0, y));
        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0_f32, l.ink)));
        let g = galley(ui, &l.name, l.rank, false);
        let rect =
            egui::Rect::from_min_size(egui::pos2(right + SIDE_GAP, y - g.size().y / 2.0), g.size());
        painter.galley(rect.min, g, Color32::PLACEHOLDER);
        // The line runs the map's width, so its hover opens by the pointer
        // rather than under the target's left end.
        let line = egui::Rect::from_x_y_ranges(frame.left()..=right, l.y - 3.0..=l.y + 3.0);
        let r = ui.interact(
            rect.union(line),
            id.with(("line", &l.name)),
            egui::Sense::hover(),
        );
        on |= r.hovered();
        r.on_hover_text_at_pointer(&l.hover);
    }
    on
}

/// How far a note beside a point stands off from it, each way, points.
const NOTE_OFF: f32 = 8.0;

/// The places a note `size` big may take beside a point (`dot`, screen),
/// in the order they are tried: above right, above left, below right,
/// below left, then up a row at a time, right and left, to the plot's
/// `top`.
pub fn beside_spots(dot: egui::Pos2, size: egui::Vec2, top: f32) -> Vec<egui::Rect> {
    let (w, h) = (size.x, size.y);
    let at = |dx: f32, dy: f32| egui::Rect::from_min_size(dot + egui::vec2(dx, dy), size);
    let (right, left) = (NOTE_OFF, -NOTE_OFF - w);
    let (above, below) = (-NOTE_OFF - h, NOTE_OFF);
    let mut spots = vec![
        at(right, above),
        at(left, above),
        at(right, below),
        at(left, below),
    ];
    let mut up = above - ROW_H;
    while dot.y + up >= top {
        spots.extend([at(right, up), at(left, up)]);
        up -= ROW_H;
    }
    spots
}

/// Whether a trace (`pts`, screen) crosses `r`: any of its segments
/// passes through it, clipped as Liang and Barsky do.
pub fn crosses(r: egui::Rect, pts: &[egui::Pos2]) -> bool {
    let through = |a: egui::Pos2, b: egui::Pos2| {
        let d = b - a;
        let (mut t0, mut t1) = (0.0_f32, 1.0_f32);
        for (p, q) in [
            (-d.x, a.x - r.left()),
            (d.x, r.right() - a.x),
            (-d.y, a.y - r.top()),
            (d.y, r.bottom() - a.y),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    return false;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
        t0 <= t1
    };
    match pts {
        [] => false,
        [p] => r.contains(*p),
        _ => pts.windows(2).any(|s| through(s[0], s[1])),
    }
}

/// Which of `spots` a note takes: the one it took last frame (`was`)
/// while that is still `clear`, so it doesn't hop as the trace moves a
/// little, else the first that is; none where none is.
pub fn beside_place(
    spots: &[egui::Rect],
    clear: &dyn Fn(egui::Rect) -> bool,
    was: Option<u8>,
) -> Option<u8> {
    if was.is_some_and(|w| spots.get(w as usize).is_some_and(|r| clear(*r))) {
        return was;
    }
    (0..spots.len()).find(|&i| clear(spots[i])).map(|i| i as u8)
}

/// The room a note beside a point takes above it: its row and its
/// stand-off, and a little air.
const NOTE_ROOM: f32 = NOTE_OFF + ROW_H + 4.0;

/// The top of a plot's y range (from `lo`, at least `top`) that leaves a
/// note's room clear above the trace's highest point (at `y`) on a plot
/// `h` points tall, so a note always has a row the trace doesn't cross:
/// a plot's tallest peak would otherwise touch its top.
pub fn room_above(lo: f64, y: f64, top: f64, h: f32) -> f64 {
    let r = f64::from(NOTE_ROOM / h.max(2.0 * NOTE_ROOM));
    top.max((y - lo * r) / (1.0 - r))
}

/// A marked point's note (a plot's tallest peak, the lag a series
/// repeats at, the period found), drawn over the plot after it is shown:
/// the dot (`at`, plot coordinates) stays on the trace, and the note goes
/// on a plate of `fill` (80% of the plot's background) in the nearest
/// clear space [`beside_place`] finds, joined to the dot by a thin
/// leader. Clear is inside the plot, not crossed by the trace (`trace`,
/// plot coordinates) and off `avoid` (other notes on the plot, screen).
/// On a `short` plot, or where nothing is clear, the note is the dot's
/// hover. `id` keeps the place it took.
#[allow(clippy::too_many_arguments)]
pub fn beside(
    ui: &egui::Ui,
    t: &egui_plot::PlotTransform,
    at: [f64; 2],
    note: &str,
    trace: &[[f64; 2]],
    avoid: &[egui::Rect],
    fill: Color32,
    short: bool,
    id: egui::Id,
) {
    let frame = *t.frame();
    let dot = t.position_from_point(&egui_plot::PlotPoint::new(at[0], at[1]));
    if !frame.contains(dot) {
        return;
    }
    let pal = theme::pal(ui);
    let mut job = egui::text::LayoutJob::single_section(
        note.to_string(),
        egui::TextFormat {
            font_id: egui::FontId::proportional(NAME_SIZE),
            color: pal.text,
            ..Default::default()
        },
    );
    job.wrap.max_rows = 1;
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let size = egui::vec2(g.size().x + 6.0, ROW_H);
    let pts: Vec<egui::Pos2> = trace
        .iter()
        .map(|p| t.position_from_point(&egui_plot::PlotPoint::new(p[0], p[1])))
        .collect();
    let spots = beside_spots(dot, size, frame.top());
    let inner = frame.shrink(1.0);
    let clear = |r: egui::Rect| {
        inner.contains_rect(r)
            && !crosses(r.expand(2.0), &pts)
            && !avoid.iter().any(|a| a.intersects(r))
    };
    let was = ui.data(|d| d.get_temp::<Option<u8>>(id)).flatten();
    let place = if short {
        None
    } else {
        beside_place(&spots, &clear, was)
    };
    ui.data_mut(|d| d.insert_temp(id, place));
    let Some(k) = place else {
        let hit = egui::Rect::from_center_size(dot, egui::vec2(12.0, 12.0));
        return hover(ui, id.with("hover"), hit, note);
    };
    let rect = spots[k as usize];
    let painter = ui.painter_at(frame);
    // The leader, from the dot's edge to the plate's nearest point.
    let near = rect.clamp(dot);
    let d = near - dot;
    if d.length() > 4.0 {
        painter.line_segment(
            [dot + d.normalized() * 4.0, near],
            egui::Stroke::new(1.0_f32, pal.text_secondary),
        );
    }
    painter.rect_filled(rect, 3.0, fill);
    painter.galley(
        egui::pos2(rect.left() + 3.0, rect.center().y - g.size().y / 2.0),
        g,
        Color32::PLACEHOLDER,
    );
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

    /// Names beside a map: level with their lines where they have room;
    /// names that would touch spread evenly about their lines' middle;
    /// the column kept inside the map.
    #[test]
    fn names_beside_a_map_slide_apart_where_they_would_touch() {
        // Apart: each level with its line.
        assert_eq!(stack(&[100.0, 40.0], 0.0, 300.0), vec![100.0, 40.0]);
        // Two lines 5 pt apart: a row apart, about their middle.
        assert_eq!(stack(&[100.0, 105.0], 0.0, 300.0), vec![95.0, 110.0]);
        // A third touching the pair joins its run, spread about all three.
        let y = stack(&[98.0, 104.0, 119.0], 0.0, 300.0);
        assert_eq!(y, vec![92.0, 107.0, 122.0]);
        // At the bottom edge the run rises to stay inside.
        assert_eq!(stack(&[298.0, 296.0], 0.0, 300.0), vec![292.5, 277.5]);
    }

    #[test]
    fn wheels_have_short_names_for_the_maps() {
        assert_eq!(wheel_short("centre wheel"), "2nd Wheel");
        assert_eq!(wheel_short("fourth wheel"), "4th Wheel");
        assert_eq!(wheel_short("escape wheel"), "Esc Wheel");
        assert_eq!(wheel_short("barrel"), "Barrel");
        assert_eq!(wheel_short("great wheel"), "Great Wheel");
        assert_eq!(title_case("escape wheel"), "Escape Wheel");
    }

    /// A lane of one row: names centred on their lines; one that would
    /// touch slides past the name there, right where it fits, else left.
    #[test]
    fn in_a_lane_of_one_row_a_crowding_name_slides_along() {
        // Apart: both centred.
        assert_eq!(
            place_row(&[(100.0, 40.0, S), (300.0, 40.0, S)], 0.0, 500.0),
            vec![80.0, 280.0]
        );
        // Drop 20 pt right of Unlock: it slides right, a gap past it.
        assert_eq!(
            place_row(&[(100.0, 40.0, S), (120.0, 40.0, S)], 0.0, 500.0),
            vec![80.0, 132.0]
        );
        // At the right edge there's no room to its right: it goes left.
        assert_eq!(
            place_row(&[(470.0, 40.0, S), (490.0, 40.0, S)], 0.0, 500.0),
            vec![450.0, 398.0]
        );
    }

    /// A note beside a point: tried above right first, then round the
    /// point and a row up; never where the trace crosses it; and kept
    /// where it was while that stays clear, though an earlier place has
    /// come clear.
    #[test]
    fn a_point_note_goes_in_the_nearest_clear_space_and_stays() {
        let dot = egui::pos2(100.0, 100.0);
        let size = egui::vec2(40.0, ROW_H);
        let spots = beside_spots(dot, size, 0.0);
        assert_eq!(spots[0].min, egui::pos2(108.0, 100.0 - 8.0 - ROW_H));
        assert_eq!(spots[1].max.x, 92.0);
        assert_eq!(spots[2].min, egui::pos2(108.0, 108.0));
        assert_eq!(spots[3].min.y, 108.0);
        assert_eq!(spots[4].min.y, spots[0].min.y - ROW_H);
        // Up a row at a time, right and left, as far as the plot's top.
        assert_eq!(spots.len(), 4 + 2 * 5);
        assert!(spots[13].min.y >= 0.0);

        // A trace rising to the right through the dot crosses above right
        // and below left; the note goes above left.
        let trace = [egui::pos2(0.0, 200.0), dot, egui::pos2(200.0, 0.0)];
        assert!(crosses(spots[0], &trace));
        assert!(!crosses(spots[1], &trace));
        assert!(crosses(spots[3], &trace));
        let clear = |r: egui::Rect| !crosses(r, &trace);
        assert_eq!(beside_place(&spots, &clear, None), Some(1));

        // Taken below right last frame and still clear there: it stays.
        assert_eq!(beside_place(&spots, &clear, Some(2)), Some(2));
        // Where the trace now crosses its place, it moves.
        assert_eq!(beside_place(&spots, &clear, Some(0)), Some(1));
        // Nowhere clear: none, and the note is the dot's hover.
        assert_eq!(beside_place(&spots, &|_| false, Some(1)), None);
    }

    /// A trace crosses a note's place only where one of its segments
    /// passes through it, not where it passes by.
    #[test]
    fn a_trace_crosses_a_place_only_through_it() {
        let r = egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(20.0, 20.0));
        let by = [egui::pos2(0.0, 25.0), egui::pos2(30.0, 25.0)];
        let through = [egui::pos2(0.0, 15.0), egui::pos2(30.0, 15.0)];
        let corner = [egui::pos2(0.0, 0.0), egui::pos2(30.0, 30.0)];
        let flat_inside = [egui::pos2(12.0, 12.0), egui::pos2(18.0, 12.0)];
        assert!(!crosses(r, &by));
        assert!(crosses(r, &through));
        assert!(crosses(r, &corner));
        assert!(crosses(r, &flat_inside));
        assert!(!crosses(r, &[]));
    }

    /// A plot keeps a note's room above its marked point: the point sits
    /// that far below the top, and a top already higher stays.
    #[test]
    fn a_plot_keeps_room_above_its_marked_point() {
        let top = room_above(0.0, 10.0, 10.3, 200.0);
        let px = (top - 10.0) / top * 200.0;
        assert!((px - f64::from(NOTE_ROOM)).abs() < 1e-3, "{px}");
        assert_eq!(room_above(0.0, 10.0, 20.0, 200.0), 20.0);
    }
}
