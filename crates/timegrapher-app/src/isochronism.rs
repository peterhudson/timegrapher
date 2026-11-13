//! The Isochronism pane: how the rate moves with the amplitude as the
//! mainspring lets down, from the same steadiness report as the
//! Steadiness pane (core `steadiness`, as `timegrapher series` gives it).
//! Nothing is computed here: the curve comes ready made with the report,
//! and the walk under it (#741) is core's [`walk_view`], as the chat app's
//! card draws it.

use crate::fact_box::FactBox;
use crate::strength::{self, Grid};
use crate::theme::{self, Icon};
use eframe::egui::{self, Align2, Color32, Pos2, RichText, Stroke};
use egui_plot::{Line, Plot, PlotPoint, PlotTransform, Points};
use timegrapher_core::chart_scale::{axis_number, round_step};
use timegrapher_core::figure_text::{fixed_text, true_minus};
use timegrapher_core::steadiness::IsochronismCurve;
use timegrapher_core::steady_views::{
    curve_amp, iso_bin_hover, iso_facts, iso_info, iso_key, iso_title, rate_padded, walk_amp,
    walk_axis, walk_off_scale, walk_spans, WalkDot, WalkView, ISO_TITLE, WALK_AMP_NAME,
    WALK_KEY_START, WALK_SINCE, WALK_START,
};

/// Room for the vertical axis labels, as on the Steadiness pane.
const GUTTER: f32 = 64.0;
/// Room beside the widest rate label for its tick.
const GUTTER_PAD: f32 = 12.0;

/// How the pane shows isochronism: the classic curve of rate by
/// amplitude, or the walk through rate and amplitude over time (Peter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum View {
    #[default]
    Curve,
    Walk,
}

/// The toolbar's choice of [`View`].
pub fn view_choice(ui: &mut egui::Ui, view: &mut View) {
    theme::tool_choice(
        ui,
        "View",
        "Isochronism the classic way, or as a walk through time",
        view,
        &[
            (
                View::Curve,
                Icon::IsoCurve,
                "Curve",
                "The median rate in each 2° of amplitude, with bars over its middle half",
            ),
            (
                View::Walk,
                Icon::IsoWalk,
                "Walk",
                "The rate at each amplitude as the watch runs down, each dot coloured by its \
                 time",
            ),
        ],
    );
}

/// The Walk view's note when core has no walk yet.
const NO_WALK: &str = "No walk yet: it needs the rate and amplitude over a longer stretch of \
                       the take.";

/// Draw the pane in `view`: the rate by amplitude, or the walk through
/// rate and amplitude over time when core has one. Both span the same
/// amplitudes, so a degree stays put when the view changes.
pub fn ui(
    ui: &mut egui::Ui,
    curve: Option<&IsochronismCurve>,
    walk: Option<&WalkView>,
    view: View,
) {
    let missing = "No isochronism curve yet: the amplitude has not moved enough. It needs at \
                   least three 2° steps of amplitude with 10 readings in each, which a run-down \
                   over some hours gives.";
    let frame = Frame {
        amp: curve.zip(walk).and_then(|(c, w)| shared_amp(c, w)),
        gutter: gutter(ui, curve, walk),
    };
    // Every word off the plot (Hugh's Text Off the Charts, #1507): the
    // finding in the title line, the end rates in a facts box, the dots
    // named in a key row, the rest in "i".
    if let Some(c) = curve {
        title_line(ui, c);
    }
    let walking = view == View::Walk;
    theme::with_info(
        ui,
        info_open(ui.ctx()),
        egui::Id::new("isochronism info"),
        |ui, h| info_panel(ui, curve, walk.filter(|_| walking), h),
        |ui| match (view, curve, walk) {
            (View::Curve, None, _) => note(ui, missing),
            (View::Curve, Some(c), _) => {
                let h = room(ui);
                chart(ui, c, h, frame);
            }
            (View::Walk, _, None) => note(ui, NO_WALK),
            (View::Walk, _, Some(w)) => {
                // The curve's end rates, then where the amplitude stops
                // being read while the rate goes on (core's, Hugh).
                let mut rows = curve.map(iso_facts).unwrap_or_default();
                rows.extend(w.rate_only_row.clone());
                let h = room(ui);
                walk_part(ui, w, rows, h, frame);
            }
        },
    );
}

/// The title line: [`ISO_TITLE`] and core's finding ([`iso_title`]).
fn title_line(ui: &mut egui::Ui, c: &IsochronismCurve) {
    let pal = theme::pal(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(theme::caption(ISO_TITLE).color(pal.text));
        ui.label(
            RichText::new(format!("  ·  {}", iso_title(c)))
                .small()
                .color(pal.text),
        );
    });
}

/// Where whether "i" is open is kept between frames.
fn info_id() -> egui::Id {
    egui::Id::new("isochronism info open")
}

/// Whether the pane's "i" is open.
fn info_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp(info_id())).unwrap_or_default()
}

/// The pane's toolbar: its `controls`, then "i" pinned beside the "?".
pub fn toolbar(ui: &mut egui::Ui, title: &str, controls: impl FnMut(&mut egui::Ui)) {
    let mut info = info_open(ui.ctx());
    let mut pin = |ui: &mut egui::Ui| {
        let accent = theme::pal(ui).accent;
        if theme::icon_toggle(ui, Icon::Info, info, accent)
            .on_hover_text("Info: the finding in full, and how the slope and its ± are read")
            .clicked()
        {
            info = !info;
        }
    };
    theme::pane_toolbar_pinned(
        ui,
        title,
        &crate::help::ISOCHRONISM,
        Some(&mut pin),
        controls,
    );
    ui.ctx().data_mut(|d| d.insert_temp(info_id(), info));
}

/// "i": core's paragraphs for the view ([`iso_info`]), then, in the Walk,
/// where the rate goes on with no amplitude in full; selectable, no
/// taller than `max_h`.
fn info_panel(
    ui: &mut egui::Ui,
    curve: Option<&IsochronismCurve>,
    walk: Option<&WalkView>,
    max_h: f32,
) {
    let pal = theme::pal(ui);
    theme::info_scroll(ui, egui::Id::new("isochronism info"), max_h, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(theme::caption("Isochronism").color(pal.text_secondary));
        let mut paras = curve
            .map(|c| iso_info(c, walk.is_some()))
            .unwrap_or_default();
        paras.extend(walk.and_then(|w| w.rate_only_text.clone()));
        for (i, t) in paras.iter().enumerate() {
            let colour = if i == 1 { pal.text } else { pal.text_secondary };
            ui.add(
                egui::Label::new(RichText::new(t).small().color(colour))
                    .wrap()
                    .selectable(true),
            );
        }
    });
}

/// The height left for the pane's charts, at least what one needs.
fn room(ui: &egui::Ui) -> f32 {
    ui.available_height().max(120.0)
}

/// A plot's height from what is left of `h`, never under what shows one.
fn plot_h(h: f32) -> f32 {
    h.max(40.0)
}

/// The plot's height from what is left of `h`, at most 1.1 times its
/// width so it is never stretched tall, and the room left over above it
/// that centres it (Hugh's F2).
fn shaped(ui: &egui::Ui, h: f32, gutter: f32) -> (f32, f32) {
    let h = plot_h(h);
    let most = plot_h(1.1 * (ui.available_width() - gutter));
    let shown = h.min(most);
    (shown, (h - shown) / 2.0)
}

/// An axis from `span` with round ends on a 1-2-5 step, cut into about 6
/// to 10 steps (Hugh's F3; core's [`walk_axis`]): the ends and the step.
fn round_axis(span: Span) -> (Span, f64) {
    let (lo, hi, step) = walk_axis(span);
    ((lo, hi), step)
}

/// Grid marks on `step`, or the next 1-2-5 step up where the labels
/// would sit closer than `px` points.
fn round_grid(g: egui_plot::GridInput, px: f64, step: f64) -> Vec<egui_plot::GridMark> {
    let ladder: Vec<f64> = std::iter::successors(Some(step), |s| Some(round_step(s * 1.01)))
        .take(12)
        .collect();
    theme::even_grid(g, px, &ladder)
}

/// The rounded axes over `x` and `y` and their grid, zero line across.
fn axes(x: Span, y: Span) -> (Span, Span, Grid) {
    let ((x, xs), (y, ys)) = (round_axis(x), round_axis(y));
    let grid = Grid::new(
        move |g| round_grid(g, 80.0, xs),
        move |g| round_grid(g, 24.0, ys),
    )
    .zero_y();
    (x, y, grid)
}

/// The height of a row of axis names.
fn name_row(ui: &egui::Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Small) + 2.0
}

/// The axes' names with their units, as #1052 set them (Hugh's F1):
/// "s/d" above the rate labels, the rate named in the title line
/// (#1507), and "Amp." with "°" under the amplitude labels at the right.
/// `plot` is the whole plot with its labels; `frame` the data within it.
fn axis_names(ui: &egui::Ui, plot: egui::Rect, frame: egui::Rect) {
    let pal = theme::pal(ui);
    let font = egui::TextStyle::Small.resolve(ui.style());
    let name = theme::semibold(font.size);
    let p = ui.painter();
    let right = frame.left() - 4.0;
    p.text(
        Pos2::new(right, frame.top() - font.size / 2.0 - 2.0),
        Align2::RIGHT_BOTTOM,
        "s/d",
        font.clone(),
        pal.text_secondary,
    );
    let deg = p.text(
        Pos2::new(frame.right(), plot.bottom() + 2.0),
        Align2::RIGHT_TOP,
        "°",
        font,
        pal.text_secondary,
    );
    p.text(
        Pos2::new(deg.left() - 3.0, deg.top()),
        Align2::RIGHT_TOP,
        WALK_AMP_NAME,
        name,
        pal.text,
    );
}

/// A rate on the axis: a bare number with as few decimals as the grid
/// step needs and a true minus, its unit named over the axis (Hugh's F1).
fn rate_label(v: f64, step: f64) -> String {
    true_minus(&axis_number(v, step))
}

/// What the two views share, so a degree sits at one x in either: the
/// amplitude range ([`shared_amp`]) and the rate axis's width.
#[derive(Clone, Copy)]
struct Frame {
    amp: Option<Span>,
    gutter: f32,
}

/// The rate axis's width for either view: room for the widest label at
/// the round ends of both views' rate ranges and for the axis's name,
/// never under [`GUTTER`].
fn gutter(ui: &egui::Ui, curve: Option<&IsochronismCurve>, walk: Option<&WalkView>) -> f32 {
    let spans = curve
        .and_then(curve_bounds)
        .map(|b| b.1)
        .into_iter()
        .chain(walk.map(|w| walk_bounds(w).1));
    let body = egui::TextStyle::Body.resolve(ui.style());
    let width = |text: String, font: egui::FontId| {
        ui.fonts_mut(|f| f.layout_no_wrap(text, font, Color32::WHITE).size().x)
    };
    spans
        .flat_map(|s| {
            let (lo, hi) = round_axis(s).0;
            [lo, hi]
        })
        // With a decimal, so either view's labels fit whatever step its
        // height picks.
        .map(|v| width(true_minus(&fixed_text(v, 1)), body.clone()))
        .fold(GUTTER, |g, w| g.max(w + GUTTER_PAD))
}

fn chart(ui: &mut egui::Ui, c: &IsochronismCurve, h: f32, frame: Frame) {
    let pal = theme::pal(ui);
    let Some((x_bounds, y_bounds)) = curve_bounds(c) else {
        return;
    };
    let (x_bounds, y_bounds, grid) = axes(frame.amp.unwrap_or(x_bounds), y_bounds);
    let (head, foot) = (name_row(ui), name_row(ui));
    let (h, centre) = shaped(ui, h - head - foot - key_row_h(ui), frame.gutter);
    ui.add_space(centre + head);
    let mut facts = FactBox::plain(ui, "isochronism facts", "curve", iso_facts(c), Vec::new());
    let resp =
        chart_plot("isochronism", &grid, (h, frame.gutter), x_bounds, y_bounds).show(ui, |p| {
            grid.draw(p, pal);
            facts.plot_h = p.transform().frame().height();
            // Each bar's ends, so the box keeps clear of the bars too.
            facts.tops = c
                .bins
                .iter()
                .flat_map(|b| [[b.amplitude_deg, b.rate_q25], [b.amplitude_deg, b.rate_q75]])
                .collect();
            p.add(facts.clone());
            chart_items(p, c, pal);
        });
    axis_names(ui, resp.response.rect, *resp.transform.frame());
    let at = ui
        .ctx()
        .pointer_hover_pos()
        .filter(|_| resp.response.hovered());
    if let Some(b) = at.and_then(|at| nearest_bin(c, &resp.transform, at)) {
        resp.response.on_hover_text_at_pointer(iso_bin_hover(b));
    }
    ui.add_space(foot);
    curve_key(ui, c);
}

/// The height of the key row under a plot.
fn key_row_h(ui: &egui::Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Small) + ui.spacing().item_spacing.y
}

/// The bin whose dot is nearest the pointer `at`, within [`HOVER_PX`].
fn nearest_bin<'a>(
    c: &'a IsochronismCurve,
    t: &PlotTransform,
    at: Pos2,
) -> Option<&'a timegrapher_core::steadiness::AmplitudeBin> {
    c.bins
        .iter()
        .map(|b| {
            let p = t.position_from_point(&PlotPoint::new(b.amplitude_deg, b.rate_s_per_day));
            (b, p.distance(at))
        })
        .filter(|&(_, d)| d <= HOVER_PX)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(b, _)| b)
}

/// The key row under the Curve: a dot on its bar, as the plot draws
/// them, and core's name for them ([`iso_key`], Hugh, #1507).
fn curve_key(ui: &mut egui::Ui, c: &IsochronismCurve) {
    let pal = theme::pal(ui);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 12.0), egui::Sense::hover());
        let colour = pal.trace_rate;
        ui.painter().line_segment(
            [r.center_top(), r.center_bottom()],
            Stroke::new(strength::DATA_W, colour),
        );
        ui.painter().circle_filled(r.center(), 3.0, colour);
        ui.label(RichText::new(iso_key(c)).small().color(pal.text_secondary));
    });
}

/// A low and high bound on one axis.
type Span = (f64, f64);

/// The chart's amplitude and rate bounds, from its bins; none with no
/// bins.
fn curve_bounds(c: &IsochronismCurve) -> Option<(Span, Span)> {
    // Scale to the bars, padded by 15% as the charts are.
    let (lo, hi) = c
        .bins
        .iter()
        .flat_map(|b| [b.rate_q25, b.rate_q75])
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
            (l.min(v), h.max(v))
        });
    let amps: Vec<f64> = c.bins.iter().map(|b| b.amplitude_deg).collect();
    Some((curve_amp(&amps, c.bin_deg)?, rate_padded(lo, hi)))
}

/// The chart's plot, `id`, fixed in place, `h` high over the given bounds, on
/// `grid`.
fn chart_plot(
    id: &str,
    grid: &Grid,
    (h, gutter): (f32, f32),
    (x_lo, x_hi): Span,
    (y_lo, y_hi): Span,
) -> Plot<'static> {
    grid.on(Plot::new(id))
        .allow_scroll(false)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_axis_zoom_drag(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .show_x(false)
        .show_y(false)
        .y_axis_min_width(gutter)
        .height(h)
        .x_axis_formatter(|m, r| amp_label(m.value, r))
        .y_axis_formatter(|m, _| rate_label(m.value, m.step_size))
        .default_x_bounds(x_lo, x_hi)
        .default_y_bounds(y_lo, y_hi)
}

/// An amplitude axis label, a bare number with its unit named under the
/// axis (Hugh's F1), or none within 3% of either end: past the right it
/// runs off the plot, and at the left it sits on the lowest rate label in
/// the corner (Walter's walk of #1133).
fn amp_label(v: f64, range: &std::ops::RangeInclusive<f64>) -> String {
    let edge = 0.03;
    if theme::near_an_end(v, range, edge, edge) {
        String::new()
    } else {
        format!("{v:.0}")
    }
}

/// The bins' middle halves and their medians.
fn chart_items(p: &mut egui_plot::PlotUi, c: &IsochronismCurve, pal: &theme::Palette) {
    let colour = pal.trace_rate;
    for b in &c.bins {
        p.line(
            Line::new(
                "middle half",
                vec![[b.amplitude_deg, b.rate_q25], [b.amplitude_deg, b.rate_q75]],
            )
            .color(colour)
            .width(strength::DATA_W),
        );
    }
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
}

/// How near the pointer must be to a dot, in points, for its hover.
const HOVER_PX: f32 = 8.0;

/// The walk: its hint, the plot of rate against amplitude with each dot
/// coloured by its time, and the colour key, `h` high in all. `amp` is
/// the amplitude range and axis width it shares with the curve.
fn walk_part(ui: &mut egui::Ui, w: &WalkView, rows: Vec<(String, String)>, h: f32, frame: Frame) {
    let pal = theme::pal(ui);
    let (head, foot) = (name_row(ui), name_row(ui));
    let (h, centre) = shaped(ui, h - key_row_h(ui) - head - foot, frame.gutter);
    ui.add_space(centre + head);
    let (x, y) = walk_bounds(w);
    let (x, y, grid) = axes(frame.amp.unwrap_or(x), y);
    let off_scale = walk_off_scale(&w.dots, x, y);
    let mut facts = FactBox::plain(ui, "isochronism facts", "walk", rows, Vec::new());
    let resp = chart_plot("isochronism walk", &grid, (h, frame.gutter), x, y).show(ui, |p| {
        grid.draw(p, pal);
        facts.plot_h = p.transform().frame().height();
        facts.tops = w.dots.iter().map(dot_at).collect();
        p.add(facts.clone());
    });
    paint_walk(ui, &resp.transform, w);
    axis_names(ui, resp.response.rect, *resp.transform.frame());
    ui.add_space(foot);
    let at = ui
        .ctx()
        .pointer_hover_pos()
        .filter(|_| resp.response.hovered());
    if let Some(i) = at.and_then(|at| nearest_dot(w, &resp.transform, at)) {
        resp.response
            .on_hover_text_at_pointer(true_minus(&w.dots[i].hover));
    }
    let dark = ui.visuals().dark_mode;
    ui.horizontal(|ui| {
        let small = |t: &str| RichText::new(t).small().color(pal.text_secondary);
        ui.label(small(WALK_KEY_START));
        crate::tfmap::ramp(ui, 120.0, |t| crate::tfmap::time_colour(dark, t));
        ui.label(small(&w.last_text));
        ui.label(small(WALK_SINCE));
        if let Some(key) = &w.thin_key {
            // A full dot beside a faded one, so the fade reads against
            // its own colour, then the name (Hugh, 13 Nov).
            ui.add_space(12.0);
            let full = crate::tfmap::time_colour(dark, 0.5);
            let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 10.0), egui::Sense::hover());
            let p = ui.painter();
            p.circle_filled(r.left_center() + egui::vec2(KEY_DOT, 0.0), KEY_DOT, full);
            p.circle_filled(
                r.right_center() - egui::vec2(KEY_DOT, 0.0),
                KEY_DOT,
                full.gamma_multiply(THIN_ALPHA),
            );
            ui.label(small(key));
        }
        if let Some(off) = &off_scale {
            ui.add_space(12.0);
            ui.label(small(off));
        }
    });
}

/// One amplitude range for the curve and the walk: the span of both
/// (core's [`walk_amp`]).
fn shared_amp(c: &IsochronismCurve, w: &WalkView) -> Option<Span> {
    let (amp, _) = curve_bounds(c)?;
    Some(walk_amp(&w.dots, Some(amp)))
}

/// The walk's amplitude and rate bounds, padded as the curve's are
/// (core's [`walk_spans`]).
fn walk_bounds(w: &WalkView) -> (Span, Span) {
    let [amp, rate] = walk_spans(&w.dots);
    (amp, rate)
}

/// The walk as Hugh's look C draws it (Peter, 11-05): a 2 pt trail
/// through the dots in time order, each step shading from its first dot's
/// colour to its last's and lifted wherever core says a dot is not joined
/// (a stretch left out or a break: never filled); the dots over it; a
/// ring at each of core's hour beads; a ring at the start and a large dot
/// at the end; then their names, over everything. A dot core calls thin
/// (read from under 60% of its windows), and the steps to and from it,
/// are drawn at [`THIN_ALPHA`].
fn paint_walk(ui: &egui::Ui, t: &PlotTransform, w: &WalkView) {
    let (pal, dark) = (theme::pal(ui), ui.visuals().dark_mode);
    let bg = ui.visuals().extreme_bg_color;
    let colour = |when: f64| crate::tfmap::time_colour(dark, when as f32);
    let faint = |c: egui::Color32, thin: bool| {
        if thin {
            c.gamma_multiply(THIN_ALPHA)
        } else {
            c
        }
    };
    let pos = |p: [f64; 2]| t.position_from_point(&PlotPoint::new(p[0], p[1]));
    let dots: Vec<Pos2> = w.dots.iter().map(|d| pos(dot_at(d))).collect();
    // The trail stops at the frame; only the dots spill their radius past
    // it (Hugh, 13 Nov: a step to a dot off the scale ran to the pane's
    // edge).
    let lines = ui.painter().with_clip_rect(*t.frame());
    let p = ui.painter().with_clip_rect(t.frame().expand(8.0));
    // Thin steps and dots first, so the full ones draw over them (Hugh).
    let mut steps = steps(w);
    steps.sort_by_key(|s| !s.3);
    let frame = *t.frame();
    let mut drawn = Vec::new();
    for (from, to, (t0, t1), thin) in steps {
        // A step into or out of a dot off the scale is a stub from the
        // dot on it (Hugh, 13 Nov: a full step read as data).
        let Some((a, b, (t0, t1))) = drawn_step(frame, pos(from), pos(to), (t0, t1)) else {
            continue;
        };
        drawn.push((a, b));
        // In short runs, so its colour moves from one dot's to the next's.
        let runs = 8;
        for k in 0..runs {
            let f = |k: usize| k as f32 / runs as f32;
            let mid = (f(k) + f(k + 1)) as f64 / 2.0;
            lines.line_segment(
                [a.lerp(b, f(k)), a.lerp(b, f(k + 1))],
                Stroke::new(2.0_f32, faint(colour(t0 + (t1 - t0) * mid), thin)),
            );
        }
    }
    let mut order: Vec<usize> = (0..w.dots.len()).collect();
    order.sort_by_key(|&i| !w.dots[i].thin);
    for (d, at) in order.iter().map(|&i| (&w.dots[i], &dots[i])) {
        p.circle_filled(*at, 2.5, faint(colour(d.when), d.thin));
    }
    let ring = |at: Pos2, r: f32, when: f64| {
        p.circle(at, r, bg, Stroke::new(2.5_f32, colour(when)));
    };
    for b in &w.beads {
        ring(dots[b.dot], 5.0, w.dots[b.dot].when);
    }
    let (Some(start), Some(end)) = (dots.first(), dots.last()) else {
        return;
    };
    ring(*start, START_RING, 0.0);
    p.circle(*end, END_DOT, colour(1.0), Stroke::new(2.0_f32, bg));
    let font = egui::FontId::proportional(12.0);
    // Each name on a halo of the plot's background, so a trail that
    // crosses it where the walk knots leaves it readable (Walter's walk of
    // #1148).
    let galley = |text: &str| p.layout_no_wrap(text.to_string(), font.clone(), pal.text);
    // Opaque, so a trail under a name never shows through it (Hugh,
    // 13 Nov: "Start" sat on the purple trail).
    let name = |r: egui::Rect, g| {
        p.rect_filled(r.expand(NAME_PAD), 3.0, bg);
        p.galley(r.min, g, pal.text);
    };
    // Kept inside the plot, so a name near an edge never sits on an axis
    // label (Walter's walk of #1496: "8.4 h" on "120").
    let inside = frame.shrink(2.0);
    // The start's and the end's names where they cover no dot, stub or
    // step (Hugh, #1542: the end's name hid a stub).
    // Each mark's outer edge: the start's ring and the end's dot with
    // half their strokes.
    let marks = [
        (*start, START_RING + 1.25, WALK_START),
        (*end, END_DOT + 1.0, w.last_text.as_str()),
    ];
    for (at, radius, text) in marks {
        let g = galley(text);
        name(place_name(at, radius, g.size(), frame, &dots, &drawn), g);
    }
    for b in &w.beads {
        let (side, align) = bead_side(&dots, b.dot);
        let g = galley(&b.text);
        name(
            keep_inside(
                align.anchor_size(dots[b.dot] + 11.0 * side, g.size()),
                inside,
            ),
            g,
        );
    }
}

/// Where a bead's name goes: square to the trail at dot `i`, on the side
/// clear of the other dots, so it never sits on the line; and how it is
/// anchored there.
fn bead_side(dots: &[Pos2], i: usize) -> (egui::Vec2, Align2) {
    let along = dots[(i + 1).min(dots.len() - 1)] - dots[i.saturating_sub(1)];
    let n = egui::vec2(-along.y, along.x).normalized();
    let n = if n.is_finite() {
        n
    } else {
        egui::vec2(0.0, -1.0)
    };
    let room = |n: egui::Vec2| {
        let at = dots[i] + 16.0 * n;
        dots.iter()
            .map(|d| d.distance(at))
            .fold(f32::INFINITY, f32::min)
    };
    let n = if room(-n) > room(n) { -n } else { n };
    // Anchored by the side it leans to: a component past half is a lean.
    let side = |v: f32| (v.round() + 1.0) as usize;
    let x = [egui::Align::RIGHT, egui::Align::Center, egui::Align::LEFT][side(n.x)];
    let y = [egui::Align::BOTTOM, egui::Align::Center, egui::Align::TOP][side(n.y)];
    (n, Align2([x, y]))
}

/// A dot's place on the plot: its amplitude and rate.
fn dot_at(d: &WalkDot) -> [f64; 2] {
    [d.amplitude_deg, d.rate_s_per_day]
}

/// The stubs drawn to dot `i`, off the scale: from each dot in `frame`
/// that a step joins it to ([`stub_toward`]).
fn stub_ends<'a>(
    w: &'a WalkView,
    dots: &'a [Pos2],
    frame: egui::Rect,
    i: usize,
) -> impl Iterator<Item = (Pos2, Pos2)> + 'a {
    let before = (i > 0 && w.dots[i].joined).then(|| i - 1);
    let after = (i + 1 < dots.len() && w.dots[i + 1].joined).then_some(i + 1);
    [before, after].into_iter().flatten().filter_map(move |j| {
        frame
            .contains(dots[j])
            .then(|| stub_toward(frame, dots[j], dots[i]).map(|end| (dots[j], end)))
            .flatten()
    })
}

/// How far `at` is from the segment `a` to `b`.
fn segment_distance(a: Pos2, b: Pos2, at: Pos2) -> f32 {
    let ab = b - a;
    let f = if ab.length_sq() > 0.0 {
        ((at - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * f).distance(at)
}

/// How long a step into or out of a dot off the scale is drawn, points:
/// enough to show which way it goes, short of reading as data (Hugh,
/// 13 Nov).
const STUB_PT: f32 = 16.0;

/// Where a step from `from` toward `to` ends: at `to` when it lies in
/// `frame`, a [`STUB_PT`] stub toward it when only `from` does, and no
/// step when neither does.
fn stub_toward(frame: egui::Rect, from: Pos2, to: Pos2) -> Option<Pos2> {
    if frame.contains(to) {
        return Some(to);
    }
    frame
        .contains(from)
        .then(|| from + (to - from).normalized() * STUB_PT.min(from.distance(to)))
}

/// The part of a step from `from` to `to`, with times `(t0, t1)`, that
/// is drawn in `frame`: its ends ([`stub_toward`] each way) and the times
/// they sit at ([`span_of`]), or `None` when none is drawn.
fn drawn_step(
    frame: egui::Rect,
    from: Pos2,
    to: Pos2,
    times: (f64, f64),
) -> Option<(Pos2, Pos2, (f64, f64))> {
    let b = stub_toward(frame, from, to)?;
    let a = stub_toward(frame, to, from)?;
    Some((a, b, span_of(from, to, a, b, times)?))
}

/// The times along a step from `from` to `to` that its drawn part, `a`
/// to `b`, covers, given the step's own `(t0, t1)`.
fn span_of(from: Pos2, to: Pos2, a: Pos2, b: Pos2, (t0, t1): (f64, f64)) -> Option<(f64, f64)> {
    let len = from.distance(to);
    let at = |q: Pos2| (q.distance(from) / len) as f64;
    (len > 0.0).then(|| (t0 + (t1 - t0) * at(a), t0 + (t1 - t0) * at(b)))
}

/// The start's ring's and the end's dot's radii, points.
const START_RING: f32 = 7.0;
const END_DOT: f32 = 6.5;

/// How far a name's box reaches past its text, points.
const NAME_PAD: f32 = 3.0;

/// How far inside the plot's frame a name's box must sit, points, so it
/// never meets an axis (Hugh, #1542: "7.8 h" broke the y-axis line).
const NAME_FRAME_PT: f32 = 4.0;

/// The room between a name's box and its own mark, points (Hugh, #1542:
/// the "h" touched the end's dot).
const NAME_MARK_PT: f32 = 3.0;

/// Where a name `size` big goes beside its mark at `at`, `radius` to its
/// outer edge (Hugh, #1542): to the right, the left, above or below, its
/// box [`NAME_MARK_PT`] clear of the mark. A place counts only when its
/// box lies [`NAME_FRAME_PT`] inside `frame`; of those, the first that
/// covers no dot of `dots` and no drawn step of `steps`, else the one that
/// covers the fewest. With none inside, the first, moved inside.
fn place_name(
    at: Pos2,
    radius: f32,
    size: egui::Vec2,
    frame: egui::Rect,
    dots: &[Pos2],
    steps: &[(Pos2, Pos2)],
) -> egui::Rect {
    let gap = radius + NAME_MARK_PT + NAME_PAD;
    let room = frame.shrink(NAME_FRAME_PT + NAME_PAD);
    let places = [
        (Align2::LEFT_CENTER, egui::vec2(gap, 0.0)),
        (Align2::RIGHT_CENTER, egui::vec2(-gap, 0.0)),
        (Align2::CENTER_BOTTOM, egui::vec2(0.0, -gap)),
        (Align2::CENTER_TOP, egui::vec2(0.0, gap)),
    ]
    .map(|(align, off)| align.anchor_size(at + off, size));
    let covered = |r: &egui::Rect| {
        let r = r.expand(NAME_PAD);
        dots.iter().filter(|d| r.contains(**d)).count()
            + steps
                .iter()
                .filter(|(a, b)| crate::marks::crosses(r, &[*a, *b]))
                .count()
    };
    // `min_by_key` keeps the first of equals, so a clear place in the
    // order above wins.
    places
        .into_iter()
        .filter(|r| room.contains_rect(*r))
        .min_by_key(covered)
        .unwrap_or_else(|| keep_inside(places[0], room))
}

/// `r` moved the least way to lie inside `frame`, where it fits.
fn keep_inside(r: egui::Rect, frame: egui::Rect) -> egui::Rect {
    let dx = (frame.left() - r.left()).max(0.0) + (frame.right() - r.right()).min(0.0);
    let dy = (frame.top() - r.top()).max(0.0) + (frame.bottom() - r.bottom()).min(0.0);
    r.translate(egui::vec2(dx, dy))
}

/// How strongly a thin dot and its steps are drawn, of their full colour.
pub const THIN_ALPHA: f32 = 0.35;

/// The radius of the key's two dots, full and thin, points.
const KEY_DOT: f32 = 4.0;

/// A step of the walk: where it goes from and to, the two dots' times, and
/// whether either dot is thin.
type Step = ([f64; 2], [f64; 2], (f64, f64), bool);

/// The walk's steps from each dot to the next one core joins to it.
fn steps(w: &WalkView) -> Vec<Step> {
    w.dots
        .windows(2)
        .filter(|p| p[1].joined)
        .map(|p| {
            (
                dot_at(&p[0]),
                dot_at(&p[1]),
                (p[0].when, p[1].when),
                p[0].thin || p[1].thin,
            )
        })
        .collect()
}

/// The dot nearest the pointer, within [`HOVER_PX`].
fn nearest_dot(w: &WalkView, t: &egui_plot::PlotTransform, at: egui::Pos2) -> Option<usize> {
    let pos =
        |d: &WalkDot| t.position_from_point(&PlotPoint::new(d.amplitude_deg, d.rate_s_per_day));
    let dots: Vec<Pos2> = w.dots.iter().map(pos).collect();
    nearest_of(w, &dots, *t.frame(), at)
}

/// [`nearest_dot`] on the dots' places on screen, `dots`, in `frame`.
fn nearest_of(w: &WalkView, dots: &[Pos2], frame: egui::Rect, at: Pos2) -> Option<usize> {
    let nearest = |found: &mut dyn Iterator<Item = (usize, f32)>| {
        found
            .filter(|&(_, dist)| dist <= HOVER_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let on = |i: &usize| frame.contains(dots[*i]);
    // A dot on the plot first; then a dot off the scale, anywhere along
    // its stubs (Hugh, 13 Nov), since a stub starts on a dot of its own.
    nearest(
        &mut (0..dots.len())
            .filter(on)
            .map(|i| (i, dots[i].distance(at))),
    )
    .or_else(|| {
        nearest(&mut (0..dots.len()).filter(|i| !on(i)).filter_map(|i| {
            stub_ends(w, dots, frame, i)
                .map(|(a, b)| (i, segment_distance(a, b, at)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
        }))
    })
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
    use timegrapher_core::steady_views::ISO_NO_AMP_AFTER;

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
            s_per_day_per_deg: Some(0.4),
            s_per_day_per_deg_se: Some(0.02),
            rate_change_s_per_day: Some(12.0),
            reliability: Some(0.9),
            headline: "The rate falls as the amplitude does.".into(),
        }
    }

    #[test]
    fn draws_a_curve_and_says_why_when_it_is_missing() {
        let curve = run_down();
        // The bins only: no fitted curve and no turn (Peter, 13 Nov).
        // The key under the plot names the dots, and the title line
        // carries the slope and its amplitudes (#1507).
        assert_eq!(iso_key(&curve), "Median rate in each 2° of amplitude (8)");
        assert!(
            !iso_key(&curve).contains("per degree"),
            "{}",
            iso_key(&curve)
        );
        assert_eq!(
            iso_title(&curve),
            "+0.40 ± 0.02 s/d per degree, 236° to 264°"
        );
        // Bare numbers, a true minus, no minus zero (Hugh's F1).
        assert_eq!(rate_label(-0.0001, 0.5), "0.0");
        assert_eq!(rate_label(0.0, 0.5), "0.0");
        assert_eq!(rate_label(1.5, 0.5), "1.5");
        assert_eq!(rate_label(-2.0, 1.0), "−2");

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
                egui::CentralPanel::default().show(ctx, |u| super::ui(u, c, None, View::Curve));
            });
            assert!(!out.shapes.is_empty());
        }
    }

    /// The curve view draws the bins' bars and medians and nothing fitted
    /// through them: no line, and no turn named (Peter, 13 Nov).
    #[test]
    fn the_curve_view_draws_no_fitted_curve_and_no_turn() {
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
            egui::CentralPanel::default()
                .show(ctx, |u| super::ui(u, Some(&curve), None, View::Curve));
        });
        let texts: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
        assert!(!texts.iter().any(|t| t.contains("Turn")), "{texts:?}");
        // Each bar is a two-point line; a fitted curve had 61 points.
        let longest = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Path(p) => Some(p.points.len()),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        assert!(longest < 10, "a line of {longest} points");
    }

    /// A walk on core's 5 min bins: six points, then a stretch left out,
    /// then three more.
    fn walk() -> WalkView {
        use timegrapher_core::walk::{IsochronismWalk, WalkPoint, WALK_BIN_S};
        let at = |k: usize| WalkPoint {
            t_s: (k as f64 + 0.5) * WALK_BIN_S,
            wind_h: 0.0,
            rate_s_per_day: 2.0 - 0.3 * k as f64,
            amplitude_deg: 290.0 - 4.0 * k as f64,
            readings: 30,
            amp_windows: 150,
            ..Default::default()
        };
        let w = IsochronismWalk {
            bin_s: WALK_BIN_S,
            points: (0..6).chain(9..12).map(at).collect(),
            ..Default::default()
        };
        timegrapher_core::steady_views::walk_view(&w).expect("nine points")
    }

    /// The line is lifted where core says a dot is not joined, never
    /// bridging the stretch left out (#741).
    #[test]
    fn the_walk_line_breaks_where_core_says() {
        let w = walk();
        let at = |i: usize| [w.dots[i].amplitude_deg, w.dots[i].rate_s_per_day];
        let s = steps(&w);
        // Five steps through the first six dots, none across to the
        // seventh, two through the last three.
        assert_eq!(s.len(), 7);
        assert!(!s.iter().any(|st| st.0 == at(5) && st.1 == at(6)));
        assert_eq!((s[5].0, s[5].1), (at(6), at(7)));
        // Each shading from the time of the dot it goes from to the
        // next's (Hugh's look C, from Peter's colour by the dot it leaves).
        for (st, from) in s.iter().zip([0, 1, 2, 3, 4, 6, 7]) {
            assert_eq!(st.2, (w.dots[from].when, w.dots[from + 1].when));
        }
    }

    /// A step to a dot off the scale is a 16 pt stub from the dot on
    /// it; with neither on it, no step (Hugh, 13 Nov).
    #[test]
    fn a_step_off_the_scale_is_a_stub() {
        let frame = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
        let (a, b) = (egui::pos2(50.0, 50.0), egui::pos2(250.0, 50.0));
        assert_eq!(stub_toward(frame, a, b), Some(egui::pos2(66.0, 50.0)));
        assert_eq!(stub_toward(frame, b, a), Some(a));
        assert_eq!(stub_toward(frame, b, egui::pos2(300.0, 0.0)), None);
        // The stub's colours are its share of the step's times.
        let (t0, t1) = span_of(a, b, a, egui::pos2(66.0, 50.0), (0.0, 1.0)).unwrap();
        assert_eq!(t0, 0.0);
        assert!((t1 - 0.08).abs() < 1e-6, "{t1}");
    }

    /// An off-scale dot's stubs come from each dot a drawn step joins it
    /// to, never across a break (Walter's walk of #1496), and its hover
    /// answers along them.
    #[test]
    fn an_off_scale_dot_has_a_stub_from_each_joined_neighbour() {
        let frame = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
        let mut w = long_walk();
        w.dots.truncate(3);
        let dots = [
            egui::pos2(10.0, 50.0),
            egui::pos2(250.0, 50.0),
            egui::pos2(50.0, 50.0),
        ];
        let ends = |w: &WalkView| stub_ends(w, &dots, frame, 1).collect::<Vec<_>>();
        w.dots[1].joined = true;
        w.dots[2].joined = true;
        assert_eq!(
            ends(&w),
            [
                (dots[0], egui::pos2(26.0, 50.0)),
                (dots[2], egui::pos2(66.0, 50.0))
            ]
        );
        w.dots[1].joined = false;
        assert_eq!(ends(&w), [(dots[2], egui::pos2(66.0, 50.0))]);
        w.dots[2].joined = false;
        assert!(ends(&w).is_empty());
        assert_eq!(
            segment_distance(dots[2], egui::pos2(66.0, 50.0), egui::pos2(60.0, 53.0)),
            3.0
        );
    }

    /// A name near the plot's edge is moved the least way to lie inside
    /// it (Walter's walk of #1496: "8.4 h" on "120").
    #[test]
    fn a_name_is_kept_inside_the_plot() {
        let frame = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
        let r =
            |x: f32, y: f32| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(20.0, 10.0));
        assert_eq!(keep_inside(r(-5.0, 95.0), frame), r(0.0, 90.0));
        assert_eq!(keep_inside(r(90.0, -3.0), frame), r(80.0, 0.0));
        assert_eq!(keep_inside(r(40.0, 40.0), frame), r(40.0, 40.0));
    }

    /// The start's and the end's names go right, left, above or below
    /// their dot, in the first place clear of dots and steps; else where
    /// they cover the fewest (Hugh, #1542).
    #[test]
    fn a_name_takes_the_first_clear_place_beside_its_dot() {
        let frame = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 200.0));
        let (at, size) = (egui::pos2(100.0, 100.0), egui::vec2(30.0, 12.0));
        let right = egui::Rect::from_min_size(egui::pos2(111.0, 94.0), size);
        let left = egui::Rect::from_min_size(egui::pos2(59.0, 94.0), size);
        let above = egui::Rect::from_min_size(egui::pos2(85.0, 77.0), size);
        let below = egui::Rect::from_min_size(egui::pos2(85.0, 111.0), size);
        let place =
            |dots: &[Pos2], steps: &[(Pos2, Pos2)]| place_name(at, 5.0, size, frame, dots, steps);
        assert_eq!(place(&[], &[]), right);
        // A stub running right from the dot (Walter's walk of #1496).
        let stub_right = (at, egui::pos2(116.0, 100.0));
        assert_eq!(place(&[], &[stub_right]), left);
        let stub_left = (at, egui::pos2(84.0, 100.0));
        assert_eq!(place(&[], &[stub_right, stub_left]), above);
        let up = (at, egui::pos2(100.0, 70.0));
        assert_eq!(place(&[], &[stub_right, stub_left, up]), below);
        // None clear: the fewest covered, the first of equals.
        let down = (at, egui::pos2(100.0, 130.0));
        let dot_left = egui::pos2(70.0, 100.0);
        assert_eq!(
            place(&[dot_left], &[stub_right, stub_left, up, down]),
            right
        );
        // A place counts only with its box 4 pt inside the frame (Hugh:
        // "7.8 h" on the left broke the y-axis): near the left edge with a
        // stub to the right, above.
        let near = egui::pos2(45.0, 100.0);
        let stub = (near, egui::pos2(61.0, 100.0));
        let r = place_name(near, 5.0, size, frame, &[], &[stub]);
        assert_eq!(r, egui::Rect::from_min_size(egui::pos2(30.0, 77.0), size));
        // In a corner with no place inside, the first, moved inside.
        let edge = place_name(egui::pos2(195.0, 195.0), 5.0, size, frame, &[], &[]);
        assert!(frame.shrink(7.0).contains_rect(edge), "{edge:?}");
        // The box clears its mark by 3 pt: radius, then 3 pt, then the
        // box's own 3 pt.
        assert_eq!(place(&[], &[]).left() - at.x, 5.0 + 3.0 + 3.0);
    }

    /// A step to a dot off the scale is drawn as its stub, coloured over
    /// the stub's own share of the step's times (Hugh, 13 Nov).
    #[test]
    fn a_drawn_stub_takes_its_share_of_the_times() {
        let frame = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
        let (on, off) = (egui::pos2(50.0, 50.0), egui::pos2(250.0, 50.0));
        let (a, b, (t0, t1)) = drawn_step(frame, on, off, (0.0, 1.0)).unwrap();
        assert_eq!((a, b, t0), (on, egui::pos2(66.0, 50.0), 0.0));
        assert!((t1 - 0.08).abs() < 1e-6, "{t1}");
        // Into the plot from off it: the last share.
        let (a, b, (t0, t1)) = drawn_step(frame, off, on, (0.0, 1.0)).unwrap();
        assert_eq!((a, b, t1), (egui::pos2(66.0, 50.0), on, 1.0));
        assert!((t0 - 0.92).abs() < 1e-6, "{t0}");
        assert_eq!(
            drawn_step(frame, off, egui::pos2(300.0, 0.0), (0.0, 1.0)),
            None
        );
    }

    /// The hover on a stub answers for its dot off the scale; at the dot's
    /// own place, off the plot, nothing (Hugh, 13 Nov).
    #[test]
    fn the_hover_on_a_stub_answers_for_its_dot() {
        let frame = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
        let mut w = long_walk();
        w.dots.truncate(2);
        w.dots[1].joined = true;
        let dots = [egui::pos2(50.0, 50.0), egui::pos2(250.0, 50.0)];
        assert_eq!(
            nearest_of(&w, &dots, frame, egui::pos2(64.0, 52.0)),
            Some(1)
        );
        assert_eq!(
            nearest_of(&w, &dots, frame, egui::pos2(51.0, 50.0)),
            Some(0)
        );
        assert_eq!(nearest_of(&w, &dots, frame, egui::pos2(250.0, 50.0)), None);
        w.dots[1].joined = false;
        assert_eq!(nearest_of(&w, &dots, frame, egui::pos2(64.0, 52.0)), None);
    }

    /// A dot core calls thin is drawn faint, and so are the steps to and
    /// from it; the rest at full colour (Norm's floors, 11-12).
    #[test]
    fn a_thin_dot_and_its_steps_are_drawn_faint() {
        let mut w = walk();
        w.dots[2].thin = true;
        let thin: Vec<bool> = steps(&w).iter().map(|s| s.3).collect();
        assert_eq!(thin, [false, true, true, false, false, false, false]);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let dark = ctx.style().visuals.dark_mode;
        let colour = |when: f64| crate::tfmap::time_colour(dark, when as f32);
        let out = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |u| {
                super::ui(u, Some(&run_down()), Some(&w), View::Walk);
            });
        });
        let fills: Vec<egui::Color32> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Circle(c) => Some(c.fill),
                _ => None,
            })
            .collect();
        let faint = colour(w.dots[2].when).gamma_multiply(THIN_ALPHA);
        assert!(fills.contains(&faint), "{fills:?}");
        assert!(!fills.contains(&colour(w.dots[2].when)), "{fills:?}");
        assert!(fills.contains(&colour(w.dots[3].when)), "{fills:?}");
        // The key names the faint mark, and the facts box core's line on
        // a rate read past the last point; neither when core gives none.
        let texts = |w: &WalkView| -> Vec<String> {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |u| {
                    super::ui(u, Some(&run_down()), Some(w), View::Walk);
                });
            })
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
        };
        w.thin_key = Some("Thin: under 60% read".into());
        w.rate_only_row = Some((ISO_NO_AMP_AFTER.into(), "1 h".into()));
        let t = texts(&w);
        assert!(t.iter().any(|t| t == "Thin: under 60% read"), "{t:?}");
        // The key draws a full and a faded dot side by side, at the key's
        // size, in the same mid colour (Hugh, 13 Nov).
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let dark = ctx.style().visuals.dark_mode;
        let out = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |u| {
                super::ui(u, Some(&run_down()), Some(&w), View::Walk);
            });
        });
        let key: Vec<egui::Color32> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Circle(c) if c.radius == KEY_DOT => Some(c.fill),
                _ => None,
            })
            .collect();
        let mid = crate::tfmap::time_colour(dark, 0.5);
        assert_eq!(key, [mid, mid.gamma_multiply(THIN_ALPHA)]);
        // The rate read past the last point is the facts box's last row
        // (Hugh's spec, 13 Nov).
        assert!(t.iter().any(|t| t == ISO_NO_AMP_AFTER), "{t:?}");
        assert!(t.iter().any(|t| t == "1 h"), "{t:?}");
        let t = texts(&walk());
        assert!(!t.iter().any(|t| t.starts_with("Thin:")), "{t:?}");
        assert!(!t.iter().any(|t| t.starts_with("Off the scale")), "{t:?}");
        // A faint dot far off the scale is counted in the key (Hugh,
        // 13 Nov).
        w.dots[2].amplitude_deg += 1000.0;
        let t = texts(&w);
        assert!(t.iter().any(|t| t == "Off the scale: 1"), "{t:?}");
    }

    /// The Walk view draws the walk with core's hint, its dots in viridis
    /// by time, the key's ends and core's hover on a dot.
    #[test]
    fn the_walk_view_draws_the_walk() {
        let (curve, w) = (run_down(), walk());
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.style_mut(|s| {
            s.interaction.show_tooltips_only_when_still = false;
            s.interaction.tooltip_delay = 0.0;
        });
        let dark = ctx.style().visuals.dark_mode;
        let colour = |when: f64| crate::tfmap::time_colour(dark, when as f32);
        let first = colour(w.dots[0].when);
        // A first frame finds the first dot; the pointer then rests on it
        // until its hover shows.
        let mut at = None;
        let mut out = None;
        for _ in 0..5 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 700.0),
                )),
                events: at.map(egui::Event::PointerMoved).into_iter().collect(),
                ..Default::default()
            };
            let o = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |u| {
                    super::ui(u, Some(&curve), Some(&w), View::Walk);
                });
            });
            at = o.shapes.iter().find_map(|c| match &c.shape {
                egui::Shape::Circle(c) if c.fill == first => Some(c.center),
                _ => None,
            });
            out = Some(o);
        }
        let out = out.unwrap();
        let texts: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
        for want in [WALK_KEY_START, w.last_text.as_str(), WALK_SINCE] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        assert!(
            texts.iter().any(|t| t == &w.dots[0].hover),
            "the first dot's hover: {texts:?}"
        );
        let fills: Vec<egui::Color32> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Circle(c) => Some(c.fill),
                _ => None,
            })
            .collect();
        // Each dot on the time map at its time; the last at the key's end.
        assert_eq!(w.dots.last().map(|d| d.when), Some(1.0));
        for d in &w.dots {
            assert!(fills.contains(&colour(d.when)), "{fills:?}");
        }
    }

    /// The curve and the walk under it share one amplitude range, so a
    /// degree sits at the same x in both (Walter's walk of #1111).
    #[test]
    fn the_curve_and_the_walk_share_the_amplitude_axis() {
        let (curve, w) = (run_down(), walk());
        let ((c_lo, c_hi), _) = curve_bounds(&curve).unwrap();
        let ((w_lo, w_hi), _) = walk_bounds(&w);
        assert_ne!((c_lo, c_hi), (w_lo, w_hi), "the test needs two ranges");
        assert_eq!(
            shared_amp(&curve, &w),
            Some((c_lo.min(w_lo), c_hi.max(w_hi)))
        );
        let ctx = egui::Context::default();
        theme::install(&ctx);
        // The amplitude labels each view draws, and where.
        let labels = |view: View| {
            let mut texts: Vec<(String, f32)> = Vec::new();
            for _ in 0..2 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 700.0),
                    )),
                    ..Default::default()
                };
                let out = ctx.run(input, |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |u| super::ui(u, Some(&curve), Some(&w), view));
                });
                texts = out
                    .shapes
                    .iter()
                    .filter_map(|c| match &c.shape {
                        // The amplitude labels: bare numbers, the only
                        // ones over 100 here.
                        egui::Shape::Text(t)
                            if t.galley.text().parse::<f64>().is_ok_and(|v| v >= 100.0) =>
                        {
                            Some((
                                t.galley.text().to_string(),
                                t.galley.rect.translate(t.pos.to_vec2()).center().x,
                            ))
                        }
                        _ => None,
                    })
                    .collect();
            }
            texts
        };
        // Every amplitude label sits at one x in either view, so a degree
        // stays put when the view changes.
        let (curve_x, walk_x) = (labels(View::Curve), labels(View::Walk));
        assert_eq!(curve_x.len(), walk_x.len(), "{curve_x:?} {walk_x:?}");
        assert!(curve_x.len() >= 4, "{curve_x:?}");
        for ((a, x), (b, y)) in curve_x.iter().zip(&walk_x) {
            assert_eq!(a, b);
            assert!((x - y).abs() < 1.0, "{a}: {x} and {y}");
        }
    }

    /// An amplitude label at either end of the axis is left off: at the
    /// left it would sit on the lowest rate label (Walter, #1133).
    #[test]
    fn no_amplitude_label_sits_in_a_corner() {
        let r = 275.0..=285.0;
        assert_eq!(amp_label(275.0, &r), "");
        assert_eq!(amp_label(285.0, &r), "");
        assert_eq!(amp_label(276.0, &r), "276");
        assert_eq!(amp_label(284.0, &r), "284");
    }

    /// Both axes run between round values on 1-2-5 steps, about 6 to 10
    /// of them (Hugh's F3).
    #[test]
    fn the_axes_have_round_ends_and_steps() {
        assert_eq!(round_axis((-118.7, -31.2)), ((-120.0, -30.0), 10.0));
        assert_eq!(round_axis((116.0, 263.0)), ((100.0, 280.0), 20.0));
        let ((lo, hi), step) = round_axis((236.3, 263.9));
        assert_eq!((lo, hi, step), (235.0, 265.0, 5.0));
        let ((lo, hi), step) = round_axis((-0.4, 2.35));
        assert!(
            (6.0..=10.0).contains(&((hi - lo) / step).round()),
            "{lo} {hi} {step}"
        );
    }

    /// A walk over 30 hours on core's 5 min bins, drifting down.
    fn long_walk() -> WalkView {
        use timegrapher_core::walk::{IsochronismWalk, WalkPoint, WALK_BIN_S};
        let points = (0..360)
            .map(|k| WalkPoint {
                t_s: (k as f64 + 0.5) * WALK_BIN_S,
                wind_h: 0.0,
                rate_s_per_day: 2.0 - 0.01 * k as f64 + 0.3 * (k as f64 / 9.0).sin(),
                amplitude_deg: 290.0 - 0.2 * k as f64,
                readings: 30,
                amp_windows: 150,
                ..Default::default()
            })
            .collect();
        let w = IsochronismWalk {
            bin_s: WALK_BIN_S,
            points,
            ..Default::default()
        };
        timegrapher_core::steady_views::walk_view(&w).expect("a long walk")
    }

    /// One frame of the pane as shapes, `size` wide and high.
    fn frame_of(
        ctx: &egui::Context,
        size: egui::Vec2,
        w: Option<&WalkView>,
        view: View,
    ) -> Vec<egui::epaint::ClippedShape> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |u| super::ui(u, Some(&run_down()), w, view));
        })
        .shapes
    }

    /// The texts drawn, with where.
    fn texts_of(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
        shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((
                    t.galley.text().to_string(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                )),
                _ => None,
            })
            .collect()
    }

    /// Look C (Peter, 11-05): a ring named "Start" at the first dot, a
    /// large dot named with its hour at the last, a ring at each of core's
    /// hour beads named off the trail, and the axes named with their units.
    #[test]
    fn the_walk_marks_its_start_end_and_hours() {
        let w = long_walk();
        assert_eq!(
            w.beads.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(),
            ["12 h", "24 h"]
        );
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let (bg, dark) = (
            ctx.style().visuals.extreme_bg_color,
            ctx.style().visuals.dark_mode,
        );
        let colour = |when: f64| crate::tfmap::time_colour(dark, when as f32);
        let shapes = frame_of(&ctx, egui::vec2(800.0, 700.0), Some(&w), View::Walk);
        let texts = texts_of(&shapes);
        for want in [
            "Start",
            w.last_text.as_str(),
            "12 h",
            "24 h",
            "Rate",
            "s/d",
            "Amp.",
            "°",
        ] {
            assert!(texts.iter().any(|t| t.0 == want), "{want}: {texts:?}");
        }
        let circles: Vec<egui::epaint::CircleShape> = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Circle(c) => Some(*c),
                _ => None,
            })
            .collect();
        let ring = |r: f32, when: f64| {
            circles
                .iter()
                .find(|c| c.radius == r && c.fill == bg && c.stroke.color == colour(when))
                .map(|c| c.center)
        };
        let start = ring(7.0, 0.0).expect("the start's ring");
        let end = circles
            .iter()
            .find(|c| c.radius == 6.5 && c.fill == colour(1.0))
            .expect("the end's dot");
        let name = |n: &str| texts.iter().find(|t| t.0 == n).unwrap().1;
        // Beside its ring, not on it (where, by #1542's rule: tested in
        // a_name_takes_the_first_clear_place_beside_its_dot).
        let first = name("Start");
        assert!(first.distance_to_pos(start) > 7.0, "{first:?}");
        let last = name(&w.last_text);
        assert!(last.distance_to_pos(end.center) > end.radius, "{last:?}");
        // Each bead's name and the end's sit clear of every dot, so never
        // on the trail.
        let dots: Vec<Pos2> = circles
            .iter()
            .filter(|c| c.radius == 2.5)
            .map(|c| c.center)
            .collect();
        assert_eq!(dots.len(), w.dots.len());
        for b in &w.beads {
            assert!(ring(5.0, w.dots[b.dot].when).is_some(), "{}", b.text);
        }
        // The trail is clipped to the plot's frame exactly (Hugh, 13 Nov).
        let plot = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Rect(r) if r.fill == bg => Some(r.rect),
                _ => None,
            })
            .max_by(|a, b| a.area().total_cmp(&b.area()))
            .expect("the plot's background");
        let trail: Vec<_> = shapes
            .iter()
            .filter(|c| matches!(&c.shape, egui::Shape::LineSegment { stroke, .. } if stroke.width == 2.0))
            .collect();
        assert!(!trail.is_empty());
        for c in trail {
            assert_eq!(c.clip_rect, plot);
        }
        // Each name sits on a box of the plot's background, painted just
        // before it, opaque so no trail shows through (Hugh, 13 Nov).
        for want in ["Start", w.last_text.as_str(), "12 h", "24 h"] {
            let i = shapes
                .iter()
                .position(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text() == want))
                .unwrap();
            let halo = matches!(&shapes[i - 1].shape,
                egui::Shape::Rect(r) if r.fill == bg
                    && r.rect.contains_rect(name(want)));
            assert!(halo, "{want}");
        }
        for text in w.beads.iter().map(|b| &b.text).chain([&w.last_text]) {
            let r = name(text);
            assert!(
                !dots.iter().any(|d| r.expand(1.0).contains(*d)),
                "{text} {r:?}"
            );
        }
    }

    /// On a tall, narrow pane the plot is never stretched taller than 1.1
    /// times its width, and sits in the middle (Hugh's F2).
    #[test]
    fn the_plot_is_never_stretched_tall() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let bg = ctx.style().visuals.extreme_bg_color;
        for view in [View::Curve, View::Walk] {
            let shapes = frame_of(&ctx, egui::vec2(360.0, 1200.0), Some(&long_walk()), view);
            let plot = shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Rect(r) if r.fill == bg => Some(r.rect),
                    _ => None,
                })
                .max_by(|a, b| a.area().total_cmp(&b.area()))
                .expect("the plot's background");
            assert!(
                plot.height() <= 1.1 * plot.width() + 1.0,
                "{view:?}: {plot:?}"
            );
            assert!(
                plot.top() > 150.0,
                "{view:?}: centred, not at the top: {plot:?}"
            );
        }
    }

    /// The headline's signed figures carry a true minus on screen, as the
    /// design guide asks (#1402): "−0.03 ± 0.01 s/d per degree".
    #[test]
    fn the_headline_carries_a_true_minus() {
        let mut curve = run_down();
        curve.headline = "Between 209° and 233° the rate goes with the amplitude by \
                          -0.03 ± 0.01 s/d per degree. The rate reads -8.8 s/d at 209°."
            .into();
        let ctx = egui::Context::default();
        theme::install(&ctx);
        // The finding in full is in the "i" (Hugh's spec, 13 Nov).
        ctx.data_mut(|d| d.insert_temp(info_id(), true));
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700.0, 700.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .show(ctx, |u| super::ui(u, Some(&curve), None, View::Curve));
        });
        let text: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
        assert!(
            text.iter()
                .any(|t| t.contains("by \u{2212}0.03 ± 0.01 s/d per degree")),
            "{text:?}"
        );
        assert!(
            text.iter().any(|t| t.contains("\u{2212}8.8 s/d")),
            "{text:?}"
        );
        assert!(
            !text
                .iter()
                .any(|t| t.contains("-0.03") || t.contains("-8.8")),
            "{text:?}"
        );
    }

    /// One view at a time: the Curve view has no walk, the Walk view no
    /// curve; and each says what it lacks (Peter).
    #[test]
    fn the_pane_shows_one_view_at_a_time() {
        let (curve, w) = (run_down(), walk());
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let texts = |c: Option<&IsochronismCurve>, wv: Option<&WalkView>, view: View| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 700.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |u| super::ui(u, c, wv, view));
            });
            out.shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let both = texts(Some(&curve), Some(&w), View::Curve);
        assert!(both.iter().any(|t| t == &iso_key(&curve)), "{both:?}");
        assert!(!both.iter().any(|t| t == WALK_KEY_START), "{both:?}");
        let walk_only = texts(Some(&curve), Some(&w), View::Walk);
        assert!(
            walk_only.iter().any(|t| t == WALK_KEY_START),
            "{walk_only:?}"
        );
        assert!(
            walk_only.iter().any(|t| t.contains(&iso_title(&curve))),
            "{walk_only:?}"
        );
        assert!(
            !walk_only.iter().any(|t| t == &iso_key(&curve)),
            "{walk_only:?}"
        );
        let none = texts(Some(&curve), None, View::Walk);
        assert!(
            none.iter().any(|t| t.starts_with("No walk yet")),
            "{none:?}"
        );
    }

    /// Every word off the plot (Hugh's Text Off the Charts, #1507): the
    /// title line, the end rates in the facts box, the key row; the
    /// finding in full and the method only in "i", the Walk's rate past
    /// its last amplitude in full there too; a dot's hover is core's.
    #[test]
    fn the_words_sit_off_the_plot_and_in_i() {
        let curve = run_down();
        let mut w = walk();
        w.rate_only_text = Some(
            "No amplitude read after 1 h. The rate to 2 h is on the Paper \
                                 Strip."
                .into(),
        );
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let texts = |view: View, info: bool| -> Vec<String> {
            ctx.data_mut(|d| d.insert_temp(info_id(), info));
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                ..Default::default()
            };
            ctx.run(input, |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |u| super::ui(u, Some(&curve), Some(&w), view));
            })
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
        };
        let shut = texts(View::Curve, false);
        let title = format!("  ·  {}", iso_title(&curve));
        for want in [ISO_TITLE, title.as_str(), iso_key(&curve).as_str()] {
            assert!(shut.iter().any(|t| t == want), "{want}: {shut:?}");
        }
        for (name, value) in iso_facts(&curve) {
            assert!(
                shut.contains(&name) && shut.contains(&value),
                "{name}: {shut:?}"
            );
        }
        let info = iso_info(&curve, false);
        assert!(!shut.iter().any(|t| info.contains(t)), "{shut:?}");
        let open = texts(View::Curve, true);
        for p in &info {
            assert!(open.contains(p), "{p}: {open:?}");
        }
        let walk_open = texts(View::Walk, true);
        for p in iso_info(&curve, true)
            .iter()
            .chain(w.rate_only_text.as_ref())
        {
            assert!(walk_open.contains(p), "{p}: {walk_open:?}");
        }
        // A dot's hover is the nearest bin's within reach, core's words.
        let t = egui_plot::PlotTransform::new(
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(280.0, 100.0)),
            egui_plot::PlotBounds::from_min_max([236.0, 0.0], [264.0, 20.0]),
            egui::Vec2b::new(false, false),
        );
        let b = &curve.bins[0];
        let at = t.position_from_point(&PlotPoint::new(b.amplitude_deg, b.rate_s_per_day));
        let near = nearest_bin(&curve, &t, at + egui::vec2(2.0, 0.0)).unwrap();
        assert_eq!(iso_bin_hover(near), iso_bin_hover(b));
        assert!(nearest_bin(&curve, &t, at + egui::vec2(0.0, HOVER_PX + 1.0)).is_none());
    }
}
