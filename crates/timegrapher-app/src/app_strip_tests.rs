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
    assert!((app.strip.span_s - app.latest_s()).abs() < 1e-9);
    let end = app.latest_s();
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

    let zoom = StripInput {
        span_factor: 2.0,
        width_factor: 0.5,
        ..Default::default()
    };
    app.apply_strip_input(zoom, app.strip_end_s());
    assert_eq!(app.strip.span_s, 60.0);
    assert_eq!(app.strip.half_width_ms, 5.0);

    // Zoomed about a time, that time stays where it was on the strip.
    let end = app.strip_end_s();
    let at = end - 15.0;
    let zoom = StripInput {
        span_factor: 0.5,
        zoom_at: Some(at),
        ..Default::default()
    };
    app.apply_strip_input(zoom, end);
    assert_eq!(app.strip.span_s, 30.0);
    let now = app.strip_end.expect("looking back");
    assert!(((now - at) / 30.0 - 15.0 / 60.0).abs() < 1e-9, "{now}");
}
