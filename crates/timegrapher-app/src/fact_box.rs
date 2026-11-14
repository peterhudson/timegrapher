//! A plot's facts box: a few results read against the plot, a row each,
//! in a corner the trace leaves clear (#1528). The Tick Tock Profile's
//! and the Distributions' box.

use crate::marks::SHORT_PLOT;
use eframe::egui::{self, Color32};

/// A plot's facts in its top-left corner, as a plot item so they sit
/// over the grid and under the trace: a spike that runs into them still
/// shows. The Tick Tock Profile's box, and the Distributions' (#1528).
#[derive(Clone)]
pub(crate) struct FactBox {
    pub(crate) base: egui_plot::PlotItemBase,
    /// The four facts, or the one line saying there are none yet.
    pub(crate) rows: Vec<(String, String)>,
    pub(crate) plot_h: f32,
    /// The trace at each time, plot coordinates: the box takes the top
    /// right corner where it would cover the trace in the left and not
    /// there.
    pub(crate) tops: Vec<[f64; 2]>,
    /// Where `held` keeps the rows and corner last drawn, and those rows'
    /// count and corner ([`Corner`]): another layout must clear the trace
    /// by `FACT_HOLD` more, so a trace on the edge doesn't make the box
    /// flick between two from frame to frame.
    pub(crate) held: egui::Id,
    pub(crate) was: Option<(usize, Corner)>,
    /// The values' column, right-aligned, as wide as the widest value on
    /// either side; and the names' column likewise.
    pub(crate) name_w: f32,
    pub(crate) value_w: f32,
    /// The same for the two rows a short plot keeps.
    pub(crate) short_name_w: f32,
    pub(crate) short_value_w: f32,
    pub(crate) fill: Color32,
    pub(crate) name_color: Color32,
    pub(crate) value_color: Color32,
    /// Which rows a plot `plot_h` tall with room for `fit` rows keeps,
    /// top to bottom, for a box with a short form: fewer rows, tight
    /// around them, as narrow as `short_name_w` and `short_value_w`,
    /// shedding rows from the top. `None`: the first rows that fit, at
    /// the full spacing, shedding rows from the bottom.
    pub(crate) keep: Option<fn(f32, usize) -> Vec<usize>>,
    /// With no `keep`, the rows to shed first when not all fit, in turn;
    /// the others go after them from the bottom up.
    pub(crate) shed: Vec<usize>,
    /// With no `keep`, a plot too short for every row in one column flows
    /// them into two, side by side, before shedding any (Hugh, #1528):
    /// for a wide, short plot.
    pub(crate) flow: bool,
}

/// The plot's corner a facts box sits in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// The box's distance from the plot's corner, points.
pub(crate) const FACT_INSET: f32 = 4.0;
/// Room inside the box around its rows, and each row's height, points.
pub(crate) const FACT_PAD: f32 = 8.0;
pub(crate) const FACT_ROW: f32 = 16.0;
/// The room around the rows in a short plot's box, and each row's height.
pub(crate) const FACT_PAD_SHORT: f32 = 3.0;
pub(crate) const FACT_ROW_SHORT: f32 = 14.0;
/// Between the names' column and the values'.
pub(crate) const FACT_GAP: f32 = 16.0;
/// How much further from the trace a facts box must be to take a layout
/// other than the one last drawn, points.
pub(crate) const FACT_HOLD: f32 = 6.0;
/// Between a box's two columns, when its rows flow into two.
pub(crate) const FACT_COLUMN_GAP: f32 = 20.0;
/// The facts' type size.
pub(crate) const FACT_SIZE: f32 = 12.0;
/// The box's corner radius, which a point's note plate shares
/// ([`crate::marks::beside`]).
pub(crate) const FACT_RADIUS: f32 = 6.0;

impl FactBox {
    /// A box of `rows` with no short form, flowing into two columns
    /// before it sheds a row, `shed` first: the Distributions' (#1528)
    /// and the Isochronism's (#1507). `held` and `name` keep its layout
    /// apart from another box's.
    pub(crate) fn plain(
        ui: &egui::Ui,
        held: &str,
        name: &str,
        rows: Vec<(String, String)>,
        shed: Vec<usize>,
    ) -> Self {
        let pal = crate::theme::pal(ui);
        let held = ui.id().with((held, name));
        FactBox {
            base: egui_plot::PlotItemBase::new(format!("{name} facts")),
            name_w: widest(ui, rows.iter().map(|r| r.0.as_str())),
            value_w: widest(ui, rows.iter().map(|r| r.1.as_str())),
            short_name_w: 0.0,
            short_value_w: 0.0,
            rows,
            plot_h: 0.0,
            tops: Vec::new(),
            held,
            was: ui.ctx().data(|d| d.get_temp(held)),
            fill: ui.visuals().extreme_bg_color.gamma_multiply(0.8),
            name_color: pal.text_secondary,
            value_color: pal.text,
            keep: None,
            shed,
            flow: true,
        }
    }

    /// Where the box goes in a plot whose frame is `frame`, `rows` tall.
    /// Fewer than four rows of facts make a short box, as narrow as its
    /// rows and tight around them.
    #[cfg(test)]
    pub(crate) fn rect(&self, frame: egui::Rect, rows: usize) -> egui::Rect {
        self.rect_in(frame, rows, self.columns(frame, rows))
    }

    /// As [`Self::rect`], in `cols` columns.
    fn rect_in(&self, frame: egui::Rect, rows: usize, cols: usize) -> egui::Rect {
        let short = self.is_short(rows);
        let (name_w, value_w) = if short {
            (self.short_name_w, self.short_value_w)
        } else {
            (self.name_w, self.value_w)
        };
        let pad = self.pad(rows);
        let tall = rows.div_ceil(cols);
        let cols = cols as f32;
        egui::Rect::from_min_size(
            frame.min + egui::vec2(FACT_INSET, FACT_INSET),
            egui::vec2(
                cols * (name_w + FACT_GAP + value_w) + (cols - 1.0) * FACT_COLUMN_GAP + 2.0 * pad,
                tall as f32 * self.row_h(rows) + 2.0 * pad,
            ),
        )
    }

    /// The rows of one column at the full spacing that fit in `frame`.
    fn fit_rows(frame: egui::Rect) -> usize {
        let room = frame.height() - 2.0 * FACT_INSET - 2.0 * FACT_PAD;
        (room / FACT_ROW).floor().max(0.0) as usize
    }

    /// How many columns `rows` of the box take in `frame`: two where they
    /// [`Self::flow`] and one column can't hold them.
    fn columns(&self, frame: egui::Rect, rows: usize) -> usize {
        if self.flow && self.keep.is_none() && rows > Self::fit_rows(frame) {
            2
        } else {
            1
        }
    }

    /// Whether `rows` of the box take its short form.
    fn is_short(&self, rows: usize) -> bool {
        self.keep.is_some() && rows < self.rows.len()
    }

    /// The room around `rows` of the box.
    fn pad(&self, rows: usize) -> f32 {
        if self.is_short(rows) {
            FACT_PAD_SHORT
        } else {
            FACT_PAD
        }
    }

    /// Each row's height when the box shows `rows`.
    fn row_h(&self, rows: usize) -> f32 {
        if self.is_short(rows) {
            FACT_ROW_SHORT
        } else {
            FACT_ROW
        }
    }

    /// The rows shown in the plot as `t` draws it: inside the plot.
    pub(crate) fn shown(&self, t: &egui_plot::PlotTransform) -> Vec<usize> {
        let frame = *t.frame();
        let top = frame.top() + FACT_INSET;
        let room = |bottom: f32, pad: f32, row: f32| {
            ((bottom - top - 2.0 * pad) / row).floor().max(0.0) as usize
        };
        // A short plot's box is tight around its rows.
        let full = self.plot_h >= SHORT_PLOT
            && room(frame.bottom() - FACT_INSET, FACT_PAD, FACT_ROW) >= self.rows.len();
        let (pad, row) = if full || self.keep.is_none() {
            (FACT_PAD, FACT_ROW)
        } else {
            (FACT_PAD_SHORT, FACT_ROW_SHORT)
        };
        let fit_frame = room(frame.bottom() - FACT_INSET, pad, row);
        match self.keep {
            Some(keep) => keep(self.plot_h, fit_frame),
            // Two columns hold twice as many.
            None if self.flow => self.kept(2 * fit_frame),
            None => self.kept(fit_frame),
        }
    }

    /// The `n` rows kept, top to bottom, of a box with no `keep`: those
    /// in `shed` go first, in turn, then the rest from the bottom up.
    fn kept(&self, n: usize) -> Vec<usize> {
        let len = self.rows.len();
        let mut order: Vec<usize> = self.shed.iter().copied().filter(|&i| i < len).collect();
        order.extend((0..len).rev().filter(|i| !self.shed.contains(i)));
        let gone = &order[..len.saturating_sub(n)];
        (0..len).filter(|i| !gone.contains(i)).collect()
    }

    /// The column counts to try for `rows` of the box in `frame`, in
    /// turn: the one [`Self::columns`] takes, then two for a box that
    /// [`Self::flow`]s, so that a box whose one column would run over the
    /// trace flows before it sheds a row (Hugh, #1528).
    fn column_tries(&self, frame: egui::Rect, rows: usize) -> Vec<usize> {
        let first = self.columns(frame, rows);
        if first == 1 && rows > 1 && self.flow && self.keep.is_none() {
            vec![1, 2]
        } else {
            vec![first]
        }
    }

    /// The box of `rows` in `cols` columns in the top left corner and in
    /// the top right.
    pub(crate) fn corners(
        &self,
        t: &egui_plot::PlotTransform,
        rows: usize,
        cols: usize,
    ) -> (egui::Rect, egui::Rect) {
        let (left, right, _, _) = self.four(t, rows, cols);
        (left, right)
    }

    /// The box of `rows` in `cols` columns in each of the plot's corners:
    /// top left, top right, bottom left, bottom right.
    fn four(
        &self,
        t: &egui_plot::PlotTransform,
        rows: usize,
        cols: usize,
    ) -> (egui::Rect, egui::Rect, egui::Rect, egui::Rect) {
        let frame = *t.frame();
        let r = self.rect_in(frame, rows, cols);
        let right = egui::vec2(frame.right() - FACT_INSET - r.right(), 0.0);
        let down = egui::vec2(0.0, frame.bottom() - FACT_INSET - r.bottom());
        (
            r,
            r.translate(right),
            r.translate(down),
            r.translate(right + down),
        )
    }

    /// How many of the trace's points fall within `r`'s span of x: the
    /// fewer, the emptier the corner.
    fn under(&self, t: &egui_plot::PlotTransform, r: egui::Rect) -> usize {
        self.tops
            .iter()
            .filter(|p| {
                let x = t.position_from_point_x(p[0]);
                x >= r.left() && x <= r.right()
            })
            .count()
    }

    /// Whether the trace runs under `r`, or within `more` points under it
    /// short of the plot's foot, where a trace at rest lies.
    pub(crate) fn covers(&self, t: &egui_plot::PlotTransform, r: egui::Rect, more: f32) -> bool {
        let edge = r.bottom() + 2.0;
        let edge = edge + more.min(t.frame().bottom() - 0.5 - edge).max(0.0);
        self.tops.iter().any(|p| {
            let x = t.position_from_point_x(p[0]);
            x >= r.left() && x <= r.right() && t.position_from_point_y(p[1]) < edge
        })
    }

    /// The rows drawn and where: those [`Self::shown`] keeps, in the top
    /// left corner, or the top right where the trace runs under the left
    /// and not the right. Where the trace runs under both top corners the
    /// box keeps fewer rows, Unlock to Drop last. Where even one row would
    /// sit on it in both, all the rows go in the emptier bottom corner
    /// the trace leaves clear, or else top left anyway, on the box's
    /// plate: a fact is never dropped (Hugh, #1626). A layout other than
    /// the one last drawn must clear the trace by [`FACT_HOLD`] more.
    pub(crate) fn layout(&self, t: &egui_plot::PlotTransform) -> (Vec<usize>, egui::Rect) {
        let (shown, r, _) = self.placed(t);
        (shown, r)
    }

    /// As [`Self::layout`], with the columns the rows take: before
    /// shedding a row, a box that flows tries its rows in two columns,
    /// half as tall, in either corner.
    fn placed(&self, t: &egui_plot::PlotTransform) -> (Vec<usize>, egui::Rect, usize) {
        let (shown, r, cols, _) = self.placed_in(t);
        (shown, r, cols)
    }

    /// As [`Self::placed`], with the corner taken.
    fn placed_in(&self, t: &egui_plot::PlotTransform) -> (Vec<usize>, egui::Rect, usize, Corner) {
        let frame = *t.frame();
        let shown = self.shown(t);
        let all = shown.len();
        let first_cols = self.columns(frame, all.max(1));
        if self.tops.is_empty() || shown.is_empty() {
            let r = self.corners(t, all.max(1), first_cols).0;
            return (shown, r, first_cols, Corner::TopLeft);
        }
        let hold = |n: usize, at: Corner| {
            if self.was.is_none_or(|w| w == (n, at)) {
                0.0
            } else {
                FACT_HOLD
            }
        };
        for n in (1..=all).rev() {
            for cols in self.column_tries(frame, n) {
                let (left, right) = self.corners(t, n, cols);
                for (r, at) in [(left, Corner::TopLeft), (right, Corner::TopRight)] {
                    if !self.covers(t, r, hold(n, at)) {
                        let kept = if self.keep.is_some() {
                            shown[shown.len() - n..].to_vec()
                        } else {
                            self.kept(n)
                        };
                        return (kept, r, cols, at);
                    }
                }
            }
        }
        for cols in self.column_tries(frame, all) {
            let (_, _, left, right) = self.four(t, all, cols);
            let mut bottoms = [(left, Corner::BottomLeft), (right, Corner::BottomRight)];
            bottoms.sort_by_key(|&(r, _)| self.under(t, r));
            for (r, at) in bottoms {
                if !self.covers_from(t, r, hold(all, at)) {
                    return (shown, r, cols, at);
                }
            }
        }
        let r = self.corners(t, all, first_cols).0;
        (shown, r, first_cols, Corner::TopLeft)
    }

    /// Whether the trace runs through `r` in a bottom corner, or within
    /// `more` points above it: a point of the trace within `r`'s span of
    /// x from `r`'s top down to its bottom, the plot's foot below it
    /// aside, where a trace at rest lies.
    fn covers_from(&self, t: &egui_plot::PlotTransform, r: egui::Rect, more: f32) -> bool {
        let (top, bottom) = (r.top() - 2.0 - more, r.bottom() + 2.0);
        self.tops.iter().any(|p| {
            let x = t.position_from_point_x(p[0]);
            let y = t.position_from_point_y(p[1]);
            x >= r.left() && x <= r.right() && y >= top && y < bottom
        })
    }

    /// The span of x the box takes, as `t` draws it, its sides 0 wide at
    /// the plot's left edge with no rows shown: nothing names itself over
    /// it.
    pub(crate) fn span(&self, t: &egui_plot::PlotTransform) -> (f32, f32) {
        let (shown, r) = self.layout(t);
        if shown.is_empty() {
            (t.frame().left(), t.frame().left())
        } else {
            (r.left() - FACT_INSET, r.right())
        }
    }

    /// The box's `shown` rows in `cols` columns, in `r`.
    fn rows_shapes(
        &self,
        ui: &egui::Ui,
        shown: &[usize],
        (r, cols): (egui::Rect, usize),
        shapes: &mut Vec<egui::Shape>,
    ) {
        let pad = self.pad(shown.len());
        let row = self.row_h(shown.len());
        shapes.push(egui::Shape::rect_filled(r, FACT_RADIUS, self.fill));
        let font = egui::FontId::proportional(FACT_SIZE);
        // Down the first column, then the second, when they flow.
        let tall = shown.len().div_ceil(cols);
        let col_w =
            (r.width() - 2.0 * pad + FACT_COLUMN_GAP) / shown.len().div_ceil(tall.max(1)) as f32;
        for (i, (name, value)) in shown.iter().map(|&k| &self.rows[k]).enumerate() {
            let y = r.top() + pad + ((i % tall) as f32 + 0.5) * row;
            let left = r.left() + pad + (i / tall) as f32 * col_w;
            let right = left + col_w - FACT_COLUMN_GAP;
            ui.fonts_mut(|f| {
                let g = f.layout_no_wrap(name.clone(), font.clone(), self.name_color);
                let at = egui::pos2(left, y - g.size().y / 2.0);
                shapes.push(egui::Shape::galley(at, g, self.name_color));
                let g = f.layout_no_wrap(value.clone(), font.clone(), self.value_color);
                let at = egui::pos2(right - g.size().x, y - g.size().y / 2.0);
                shapes.push(egui::Shape::galley(at, g, self.value_color));
            });
        }
    }

    /// The box's size with all its rows in one column.
    pub(crate) fn size(&self) -> egui::Vec2 {
        let none = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::ZERO);
        self.rect_in(none, self.rows.len(), 1).size()
    }

    /// The box drawn by `painter` on a picture that isn't a plot, all its
    /// rows in one column; where it went. It takes the first of `tops`
    /// (each its top left corner) where no point of `trace` falls under
    /// it; else the one of `bottoms` farthest from the trace of those it
    /// keeps clear of (the last of those as far); else the first of `tops` anyway, on its plate: a
    /// fact is never dropped (Hugh, #1626). The Dynamic Poise pane's
    /// (#1537): the plot items' box, drawn the same way.
    pub(crate) fn paint(
        &self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        (tops, bottoms): (&[egui::Pos2], &[egui::Pos2]),
        trace: &[egui::Pos2],
    ) -> egui::Rect {
        let size = self.size();
        let at = |p: &egui::Pos2| egui::Rect::from_min_size(*p, size);
        let clear = |r: &egui::Rect| !trace.iter().any(|p| r.expand(2.0).contains(*p));
        let gap = |r: &egui::Rect| {
            trace
                .iter()
                .map(|p| r.distance_to_pos(*p))
                .fold(f32::INFINITY, f32::min)
        };
        let r = tops
            .iter()
            .map(at)
            .find(clear)
            .or_else(|| {
                bottoms
                    .iter()
                    .map(at)
                    .filter(clear)
                    .max_by(|a, b| gap(a).total_cmp(&gap(b)))
            })
            .unwrap_or_else(|| at(&tops[0]));
        let all: Vec<usize> = (0..self.rows.len()).collect();
        let mut shapes = Vec::new();
        self.rows_shapes(ui, &all, (r, 1), &mut shapes);
        painter.extend(shapes);
        r
    }
}

impl egui_plot::PlotItem for FactBox {
    fn shapes(
        &self,
        ui: &egui::Ui,
        transform: &egui_plot::PlotTransform,
        shapes: &mut Vec<egui::Shape>,
    ) {
        let (shown, r, cols, at) = self.placed_in(transform);
        // Kept for the next frame's layout, as `held` reads it.
        ui.ctx()
            .data_mut(|d| d.insert_temp(self.held, (shown.len(), at)));
        if shown.is_empty() {
            return;
        }
        self.rows_shapes(ui, &shown, (r, cols), shapes);
    }

    fn initialize(&mut self, _x_range: std::ops::RangeInclusive<f64>) {}

    fn color(&self) -> Color32 {
        self.value_color
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

/// The widest of `texts` in the facts' type, points.
pub(crate) fn widest<'a>(ui: &egui::Ui, texts: impl Iterator<Item = &'a str>) -> f32 {
    texts
        .map(|t| {
            ui.fonts_mut(|f| {
                f.layout_no_wrap(
                    t.to_owned(),
                    egui::FontId::proportional(FACT_SIZE),
                    Color32::WHITE,
                )
                .size()
                .x
            })
        })
        .fold(0.0, f32::max)
}
