//! The paper strip.
//!
//! The strip is drawn the way tg draws its paperstrip: one dot per beat,
//! placed across the strip by how early or late the beat came against a
//! clock running at the nominal beat rate, and down the strip by time. The
//! newest beats are at the top. A watch on rate draws a vertical line; one
//! that gains leans right as it rises (/), one that loses leans left (\).
//! Tick and tock (the even and odd beats) draw two lines whose gap is the beat error.
//! A line that runs off one side comes back on the other, as on a
//! Witschi diagram, so the strip's width sets the zoom. The strip can also
//! lie on its side, time running left to right with the newest beats on the
//! right and early beats towards the top.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use timegrapher_core::beats::Beat;

/// Colours of the ticks (even beats) and tocks (odd beats).
pub fn side_colors(dark: bool) -> [Color32; 2] {
    let p = crate::theme::palette(dark);
    [p.tick, p.tock]
}

/// Where the strip is anchored: a beat number and the time it is drawn at
/// the centre line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub index: i64,
    pub time: f64,
}

impl Anchor {
    /// Centre the strip on the last `n` beats (tick and tock together).
    pub fn centre_on(beats: &[Beat], period_s: f64, n: usize) -> Option<Anchor> {
        let last = beats.last()?;
        let recent = &beats[beats.len().saturating_sub(n)..];
        let t = recent
            .iter()
            .map(|b| b.time - (b.index - last.index) as f64 * period_s)
            .sum::<f64>()
            / recent.len() as f64;
        Some(Anchor {
            index: last.index,
            time: t,
        })
    }

    /// How early the beat came, in ms, against this anchor and a beat
    /// period of `period_s`. Positive means early (the watch gains).
    pub fn lead_ms(&self, b: &Beat, period_s: f64) -> f64 {
        let expected = self.time + (b.index - self.index) as f64 * period_s;
        (expected - b.time) * 1000.0
    }
}

/// Fold `x` into `[-half, half)`.
pub fn wrap(x: f64, half: f64) -> f64 {
    (x + half).rem_euclid(2.0 * half) - half
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StripView {
    /// Half the strip's width, ms.
    pub half_width_ms: f64,
    /// Time shown along the strip, seconds.
    pub span_s: f64,
    /// Time runs left to right (newest on the right) instead of down the
    /// strip (newest at the top).
    pub horizontal: bool,
}

impl StripView {
    pub const MIN_HALF_WIDTH_MS: f64 = 0.1;
    pub const MAX_HALF_WIDTH_MS: f64 = 250.0;
    pub const MIN_SPAN_S: f64 = 2.0;
    pub const MAX_SPAN_S: f64 = 24.0 * 3600.0;

    pub fn set_half_width(&mut self, ms: f64) {
        self.half_width_ms = ms.clamp(Self::MIN_HALF_WIDTH_MS, Self::MAX_HALF_WIDTH_MS);
    }

    pub fn set_span(&mut self, s: f64) {
        self.span_s = s.clamp(Self::MIN_SPAN_S, Self::MAX_SPAN_S);
    }
}

/// What the mouse did to the strip in one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripInput {
    /// Seconds to move the newest edge by (positive: towards newer beats).
    pub time_shift_s: f64,
    /// Ms to slide the trace across by (positive: towards early).
    pub lead_shift_ms: f64,
    /// Factor for the length (below 1 zooms in).
    pub span_factor: f64,
    /// Factor for the width (below 1 zooms in).
    pub width_factor: f64,
    /// Double-clicked: back to the newest beats, centred.
    pub reset: bool,
    /// The time under the pointer, for the cursor shared with the charts.
    pub hover_t: Option<f64>,
}

impl Default for StripInput {
    fn default() -> Self {
        StripInput {
            time_shift_s: 0.0,
            lead_shift_ms: 0.0,
            span_factor: 1.0,
            width_factor: 1.0,
            reset: false,
            hover_t: None,
        }
    }
}

/// A step of 1, 2 or 5 times a power of ten, about `target` long.
fn nice_step(target: f64) -> f64 {
    let p = 10f64.powf(target.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if m * p >= target {
            return m * p;
        }
    }
    10.0 * p
}

/// Places points on the strip from (lead in ms, time in s).
struct Geom {
    r: Rect,
    half: f64,
    span: f64,
    end: f64,
    horizontal: bool,
}

impl Geom {
    /// The time at a point on the strip.
    fn time_at(&self, p: Pos2) -> f64 {
        let r = self.r;
        let back = if self.horizontal {
            (r.right() - p.x) / r.width().max(1.0)
        } else {
            (p.y - r.top()) / r.height().max(1.0)
        };
        self.end - back as f64 * self.span
    }

    fn pos(&self, ms: f64, t: f64) -> Pos2 {
        let r = self.r;
        let across = (ms / self.half) as f32;
        let back = ((self.end - t) / self.span) as f32;
        if self.horizontal {
            Pos2::new(
                r.right() - back * r.width(),
                r.center().y - across * r.height() / 2.0,
            )
        } else {
            Pos2::new(
                r.center().x + across * r.width() / 2.0,
                r.top() + back * r.height(),
            )
        }
    }
}

/// The rate reading to draw over the beats it was fitted to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateLine {
    pub from_s: f64,
    pub to_s: f64,
    pub rate_s_per_day: f64,
}

impl RateLine {
    /// The line's lead (ms) at each end, through the middle of the beats it
    /// covers: a line of slope `rate` through their mean time and lead.
    fn ends(&self, beats: &[Beat], anchor: Anchor, period_s: f64) -> Option<[(f64, f64); 2]> {
        let lo = beats.partition_point(|b| b.time < self.from_s);
        let hi = beats.partition_point(|b| b.time <= self.to_s);
        let used: Vec<&Beat> = beats[lo..hi].iter().filter(|b| b.quality >= 0.4).collect();
        if used.len() < 6 {
            return None;
        }
        let n = used.len() as f64;
        let t0 = used.iter().map(|b| b.time).sum::<f64>() / n;
        let l0 = used
            .iter()
            .map(|b| anchor.lead_ms(b, period_s))
            .sum::<f64>()
            / n;
        // A gaining watch beats a little early each time: ms of lead per second.
        let slope = self.rate_s_per_day / 86400.0 * 1000.0;
        let at = |t: f64| (t, l0 + slope * (t - t0));
        Some([at(used[0].time), at(used[used.len() - 1].time)])
    }
}

/// Another reading drawn over the strip on its own scale, against time.
#[derive(Debug, Clone)]
pub struct Overlay {
    pub name: &'static str,
    pub unit: &'static str,
    pub decimals: usize,
    pub color: Color32,
    /// The smallest range the scale spans, so a steady reading isn't
    /// blown up into noise.
    pub min_span: f64,
    /// (time in s, value), oldest first.
    pub points: Vec<[f64; 2]>,
}

impl Overlay {
    /// The scale's ends: the values in view with a little room either side.
    fn range(&self, from_s: f64, to_s: f64) -> Option<(f64, f64)> {
        let mut vals: Vec<f64> = self
            .points
            .iter()
            .filter(|p| p[0] >= from_s && p[0] <= to_s && p[1].is_finite())
            .map(|p| p[1])
            .collect();
        if vals.is_empty() {
            return None;
        }
        // The 2nd to 98th percentile, so a few stray readings (the first
        // seconds of a session, say) don't squash the rest flat.
        vals.sort_by(f64::total_cmp);
        let at = |q: f64| vals[((vals.len() - 1) as f64 * q).round() as usize];
        let (lo, hi) = (at(0.02), at(0.98));
        let mid = (lo + hi) / 2.0;
        let half = ((hi - lo) * 0.55).max(self.min_span / 2.0);
        Some((mid - half, mid + half))
    }
}

/// What is drawn over the beats.
#[derive(Debug, Clone, Default)]
pub struct Extras<'a> {
    /// A line of text over the strip, such as where the view is.
    pub note: Option<&'a str>,
    pub rate_line: Option<RateLine>,
    /// Faint lines parallel to the rate line across the whole strip.
    pub guides: bool,
    /// A time to mark with a cursor line: where the pointer is on a chart.
    pub cursor_t: Option<f64>,
    /// Space left of the plotting area when the strip lies across, so its
    /// time axis lines up with the charts' below it.
    pub gutter: f32,
    pub overlays: &'a [Overlay],
    /// Stretches of the session in one position to mark: start, end and
    /// the position's name. Empty while the watch stayed put.
    pub bands: &'a [(f64, f64, &'a str)],
}

/// Mark each stretch of the session in one position across the plotting
/// area `r`: every other stretch faintly shaded, a line where each new one
/// begins, and the position's name at its start. `at` places a time on the
/// time axis; `horizontal` when time runs across.
pub fn paint_bands(
    painter: &egui::Painter,
    r: Rect,
    bands: &[(f64, f64, &str)],
    at: impl Fn(f64) -> Pos2,
    horizontal: bool,
    vis: &egui::Visuals,
) {
    let text = vis.weak_text_color();
    let shade = text.gamma_multiply(0.07);
    let edge = Stroke::new(1.0_f32, text.gamma_multiply(0.6));
    let font = FontId::proportional(11.0);
    let along = |p: Pos2| if horizontal { p.x } else { p.y };
    let (lo, hi) = if horizontal {
        (r.left(), r.right())
    } else {
        (r.top(), r.bottom())
    };
    for (i, &(a, b, name)) in bands.iter().enumerate() {
        let (pa, pb) = (along(at(a)), along(at(b.min(1e12))));
        let (s0, s1) = (pa.min(pb).max(lo), pa.max(pb).min(hi));
        if s1 <= s0 {
            continue;
        }
        let band = if horizontal {
            Rect::from_x_y_ranges(s0..=s1, r.y_range())
        } else {
            Rect::from_x_y_ranges(r.x_range(), s0..=s1)
        };
        if i % 2 == 1 {
            painter.rect_filled(band, 0.0, shade);
        }
        let start_seen = pa >= lo && pa <= hi;
        if i > 0 && start_seen {
            if horizontal {
                painter.vline(pa, r.y_range(), edge);
            } else {
                painter.hline(r.x_range(), pa, edge);
            }
        }
        // The name at the stretch's start, or at the edge it runs in from.
        let (pos, align) = if horizontal {
            (Pos2::new(s0 + 4.0, r.top() + 2.0), Align2::LEFT_TOP)
        } else {
            (Pos2::new(r.left() + 4.0, s1 - 2.0), Align2::LEFT_BOTTOM)
        };
        let galley = painter.layout_no_wrap(name.to_string(), font.clone(), text);
        if galley.size().x + 8.0 <= if horizontal { s1 - s0 } else { r.width() } {
            // On a backing of the plot's colour, so a line under it can't
            // hide it.
            let rect = align.anchor_size(pos, galley.size()).expand(2.0);
            painter.rect_filled(rect, 2.0, vis.extreme_bg_color.gamma_multiply(0.85));
            painter.galley(rect.min + Vec2::splat(2.0), galley, text);
        }
    }
}

/// A signed number of ms for an axis label.
fn axis_ms(ms: f64) -> String {
    if ms.abs() < 1e-9 {
        "0 ms".into()
    } else {
        let sign = if ms > 0.0 { "+" } else { "−" };
        format!("{sign}{}", fmt_ms(ms.abs()))
    }
}

/// Draw the strip for the beats up to `end_s` and report what the mouse
/// did to it: the wheel zooms the length, Ctrl and the wheel (or a pinch)
/// zooms the width, dragging along the time axis looks back, dragging
/// across slides the trace, and a double click returns to the newest
/// beats.
#[allow(clippy::too_many_arguments)]
pub fn draw_strip(
    ui: &mut egui::Ui,
    beats: &[Beat],
    period_s: f64,
    anchor: Option<Anchor>,
    end_s: f64,
    view: &StripView,
    extras: &Extras,
    size: Vec2,
) -> StripInput {
    let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let outer = resp.rect;
    let vis = ui.visuals().clone();
    let pal = crate::theme::pal(ui);
    let dark = vis.dark_mode;
    painter.rect_filled(outer, 2.0, vis.extreme_bg_color);
    let font = FontId::proportional(11.0);
    let row_h = 15.0;
    let half = view.half_width_ms;
    let oldest = end_s - view.span_s;
    let overlays: Vec<(&Overlay, (f64, f64))> = extras
        .overlays
        .iter()
        .filter_map(|o| o.range(oldest, end_s).map(|r| (o, r)))
        .collect();
    // The plotting area inside its axes: time labels down the left (or
    // along the bottom when lying across), ms labels along the bottom (or
    // down the left), the key along the top, and a scale for each overlay
    // on top (or down the right).
    let r = if view.horizontal {
        Rect::from_min_max(
            outer.min + Vec2::new(extras.gutter.max(46.0), row_h + 4.0),
            outer.max - Vec2::new(4.0 + 58.0 * overlays.len() as f32, row_h + 2.0),
        )
    } else {
        Rect::from_min_max(
            outer.min + Vec2::new(36.0, row_h * (1.0 + overlays.len() as f32) + 4.0),
            outer.max - Vec2::new(8.0, row_h + 2.0),
        )
    };
    let g = Geom {
        r,
        half,
        span: view.span_s,
        end: end_s,
        horizontal: view.horizontal,
    };
    let mut input = StripInput::default();
    let d = resp.drag_delta();
    if view.horizontal {
        input.time_shift_s = -(d.x / r.width().max(1.0)) as f64 * view.span_s;
        input.lead_shift_ms = -(d.y / (r.height() / 2.0).max(1.0)) as f64 * view.half_width_ms;
    } else {
        input.time_shift_s = (d.y / r.height().max(1.0)) as f64 * view.span_s;
        input.lead_shift_ms = (d.x / (r.width() / 2.0).max(1.0)) as f64 * view.half_width_ms;
    }
    if resp.hovered() {
        let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
        let wheel = scroll.x + scroll.y;
        if wheel != 0.0 {
            input.span_factor = (-wheel as f64 * 0.003).exp();
        }
        if zoom != 1.0 {
            input.width_factor = 1.0 / zoom as f64;
        }
    }
    input.reset = resp.double_clicked();
    input.hover_t = resp
        .hover_pos()
        .filter(|p| r.contains(*p))
        .map(|p| g.time_at(p));
    if resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    let grid = Stroke::new(1.0_f32, vis.widgets.noninteractive.bg_stroke.color);
    let text = vis.weak_text_color();

    // Grid, every line alike: a nice step of ms along the strip, labelled
    // on the ms axis, and a nice step of seconds across it, labelled on the
    // time axis.
    let step = nice_step(half / 4.0);
    let mut k = (-half / step).ceil() as i64;
    while (k as f64) * step < half + 1e-9 {
        let ms = k as f64 * step;
        let (a, b) = (g.pos(ms, end_s), g.pos(ms, oldest));
        if (k as f64) * step > -half + 1e-9 {
            painter.line_segment([a, b], grid);
        }
        // Labels, leaving the ends for "Early" and "Late".
        let (at, align, room) = if view.horizontal {
            (
                Pos2::new(r.left() - 4.0, a.y),
                Align2::RIGHT_CENTER,
                (a.y - r.top()).min(r.bottom() - a.y),
            )
        } else {
            (
                Pos2::new(a.x, r.bottom() + 2.0),
                Align2::CENTER_TOP,
                (a.x - r.left()).min(r.right() - a.x),
            )
        };
        if room > if view.horizontal { 14.0 } else { 34.0 } {
            painter.text(at, align, axis_ms(ms), font.clone(), text);
        }
        k += 1;
    }
    let (early_at, early_align, late_at, late_align) = if view.horizontal {
        (
            Pos2::new(r.left() - 4.0, r.top()),
            Align2::RIGHT_TOP,
            Pos2::new(r.left() - 4.0, r.bottom()),
            Align2::RIGHT_BOTTOM,
        )
    } else {
        (
            Pos2::new(r.right(), r.bottom() + 2.0),
            Align2::RIGHT_TOP,
            Pos2::new(r.left(), r.bottom() + 2.0),
            Align2::LEFT_TOP,
        )
    };
    painter.text(early_at, early_align, "Early", font.clone(), text);
    painter.text(late_at, late_align, "Late", font.clone(), text);
    // Time lines at whole seconds, minutes or hours, about 90 points apart.
    let along_px = if view.horizontal {
        r.width()
    } else {
        r.height()
    } as f64;
    let want = view.span_s * 90.0 / along_px.max(1.0);
    let tstep = crate::theme::TIME_STEPS
        .iter()
        .copied()
        .find(|&s| s >= want)
        .unwrap_or(172800.0);
    let mut t = (end_s / tstep).floor() * tstep;
    // Nothing before the session started.
    while t > oldest && t >= -1e-9 {
        let (a, b) = (g.pos(-half, t), g.pos(half, t));
        painter.line_segment([a, b], grid);
        let (at, align) = if view.horizontal {
            (Pos2::new(a.x, r.bottom() + 2.0), Align2::CENTER_TOP)
        } else {
            (Pos2::new(r.left() - 4.0, a.y), Align2::RIGHT_CENTER)
        };
        painter.text(at, align, fmt_time(t), font.clone(), text);
        t -= tstep;
    }
    paint_bands(
        &painter,
        r,
        extras.bands,
        |t| g.pos(0.0, t),
        view.horizontal,
        &vis,
    );

    // The key along the top: Tick, Tock, the rate line and the overlays,
    // each in its colour.
    let colors = side_colors(dark);
    let mut at = Pos2::new(r.left(), outer.top() + 3.0);
    let mut keys: Vec<(&str, Color32)> = vec![("Tick", colors[0]), ("Tock", colors[1])];
    if extras.rate_line.is_some() {
        keys.push(("Rate Line", pal.trace_rate));
    }
    for (o, _) in &overlays {
        keys.push((o.name, o.color));
    }
    for (label, c) in keys {
        let k = painter.text(at, Align2::LEFT_TOP, label, font.clone(), c);
        at.x = k.right() + 12.0;
    }
    if let Some(n) = extras.note {
        painter.text(
            Pos2::new(outer.right() - 6.0, outer.top() + 3.0),
            Align2::RIGHT_TOP,
            n,
            font.clone(),
            vis.text_color(),
        );
    }
    // Each overlay's scale: its ends in its colour, in a row of its own
    // above the strip, or a column of its own right of it.
    for (i, (o, (lo, hi))) in overlays.iter().enumerate() {
        let f = |v: f64| format!("{v:.*}{}", o.decimals, o.unit);
        if view.horizontal {
            let x = r.right() + 6.0 + 58.0 * i as f32;
            painter.text(
                Pos2::new(x, r.top()),
                Align2::LEFT_TOP,
                f(*hi),
                font.clone(),
                o.color,
            );
            painter.text(
                Pos2::new(x, r.bottom()),
                Align2::LEFT_BOTTOM,
                f(*lo),
                font.clone(),
                o.color,
            );
        } else {
            let y = outer.top() + 3.0 + row_h * (1 + i) as f32;
            painter.text(
                Pos2::new(r.left(), y),
                Align2::LEFT_TOP,
                f(*lo),
                font.clone(),
                o.color,
            );
            painter.text(
                Pos2::new(r.center().x, y),
                Align2::CENTER_TOP,
                format!("{} scale", o.name),
                font.clone(),
                o.color.gamma_multiply(0.8),
            );
            painter.text(
                Pos2::new(r.right(), y),
                Align2::RIGHT_TOP,
                f(*hi),
                font.clone(),
                o.color,
            );
        }
    }

    let Some(anchor) = anchor else {
        return input;
    };
    let lo = beats.partition_point(|b| b.time < oldest);
    let hi = beats.partition_point(|b| b.time <= end_s);
    let shown = &beats[lo..hi];
    // Dots shrink when the strip is crowded.
    let along = if view.horizontal {
        r.width()
    } else {
        r.height()
    };
    let per_px = shown.len() as f32 / along.max(1.0);
    let radius = (2.2 / per_px.max(1.0).sqrt()).clamp(0.8, 2.2);
    let painter = painter.with_clip_rect(r.expand(3.0));
    for b in shown {
        if b.quality < 0.4 {
            continue;
        }
        let p = g.pos(wrap(anchor.lead_ms(b, period_s), half), b.time);
        let c = colors[b.index.rem_euclid(2) as usize];
        painter.circle_filled(p, radius, c);
    }
    let painter = painter.with_clip_rect(r);
    // The overlays, each scaled across the strip's width.
    for (o, (lo, hi)) in &overlays {
        let across = |v: f64| -half + (v - lo) / (hi - lo) * 2.0 * half;
        let pts: Vec<&[f64; 2]> = o
            .points
            .iter()
            .filter(|p| p[0] >= oldest - 2.0 && p[0] <= end_s)
            .collect();
        for w in pts.windows(2) {
            // A gap in the readings is a gap in the line.
            if w[1][0] - w[0][0] > 5.0 {
                continue;
            }
            painter.line_segment(
                [
                    g.pos(across(w[0][1]), w[0][0]),
                    g.pos(across(w[1][1]), w[1][0]),
                ],
                Stroke::new(1.6_f32, o.color),
            );
        }
    }
    // The rate reading over its beats, on top of them, wrapping as the dots
    // do; and faint parallels to it across the whole strip, one per grid
    // step, so the eye can tell whether the dots run parallel to it.
    if let Some(line) = extras.rate_line {
        if let Some([(ta, la), (tb, lb)]) = line.ends(beats, anchor, period_s) {
            let slope = if tb > ta { (lb - la) / (tb - ta) } else { 0.0 };
            let at = |t: f64| la + slope * (t - ta);
            let polyline = |t0: f64, t1: f64, offset: f64, stroke: Stroke| {
                let steps = 200;
                let mut prev: Option<(f64, Pos2)> = None;
                for k in 0..=steps {
                    let t = t0 + (t1 - t0) * k as f64 / steps as f64;
                    let w = wrap(at(t) + offset, half);
                    let p = g.pos(w, t);
                    if let Some((pw, pp)) = prev {
                        if (w - pw).abs() < half {
                            painter.line_segment([pp, p], stroke);
                        }
                    }
                    prev = Some((w, p));
                }
            };
            if extras.guides {
                let n = (2.0 * half / step).round().max(1.0) as i64;
                let faint = Stroke::new(1.0_f32, pal.trace_rate.gamma_multiply(0.3));
                for k in 0..n {
                    polyline(oldest, end_s, k as f64 * step, faint);
                }
            }
            // A thin dark edge under the line keeps it readable over the dots.
            polyline(
                ta,
                tb,
                0.0,
                Stroke::new(2.6_f32, vis.extreme_bg_color.gamma_multiply(0.7)),
            );
            polyline(ta, tb, 0.0, Stroke::new(1.2_f32, pal.trace_rate));
        }
    }
    if let Some(t) = extras.cursor_t.filter(|t| *t >= oldest && *t <= end_s) {
        let (a, b) = (g.pos(-half, t), g.pos(half, t));
        painter.line_segment([a, b], Stroke::new(1.0_f32, vis.weak_text_color()));
    }
    input
}

pub fn fmt_ms(ms: f64) -> String {
    if ms >= 10.0 {
        format!("{ms:.0}")
    } else if ms >= 1.0 {
        format!("{ms:.1}")
    } else {
        format!("{ms:.2}").trim_end_matches('0').to_string()
    }
}

pub fn fmt_time(t: f64) -> String {
    let s = t.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat(index: i64, time: f64) -> Beat {
        Beat {
            index,
            time,
            quality: 1.0,
        }
    }

    #[test]
    fn a_gaining_watch_leans_early() {
        // 28,800 bph gaining 86.4 s/d (1 part in 1000): each beat 0.125 ms sooner.
        let p = 0.125;
        let actual = p * (1.0 - 86.4 / 86400.0);
        let beats: Vec<Beat> = (0..80).map(|k| beat(k, k as f64 * actual)).collect();
        let a = Anchor::centre_on(&beats[..8], p, 8).unwrap();
        let first = a.lead_ms(&beats[8], p);
        let later = a.lead_ms(&beats[79], p);
        assert!(later > first, "{first} -> {later}");
        // 71 beats later the lead has grown by 71 * 0.125 ms.
        assert!((later - first - 71.0 * 0.125).abs() < 1e-9);
    }

    #[test]
    fn centring_splits_the_beat_error() {
        let p = 0.125;
        let e = 0.0008;
        let beats: Vec<Beat> = (0..16)
            .map(|k| {
                beat(
                    k,
                    k as f64 * p + if k % 2 == 0 { e / 2.0 } else { -e / 2.0 },
                )
            })
            .collect();
        let a = Anchor::centre_on(&beats, p, 16).unwrap();
        let even = a.lead_ms(&beats[14], p);
        let odd = a.lead_ms(&beats[15], p);
        assert!(
            (even + 0.4).abs() < 1e-9 && (odd - 0.4).abs() < 1e-9,
            "{even} {odd}"
        );
    }

    #[test]
    fn the_time_under_the_pointer_is_where_the_beat_is_drawn() {
        for horizontal in [false, true] {
            let g = Geom {
                r: Rect::from_min_size(Pos2::new(50.0, 20.0), Vec2::new(400.0, 300.0)),
                half: 10.0,
                span: 60.0,
                end: 100.0,
                horizontal,
            };
            for t in [40.0, 55.5, 100.0] {
                assert!((g.time_at(g.pos(3.0, t)) - t).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn wrapping() {
        assert_eq!(wrap(0.0, 5.0), 0.0);
        assert!((wrap(6.0, 5.0) + 4.0).abs() < 1e-12);
        assert!((wrap(-6.0, 5.0) - 4.0).abs() < 1e-12);
        assert_eq!(nice_step(3.0), 5.0);
        assert_eq!(nice_step(0.12), 0.2);
        assert_eq!(fmt_time(3725.0), "1:02:05");
    }
}
