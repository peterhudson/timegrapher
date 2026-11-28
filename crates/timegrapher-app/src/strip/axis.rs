//! The labels on the strip's ms axis, across the time.

use eframe::egui::{self, Color32, FontId};
use timegrapher_core::chart_scale::{axis_tick, round_step};
use timegrapher_core::time_of_day::{time_lines, WallClock};

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

/// The step between the strip's labelled time lines, seconds: the first
/// of the time axis's steps that leaves `gap` points between lines over
/// `along_px` points showing `span` seconds, and never under 1 s, as its
/// m:ss labels name whole seconds, so no two lines share a label (#2068,
/// Hugh).
pub(super) fn time_step(span: f64, gap: f64, along_px: f64) -> f64 {
    let want = span * gap / along_px.max(1.0);
    crate::theme::TIME_STEPS
        .iter()
        .copied()
        .filter(|&s| s >= 1.0)
        .find(|&s| s >= want)
        .unwrap_or(172800.0)
}

/// Down, with Time of Day, the label column's least width, points: room
/// for "14:35:20".
pub(super) const CLOCK_COLUMN: f32 = 52.0;

/// The room the label column keeps beside its widest label, points: 4 to
/// the strip's edge and 4 to the column's.
const CLOCK_PAD: f32 = 8.0;

/// The least room between two labelled time lines, points: lying
/// across, a label's width ("10:00:00"); down, its height with space
/// each side.
pub(super) fn time_gap(horizontal: bool) -> f64 {
    if horizontal {
        90.0
    } else {
        40.0
    }
}

/// Down, the label column as wide as the widest time of day on show
/// needs, "Fri 00:04:27" too, so none is cut, and on a take past midnight
/// as wide as a label naming any of its days, so it holds one width
/// wherever the strip looks (Hugh on #2174), points; on the strip `g` of a
/// take `take_end` seconds long so far.
pub(super) fn clock_column(
    painter: &egui::Painter,
    font: &FontId,
    w: &WallClock,
    g: &super::Geom,
    take_end: f64,
) -> f32 {
    let tstep = time_step(g.span, time_gap(false), g.r.height() as f64);
    let oldest = g.oldest();
    time_lines(g.end, oldest, tstep)
        .into_iter()
        .map(|t| w.label(t, tstep, (oldest, take_end)))
        .chain(w.day_labels(take_end))
        .map(|label| {
            painter
                .layout_no_wrap(label, font.clone(), Color32::WHITE)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max)
        + CLOCK_PAD
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

    /// On every strip from 1 s to 60 s, lying across or standing down,
    /// at least one time line is labelled, and no two neighbours share a
    /// label: the step is whole seconds at the least (#2068).
    #[test]
    fn short_strips_label_each_time_line_once() {
        use timegrapher_core::library::length_text;
        // A 5 s take lying across a wide strip: a label each second.
        let step = time_step(5.0, 90.0, 1000.0);
        let labels: Vec<String> = time_lines(5.0, -0.5, step)
            .into_iter()
            .map(length_text)
            .collect();
        assert_eq!(labels, ["0:05", "0:04", "0:03", "0:02", "0:01", "0:00"]);
        for (gap, along) in [
            (90.0, 300.0),
            (90.0, 1000.0),
            (90.0, 1900.0),
            (40.0, 200.0),
            (40.0, 900.0),
        ] {
            for span in 1..=60 {
                let span = span as f64;
                let step = time_step(span, gap, along);
                assert!(step >= 1.0, "{step} s at {span} s over {along}");
                if span * gap / along <= 1.0 {
                    assert_eq!(step, 1.0, "1 s lines where they fit, {span} s over {along}");
                }
                let labels: Vec<String> = time_lines(span, -0.5, step)
                    .into_iter()
                    .map(length_text)
                    .collect();
                assert!(!labels.is_empty(), "none at {span} s over {along}");
                for pair in labels.windows(2) {
                    assert_ne!(pair[0], pair[1], "{labels:?} at {span} s over {along}");
                }
            }
        }
    }

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
        texts_with(height, half_width_ms, &Extras::default())
    }

    /// [`texts_at`] with `extras`.
    fn texts_with(height: f32, half_width_ms: f64, extras: &Extras) -> Vec<(String, Rect)> {
        let ctx = egui::Context::default();
        let view = StripView {
            half_width_ms,
            span_s: 100.0,
            horizontal: true,
            still: false,
            clock: false,
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
                        extras,
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

    /// With Time of Day on, each of the take's time labels has the time
    /// of day under it, and both are drawn; off, the take's alone.
    #[test]
    fn time_of_day_goes_under_the_take_time() {
        use timegrapher_core::time_of_day::WallClock;
        let is_take_time = |t: &str| {
            let p: Vec<&str> = t.split(':').collect();
            p.len() == 2 && p[0].len() == 1 && p[1].len() == 2
        };
        let is_clock = |t: &str| {
            let p: Vec<&str> = t.split(':').collect();
            p.len() == 3
                && p.iter()
                    .all(|x| x.len() == 2 && x.bytes().all(|b| b.is_ascii_digit()))
        };
        let off = texts_at(260.0, 20.0);
        let take: Vec<_> = off.iter().filter(|t| is_take_time(&t.0)).collect();
        assert!(take.len() >= 3, "{off:?}");
        assert!(!off.iter().any(|t| is_clock(&t.0)), "{off:?}");
        // 2026-11-20 14:32:17.5 UTC.
        let wall = WallClock::new(1_795_185_137.5, &[]);
        let extras = Extras {
            wall: Some(&wall),
            ..Default::default()
        };
        let on = texts_with(260.0, 20.0, &extras);
        let take: Vec<_> = on.iter().filter(|t| is_take_time(&t.0)).collect();
        let clock: Vec<_> = on.iter().filter(|t| is_clock(&t.0)).collect();
        assert!(take.len() >= 3, "{on:?}");
        assert_eq!(take.len(), clock.len(), "{on:?}");
        // Each time of day sits under its take time, on the same line.
        for (a, b) in take.iter().zip(&clock) {
            assert!((a.1.center().x - b.1.center().x).abs() < 1.0, "{a:?} {b:?}");
            assert!(b.1.top() >= a.1.bottom() - 1.0, "{a:?} {b:?}");
        }
    }

    /// Whether a drawn text is a time of day naming its day: "Fri
    /// 00:04:27".
    fn names_day(t: &str) -> bool {
        let b = t.as_bytes();
        t.len() == 12 && b[..3].iter().all(u8::is_ascii_alphabetic) && b[3] == b' '
    }

    /// Every text drawn, and where, on a strip `size` points, lying across
    /// or down, with Time of Day on over a take begun 2026-11-20 14:32:17.5
    /// UTC whose last beat is at `take_end`; the strip shows `span`
    /// seconds to `end`, the cursor at `cursor_t`. With the strip's outer
    /// rect.
    fn clock_texts(
        horizontal: bool,
        size: Vec2,
        span_end: (f64, f64),
        take_end: f64,
        cursor_t: Option<f64>,
    ) -> (Vec<(String, Rect)>, Rect) {
        clock_texts_from(
            1_795_185_137.5,
            horizontal,
            size,
            span_end,
            (take_end, cursor_t),
        )
    }

    /// [`clock_texts`] for a take begun at Unix second `start`.
    fn clock_texts_from(
        start: f64,
        horizontal: bool,
        size: Vec2,
        (span_s, end): (f64, f64),
        (take_end, cursor_t): (f64, Option<f64>),
    ) -> (Vec<(String, Rect)>, Rect) {
        let wall = timegrapher_core::time_of_day::WallClock::new(start, &[]);
        let extras = Extras {
            wall: Some(&wall),
            cursor_t,
            ..Default::default()
        };
        let view = StripView {
            half_width_ms: 20.0,
            span_s,
            horizontal,
            still: false,
            clock: true,
        };
        let beats = [Beat {
            index: 0,
            time: take_end,
            quality: 1.0,
        }];
        let anchor = Some(Anchor {
            index: 0,
            time: take_end,
        });
        let ctx = egui::Context::default();
        let mut outer = Rect::NOTHING;
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size + Vec2::splat(40.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    outer = Rect::from_min_size(ui.max_rect().min, size);
                    draw_strip(ui, &beats, 0.125, anchor, end, &view, &extras, size);
                });
            },
        );
        let texts = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((
                    t.galley.text().to_string(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                )),
                _ => None,
            })
            .collect();
        (texts, outer)
    }

    /// On a take falling on more than one day, its first label names the
    /// day, at a step under six hours too, and the label is drawn whole
    /// inside the strip, standing down (the column widened for it) or
    /// lying across, at 1000 and 800 pt (Hugh on #2174). The strip shows
    /// the take's first 10 min, its last beat 30 h in.
    #[test]
    fn a_label_naming_its_day_is_drawn_whole() {
        for horizontal in [false, true] {
            for width in [1000.0, 800.0] {
                let size = Vec2::new(width, 400.0);
                let (texts, outer) =
                    clock_texts(horizontal, size, (1200.0, 600.0), 108_000.0, None);
                let named: Vec<_> = texts.iter().filter(|t| names_day(&t.0)).collect();
                assert_eq!(named.len(), 1, "{horizontal} at {width}: {texts:?}");
                let at = named[0].1;
                assert!(
                    at.left() >= outer.left() && at.right() <= outer.right(),
                    "{horizontal} at {width}: {named:?} cut by {outer:?}"
                );
            }
        }
    }

    /// Standing down, the label column widens only for a label that needs
    /// it: on a take inside one day the plain times sit where they did,
    /// 4 pt from a 52 pt column's edge; one naming its day pushes the
    /// strip right.
    #[test]
    fn the_down_column_widens_only_for_a_day() {
        let size = Vec2::new(800.0, 400.0);
        let right_edge = |take_end: f64| {
            let (texts, outer) = clock_texts(false, size, (1200.0, 600.0), take_end, None);
            let clock = texts
                .iter()
                .filter(|t| t.0.len() >= 8 && t.0.matches(':').count() == 2)
                .map(|t| t.1.right() - outer.left())
                .fold(0.0_f32, f32::max);
            assert!(clock > 0.0, "{texts:?}");
            clock
        };
        assert!((right_edge(600.0) - 48.0).abs() < 0.5);
        assert!(right_edge(108_000.0) > 52.0);
    }

    /// The cursor's time of day names the day only on a take falling on
    /// more than one day.
    #[test]
    fn the_cursor_names_the_day_on_a_take_of_days() {
        let size = Vec2::new(800.0, 400.0);
        let cursor = |take_end: f64| {
            let (texts, _) = clock_texts(true, size, (1200.0, 600.0), take_end, Some(300.0));
            texts
                .into_iter()
                .map(|t| t.0)
                .find(|t| t.starts_with("5:00.00"))
                .unwrap_or_default()
        };
        let one_day = cursor(600.0);
        let two_days = cursor(108_000.0);
        assert!(
            one_day
                .split("  ")
                .nth(1)
                .is_some_and(|c| !names_day_fine(c)),
            "{one_day}"
        );
        assert!(
            two_days.split("  ").nth(1).is_some_and(names_day_fine),
            "{two_days}"
        );
    }

    /// "Fri 14:42:17.50": the cursor's time of day naming its day.
    fn names_day_fine(t: &str) -> bool {
        t.len() == 15 && names_day(&t[..12])
    }

    /// Standing down on a take past midnight, the plot's left edge is the
    /// same wherever the strip looks: at the take's first 10 min, around
    /// its first midnight on this computer's clock, and a few hours after
    /// it with no day named on show (Hugh on #2174).
    #[test]
    fn the_down_column_holds_one_width_over_a_take_of_days() {
        use chrono::{Local, TimeZone};
        let start = Local.timestamp_opt(1_795_185_137, 0).unwrap();
        let next = start
            .date_naive()
            .succ_opt()
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let midnight = Local.from_local_datetime(&next).earliest().unwrap();
        let t_mid = (midnight.timestamp() - start.timestamp()) as f64 + 0.5;
        let size = Vec2::new(800.0, 400.0);
        let edge = |span_end: (f64, f64)| {
            let (texts, _) = clock_texts(false, size, span_end, 108_000.0, None);
            let clock: Vec<_> = texts
                .iter()
                .filter(|t| t.0.matches(':').count() == 2)
                .collect();
            assert!(!clock.is_empty(), "{texts:?}");
            let named = clock.iter().any(|t| names_day(&t.0));
            let right = clock.iter().map(|t| t.1.right()).fold(0.0_f32, f32::max);
            (right, named)
        };
        let (first, named) = edge((1200.0, 600.0));
        assert!(named);
        let (around, named) = edge((1200.0, t_mid + 300.0));
        assert!(named);
        let (after, named) = edge((600.0, t_mid + 3.0 * 3600.0));
        assert!(!named);
        assert!((first - around).abs() < 0.01, "{first} {around}");
        assert!((first - after).abs() < 0.01, "{first} {after}");
    }

    /// Fitted to a take past midnight, the strip starts exactly at its
    /// second 0 and draws no line there, so its first line drawn names the
    /// day, across and down (Walter on #2556).
    #[test]
    fn a_fitted_take_of_days_names_the_day_on_its_first_line() {
        for horizontal in [false, true] {
            let (texts, _) = clock_texts(
                horizontal,
                Vec2::new(800.0, 400.0),
                (600.0, 600.0),
                108_000.0,
                None,
            );
            assert!(
                !texts.iter().any(|t| t.0 == "0:00"),
                "{horizontal}: {texts:?}"
            );
            let named: Vec<_> = texts.iter().filter(|t| names_day(&t.0)).collect();
            assert_eq!(named.len(), 1, "{horizontal}: {texts:?}");
        }
    }

    /// Standing down at a 6 h step on a take inside one day, the label
    /// naming its day ("Wed 14:32", wider than 52 pt holds) is drawn
    /// whole, 4 pt clear of the strip's edge: the column keeps the measure
    /// of the labels on show as its floor, not only the take's day labels
    /// (Hugh and Trevor on #2556). The take begins on Wednesday 18 Nov
    /// 2026 at 14:32:17.5 UTC; the strip shows two days back from 10 min
    /// into it, so only the line at 0 is drawn.
    #[test]
    fn the_down_column_fits_a_six_hour_label_on_a_one_day_take() {
        let size = Vec2::new(800.0, 400.0);
        let wed = 1_795_185_137.5 - 2.0 * 86_400.0;
        let (texts, outer) =
            clock_texts_from(wed, false, size, (2.0 * 86_400.0, 600.0), (600.0, None));
        let named: Vec<_> = texts
            .iter()
            .filter(|t| t.0.len() == 9 && t.0.as_bytes()[3] == b' ')
            .collect();
        assert_eq!(named.len(), 1, "{texts:?}");
        assert!(
            named[0].1.left() >= outer.left() + 3.9,
            "{named:?} by {outer:?}"
        );
    }
}
