//! A lift change while listening reads each window again from its edges
//! at the new lift and gates it there, both ways, as a kept take does
//! (#1333): a side the old lift culled comes back, and one the new lift
//! puts out of range goes.

use timegrapher_core::amplitude::AmplitudeWindow;
use timegrapher_core::live::{LiveAnalyzer, LiveConfig};
use timegrapher_core::stream::BeatLog;
use timegrapher_core::synth::{generate, SynthConfig};

#[test]
fn a_live_lift_change_brings_culled_sides_back_and_culls_again() {
    // 395° at the synth's 52°: over the 380° gate, so every side is
    // culled; about 342° at 45°.
    let cfg = SynthConfig {
        duration_s: 30.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 395.0, |_| 0.0);
    let mut l = LiveAnalyzer::new(cfg.sample_rate, LiveConfig::default());
    for c in audio.samples.chunks(4800) {
        l.push(c);
    }
    let read = |l: &LiveAnalyzer| l.reading(20.0).amplitude_deg;
    assert!(!l.amplitude_windows().is_empty());
    assert_eq!(read(&l), None, "culled at 52°");
    l.set_lift(45.0);
    let at45 = read(&l).expect("back at 45°");
    assert!((at45 - 395.0 * 45.0 / 52.0).abs() < 5.0, "{at45}");
    l.set_lift(52.0);
    assert_eq!(read(&l), None, "culled again at 52°");
}

/// A side screened out as a lone wrong sound stays out after a live lift
/// change: the windows are screened again at the new lift, not only read
/// again (#1333, Trevor).
#[test]
fn a_lone_wrong_sound_stays_out_after_a_live_lift_change() {
    let w = |i: usize| AmplitudeWindow {
        start_s: 2.0 * i as f64,
        end_s: 2.0 * i as f64 + 2.0,
        // 318.7° and 345.2° at 52°; window 10's Tick, at 398°, was left
        // out as an outlier with its edges kept.
        even_deg: (i != 10).then_some(318.7),
        odd_deg: Some(345.2),
        beat_error_ms: Some(0.1),
        beat_error_unlock_ms: None,
        even_unlock_ms: Some(if i == 10 { -5.2 } else { -6.5 }),
        odd_unlock_ms: Some(-6.0),
        even_drop_ms: Some(0.0),
        odd_drop_ms: Some(0.0),
        culled_ms: None,
    };
    let log = BeatLog {
        lock_template_kept: false,
        sample_rate: 48000,
        duration_s: 40.0,
        bph: 28800,
        lift_deg: 52.0,
        beats: Vec::new(),
        amplitude_windows: (0..20).map(w).collect(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    let mut l = LiveAnalyzer::from_log(log, LiveConfig::default());
    // At 45° it would read 345° against its neighbours' 276°.
    l.set_lift(45.0);
    let wins = l.amplitude_windows();
    assert_eq!(wins[10].even_deg, None, "screened again at 45°");
    assert_eq!(wins[10].even_unlock_ms, Some(-5.2));
    assert!(wins[9].even_deg.is_some_and(|a| (a - 275.8).abs() < 0.1));
}
