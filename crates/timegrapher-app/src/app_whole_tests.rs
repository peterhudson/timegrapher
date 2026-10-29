//! The whole take's reading, headless: it is session's reading of the
//! same stretch, at any length session reads.

use super::tests::synthetic;
use super::*;

/// The whole take's beat error, where its sign turns over at a jump
/// in the beats, combines the sizes either side and says so, as
/// `session` does, not a figure that cancels to nothing; where the
/// beats agree in sign either side it stays signed.
#[test]
fn whole_take_beat_error_keeps_its_size_across_a_turn() {
    // 120 s at 28,800 bph with a jump at 80 s: the tock 0.6 ms late
    // for the 640 beats before it and `after` ms for the 320 after.
    let app_with = |after: f64| {
        let beats = (0..960)
            .map(|index| {
                let be = if index < 640 { 0.6 } else { after };
                let half = be / 2000.0 * if index % 2 == 1 { 1.0 } else { -1.0 };
                timegrapher_core::beats::Beat {
                    time: index as f64 * 0.125 + half,
                    index,
                    quality: 1.0,
                }
            })
            .collect();
        let log = BeatLog {
            sample_rate: 48_000,
            duration_s: 120.0,
            bph: 28_800,
            lift_deg: 52.0,
            beats,
            amplitude_windows: Vec::new(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: vec![80.0],
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        };
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.live = Some(LiveAnalyzer::from_log(log, LiveConfig::default()));
        app
    };
    let sizes = |w: &WholeTake| w.m.caveats.contains(&Caveat::BeatErrorAsSize);
    // Sizes across the turn, beat-weighted: (0.6 × 640 + 0.4 × 320) / 960.
    let size = (0.6 * 640.0 + 0.4 * 320.0) / 960.0;
    // What a signed combination would cancel to: (0.6 × 640 − 0.4 × 320) / 960.
    let cancelled = (0.6 * 640.0 - 0.4 * 320.0) / 960.0;
    let w = app_with(-0.4).whole_take().expect("whole take");
    let be = w.m.beat_error_ms.unwrap();
    assert!(sizes(&w));
    // The jump is a break the whole take's hover names, as a card's
    // own reading never reaches across it.
    assert!(w.m.caveats.contains(&Caveat::Breaks { count: 1 }));
    assert!(
        whole_breaks_text(&w).starts_with(" 1 break in this stretch: a hole in the sound"),
        "{}",
        whole_breaks_text(&w)
    );
    assert!((be - size).abs() < 1e-3, "{be} vs {size}");
    assert_eq!(w.m.beat_error(), Some(be));
    assert!((be - cancelled).abs() > 0.2, "{be} all but cancelled");
    // Where the beats keep one sign either side, the figure keeps it:
    // the same size, signed.
    let w = app_with(0.4).whole_take().expect("whole take");
    let be = w.m.beat_error_ms.unwrap();
    assert!(!sizes(&w));
    assert!((be.abs() - size).abs() < 1e-3, "{be} vs {size}");
    assert!(be < 0.0, "signed: {be}");
}

/// A stopped take has its whole-take reading at any length session
/// reads it, and Average Over, which session has none of, changes
/// nothing in it: the app adds no shortest take of its own.
#[test]
fn the_whole_take_is_read_at_any_length_and_any_average() {
    let mut app = synthetic(60.0, 12.0);
    app.average_s = 60.0;
    let long = app.whole_take().expect("60 s at an Average Over of 60 s");
    app.average_s = 10.0;
    let short = app.whole_take().expect("60 s at an Average Over of 10 s");
    assert_eq!(long.m.rate_s_per_day, short.m.rate_s_per_day);
    assert_eq!(long.span, short.span);
    let app = synthetic(8.0, 12.0);
    let w = app.whole_take().expect("an 8 s take, as session reads one");
    assert!(w.span > 0.0 && w.span <= 8.0, "{}", w.span);
}

/// The whole take line is `session`'s reading of the same stretch:
/// the rate fitted over every beat, not a mean of the readings shown.
#[test]
fn whole_take_is_the_session_reading() {
    let app = synthetic(60.0, 12.0);
    let w = app.whole_take().expect("whole take");
    let log = steady::log_of(app.live.as_ref().unwrap()).unwrap();
    let spans = app.current_spans();
    // Just listened to, the take reads to its last reading with a
    // watch heard, never past its end.
    let to = app
        .last_heard_s()
        .min(app.live.as_ref().unwrap().duration_s());
    let from = spans[0].0.max(0.0);
    let m = timegrapher_core::session::measure(&log, None, from, to);
    assert_eq!(w.m.rate_s_per_day, m.rate_s_per_day);
    assert_eq!(w.m.amplitude_deg, m.amplitude_deg);
    assert!((w.span - (to - from)).abs() < 1e-9, "{}", w.span);
}
