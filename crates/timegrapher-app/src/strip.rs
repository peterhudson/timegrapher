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

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use timegrapher_core::beats::Beat;
use timegrapher_core::library::length_text;
pub use timegrapher_core::timing::Anchor;

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
}

impl StripView {
    pub const MIN_HALF_WIDTH_MS: f64 = 0.1;
    /// Wide enough for a long run-down take to drift by tens of seconds
    /// without its beats wrapping.
    pub const MAX_HALF_WIDTH_MS: f64 = 60_000.0;
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
    /// Neighbouring points further apart than this have a gap between
    /// them, and no line.
    pub gap_s: f64,
}

/// The pairs of neighbouring `points` joined by a line: those no further
/// apart than `gap_s`.
fn joined(points: &[&[f64; 2]], gap_s: f64) -> Vec<([f64; 2], [f64; 2])> {
    points
        .windows(2)
        .filter(|w| w[1][0] - w[0][0] <= gap_s)
        .map(|w| (*w[0], *w[1]))
        .collect()
}

impl Overlay {
    /// The scale's ends: the values in view with a little room either side.
    fn range(&self, from_s: f64, to_s: f64) -> Option<(f64, f64)> {
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
        Some((mid - half, mid + half))
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
        let mut cuts: Vec<(f64, f64)> = plain_parts
            .iter()
            .map(|p| (p.0, p.1))
            .chain(masked.iter().copied())
            .chain(covered)
            .chain(band)
            .filter(|p| p.1 > a && p.0 < b)
            .collect();
        cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut from = a;
        for &(p0, p1) in &cuts {
            if p0 > from {
                if let Some(seg) = along_rect(lane, from, p0, &at, horizontal) {
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
            from = from.max(p1);
        }
        if from < b {
            if let Some(seg) = along_rect(lane, from, b, &at, horizontal) {
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
        let mut hidden: Vec<(f64, f64)> = plain_parts
            .iter()
            .map(|p| (p.0, p.1))
            .chain(masked.iter().copied())
            .chain(covered)
            .filter(|p| p.1 > s0 && p.0 < s1)
            .collect();
        hidden.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut from = s0;
        let mut parts = Vec::new();
        for &(h0, h1) in &hidden {
            if h0 > from {
                parts.push((from, h0));
            }
            from = from.max(h1);
        }
        if from < s1 {
            parts.push((from, s1));
        }
        for (p0, p1) in parts {
            if let Some(seg) = along_rect(lane, p0, p1, &at, horizontal) {
                settle_word(&painter, seg, horizontal);
            }
        }
    }
    for &(a, b) in masked {
        if let Some(seg) = along_rect(lane, a, b, &at, horizontal) {
            painter.rect_filled(seg, 0.0, vis.extreme_bg_color);
            crate::theme::hatch(&painter, seg, pal.text_tertiary, 2.5, 6.0);
            track_label(
                &painter,
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

/// A signed number of ms for an axis label.
fn axis_ms(ms: f64) -> String {
    if ms.abs() < 1e-9 {
        "0 ms".into()
    } else {
        let sign = if ms > 0.0 { "+" } else { "−" };
        if ms.abs() >= 1000.0 {
            format!("{sign}{} s", fmt_ms(ms.abs() / 1000.0))
        } else {
            format!("{sign}{}", fmt_ms(ms.abs()))
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
    // down the left), and a scale for each overlay on top (or down the
    // right). The track of positions runs along the top when the strip
    // lies across, the key in the corners either side of it; else the
    // track is a column on the right, under the key.
    let lane_h = row_h + 4.0;
    let r = if view.horizontal {
        Rect::from_min_max(
            outer.min + Vec2::new(extras.gutter.max(46.0), 3.0 + lane_h),
            outer.max - Vec2::new(4.0 + 58.0 * overlays.len() as f32, row_h + 2.0),
        )
    } else {
        Rect::from_min_max(
            outer.min + Vec2::new(36.0, row_h * (1.0 + overlays.len() as f32) + 4.0),
            outer.max - Vec2::new(8.0 + lane_h, row_h + 2.0),
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
    let g = Geom {
        r,
        half,
        span: view.span_s,
        end: end_s,
        horizontal: view.horizontal,
    };
    let mut input = StripInput::default();
    let at = |p: Option<Pos2>| p.map(|p| g.time_at(p).clamp(oldest, end_s));
    // A change of position on the track within reach of `p`: its time.
    let changes: Vec<f64> = extras.track.iter().skip(1).map(|s| s.0).collect();
    let change_near = |p: Pos2| -> Option<f64> {
        if !lane.expand(2.0).contains(p) {
            return None;
        }
        let along = |q: Pos2| if view.horizontal { q.x } else { q.y };
        changes
            .iter()
            .map(|&t| (t, (along(g.pos(0.0, t)) - along(p)).abs()))
            .filter(|c| c.1 <= EDGE_REACH)
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .map(|c| c.0)
    };
    // A drag that begins on a change of position in the track moves it,
    // whatever tool is on, unless the Mask tool has a masked edge there.
    let change_id = resp.id.with("change-edge");
    if resp.drag_started_by(egui::PointerButton::Primary) {
        let origin = ui.input(|i| i.pointer.press_origin());
        if let Some(p) = origin.or(resp.interact_pointer_pos()) {
            let mask_edge = extras.masking && edge_near(extras.masked, p, &g).is_some();
            if let Some(t) = change_near(p).filter(|_| !mask_edge) {
                ui.data_mut(|d| d.insert_temp(change_id, t));
            }
        }
    }
    let moving_change: Option<f64> = ui.data(|d| d.get_temp(change_id));
    // A drag moves the strip; with the Mask tool or the Position Brush on,
    // or a change being moved, the left button does that and the right
    // one still moves it.
    let tool = extras.masking || extras.brush.is_some() || moving_change.is_some();
    let d = if tool && !resp.dragged_by(egui::PointerButton::Secondary) {
        Vec2::ZERO
    } else {
        resp.drag_delta()
    };
    if view.horizontal {
        input.time_shift_s = -(d.x / r.width().max(1.0)) as f64 * view.span_s;
        input.lead_shift_ms = -(d.y / (r.height() / 2.0).max(1.0)) as f64 * view.half_width_ms;
    } else {
        input.time_shift_s = (d.y / r.height().max(1.0)) as f64 * view.span_s;
        input.lead_shift_ms = (d.x / (r.width() / 2.0).max(1.0)) as f64 * view.half_width_ms;
    }
    if resp.hovered() {
        let (scroll, zoom, shift) =
            ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.shift));
        let wheel = scroll.x + scroll.y;
        if wheel != 0.0 && shift {
            // Shift and the wheel zoom the width, as a pinch does.
            input.width_factor = (-wheel as f64 * 0.003).exp();
        } else if wheel != 0.0 {
            input.span_factor = (-wheel as f64 * 0.003).exp();
            input.zoom_at = resp
                .hover_pos()
                .filter(|p| r.contains(*p))
                .map(|p| g.time_at(p));
        }
        if zoom != 1.0 {
            input.width_factor = 1.0 / zoom as f64;
        }
    }
    input.reset = resp.double_clicked();
    // Over a settle band on the track: what it is and how long, whatever
    // word fits it.
    let band_at = |p: Pos2| -> Option<f64> {
        if !lane.contains(p) {
            return None;
        }
        let t = g.time_at(p);
        extras.track.iter().find_map(|&(a, b, _, _)| {
            let (s0, s1) = settle_band(a, b, extras.settles)?;
            (t >= s0 && t < s1).then_some(s1 - s0)
        })
    };
    // Over a painted stretch on the track: the painting, and the way back.
    let painted_at = |p: Pos2| -> Option<f64> {
        let t = g.time_at(p);
        (lane.contains(p) && extras.painted.iter().any(|&(a, b)| t >= a && t < b)).then_some(t)
    };
    if let Some(len) = resp.hover_pos().and_then(band_at) {
        resp.clone()
            .on_hover_text_at_pointer(format!("Settling, {}", crate::fields::duration(len)));
    } else if resp.hover_pos().and_then(painted_at).is_some() {
        resp.clone().on_hover_text_at_pointer(
            "Painted with the Position Brush. Right-click to remove the painting and bring \
             back the positions recorded.",
        );
    }
    input.hover_t = resp
        .hover_pos()
        .filter(|p| r.contains(*p))
        .map(|p| g.time_at(p));
    // A right click on a painted stretch in the track's lane takes the
    // painting off; anywhere else, on a masked stretch, it brings that back,
    // Mask on or off.
    if resp.secondary_clicked() {
        match resp.interact_pointer_pos().and_then(painted_at) {
            Some(t) => input.unpaint_at = Some(t),
            None => input.unmask_at = at(resp.interact_pointer_pos()),
        }
    }
    // The Mask tool: where a drag began, kept between frames, and the
    // stretch it covers so far. A drag that begins on the edge of a masked
    // stretch moves that edge instead.
    let mut masking_now: Option<(f64, f64)> = None;
    let mut reshaping: Option<(f64, f64)> = None;
    let mut painting_now: Option<(f64, f64)> = None;
    let mut change_now: Option<f64> = None;
    if let Some(was) = moving_change {
        let now = at(resp.interact_pointer_pos());
        change_now = now;
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            input.move_change = now.map(|t| (was, t));
            ui.data_mut(|d| d.remove::<f64>(change_id));
        }
        ui.ctx().set_cursor_icon(if view.horizontal {
            egui::CursorIcon::ResizeHorizontal
        } else {
            egui::CursorIcon::ResizeVertical
        });
    } else if let (Some(_), false) = (extras.brush, extras.masking) {
        // The Position Brush: where a drag began, kept between frames.
        let id = resp.id.with("paint-from");
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin());
            if let Some(t) = at(origin.or(resp.interact_pointer_pos())) {
                ui.data_mut(|d| d.insert_temp(id, t));
            }
        }
        let from: Option<f64> = ui.data(|d| d.get_temp(id));
        if let (Some(a), Some(b)) = (from, at(resp.interact_pointer_pos())) {
            painting_now = Some((a.min(b), a.max(b)));
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            input.paint = painting_now;
            ui.data_mut(|d| d.remove::<f64>(id));
        }
        let on_change = resp.hover_pos().is_some_and(|p| change_near(p).is_some());
        if resp.dragged_by(egui::PointerButton::Secondary) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if resp.hovered() && on_change {
            ui.ctx().set_cursor_icon(if view.horizontal {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::ResizeVertical
            });
        } else if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
    } else if extras.masking {
        let id = resp.id.with("mask-from");
        let edge_id = resp.id.with("mask-edge");
        if resp.drag_started_by(egui::PointerButton::Primary) {
            // Where the button went down: a drag only starts once the
            // pointer has moved a few points on, past an edge's reach.
            let origin = ui.input(|i| i.pointer.press_origin());
            if let Some(p) = origin.or(resp.interact_pointer_pos()) {
                match edge_near(extras.masked, p, &g) {
                    Some(e) => ui.data_mut(|d| d.insert_temp(edge_id, e)),
                    None => {
                        if let Some(t) = at(Some(p)) {
                            ui.data_mut(|d| d.insert_temp(id, t));
                        }
                    }
                }
            }
        }
        let from: Option<f64> = ui.data(|d| d.get_temp(id));
        let edge: Option<(f64, f64, bool)> = ui.data(|d| d.get_temp(edge_id));
        let now = at(resp.interact_pointer_pos());
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
            input.unmask_at = at(resp.interact_pointer_pos());
        }
        let on_edge = resp
            .hover_pos()
            .is_some_and(|p| edge_near(extras.masked, p, &g).is_some() || change_near(p).is_some());
        if resp.dragged_by(egui::PointerButton::Secondary) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if resp.hovered() && (on_edge || edge.is_some()) {
            ui.ctx().set_cursor_icon(if view.horizontal {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::ResizeVertical
            });
        } else if resp.hovered() {
            // A cross for a new mask, unlike the edge's two-way arrow on
            // every system: on a Mac the column and two-way arrows look
            // the same.
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
    } else if resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if resp.hover_pos().is_some_and(|p| change_near(p).is_some()) {
        ui.ctx().set_cursor_icon(if view.horizontal {
            egui::CursorIcon::ResizeHorizontal
        } else {
            egui::CursorIcon::ResizeVertical
        });
    }

    let grid = Stroke::new(1.0_f32, vis.widgets.noninteractive.bg_stroke.color);
    let text = vis.weak_text_color();

    // Grid, every line alike: a nice step of ms along the strip, labelled
    // on the ms axis, and a nice step of seconds across it, labelled on the
    // time axis.
    let step = nice_step(half / 4.0);
    // On a strip too short for its labels to stand apart, only "Early"
    // and "Late" are named.
    let step_px = step / half
        * f64::from(if view.horizontal {
            r.height()
        } else {
            r.width()
        })
        / 2.0;
    let labels = step_px as f32
        > if view.horizontal {
            2.0 * font.size + 4.0
        } else {
            60.0
        };
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
        if labels && room > if view.horizontal { 14.0 } else { 34.0 } {
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
    // Too short a strip for both to stand apart names neither.
    let ends = if view.horizontal {
        r.height()
    } else {
        r.width()
    } > 2.0 * font.size + 4.0;
    if ends {
        painter.text(early_at, early_align, "Early", font.clone(), text);
        painter.text(late_at, late_align, "Late", font.clone(), text);
    }
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
        painter.text(at, align, length_text(t), font.clone(), text);
        t -= tstep;
    }
    let test = !extras.measured.is_empty();
    let place = |t: f64| g.pos(0.0, t);
    if test {
        paint_phases(
            &painter,
            r,
            extras.settles,
            extras.measured,
            place,
            view.horizontal,
            &vis,
        );
    } else {
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
                pal.positions,
                pal.track,
                crate::theme::TRACK_WASH,
                &place,
                view.horizontal,
            );
        }
    }
    // A line at each change of position, where the one being dragged
    // will be.
    let bands: Vec<(f64, f64, &str)> = extras
        .bands
        .iter()
        .map(|&(a, b, n)| match (moving_change, change_now) {
            (Some(was), Some(now)) if (a - was).abs() < 1e-6 => (now, b, n),
            _ => (a, b, n),
        })
        .collect();
    paint_bands(
        &painter,
        r,
        &bands,
        place,
        view.horizontal,
        false,
        false,
        &vis,
    );
    // The stretch whose edge is being dragged is drawn as it will be.
    let masked: Vec<(f64, f64)> = extras
        .masked
        .iter()
        .copied()
        .filter(|&m| Some(m) != reshaping)
        .collect();
    paint_masked(
        &painter,
        r,
        &masked,
        masking_now,
        oldest,
        end_s,
        place,
        view.horizontal,
        &vis,
    );
    // The track, with a change being dragged where it will be.
    let track: Vec<(f64, f64, &str, &str)> = match (moving_change, change_now) {
        (Some(was), Some(now)) => {
            let mut t: Vec<(f64, f64, &str, &str)> = extras.track.to_vec();
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
        _ => extras.track.to_vec(),
    };
    let mut track_masked = masked.clone();
    track_masked.extend(masking_now);
    paint_track(
        &painter,
        lane,
        &track,
        extras.settles,
        &track_masked,
        painting_now.filter(|_| extras.brush.is_some()),
        place,
        view.horizontal,
        &vis,
        pal,
    );
    // The stretch being painted, in teal over the track and the plot,
    // with the position and its times.
    if let (Some(code), Some((a, b))) = (extras.brush, painting_now) {
        if let Some(rect) = along_rect(r, a, b, &place, view.horizontal) {
            painter.rect_filled(rect, 0.0, pal.positions.gamma_multiply(0.18));
        }
        if let Some(seg) = along_rect(lane, a, b, &place, view.horizontal) {
            painter.rect_filled(seg, 0.0, pal.positions.gamma_multiply(0.85));
            painter.rect_stroke(
                seg,
                0.0,
                Stroke::new(1.0_f32, pal.positions),
                egui::StrokeKind::Inside,
            );
            let words = format!("{code} {} to {}", length_text(a), length_text(b));
            let galley = painter.layout_no_wrap(words, font.clone(), vis.strong_text_color());
            let size = galley.size();
            // Inside the plot, beside the stretch's newest end, on a backing.
            let newest = place(b);
            let pos = if view.horizontal {
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
    }

    // The key: Tick and Tock in their colours, in the corner left of the
    // track when the strip lies across, else along the top; each overlay
    // named in its colour above its scale.
    let colors = side_colors(dark);
    let mut at = if view.horizontal {
        Pos2::new(outer.left() + 6.0, lane.top() + 1.0)
    } else {
        Pos2::new(r.left(), outer.top() + 3.0)
    };
    for (label, c) in [("Tick", colors[0]), ("Tock", colors[1])] {
        let k = painter.text(at, Align2::LEFT_TOP, label, font.clone(), c);
        at.x = k.right() + 8.0;
    }
    if let Some(n) = extras.note {
        if view.horizontal {
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
    // Each overlay's scale: its ends in its colour, in a column of its own
    // right of the strip under its name, or a row of its own above it.
    for (i, (o, (lo, hi))) in overlays.iter().enumerate() {
        let f = |v: f64| format!("{v:.*}{}", o.decimals, o.unit);
        // Words in the text colour where the reading's own is too faint for
        // small text (beat error's amber in light mode).
        let ink = pal.ink(o.color);
        if view.horizontal {
            let x = r.right() + 6.0 + 58.0 * i as f32;
            painter.text(
                Pos2::new(x, lane.top() + 1.0),
                Align2::LEFT_TOP,
                o.name,
                font.clone(),
                ink,
            );
            painter.text(
                Pos2::new(x, r.top()),
                Align2::LEFT_TOP,
                f(*hi),
                font.clone(),
                ink,
            );
            painter.text(
                Pos2::new(x, r.bottom()),
                Align2::LEFT_BOTTOM,
                f(*lo),
                font.clone(),
                ink,
            );
        } else {
            let y = outer.top() + 3.0 + row_h * (1 + i) as f32;
            painter.text(
                Pos2::new(r.left(), y),
                Align2::LEFT_TOP,
                f(*lo),
                font.clone(),
                ink,
            );
            painter.text(
                Pos2::new(r.center().x, y),
                Align2::CENTER_TOP,
                format!("{} Scale", o.name),
                font.clone(),
                ink.gamma_multiply(0.8),
            );
            painter.text(
                Pos2::new(r.right(), y),
                Align2::RIGHT_TOP,
                f(*hi),
                font.clone(),
                ink,
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
        let mut c = colors[b.index.rem_euclid(2) as usize];
        // Faint while the watch settles after a move, or masked out.
        if extras.settles.iter().any(|s| b.time > s.0 && b.time <= s.1)
            || extras.masked.iter().any(|m| b.time >= m.0 && b.time <= m.1)
        {
            c = c.gamma_multiply(0.3);
        }
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
        for (a, b) in joined(&pts, o.gap_s) {
            painter.line_segment(
                [g.pos(across(a[1]), a[0]), g.pos(across(b[1]), b[0])],
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
            if extras.line {
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
    }
    if let Some(t) = extras.cursor_t.filter(|t| *t >= oldest && *t <= end_s) {
        let (a, b) = (g.pos(-half, t), g.pos(half, t));
        painter.line_segment([a, b], Stroke::new(1.0_f32, vis.weak_text_color()));
        // The line's time, to the hundredth, at the edge away from the
        // positions' names: along the bottom when time runs across, at the
        // right otherwise. Kept inside the plotting area.
        let galley = painter.layout_no_wrap(
            fmt_time_fine(t),
            FontId::proportional(11.0),
            vis.strong_text_color(),
        );
        let size = galley.size() + Vec2::splat(4.0);
        let rect = if view.horizontal {
            let x = (a.x - size.x / 2.0).clamp(r.left(), (r.right() - size.x).max(r.left()));
            Rect::from_min_size(Pos2::new(x, r.bottom() - size.y - 2.0), size)
        } else {
            let y = (a.y - size.y / 2.0).clamp(r.top(), (r.bottom() - size.y).max(r.top()));
            Rect::from_min_size(Pos2::new(r.right() - size.x - 2.0, y), size)
        };
        painter.rect_filled(rect, 2.0, vis.extreme_bg_color.gamma_multiply(0.9));
        painter.galley(rect.min + Vec2::splat(2.0), galley, vis.strong_text_color());
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
    fn readings_further_apart_than_the_gap_are_not_joined() {
        // A 7 h 28 min take opened whole keeps a reading every take/4000 s,
        // 6.7 s apart: joined, with the gap set from that spacing.
        let step = 26_880.0 / 4000.0;
        let pts: Vec<[f64; 2]> = (0..10).map(|i| [i as f64 * step, 10.0]).collect();
        let refs: Vec<&[f64; 2]> = pts.iter().collect();
        assert_eq!(joined(&refs, (2.5 * step).max(5.0)).len(), 9);
        // The old fixed 5 s gap joined none of them.
        assert!(joined(&refs, 5.0).is_empty());
        // A real hole in the readings stays a hole.
        let holed: Vec<&[f64; 2]> = pts.iter().filter(|p| p[0] < 20.0 || p[0] > 50.0).collect();
        assert_eq!(joined(&holed, (2.5 * step).max(5.0)).len(), holed.len() - 2);
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

    /// The wheel zooms about the time under the pointer.
    #[test]
    fn the_wheel_zooms_about_the_pointer() {
        let ctx = egui::Context::default();
        let p = egui::Event::PointerMoved(Pos2::new(300.0, 200.0));
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -50.0),
            modifiers: Default::default(),
        };
        let got = masking_frames(
            &ctx,
            &[
                vec![p.clone()],
                vec![p.clone()],
                vec![p, wheel],
                vec![],
                vec![],
            ],
        );
        let zoom = got.iter().find(|i| i.span_factor != 1.0).expect("zoomed");
        let at = zoom.zoom_at.expect("about the pointer");
        let under = got.iter().find_map(|i| i.hover_t).expect("hovered");
        assert!((at - under).abs() < 1e-9 && at < 90.0, "{at} {under}");
    }

    /// Shift and the wheel zoom the width, not the length.
    #[test]
    fn shift_and_the_wheel_zoom_the_width() {
        let ctx = egui::Context::default();
        let p = egui::Event::PointerMoved(Pos2::new(300.0, 200.0));
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, 50.0),
            modifiers: shift,
        };
        let got = masking_frames_with(
            &ctx,
            shift,
            &[
                vec![p.clone()],
                vec![p.clone()],
                vec![p, wheel],
                vec![],
                vec![],
            ],
        );
        let zoom = got
            .iter()
            .find(|i| i.width_factor != 1.0)
            .expect("the width zoomed");
        assert!(zoom.width_factor < 1.0, "{zoom:?}");
        assert!(got.iter().all(|i| i.span_factor == 1.0), "{got:?}");
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
    fn axis_labels_turn_to_seconds() {
        assert_eq!(axis_ms(0.0), "0 ms");
        assert_eq!(axis_ms(-250.0), "−250");
        assert_eq!(axis_ms(2000.0), "+2.0 s");
        assert_eq!(axis_ms(-20_000.0), "−20 s");
    }

    #[test]
    fn wrapping() {
        assert_eq!(wrap(0.0, 5.0), 0.0);
        assert!((wrap(6.0, 5.0) + 4.0).abs() < 1e-12);
        assert!((wrap(-6.0, 5.0) - 4.0).abs() < 1e-12);
        assert_eq!(nice_step(3.0), 5.0);
        assert_eq!(nice_step(0.12), 0.2);
        assert_eq!(length_text(3725.0), "1:02:05");
        assert_eq!(fmt_time_fine(83.456), "1:23.46");
        assert_eq!(fmt_time_fine(59.996), "1:00.00");
        assert_eq!(fmt_time_fine(3725.5), "1:02:05.50");
    }
}
