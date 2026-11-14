//! The labels on the strip's ms axis, across the time.

use super::fmt_ms;
use timegrapher_core::chart_scale::round_step;

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

/// A signed number of ms for an axis label on lines `step` ms apart:
/// zero in seconds, as its neighbours are, once every line is a whole
/// second or more from the next (Hugh: "0 s" between "−10 s" and "+10 s").
pub(super) fn axis_ms(ms: f64, step: f64) -> String {
    if ms.abs() < 1e-9 {
        if step >= 1000.0 { "0 s" } else { "0 ms" }.into()
    } else {
        let sign = if ms > 0.0 { "+" } else { "−" };
        if ms.abs() >= 1000.0 {
            format!("{sign}{} s", fmt_ms(ms.abs() / 1000.0))
        } else {
            format!("{sign}{}", fmt_ms(ms.abs()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_labels_turn_to_seconds() {
        assert_eq!(axis_ms(0.0, 50.0), "0 ms");
        assert_eq!(axis_ms(-250.0, 50.0), "−250");
        assert_eq!(axis_ms(2000.0, 500.0), "+2.0 s");
        assert_eq!(axis_ms(-20_000.0, 5000.0), "−20 s");
        // Zero takes its neighbours' unit: ms while any line is under a
        // second from the next, seconds once all are.
        assert_eq!(axis_ms(0.0, 500.0), "0 ms");
        assert_eq!(axis_ms(0.0, 1000.0), "0 s");
        assert_eq!(axis_ms(0.0, 10_000.0), "0 s");
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
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .filter(|t| t == "0 ms" || t == "0 s" || t.starts_with('+') || t.starts_with('−'))
            .collect()
    }

    /// A short strip at ±20 ms, its 5 ms lines too close for a label
    /// each, labels every second one: −10, 0 ms and +10, not ±5 or ±15.
    #[test]
    fn a_short_strip_labels_every_second_ms_line() {
        let labels = ms_labels(180.0);
        for shown in ["−10", "0 ms", "+10"] {
            assert!(labels.iter().any(|t| t == shown), "{shown}: {labels:?}");
        }
        for skipped in ["−15", "−5.0", "+5.0", "+15"] {
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
