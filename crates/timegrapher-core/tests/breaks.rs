//! A take with a pause: two stretches of one watch joined with no audio
//! between them, while two minutes pass.

use timegrapher_core::audio::{write_wav, Audio};
use timegrapher_core::breaks;
use timegrapher_core::clock::ClockFit;
use timegrapher_core::longrun;
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::measure::Caveat;
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
    breaks::renumber(
        &mut log.beats,
        &mut log.amplitude_windows,
        &found,
        &log.sided,
        log.bph,
    );

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

        breaks::renumber(
            &mut log.beats,
            &mut log.amplitude_windows,
            &found,
            &log.sided,
            log.bph,
        );
        let m = session::measure_with_breaks(&log, None, &found, 0.0, log.duration_s);
        let r = m.rate_s_per_day.expect("rate");
        assert!((r - 10.0).abs() < 0.3, "{first_s}: session rate {r}");
        assert_eq!(m.breaks.len(), 1);
        // One sign either side: the beat error stays signed.
        assert!(
            !m.caveats.contains(&Caveat::BeatErrorAsSize),
            "{:?}",
            m.caveats
        );
    }
}

#[test]
fn the_clock_takes_the_listed_break() {
    // Logged as the recorder does: the first entry after the resume comes
    // a block (here 1 s) after the listed frame, so the log alone puts the
    // gap there and the audio in between reads as before it.
    let dir = std::env::temp_dir().join(format!("tg-breaks-clock-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (path, mut pairs, join_s) = take(&dir);
    let i = pairs.iter().position(|q| q.0 == join_s).unwrap();
    pairs[i].0 += 1.0;
    pairs[i].1 += 1.0 * (1.0 + 20e-6);
    let mut clock = ClockFit::new(&pairs).unwrap();
    assert!((clock.breaks[0].audio_s - (join_s + 1.0)).abs() < 1e-6);
    let before = clock.map(join_s + 0.5);
    let found = breaks::of_files_on_clock(&[path.as_path()], Some(&mut clock), 48000);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(found.len(), 1);
    assert!(
        (clock.breaks[0].audio_s - join_s).abs() < 1e-6,
        "{:?}",
        clock.breaks
    );
    let after = clock.map(join_s + 0.5);
    assert!((after - before - GAP_S).abs() < 0.01, "{before} {after}");
    assert!((clock.map(join_s - 0.5) - (join_s - 0.5) * (1.0 + 20e-6)).abs() < 0.01);
}

#[test]
fn a_real_change_of_beat_error_sign_keeps_its_sides() {
    // Two stretches joined out of phase, the beat error -0.6 ms before
    // the join and +0.6 ms after. The Tock sounds unlike the Tick, so the
    // side sounds tell the sides apart at the jump, and renumbering keeps
    // the change of sign rather than turning the second stretch over.
    let piece = |duration_s: f64, beat_error_ms: f64, seed: u64| {
        generate(
            &SynthConfig {
                duration_s,
                rate_s_per_day: 10.0,
                beat_error_ms,
                odd_gains: Some([0.9, 0.25, 1.0]),
                seed,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        )
    };
    let a = piece(90.03, -0.6, 1);
    let b = piece(90.0, 0.6, 2);
    let fs = a.sample_rate;
    let join_s = a.samples.len() as f64 / fs as f64;
    let mut samples = a.samples;
    samples.extend(b.samples);
    let mut log = timegrapher_core::stream::analyze_samples(&samples, fs, &StreamConfig::default());
    assert_eq!(log.jumps.len(), 1, "{:?}", log.jumps);
    assert_eq!(log.sided.len(), 1, "{:?}", log.sided);
    let found = breaks::with_jumps(Vec::new(), &log.jumps);
    breaks::renumber(
        &mut log.beats,
        &mut log.amplitude_windows,
        &found,
        &log.sided,
        log.bph,
    );
    let be = |from: f64, to: f64| {
        let run: Vec<_> = log
            .beats
            .iter()
            .filter(|b| b.time >= from && b.time < to)
            .copied()
            .collect();
        timegrapher_core::timing::beat_error_ms(&run).unwrap()
    };
    let (before, after) = (be(5.0, join_s - 1.0), be(join_s + 5.0, 175.0));
    // The sides keep their names, so the sign turns over at the join.
    assert!(
        before * after < 0.0 && before.abs() > 0.4 && after.abs() > 0.4,
        "{before} {after}"
    );
}

#[test]
fn a_negative_pause_moves_no_beat_numbers_back() {
    // A pause the recorder logged as -0.2 s counts as none: the beats
    // after it number on from those before, as with no gap at all.
    let period = 3600.0 / 28800.0;
    let mut beats: Vec<timegrapher_core::beats::Beat> = (0..960)
        .map(|k| timegrapher_core::beats::Beat {
            index: k,
            time: k as f64 * period,
            quality: 1.0,
        })
        .collect();
    let gap = |gap_s: f64| breaks::Break {
        audio_s: 60.0,
        gap_s,
    };
    let mut zero = beats.clone();
    breaks::renumber(&mut zero, &mut Vec::new(), &[gap(0.0)], &[], 28800);
    breaks::renumber(&mut beats, &mut Vec::new(), &[gap(-0.2)], &[], 28800);
    let idx = |v: &[timegrapher_core::beats::Beat]| v.iter().map(|b| b.index).collect::<Vec<_>>();
    assert_eq!(idx(&beats), idx(&zero));
    assert_eq!(beats[480].index, 480);
    assert_eq!(breaks::wall(&[gap(-0.2)], 70.0), 70.0);
}

#[test]
fn sides_too_alike_to_tell_keep_the_beat_errors_sign() {
    // As above, but Tick and Tock sound alike, so the side sounds can't
    // tell them apart at the jump. The beat error is 0.6 ms either side of
    // it, clearly away from zero, so its sign names the sides: a turn of
    // sign at such a hole reads as the sides swapped by the count, and the
    // beat error keeps its sign across it (the project's rule, 2026-10-27).
    let piece = |duration_s: f64, beat_error_ms: f64, seed: u64| {
        generate(
            &SynthConfig {
                duration_s,
                rate_s_per_day: 10.0,
                beat_error_ms,
                seed,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        )
    };
    let a = piece(90.03, -0.6, 1);
    let b = piece(90.0, 0.6, 2);
    let fs = a.sample_rate;
    let join_s = a.samples.len() as f64 / fs as f64;
    let mut samples = a.samples;
    samples.extend(b.samples);
    let mut log = timegrapher_core::stream::analyze_samples(&samples, fs, &StreamConfig::default());
    assert_eq!(log.jumps.len(), 1, "{:?}", log.jumps);
    assert_eq!(log.sided, log.jumps);
    let found = breaks::with_jumps(Vec::new(), &log.jumps);
    log.by_count = breaks::renumber(
        &mut log.beats,
        &mut log.amplitude_windows,
        &found,
        &log.sided,
        log.bph,
    );
    assert!(log.by_count.is_empty(), "{:?}", log.by_count);
    let be = |from: f64, to: f64| {
        let run: Vec<_> = log
            .beats
            .iter()
            .filter(|b| b.time >= from && b.time < to)
            .copied()
            .collect();
        timegrapher_core::timing::beat_error_ms(&run).unwrap()
    };
    let (before, after) = (be(5.0, join_s - 1.0), be(join_s + 5.0, 175.0));
    assert!(
        before * after > 0.0 && before.abs() > 0.4 && after.abs() > 0.4,
        "{before} {after}"
    );
    let m = session::measure_with_breaks(&log, None, &found, 0.0, log.duration_s);
    assert!(
        !m.caveats
            .iter()
            .any(|c| matches!(c, Caveat::SidesByCount { .. })),
        "{:?}",
        m.caveats
    );
}

#[test]
fn sides_too_alike_to_tell_near_no_beat_error_keep_the_count_and_say_so() {
    // As above, but the beat error is near zero, too small for its sign to
    // name the sides: the beat count carries them across the jump, and the
    // reading says the sides there were named by the count.
    let piece = |duration_s: f64, beat_error_ms: f64, seed: u64| {
        generate(
            &SynthConfig {
                duration_s,
                rate_s_per_day: 10.0,
                beat_error_ms,
                seed,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        )
    };
    let a = piece(90.03, -0.03, 1);
    let b = piece(90.0, 0.03, 2);
    let fs = a.sample_rate;
    let mut samples = a.samples;
    samples.extend(b.samples);
    let mut log = timegrapher_core::stream::analyze_samples(&samples, fs, &StreamConfig::default());
    assert_eq!(log.jumps.len(), 1, "{:?}", log.jumps);
    assert!(log.sided.is_empty(), "{:?}", log.sided);
    let found = breaks::with_jumps(Vec::new(), &log.jumps);
    log.by_count = breaks::renumber(
        &mut log.beats,
        &mut log.amplitude_windows,
        &found,
        &log.sided,
        log.bph,
    );
    assert_eq!(log.by_count, log.jumps);
    let m = session::measure_with_breaks(&log, None, &found, 0.0, log.duration_s);
    assert!(
        m.caveats.contains(&Caveat::SidesByCount { count: 1 }),
        "{:?}",
        m.caveats
    );
}

#[test]
fn a_reading_across_a_turn_of_sign_gives_the_size_either_side() {
    // The beat error turns from -0.6 to +0.3 ms at a hole whose sides the
    // sound can't tell apart, and its size changes there too, so neither
    // the sound nor the sign names the sides and the count keeps the turn:
    // a reading across the hole gives the size, about 0.45 ms, rather than
    // the near-zero mean of the two signs.
    let piece = |duration_s: f64, beat_error_ms: f64, seed: u64| {
        generate(
            &SynthConfig {
                duration_s,
                rate_s_per_day: 10.0,
                beat_error_ms,
                seed,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        )
    };
    let a = piece(90.03, -0.6, 1);
    let b = piece(90.0, 0.3, 2);
    let fs = a.sample_rate;
    let mut samples = a.samples;
    samples.extend(b.samples);
    let mut log = timegrapher_core::stream::analyze_samples(&samples, fs, &StreamConfig::default());
    let found = breaks::with_jumps(Vec::new(), &log.jumps);
    log.by_count = breaks::renumber(
        &mut log.beats,
        &mut log.amplitude_windows,
        &found,
        &log.sided,
        log.bph,
    );
    let m = session::measure_with_breaks(&log, None, &found, 0.0, log.duration_s);
    let drop = m.beat_error_ms.expect("beat error");
    let unlock = m.beat_error_unlock_ms.expect("unlock beat error");
    assert!((drop - 0.45).abs() < 0.1, "drop {drop}");
    assert!((unlock - 0.45).abs() < 0.15, "unlock {unlock}");
    assert!(
        m.caveats.contains(&Caveat::BeatErrorAsSize),
        "{:?}",
        m.caveats
    );
}

#[test]
fn the_drop_and_the_unlock_keep_one_sign_near_a_whole_stretch() {
    // A long stretch at one sign, a hole, and a short one at the other,
    // near the length a stretch needs to be fitted on its own: a tail of
    // 12.5 to 13.5 s (about 10.4 to 11.4 s of beats, but only 8 s of 2 s
    // windows) and a head of 9 to 10 s (under 10 s of beats, but 10 s of
    // windows), both ways round. The drop's and the unlock's beat errors
    // decide together whether the sign turns over, so in `session` and in
    // `long` they never come out with opposite signs.
    let piece = |duration_s: f64, beat_error_ms: f64, seed: u64| {
        generate(
            &SynthConfig {
                duration_s,
                rate_s_per_day: 10.0,
                beat_error_ms,
                seed,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        )
    };
    let lc = LongConfig::default();
    for (first_s, then_s) in [
        (90.03, 6.0),
        (90.03, 12.5),
        (90.03, 13.0),
        (90.03, 13.5),
        (9.03, 90.0),
        (9.53, 90.0),
        (10.03, 90.0),
    ] {
        for sign in [1.0, -1.0] {
            let a = piece(first_s, 0.6 * sign, 1);
            let b = piece(then_s, -0.6 * sign, 2);
            let fs = a.sample_rate;
            let mut samples = a.samples;
            samples.extend(b.samples);
            let mut log =
                timegrapher_core::stream::analyze_samples(&samples, fs, &StreamConfig::default());
            let found = breaks::with_jumps(Vec::new(), &log.jumps);
            log.by_count = breaks::renumber(
                &mut log.beats,
                &mut log.amplitude_windows,
                &found,
                &log.sided,
                log.bph,
            );
            let m = session::measure_with_breaks(&log, None, &found, 0.0, log.duration_s);
            let long = longrun::analyse_with_breaks(&log, None, &found, &lc);
            for (what, drop, unlock) in [
                ("session", m.beat_error_ms, m.beat_error_unlock_ms),
                (
                    "long",
                    long.overall.map(|f| f.beat_error_ms),
                    long.beat_error_unlock_ms,
                ),
            ] {
                let (drop, unlock) = (drop.expect("beat error"), unlock.expect("unlock"));
                assert!(
                    drop * unlock > 0.0 && drop.abs() > 0.4 && unlock.abs() > 0.4,
                    "{what}, {first_s} s then {then_s} s, sign {sign}: drop {drop}, unlock {unlock}"
                );
            }
        }
    }
}

#[test]
fn a_clock_step_no_listed_pause_explains_is_a_break_too() {
    // The take lists its pause, and the clock log also steps 2 s at 140 s
    // with no pause listed there (a system clock step): the breaks are the
    // listed one and that step, as the step alone would be with nothing
    // listed.
    let dir = std::env::temp_dir().join(format!("tg-breaks-step-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (path, mut pairs, join_s) = take(&dir);
    for q in pairs.iter_mut().filter(|q| q.0 >= 140.0) {
        q.1 += 2.0;
    }
    let mut clock = ClockFit::new(&pairs).unwrap();
    assert_eq!(clock.breaks.len(), 2, "{:?}", clock.breaks);
    let found = breaks::of_files_on_clock(&[path.as_path()], Some(&mut clock), 48000);
    std::fs::remove_dir_all(&dir).ok();
    let at: Vec<f64> = found.iter().map(|b| b.audio_s).collect();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        (at[0] - join_s).abs() < 1e-6 && (at[1] - 140.0).abs() < 1e-6,
        "{found:?}"
    );
    assert!((found[1].gap_s - 2.0).abs() < 0.01, "{found:?}");
}
