use timegrapher_core::{
    amplitude::AmplitudeWindow,
    beats::Beat,
    breaks::Break,
    live::{LiveAnalyzer, LiveConfig},
    longrun,
    longterm::LongConfig,
    session, steadiness, steady_views,
    stream::BeatLog,
};

// The same watch, with an optional arbitrary exchange of Tick/Tock labels.
// Deliberately no jump metadata: display correctness must not require finding it.
fn log(swapped: bool) -> BeatLog {
    let beats = (0..4800)
        .map(|i| Beat {
            time: 0.05
                + (i + i64::from(swapped && i >= 2400)) as f64 * 0.125
                + if i % 2 == 0 { 0.0003 } else { 0.0 },
            index: i + i64::from(swapped && i >= 2400),
            quality: 1.0,
        })
        .collect();
    let amplitude_windows = (0..300)
        .map(|i| {
            let sign = if swapped && i >= 150 { -1.0 } else { 1.0 };
            AmplitudeWindow {
                start_s: i as f64 * 2.0,
                end_s: (i + 1) as f64 * 2.0,
                even_deg: Some(270.0),
                odd_deg: Some(270.0),
                beat_error_ms: Some(sign * 0.3),
                beat_error_unlock_ms: Some(sign * 0.5),
                even_unlock_ms: None,
                odd_unlock_ms: None,
                even_drop_ms: None,
                odd_drop_ms: None,
            }
        })
        .collect();
    BeatLog {
        sample_rate: 48000,
        duration_s: 600.125,
        bph: 28800,
        lift_deg: 52.0,
        beats,
        amplitude_windows,
        clipped_beats: vec![],
        clipped_samples: 0,
        jumps: vec![],
        sided: vec![],
        by_count: vec![],
        snippets: vec![],
    }
}

#[test]
fn local_sign_changes_do_not_cancel_live_or_whole_take_readings() {
    let log = log(true);
    let m = session::measure(&log, None, 0.0, 600.0);
    assert_eq!(
        m.beat_error_unlock_ms,
        Some(0.0),
        "signed diagnostics remain signed"
    );
    assert_eq!(m.beat_error(), Some(0.5));
    assert_eq!(m.beat_error_magnitudes.drop_ms, Some(0.3));
    assert_eq!(m.confidence.unwrap().beat_error.se, Some(0.0));
    let live = LiveAnalyzer::from_log(log, LiveConfig::default());
    // Include both complete halves of the synthetic, noiseless beat train.
    let both = live.reading_at(live.duration_s(), live.duration_s());
    assert_eq!(both.beat_error_unlock_ms, Some(0.0));
    assert_eq!(both.beat_error_magnitudes.unlock_ms, Some(0.5));
    assert_eq!(both.beat_error_unlock_error_ms, Some(0.0));
    let after = live.reading_at(400.0, 10.0);
    assert!(
        after.beat_error_ms.unwrap() < 0.0,
        "profile alignment keeps its sign"
    );
    assert_eq!(after.beat_error_magnitudes.drop_ms, Some(0.3));
}

#[test]
fn masks_and_breaks_exclude_windows_before_magnitudes_are_combined() {
    let mut log = log(true);
    for w in &mut log.amplitude_windows {
        if w.start_s >= 280.0 && w.start_s < 320.0 {
            w.beat_error_unlock_ms = Some(99.0);
        }
    }
    let mask = [(280.0, 320.0)];
    // Leave complete 10 s windows clear of the mask on each side.
    let m = session::measure_masked(&log, None, &[], &mask, 260.0, 340.0);
    assert_eq!(m.beat_error(), Some(0.5));
    assert_eq!(m.confidence.unwrap().beat_error.se, Some(0.0));
    let mut live = LiveAnalyzer::from_log(log.clone(), LiveConfig::default());
    live.set_masked(&mask);
    assert!(!live.reading_at(310.0, 10.0).whole);
    assert_eq!(
        live.reading_at(310.0, 10.0).beat_error_magnitudes.unlock_ms,
        None
    );
    assert_eq!(
        live.reading_at(330.0, 10.0).beat_error_magnitudes.unlock_ms,
        Some(0.5)
    );
    let breaks = [Break {
        audio_s: 300.5,
        gap_s: 0.0,
    }];
    let be = &steady_views::grids_with_breaks(&log, 10.0, &breaks)[2];
    assert!(
        be.y[150].is_nan(),
        "a 2 s window spanning a break stays missing"
    );
}

#[test]
fn downstream_series_and_spectra_analyse_magnitudes_before_rebinning() {
    let lc = LongConfig::default();
    let base = log(false);
    let swapped = log(true);
    let a = longrun::analyse(&base, None, &lc);
    let b = longrun::analyse(&swapped, None, &lc);
    assert_eq!(a.beat_error_magnitudes, b.beat_error_magnitudes);
    assert!(b
        .slices
        .iter()
        .all(|s| s.beat_error_magnitudes.unlock_ms == Some(0.5)));
    let ga = steady_views::grids(&base, 10.0);
    let gb = steady_views::grids(&swapped, 10.0);
    assert_eq!(ga[2].y, gb[2].y);
    assert!(gb[2].y.iter().all(|&v| v == 0.5));
    let cfg = steadiness::Config::default();
    let ra = steadiness::check(&base, None, &a, &lc, &cfg);
    let rb = steadiness::check(&swapped, None, &b, &lc, &cfg);
    // The full BE result includes its trend/change tests; a label exchange must
    // not create a level change, cycle, or distribution peak.
    assert_eq!(
        serde_json::to_value(&ra.series[2]).unwrap(),
        serde_json::to_value(&rb.series[2]).unwrap()
    );
    let spectrum = |r: &steadiness::Report| {
        serde_json::to_value(r.spectra.iter().find(|s| s.series == "beat_error")).unwrap()
    };
    assert_eq!(spectrum(&ra), spectrum(&rb));
}
