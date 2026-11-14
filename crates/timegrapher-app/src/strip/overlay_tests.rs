//! A reading's line over the strip drawn whole when the strip is long:
//! readings half a second apart sit under a point apart there.

use super::*;

/// The colour the test's line is drawn in, used by nothing else on the strip.
const LINE: Color32 = Color32::from_rgb(201, 3, 199);

/// Where along the strip's time the line's ink lies, in points: each
/// triangle of the drawn line, from its least to its most along time.
/// Only the ink clipped to the strip itself, where the line is drawn:
/// the reading's scale beside the strip is drawn in its colour too.
fn inked_along(ctx: &egui::Context, out: egui::FullOutput, horizontal: bool) -> Vec<(f32, f32)> {
    let mut by_clip: Vec<(Rect, Vec<(f32, f32)>)> = Vec::new();
    for p in ctx.tessellate(out.shapes, out.pixels_per_point) {
        let egui::epaint::Primitive::Mesh(m) = p.primitive else {
            continue;
        };
        let at = |v: &egui::epaint::Vertex| if horizontal { v.pos.x } else { v.pos.y };
        let spans = m.indices.chunks(3).filter_map(|tri| {
            let v: Vec<_> = tri.iter().map(|&i| m.vertices[i as usize]).collect();
            v.iter().any(|v| v.color == LINE).then(|| {
                let lo = v.iter().map(at).fold(f32::INFINITY, f32::min);
                let hi = v.iter().map(at).fold(f32::NEG_INFINITY, f32::max);
                (lo, hi)
            })
        });
        match by_clip.iter_mut().find(|(r, _)| *r == p.clip_rect) {
            Some((_, spans_here)) => spans_here.extend(spans),
            None => by_clip.push((p.clip_rect, spans.collect())),
        }
    }
    let mut along = by_clip
        .into_iter()
        .map(|(_, s)| s)
        .max_by_key(Vec::len)
        .unwrap_or_default();
    along.sort_by(|a, b| a.0.total_cmp(&b.0));
    along
}

/// The widest stretch from `from` to `to` that no inked span covers.
fn widest_blank(inked: &[(f32, f32)], from: f32, to: f32) -> f32 {
    let mut reached = from;
    let mut widest = 0.0f32;
    for &(lo, hi) in inked {
        if lo > reached {
            widest = widest.max(lo.min(to) - reached);
        }
        reached = reached.max(hi);
    }
    widest.max(to - reached)
}

/// Amplitude is read again only every few seconds, so its readings half
/// a second apart run level for a while and then step. On a 20 min strip
/// they sit under a point apart, and each level piece drawn on its own
/// was rounded to the pixel at both ends into nothing: the line showed
/// dashes, the steps only. Zoomed in, the pieces were long enough to
/// keep. The line is drawn whole at any length, across or down.
#[test]
fn a_level_reading_close_spaced_is_drawn_without_gaps() {
    let points: Vec<[f64; 2]> = (0..=1200)
        .map(|i| [i as f64 * 0.5, 240.0 + (i / 40) as f64])
        .collect();
    for horizontal in [true, false] {
        let ctx = egui::Context::default();
        let view = StripView {
            half_width_ms: 2.5,
            span_s: 600.0,
            horizontal,
            still: false,
        };
        let overlays = [Overlay {
            name: "Amplitude",
            short: "Amp.",
            unit: "°",
            color: LINE,
            min_span: 10.0,
            points: points.clone(),
        }];
        let extras = Extras {
            overlays: &overlays,
            ..Default::default()
        };
        let mut drawn = Rect::NOTHING;
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 800.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    drawn = ui.available_rect_before_wrap();
                    draw_strip(
                        ui,
                        &[],
                        0.125,
                        Some(Anchor {
                            index: 0,
                            time: 0.0,
                        }),
                        view.span_s,
                        &view,
                        &extras,
                        Vec2::new(760.0, 760.0),
                    );
                });
            },
        );
        let inked = inked_along(&ctx, out, horizontal);
        assert!(!inked.is_empty(), "no line drawn (horizontal {horizontal})");
        // From the line's first ink to its last: the strip's own edges
        // and the scales beside it are not the line's to cover.
        let (from, to) = (inked[0].0, inked.iter().map(|s| s.1).fold(0.0, f32::max));
        let span = if horizontal {
            drawn.width()
        } else {
            drawn.height()
        };
        assert!(to - from > span / 2.0, "line runs {from}..{to} of {span}");
        let blank = widest_blank(&inked, from, to);
        assert!(
            blank < 0.5,
            "a gap of {blank} pt in the line (horizontal {horizontal})"
        );
    }
}
