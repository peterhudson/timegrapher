//! The paper strip.
//!
//! The strip is drawn the way tg draws its paperstrip: one dot per beat,
//! placed across the strip by how early or late the beat came against a
//! clock running at the nominal beat rate, and down the strip by time. The
//! newest beats are at the top. A watch on rate draws a vertical line; one
//! that gains leans right as it rises (/), one that loses leans left (\).
//! Tick and tock (the even and odd beats) draw two lines whose gap is the beat error.
//! A line that runs off one side comes back on the other, so the strip's
//! width sets the zoom. The strip can also
//! lie on its side, time running left to right with the newest beats on the
//! right and early beats towards the top.

use crate::strength::{self, Zero};
use axis::{axis_ms, label_every};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use sweep::{Geom, Sweep};
use timegrapher_core::beats::Beat;
use timegrapher_core::breaks::Break;
use timegrapher_core::chart_scale::{axis_number, round_step, steady_ends};
use timegrapher_core::clock::ClockFit;
use timegrapher_core::library::length_text;
pub use timegrapher_core::timing::Anchor;

mod axis;
#[cfg(test)]
mod key_tests;
#[cfg(test)]
mod still_tests;
mod sweep;

/// Colours of the ticks (even beats) and tocks (odd beats).
pub fn side_colors(dark: bool) -> [Color32; 2] {
    let p = crate::theme::palette(dark);
    [p.tick, p.tock]
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
    /// The paper is still and the pen sweeps along it, back to the start
    /// every `span_s` ([`Sweep::Still`]), instead of the paper moving.
    #[serde(default)]
    pub still: bool,
}

/// The most beats drawn to a point along the strip.
const MAX_BEATS_PER_PX: f32 = 48.0;

impl StripView {
    pub const MIN_HALF_WIDTH_MS: f64 = 0.1;
    /// Wide enough for a long run-down take to drift by tens of seconds
    /// without its beats wrapping.
    pub const MAX_HALF_WIDTH_MS: f64 = 60_000.0;
    pub const MIN_SPAN_S: f64 = 2.0;
    /// As long as a power reserve run may go, 80 hours: a run-down take
    /// shows whole.
    pub const MAX_SPAN_S: f64 = timegrapher_core::power_reserve::LIMIT_S;

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
    /// The time under the pointer the length is zoomed about, which stays
    /// where it is on the strip; `None` zooms about the newest edge.
    pub zoom_at: Option<f64>,
    /// Factor for the width (below 1 zooms in).
    pub width_factor: f64,
    /// Double-clicked: back to the newest beats, centred.
    pub reset: bool,
    /// The time under the pointer, for the cursor shared with the charts.
    pub hover_t: Option<f64>,
    /// With the Mask tool on: a stretch dragged over, start and end, to
    /// mask out of the readings.
    pub mask: Option<(f64, f64)>,
    /// A right click at this time, or a click with the Mask tool on, to
    /// bring back the masked stretch there.
    pub unmask_at: Option<f64>,
    /// A right click on a painted stretch in the track's lane, at this
    /// time: take the painting off, back to the positions recorded.
    pub unpaint_at: Option<f64>,
    /// With the Mask tool on: an edge of a masked stretch dragged, the
    /// stretch as it was and as it is now.
    pub reshape: Option<((f64, f64), (f64, f64))>,
    /// With the Position Brush on: a stretch dragged over, start and end,
    /// to set to the brush's position.
    pub paint: Option<(f64, f64)>,
    /// A change of position dragged along the track: where it was and
    /// where it is now.
    pub move_change: Option<(f64, f64)>,
}

impl Default for StripInput {
    fn default() -> Self {
        StripInput {
            time_shift_s: 0.0,
            lead_shift_ms: 0.0,
            span_factor: 1.0,
            zoom_at: None,
            width_factor: 1.0,
            reset: false,
            hover_t: None,
            mask: None,
            unmask_at: None,
            unpaint_at: None,
            reshape: None,
            paint: None,
            move_change: None,
        }
    }
}

/// An overlay's scale: its round ends and step, `(lo, hi, step)`.
type Scale = (f64, f64, f64);

/// An overlay's values in view, lowest and highest, and the room wanted
/// for them on its scale.
type Range = ((f64, f64), (f64, f64));

/// How tall an overlay's axis row above an upright strip is, points: two
/// lines, its short name over its unit and numbers (#1278).
const SCALE_ROW: f32 = 36.0;

/// An overlay axis's tick, points.
const TICK: f32 = 4.0;

/// The room before each overlay axis right of the strip, points.
const SCALE_GAP: f32 = 8.0;

/// An overlay's scale for its `range` ([`Overlay::range`]): round ends
/// that hold still while the readings move inside them ([`steady_ends`]),
/// kept from one frame to the next under the overlay's `name`.
fn held_scale(ui: &egui::Ui, name: &str, range: Range) -> Scale {
    let id = ui.id().with(("strip scale", name));
    let s = steady_ends(ui.data(|d| d.get_temp::<Scale>(id)), range.0, range.1);
    ui.data_mut(|d| d.insert_temp(id, s));
    s
}

/// Where `v` on scale `s` lies in ms across a strip `half` ms either side:
/// the scale's ends at the strip's edges.
fn scale_ms((lo, hi, _): Scale, v: f64, half: f64) -> f64 {
    -half + (v - lo) / (hi - lo) * 2.0 * half
}

/// The round values a scale ticks at, end to end.
fn ticks((lo, hi, step): Scale) -> impl Iterator<Item = f64> {
    let n = ((hi - lo) / step).round() as i64;
    (0..=n).map(move |k| lo + k as f64 * step)
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
    fn ends(
        &self,
        beats: &[Beat],
        anchor: Anchor,
        period_s: f64,
        breaks: &[Break],
        clock: Option<&ClockFit>,
    ) -> Option<[(f64, f64); 2]> {
        let lo = beats.partition_point(|b| b.time < self.from_s);
        let hi = beats.partition_point(|b| b.time <= self.to_s);
        let used: Vec<&Beat> = beats[lo..hi].iter().filter(|b| b.is_good()).collect();
        if used.len() < 6 {
            return None;
        }
        let n = used.len() as f64;
        let t0 = used.iter().map(|b| b.time).sum::<f64>() / n;
        let l0 = used
            .iter()
            .map(|b| anchor.lead_on_true(b, period_s, breaks, clock))
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
    /// The name its axis carries, short so the axis takes little room:
    /// "Rate", "Amp.", "B.E.".
    pub short: &'static str,
    pub unit: &'static str,
    pub color: Color32,
    /// The smallest range the scale spans, so a steady reading isn't
    /// blown up into noise.
    pub min_span: f64,
    /// (time in s, value), oldest first.
    pub points: Vec<[f64; 2]>,
}

/// Whether the step to `points[i]` from the one before (points sorted by
/// time) is a gap in the readings rather than their spacing: more than
/// half again the steps either side of it. Readings worked out again over
/// a long take and those read live since are spaced differently and join
/// up; a stretch with none read (a mask, a pause, beats not found) is a
/// gap. With no step beside it to go by, it isn't one.
pub fn gap_before(points: &[[f64; 2]], i: usize) -> bool {
    if i == 0 || i >= points.len() {
        return false;
    }
    let step = |j: usize| points[j][0] - points[j - 1][0];
    let beside = [
        (i >= 2).then(|| step(i - 1)),
        (i + 1 < points.len()).then(|| step(i + 1)),
    ];
    beside
        .into_iter()
        .flatten()
        .reduce(f64::max)
        .is_some_and(|usual| step(i) > 1.5 * usual)
}

/// The pairs of neighbouring `points` from `from_s` to `to_s` joined by a
/// line: those with no gap ([`gap_before`]) between them.
fn joined(points: &[[f64; 2]], from_s: f64, to_s: f64) -> Vec<([f64; 2], [f64; 2])> {
    let lo = points.partition_point(|p| p[0] < from_s);
    let hi = points.partition_point(|p| p[0] <= to_s);
    (lo.max(1)..hi)
        .filter(|&i| !gap_before(points, i))
        .map(|i| (points[i - 1], points[i]))
        .filter(|(a, b)| a[0] >= from_s && a[1].is_finite() && b[1].is_finite())
        .collect()
}

impl Overlay {
    /// The values in view, lowest and highest, and the room wanted for
    /// them: a little either side, and at least the least span.
    fn range(&self, from_s: f64, to_s: f64) -> Option<Range> {
        let vals: Vec<f64> = self
            .points
            .iter()
            .filter(|p| p[0] >= from_s && p[0] <= to_s && p[1].is_finite())
            .map(|p| p[1])
            .collect();
        if vals.is_empty() {
            return None;
        }
        // Every value in view fits, as on the charts.
        let lo = vals.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mid = (lo + hi) / 2.0;
        let half = ((hi - lo) * 0.55).max(self.min_span / 2.0);
        Some(((lo, hi), (mid - half, mid + half)))
    }
}

/// What is drawn over the beats.
#[derive(Debug, Clone, Default)]
pub struct Extras<'a> {
    /// A line of text over the strip, such as where the view is.
    pub note: Option<&'a str>,
    /// The Rate reading the line and the guides are drawn from.
    pub rate_line: Option<RateLine>,
    /// Draw the rate line itself.
    pub line: bool,
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
    /// Stretches the readings leave out: start, end and what the ticker
    /// says over it ("Settling" after a move; in a Positions Test also
    /// "Not Used for Positions Test" and "Before Test"). Their beats are
    /// drawn faintly.
    pub settles: &'a [(f64, f64, &'a str)],
    /// The stretches a Positions Test measures, the one under way
    /// included. Not empty once a test has begun: the strip then shades
    /// by the test's phases in place of every other position.
    pub measured: &'a [(f64, f64)],
    /// Stretches masked out of the readings: start and end. They are
    /// shaded and their beats drawn faintly.
    pub masked: &'a [(f64, f64)],
    /// The Mask tool is on: dragging along the strip masks a stretch in
    /// place of looking back, and a click brings one back.
    pub masking: bool,
    /// Every stretch of the take in one position, for the track: start,
    /// end, the position's name and its code. One over the whole take
    /// while the watch stayed put.
    pub track: &'a [(f64, f64, &'a str, &'a str)],
    /// The Position Brush is on, with this position's code: dragging along
    /// the strip sets that stretch to the position in place of looking
    /// back.
    pub brush: Option<&'a str>,
    /// Where painting with the Position Brush shows over the positions
    /// recorded: start and end. A right click on one in the track's lane
    /// takes the painting off.
    pub painted: &'a [(f64, f64)],
    /// The take's breaks, so each beat's lead is read on true time: a
    /// steady watch's trace runs on through a gap. Empty while listening.
    pub breaks: &'a [Break],
    /// The clock the rate line's reading is on, so the beats are read on
    /// it too and run along the line on a corrected take (#643).
    pub clock: Option<&'a ClockFit>,
}

/// Mark each stretch of the session in one position across the plotting
/// area `r`: every other stretch faintly shaded (unless `shade` is off, as
/// in a Positions Test, which [`paint_phases`] shades), a line where each
/// new one begins, and (with `names`) the position's name at its start.
/// `at` places a time on the time axis; `horizontal` when time runs
/// across.
#[allow(clippy::too_many_arguments)]
pub fn paint_bands(
    painter: &egui::Painter,
    r: Rect,
    bands: &[(f64, f64, &str)],
    at: impl Fn(f64) -> Pos2,
    horizontal: bool,
    names: bool,
    shade: bool,
    vis: &egui::Visuals,
) {
    let text = vis.weak_text_color();
    let fill = text.gamma_multiply(SETTLING_SHADE);
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
        if shade && i % 2 == 1 {
            painter.rect_filled(band, 0.0, fill);
        }
        let start_seen = pa >= lo && pa <= hi;
        if i > 0 && start_seen {
            if horizontal {
                painter.vline(pa, r.y_range(), edge);
            } else {
                painter.hline(r.x_range(), pa, edge);
            }
        }
        if !names {
            continue;
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

/// The time along a stretch of the strip from `a` to `b` as a rectangle
/// across `r`, cut to `r`; `None` when none of it is in view.
fn along_rect(
    r: Rect,
    a: f64,
    b: f64,
    at: &impl Fn(f64) -> Pos2,
    horizontal: bool,
) -> Option<Rect> {
    let along = |p: Pos2| if horizontal { p.x } else { p.y };
    let (lo, hi) = if horizontal {
        (r.left(), r.right())
    } else {
        (r.top(), r.bottom())
    };
    let (pa, pb) = (along(at(a.max(-1e12))), along(at(b.min(1e12))));
    let (s0, s1) = (pa.min(pb).max(lo), pa.max(pb).min(hi));
    (s1 > s0).then(|| {
        if horizontal {
            Rect::from_x_y_ranges(s0..=s1, r.y_range())
        } else {
            Rect::from_x_y_ranges(r.x_range(), s0..=s1)
        }
    })
}

/// When the settling that began the stretch from `a` to `b` ends: the
/// latest end of those in `settles` starting in it, else its start.
fn settled_from(a: f64, b: f64, settles: &[(f64, f64, &str)]) -> f64 {
    settles
        .iter()
        .filter(|s| s.0 >= a && s.0 < b && s.2 == SETTLING)
        .fold(a, |to, s| to.max(s.1.min(b)))
}

/// The track's strength at `t` in a stretch settled from `settled`: solid
/// through the settling, then fading (see [`crate::theme::Palette::track`]).
fn track_density(t: f64, settled: f64, track: [f32; 3]) -> f32 {
    let [settling, fade_from, fade_to] = track;
    if t < settled {
        settling
    } else {
        let k = ((t - settled) / crate::theme::TRACK_FADE_S).clamp(0.0, 1.0) as f32;
        fade_from + (fade_to - fade_from) * k
    }
}

/// Fill the stretch from `a` to `b` across `r` in `color` at the track's
/// strength (times `scale`), settled from `settled`: the fade drawn in
/// slices a couple of points wide.
#[allow(clippy::too_many_arguments)]
fn paint_density(
    painter: &egui::Painter,
    r: Rect,
    a: f64,
    b: f64,
    settled: f64,
    color: Color32,
    track: [f32; 3],
    scale: f32,
    at: &impl Fn(f64) -> Pos2,
    horizontal: bool,
) {
    let fade_to = settled + crate::theme::TRACK_FADE_S;
    let fill = |a: f64, b: f64, t: f64| {
        if let Some(rect) = along_rect(r, a, b, at, horizontal) {
            painter.rect_filled(
                rect,
                0.0,
                color.gamma_multiply(track_density(t, settled, track) * scale),
            );
        }
    };
    if settled > a {
        fill(a, settled.min(b), a);
    }
    if fade_to > settled && b > settled {
        let to = fade_to.min(b);
        let along = |p: Pos2| if horizontal { p.x } else { p.y };
        let px = (along(at(to)) - along(at(settled))).abs();
        let n = ((px / 2.0).ceil() as usize).clamp(1, 64);
        let step = (to - settled) / n as f64;
        for k in 0..n {
            let t0 = settled + step * k as f64;
            fill(t0, t0 + step, t0 + step / 2.0);
        }
    }
    if b > fade_to {
        fill(fade_to.max(a), b, fade_to);
    }
}

/// Write `name` along the stretch `seg` of the track `lane` from its start
/// (`start_at` its start's place along the time), else `code` when the
/// name doesn't fit, else nothing, in `color` on a small `plate`, so it
/// reads the same on solid teal, faded teal or hatching. Turned a quarter,
/// read from the top, when the track runs down.
#[allow(clippy::too_many_arguments)]
fn track_label(
    painter: &egui::Painter,
    lane: Rect,
    seg: Rect,
    start_at: f32,
    name: &str,
    code: &str,
    color: Color32,
    plate: Color32,
    horizontal: bool,
) {
    // Only the part of the stretch that shows: on still paper a lap is cut
    // at the wrap and the blank ahead of the pen, and a name placed by the
    // whole stretch would be cut there too.
    let seg = seg.intersect(painter.clip_rect());
    if !seg.is_positive() {
        return;
    }
    let font = FontId::proportional(11.0);
    let room = if horizontal {
        seg.width()
    } else {
        seg.height()
    } - 8.0;
    let galley = [name, code]
        .into_iter()
        .filter(|t| !t.is_empty())
        .map(|t| painter.layout_no_wrap(t.to_string(), font.clone(), color))
        .find(|g| g.size().x <= room);
    let Some(galley) = galley else { return };
    let (w, h) = (galley.size().x, galley.size().y);
    if horizontal {
        // From the start, or the left edge it runs in from.
        let x = start_at.max(seg.left()) + 4.0;
        let x = x.min(seg.right() - 4.0 - w);
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(x - 2.0, lane.center().y - h / 2.0),
                Vec2::new(w + 4.0, h),
            ),
            2.0,
            plate,
        );
        painter.galley(
            Pos2::new(x, lane.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
    } else {
        // Newest at the top: the start is the bottom.
        let y = start_at.min(seg.bottom()) - 4.0 - w;
        let y = y.max(seg.top() + 4.0);
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(lane.center().x - h / 2.0, y - 2.0),
                Vec2::new(h, w + 4.0),
            ),
            2.0,
            plate,
        );
        let shape = egui::epaint::TextShape::new(
            Pos2::new(lane.center().x + galley.size().y / 2.0, y),
            galley,
            color,
        )
        .with_angle(std::f32::consts::FRAC_PI_2);
        painter.add(shape);
    }
}

/// The track in `lane`, beside the plotting area: each stretch of the take
/// in one position in teal, solid through its settling and then fading,
/// named at its start, with a thin line at each change; during a
/// Positions Test the time it didn't use plain, with what it is; masked
/// stretches grey over them, named "Masked".
#[allow(clippy::too_many_arguments)]
pub fn paint_track(
    painter: &egui::Painter,
    lane: Rect,
    track: &[(f64, f64, &str, &str)],
    settles: &[(f64, f64, &str)],
    masked: &[(f64, f64)],
    covered: Option<(f64, f64)>,
    at: impl Fn(f64) -> Pos2,
    horizontal: bool,
    vis: &egui::Visuals,
    pal: &crate::theme::Palette,
) {
    let painter = painter.with_clip_rect(lane);
    let weak = vis.weak_text_color();
    let plain = weak.gamma_multiply(SETTLING_SHADE);
    painter.rect_filled(lane, 2.0, plain);
    let along = |p: Pos2| if horizontal { p.x } else { p.y };
    for &(a, b, _, _) in track {
        let settled = settled_from(a, b, settles);
        paint_density(
            &painter,
            lane,
            a,
            b,
            settled,
            pal.positions,
            pal.track,
            1.0,
            &at,
            horizontal,
        );
    }
    // A test's time not used, and before it, stays plain.
    let plain_parts: Vec<(f64, f64, &str)> = settles
        .iter()
        .copied()
        .filter(|s| s.2 == crate::guided::NOT_USED || s.2 == crate::guided::BEFORE_TEST)
        .collect();
    for &(a, b, _) in &plain_parts {
        if let Some(seg) = along_rect(lane, a, b, &at, horizontal) {
            painter.rect_filled(seg, 0.0, vis.extreme_bg_color);
            painter.rect_filled(seg, 0.0, plain);
        }
    }
    let cut = Stroke::new(1.5_f32, Color32::from_black_alpha(150));
    let ink = vis.text_color();
    for (i, &(a, b, name, code)) in track.iter().enumerate() {
        let start = along(at(a.max(0.0)));
        if i > 0 {
            if horizontal {
                painter.vline(start, lane.y_range(), cut);
            } else {
                painter.hline(lane.x_range(), start, cut);
            }
        }
        // Named in each part of it left showing: not the time a test
        // didn't use, nor masked, nor being painted over.
        // The name starts after the settle band, which has its own word.
        let band = settle_band(a, b, settles);
        let hidden = plain_parts.iter().map(|p| (p.0, p.1));
        let hidden = hidden
            .chain(masked.iter().copied())
            .chain(covered)
            .chain(band);
        for (from, to) in showing(a, b, hidden) {
            if let Some(seg) = along_rect(lane, from, to, &at, horizontal) {
                track_label(
                    &painter,
                    lane,
                    seg,
                    along(at(from.max(0.0))),
                    name,
                    code,
                    ink,
                    pal.card,
                    horizontal,
                );
            }
        }
    }
    for &(a, b, why) in &plain_parts {
        if let Some(seg) = along_rect(lane, a, b, &at, horizontal) {
            track_label(
                &painter,
                lane,
                seg,
                along(at(a.max(0.0))),
                why,
                "",
                ink.gamma_multiply(0.7),
                pal.card,
                horizontal,
            );
        }
    }
    // Each settle band's word, in each part of it left showing.
    for &(a, b, _, _) in track {
        let Some((s0, s1)) = settle_band(a, b, settles) else {
            continue;
        };
        let hidden = plain_parts.iter().map(|p| (p.0, p.1));
        let hidden = hidden.chain(masked.iter().copied()).chain(covered);
        for (p0, p1) in showing(s0, s1, hidden) {
            if let Some(seg) = along_rect(lane, p0, p1, &at, horizontal) {
                settle_word(&painter, seg, horizontal);
            }
        }
    }
    track_masked(&painter, lane, masked, &at, horizontal, vis, pal);
}

/// Masked stretches in the track's `lane`: hatched grey, named "Masked".
fn track_masked(
    painter: &egui::Painter,
    lane: Rect,
    masked: &[(f64, f64)],
    at: &impl Fn(f64) -> Pos2,
    horizontal: bool,
    vis: &egui::Visuals,
    pal: &crate::theme::Palette,
) {
    let along = |p: Pos2| if horizontal { p.x } else { p.y };
    let ink = vis.text_color();
    for &(a, b) in masked {
        if let Some(seg) = along_rect(lane, a, b, at, horizontal) {
            painter.rect_filled(seg, 0.0, vis.extreme_bg_color);
            crate::theme::hatch(painter, seg, pal.text_tertiary, 2.5, 6.0);
            track_label(
                painter,
                lane,
                seg,
                along(at(a)),
                "Masked",
                "",
                ink,
                pal.card,
                horizontal,
            );
        }
    }
}

/// The parts of `a` to `b` that none of the stretches `hidden` covers, in
/// order.
pub(crate) fn showing(a: f64, b: f64, hidden: impl Iterator<Item = (f64, f64)>) -> Vec<(f64, f64)> {
    let mut hidden: Vec<(f64, f64)> = hidden.filter(|p| p.1 > a && p.0 < b).collect();
    hidden.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut from = a;
    let mut parts = Vec::new();
    for &(h0, h1) in &hidden {
        if h0 > from {
            parts.push((from, h0));
        }
        from = from.max(h1);
    }
    if from < b {
        parts.push((from, b));
    }
    parts
}

use crate::guided::SETTLING;

/// The settle band that begins the stretch from `a` to `b`: from its start
/// to where its settling ends. None when it began without one.
pub fn settle_band(a: f64, b: f64, settles: &[(f64, f64, &str)]) -> Option<(f64, f64)> {
    let settled = settled_from(a, b, settles);
    (settled > a).then_some((a, settled))
}

/// The word for a settle band `w` points long: "Settling" from 64 pt,
/// "STL" from 28, "S" from 12, none below.
pub fn settle_word_for(w: f32) -> Option<&'static str> {
    match w {
        w if w >= 64.0 => Some("Settling"),
        w if w >= 28.0 => Some("STL"),
        w if w >= 12.0 => Some("S"),
        _ => None,
    }
}

/// The dark ink of a settle band's word, directly on the solid teal in
/// both themes.
const SETTLE_INK: Color32 = Color32::from_rgb(0x06, 0x2b, 0x26);

/// Write the word that fits in the settle band `seg`, in the middle of it,
/// with no plate. Turned a quarter, read from the top, when the track
/// runs down.
fn settle_word(painter: &egui::Painter, seg: Rect, horizontal: bool) {
    let w = if horizontal {
        seg.width()
    } else {
        seg.height()
    };
    let Some(word) = settle_word_for(w) else {
        return;
    };
    let galley = painter.layout_no_wrap(word.to_string(), crate::theme::semibold(11.0), SETTLE_INK);
    let size = galley.size();
    if horizontal {
        painter.galley(seg.center() - size / 2.0, galley, SETTLE_INK);
    } else {
        let at = Pos2::new(seg.center().x + size.y / 2.0, seg.center().y - size.x / 2.0);
        painter.add(
            egui::epaint::TextShape::new(at, galley, SETTLE_INK)
                .with_angle(std::f32::consts::FRAC_PI_2),
        );
    }
}

/// How dark the shade over a measured stretch is, and over a settling (and
/// the lighter of two positions when no test is under way), as fractions
/// of the faint text colour.
const MEASURED_SHADE: f32 = 0.16;
const SETTLING_SHADE: f32 = 0.07;
/// A settling's shade as a share of a measured stretch's, for other tracks.
pub(crate) const SETTLE_OF_MEASURED: f32 = SETTLING_SHADE / MEASURED_SHADE;
/// The phase name [`paint_phase`] takes for a measured stretch.
const MEASURED: &str = "Measured";

/// Shade `rect` for one phase of a Positions Test: a measured stretch
/// darkest, a settling lighter, time not used hatched, and the time before
/// the test left plain.
fn paint_phase(painter: &egui::Painter, rect: Rect, phase: &str, vis: &egui::Visuals) {
    let text = vis.weak_text_color();
    match phase {
        MEASURED => {
            painter.rect_filled(rect, 0.0, text.gamma_multiply(MEASURED_SHADE));
        }
        crate::guided::NOT_USED => {
            let p = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
            let stroke = Stroke::new(1.0_f32, text.gamma_multiply(0.22));
            let pitch = 7.0;
            let h = rect.height();
            let mut x = rect.left() - h;
            while x < rect.right() {
                p.line_segment(
                    [Pos2::new(x, rect.bottom()), Pos2::new(x + h, rect.top())],
                    stroke,
                );
                x += pitch;
            }
        }
        crate::guided::BEFORE_TEST => {}
        _ => {
            painter.rect_filled(rect, 0.0, text.gamma_multiply(SETTLING_SHADE));
        }
    }
}

/// Shade the plotting area `r` by a Positions Test's phases: each measured
/// stretch, and each stretch in `settles` by what it is, with a line where
/// the test began.
pub fn paint_phases(
    painter: &egui::Painter,
    r: Rect,
    settles: &[(f64, f64, &str)],
    measured: &[(f64, f64)],
    at: impl Fn(f64) -> Pos2,
    horizontal: bool,
    vis: &egui::Visuals,
) {
    let along = |p: Pos2| if horizontal { p.x } else { p.y };
    let (lo, hi) = if horizontal {
        (r.left(), r.right())
    } else {
        (r.top(), r.bottom())
    };
    let span = |a: f64, b: f64| {
        let (pa, pb) = (along(at(a)), along(at(b.min(1e12))));
        let (s0, s1) = (pa.min(pb).max(lo), pa.max(pb).min(hi));
        (s1 > s0).then(|| {
            if horizontal {
                Rect::from_x_y_ranges(s0..=s1, r.y_range())
            } else {
                Rect::from_x_y_ranges(r.x_range(), s0..=s1)
            }
        })
    };
    let phases = measured
        .iter()
        .map(|&(a, b)| (a, b, MEASURED))
        .chain(settles.iter().copied());
    for (a, b, phase) in phases {
        if let Some(rect) = span(a, b) {
            paint_phase(painter, rect, phase, vis);
        }
    }
    let edge = Stroke::new(1.0_f32, vis.weak_text_color().gamma_multiply(0.6));
    for &(_, b, _) in settles.iter().filter(|s| s.2 == crate::guided::BEFORE_TEST) {
        let p = along(at(b));
        if p >= lo && p <= hi {
            if horizontal {
                painter.vline(p, r.y_range(), edge);
            } else {
                painter.hline(r.x_range(), p, edge);
            }
        }
    }
}

/// How near a masked stretch's edge, in points along the time, a drag
/// must begin to move that edge rather than mask anew.
const EDGE_REACH: f32 = 8.0;

/// The masked stretch with an edge within a few points of `p` along the
/// time axis: its start and end, and whether the edge is its start.
fn edge_near(masked: &[(f64, f64)], p: Pos2, g: &Geom) -> Option<(f64, f64, bool)> {
    let along = |q: Pos2| if g.horizontal { q.x } else { q.y };
    let gap = |t: f64| (along(g.pos(0.0, t)) - along(p)).abs();
    masked
        .iter()
        .flat_map(|&(a, b)| [(a, b, true, gap(a)), (a, b, false, gap(b))])
        .filter(|e| e.3 <= EDGE_REACH)
        .min_by(|x, y| x.3.total_cmp(&y.3))
        .map(|(a, b, start, _)| (a, b, start))
}

/// How strongly a masked beat's dot shows its colour: 30% of the way
/// from the strip's ground (Hugh, #1068 item 2).
const MASKED_BEAT: f32 = 0.3;

/// Shade the masked stretches across the plotting area `r` between
/// `oldest` and `end` (the track names them), and the stretch being
/// dragged over (`dragging`) more strongly.
/// `at` places a time on the time axis; `horizontal` when time runs
/// across.
#[allow(clippy::too_many_arguments)]
fn paint_masked(
    painter: &egui::Painter,
    r: Rect,
    masked: &[(f64, f64)],
    dragging: Option<(f64, f64)>,
    oldest: f64,
    end: f64,
    at: impl Fn(f64) -> Pos2,
    horizontal: bool,
    vis: &egui::Visuals,
) {
    let painter = painter.with_clip_rect(r);
    let band = |a: f64, b: f64| {
        let (p, q) = (at(a.max(oldest)), at(b.min(end)));
        if horizontal {
            Rect::from_x_y_ranges(p.x.min(q.x)..=p.x.max(q.x), r.y_range())
        } else {
            Rect::from_x_y_ranges(r.x_range(), p.y.min(q.y)..=p.y.max(q.y))
        }
    };
    let ink = vis.weak_text_color();
    let hatch = crate::theme::palette(vis.dark_mode)
        .text_tertiary
        .gamma_multiply(crate::theme::MASK_WASH);
    for &(a, b) in masked.iter().filter(|m| m.1 > oldest && m.0 < end) {
        crate::theme::hatch(&painter, band(a, b), hatch, 2.5, 6.0);
    }
    if let Some((a, b)) = dragging {
        let rect = band(a, b);
        painter.rect_filled(rect, 0.0, ink.gamma_multiply(0.28));
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0_f32, ink),
            egui::StrokeKind::Inside,
        );
    }
}

/// Draw the strip for the beats up to `end_s` and report what the mouse
/// did to it: the wheel or dragging along the time axis looks back, Cmd
/// or Ctrl and the wheel (or a pinch) zooms the length, Alt and the wheel
/// zooms the width, dragging
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
    let vis = ui.visuals().clone();
    painter.rect_filled(resp.rect, 2.0, vis.extreme_bg_color);
    let oldest = end_s - view.span_s;
    let overlays: Vec<(&Overlay, Scale)> = extras
        .overlays
        .iter()
        .filter_map(|o| {
            o.range(oldest, end_s)
                .map(|range| (o, held_scale(ui, o.name, range)))
        })
        .collect();
    let row_h = 15.0;
    let font = FontId::proportional(11.0);
    let cols: Vec<f32> = overlays
        .iter()
        .map(|(o, s)| scale_column(ui, &font, o, *s))
        .collect();
    let (r, lane) = strip_rects(resp.rect, view, extras.gutter, &cols, row_h);
    if view.horizontal {
        crate::theme::note_time_span(ui.ctx(), r.x_range());
    }
    let g = Geom {
        r,
        half: view.half_width_ms,
        span: view.span_s,
        end: end_s,
        horizontal: view.horizontal,
        sweep: if view.still {
            Sweep::Still
        } else {
            Sweep::Moving
        },
    };
    let f = Frame {
        painter,
        outer: resp.rect,
        lane,
        g,
        view,
        extras,
        vis,
        pal: crate::theme::pal(ui),
        font,
        row_h,
        cols,
        oldest: g.oldest(),
    };
    let mut input = StripInput::default();
    let drags = f.pointer(ui, &resp, &mut input);
    let (step, ms_lines) = f.paint_ms_grid();
    let time_lines = f.paint_time_grid();
    // Masked beats lie under the position washes and the hatching.
    if let Some(anchor) = anchor {
        f.paint_beats(beats, period_s, anchor, true);
    }
    // Stretches and lines a lap at a time on still paper, so none runs
    // across the wrap or the blank ahead of the pen.
    let laps = f.laps();
    for lap in &laps {
        lap.paint_layers(&drags);
    }
    f.paint_grid(&ms_lines, &time_lines);
    f.paint_key(beats, period_s, anchor);
    f.paint_scales(&overlays);
    let Some(anchor) = anchor else {
        return input;
    };
    f.paint_beats(beats, period_s, anchor, false);
    for lap in &laps {
        lap.paint_overlay_lines(&overlays);
        lap.paint_rate_line(beats, period_s, anchor, step);
    }
    f.paint_cursor();
    input
}

/// How wide an overlay's axis column right of the strip is: its widest
/// text (its name, its unit, or a number past its tick) and a gap.
fn scale_column(ui: &egui::Ui, font: &FontId, o: &Overlay, s: Scale) -> f32 {
    let wide = |words: String| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(words, font.clone(), Color32::WHITE)
                .size()
                .x
        })
    };
    let numbers = ticks(s)
        .map(|v| wide(axis_number(v, s.2)))
        .fold(0.0_f32, f32::max);
    let text = wide(o.short.into()).max(wide(unit_line(o)));
    text.max(TICK + 2.0 + numbers) + SCALE_GAP
}

/// An overlay's unit as its axis writes it, under its name: "(s/d)".
fn unit_line(o: &Overlay) -> String {
    format!("({})", o.unit.trim())
}

/// The plotting area inside its axes, and the track's lane: time labels
/// down the left (or along the bottom when lying across), ms labels along
/// the bottom (or down the left), and a scale for each overlay on top (or
/// down the right, in columns `cols` wide, their names and units over
/// them). The track of positions runs along the top when the strip lies
/// across, the key in the corners either side of it; else the track is a
/// column on the right, under the key.
fn strip_rects(
    outer: Rect,
    view: &StripView,
    gutter: f32,
    cols: &[f32],
    row_h: f32,
) -> (Rect, Rect) {
    let lane_h = row_h + 4.0;
    let overlays = cols.len();
    let r = if view.horizontal {
        // Room over the plot for each axis's second line, its unit, clear
        // of its top number.
        let units = if cols.is_empty() { 0.0 } else { row_h - 4.0 };
        Rect::from_min_max(
            outer.min + Vec2::new(gutter.max(crate::theme::GUTTER_MIN), 3.0 + lane_h + units),
            outer.max - Vec2::new(4.0 + cols.iter().sum::<f32>(), row_h + 2.0),
        )
    } else {
        // Two lines under the plot: the ms numbers, and "Late" and
        // "Early" on their own line under the end ones (#1278).
        Rect::from_min_max(
            outer.min + Vec2::new(36.0, row_h + SCALE_ROW * overlays as f32 + 4.0),
            outer.max - Vec2::new(8.0 + lane_h, 2.0 * row_h + 2.0),
        )
    };
    let lane = if view.horizontal {
        Rect::from_min_max(
            Pos2::new(r.left(), r.top() - lane_h + 1.0),
            Pos2::new(r.right(), r.top() - 2.0),
        )
    } else {
        Rect::from_min_max(
            Pos2::new(r.right() + 2.0, r.top()),
            Pos2::new(r.right() + lane_h, r.bottom()),
        )
    };
    (r, lane)
}

/// One frame of the strip: where it draws, and what with.
#[derive(Clone)]
struct Frame<'a> {
    painter: egui::Painter,
    outer: Rect,
    /// The track's lane.
    lane: Rect,
    g: Geom,
    view: &'a StripView,
    extras: &'a Extras<'a>,
    vis: egui::Visuals,
    pal: &'static crate::theme::Palette,
    font: FontId,
    /// A line of the key's height.
    row_h: f32,
    /// Each overlay axis's column's width, when the strip lies across.
    cols: Vec<f32>,
    /// The time at the strip's oldest end.
    oldest: f64,
}

/// What a drag on the strip is doing this frame.
#[derive(Default)]
struct Drags {
    /// The change of position being moved, where it was.
    moving_change: Option<f64>,
    /// Where that change is now.
    change_now: Option<f64>,
    /// The stretch being masked so far.
    masking_now: Option<(f64, f64)>,
    /// The masked stretch whose edge is being dragged.
    reshaping: Option<(f64, f64)>,
    /// The stretch the Position Brush is painting so far.
    painting_now: Option<(f64, f64)>,
}

impl Frame<'_> {
    /// This frame a lap at a time ([`Geom::laps`]): each laid out without
    /// wrapping and drawing only where its lap shows. Moving paper is one.
    fn laps(&self) -> Vec<Self> {
        self.g
            .laps()
            .into_iter()
            .map(|(g, share)| Frame {
                painter: self.painter.with_clip_rect(self.g.clip(self.outer, share)),
                g,
                ..self.clone()
            })
            .collect()
    }

    /// The time at `p`, kept on the strip.
    fn at(&self, p: Option<Pos2>) -> Option<f64> {
        p.map(|p| self.g.time_at(p).clamp(self.oldest, self.g.end))
    }

    /// A change of position on the track within reach of `p`: its time.
    fn change_near(&self, p: Pos2) -> Option<f64> {
        if !self.lane.expand(2.0).contains(p) {
            return None;
        }
        let along = |q: Pos2| if self.view.horizontal { q.x } else { q.y };
        self.extras
            .track
            .iter()
            .skip(1)
            .map(|s| s.0)
            .map(|t| (t, (along(self.g.pos(0.0, t)) - along(p)).abs()))
            .filter(|c| c.1 <= EDGE_REACH)
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .map(|c| c.0)
    }

    /// Over a settle band on the track: how long it is.
    fn band_at(&self, p: Pos2) -> Option<f64> {
        if !self.lane.contains(p) {
            return None;
        }
        let t = self.g.time_at(p);
        self.extras.track.iter().find_map(|&(a, b, _, _)| {
            let (s0, s1) = settle_band(a, b, self.extras.settles)?;
            (t >= s0 && t < s1).then_some(s1 - s0)
        })
    }

    /// Over a painted stretch on the track: the time there.
    fn painted_at(&self, p: Pos2) -> Option<f64> {
        let t = self.g.time_at(p);
        (self.lane.contains(p) && self.extras.painted.iter().any(|&(a, b)| t >= a && t < b))
            .then_some(t)
    }

    /// The two-way arrow along the time axis.
    fn resize_cursor(&self, ui: &egui::Ui) {
        ui.ctx().set_cursor_icon(if self.view.horizontal {
            egui::CursorIcon::ResizeHorizontal
        } else {
            egui::CursorIcon::ResizeVertical
        });
    }

    /// What the mouse did to the strip, into `input`, and what a drag on
    /// it is doing.
    fn pointer(&self, ui: &egui::Ui, resp: &egui::Response, input: &mut StripInput) -> Drags {
        let extras = self.extras;
        let mut drags = Drags {
            moving_change: self.change_drag(ui, resp),
            ..Default::default()
        };
        self.pan_and_zoom(ui, resp, input, drags.moving_change.is_some());
        self.hover(resp, input);
        if let Some(was) = drags.moving_change {
            let now = self.at(resp.interact_pointer_pos());
            drags.change_now = now;
            if resp.drag_stopped_by(egui::PointerButton::Primary) {
                input.move_change = now.map(|t| (was, t));
                ui.data_mut(|d| d.remove::<f64>(resp.id.with("change-edge")));
            }
            self.resize_cursor(ui);
        } else if let (Some(_), false) = (extras.brush, extras.masking) {
            drags.painting_now = self.brush_drag(ui, resp, input);
        } else if extras.masking {
            self.mask_drag(ui, resp, input, &mut drags);
        } else if resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if resp
            .hover_pos()
            .is_some_and(|p| self.change_near(p).is_some())
        {
            self.resize_cursor(ui);
        }
        drags
    }

    /// A drag that begins on a change of position in the track moves it,
    /// whatever tool is on, unless the Mask tool has a masked edge there.
    /// The change being moved, where it was.
    fn change_drag(&self, ui: &egui::Ui, resp: &egui::Response) -> Option<f64> {
        let change_id = resp.id.with("change-edge");
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin());
            if let Some(p) = origin.or(resp.interact_pointer_pos()) {
                let mask_edge =
                    self.extras.masking && edge_near(self.extras.masked, p, &self.g).is_some();
                if let Some(t) = self.change_near(p).filter(|_| !mask_edge) {
                    ui.data_mut(|d| d.insert_temp(change_id, t));
                }
            }
        }
        ui.data(|d| d.get_temp(change_id))
    }

    /// A drag moves the strip; with the Mask tool or the Position Brush
    /// on, or a change being moved, the left button does that and the
    /// right one still moves it. The wheel or a swipe pans time, as a
    /// drag does; Cmd (Mac) or Ctrl with the wheel, or a pinch, zooms time
    /// about the pointer; Alt with the wheel zooms the width. A double
    /// click resets it.
    fn pan_and_zoom(
        &self,
        ui: &egui::Ui,
        resp: &egui::Response,
        input: &mut StripInput,
        moving_change: bool,
    ) {
        let (r, view, g) = (self.g.r, self.view, &self.g);
        let tool = self.extras.masking || self.extras.brush.is_some() || moving_change;
        let mut d = if tool && !resp.dragged_by(egui::PointerButton::Secondary) {
            Vec2::ZERO
        } else {
            resp.drag_delta()
        };
        if resp.hovered() {
            let (scroll, zoom, alt) =
                ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.alt));
            let wheel = scroll.x + scroll.y;
            if wheel != 0.0 && alt {
                // Alt and the wheel zoom the width.
                input.width_factor = (-wheel as f64 * 0.003).exp();
            } else if view.horizontal {
                // Either way the wheel or a swipe goes, along time.
                d.x += wheel;
            } else {
                d.y += wheel;
            }
            if zoom != 1.0 {
                input.span_factor = 1.0 / zoom as f64;
                // Still paper zooms about the pen: a time's place on it
                // is its place in its lap, which a new Length moves.
                input.zoom_at = resp
                    .hover_pos()
                    .filter(|p| r.contains(*p) && g.sweep == Sweep::Moving)
                    .map(|p| g.time_at(p));
            }
        }
        if view.horizontal {
            input.time_shift_s = -(d.x / r.width().max(1.0)) as f64 * view.span_s;
            input.lead_shift_ms = -(d.y / (r.height() / 2.0).max(1.0)) as f64 * view.half_width_ms;
        } else {
            input.time_shift_s = (d.y / r.height().max(1.0)) as f64 * view.span_s;
            input.lead_shift_ms = (d.x / (r.width() / 2.0).max(1.0)) as f64 * view.half_width_ms;
        }
        input.reset = resp.double_clicked();
    }

    /// Over a settle band on the track, what it is and how long, whatever
    /// word fits it; over a painted stretch, the painting and the way back.
    /// A right click on a painted stretch in the track's lane takes the
    /// painting off; anywhere else, on a masked stretch, it brings that
    /// back, Mask on or off.
    fn hover(&self, resp: &egui::Response, input: &mut StripInput) {
        if let Some(len) = resp.hover_pos().and_then(|p| self.band_at(p)) {
            resp.clone().on_hover_text_at_pointer(format!(
                "Settling, {}: not read",
                crate::fields::duration(len)
            ));
        } else if resp.hover_pos().and_then(|p| self.painted_at(p)).is_some() {
            resp.clone().on_hover_text_at_pointer(
                "Painted with the Position Brush. Right-click to remove the painting and bring \
                 back the positions recorded.",
            );
        }
        input.hover_t = resp
            .hover_pos()
            .filter(|p| self.g.r.contains(*p))
            .map(|p| self.g.time_at(p));
        if resp.secondary_clicked() {
            match resp.interact_pointer_pos().and_then(|p| self.painted_at(p)) {
                Some(t) => input.unpaint_at = Some(t),
                None => input.unmask_at = self.at(resp.interact_pointer_pos()),
            }
        }
    }

    /// The Position Brush: where a drag began, kept between frames. The
    /// stretch it covers so far.
    fn brush_drag(
        &self,
        ui: &egui::Ui,
        resp: &egui::Response,
        input: &mut StripInput,
    ) -> Option<(f64, f64)> {
        let mut painting_now = None;
        let id = resp.id.with("paint-from");
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin());
            if let Some(t) = self.at(origin.or(resp.interact_pointer_pos())) {
                ui.data_mut(|d| d.insert_temp(id, t));
            }
        }
        let from: Option<f64> = ui.data(|d| d.get_temp(id));
        if let (Some(a), Some(b)) = (from, self.at(resp.interact_pointer_pos())) {
            painting_now = Some((a.min(b), a.max(b)));
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            input.paint = painting_now;
            ui.data_mut(|d| d.remove::<f64>(id));
        }
        let on_change = resp
            .hover_pos()
            .is_some_and(|p| self.change_near(p).is_some());
        if resp.dragged_by(egui::PointerButton::Secondary) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if resp.hovered() && on_change {
            self.resize_cursor(ui);
        } else if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        painting_now
    }

    /// The Mask tool: where a drag began, kept between frames. A drag that
    /// begins on the edge of a masked stretch moves that edge instead. The
    /// stretch it covers so far, and the masked stretch being reshaped, go
    /// in `drags`.
    fn mask_drag(
        &self,
        ui: &egui::Ui,
        resp: &egui::Response,
        input: &mut StripInput,
        drags: &mut Drags,
    ) {
        let (masked, g) = (self.extras.masked, &self.g);
        let mut masking_now = None;
        let mut reshaping = None;
        let id = resp.id.with("mask-from");
        let edge_id = resp.id.with("mask-edge");
        if resp.drag_started_by(egui::PointerButton::Primary) {
            // Where the button went down: a drag only starts once the
            // pointer has moved a few points on, past an edge's reach.
            let origin = ui.input(|i| i.pointer.press_origin());
            if let Some(p) = origin.or(resp.interact_pointer_pos()) {
                match edge_near(masked, p, g) {
                    Some(e) => ui.data_mut(|d| d.insert_temp(edge_id, e)),
                    None => {
                        if let Some(t) = self.at(Some(p)) {
                            ui.data_mut(|d| d.insert_temp(id, t));
                        }
                    }
                }
            }
        }
        let from: Option<f64> = ui.data(|d| d.get_temp(id));
        let edge: Option<(f64, f64, bool)> = ui.data(|d| d.get_temp(edge_id));
        let now = self.at(resp.interact_pointer_pos());
        if let (Some(a), Some(b)) = (from, now) {
            masking_now = Some((a.min(b), a.max(b)));
        }
        if let (Some((a, b, start)), Some(t)) = (edge, now) {
            // The other edge stays put; dragging past it turns the
            // stretch round.
            let fixed = if start { b } else { a };
            reshaping = Some((a, b));
            masking_now = Some((fixed.min(t), fixed.max(t)));
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            match (edge, masking_now) {
                (Some((a, b, _)), Some(new)) => input.reshape = Some(((a, b), new)),
                _ => input.mask = masking_now,
            }
            ui.data_mut(|d| {
                d.remove::<f64>(id);
                d.remove::<(f64, f64, bool)>(edge_id);
            });
        } else if resp.clicked() {
            input.unmask_at = now;
        }
        let on_edge = resp
            .hover_pos()
            .is_some_and(|p| edge_near(masked, p, g).is_some() || self.change_near(p).is_some());
        if resp.dragged_by(egui::PointerButton::Secondary) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if resp.hovered() && (on_edge || edge.is_some()) {
            self.resize_cursor(ui);
        } else if resp.hovered() {
            // A cross for a new mask, unlike the edge's two-way arrow on
            // every system: on a Mac the column and two-way arrows look
            // the same.
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        drags.masking_now = masking_now;
        drags.reshaping = reshaping;
    }

    /// The labels of the grid along the strip: a nice step of ms, on the
    /// ms axis, with "Early" and "Late" at its ends. The step, and where
    /// its lines fall across the strip, in points ([`Frame::paint_grid`]
    /// draws them).
    fn paint_ms_grid(&self) -> (f64, Vec<f32>) {
        let (painter, g, r, font) = (&self.painter, &self.g, self.g.r, &self.font);
        let (half, horizontal) = (g.half, g.horizontal);
        let mut lines = Vec::new();
        let text = self.vis.weak_text_color();
        let step = round_step(half / 4.0);
        // Every line, or on a strip too short for that the fewest lines
        // that keep the labels apart on the 1-2-5 series (Hugh, #1512);
        // on one too short for even that, only 0, "Early" and "Late".
        let step_px =
            step / half * f64::from(if horizontal { r.height() } else { r.width() }) / 2.0;
        let need = if horizontal {
            2.0 * font.size + 4.0
        } else {
            60.0
        };
        // 0 ms is labelled even when nothing else fits.
        let every = label_every(step, step_px as f32, need).unwrap_or(i64::MAX);
        let mut k = (-half / step).ceil() as i64;
        while (k as f64) * step < half + 1e-9 {
            let ms = k as f64 * step;
            let a = g.pos(ms, g.end);
            lines.push(if horizontal { a.y } else { a.x });
            // Labels, leaving the ends for "Early" and "Late".
            let (at, align, room) = if horizontal {
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
            let labelled = k % every == 0;
            if labelled && room > if horizontal { 14.0 } else { 34.0 } {
                painter.text(at, align, axis_ms(ms), font.clone(), text);
            }
            k += 1;
        }
        let (early_at, early_align, late_at, late_align) = if horizontal {
            (
                Pos2::new(r.left() - 4.0, r.top()),
                Align2::RIGHT_TOP,
                Pos2::new(r.left() - 4.0, r.bottom()),
                Align2::RIGHT_BOTTOM,
            )
        } else {
            // On the line under the numbers, never run into the end ones.
            let y = r.bottom() + 2.0 + self.row_h;
            (
                Pos2::new(r.right(), y),
                Align2::RIGHT_TOP,
                Pos2::new(r.left(), y),
                Align2::LEFT_TOP,
            )
        };
        // Too short a strip for both to stand apart names neither.
        let ends = if horizontal { r.height() } else { r.width() } > 2.0 * font.size + 4.0;
        if ends {
            painter.text(early_at, early_align, "Early", font.clone(), text);
            painter.text(late_at, late_align, "Late", font.clone(), text);
        }
        (step, lines)
    }

    /// The labels of the time lines across the strip, at whole seconds,
    /// minutes or hours, on the time axis; and where the lines fall along
    /// the strip, in points. Lines are far enough apart for their labels:
    /// lying across, a label's width; down, a label's height, so a
    /// short strip down still names each minute (#1268).
    fn paint_time_grid(&self) -> Vec<f32> {
        let (painter, g, r) = (&self.painter, &self.g, self.g.r);
        let mut lines = Vec::new();
        let text = self.vis.weak_text_color();
        let along_px = if g.horizontal { r.width() } else { r.height() } as f64;
        // The least room between two labelled lines, points: across, a
        // label's width ("10:00:00"); down, its height with space each side.
        let gap = if g.horizontal { 90.0 } else { 40.0 };
        let want = g.span * gap / along_px.max(1.0);
        let tstep = crate::theme::TIME_STEPS
            .iter()
            .copied()
            .find(|&s| s >= want)
            .unwrap_or(172800.0);
        let mut t = (g.end / tstep).floor() * tstep;
        // Nothing before the session started.
        while t > self.oldest && t >= -1e-9 {
            let a = g.pos(-g.half, t);
            lines.push(if g.horizontal { a.x } else { a.y });
            let (at, align) = if g.horizontal {
                (Pos2::new(a.x, r.bottom() + 2.0), Align2::CENTER_TOP)
            } else {
                (Pos2::new(r.left() - 4.0, a.y), Align2::RIGHT_CENTER)
            };
            painter.text(at, align, length_text(t), self.font.clone(), text);
            t -= tstep;
        }
        lines
    }

    /// The grid at `ms` and `time` (from [`Frame::paint_ms_grid`] and
    /// [`Frame::paint_time_grid`]), 0 ms as a zero line and the left and
    /// bottom axes: at the strengths every plot draws them, over the wash
    /// so they keep their strength on it.
    fn paint_grid(&self, ms: &[f32], time: &[f32]) {
        let g = &self.g;
        let zero = g.pos(0.0, g.end);
        let (lines, zero) = if g.horizontal {
            ((time, ms), Zero::Across(zero.y))
        } else {
            ((ms, time), Zero::Down(zero.x))
        };
        strength::paint(&self.painter, g.r, lines, zero, self.pal);
    }

    /// Behind the beats: the test's phases or the track's wash, the
    /// changes of position, the masked stretches, the track, and the
    /// stretch being painted; each as a drag will leave it.
    fn paint_layers(&self, drags: &Drags) {
        let (painter, r, extras, vis) = (&self.painter, self.g.r, self.extras, &self.vis);
        let horizontal = self.g.horizontal;
        let place = |t: f64| self.g.pos(0.0, t);
        if extras.measured.is_empty() {
            // The track's teal washed faintly down the plot behind the beats.
            let plot = painter.with_clip_rect(r);
            for &(a, b, _, _) in extras.track {
                let settled = settled_from(a, b, extras.settles);
                paint_density(
                    &plot,
                    r,
                    a,
                    b,
                    settled,
                    self.pal.positions,
                    self.pal.track,
                    crate::theme::TRACK_WASH,
                    &place,
                    horizontal,
                );
            }
        } else {
            paint_phases(
                painter,
                r,
                extras.settles,
                extras.measured,
                place,
                horizontal,
                vis,
            );
        }
        // A line at each change of position, where the one being dragged
        // will be.
        let bands: Vec<(f64, f64, &str)> = extras
            .bands
            .iter()
            .map(|&(a, b, n)| match (drags.moving_change, drags.change_now) {
                (Some(was), Some(now)) if (a - was).abs() < 1e-6 => (now, b, n),
                _ => (a, b, n),
            })
            .collect();
        paint_bands(painter, r, &bands, place, horizontal, false, false, vis);
        // The stretch whose edge is being dragged is drawn as it will be.
        let masked: Vec<(f64, f64)> = extras
            .masked
            .iter()
            .copied()
            .filter(|&m| Some(m) != drags.reshaping)
            .collect();
        paint_masked(
            painter,
            r,
            &masked,
            drags.masking_now,
            self.oldest,
            self.g.end,
            place,
            horizontal,
            vis,
        );
        let mut track_masked = masked;
        track_masked.extend(drags.masking_now);
        paint_track(
            painter,
            self.lane,
            &self.dragged_track(drags),
            extras.settles,
            &track_masked,
            drags.painting_now.filter(|_| extras.brush.is_some()),
            place,
            horizontal,
            vis,
            self.pal,
        );
        if let (Some(code), Some((a, b))) = (extras.brush, drags.painting_now) {
            self.paint_brushed(code, a, b);
        }
    }

    /// The track, with a change being dragged where it will be.
    fn dragged_track(&self, drags: &Drags) -> Vec<(f64, f64, &str, &str)> {
        let mut t: Vec<(f64, f64, &str, &str)> = self.extras.track.to_vec();
        let (Some(was), Some(now)) = (drags.moving_change, drags.change_now) else {
            return t;
        };
        if let Some(i) = t.iter().position(|s| (s.0 - was).abs() < 1e-6) {
            if i > 0 {
                let lo = t[i - 1].0 + 0.5;
                let hi = t[i].1 - 0.5;
                let now = now.clamp(lo, hi.max(lo));
                t[i].0 = now;
                t[i - 1].1 = now;
            }
        }
        t
    }

    /// The stretch from `a` to `b` being painted `code`, in teal over the
    /// track and the plot, with the position and its times.
    fn paint_brushed(&self, code: &str, a: f64, b: f64) {
        let (painter, r, pal, vis) = (&self.painter, self.g.r, self.pal, &self.vis);
        let horizontal = self.g.horizontal;
        let place = |t: f64| self.g.pos(0.0, t);
        if let Some(rect) = along_rect(r, a, b, &place, horizontal) {
            painter.rect_filled(rect, 0.0, pal.positions.gamma_multiply(0.18));
        }
        let Some(seg) = along_rect(self.lane, a, b, &place, horizontal) else {
            return;
        };
        painter.rect_filled(seg, 0.0, pal.positions.gamma_multiply(0.85));
        painter.rect_stroke(
            seg,
            0.0,
            Stroke::new(1.0_f32, pal.positions),
            egui::StrokeKind::Inside,
        );
        let words = format!("{code} {} to {}", length_text(a), length_text(b));
        let galley = painter.layout_no_wrap(words, self.font.clone(), vis.strong_text_color());
        let size = galley.size();
        // Inside the plot, beside the stretch's newest end, on a backing.
        let newest = place(b);
        let pos = if horizontal {
            Pos2::new(
                (newest.x - size.x).clamp(r.left() + 2.0, r.right() - size.x - 2.0),
                r.top() + 4.0,
            )
        } else {
            Pos2::new(
                r.right() - size.x - 4.0,
                newest.y.clamp(r.top() + 2.0, r.bottom() - size.y - 2.0),
            )
        };
        let back = Rect::from_min_size(pos, size).expand(3.0);
        painter.rect_filled(back, 3.0, vis.extreme_bg_color.gamma_multiply(0.92));
        painter.rect_stroke(
            back,
            3.0,
            Stroke::new(1.0_f32, pal.positions),
            egui::StrokeKind::Inside,
        );
        painter.galley(pos, galley, vis.strong_text_color());
    }

    /// The key: Tick and Tock in their colours, in the corner left of the
    /// track when the strip lies across, else along the top; and the note.
    fn paint_key(&self, beats: &[Beat], period_s: f64, anchor: Option<Anchor>) {
        let (painter, r, outer, vis, font) =
            (&self.painter, self.g.r, self.outer, &self.vis, &self.font);
        let colors = side_colors(vis.dark_mode);
        if self.g.horizontal {
            self.paint_corner_key(colors, |area| {
                anchor.map_or(0, |a| self.dots_in(beats, period_s, a, area))
            });
        } else {
            let mut at = Pos2::new(r.left(), outer.top() + 3.0);
            for (label, c) in [("Tick", colors[0]), ("Tock", colors[1])] {
                let k = painter.text(at, Align2::LEFT_TOP, label, font.clone(), c);
                at.x = k.right() + 8.0;
            }
        }
        let Some(n) = self.extras.note else {
            return;
        };
        if self.g.horizontal {
            // In the plot's top corner, on a backing.
            let galley = painter.layout_no_wrap(n.to_string(), font.clone(), vis.text_color());
            let pos = Pos2::new(r.right() - galley.size().x - 6.0, r.top() + 4.0);
            let back = Rect::from_min_size(pos, galley.size()).expand(2.0);
            painter.rect_filled(back, 2.0, vis.extreme_bg_color.gamma_multiply(0.85));
            painter.galley(pos, galley, vis.text_color());
        } else {
            painter.text(
                Pos2::new(outer.right() - 6.0, outer.top() + 3.0),
                Align2::RIGHT_TOP,
                n,
                font.clone(),
                vis.text_color(),
            );
        }
    }

    /// The key lying across: in the plot's left corner, Tick over Tock,
    /// each a dot and its word in its side's colour, on a plate of the
    /// plot's ground at 80% under the beats (#1225, Peter's C2); at the
    /// top, or at the bottom where fewer of the dots (`dots`) lie, as the
    /// facts box moves (Hugh, #1536).
    fn paint_corner_key(&self, colors: [Color32; 2], dots: impl Fn(Rect) -> usize) {
        let (painter, r) = (&self.painter, self.g.r);
        let rows = [("Tick", colors[0]), ("Tock", colors[1])]
            .map(|(w, c)| (painter.layout_no_wrap(w.into(), self.font.clone(), c), c));
        let dot = 3.0;
        let text_x = 4.0 + 2.0 * dot + 4.0;
        let row_h = rows.iter().map(|(g, _)| g.size().y).fold(0.0, f32::max);
        let wide = rows.iter().map(|(g, _)| g.size().x).fold(0.0, f32::max);
        let size = Vec2::new(text_x + wide + 4.0, 2.0 * row_h + 4.0);
        let top = Rect::from_min_size(r.min + Vec2::splat(4.0), size);
        let bottom = Rect::from_min_size(Pos2::new(top.left(), r.bottom() - 4.0 - size.y), size);
        let plate = if dots(bottom) < dots(top) {
            bottom
        } else {
            top
        };
        painter.rect_filled(plate, 2.0, self.vis.extreme_bg_color.gamma_multiply(0.8));
        for (i, (g, c)) in rows.into_iter().enumerate() {
            let top = plate.top() + 2.0 + i as f32 * row_h;
            let mid = top + row_h / 2.0;
            painter.circle_filled(Pos2::new(plate.left() + 4.0 + dot, mid), dot, c);
            painter.galley(Pos2::new(plate.left() + text_x, top), g, c);
        }
    }

    /// Each overlay's axis: a 1 pt line in the reading's colour with a
    /// tick and a number at each round step, its name and unit, all in its
    /// colour; in a column of its own right of the strip, or a row of its
    /// own above it. They have no grid lines: the strip's grid is the
    /// beats'.
    fn paint_scales(&self, overlays: &[(&Overlay, Scale)]) {
        for (i, (o, s)) in overlays.iter().enumerate() {
            if self.g.horizontal {
                self.scale_down(i, o, *s);
            } else {
                self.scale_across(i, o, *s);
            }
        }
    }

    /// Where `v` on an overlay's scale `s` falls along the strip's ms
    /// axis, in points: the scale spans the strip, as its line does.
    fn scale_at(&self, s: Scale, v: f64) -> f32 {
        let p = self.g.pos(scale_ms(s, v, self.g.half), self.g.end);
        if self.g.horizontal {
            p.y
        } else {
            p.x
        }
    }

    /// An overlay's axis in column `i` right of the strip, under its
    /// short name with its unit on the line under that.
    fn scale_down(&self, i: usize, o: &Overlay, s: Scale) {
        let (painter, r, font) = (&self.painter, self.g.r, &self.font);
        let x = r.right() + SCALE_GAP + self.cols[..i].iter().sum::<f32>();
        let (line, ink) = (Stroke::new(1.0_f32, o.color), self.pal.axis_ink(o.color));
        let text = |at: Pos2, align: Align2, words: String| {
            painter.text(at, align, words, font.clone(), ink);
        };
        // The unit just clear of the top number, the name over it.
        let top_tick = ticks(s)
            .map(|v| self.scale_at(s, v))
            .fold(r.bottom(), f32::min);
        let unit = painter.text(
            Pos2::new(x, top_tick - font.size / 2.0 - 1.0),
            Align2::LEFT_BOTTOM,
            unit_line(o),
            font.clone(),
            ink,
        );
        let named = painter.text(
            Pos2::new(x, unit.top() + 1.0),
            Align2::LEFT_BOTTOM,
            o.short,
            font.clone(),
            ink,
        );
        // The short name in full on a hover.
        let ctx = painter.ctx();
        if ctx
            .pointer_hover_pos()
            .is_some_and(|p| named.union(unit).contains(p))
        {
            egui::Tooltip::always_open(
                ctx.clone(),
                painter.layer_id(),
                egui::Id::new(("strip scale name", i)),
                egui::PopupAnchor::Pointer,
            )
            .gap(12.0)
            .show(|ui| ui.label(format!("{} {}", o.name, unit_line(o))));
        }
        painter.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], line);
        let mut last = f32::INFINITY;
        for v in ticks(s) {
            let y = self.scale_at(s, v);
            painter.line_segment([Pos2::new(x, y), Pos2::new(x + TICK, y)], line);
            // Numbers too close to stand apart: every other one.
            if (last - y).abs() > font.size + 1.0 {
                let at = Pos2::new(x + TICK + 2.0, y);
                text(at, Align2::LEFT_CENTER, axis_number(v, s.2));
                last = y;
            }
        }
    }

    /// An overlay's axis in row `i` above the strip: its short name at the
    /// left with its unit on the line under it, as every axis writes them
    /// (#1052, #1278), the numbers over their ticks on the unit's line, the
    /// line under them.
    fn scale_across(&self, i: usize, o: &Overlay, s: Scale) {
        let (painter, r, font) = (&self.painter, self.g.r, &self.font);
        let top = self.outer.top() + 3.0 + self.row_h + SCALE_ROW * i as f32;
        let y = top + SCALE_ROW - 2.0;
        let (line, ink) = (Stroke::new(1.0_f32, o.color), self.pal.axis_ink(o.color));
        let short = painter.text(
            Pos2::new(r.left(), top),
            Align2::LEFT_TOP,
            o.short,
            font.clone(),
            ink,
        );
        let unit = painter.text(
            Pos2::new(r.left(), short.bottom()),
            Align2::LEFT_TOP,
            unit_line(o),
            font.clone(),
            ink,
        );
        let named = short.union(unit);
        painter.line_segment([Pos2::new(r.left(), y), Pos2::new(r.right(), y)], line);
        // The near end's number would sit under the name, so it is left
        // out; the row's hover gives both ends.
        let row = Rect::from_min_max(Pos2::new(r.left(), top), Pos2::new(r.right(), y + 1.0));
        let ctx = painter.ctx();
        if ctx.pointer_hover_pos().is_some_and(|p| row.contains(p)) {
            let range = format!(
                "{}: {} to {} {}",
                o.name,
                axis_number(s.0, s.2),
                axis_number(s.1, s.2),
                o.unit.trim()
            );
            egui::Tooltip::always_open(
                ctx.clone(),
                painter.layer_id(),
                egui::Id::new(("strip scale range", i)),
                egui::PopupAnchor::Pointer,
            )
            .gap(12.0)
            .show(|ui| ui.label(range));
        }
        let mut taken = named;
        for v in ticks(s) {
            let x = self.scale_at(s, v);
            painter.line_segment([Pos2::new(x, y), Pos2::new(x, y - TICK)], line);
            let words = axis_number(v, s.2);
            let galley = painter.layout_no_wrap(words, font.clone(), ink);
            let at = Rect::from_center_size(
                Pos2::new(x, unit.top() + galley.size().y / 2.0),
                galley.size(),
            );
            // The far end's number ends at the strip's end.
            let at = at.translate(Vec2::new((r.right() - at.right()).min(0.0), 0.0));
            // Clear of the name and the number before it.
            if at.left() > taken.right() + 4.0 {
                painter.galley(at.min, galley, ink);
                taken = at;
            }
        }
    }

    /// The beats on show: from the oldest on the strip to its end.
    fn shown<'b>(&self, beats: &'b [Beat]) -> &'b [Beat] {
        let lo = beats.partition_point(|b| b.time < self.oldest);
        let hi = beats.partition_point(|b| b.time <= self.g.end);
        &beats[lo..hi]
    }

    /// Where beat `b` is drawn on the strip.
    fn beat_at(&self, b: &Beat, period_s: f64, anchor: Anchor) -> Pos2 {
        let (g, extras) = (&self.g, self.extras);
        let lead = anchor.lead_on_true(b, period_s, extras.breaks, extras.clock);
        g.pos(wrap(lead, g.half), b.time)
    }

    /// How many of the good beats on show lie in `area`.
    fn dots_in(&self, beats: &[Beat], period_s: f64, anchor: Anchor, area: Rect) -> usize {
        self.shown(beats)
            .iter()
            .filter(|b| b.is_good() && area.contains(self.beat_at(b, period_s, anchor)))
            .count()
    }

    /// The beats on the strip as dots, Tick and Tock in their colours,
    /// faint while the watch settles. With `masked`, only the beats in a
    /// masked stretch, at 30% of their colour over the strip's ground and
    /// opaque, so crowded dots don't add up to full strength: a masked
    /// beat is still there to judge the mask by, but reads as unused
    /// (Hugh, #1068 item 2). Without, only the others.
    fn paint_beats(&self, beats: &[Beat], period_s: f64, anchor: Anchor, masked: bool) {
        let (g, r, extras) = (&self.g, self.g.r, self.extras);
        let shown = self.shown(beats);
        // Dots shrink when the strip is crowded.
        let along = if g.horizontal { r.width() } else { r.height() };
        let per_px = shown.len() as f32 / along.max(1.0);
        let radius = (2.2 / per_px.max(1.0).sqrt()).clamp(0.8, 2.2);
        // Past a few dozen beats to a point, the dots only lie on one another:
        // every nth Tick and its Tock are drawn, so a strip days long draws
        // no more dots than a short one crowded to that.
        let every = (per_px / MAX_BEATS_PER_PX).ceil().max(1.0) as i64;
        let colors = side_colors(self.vis.dark_mode);
        let painter = self.painter.with_clip_rect(r.expand(3.0));
        let ground = self.vis.extreme_bg_color;
        for b in shown {
            if !b.is_good() || b.index.div_euclid(2).rem_euclid(every) != 0 {
                continue;
            }
            let in_mask = extras.masked.iter().any(|m| b.time >= m.0 && b.time <= m.1);
            if in_mask != masked {
                continue;
            }
            let p = self.beat_at(b, period_s, anchor);
            let mut c = colors[b.index.rem_euclid(2) as usize];
            if in_mask {
                c = ground.lerp_to_gamma(c, MASKED_BEAT);
            } else if extras.settles.iter().any(|s| b.time > s.0 && b.time <= s.1) {
                // Faint while the watch settles after a move.
                c = c.gamma_multiply(0.3);
            }
            painter.circle_filled(p, radius, c);
        }
    }

    /// The overlays, each scaled across the strip's width.
    fn paint_overlay_lines(&self, overlays: &[(&Overlay, Scale)]) {
        let (g, half) = (&self.g, self.g.half);
        let painter = self.painter.with_clip_rect(g.r);
        for (o, s) in overlays {
            let across = |v: f64| scale_ms(*s, v, half);
            for (a, b) in joined(&o.points, self.oldest - 2.0, g.end) {
                painter.line_segment(
                    [g.pos(across(a[1]), a[0]), g.pos(across(b[1]), b[0])],
                    Stroke::new(1.6_f32, o.color),
                );
            }
        }
    }

    /// The rate reading over its beats, on top of them, wrapping as the
    /// dots do; and faint parallels to it across the whole strip, one per
    /// grid `step`, so the eye can tell whether the dots run parallel to it.
    fn paint_rate_line(&self, beats: &[Beat], period_s: f64, anchor: Anchor, step: f64) {
        let (g, half, extras, pal) = (&self.g, self.g.half, self.extras, self.pal);
        let Some(line) = extras.rate_line else {
            return;
        };
        let Some([(ta, la), (tb, lb)]) =
            line.ends(beats, anchor, period_s, extras.breaks, extras.clock)
        else {
            return;
        };
        let painter = self.painter.with_clip_rect(g.r);
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
                polyline(self.oldest, g.end, k as f64 * step, faint);
            }
        }
        if extras.line {
            // A thin dark edge under the line keeps it readable over the dots.
            polyline(
                ta,
                tb,
                0.0,
                Stroke::new(2.6_f32, self.vis.extreme_bg_color.gamma_multiply(0.7)),
            );
            polyline(ta, tb, 0.0, Stroke::new(1.2_f32, pal.trace_rate));
        }
    }

    /// The cursor's line across the strip with its time, to the
    /// hundredth, at the edge away from the positions' names: along the
    /// bottom when time runs across, at the right otherwise. Kept inside
    /// the plotting area.
    fn paint_cursor(&self) {
        let (g, r, vis) = (&self.g, self.g.r, &self.vis);
        let Some(t) = self
            .extras
            .cursor_t
            .filter(|t| *t >= self.oldest && *t <= g.end)
        else {
            return;
        };
        let painter = self.painter.with_clip_rect(r);
        let (a, b) = (g.pos(-g.half, t), g.pos(g.half, t));
        painter.line_segment([a, b], Stroke::new(1.0_f32, vis.weak_text_color()));
        let galley = painter.layout_no_wrap(
            fmt_time_fine(t),
            FontId::proportional(11.0),
            vis.strong_text_color(),
        );
        let size = galley.size() + Vec2::splat(4.0);
        let rect = if g.horizontal {
            let x = (a.x - size.x / 2.0).clamp(r.left(), (r.right() - size.x).max(r.left()));
            Rect::from_min_size(Pos2::new(x, r.bottom() - size.y - 2.0), size)
        } else {
            let y = (a.y - size.y / 2.0).clamp(r.top(), (r.bottom() - size.y).max(r.top()));
            Rect::from_min_size(Pos2::new(r.right() - size.x - 2.0, y), size)
        };
        painter.rect_filled(rect, 2.0, vis.extreme_bg_color.gamma_multiply(0.9));
        painter.galley(rect.min + Vec2::splat(2.0), galley, vis.strong_text_color());
    }
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

/// A time to the hundredth of a second, such as "1:23.45", for the
/// cursor's label.
pub fn fmt_time_fine(t: f64) -> String {
    let cs = (t.max(0.0) * 100.0).round() as u64;
    let (s, frac) = (cs / 100, cs % 100);
    if s >= 3600 {
        format!("{}:{:02}:{:02}.{frac:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}.{frac:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_overlay_readings_break_the_line_even_without_a_time_gap() {
        let points = [[0.0, 0.2], [1.0, f64::NAN], [2.0, 0.3], [3.0, 0.4]];
        assert_eq!(joined(&points, 0.0, 3.0), vec![(points[2], points[3])]);
    }

    #[test]
    fn readings_further_apart_than_the_gap_are_not_joined() {
        // A 7 h 28 min take opened whole keeps a reading every take/4000 s,
        // 6.7 s apart: joined.
        let step = 26_880.0 / 4000.0;
        let pts: Vec<[f64; 2]> = (0..10).map(|i| [i as f64 * step, 10.0]).collect();
        assert_eq!(joined(&pts, 0.0, 1e9).len(), 9);
        // A real hole in the readings stays a hole.
        let holed: Vec<[f64; 2]> = pts
            .iter()
            .copied()
            .filter(|p| p[0] < 20.0 || p[0] > 50.0)
            .collect();
        assert_eq!(joined(&holed, 0.0, 1e9).len(), holed.len() - 2);
        // Only the pairs in view, and the one reaching into it.
        assert_eq!(joined(&pts, 3.0 * step, 6.0 * step).len(), 3);
    }

    /// A long take worked out again keeps a reading every 20 s; those
    /// read live since come every half second. Both join up, and the line
    /// runs on from one to the other; a stretch with none read is still a
    /// gap, in either.
    #[test]
    fn readings_spaced_two_ways_join_up() {
        let mut pts: Vec<[f64; 2]> = (0..100).map(|i| [i as f64 * 20.0, 1.0]).collect();
        let t0 = pts.last().unwrap()[0];
        pts.extend((1..400).map(|i| [t0 + i as f64 * 0.5, 1.0]));
        assert_eq!(joined(&pts, 0.0, 1e9).len(), pts.len() - 1);
        let holed: Vec<[f64; 2]> = pts
            .iter()
            .copied()
            .filter(|p| !(500.0..600.0).contains(&p[0]) && !(t0 + 50.0..t0 + 60.0).contains(&p[0]))
            .collect();
        assert_eq!(joined(&holed, 0.0, 1e9).len(), holed.len() - 3);
    }

    /// Frames of the strip lying across, 100 s long with 40 to 60 s
    /// masked and the Mask tool on, each given `events`: what it asked for.
    fn masking_frames(ctx: &egui::Context, frames: &[Vec<egui::Event>]) -> Vec<StripInput> {
        masking_frames_with(ctx, egui::Modifiers::default(), frames)
    }

    /// As `masking_frames`, with `modifiers` held throughout.
    fn masking_frames_with(
        ctx: &egui::Context,
        modifiers: egui::Modifiers,
        frames: &[Vec<egui::Event>],
    ) -> Vec<StripInput> {
        let view = StripView {
            half_width_ms: 10.0,
            span_s: 100.0,
            horizontal: true,
            still: false,
        };
        let masked = [(40.0, 60.0)];
        let extras = Extras {
            masked: &masked,
            masking: true,
            ..Default::default()
        };
        frames
            .iter()
            .map(|events| {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 400.0))),
                    events: events.clone(),
                    modifiers,
                    ..Default::default()
                };
                let mut got = StripInput::default();
                let _ = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        got = draw_strip(
                            ui,
                            &[],
                            0.125,
                            None,
                            100.0,
                            &view,
                            &extras,
                            Vec2::new(860.0, 360.0),
                        );
                    });
                });
                got
            })
            .collect()
    }

    /// A drag that begins on a masked stretch's edge moves the edge, even
    /// though the drag only starts once the pointer has moved on past the
    /// edge's reach; the right button moves the strip instead of masking.
    #[test]
    fn a_masked_edge_can_be_grabbed_and_the_right_button_pans() {
        let ctx = egui::Context::default();
        let moved = |x: f32| egui::Event::PointerMoved(Pos2::new(x, 200.0));
        let button = |x: f32, b: egui::PointerButton, pressed: bool| egui::Event::PointerButton {
            pos: Pos2::new(x, 200.0),
            button: b,
            pressed,
            modifiers: Default::default(),
        };
        // Where 40 s is: the time under the pointer, across the strip.
        let mut edge = None;
        for x in (60..860).step_by(2) {
            let got = masking_frames(&ctx, &[vec![moved(x as f32)]]);
            if got[0].hover_t.is_some_and(|t| t >= 40.0) {
                edge = Some(x as f32);
                break;
            }
        }
        let x = edge.expect("40 s is on the strip");
        let primary = egui::PointerButton::Primary;
        let got = masking_frames(
            &ctx,
            &[
                vec![moved(x)],
                vec![button(x, primary, true)],
                vec![moved(x + 4.0)],
                vec![moved(x + 12.0)],
                vec![moved(x + 20.0)],
                vec![button(x + 20.0, primary, false)],
                vec![],
            ],
        );
        let reshaped = got.iter().find_map(|i| i.reshape);
        let ((a, b), (c, d)) = reshaped.expect("the edge moved, no new mask");
        assert_eq!((a, b), (40.0, 60.0));
        assert!(c > 40.0 && d == 60.0, "{c} {d}");
        assert!(got.iter().all(|i| i.mask.is_none()));

        let secondary = egui::PointerButton::Secondary;
        let got = masking_frames(
            &ctx,
            &[
                vec![moved(x - 100.0)],
                vec![button(x - 100.0, secondary, true)],
                vec![moved(x - 90.0)],
                vec![moved(x - 60.0)],
                vec![button(x - 60.0, secondary, false)],
                vec![],
            ],
        );
        assert!(got.iter().all(|i| i.mask.is_none() && i.reshape.is_none()));
        assert!(got.iter().any(|i| i.time_shift_s < 0.0), "{got:?}");
    }

    /// Frames of the strip lying across, 100 s long, with a change of
    /// position at 50 s and `brush` on or off, each given `events`.
    fn track_frames(
        ctx: &egui::Context,
        brush: Option<&str>,
        frames: &[Vec<egui::Event>],
    ) -> Vec<StripInput> {
        painted_frames(ctx, brush, &[], frames)
    }

    /// [`track_frames`] with `painted` stretches over the positions
    /// recorded.
    fn painted_frames(
        ctx: &egui::Context,
        brush: Option<&str>,
        painted: &[(f64, f64)],
        frames: &[Vec<egui::Event>],
    ) -> Vec<StripInput> {
        let view = StripView {
            half_width_ms: 10.0,
            span_s: 100.0,
            horizontal: true,
            still: false,
        };
        let track = [
            (0.0, 50.0, "Dial Up (DU)", "DU"),
            (50.0, f64::INFINITY, "Dial Down (DD)", "DD"),
        ];
        let extras = Extras {
            track: &track,
            brush,
            painted,
            ..Default::default()
        };
        frames
            .iter()
            .map(|events| {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 400.0))),
                    events: events.clone(),
                    ..Default::default()
                };
                let mut got = StripInput::default();
                let _ = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        got = draw_strip(
                            ui,
                            &[],
                            0.125,
                            None,
                            100.0,
                            &view,
                            &extras,
                            Vec2::new(860.0, 360.0),
                        );
                    });
                });
                got
            })
            .collect()
    }

    /// A drag along the strip with the Position Brush on paints that
    /// stretch rather than moving the strip; a drag that begins on a change
    /// of position in the track moves the change, brush or no brush.
    #[test]
    fn the_brush_paints_and_a_change_moves_in_the_track() {
        let ctx = egui::Context::default();
        let at = |x: f32, y: f32| Pos2::new(x, y);
        let moved = |p: Pos2| egui::Event::PointerMoved(p);
        let button = |p: Pos2, pressed: bool| egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let drag = |from: Pos2, to: Pos2| {
            vec![
                vec![moved(from)],
                vec![button(from, true)],
                vec![moved(from + (to - from) * 0.3)],
                vec![moved(from + (to - from) * 0.7)],
                vec![moved(to)],
                vec![button(to, false)],
                vec![],
            ]
        };
        // Where 50 s is across the strip, and the track's row above it.
        let mut x50 = None;
        for x in (60..860).step_by(2) {
            let got = track_frames(&ctx, None, &[vec![moved(at(x as f32, 200.0))]]);
            if got[0].hover_t.is_some_and(|t| t >= 50.0) {
                x50 = Some(x as f32);
                break;
            }
        }
        let x50 = x50.expect("50 s is on the strip");
        let lane_y = (1..120)
            .map(|y| y as f32)
            .find(|&y| {
                track_frames(&ctx, None, &[vec![moved(at(x50, y))]])[0]
                    .hover_t
                    .is_some()
            })
            .expect("the plot's top")
            - 8.0;
        // The brush paints from 20 px before 50 s to 60 px after it.
        let got = track_frames(
            &ctx,
            Some("CD"),
            &drag(at(x50 - 20.0, 200.0), at(x50 + 60.0, 200.0)),
        );
        let (a, b) = got.iter().find_map(|i| i.paint).expect("painted");
        assert!(a < 50.0 && b > 55.0, "{a} {b}");
        assert!(got
            .iter()
            .all(|i| i.time_shift_s == 0.0 && i.move_change.is_none()));
        // The change in the track moves, the brush on or off.
        for brush in [None, Some("CD")] {
            let got = track_frames(&ctx, brush, &drag(at(x50, lane_y), at(x50 + 40.0, lane_y)));
            let (was, now) = got
                .iter()
                .find_map(|i| i.move_change)
                .expect("the change moved");
            assert_eq!(was, 50.0);
            assert!(now > 52.0, "{now}");
            assert!(got
                .iter()
                .all(|i| i.paint.is_none() && i.time_shift_s == 0.0));
        }
        // Without the brush, a drag on the plot still moves the strip.
        let got = track_frames(
            &ctx,
            None,
            &drag(at(x50 - 100.0, 200.0), at(x50 - 40.0, 200.0)),
        );
        assert!(got.iter().any(|i| i.time_shift_s != 0.0));
        assert!(got
            .iter()
            .all(|i| i.paint.is_none() && i.move_change.is_none()));
        // A right click on a painted stretch in the track's lane takes the
        // painting off; on the plot, or on the lane where nothing is
        // painted, it is the masks' right click as before.
        let right = |p: Pos2| {
            let b = |pressed| egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Default::default(),
            };
            vec![vec![moved(p)], vec![b(true)], vec![b(false)], vec![]]
        };
        let painted = [(50.0, 100.0)];
        let got = painted_frames(&ctx, None, &painted, &right(at(x50 + 40.0, lane_y)));
        let t = got.iter().find_map(|i| i.unpaint_at).expect("painting off");
        assert!(t > 50.0 && t < 100.0, "{t}");
        assert!(got.iter().all(|i| i.unmask_at.is_none()));
        for p in [at(x50 + 40.0, 200.0), at(x50 - 100.0, lane_y)] {
            let got = painted_frames(&ctx, None, &painted, &right(p));
            assert!(got.iter().all(|i| i.unpaint_at.is_none()), "{p:?}");
            assert!(got.iter().any(|i| i.unmask_at.is_some()), "{p:?}");
        }
    }

    /// Down, a short strip still names each time line it draws: a 4 min
    /// strip about 200 points tall labels every minute, not every other
    /// (#1268). Across, labels stay a label's width apart.
    #[test]
    fn a_short_strip_down_labels_each_minute() {
        let labels = |horizontal: bool, size: Vec2| {
            let ctx = egui::Context::default();
            let view = StripView {
                half_width_ms: 10.0,
                span_s: 240.0,
                horizontal,
                still: false,
            };
            let extras = Extras {
                gutter: 46.0,
                ..Default::default()
            };
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 400.0))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        draw_strip(ui, &[], 0.125, None, 240.0, &view, &extras, size);
                    });
                },
            );
            let mut found: Vec<String> = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .filter(|t| ["1:00", "2:00", "3:00"].contains(&t.as_str()))
                .collect();
            found.sort();
            found
        };
        assert_eq!(
            labels(false, Vec2::new(460.0, 230.0)),
            ["1:00", "2:00", "3:00"]
        );
        // Across, 400 points wide: two minutes apart, as before.
        assert_eq!(labels(true, Vec2::new(400.0, 230.0)), ["2:00"]);
    }

    /// Lying across, the key is in the plot's top-left corner, Tick over
    /// Tock, on a plate; lying down it is a row over the plot, as before
    /// (#1225, Peter's C2).
    #[test]
    fn the_key_lies_in_the_plots_corner_across_and_over_it_down() {
        for horizontal in [true, false] {
            let ctx = egui::Context::default();
            let view = StripView {
                half_width_ms: 10.0,
                span_s: 100.0,
                horizontal,
                still: false,
            };
            let extras = Extras {
                gutter: 46.0,
                ..Default::default()
            };
            let mut origin = Pos2::ZERO;
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 300.0))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        origin = ui.cursor().min;
                        draw_strip(
                            ui,
                            &[],
                            0.125,
                            None,
                            100.0,
                            &view,
                            &extras,
                            Vec2::new(460.0, 260.0),
                        );
                    });
                },
            );
            let at = |want: &str| {
                out.shapes
                    .iter()
                    .find_map(|c| match &c.shape {
                        egui::Shape::Text(t) if t.galley.text() == want => {
                            Some(c.shape.visual_bounding_rect())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("{want} drawn"))
            };
            let (tick, tock) = (at("Tick"), at("Tock"));
            if horizontal {
                assert!(
                    tick.left() >= origin.x + 46.0,
                    "in the plot, not the gutter: {tick:?}"
                );
                assert!(
                    tick.bottom() <= tock.top() + 1.0,
                    "Tick over Tock: {tick:?} {tock:?}"
                );
                assert!((tick.left() - tock.left()).abs() < 0.5);
            } else {
                assert!((tick.center().y - tock.center().y).abs() < 0.5, "a row");
                assert!(tock.left() > tick.right());
            }
        }
    }

    /// A position's name on the track is never cut through by a masked
    /// stretch drawn over it: it is written in the parts left showing,
    /// or shortened to the code.
    #[test]
    fn a_mask_does_not_cut_through_a_positions_name() {
        let ctx = egui::Context::default();
        let view = StripView {
            half_width_ms: 10.0,
            span_s: 100.0,
            horizontal: true,
            still: false,
        };
        let track = [(0.0, f64::INFINITY, "Crown Up (CU, 3H)", "CU")];
        let masked = [(12.0, 20.0)];
        let extras = Extras {
            track: &track,
            masked: &masked,
            ..Default::default()
        };
        let mut mask_x = None;
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 300.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let origin = ui.cursor().min;
                    draw_strip(
                        ui,
                        &[],
                        0.125,
                        None,
                        100.0,
                        &view,
                        &extras,
                        Vec2::new(460.0, 260.0),
                    );
                    // Where 12 s and 20 s fall: the plot spans the width
                    // between the gutter and the right margin.
                    let left = origin.x + 46.0;
                    let right = origin.x + 460.0 - 4.0;
                    let x = |t: f64| right - ((100.0 - t) / 100.0) as f32 * (right - left);
                    mask_x = Some((x(12.0), x(20.0)));
                });
            },
        );
        let (m0, m1) = mask_x.unwrap();
        let mut names = 0;
        for c in &out.shapes {
            if let egui::Shape::Text(t) = &c.shape {
                let text = t.galley.text();
                if text == "Crown Up (CU, 3H)" || text == "CU" {
                    names += 1;
                    let r = c.shape.visual_bounding_rect();
                    assert!(
                        r.right() <= m0 + 0.5 || r.left() >= m1 - 0.5,
                        "{text} at {r:?} under the mask {m0}..{m1}"
                    );
                }
            }
        }
        assert!(names >= 1, "the position is named");
    }

    /// A settle band says what it is in the word that fits it, and the
    /// position's name starts after it; a mask over part of the band
    /// leaves the word in the part that shows.
    #[test]
    fn a_settle_band_is_named_by_the_word_that_fits() {
        assert_eq!(settle_word_for(64.0), Some("Settling"));
        assert_eq!(settle_word_for(63.9), Some("STL"));
        assert_eq!(settle_word_for(28.0), Some("STL"));
        assert_eq!(settle_word_for(27.9), Some("S"));
        assert_eq!(settle_word_for(12.0), Some("S"));
        assert_eq!(settle_word_for(11.9), None);
        assert_eq!(
            settle_band(50.0, 100.0, &[(50.0, 70.0, SETTLING)]),
            Some((50.0, 70.0))
        );
        assert_eq!(settle_band(0.0, 50.0, &[(50.0, 70.0, SETTLING)]), None);

        // The words drawn on a track 100 s across about 410 pt, a change at
        // 50 s settling for `settle` seconds, with `masked` over it: each
        // text and its place.
        let drawn = |settle: f64, masked: &[(f64, f64)]| {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let view = StripView {
                half_width_ms: 10.0,
                span_s: 100.0,
                horizontal: true,
                still: false,
            };
            let track = [
                (0.0, 50.0, "Dial Up (DU)", "DU"),
                (50.0, f64::INFINITY, "Dial Down (DD)", "DD"),
            ];
            let settles = [(50.0, 50.0 + settle, SETTLING)];
            let extras = Extras {
                track: &track,
                settles: &settles,
                masked,
                ..Default::default()
            };
            let mut x = None;
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 300.0))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let origin = ui.cursor().min;
                        draw_strip(
                            ui,
                            &[],
                            0.125,
                            None,
                            100.0,
                            &view,
                            &extras,
                            Vec2::new(460.0, 260.0),
                        );
                        let left = origin.x + 46.0;
                        let right = origin.x + 460.0 - 4.0;
                        x = Some(move |t: f64| {
                            right - ((100.0 - t) / 100.0) as f32 * (right - left)
                        });
                    });
                },
            );
            let x = x.unwrap();
            let texts: Vec<(String, Rect)> = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => {
                        Some((t.galley.text().to_string(), c.shape.visual_bounding_rect()))
                    }
                    _ => None,
                })
                .collect();
            (texts, x)
        };
        let find = |texts: &[(String, Rect)], want: &str| {
            texts.iter().find(|(t, _)| t == want).map(|(_, r)| *r)
        };
        // 20 s is about 82 pt: "Settling", inside the band, the name after it.
        let (texts, x) = drawn(20.0, &[]);
        let word = find(&texts, "Settling").expect("Settling");
        assert!(
            word.left() >= x(50.0) && word.right() <= x(70.0),
            "{word:?}"
        );
        let name = find(&texts, "Dial Down (DD)").expect("the name");
        assert!(name.left() >= x(70.0), "{name:?} starts after the band");
        // 10 s, about 41 pt: "STL". 4 s, about 16 pt: "S". 2 s: no word.
        assert!(find(&drawn(10.0, &[]).0, "STL").is_some());
        assert!(find(&drawn(4.0, &[]).0, "S").is_some());
        let none = drawn(2.0, &[]).0;
        assert!(["Settling", "STL", "S"]
            .iter()
            .all(|w| find(&none, w).is_none()));
        // Masked from 45 s to 60 s: the 10 s of band after it read "STL".
        let (texts, x) = drawn(20.0, &[(45.0, 60.0)]);
        assert!(find(&texts, "Settling").is_none());
        let stl = find(&texts, "STL").expect("STL after the mask");
        assert!(stl.left() >= x(60.0) && stl.right() <= x(70.0), "{stl:?}");
    }

    /// The track's strength: solid through the settling, then fading over
    /// 40 s to its floor.
    #[test]
    fn the_track_fades_after_the_settling() {
        for track in [crate::theme::DARK.track, crate::theme::LIGHT.track] {
            let [settling, fade_from, fade_to] = track;
            assert_eq!(track_density(5.0, 20.0, track), settling);
            assert_eq!(track_density(20.0, 20.0, track), fade_from);
            assert!((track_density(40.0, 20.0, track) - (fade_from + fade_to) / 2.0).abs() < 1e-6);
            assert_eq!(track_density(60.0, 20.0, track), fade_to);
            assert_eq!(track_density(600.0, 20.0, track), fade_to);
        }
        // Stronger in light mode, where teal on white is fainter.
        assert_eq!(crate::theme::DARK.track, [0.62, 0.48, 0.24]);
        assert_eq!(crate::theme::LIGHT.track, [0.80, 0.62, 0.36]);
        assert_eq!(settled_from(50.0, 100.0, &[(50.0, 70.0, SETTLING)]), 70.0);
        assert_eq!(settled_from(0.0, 50.0, &[(50.0, 70.0, SETTLING)]), 0.0);
    }

    /// The strip's input after a wheel turn with `modifiers` over it.
    fn wheel_frames(delta: Vec2, modifiers: egui::Modifiers) -> Vec<StripInput> {
        let ctx = egui::Context::default();
        let p = egui::Event::PointerMoved(Pos2::new(300.0, 200.0));
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            modifiers,
        };
        masking_frames_with(
            &ctx,
            modifiers,
            &[
                vec![p.clone()],
                vec![p.clone()],
                vec![p, wheel],
                vec![],
                vec![],
            ],
        )
    }

    /// The wheel, or a swipe either way, pans time and zooms nothing;
    /// with Shift too (Play Head rule 10).
    #[test]
    fn the_wheel_pans_time() {
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        for (delta, m) in [
            (Vec2::new(0.0, 50.0), Default::default()),
            (Vec2::new(50.0, 0.0), Default::default()),
            (Vec2::new(0.0, 50.0), shift),
        ] {
            let got = wheel_frames(delta, m);
            let back: f64 = got.iter().map(|i| i.time_shift_s).sum();
            assert!(back < 0.0, "{delta:?} {m:?}: {got:?}");
            assert!(
                got.iter()
                    .all(|i| i.span_factor == 1.0 && i.width_factor == 1.0),
                "{got:?}"
            );
        }
        // The other way, towards the newest beats.
        let got = wheel_frames(Vec2::new(0.0, -50.0), Default::default());
        assert!(
            got.iter().map(|i| i.time_shift_s).sum::<f64>() > 0.0,
            "{got:?}"
        );
    }

    /// Cmd (Mac) or Ctrl with the wheel zooms time about the time under
    /// the pointer, and pans nothing.
    #[test]
    fn the_command_wheel_zooms_time_about_the_pointer() {
        let command = egui::Modifiers {
            ctrl: !cfg!(target_os = "macos"),
            mac_cmd: cfg!(target_os = "macos"),
            command: true,
            ..Default::default()
        };
        let got = wheel_frames(Vec2::new(0.0, 50.0), command);
        let zoom = got.iter().find(|i| i.span_factor != 1.0).expect("zoomed");
        assert!(zoom.span_factor < 1.0, "{zoom:?}");
        let at = zoom.zoom_at.expect("about the pointer");
        let under = got.iter().find_map(|i| i.hover_t).expect("hovered");
        assert!((at - under).abs() < 1e-9 && at < 90.0, "{at} {under}");
        assert!(got.iter().all(|i| i.time_shift_s == 0.0), "{got:?}");
    }

    /// Alt and the wheel zoom the width, not the length.
    #[test]
    fn alt_and_the_wheel_zoom_the_width() {
        let alt = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        let got = wheel_frames(Vec2::new(0.0, 50.0), alt);
        let zoom = got
            .iter()
            .find(|i| i.width_factor != 1.0)
            .expect("the width zoomed");
        assert!(zoom.width_factor < 1.0, "{zoom:?}");
        assert!(
            got.iter()
                .all(|i| i.span_factor == 1.0 && i.time_shift_s == 0.0),
            "{got:?}"
        );
    }

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

    /// On a take whose sound card runs 100 ppm slow, the beats lie along
    /// the rate line drawn from the corrected rate, where on the card's
    /// seconds they tilted from it by 8.64 s/d, 0.0864 s/d per ppm (#643).
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a test's clock fixture, not a take being opened"
    )]
    fn on_a_corrected_take_the_beats_run_along_the_rate_line() {
        let p = 0.125;
        let rate = 5.0;
        // A card 100 ppm slow.
        let clock = ClockFit::fixed(-100.0, 120.0);
        let slow = 1.0 + 100.0 * 1e-6;
        // Beat k at its true second k periods in, gaining `rate`; the card
        // hears it at that over `slow`.
        let true_s = |k: i64| k as f64 * timegrapher_core::synth::period(p, rate);
        let beats: Vec<Beat> = (0..800).map(|k| beat(k, true_s(k) / slow)).collect();
        let line = RateLine {
            from_s: 0.0,
            to_s: beats[799].time,
            rate_s_per_day: rate,
        };
        let off_line = |clock: Option<&ClockFit>| -> f64 {
            let a = Anchor::centre_on_true(&beats[..8], p, 8, &[], clock).unwrap();
            let [(ta, la), (tb, lb)] = line.ends(&beats, a, p, &[], clock).unwrap();
            let at = |t: f64| la + (lb - la) / (tb - ta) * (t - ta);
            beats
                .iter()
                .map(|b| (a.lead_on_true(b, p, &[], clock) - at(b.time)).abs())
                .fold(0.0, f64::max)
        };
        // Within a hundredth of a millisecond: the strip's time axis stays
        // on the card's seconds, 100 ppm short of the true ones.
        let on_clock = off_line(Some(&clock));
        assert!(on_clock < 0.01, "{on_clock} ms off the line");
        // On the card's seconds the last beats stand ~5 ms off it.
        let on_card = off_line(None);
        assert!(on_card > 3.0, "{on_card} ms off the line");
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
                sweep: Sweep::Moving,
            };
            for t in [40.0, 55.5, 100.0] {
                assert!((g.time_at(g.pos(3.0, t)) - t).abs() < 1e-3);
            }
        }
    }

    /// A drag that begins within a few points of a masked stretch's edge
    /// takes that edge, the nearer one when two are close.
    #[test]
    fn an_edge_is_found_under_the_pointer() {
        for horizontal in [false, true] {
            let g = Geom {
                r: Rect::from_min_size(Pos2::new(50.0, 20.0), Vec2::new(400.0, 300.0)),
                half: 10.0,
                span: 60.0,
                end: 100.0,
                horizontal,
                sweep: Sweep::Moving,
            };
            let m = [(50.0, 60.0), (60.5, 70.0)];
            assert_eq!(
                edge_near(&m, g.pos(2.0, 50.0), &g),
                Some((50.0, 60.0, true))
            );
            assert_eq!(
                edge_near(&m, g.pos(-4.0, 70.0), &g),
                Some((60.5, 70.0, false))
            );
            assert_eq!(
                edge_near(&m, g.pos(0.0, 60.1), &g),
                Some((50.0, 60.0, false))
            );
            assert_eq!(edge_near(&m, g.pos(0.0, 55.0), &g), None);
        }
    }

    #[test]
    fn an_overlay_axis_ticks_at_round_steps_end_to_end() {
        let s = (-100.0, -20.0, 20.0);
        let t: Vec<f64> = ticks(s).collect();
        assert_eq!(t, vec![-100.0, -80.0, -60.0, -40.0, -20.0]);
        // The scale's ends at the strip's edges, as its line is drawn.
        assert_eq!(scale_ms(s, -100.0, 5.0), -5.0);
        assert_eq!(scale_ms(s, -20.0, 5.0), 5.0);
    }

    #[test]
    fn wrapping() {
        assert_eq!(wrap(0.0, 5.0), 0.0);
        assert!((wrap(6.0, 5.0) + 4.0).abs() < 1e-12);
        assert!((wrap(-6.0, 5.0) - 4.0).abs() < 1e-12);
        assert_eq!(round_step(3.0), 5.0);
        assert_eq!(round_step(0.12), 0.2);
        assert_eq!(length_text(3725.0), "1:02:05");
        assert_eq!(fmt_time_fine(83.456), "1:23.46");
        assert_eq!(fmt_time_fine(59.996), "1:00.00");
        assert_eq!(fmt_time_fine(3725.5), "1:02:05.50");
    }
}
