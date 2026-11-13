//! Where the Tick / Tock key sits on the strip lying across.

use super::*;

/// The key sits in the plot's left corner, top or bottom, whichever has
/// fewer dots under it (Hugh, #1536): with the beats early, near the top,
/// it goes to the bottom; with them late, near the bottom, it stays at the
/// top.
#[test]
fn the_key_moves_to_the_left_corner_with_fewer_dots() {
    for (shift_ms, at_top) in [(8.0, false), (-8.0, true)] {
        let p = 0.125;
        // The first beats on time to anchor on, the rest `shift_ms` early.
        let beats: Vec<Beat> = (0..800_i64)
            .map(|k| Beat {
                index: k,
                time: k as f64 * p - if k < 8 { 0.0 } else { shift_ms / 1000.0 },
                quality: 1.0,
            })
            .collect();
        let anchor = Anchor::centre_on(&beats[..8], p, 8);
        let ctx = egui::Context::default();
        let view = StripView {
            half_width_ms: 10.0,
            span_s: 100.0,
            horizontal: true,
            still: false,
        };
        let extras = Extras::default();
        let mut plot_mid = 0.0;
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 300.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    plot_mid = ui.cursor().min.y + 130.0;
                    draw_strip(
                        ui,
                        &beats,
                        p,
                        anchor,
                        100.0,
                        &view,
                        &extras,
                        Vec2::new(460.0, 260.0),
                    );
                });
            },
        );
        let tick = out
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == "Tick" => {
                    Some(c.shape.visual_bounding_rect())
                }
                _ => None,
            })
            .expect("Tick drawn");
        assert_eq!(
            tick.center().y < plot_mid,
            at_top,
            "beats {shift_ms} ms early: key at {tick:?}"
        );
    }
}
