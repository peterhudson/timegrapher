//! The strip across a take's breaks, headless: a steady watch's trace
//! runs on through a gap, as core reads its beats on true time.

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
        app.put_take_breaks(Some(breaks.clone()));
        app.show_log(log, "paused".into(), vec![], vec![]);
        let half = app.strip.half_width_ms;
        assert!(half <= 1.0, "follow {follow}: ±{half} ms");
        let live = app.live.as_ref().unwrap();
        let anchor = app.anchor.unwrap();
        for b in live.beats() {
            let lead = anchor.lead_on_wall(b, 0.125, &breaks);
            assert!(lead.abs() < half, "{} at {}: {lead}", b.index, b.time);
        }
    }
}
