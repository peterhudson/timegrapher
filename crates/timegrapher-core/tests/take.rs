//! A take opened through [`timegrapher_core::take`] reads as `timegrapher
//! session` reads it, and putting it together another way does not.

use timegrapher_core::audio::{write_wav, Audio};
use timegrapher_core::breaks;
use timegrapher_core::clock::ClockFit;
use timegrapher_core::session;
use timegrapher_core::stream::{analyze_file, BeatLog, StreamConfig};
use timegrapher_core::synth::{generate, SynthConfig};
use timegrapher_core::take::{self, Listed};

const GAP_S: f64 = 120.0;

/// A take of one watch with a listed pause of [`GAP_S`] after `first_s`,
/// and a clock log of a card 20 ppm slow: its folder, sound file and log.
fn take_with_a_pause(name: &str, first_s: f64) -> (std::path::PathBuf, std::path::PathBuf, String) {
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
    let a = piece(first_s, 1);
    let b = piece(90.0, 2);
    let fs = a.sample_rate;
    let join = a.samples.len();
    let mut samples = a.samples;
    samples.extend(b.samples);
    let dir = std::env::temp_dir().join(format!("tg-take-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("audio-001.wav");
    write_wav(
        &path,
        &Audio {
            samples,
            sample_rate: fs,
        },
    )
    .unwrap();
    let join_s = join as f64 / fs as f64;
    std::fs::write(
        dir.join("session.json"),
        format!(
            r#"{{"segments": ["audio-001.wav"], "breaks": [{{"frame": {join}, "audio_s": {join_s}, "utc": "2026-10-11T06:00:00.000Z", "gap_s": {GAP_S}}}]}}"#
        ),
    )
    .unwrap();
    let k = 1.0 + 20e-6;
    let log: String = (0..=36)
        .map(|i| {
            let t = (i as f64 * 5.0).min(first_s + 90.0);
            let t = if i == 18 { join_s } else { t };
            format!(
                "{t},{}\n",
                1.79e9 + t * k + if t >= join_s { GAP_S } else { 0.0 }
            )
        })
        .collect();
    (dir, path, log)
}

/// `timegrapher session`'s own steps (session_cmd.rs), for comparison.
fn as_session_reads(
    log: &mut BeatLog,
    path: &std::path::Path,
    clock: &mut ClockFit,
) -> Vec<breaks::Break> {
    let listed = breaks::of_files_on_clock(&[path], Some(clock), log.sample_rate);
    let all = breaks::with_jumps(listed, &log.jumps);
    log.by_count = breaks::renumber(
        &mut log.beats,
        &mut log.amplitude_windows,
        &all,
        &log.sided,
        log.bph,
    );
    all
}

#[test]
fn a_take_opened_here_reads_as_session_reads_it() {
    for (n, first_s) in [90.03, 90.155, 90.4].into_iter().enumerate() {
        let (dir, path, text) = take_with_a_pause(&format!("same{n}"), first_s);
        let log = analyze_file(&path, &StreamConfig::default(), |_| {}).unwrap();
        let mk = || {
            take::clock(Some(&text), log.sample_rate, 2, None, None, log.duration_s)
                .unwrap()
                .unwrap()
        };

        let (mut a, mut clock_a) = (log.clone(), mk());
        let here = take::breaks(&mut a, Listed::Files(&[path.as_path()]), Some(&mut clock_a));
        let (mut b, mut clock_b) = (log.clone(), mk());
        let there = as_session_reads(&mut b, &path, &mut clock_b);
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(here, there);
        assert_eq!(a.by_count, b.by_count);
        let key = |l: &BeatLog| {
            l.beats
                .iter()
                .map(|x| (x.index, x.time.to_bits()))
                .collect::<Vec<_>>()
        };
        assert_eq!(key(&a), key(&b));
        let m = |l: &BeatLog, c: &ClockFit, br: &[breaks::Break]| {
            session::measure_masked(l, Some(c), br, &[], 0.0, l.duration_s)
        };
        let (x, y) = (m(&a, &clock_a, &here), m(&b, &clock_b, &there));
        assert_eq!(x.rate_s_per_day, y.rate_s_per_day);
        assert_eq!(x.beat_error_unlock_ms, y.beat_error_unlock_ms);
    }
}

/// The app opens a take with the jumps the beats show and no list of
/// breaks (app.rs analyse_whole and show_log; whole_take passes the jumps
/// as breaks of no length). On a take with a listed pause it keeps the
/// amplitude windows that straddle the pause, which `session` drops, so
/// the beat error it reads is not `session`'s.
#[test]
fn a_take_read_with_its_jumps_alone_keeps_windows_across_its_pause() {
    for (n, first_s) in [90.03, 90.155, 90.4].into_iter().enumerate() {
        let (dir, path, text) = take_with_a_pause(&format!("app{n}"), first_s);
        let log = analyze_file(&path, &StreamConfig::default(), |_| {}).unwrap();
        // The app's take clock: core clock::of_take, as app.rs reads it.
        let pairs = timegrapher_core::clock::parse_log(&text, log.sample_rate, 2).unwrap();
        let mut app_clock = ClockFit::for_rates(&pairs).unwrap();
        breaks::of_files_on_clock(&[path.as_path()], Some(&mut app_clock), log.sample_rate);
        let app_breaks = breaks::with_jumps(Vec::new(), &log.jumps);
        let app = session::measure_masked(
            &log,
            Some(&app_clock),
            &app_breaks,
            &[],
            0.0,
            log.duration_s,
        );

        let mut c = take::clock(Some(&text), log.sample_rate, 2, None, None, log.duration_s)
            .unwrap()
            .unwrap();
        let mut l = log.clone();
        let found = take::breaks(&mut l, Listed::Files(&[path.as_path()]), Some(&mut c));
        let cli = session::measure_masked(&l, Some(&c), &found, &[], 0.0, l.duration_s);
        std::fs::remove_dir_all(&dir).ok();

        let pause = found
            .iter()
            .find(|b| b.gap_s > 0.0)
            .expect("the listed pause")
            .audio_s;
        let across = |l: &BeatLog| {
            l.amplitude_windows
                .iter()
                .filter(|w| w.start_s < pause && pause < w.end_s)
                .count()
        };
        assert!(across(&log) > 0, "{first_s}: no window across the pause");
        assert_eq!(across(&l), 0, "{first_s}");
        // The rate agrees: the clock carries the pause either way.
        assert_eq!(app.rate_s_per_day, cli.rate_s_per_day, "{first_s}");
        assert_ne!(
            app.beat_error_unlock_ms, cli.beat_error_unlock_ms,
            "{first_s}"
        );
    }
}

/// An entry read through [`take::measure_entry`] reads as its steps taken
/// one by one: the clock from its log, its breaks, and the stretch after
/// it settles.
#[test]
fn an_entry_measured_here_reads_as_its_steps_by_hand() {
    let (dir, path, text) = take_with_a_pause("entry", 90.155);
    std::fs::write(dir.join("clock.csv"), &text).unwrap();
    let m: session::Manifest = toml::from_str(
        r#"
        settle_s = 15
        [[recording]]
        file = "audio-001.wav"
        clock = "clock.csv"
        position = "CH"
        "#,
    )
    .unwrap();
    let s = take::EntrySettings {
        stream: StreamConfig::default(),
        card_clock: false,
        settle_s: None,
    };
    let read = take::measure_entry(
        &dir,
        &m,
        &m.recordings[0],
        &s,
        |_| Ok(None),
        |_| panic!("an entry with a clock log asks for no correction"),
    )
    .unwrap();

    let mut log = analyze_file(&path, &StreamConfig::default(), |_| {}).unwrap();
    let mut clock = take::clock(Some(&text), log.sample_rate, 2, None, None, log.duration_s)
        .unwrap()
        .unwrap();
    let breaks = take::breaks(&mut log, Listed::Files(&[path.as_path()]), Some(&mut clock));
    let (from, to) = take::measured(0.0, log.duration_s, Some(15.0), None);
    let by_hand = session::measure_masked(&log, Some(&clock), &breaks, &[], from, to);
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(read.breaks, breaks);
    assert_eq!((read.settle_s, read.end_s), (from, to));
    let m = &read.measurement;
    assert_eq!(m.rate_s_per_day, by_hand.rate_s_per_day);
    assert_eq!(m.beat_error_unlock_ms, by_hand.beat_error_unlock_ms);
    assert_eq!(m.amplitude_deg, by_hand.amplitude_deg);
    assert!(m.rate_s_per_day.is_some());
}
