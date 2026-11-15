//! An unlisted loss of beats cuts a saved fit just as it cuts a live fit.
use timegrapher_core::{
    beats::Beat,
    clean::Cuts,
    clock::ClockFit,
    live::{LiveAnalyzer, LiveConfig},
    session,
    stream::BeatLog,
    timing,
};

#[test]
fn live_and_saved_share_the_strict_gap_boundary() {
    let cuts = Cuts::none();
    for gap in [1.999, 2.0, 2.001] {
        let beats = [0.0, gap].map(|time| Beat {
            index: 0,
            time,
            quality: 1.0,
        });
        let saved = cuts.fit_cuts(&beats);
        let live = cuts.run_cut(&beats, 0.0, 3.0);
        if gap > 2.0 {
            assert_eq!(saved, [live]);
            assert!(live > 0.0 && live < gap);
        } else {
            assert!(saved.is_empty());
            assert_eq!(live, f64::NEG_INFINITY);
        }
    }
    assert!(cuts.fit_cuts(&[]).is_empty());
}

fn run(start: f64, first: i64, seconds: usize, rate: f64, error_ms: f64) -> Vec<Beat> {
    let period = timing::period_s(0.125, rate);
    (0..seconds * 8)
        .map(|i| {
            let index = first + i as i64;
            Beat {
                index,
                time: start
                    + i as f64 * period
                    + if index % 2 == 0 {
                        error_ms / 2000.0
                    } else {
                        -error_ms / 2000.0
                    },
                quality: 1.0,
            }
        })
        .collect()
}

fn log(beats: Vec<Beat>) -> BeatLog {
    BeatLog {
        lock_template_kept: false,
        sample_rate: 48_000,
        duration_s: beats.last().unwrap().time + 0.125,
        bph: 28_800,
        lift_deg: 52.0,
        beats,
        amplitude_windows: vec![],
        clipped_beats: vec![],
        clipped_samples: 0,
        jumps: vec![],
        sided: vec![],
        by_count: vec![],
        snippets: vec![],
    }
}

#[test]
fn saved_fit_pools_unlisted_runs_without_fitting_across_the_gap() {
    let first = run(0.0, 0, 30, 8.0, 0.4);
    let second = run(38.09, 240, 45, -12.0, -0.4);
    let a = timing::fit(&first, 28_800).unwrap();
    let b = timing::fit(&second, 28_800).unwrap();
    let n = (a.beats_used + b.beats_used) as f64;
    let expected =
        (a.rate_s_per_day * a.beats_used as f64 + b.rate_s_per_day * b.beats_used as f64) / n;
    let log = log([first, second].concat());
    let reading = session::measure(&log, None, 0.0, log.duration_s);
    assert!(
        (reading.rate_s_per_day.unwrap() - expected).abs() < 1e-6,
        "{reading:?}; expected {expected}"
    );
    assert!(
        (reading.beat_error_ms.unwrap() - 0.4).abs() < 1e-6,
        "{reading:?}"
    );
    assert!(reading
        .caveats
        .contains(&timegrapher_core::measure::Caveat::BeatErrorAsSize));
    let live = LiveAnalyzer::from_log(log.clone(), LiveConfig::default()).reading(120.0);
    let last = session::measure(&log, None, 38.0, log.duration_s);
    assert!((live.rate_s_per_day.unwrap() - last.rate_s_per_day.unwrap()).abs() < 1e-6);
}

#[test]
fn a_short_isolated_tail_does_not_bias_the_saved_rate() {
    let log = log([run(0.0, 0, 30, 8.0, 0.4), run(38.09, 240, 5, -200.0, 0.4)].concat());
    let reading = session::measure(&log, None, 0.0, log.duration_s);
    assert!(
        (reading.rate_s_per_day.unwrap() - 8.0).abs() < 1e-6,
        "{reading:?}"
    );
}

#[test]
fn unlisted_gaps_are_cut_on_the_readings_clock() {
    let log = log([run(0.0, 0, 30, 8.0, 0.4), run(38.09, 240, 30, 8.0, 0.4)].concat());
    let clock = ClockFit::fixed(50.0, log.duration_s);
    let first = session::measure(&log, Some(&clock), 0.0, 30.0);
    let whole = session::measure(&log, Some(&clock), 0.0, log.duration_s);
    assert!(
        (first.rate_s_per_day.unwrap() - whole.rate_s_per_day.unwrap()).abs() < 1e-6,
        "{whole:?}"
    );
    // A mask and a listed pause still use their original boundaries.
    let gaps = [timegrapher_core::breaks::Break {
        audio_s: 38.09,
        gap_s: 60.0,
    }];
    let masked = session::measure_masked(
        &log,
        Some(&clock),
        &gaps,
        &[(12.0, 15.0)],
        0.0,
        log.duration_s,
    );
    assert!((first.rate_s_per_day.unwrap() - masked.rate_s_per_day.unwrap()).abs() < 1e-6);
}
