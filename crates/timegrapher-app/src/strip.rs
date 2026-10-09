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
    if dark {
        [
            Color32::from_rgb(0x5c, 0xc8, 0xff),
            Color32::from_rgb(0xff, 0xa8, 0x4a),
        ]
    } else {
        [
            Color32::from_rgb(0x00, 0x6e, 0xc4),
            Color32::from_rgb(0xc8, 0x5a, 0x00),
        ]
    }
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

#[derive(Debug, Clone, Copy, PartialEq)]
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
}

impl Default for StripInput {
    fn default() -> Self {
        StripInput {
            time_shift_s: 0.0,
            lead_shift_ms: 0.0,
            span_factor: 1.0,
            width_factor: 1.0,
            reset: false,
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
    note: Option<&str>,
    size: Vec2,
) -> StripInput {
    let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let r = resp.rect;
    let g = Geom {
        r,
        half: view.half_width_ms,
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
    if resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    let vis = ui.visuals();
    let dark = vis.dark_mode;
    painter.rect_filled(r, 2.0, vis.extreme_bg_color);
    let grid = vis.widgets.noninteractive.bg_stroke.color;
    let text = vis.weak_text_color();
    let font = FontId::proportional(11.0);
    let half = view.half_width_ms;
    let oldest = end_s - view.span_s;

    // Grid: lines every nice step of ms along the strip, every nice step of
    // seconds across it.
    let step = nice_step(half / 4.0);
    let mut k = (-half / step).ceil() as i64;
    while (k as f64) * step < half {
        let ms = k as f64 * step;
        let s = if k == 0 {
            Stroke::new(1.5_f32, grid.gamma_multiply(2.0))
        } else {
            Stroke::new(1.0_f32, grid)
        };
        painter.line_segment([g.pos(ms, end_s), g.pos(ms, oldest)], s);
        k += 1;
    }
    let tstep = nice_step(view.span_s / 6.0);
    let mut t = (end_s / tstep).floor() * tstep;
    while t > oldest {
        let (a, b) = (g.pos(-half, t), g.pos(half, t));
        painter.line_segment([a, b], Stroke::new(1.0_f32, grid));
        // Vertical: label at the left edge above the line; horizontal: at
        // the bottom edge right of the line.
        let at = if view.horizontal {
            a + Vec2::new(3.0, -16.0)
        } else {
            a + Vec2::new(4.0, -2.0)
        };
        // Leave room for the "late" label in the corner.
        let crowded = if view.horizontal {
            at.x < r.left() + 32.0
        } else {
            at.y > r.bottom() - 16.0
        };
        if !crowded {
            painter.text(at, Align2::LEFT_BOTTOM, fmt_time(t), font.clone(), text);
        }
        t -= tstep;
    }
    let caption = format!(
        "{} ms per line, {} ms across",
        fmt_ms(step),
        fmt_ms(2.0 * half)
    );
    if view.horizontal {
        let pad = Vec2::new(4.0, 2.0);
        painter.text(
            r.left_top() + pad,
            Align2::LEFT_TOP,
            "early",
            font.clone(),
            text,
        );
        painter.text(
            r.left_bottom() + Vec2::new(4.0, -2.0),
            Align2::LEFT_BOTTOM,
            "late",
            font.clone(),
            text,
        );
        painter.text(
            r.center_top() + Vec2::new(0.0, 2.0),
            Align2::CENTER_TOP,
            caption,
            font.clone(),
            text,
        );
    } else {
        painter.text(
            r.left_bottom() + Vec2::new(4.0, -4.0),
            Align2::LEFT_BOTTOM,
            "late",
            font.clone(),
            text,
        );
        painter.text(
            r.center_bottom() + Vec2::new(0.0, -4.0),
            Align2::CENTER_BOTTOM,
            caption,
            font.clone(),
            text,
        );
        painter.text(
            r.right_bottom() + Vec2::new(-4.0, -4.0),
            Align2::RIGHT_BOTTOM,
            "early",
            font.clone(),
            text,
        );
    }
    // Which colour is which.
    let colors = side_colors(dark);
    let mut at = if view.horizontal {
        r.left_top() + Vec2::new(48.0, 2.0)
    } else {
        r.left_top() + Vec2::new(4.0, 2.0)
    };
    for (label, c) in [("Tick", colors[0]), ("Tock", colors[1])] {
        let g = painter.text(at, Align2::LEFT_TOP, label, font.clone(), c);
        at.x = g.right() + 10.0;
    }
    if let Some(n) = note {
        let at = if view.horizontal {
            r.right_top() + Vec2::new(-4.0, 2.0)
        } else {
            r.center_top() + Vec2::new(0.0, 4.0)
        };
        let align = if view.horizontal {
            Align2::RIGHT_TOP
        } else {
            Align2::CENTER_TOP
        };
        painter.text(at, align, n, font, vis.text_color());
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
    let painter = painter.with_clip_rect(r);
    for b in shown {
        if b.quality < 0.4 {
            continue;
        }
        let p = g.pos(wrap(anchor.lead_ms(b, period_s), half), b.time);
        let c = colors[b.index.rem_euclid(2) as usize];
        painter.circle_filled(p, radius, c);
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
    fn wrapping() {
        assert_eq!(wrap(0.0, 5.0), 0.0);
        assert!((wrap(6.0, 5.0) + 4.0).abs() < 1e-12);
        assert!((wrap(-6.0, 5.0) - 4.0).abs() < 1e-12);
        assert_eq!(nice_step(3.0), 5.0);
        assert_eq!(nice_step(0.12), 0.2);
        assert_eq!(fmt_time(3725.0), "1:02:05");
    }
}
