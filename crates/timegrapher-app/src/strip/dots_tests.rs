//! The strip's dots: drawn once where they lie on one another, and placed
//! again when what they rest on changes (#1857).

use super::*;

/// A take of `n` beats on time, 8 a second.
fn on_time(n: i64) -> Vec<Beat> {
    (0..n)
        .map(|k| Beat {
            index: k,
            time: k as f64 * 0.125,
            quality: 1.0,
        })
        .collect()
}

/// The circles one frame of the strip draws over its last `end` seconds.
fn circles(ctx: &egui::Context, beats: &[Beat], end: f64, extras: &Extras) -> Vec<(Pos2, Color32)> {
    let view = StripView {
        half_width_ms: 10.0,
        span_s: end,
        horizontal: true,
        still: false,
    };
    let anchor = Anchor::centre_on(&beats[..8], 0.125, 8);
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 300.0))),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                draw_strip(
                    ui,
                    beats,
                    0.125,
                    anchor,
                    end,
                    &view,
                    extras,
                    Vec2::new(460.0, 260.0),
                );
            });
        },
    );
    out.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Circle(s) => Some((s.center, s.fill)),
            _ => None,
        })
        .collect()
}

/// Forty thousand beats on time lie on one line across the strip: it is
/// drawn end to end, with no more dots than half-points along it, where
/// every beat's own dot was once drawn.
#[test]
fn dots_on_one_another_are_drawn_once() {
    let beats = on_time(40_000);
    let dots = circles(
        &egui::Context::default(),
        &beats,
        5000.0,
        &Extras::default(),
    );
    let xs = dots.iter().map(|d| d.0.x);
    let (left, right) = xs.fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
    assert!(dots.len() > 200, "{} dots", dots.len());
    assert!(dots.len() < 2 * 2 * 460, "{} dots", dots.len());
    assert!(right - left > 400.0, "dots from {left} to {right}");
}

/// The dots kept from one frame serve the next only while nothing they
/// rest on changed: beats read again move them on the next frame, and
/// masking the whole take greys every dot.
#[test]
fn dots_are_placed_again_when_what_they_rest_on_changes() {
    let ctx = egui::Context::default();
    let sides = side_colors(ctx.style().visuals.dark_mode);
    let beats = on_time(4_000);
    let plain = Extras::default();
    let before = circles(&ctx, &beats, 500.0, &plain);
    let in_side_colour = |d: &[(Pos2, Color32)]| d.iter().filter(|c| sides.contains(&c.1)).count();
    assert!(in_side_colour(&before) > 200, "{}", in_side_colour(&before));
    assert_eq!(
        circles(&ctx, &beats, 500.0, &plain),
        before,
        "kept while unchanged"
    );
    // The beats read again, all but the first eight 2 ms later.
    let mut later = beats.clone();
    later.iter_mut().skip(8).for_each(|b| b.time += 0.002);
    assert_ne!(
        circles(&ctx, &later, 500.0, &plain),
        before,
        "read again, placed again"
    );
    let masked = [(-1.0, 1e6)];
    let all_masked = Extras {
        masked: &masked,
        ..Extras::default()
    };
    // Back to the beats as first read, then masked: only the key's two
    // dots keep the sides' colours.
    assert_eq!(circles(&ctx, &beats, 500.0, &plain), before);
    let greyed = circles(&ctx, &beats, 500.0, &all_masked);
    assert!(in_side_colour(&greyed) <= 2, "{}", in_side_colour(&greyed));
}
