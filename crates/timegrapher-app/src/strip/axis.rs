//! The labels on the strip's ms axis, across the time.

use timegrapher_core::chart_scale::{axis_tick, round_step};

/// Which grid lines across the strip, `step` ms and `step_px` points
/// apart, carry a label: every `n`th, the fewest skipped that leaves
/// `need` points between labels and keeps them on the 1-2-5 series
/// (Hugh, #1512): 5 ms lines labelled every 10, else 20, else 50 ms,
/// never 25. `None` when none does.
pub(super) fn label_every(step: f64, step_px: f32, need: f32) -> Option<i64> {
    // Every series step is one of these many lines, whatever the step;
    // the rest (4 of 1 ms, 5 of 5 ms) are filtered out as off it.
    [1, 2, 4, 5, 10].into_iter().find(|&n| {
        let at = step * n as f64;
        step_px * n as f32 > need && (round_step(at) - at).abs() < at * 1e-9
    })
}

/// A signed number of ms for an axis label on lines `step` ms apart, on
/// an axis reaching `half` ms either side of 0: every label in seconds,
/// zero too, once the axis reaches a second, so one axis never mixes
/// "+500" with "+1.0 s" (Walter on #1841; Hugh: "0 s" between "−10 s"
/// and "+10 s"). The rest as every value axis writes its ticks (core
/// `axis_tick`), so all on one axis share the step's decimals: "+5"
/// beside "+10", never "+5.0" (#1541).
pub(super) fn axis_ms(ms: f64, step: f64, half: f64) -> String {
    let seconds = half >= 1000.0;
    if ms.abs() < 1e-9 {
        if seconds { "0 s" } else { "0 ms" }.into()
    } else if seconds {
        format!("{} s", axis_tick(ms / 1000.0, step / 1000.0, true))
    } else {
        axis_tick(ms, step, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_labels_turn_to_seconds() {
        assert_eq!(axis_ms(0.0, 50.0, 200.0), "0 ms");
        assert_eq!(axis_ms(-250.0, 50.0, 500.0), "−250");
        assert_eq!(axis_ms(2000.0, 500.0, 2000.0), "+2.0 s");
        assert_eq!(axis_ms(-20_000.0, 5000.0, 30_000.0), "−20 s");
        // Once the axis reaches a second, every label is in seconds, zero
        // and those under a second included: never "+500" beside "+1.0 s".
        assert_eq!(axis_ms(500.0, 500.0, 1000.0), "+0.5 s");
        assert_eq!(axis_ms(1000.0, 500.0, 1000.0), "+1.0 s");
        assert_eq!(axis_ms(0.0, 500.0, 1000.0), "0 s");
        assert_eq!(axis_ms(0.0, 500.0, 500.0), "0 ms");
        assert_eq!(axis_ms(0.0, 1000.0, 5000.0), "0 s");
        // One axis, one number of decimals: the step's (#1541).
        assert_eq!(axis_ms(5.0, 5.0, 10.0), "+5");
        assert_eq!(axis_ms(10.0, 5.0, 10.0), "+10");
        assert_eq!(axis_ms(-1.0, 1.0, 5.0), "−1");
        assert_eq!(axis_ms(0.5, 0.5, 2.0), "+0.5");
        assert_eq!(axis_ms(1.0, 0.5, 2.0), "+1.0");
    }

    /// Lines far enough apart are all labelled; closer, the labels step
    /// up the 1-2-5 series and never off it, so never 4 ms apart on 2 ms
    /// lines or 25 ms apart on 5 ms lines; none when nothing on it leaves
    /// room (#1512: ±20 ms on a short strip, lines 5 ms and 21 pt apart,
    /// labelled −10, 0, +10).
    #[test]
    fn the_ms_axis_labels_step_up_the_one_two_five_series() {
        assert_eq!(label_every(5.0, 30.0, 26.0), Some(1));
        assert_eq!(label_every(5.0, 21.0, 26.0), Some(2));
        // 5 ms lines: 20 ms (every 4th) before 50, never 25 (every 5th).
        assert_eq!(label_every(5.0, 8.0, 26.0), Some(4));
        assert_eq!(label_every(5.0, 6.0, 26.0), Some(10));
        assert_eq!(label_every(5.0, 2.0, 26.0), None);
        assert_eq!(label_every(1.0, 8.0, 26.0), Some(5));
        assert_eq!(label_every(1.0, 2.0, 26.0), None);
        // 2 ms lines: 10 ms, never 4.
        assert_eq!(label_every(2.0, 21.0, 26.0), Some(5));
        assert_eq!(label_every(1.0, 21.0, 26.0), Some(2));
        assert_eq!(label_every(0.5, 21.0, 26.0), Some(2));
    }
}

/// The labels the strip itself draws on its ms axis (#1512).
#[cfg(test)]
mod drawn {
    use super::super::*;

    /// The ms labels drawn on a strip lying across, `height` points tall
    /// overall, at Width ±20 ms.
    fn ms_labels(height: f32) -> Vec<String> {
        labels_at(height, 20.0)
    }

    /// The axis labels drawn on a strip lying across, `height` points
    /// tall overall, at Width ±`half_width_ms`.
    fn labels_at(height: f32, half_width_ms: f64) -> Vec<String> {
        texts_at(height, half_width_ms)
            .into_iter()
            .map(|t| t.0)
            .filter(|t| t == "0 ms" || t == "0 s" || t.starts_with('+') || t.starts_with('−'))
            .collect()
    }

    /// Every text drawn on a strip lying across, `height` points tall
    /// overall, at Width ±`half_width_ms`, with where it is drawn.
    fn texts_at(height: f32, half_width_ms: f64) -> Vec<(String, Rect)> {
        let ctx = egui::Context::default();
        let view = StripView {
            half_width_ms,
            span_s: 100.0,
            horizontal: true,
            still: false,
        };
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 400.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw_strip(
                        ui,
                        &[],
                        0.125,
                        None,
                        100.0,
                        &view,
                        &Extras::default(),
                        Vec2::new(460.0, height),
                    );
                });
            },
        );
        out.shapes
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

    /// No ms label touches "Early" or "Late" at any Width or height, the
    /// end ones at ±5, ±50 and ±500 ms included (Walter's #1540 walk,
    /// #1541).
    #[test]
    fn no_ms_label_touches_early_or_late() {
        for half in [2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0] {
            for height in [140.0, 180.0, 260.0, 400.0] {
                let texts = texts_at(height, half);
                let ends: Vec<&Rect> = texts
                    .iter()
                    .filter(|t| t.0 == "Early" || t.0 == "Late")
                    .map(|t| &t.1)
                    .collect();
                assert_eq!(ends.len(), 2, "±{half} at {height}: {texts:?}");
                for (words, at) in texts
                    .iter()
                    .filter(|t| t.0 == "0 ms" || t.0.starts_with('+') || t.0.starts_with('−'))
                {
                    for end in &ends {
                        assert!(
                            !at.intersects(**end),
                            "±{half} at {height}: {words} {at:?} touches {end:?}"
                        );
                    }
                }
            }
        }
    }

    /// At ±1 s and ±2 s, lines half a second apart, every label is in
    /// seconds, "+0.5 s" beside "+1.0 s", never "+500" (Walter on #1841);
    /// under a second every label is in ms.
    #[test]
    fn one_axis_writes_one_unit() {
        for half in [1000.0, 2000.0] {
            let labels = labels_at(400.0, half);
            assert!(labels.len() > 2, "±{half}: {labels:?}");
            assert!(
                labels.iter().all(|t| t.ends_with(" s")),
                "±{half}: {labels:?}"
            );
        }
        let labels = labels_at(400.0, 500.0);
        assert!(labels.iter().all(|t| !t.ends_with(" s")), "{labels:?}");
    }

    /// A short strip at ±20 ms, its 5 ms lines too close for a label
    /// each, labels every second one: −10, 0 ms and +10, not ±5 or ±15.
    #[test]
    fn a_short_strip_labels_every_second_ms_line() {
        let labels = ms_labels(180.0);
        for shown in ["−10", "0 ms", "+10"] {
            assert!(labels.iter().any(|t| t == shown), "{shown}: {labels:?}");
        }
        for skipped in ["−15", "−5", "+5", "+15"] {
            assert!(
                !labels.iter().any(|t| t == skipped),
                "{skipped}: {labels:?}"
            );
        }
    }

    /// A strip too short for labels 10 or 20 ms apart labels only 0 ms:
    /// the next step, 50 ms, is past both ends. (Shorter still, with no
    /// step at all, there is no room for even 0 ms by the ends, so what
    /// `label_every` gives as `None` draws the same.)
    #[test]
    fn a_strip_too_short_for_any_step_labels_only_zero() {
        assert_eq!(ms_labels(90.0), vec!["0 ms".to_string()]);
    }

    /// At Width ±30 s every line is seconds apart, so zero reads "0 s"
    /// like the "+10 s" and "−10 s" beside it, not "0 ms" (Hugh, on
    /// Walter's #1603 shots).
    #[test]
    fn zero_takes_the_seconds_of_its_neighbours() {
        let labels = labels_at(400.0, 30_000.0);
        assert!(labels.iter().any(|t| t == "0 s"), "{labels:?}");
        assert!(
            labels.iter().any(|t| t.ends_with(" s") && t != "0 s"),
            "{labels:?}"
        );
        assert!(!labels.iter().any(|t| t == "0 ms"), "{labels:?}");
    }
}
