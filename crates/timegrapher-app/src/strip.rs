//! The paper strip and the trend charts.
//!
//! The strip is drawn the way tg draws its paperstrip: one dot per beat,
//! placed across the strip by how early or late the beat came against a
//! clock running at the nominal beat rate, and down the strip by time. The
//! newest beats are at the top. A watch on rate draws a vertical line; one
//! that gains leans right as it rises (/), one that loses leans left (\).
//! Tick and toc (beats A and B) draw two lines whose gap is the beat error.
//! A line that runs off one side comes back on the other, as on a
//! Witschi diagram, so the strip's width sets the zoom.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use timegrapher_core::beats::Beat;

/// Colours of beats A (even) and B (odd).
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
    /// Centre the strip on the last `n` beats (tick and toc together).
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

pub struct StripView {
    /// Half the strip's width, ms.
    pub half_width_ms: f64,
    /// Time shown top to bottom, seconds.
    pub span_s: f64,
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

/// Draw the strip for the beats up to `end_s`.
pub fn draw_strip(
    ui: &mut egui::Ui,
    beats: &[Beat],
    period_s: f64,
    anchor: Option<Anchor>,
    end_s: f64,
    view: &StripView,
    size: Vec2,
) {
    let (resp, painter) = ui.allocate_painter(size, Sense::hover());
    let r = resp.rect;
    let vis = ui.visuals();
    let dark = vis.dark_mode;
    painter.rect_filled(r, 2.0, vis.extreme_bg_color);
    let grid = vis.widgets.noninteractive.bg_stroke.color;
    let text = vis.weak_text_color();
    let font = FontId::proportional(11.0);
    let half = view.half_width_ms;
    let x_of = |ms: f64| r.center().x + (ms / half) as f32 * r.width() / 2.0;
    let y_of = |t: f64| r.top() + ((end_s - t) / view.span_s) as f32 * r.height();

    // Grid: lines every nice step of ms across, every nice step of seconds down.
    let step = nice_step(half / 4.0);
    let mut k = (-half / step).ceil() as i64;
    while (k as f64) * step < half {
        let x = x_of(k as f64 * step);
        let s = if k == 0 {
            Stroke::new(1.5_f32, grid.gamma_multiply(2.0))
        } else {
            Stroke::new(1.0_f32, grid)
        };
        painter.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], s);
        k += 1;
    }
    let tstep = nice_step(view.span_s / 6.0);
    let mut t = (end_s / tstep).floor() * tstep;
    while t > end_s - view.span_s {
        let y = y_of(t);
        painter.line_segment(
            [Pos2::new(r.left(), y), Pos2::new(r.right(), y)],
            Stroke::new(1.0_f32, grid),
        );
        painter.text(
            Pos2::new(r.left() + 4.0, y - 2.0),
            Align2::LEFT_BOTTOM,
            fmt_time(t),
            font.clone(),
            text,
        );
        t -= tstep;
    }
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
        format!(
            "{} ms per line, {} ms across",
            fmt_ms(step),
            fmt_ms(2.0 * half)
        ),
        font.clone(),
        text,
    );
    painter.text(
        r.right_bottom() + Vec2::new(-4.0, -4.0),
        Align2::RIGHT_BOTTOM,
        "early",
        font,
        text,
    );

    let Some(anchor) = anchor else { return };
    let colors = side_colors(dark);
    let lo = beats.partition_point(|b| b.time < end_s - view.span_s);
    let hi = beats.partition_point(|b| b.time <= end_s);
    let shown = &beats[lo..hi];
    // Dots shrink when the strip is crowded.
    let per_px = shown.len() as f32 / r.height().max(1.0);
    let radius = (2.2 / per_px.max(1.0).sqrt()).clamp(0.8, 2.2);
    for b in shown {
        if b.quality < 0.4 {
            continue;
        }
        let x = x_of(wrap(anchor.lead_ms(b, period_s), half));
        let c = colors[b.index.rem_euclid(2) as usize];
        painter.circle_filled(Pos2::new(x, y_of(b.time)), radius, c);
    }
}

fn fmt_ms(ms: f64) -> String {
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

/// A small chart of one quantity over the session.
pub fn draw_series(
    ui: &mut egui::Ui,
    title: &str,
    unit: &str,
    points: &[(f64, f64)],
    t_range: (f64, f64),
    color: Color32,
    size: Vec2,
) {
    let (resp, painter) = ui.allocate_painter(size, Sense::hover());
    let r = resp.rect;
    let vis = ui.visuals();
    painter.rect_filled(r, 2.0, vis.extreme_bg_color);
    let text = vis.weak_text_color();
    let grid = vis.widgets.noninteractive.bg_stroke.color;
    let font = FontId::proportional(11.0);
    painter.text(
        r.left_top() + Vec2::new(4.0, 2.0),
        Align2::LEFT_TOP,
        title,
        font.clone(),
        vis.text_color(),
    );
    if points.len() < 2 {
        return;
    }
    // Scale to the 2nd to 98th percentile so a glitch doesn't flatten the line.
    let mut ys: Vec<f64> = points.iter().map(|p| p.1).collect();
    ys.sort_by(f64::total_cmp);
    let q = |f: f64| ys[((ys.len() - 1) as f64 * f).round() as usize];
    let (mut y0, mut y1) = (q(0.02), q(0.98));
    let pad = ((y1 - y0) * 0.15).max(1.0);
    y0 -= pad;
    y1 += pad;
    let (t0, t1) = (t_range.0, t_range.1.max(t_range.0 + 1.0));
    let inner = r.shrink2(Vec2::new(40.0, 16.0));
    let px = |t: f64, y: f64| {
        Pos2::new(
            inner.left() + ((t - t0) / (t1 - t0)) as f32 * inner.width(),
            inner.bottom() - ((y - y0) / (y1 - y0)) as f32 * inner.height(),
        )
    };
    let ystep = nice_step((y1 - y0) / 4.0);
    let mut y = (y0 / ystep).ceil() * ystep;
    while y <= y1 {
        let p = px(t0, y);
        painter.line_segment(
            [Pos2::new(inner.left(), p.y), Pos2::new(inner.right(), p.y)],
            Stroke::new(1.0_f32, grid),
        );
        painter.text(
            Pos2::new(inner.left() - 4.0, p.y),
            Align2::RIGHT_CENTER,
            format!("{y:.0}{unit}"),
            font.clone(),
            text,
        );
        y += ystep;
    }
    painter.text(
        Pos2::new(inner.left(), r.bottom() - 2.0),
        Align2::LEFT_BOTTOM,
        fmt_time(t0),
        font.clone(),
        text,
    );
    painter.text(
        Pos2::new(inner.right(), r.bottom() - 2.0),
        Align2::RIGHT_BOTTOM,
        fmt_time(t1),
        font,
        text,
    );
    let clip = Rect::from_min_max(inner.min, inner.max);
    let painter = painter.with_clip_rect(clip);
    // One point per pixel column at most.
    let step = (points.len() as f32 / inner.width().max(1.0))
        .ceil()
        .max(1.0) as usize;
    let line: Vec<Pos2> = points
        .iter()
        .step_by(step)
        .map(|&(t, y)| px(t, y))
        .collect();
    painter.add(egui::Shape::line(line, Stroke::new(1.5_f32, color)));
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
