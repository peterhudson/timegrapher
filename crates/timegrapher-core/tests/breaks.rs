//! A take with a pause: two stretches of one watch joined with no audio
//! between them, while two minutes pass.

use timegrapher_core::audio::{write_wav, Audio};
use timegrapher_core::breaks;
use timegrapher_core::clock::ClockFit;
use timegrapher_core::longrun;
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::periodicity::standard_wheels;
use timegrapher_core::session;
use timegrapher_core::stream::{analyze_file, StreamConfig};
use timegrapher_core::synth::{generate, SynthConfig};

const GAP_S: f64 = 120.0;

/// The joined take on disk with its session.json, and the clock log's
/// entries (a card 20 ppm slow, logged every 5 s).
fn take(dir: &std::path::Path) -> (std::path::PathBuf, Vec<(f64, f64)>, f64) {
    let piece = |duration_s: f64, seed: u64| {
        generate(
            &SynthConfig {
                duration_s,
                rate_s_per_day: 10.0,
                beat_error_ms: 0.6,
                seed,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        )
    };
    // The first stretch ends partway through a beat period, so the second
    // starts out of phase with it.
    let a = piece(90.03, 1);
    let b = piece(90.0, 2);
    let fs = a.sample_rate;
    let join = a.samples.len();
    let mut samples = a.samples;
    samples.extend(b.samples);
    let path = dir.join("audio-001.wav");
    write_wav(
        &path,
        &Audio {
            samples,
            sample_rate: fs,
        },
    )
    .unwrap();
    std::fs::write(
        dir.join("session.json"),
        format!(
            r#"{{"segments": ["audio-001.wav"], "breaks": [{{"frame": {join}, "audio_s": {}, "utc": "2026-10-11T06:00:00.000Z", "gap_s": {GAP_S}}}]}}"#,
            join as f64 / fs as f64
        ),
    )
    .unwrap();
    let k = 1.0 + 20e-6;
    let join_s = join as f64 / fs as f64;
    let pairs = (0..=36)
        .map(|i| {
            let t = (i as f64 * 5.0).min(180.03);
            let t = if i == 18 { join_s } else { t };
            (t, 1.79e9 + t * k + if t >= join_s { GAP_S } else { 0.0 })
        })
        .collect();
    (path, pairs, join_s)
}

#[test]
fn a_take_with_a_pause_reads_as_one_watch() {
    let dir = std::env::temp_dir().join(format!("tg-breaks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (path, pairs, join_s) = take(&dir);
    let clock = ClockFit::new(&pairs).unwrap();
    assert!((clock.ppm - 20.0).abs() < 0.5, "clock {} ppm", clock.ppm);
    assert_eq!(clock.breaks.len(), 1, "{:?}", clock.breaks);

    let mut log = analyze_file(&path, &StreamConfig::default(), |_| {}).unwrap();
    let lc = LongConfig {
        wheels: standard_wheels(log.bph, 15),
        ..Default::default()
    };
    // Read straight through, the phase jump at the join spoils the rate.
    let naive = longrun::analyse(&log, None, &lc).overall.expect("fit");
    assert!(
        (naive.rate_s_per_day - 10.0).abs() > 1.0,
        "naive {}",
        naive.rate_s_per_day
    );
    let found = breaks::of_files(&[path.as_path()], Some(&clock), log.sample_rate);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(found.len(), 1);
    assert!((found[0].audio_s - join_s).abs() < 1e-6);
    assert!((found[0].gap_s - GAP_S).abs() < 1e-9);
    breaks::renumber(&mut log.beats, &mut log.amplitude_windows, &found, log.bph);

    // Rate on the card's clock is 10 s/d; corrected for 20 ppm slow, 8.3.
    let want = (1.0 + 10.0 / 86400.0) / (1.0 + 20e-6) * 86400.0 - 86400.0;
    let long = longrun::analyse_with_breaks(&log, Some(&clock), &found, &lc);
    let f = long.overall.expect("fit");
    assert!(
        (f.rate_s_per_day - want).abs() < 0.3,
        "long rate {}",
        f.rate_s_per_day
    );
    assert!(
        (f.beat_error_ms.abs() - 0.6).abs() < 0.1,
        "beat error {}",
        f.beat_error_ms
    );
    assert_eq!(long.breaks.len(), 1);
    assert!(
        (long.duration_s - (180.03 + GAP_S)).abs() < 1.0,
        "{}",
        long.duration_s
    );
    assert!(long.clean_fraction > 0.95, "clean {}", long.clean_fraction);

    let m = session::measure_with_breaks(&log, Some(&clock), &found, 0.0, log.duration_s);
    let r = m.rate_s_per_day.expect("rate");
    assert!((r - want).abs() < 0.3, "session rate {r}");
    assert_eq!(m.breaks.len(), 1);
    assert!(m.clean_fraction > 0.95, "clean {}", m.clean_fraction);
}

/// A hole in the sound with no break listed and no clock log: the beats
/// show the jump, the sides keep their names across it by their sound,
/// and the rate is fitted on each stretch. One of the two joins puts an
/// odd number of beats in the hole, so counting alone would swap the sides
/// there.
#[test]
fn a_hole_nobody_listed_keeps_the_sides_and_the_rate() {
    let piece = |duration_s: f64, seed: u64| {
        generate(
            &SynthConfig {
                duration_s,
                rate_s_per_day: 10.0,
                beat_error_ms: 0.6,
                // The Tock sounds unlike the Tick: a louder unlock.
                odd_gains: Some([0.9, 0.25, 1.0]),
                seed,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        )
    };
    for (n, first_s) in [90.03, 90.155].into_iter().enumerate() {
        let a = piece(first_s, 1);
        let b = piece(90.0, 2);
        let fs = a.sample_rate;
        let join_s = a.samples.len() as f64 / fs as f64;
        let mut samples = a.samples;
        samples.extend(b.samples);
        let dir = std::env::temp_dir().join(format!("tg-hole-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hole.wav");
        write_wav(
            &path,
            &Audio {
                samples,
                sample_rate: fs,
            },
        )
        .unwrap();
        let mut log = analyze_file(&path, &StreamConfig::default(), |_| {}).unwrap();
        let listed = breaks::of_files(&[path.as_path()], None, log.sample_rate);
        std::fs::remove_dir_all(&dir).ok();
        assert!(listed.is_empty());
        assert_eq!(log.jumps.len(), 1, "{first_s}: {:?}", log.jumps);
        // Found once the beats are found again: within the 2 s of beats the
        // tracker may miss before it takes what it finds, and a few beats.
        assert!(
            log.jumps[0] >= join_s && log.jumps[0] - join_s < 3.0,
            "{first_s}: jump at {:?}, join at {join_s}",
            log.jumps
        );
        let found = breaks::with_jumps(listed, &log.jumps);
        assert_eq!(found.len(), 1);

        // The same signed beat error on both sides of the hole.
        let side = |from: f64, to: f64| {
            let run: Vec<_> = log
                .beats
                .iter()
                .filter(|b| b.time >= from && b.time < to)
                .copied()
                .collect();
            timegrapher_core::timing::beat_error_ms(&run).expect("beat error")
        };
        let (before, after) = (side(5.0, join_s - 1.0), side(join_s + 5.0, f64::INFINITY));
        assert!(
            before.abs() > 0.4 && (before - after).abs() < 0.1,
            "{first_s}: beat error {before} then {after}"
        );

        breaks::renumber(&mut log.beats, &mut log.amplitude_windows, &found, log.bph);
        let m = session::measure_with_breaks(&log, None, &found, 0.0, log.duration_s);
        let r = m.rate_s_per_day.expect("rate");
        assert!((r - 10.0).abs() < 0.3, "{first_s}: session rate {r}");
        assert_eq!(m.breaks.len(), 1);
    }
}
