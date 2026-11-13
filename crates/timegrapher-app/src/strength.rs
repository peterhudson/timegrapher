//! How strongly each line on a plot is drawn: every line takes one of
//! four strengths, weakest to strongest grid, axis, call-out and data,
//! each one colour and one width on every plot and at every window size
//! ("Line Strength on Plots"). A line's strength comes from what it is,
//! never from how far apart the lines are, so the plotting library's own
//! grid (which fades with spacing) is turned off and [`Grid`] draws the
//! grid and the axes instead. The colours are the core palette's, so the
//! MCP charts can draw the same.

use crate::theme::Palette;
use eframe::egui::{self, Color32, Pos2, Shape, Stroke, Ui};
use egui_plot::{
    GridInput, GridMark, Plot, PlotBounds, PlotGeometry, PlotItem, PlotItemBase, PlotTransform,
    PlotUi,
};
use std::rc::Rc;

/// What a line on a plot is, which sets how strongly it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strength {
    /// Every grid line, across and down.
    Grid,
    /// The plot's left and bottom edges, and a zero line where zero
    /// means something.
    Axis,
    /// A named or reference line (Unlock, Drop, Peak, the sounds, the
    /// wheels, a threshold, the hover line): one grey, told apart by its
    /// pattern.
    CallOut,
}

/// The fourth and strongest, data (anything drawn from the readings),
/// takes its reading's own colour, so it is not a [`Strength`]: its lines
/// are at least this wide.
pub const DATA_W: f32 = 1.5;

/// A call-out's width.
pub const CALL_OUT_W: f32 = 1.2;

/// A fainter call-out's width (a wheel's ×2, a reference period): the
/// same grey, thinner.
pub const FAINT_CALL_OUT_W: f32 = 1.0;

/// The length of a dash, points.
const DASH: f32 = 6.0;

/// The pattern of a reference or fitted call-out (a threshold, a band, a
/// state level): named marks (and a reference period) are solid, sounds
/// and wheels dotted.
pub fn dashed() -> egui_plot::LineStyle {
    egui_plot::LineStyle::Dashed { length: DASH }
}

/// The colour of a grid, axis or call-out line. Grid and axis are the
/// text colour see-through, so they keep their strength over a tint such
/// as the Paper Strip's wash.
pub fn colour(s: Strength, pal: &Palette) -> Color32 {
    match s {
        Strength::Grid => see_through(pal.text, pal.grid_opacity),
        Strength::Axis => see_through(pal.text, pal.axis_opacity),
        Strength::CallOut => pal.callout,
    }
}

/// The stroke of a grid, axis or call-out line: 1 pt for grid and axis,
/// [`CALL_OUT_W`] for a call-out.
pub fn stroke(s: Strength, pal: &Palette) -> Stroke {
    let w = match s {
        Strength::Grid | Strength::Axis => 1.0,
        Strength::CallOut => CALL_OUT_W,
    };
    Stroke::new(w, colour(s, pal))
}

/// A time `x` drawn across a plot whose transform is `t`, at strength
/// `s`, where it falls inside the plot: the moment shown and the
/// pointer's time on the Charts and on Readings Over Time (#1063, #1277).
pub fn time_line(
    painter: &egui::Painter,
    t: &egui_plot::PlotTransform,
    x: f64,
    s: Strength,
    pal: &Palette,
) {
    let frame = *t.frame();
    let px = t.position_from_point_x(x);
    if px >= frame.left() && px <= frame.right() {
        painter.vline(px, frame.y_range(), stroke(s, pal));
    }
}

fn see_through(c: Color32, opacity: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (opacity * 255.0).round() as u8)
}

/// The marks one axis of a plot is labelled at (and gridded at).
pub type Spacer = Rc<dyn Fn(GridInput) -> Vec<GridMark>>;

/// A plot's grid and axes: the marks along each axis (the same marks its
/// labels sit at) and whether zero across is a zero line.
#[derive(Clone)]
pub struct Grid {
    x: Spacer,
    y: Spacer,
    zero_y: bool,
}

impl Grid {
    pub fn new(
        x: impl Fn(GridInput) -> Vec<GridMark> + 'static,
        y: impl Fn(GridInput) -> Vec<GridMark> + 'static,
    ) -> Self {
        Grid {
            x: Rc::new(x),
            y: Rc::new(y),
            zero_y: false,
        }
    }

    /// Draw zero across at axis strength (0 s/d, 0 ms, a correlation of
    /// 0), where zero means something.
    pub fn zero_y(mut self) -> Self {
        self.zero_y = true;
        self
    }

    /// `plot` labelled at this grid's marks, with the library's grid and
    /// background off: [`Grid::draw`] paints the background without the
    /// library's border, which lay under the axes and doubled their grey,
    /// and drew a top and right edge no plot has.
    pub fn on<'a>(&self, plot: Plot<'a>) -> Plot<'a> {
        let (x, y) = (self.x.clone(), self.y.clone());
        plot.x_grid_spacer(move |g| x(g))
            .y_grid_spacer(move |g| y(g))
            .show_grid(false)
            .show_background(false)
    }

    /// Add the background, grid and axes to a plot, before anything else
    /// so they lie under it.
    pub fn draw(&self, p: &mut PlotUi<'_>, pal: &Palette) {
        self.add(p, pal, Layers::All);
    }

    /// Add the background and a map's picture over it, for a plot filled
    /// edge to edge, whose grid would lie hidden under the fill. One item,
    /// so the picture is painted after the background: egui_plot paints a
    /// `PlotImage` as it is added, under every other item, which hid the
    /// time-frequency maps under the background (Walter, 11-05).
    pub fn draw_map(&self, p: &mut PlotUi<'_>, pal: &Palette, map: Picture) {
        self.add(p, pal, Layers::Map(map));
    }

    /// Add the axes alone, over such a plot, so they show on its fill.
    pub fn draw_axes(&self, p: &mut PlotUi<'_>, pal: &Palette) {
        self.add(p, pal, Layers::Axes);
    }

    fn add(&self, p: &mut PlotUi<'_>, pal: &Palette, layers: Layers) {
        p.add(GridItem {
            base: PlotItemBase::new(String::new()),
            grid: self.clone(),
            layers,
            grid_c: colour(Strength::Grid, pal),
            axis_c: colour(Strength::Axis, pal),
        });
    }
}

/// A picture laid over a plot: its texture, stretched from `min` to
/// `max` in plot values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Picture {
    pub texture: egui::TextureId,
    pub min: [f64; 2],
    pub max: [f64; 2],
}

/// What a [`GridItem`] paints.
#[derive(Clone, Copy, PartialEq)]
enum Layers {
    /// The background, the grid and the axes.
    All,
    /// The background and a picture over it.
    Map(Picture),
    /// The axes alone.
    Axes,
}

/// A zero line, at axis strength: down a plot at an x, or across it at
/// a y, in screen points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zero {
    None,
    Down(f32),
    Across(f32),
}

/// Paint a plot's grid lines down (`xs`) and across (`ys`), its zero
/// line and its left and bottom axes into `frame`, for a plot drawn by
/// hand (the Paper Strip, the Scope) rather than by egui_plot. Positions
/// are in screen points; any off the frame or on an axis are left out.
pub fn paint(
    painter: &egui::Painter,
    frame: egui::Rect,
    (xs, ys): (&[f32], &[f32]),
    zero: Zero,
    pal: &Palette,
) {
    let (grid, axis) = (colour(Strength::Grid, pal), colour(Strength::Axis, pal));
    painter
        .with_clip_rect(frame)
        .extend(lines(frame, (xs, ys), zero, (grid, axis), true));
}

/// The grid lines (when `grid`), the zero line and the axes of a plot in
/// `frame`, as shapes in the grid's and the axes' colours.
fn lines(
    f: egui::Rect,
    (xs, ys): (&[f32], &[f32]),
    zero: Zero,
    (grid_c, axis_c): (Color32, Color32),
    grid: bool,
) -> Vec<Shape> {
    let (down, across) = if grid {
        layout(f, xs, ys, zero)
    } else {
        (Vec::new(), Vec::new())
    };
    let grid = Stroke::new(1.0_f32, grid_c);
    let axis = Stroke::new(1.0_f32, axis_c);
    let seg = |a: Pos2, b: Pos2, s: Stroke| Shape::line_segment([a, b], s);
    let mut shapes = Vec::new();
    for x in down {
        shapes.push(seg(Pos2::new(x, f.top()), Pos2::new(x, f.bottom()), grid));
    }
    for y in across {
        shapes.push(seg(Pos2::new(f.left(), y), Pos2::new(f.right(), y), grid));
    }
    match zero {
        Zero::Down(z) if f.x_range().contains(z) => {
            shapes.push(seg(Pos2::new(z, f.top()), Pos2::new(z, f.bottom()), axis));
        }
        Zero::Across(z) if f.y_range().contains(z) => {
            shapes.push(seg(Pos2::new(f.left(), z), Pos2::new(f.right(), z), axis));
        }
        _ => {}
    }
    let (left, bottom) = axes(f);
    shapes.push(seg(Pos2::new(left, f.top()), Pos2::new(left, bottom), axis));
    shapes.push(seg(
        Pos2::new(left, bottom),
        Pos2::new(f.right(), bottom),
        axis,
    ));
    shapes
}

/// The plot's minimum grid spacing, in points: what egui_plot hands a
/// spacer as its base step.
const BASE_SPACING: f32 = 8.0;

/// Within this many points of an axis a grid line is not drawn, so the
/// axis never doubles up or flickers as the range moves.
const ON_AXIS: f32 = 1.0;

/// Where the left and bottom axes go in a plot's `frame`: half a line in
/// from its edges, so the plot's clip to its frame keeps the whole line
/// (on the edge itself, half of it was cut and the frame covered the
/// rest).
fn axes(frame: egui::Rect) -> (f32, f32) {
    (frame.left() + 0.5, frame.bottom() - 0.5)
}

/// The grid and the axes, as one item drawn under the plot's others.
struct GridItem {
    base: PlotItemBase,
    grid: Grid,
    layers: Layers,
    grid_c: Color32,
    axis_c: Color32,
}

impl GridItem {
    /// Where each grid line falls along `axis` (0 across, 1 down), in
    /// screen points.
    fn lines(&self, t: &PlotTransform, axis: usize) -> Vec<f32> {
        let b = t.bounds();
        let (lo, hi) = if axis == 0 {
            (b.min()[0], b.max()[0])
        } else {
            (b.min()[1], b.max()[1])
        };
        let input = GridInput {
            bounds: (lo, hi),
            base_step_size: t.dvalue_dpos()[axis].abs() * f64::from(BASE_SPACING),
        };
        let spacer = if axis == 0 {
            &self.grid.x
        } else {
            &self.grid.y
        };
        spacer(input)
            .into_iter()
            .map(|m| {
                if axis == 0 {
                    t.position_from_point_x(m.value)
                } else {
                    t.position_from_point_y(m.value)
                }
            })
            .collect()
    }
}

/// The screen positions of the grid lines of one plot: lines down at
/// `xs` and across at `ys`, less any off the plot or on an axis or the
/// zero line.
fn layout(frame: egui::Rect, xs: &[f32], ys: &[f32], zero: Zero) -> (Vec<f32>, Vec<f32>) {
    let clear = |v: f32, axes: &[f32]| axes.iter().all(|a| (v - a).abs() > ON_AXIS);
    let (left, bottom) = axes(frame);
    let (mut down_axes, mut across_axes) = (vec![left], vec![bottom]);
    match zero {
        Zero::Down(z) if frame.x_range().contains(z) => down_axes.push(z),
        Zero::Across(z) if frame.y_range().contains(z) => across_axes.push(z),
        _ => {}
    }
    let down = xs
        .iter()
        .copied()
        .filter(|&x| frame.x_range().contains(x) && clear(x, &down_axes))
        .collect();
    let across = ys
        .iter()
        .copied()
        .filter(|&y| frame.y_range().contains(y) && clear(y, &across_axes))
        .collect();
    (down, across)
}

impl PlotItem for GridItem {
    fn shapes(&self, ui: &Ui, t: &PlotTransform, shapes: &mut Vec<Shape>) {
        let f = *t.frame();
        if self.layers != Layers::Axes {
            // The library's background, less its border.
            shapes.push(Shape::rect_filled(f, 2.0, ui.visuals().extreme_bg_color));
        }
        if let Layers::Map(m) = self.layers {
            let at = |v: [f64; 2]| t.position_from_point(&egui_plot::PlotPoint::new(v[0], v[1]));
            let whole = egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            shapes.push(Shape::image(
                m.texture,
                egui::Rect::from_two_pos(at(m.min), at(m.max)),
                whole,
                Color32::WHITE,
            ));
            return;
        }
        let zero = if self.grid.zero_y {
            Zero::Across(t.position_from_point_y(0.0))
        } else {
            Zero::None
        };
        let colours = (self.grid_c, self.axis_c);
        let grid = self.layers == Layers::All;
        let (xs, ys) = if grid {
            (self.lines(t, 0), self.lines(t, 1))
        } else {
            (Vec::new(), Vec::new())
        };
        shapes.extend(lines(f, (&xs, &ys), zero, colours, grid));
    }

    /// Nothing to point at: the grid has no values.
    fn allow_hover(&self) -> bool {
        false
    }

    fn initialize(&mut self, _x_range: std::ops::RangeInclusive<f64>) {}

    fn color(&self) -> Color32 {
        self.grid_c
    }

    fn geometry(&self) -> PlotGeometry<'_> {
        PlotGeometry::None
    }

    fn bounds(&self) -> PlotBounds {
        PlotBounds::NOTHING
    }

    fn base(&self) -> &PlotItemBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut PlotItemBase {
        &mut self.base
    }
}

#[cfg(test)]
#[path = "strength_tests.rs"]
mod tests;
