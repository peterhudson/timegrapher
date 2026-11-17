//! The strip's beats as dots: placed once, and kept while nothing they
//! rest on changes, so a frame costs no more at thirty hours on show than
//! at thirty seconds (#1857).

use super::{side_colors, Frame, MASKED_BEAT, MAX_BEATS_PER_PX};
use eframe::egui::{self, Color32, Pos2, Rect};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use timegrapher_core::beats::Beat;
use timegrapher_core::timing::Anchor;

/// One dot: where it lies, its colour, and whether its beat is masked.
#[derive(Clone, Copy, Debug)]
pub(super) struct Dot {
    pub at: Pos2,
    pub color: Color32,
    pub masked: bool,
}

/// The dots on show, the radius they are drawn at, and what they were
/// placed for.
#[derive(Clone)]
pub(super) struct Dots {
    key: u64,
    pub radius: f32,
    pub list: Arc<Vec<Dot>>,
}

/// Dots of one colour closer than this, points, are drawn once: at the
/// smallest radius they cover one another all but a hair's breadth.
const CELL: f32 = 0.5;

impl Frame<'_> {
    /// The dots for `beats`, placed again only when what they rest on
    /// changed since the last frame.
    pub(super) fn dots(&self, beats: &[Beat], period_s: f64, anchor: Anchor) -> Dots {
        let key = self.dots_key(beats, period_s, anchor);
        let ctx = self.painter.ctx();
        let id = egui::Id::new("strip dots").with(self.painter.layer_id());
        let kept = ctx.data(|d| d.get_temp::<Dots>(id));
        if let Some(dots) = kept.filter(|d| d.key == key) {
            return dots;
        }
        let dots = self.place_dots(beats, period_s, anchor, key);
        ctx.data_mut(|d| d.insert_temp(id, dots.clone()));
        dots
    }

    /// Everything the dots' places and colours rest on, hashed: the
    /// strip's shape and span, the beats on show (their count and a
    /// sample through them, which any reading again moves), the anchor,
    /// the clock and breaks, the masks and settles, and the theme.
    fn dots_key(&self, beats: &[Beat], period_s: f64, anchor: Anchor) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let shown = self.shown(beats);
        shown.len().hash(&mut h);
        let sample = shown.iter().step_by((shown.len() / 64).max(1));
        for b in sample.chain(shown.last()) {
            (b.index, b.time.to_bits(), b.quality.to_bits()).hash(&mut h);
        }
        format!("{:?}", (self.g, anchor)).hash(&mut h);
        let e = self.extras;
        let spans = e
            .masked
            .iter()
            .copied()
            .chain(e.settles.iter().map(|s| (s.0, s.1)));
        let gaps = e.breaks.iter().map(|b| (b.audio_s, b.gap_s));
        for (a, b) in spans.chain(gaps).chain([(period_s, 0.0)]) {
            (a.to_bits(), b.to_bits()).hash(&mut h);
        }
        (e.masked.len(), e.settles.len(), e.breaks.len()).hash(&mut h);
        e.clock.map(|c| c.fingerprint()).hash(&mut h);
        (self.vis.dark_mode, self.vis.extreme_bg_color).hash(&mut h);
        h.finish()
    }

    /// The good beats on show as dots, Tick and Tock in their colours.
    /// Past a few dozen beats to a point, the dots only lie on one another:
    /// every nth Tick and its Tock are placed, so a strip days long places
    /// no more than a short one crowded to that; and of those, dots of one
    /// colour within [`CELL`] of one drawn already are left out, so it
    /// draws no more than its dots can show.
    fn place_dots(&self, beats: &[Beat], period_s: f64, anchor: Anchor, key: u64) -> Dots {
        let (g, r, extras) = (&self.g, self.g.r, self.extras);
        let shown = self.shown(beats);
        // Dots shrink when the strip is crowded.
        let along = if g.horizontal { r.width() } else { r.height() };
        let per_px = shown.len() as f32 / along.max(1.0);
        let radius = (2.2 / per_px.max(1.0).sqrt()).clamp(0.8, 2.2);
        let every = (per_px / MAX_BEATS_PER_PX).ceil().max(1.0) as i64;
        let colors = side_colors(self.vis.dark_mode);
        let ground = self.vis.extreme_bg_color;
        let mut seen = HashSet::new();
        let mut list = Vec::new();
        let placed = shown
            .iter()
            .filter(|b| b.is_good() && b.index.div_euclid(2).rem_euclid(every) == 0);
        for b in placed {
            let at = self.beat_at(b, period_s, anchor);
            let masked = extras.masked.iter().any(|m| b.time >= m.0 && b.time <= m.1);
            let mut color = colors[b.index.rem_euclid(2) as usize];
            if masked {
                color = ground.lerp_to_gamma(color, MASKED_BEAT);
            } else if extras.settles.iter().any(|s| b.time > s.0 && b.time <= s.1) {
                // Faint while the watch settles after a move.
                color = color.gamma_multiply(0.3);
            }
            let cell = ((at.x / CELL).round() as i32, (at.y / CELL).round() as i32);
            if seen.insert((cell, color)) {
                list.push(Dot { at, color, masked });
            }
        }
        Dots {
            key,
            radius,
            list: Arc::new(list),
        }
    }

    /// How many of the dots drawn lie in `area`.
    pub(super) fn dots_in(&self, dots: &Dots, area: Rect) -> usize {
        dots.list.iter().filter(|d| area.contains(d.at)).count()
    }

    /// The beats on the strip as dots, faint while the watch settles.
    /// With `masked`, only the beats in a masked stretch, at 30% of their
    /// colour over the strip's ground and opaque, so crowded dots don't
    /// add up to full strength: a masked beat is still there to judge the
    /// mask by, but reads as unused (Hugh, #1068 item 2). Without, only
    /// the others.
    pub(super) fn paint_beats(&self, dots: &Dots, masked: bool) {
        let painter = self.painter.with_clip_rect(self.g.r.expand(3.0));
        for d in dots.list.iter().filter(|d| d.masked == masked) {
            painter.circle_filled(d.at, dots.radius, d.color);
        }
    }
}
