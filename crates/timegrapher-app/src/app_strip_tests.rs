//! The strip, headless: a steady watch's trace runs on through a gap,
//! as core reads its beats on true time; Fit, and dragging to look back.

use super::tests::{synthetic, themed};
use super::*;
use timegrapher_core::beats::Beat;
use timegrapher_core::breaks::{renumber, Break};

/// Four minutes of a steady watch at 28,800 bph with 0.3 ms beat error,
/// paused for half an hour and a fraction of a beat after two minutes:
/// its beats as core numbers them across the pause, and the pause.
#[expect(
    clippy::disallowed_methods,
    reason = "a test's take fixture, numbered as core numbers a take opened"
)]
fn paused_take() -> (BeatLog, Vec<Break>) {
    let (p, at, gap_s) = (0.125, 120.0, 1800.0371);
    let mut beats: Vec<Beat> = Vec::new();
    for k in 0.. {
        let t = k as f64 * p + if k % 2 == 0 { 3e-4 } else { -3e-4 };
        let audio = if t < at {
            t
        } else if t >= at + gap_s {
            t - gap_s
        } else {
            continue;
        };
        if audio > 240.0 {
            break;
        }
        let index = beats.last().map_or(0, |l| {
            l.index + ((audio - l.time) / p).round().max(1.0) as i64
        });
        beats.push(Beat {
            index,
            time: audio,
            quality: 1.0,
        });
    }
    let breaks = vec![Break { audio_s: at, gap_s }];
    renumber(&mut beats, &mut Vec::new(), &breaks, &[], 28800);
    let log = BeatLog {
        lock_template_kept: false,
        sample_rate: 48000,
        duration_s: 240.0,
        bph: 28800,
        lift_deg: 52.0,
        beats,
        amplitude_windows: vec![],
        clipped_beats: vec![],
        clipped_samples: 0,
        jumps: vec![],
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    (log, breaks)
}

/// A steady take paused for half an hour fits the narrowest strip, with
/// Auto-Centre off or on: on true time its beats after the pause lie on
/// the line of those before, where on audio seconds they stood 37 ms off.
#[test]
fn a_steady_take_runs_on_through_a_pause_on_the_strip() {
    for follow in [false, true] {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.follow = follow;
        let (log, breaks) = paused_take();
        app.put_take_breaks(Some(breaks.clone()), None);
        app.show_log(log, "paused".into(), vec![], vec![]);
        let half = app.strip.half_width_ms;
        assert!(half <= 1.0, "follow {follow}: ±{half} ms");
        let live = app.live.as_ref().unwrap();
        let anchor = app.anchor.unwrap();
        for b in live.beats() {
            let lead = anchor.lead_on_true(b, 0.125, &breaks, app.take_rate_clock());
            assert!(lead.abs() < half, "{} at {}: {lead}", b.index, b.time);
        }
    }
}

/// Fit stays on and keeps the whole session in view frame by frame;
/// zooming or panning the strip's time turns it off, its width doesn't.
#[test]
fn fit_follows_the_take_until_the_strip_is_zoomed_or_panned() {
    let mut app = synthetic(40.0, 0.0);
    app.fit_strip();
    // As if the session had grown since the last frame.
    app.strip.set_span(10.0);
    app.follow_fit();
    let end = app.take_span().unwrap().1;
    assert!((app.strip.span_s - end).abs() < 1e-9);
    app.apply_strip_input(
        StripInput {
            width_factor: 2.0,
            ..Default::default()
        },
        end,
    );
    assert!(app.strip_fit, "the width is not the strip's time");
    app.apply_strip_input(
        StripInput {
            span_factor: 0.5,
            ..Default::default()
        },
        end,
    );
    assert!(!app.strip_fit);
    let zoomed = app.strip.span_s;
    app.follow_fit();
    assert_eq!(app.strip.span_s, zoomed, "off, it leaves the length alone");
    app.fit_strip();
    app.apply_strip_input(
        StripInput {
            time_shift_s: -5.0,
            ..Default::default()
        },
        end,
    );
    assert!(!app.strip_fit);
}

/// Fit is one fixed switch: the strip's toolbar is as wide with it on
/// as off, so nothing after it moves.
#[test]
fn the_fit_switch_never_moves_the_strip_toolbar() {
    let mut app = synthetic(40.0, 0.0);
    let ctx = themed();
    let width = |app: &mut TimegrapherApp| {
        let mut w = 0.0;
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    w = ui
                        .horizontal(|ui| app.strip_settings(ui))
                        .response
                        .rect
                        .width();
                });
            });
        }
        w
    };
    app.strip_fit = false;
    let off = width(&mut app);
    app.fit_take();
    assert!(app.strip_fit, "an opened take is fitted");
    let on = width(&mut app);
    assert_eq!(off, on);
}

#[test]
fn dragging_the_strip_looks_back_and_returns() {
    let mut app = synthetic(40.0, 0.0);
    let latest = app.latest_s();
    let back = StripInput {
        time_shift_s: -5.0,
        ..Default::default()
    };
    app.apply_strip_input(back, app.strip_end_s());
    assert!((app.strip_end_s() - (latest - 5.0)).abs() < 1e-6);
    let forward = StripInput {
        time_shift_s: 20.0,
        ..Default::default()
    };
    app.apply_strip_input(forward, app.strip_end_s());
    assert_eq!(
        app.strip_end, None,
        "past the newest beat follows live again"
    );

    app.follow = true;
    let slide = StripInput {
        lead_shift_ms: 1.0,
        ..Default::default()
    };
    let before = app.anchor.unwrap().time;
    app.apply_strip_input(slide, app.strip_end_s());
    assert!((app.anchor.unwrap().time - before - 0.001).abs() < 1e-12);
    assert!(!app.follow, "sliding by hand ends auto-centring");

    // Zoomed out within the 40 s take.
    app.strip.set_span(15.0);
    let zoom = StripInput {
        span_factor: 2.0,
        width_factor: 0.5,
        ..Default::default()
    };
    app.apply_strip_input(zoom, app.strip_end_s());
    assert_eq!(app.strip.span_s, 30.0);
    assert_eq!(app.strip.half_width_ms, 5.0);

    // Live, a zoom keeps the newest beat at the edge: the moment the
    // cards and the Profile show stays put.
    let end = app.strip_end_s();
    let zoom = StripInput {
        span_factor: 0.5,
        zoom_at: Some(end - 15.0),
        ..Default::default()
    };
    app.apply_strip_input(zoom, end);
    assert_eq!(app.strip.span_s, 15.0);
    assert_eq!(app.strip_end, None, "a zoom while live stays live");

    // Looking back, a zoom about a time keeps that time where it was on
    // the strip.
    app.strip.set_span(30.0);
    app.apply_strip_input(back, app.strip_end_s());
    let end = app.strip_end_s();
    let at = end - 15.0;
    let zoom = StripInput {
        span_factor: 0.5,
        zoom_at: Some(at),
        ..Default::default()
    };
    app.apply_strip_input(zoom, end);
    assert_eq!(app.strip.span_s, 15.0);
    let now = app.strip_end.expect("looking back");
    assert!(((now - at) / 15.0 - 15.0 / 30.0).abs() < 1e-9, "{now}");
}

/// A zoom leaves the strip at a round length, never such as 3:01, and
/// the small steps a wheel turn plays out in still add up (#1068 item 3).
#[test]
fn a_zoom_leaves_a_round_length() {
    let mut app = synthetic(600.0, 0.0);
    app.strip.set_span(120.0);
    let step = StripInput {
        span_factor: 1.02,
        ..Default::default()
    };
    let mut seen = Vec::new();
    for _ in 0..20 {
        app.apply_strip_input(step, app.strip_end_s());
        let s = app.strip.span_s;
        assert_eq!(s % 60.0, 0.0, "{s}");
        seen.push(s);
    }
    assert_eq!(seen.last(), Some(&180.0), "{seen:?}");
    // Below 2 min, a 5 s step; below 10 s, whole seconds.
    app.strip.set_span(60.0);
    let zoom_in = |f: f64| StripInput {
        span_factor: f,
        ..Default::default()
    };
    app.apply_strip_input(zoom_in(0.93), app.strip_end_s());
    assert_eq!(app.strip.span_s, 55.0);
    app.strip.set_span(8.0);
    app.apply_strip_input(zoom_in(0.8), app.strip_end_s());
    assert_eq!(app.strip.span_s, 6.0);
    // Zoomed in far past the shortest length, a zoom back out starts
    // from it, not from where the zooms in were heading.
    for _ in 0..20 {
        app.apply_strip_input(zoom_in(0.5), app.strip_end_s());
    }
    assert_eq!(app.strip.span_s, StripView::MIN_SPAN_S);
    app.apply_strip_input(zoom_in(1.1), app.strip_end_s());
    assert_eq!(app.strip.span_s, 2.0);
    app.apply_strip_input(zoom_in(1.5), app.strip_end_s());
    assert_eq!(app.strip.span_s, 3.0);
    // Nor from below it when that still rounds to the shortest length.
    app.strip.set_span(2.0);
    app.apply_strip_input(zoom_in(0.76), app.strip_end_s());
    assert_eq!(app.strip.span_s, 2.0);
    app.apply_strip_input(zoom_in(1.3), app.strip_end_s());
    assert_eq!(app.strip.span_s, 3.0);
}

/// Zooming the strip's time out stops at the take's length: past it is
/// only blank paper. A length already longer, typed in, is kept.
#[test]
fn zooming_out_stops_at_the_take() {
    let mut app = synthetic(40.0, 0.0);
    let latest = app.latest_s();
    app.strip.set_span(10.0);
    let out = StripInput {
        span_factor: 10.0,
        ..Default::default()
    };
    app.apply_strip_input(out, app.strip_end_s());
    assert_eq!(app.strip.span_s, latest);
    app.strip.set_span(100.0);
    app.apply_strip_input(out, app.strip_end_s());
    assert_eq!(app.strip.span_s, 100.0);
}

/// The Tick Tock Profile and the Scope follow the strip's newest edge:
/// panning back moves the moment the Profile shows, and says it in the
/// toolbar; a hover alone changes nothing; a double-click goes back to
/// the newest beats. Where no sound was kept, or the moment is masked,
/// one line says so in place of the plot.
#[test]
fn the_sound_panes_follow_the_strips_newest_edge() {
    let mut app = synthetic(40.0, 0.0);
    let heard = app.live.as_ref().unwrap().tick_profiles()[0]
        .clone()
        .unwrap();
    // A kept sound every 2 s, each told apart by its beats.
    let kept = |t: f64| {
        let p = TickProfile {
            beats: t as usize,
            ..heard.clone()
        };
        let span_s = 10.0;
        (
            t,
            scope_panes::Kept {
                span_s,
                sides: [Some(p), None],
            },
        )
    };
    app.sound_history = (5..=20).map(|k| kept(2.0 * k as f64)).collect();
    let latest = app.latest_s();
    let beats_shown = |app: &mut TimegrapherApp| {
        let end = app.strip_end_s();
        let s = app.profiles_at(end);
        s.profiles[0].as_ref().map(|p| p.beats)
    };
    let newest = beats_shown(&mut app);
    assert_eq!(newest, Some(2 * (latest as usize / 2)));
    assert_eq!(app.sound_moment(), None);
    // A hover alone moves nothing.
    let hover = StripInput {
        hover_t: Some(15.0),
        ..Default::default()
    };
    app.apply_strip_input(hover, app.strip_end_s());
    assert_eq!(app.strip_end, None);
    assert_eq!(beats_shown(&mut app), newest);
    // Panning back moves the Profile's window end with the strip.
    let back = StripInput {
        time_shift_s: -5.0,
        ..Default::default()
    };
    app.apply_strip_input(back, app.strip_end_s());
    let end = app.strip_end_s();
    assert!((end - (latest - 5.0)).abs() < 1e-6);
    assert_eq!(beats_shown(&mut app), Some(2 * (end as usize / 2)));
    let at = length_text(end);
    assert_eq!(app.sound_moment(), Some(format!("at {at}")));
    // Masked there: one line in place of the plot.
    app.masked = vec![(end - 15.0, end + 1.0)];
    let s = app.profiles_at(end);
    assert_eq!(s.line, Some(format!("Masked at {at}")));
    app.masked.clear();
    // Kept nowhere near: said, not filled from either side.
    app.sound_history.retain(|h| h.0 > end + 2.0);
    let s = app.profiles_at(end);
    assert_eq!(s.line, Some(format!("No sound kept from {at}")));
    // A double-click goes back to the newest beats.
    let reset = StripInput {
        reset: true,
        ..Default::default()
    };
    app.apply_strip_input(reset, app.strip_end_s());
    assert_eq!(app.strip_end, None);
    assert_eq!(app.sound_moment(), None);
    assert_eq!(beats_shown(&mut app), newest);
}

/// In an upright strip each overlay's axis is a row above it, its short
/// name over its unit (#1278), whose near end's number would sit under the
/// unit: the number is left out, and hovering the row gives the scale end
/// to end.
#[test]
fn hovering_an_overlay_axis_row_gives_its_range() {
    let ctx = themed();
    let mut app = synthetic(40.0, 5.0);
    app.overlays = [true, false, false];
    app.strip.horizontal = false;
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 500.0));
    let pass = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.strip_pane(ui));
        })
    };
    for _ in 0..3 {
        pass(&mut app, Vec::new());
    }
    let out = pass(&mut app, Vec::new());
    let drawn = super::tests::painted_text(&out);
    // The first switch in the settings is Amplitude: its short name, and
    // its unit on the line under it, never "Amp. (°)" on one line.
    let at = |words: &str| {
        out.shapes.iter().find_map(|c| match &c.shape {
            egui::Shape::Text(t) if t.galley.text() == words => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
            }
            _ => None,
        })
    };
    let name = at("Amp.").unwrap_or_else(|| panic!("{drawn}"));
    let unit = at("(°)").unwrap_or_else(|| panic!("{drawn}"));
    assert!(unit.top() >= name.bottom() - 1.0, "{name:?} {unit:?}");
    assert!((unit.left() - name.left()).abs() < 0.5, "one column");
    assert!(!drawn.contains("Amp. (°)"), "{drawn}");
    assert!(!drawn.contains("Amplitude: "), "no hover yet: {drawn}");
    // The row: under the key's line, across the middle of the strip.
    let over = egui::pos2(450.0, 30.0);
    pass(&mut app, vec![egui::Event::PointerMoved(over)]);
    let hovered = super::tests::painted_text(&pass(&mut app, Vec::new()));
    let line = hovered
        .lines()
        .find(|l| l.starts_with("Amplitude: "))
        .unwrap_or_else(|| panic!("{hovered}"));
    assert!(line.contains(" to ") && line.ends_with(" °"), "{line}");
}

/// Upright with two overlays on, each axis row is two lines: its short
/// name, then its unit with the numbers on the unit's line and none on
/// the name's; and the first row's unit ends above the second row's name
/// (#1278).
#[test]
fn upright_overlay_rows_put_their_numbers_on_the_units_line() {
    let ctx = themed();
    let mut app = synthetic(40.0, 5.0);
    app.overlays = [true, true, false];
    app.strip.horizontal = false;
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 500.0));
    let mut out = None;
    for _ in 0..4 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        out = Some(ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.strip_pane(ui));
        }));
    }
    let out = out.unwrap();
    let texts: Vec<(String, egui::Rect)> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some((
                t.galley.text().to_string(),
                egui::Rect::from_min_size(t.pos, t.galley.size()),
            )),
            _ => None,
        })
        .collect();
    // Each row: a unit "(…)" and the name in the same column just over it.
    let mut rows: Vec<(egui::Rect, egui::Rect)> = texts
        .iter()
        .filter(|t| t.0.starts_with('(') && t.0.ends_with(')'))
        .filter_map(|(_, unit)| {
            texts
                .iter()
                .find(|(_, n)| {
                    (n.left() - unit.left()).abs() < 0.5 && (n.bottom() - unit.top()).abs() < 1.0
                })
                .map(|(_, name)| (*name, *unit))
        })
        .collect();
    rows.sort_by(|a, b| a.0.top().total_cmp(&b.0.top()));
    assert_eq!(rows.len(), 2, "{texts:?}");
    for (name, unit) in &rows {
        let on = |line: &egui::Rect| {
            texts
                .iter()
                .filter(|(_, r)| {
                    r.left() > name.right() && (r.center().y - line.center().y).abs() < 1.0
                })
                .count()
        };
        assert!(on(unit) > 0, "numbers on the unit's line: {texts:?}");
        assert_eq!(on(name), 0, "none on the name's line: {texts:?}");
    }
    assert!(rows[0].1.bottom() <= rows[1].0.top() + 0.5, "{rows:?}");
}

/// Upright, "Late" and "Early" sit on a line of their own under the ms
/// numbers, clear of the end ones (#1278: never "Late −60" or "+60Early").
#[test]
fn an_upright_strips_early_and_late_stand_clear_of_its_numbers() {
    let ctx = themed();
    let mut app = synthetic(40.0, 5.0);
    app.strip.horizontal = false;
    for width in [460.0, 900.0] {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 500.0));
        let mut out = None;
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            out = Some(ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.strip_pane(ui));
            }));
        }
        let out = out.unwrap();
        let texts: Vec<(String, egui::Rect)> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((
                    t.galley.text().to_string(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                )),
                _ => None,
            })
            .collect();
        let at = |w: &str| {
            texts
                .iter()
                .find(|t| t.0 == w)
                .unwrap_or_else(|| panic!("{w}: {texts:?}"))
                .1
        };
        // The ms numbers: signed, or 0, under the plot.
        let numbers: Vec<egui::Rect> = texts
            .iter()
            .filter(|t| t.0 == "0" || t.0.starts_with('+') || t.0.starts_with('−'))
            .map(|t| t.1)
            .filter(|r| r.top() > at("Late").top() - 30.0)
            .collect();
        assert!(!numbers.is_empty(), "{texts:?}");
        for word in ["Late", "Early"] {
            let w = at(word);
            // Inside the pane, never cut off at its foot.
            assert!(w.bottom() <= screen.bottom(), "{word} {w:?} in {screen:?}");
            for n in &numbers {
                assert!(w.top() >= n.bottom() - 1.0, "{word} {w:?} under {n:?}");
            }
            for (other, r) in texts.iter().filter(|t| t.0 != word) {
                assert!(
                    !w.expand(2.0).intersects(*r),
                    "{word} {w:?} runs into {other} {r:?}"
                );
            }
        }
    }
}

/// Lying across, an overlay's axis is a column right of the strip under
/// its short name, its unit on the line under that, and nothing at its
/// foot; hovering the name gives it in full. The column is as wide as
/// its widest text, narrower than the full name would need.
#[test]
fn an_overlay_axis_column_says_its_short_name_over_its_unit() {
    let ctx = themed();
    let mut app = synthetic(40.0, 5.0);
    app.overlays = [true, false, false];
    app.strip.horizontal = true;
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 500.0));
    let pass = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.strip_pane(ui));
        })
    };
    for _ in 0..3 {
        pass(&mut app, Vec::new());
    }
    let out = pass(&mut app, Vec::new());
    let at = |words: &str| {
        out.shapes.iter().find_map(|c| match &c.shape {
            egui::Shape::Text(t) if t.galley.text() == words => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
            }
            _ => None,
        })
    };
    let name = at("Amp.").expect("the short name");
    let unit = at("(°)").expect("the unit");
    assert!(unit.top() >= name.bottom() - 1.0, "{name:?} {unit:?}");
    assert!((unit.left() - name.left()).abs() < 0.5, "one column");
    assert!(at("°").is_none(), "no unit at the foot");
    let drawn = super::tests::painted_text(&out);
    assert!(!drawn.contains("Amplitude"), "{drawn}");
    // The column ends at the pane's right, narrower than a 58 pt one.
    assert!(screen.right() - name.left() < 58.0, "{name:?}");
    pass(&mut app, vec![egui::Event::PointerMoved(name.center())]);
    let hovered = super::tests::painted_text(&pass(&mut app, Vec::new()));
    assert!(hovered.contains("Amplitude (°)"), "{hovered}");
}

/// Lying across with all three readings on, the overlay axes' columns
/// run left to right in the toolbar's order, each clear of the one before
/// (its name, unit and numbers), and the last ends inside the pane.
#[test]
fn overlay_axis_columns_lie_side_by_side() {
    let ctx = themed();
    let mut app = synthetic(40.0, 5.0);
    app.overlays = [true, true, true];
    app.strip.horizontal = true;
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 500.0));
    let mut out = None;
    for _ in 0..4 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        out = Some(ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.strip_pane(ui));
        }));
    }
    let out = out.unwrap();
    let texts: Vec<(String, egui::Rect)> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some((
                t.galley.text().to_string(),
                egui::Rect::from_min_size(t.pos, t.galley.size()),
            )),
            _ => None,
        })
        .collect();
    let at = |words: &str| {
        texts
            .iter()
            .find(|(t, _)| t == words)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("{words}: {texts:?}"))
    };
    let names: Vec<egui::Rect> = ["Rate", "Amp.", "B.E. U"].map(at).to_vec();
    // Every text in a column starts at its name's left and ends before
    // the next column's.
    for (k, pair) in names.windows(2).enumerate() {
        let (this, next) = (pair[0], pair[1]);
        let widest = texts
            .iter()
            .filter(|(_, r)| r.left() >= this.left() - 0.5 && r.left() < next.left() - 0.5)
            .filter(|(_, r)| r.left() - this.left() < 10.0)
            .map(|(_, r)| r.right())
            .fold(this.right(), f32::max);
        assert!(
            widest < next.left(),
            "column {k} runs into the next: {texts:?}"
        );
    }
    let last = names[2];
    let widest = texts
        .iter()
        .filter(|(_, r)| r.left() >= last.left() - 0.5)
        .map(|(_, r)| r.right())
        .fold(last.right(), f32::max);
    assert!(widest <= screen.right(), "the last column leaves the pane");
}

/// A masked beat is drawn faint, under the hatching: 30% of its colour
/// over the strip's ground, opaque so crowded dots don't add up to full
/// strength, and painted before the hatching (#1068 item 2).
#[test]
fn masked_beats_are_faint_under_the_hatching() {
    let mut app = synthetic(40.0, 0.0);
    for p in [Pane::Sound, Pane::Scope, Pane::Charts, Pane::Distributions] {
        set_pane_visible(&mut app.panes.tiles, p, false);
    }
    app.mask(20.0, 30.0);
    let ctx = themed();
    let mut out = None;
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1440.0, 900.0),
            )),
            ..Default::default()
        };
        out = Some(ctx.run(input, |ctx| app.panels(ctx)));
    }
    let out = out.unwrap();
    let vis = ctx.style().visuals.clone();
    let [tick, tock] = strip::side_colors(vis.dark_mode);
    let faint = [tick, tock].map(|c| vis.extreme_bg_color.lerp_to_gamma(c, 0.3));
    let dots: Vec<(usize, Color32)> = out
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(i, c)| match &c.shape {
            egui::Shape::Circle(d)
                if d.fill == tick || d.fill == tock || faint.contains(&d.fill) =>
            {
                Some((i, d.fill))
            }
            _ => None,
        })
        .collect();
    let first_full = dots
        .iter()
        .find(|(_, c)| !faint.contains(c))
        .expect("beats")
        .0;
    let masked: Vec<usize> = dots
        .iter()
        .filter(|(_, c)| faint.contains(c))
        .map(|d| d.0)
        .collect();
    assert!(!masked.is_empty(), "the masked beats are drawn");
    // Before the hatching and every other beat.
    let hatch = theme::palette(vis.dark_mode)
        .text_tertiary
        .gamma_multiply(theme::MASK_WASH);
    let first_line = out
        .shapes
        .iter()
        .position(|c| matches!(&c.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == hatch))
        .expect("the hatching");
    let last_masked = *masked.last().unwrap();
    assert!(last_masked < first_line && last_masked < first_full);
}

/// The take's span ends at its newest beat or its newest reading, whichever
/// is later, with none before there is either; with Fit on, the strip's
/// newest edge is that end and never past it (#1256).
#[test]
fn the_takes_span_ends_at_its_newest_beat_or_reading() {
    let empty = TimegrapherApp::with_devices(Vec::new(), None);
    assert_eq!(empty.take_span(), None);
    let mut app = synthetic(40.0, 0.0);
    let beat = app.latest_s();
    assert!(beat > 30.0, "{beat}");
    let reading = |t| TrendPoint {
        t,
        rate_s_per_day: Some(1.0),
        amplitude_deg: Some(280.0),
        amplitude_even_deg: None,
        amplitude_odd_deg: None,
        beat_error_unlock_ms: None,
        beat_error_ms: None,
        rate_error_s_per_day: None,
        amplitude_error_deg: None,
        beat_error_unlock_error_ms: None,
        heard: true,
    };
    app.trend = vec![reading(beat - 10.0)];
    assert_eq!(app.take_span(), Some((0.0, beat)));
    app.trend = vec![reading(beat - 10.0), reading(beat + 5.0)];
    assert_eq!(app.take_span(), Some((0.0, beat + 5.0)));

    app.strip_fit = true;
    app.strip_end = None;
    assert_eq!(app.strip_end_s(), beat + 5.0);
    app.strip_end = Some(1e9);
    assert_eq!(app.strip_end_s(), beat + 5.0);
    app.strip_end = Some(12.0);
    assert_eq!(app.strip_end_s(), 12.0);
    app.strip_fit = false;
    app.strip_end = None;
    assert_eq!(app.strip_end_s(), beat);
}

/// Still paper has nothing to fit: Fit leaves the Length alone while the
/// paper is still, and its newest edge is the latest beat, as with Fit
/// off. Moving the paper again brings Fit back as it was.
#[test]
fn fit_waits_while_the_paper_is_still() {
    let mut app = synthetic(40.0, 0.0);
    app.fit_strip();
    app.strip.still = true;
    app.strip.set_span(10.0);
    app.follow_fit();
    assert_eq!(app.strip.span_s, 10.0, "the Length is the lap");
    assert!(!app.strip_fitted());
    app.strip_fit = false;
    app.fit_strip();
    assert!(!app.strip_fit, "Fit can't be turned on over still paper");
    app.strip_fit = true;
    app.strip.still = false;
    app.follow_fit();
    let end = app.take_span().unwrap().1;
    assert!(
        (app.strip.span_s - end).abs() < 1e-9,
        "moving, Fit fits again"
    );
}
