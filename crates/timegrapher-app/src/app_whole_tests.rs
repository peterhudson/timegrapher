//! The whole take's reading, headless: it is session's reading of the
//! same stretch, at any length session reads.

use super::readouts_tests::{cards_text, painted, stopped_listened};
use super::tests::{painted_text, synthetic, themed};
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
            lock_template_kept: false,
            sample_rate: 48_000,
            duration_s: 120.0,
            bph: 28_800,
            lift_deg: 52.0,
            escapement: Default::default(),
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
        app.bench.recording.live = Some(LiveAnalyzer::from_log(log, LiveConfig::default()));
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
    app.bench.take.average_s = 60.0;
    let long = app.whole_take().expect("60 s at an Average Over of 60 s");
    app.bench.take.average_s = 10.0;
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
    let log = steady::log_of(app.bench.recording.live.as_ref().unwrap()).unwrap();
    let spans = app.current_spans();
    // Just listened to, the take reads to its last reading with a
    // watch heard, never past its end.
    let to = app
        .last_heard_s()
        .min(app.bench.recording.live.as_ref().unwrap().duration_s());
    let from = spans[0].0.max(0.0);
    let m = timegrapher_core::session::measure(&log, None, from, to);
    assert_eq!(w.m.rate_s_per_day, m.rate_s_per_day);
    assert_eq!(w.m.amplitude_deg, m.amplitude_deg);
    assert!((w.span - (to - from)).abs() < 1e-9, "{}", w.span);
}

/// A take whose breaks are not all its own list's says so, in core's
/// words, on the cards and beside the whole take's breaks, as session
/// says it.
#[test]
fn a_take_whose_breaks_are_not_its_own_lists_says_so() {
    use timegrapher_core::breaks::Unlisted;
    let mut app = synthetic(60.0, 12.0);
    // Opened as a take is, its beats read to its end: an opened take's
    // cards read to its end (#2073), and the engine listened to here had
    // not yet read the last of its sound.
    let mut log = steady::log_of(app.bench.recording.live.as_ref().unwrap()).unwrap();
    log.duration_s = log.beats.last().unwrap().time;
    app.bench.recording.live = Some(LiveAnalyzer::from_log(log, LiveConfig::default()));
    app.put_take_breaks(Some(Vec::new()), Some(Unlisted::Unreadable));
    let w = app.whole_take().expect("whole take");
    assert!(
        w.m.caveats
            .iter()
            .any(|c| matches!(c, Caveat::BreaksUnlisted { .. })),
        "{:?}",
        w.m.caveats
    );
    assert!(
        whole_breaks_text(&w).contains("session.json could not be read"),
        "{}",
        whole_breaks_text(&w)
    );
    let ctx = themed();
    let text = painted_text(&ctx.run(Default::default(), |ctx| app.panels(ctx)));
    assert!(
        text.to_lowercase().contains("breaks from the clock log"),
        "{text}"
    );
    // A take with its own list says nothing of it.
    app.put_take_breaks(Some(Vec::new()), None);
    let w = app.whole_take().expect("whole take");
    assert!(!whole_breaks_text(&w).contains("session.json"));
}

/// A hand mask across the end of the settle, core's edge take
/// `edge_mask_across_settle`, reads as session reads it: masked, and no
/// break where the mask's ends cut the beats (#682).
#[test]
fn a_mask_across_the_settle_is_no_break() {
    let root = std::env::temp_dir().join(format!("tg-app-mask-settle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let takes = timegrapher_core::edge_takes::write_all(&root).unwrap();
    let take = takes
        .iter()
        .find(|t| t.id == "edge_mask_across_settle")
        .expect("the edge take");
    let app = super::tests::reopened(&root.join("takes").join(take.id));
    let w = app.whole_take().expect("whole take");
    let codes = timegrapher_core::edge_takes::caveat_codes(&w.m);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(codes, ["masked"], "{codes:?}");
}

/// A take whose session file gives the default lift with no `lift_known`,
/// as older files do, reads it as assumed, and the whole take says so as
/// session's reading does; with the marker it says nothing of it (#685).
#[test]
fn an_older_takes_default_lift_is_named_assumed_on_the_whole_take() {
    let root = std::env::temp_dir().join(format!("tg-app-bare-lift-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let takes = timegrapher_core::edge_takes::write_all(&root).unwrap();
    let take = takes
        .iter()
        .find(|t| t.id == "edge_mask_across_settle")
        .expect("the edge take");
    let dir = root.join("takes").join(take.id);
    let codes = || {
        let app = super::tests::reopened(&dir);
        timegrapher_core::edge_takes::caveat_codes(&app.whole_take().expect("whole take").m)
    };
    let marked = codes();
    let file = dir.join("session.toml");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("lift = 52\nlift_known = true\n"), "{text}");
    std::fs::write(&file, text.replace("lift_known = true\n", "")).unwrap();
    let bare = codes();
    let _ = std::fs::remove_dir_all(&root);
    assert!(!marked.iter().any(|c| c == "lift_unknown"), "{marked:?}");
    assert!(bare.iter().any(|c| c == "lift_unknown"), "{bare:?}");
}

/// The whole take's hover says the breaks it is read across and, where
/// Tick and Tock were named by the count across them, that too.
#[test]
fn the_whole_takes_hover_says_its_sides_were_named_by_count() {
    let text = cut_caveats_text(&[
        Caveat::Breaks { count: 1 },
        Caveat::SidesByCount { count: 1 },
        Caveat::ClockUnknown,
    ]);
    assert!(text.starts_with(" 1 break in this stretch: "), "{text}");
    assert!(
        text.ends_with(
            " Tick and Tock are named by the beat count across 1 break in this \
             stretch, not told apart by their sound."
        ),
        "{text}"
    );
    assert_eq!(cut_caveats_text(&[Caveat::ClockUnknown]), "");
}

/// The whole take reads the settled stretches of the position it is in
/// together, as a Positions Test table reads a position's (core's
/// `session::measure_together`): a mask over the time between two of them
/// is masked time once, not again as the time between them.
#[test]
fn a_mask_between_two_stretches_counts_once_on_the_whole_take() {
    let base = std::env::temp_dir().join(format!("tg-app-whole-mask-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(
        base.join(timegrapher_core::take::MANIFEST),
        "lift = 52.0\nsettle_s = 5.0\n\n\
         [[recording]]\nfile = \"take.wav\"\nposition = \"CU\"\nend_s = 20.0\n\n\
         [[recording]]\nfile = \"take.wav\"\nposition = \"CD\"\nstart_s = 20.0\nend_s = 30.0\n\n\
         [[recording]]\nfile = \"take.wav\"\nposition = \"CU\"\nstart_s = 30.0\n\
         masked = [[15.0, 40.0]]\n",
    )
    .unwrap();
    let app = super::tests::opened_recording(&base, "take.wav", 60.0);
    let _ = std::fs::remove_dir_all(&base);
    let spans: Vec<(f64, f64)> = app
        .current_spans()
        .into_iter()
        .map(|(a, b)| (a, b.min(60.0)))
        .collect();
    assert_eq!(spans, [(5.0, 20.0), (35.0, 60.0)]);
    let w = app.whole_take().expect("whole take");
    // 15 s to 40 s: the 15 s between the stretches is in it.
    assert!((w.m.masked_s - 25.0).abs() < 1e-9, "{}", w.m.masked_s);
    // Its caveat says only what was masked by hand in CU: 15 s to 20 s
    // and 35 s to 40 s (#1228).
    assert!(
        w.m.caveats.contains(&Caveat::Masked { seconds: 10.0 }),
        "{:?}",
        w.m.caveats
    );
    // Its hover's "over" is the stretch read less what that left out
    // (core's `Measurement::measured_s`): 5 s to 60 s less the 25 s
    // masked, not the stretches' 40 s with the hand's 10 s in them (#2177).
    assert!((w.span - 30.0).abs() < 1e-9, "{}", w.span);
    assert_eq!(w.span, w.m.measured_s());
}

/// A take kept as several files, its last position listed as more than
/// one of them: its Watches row (core `library::reading`) and the Bench's
/// whole take once it is opened give one rate, amplitude and beat error,
/// with the same caveats, to the last bit (#2121, Trevor): the row reads
/// the whole position as the Bench does. Two files in Dial Up, and Dial
/// Up, Dial Down, Dial Up, where the position's stretches are apart.
#[test]
fn a_takes_row_reads_its_last_position_as_the_bench_does() {
    use timegrapher_core::synth::{generate, SynthConfig};
    let cases: [&[(&str, f64)]; 2] = [
        &[("DU", 40.0), ("DU", 0.0)],
        &[("DU", 0.0), ("DD", 200.0), ("DU", 6.0)],
    ];
    let part_s = 60.0;
    let mut wrong = Vec::new();
    for (k, parts) in cases.iter().enumerate() {
        let dir = std::env::temp_dir().join(format!("tg-row-bench-{k}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rates: Vec<f64> = parts.iter().map(|p| p.1).collect();
        let n = rates.len();
        let audio = generate(
            &SynthConfig {
                duration_s: part_s * n as f64,
                rate_s_per_day: 0.0,
                ..Default::default()
            },
            |_| 270.0,
            |t| rates[((t / part_s) as usize).min(n - 1)],
        );
        let per = (part_s * f64::from(audio.sample_rate)) as usize;
        let mut session = "bph = 28800\nlift = 52\nlift_known = true\n".to_string();
        for (i, (position, _)) in parts.iter().enumerate() {
            let name = format!("audio-{:03}.wav", i + 1);
            let end = ((i + 1) * per).min(audio.samples.len());
            let part = timegrapher_core::audio::Audio {
                samples: audio.samples[i * per..end].to_vec(),
                sample_rate: audio.sample_rate,
            };
            timegrapher_core::audio::write_wav(&dir.join(&name), &part).unwrap();
            session.push_str(&format!(
                "\n[[recording]]\nfile = '{name}'\nposition = '{position}'\n"
            ));
        }
        std::fs::write(dir.join("session.toml"), session).unwrap();
        let row = timegrapher_core::library::reading(&dir).unwrap();
        let app = super::tests::reopened(&dir);
        let w = app
            .whole_take()
            .expect("the whole take in the last position");
        let _ = std::fs::remove_dir_all(&dir);
        let bench = (
            w.m.rate_s_per_day,
            w.m.amplitude_deg,
            w.m.beat_error_signed(),
            &w.m.caveats,
        );
        let kept = (
            row.rate_s_per_day,
            row.amplitude_deg,
            row.beat_error_ms,
            &row.caveats,
        );
        if bench != kept {
            wrong.push(format!("{parts:?}: row {kept:?}, Bench {bench:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A watch beating 17,570 bph on Auto, opened as File > Open opens it
/// (#2327): Auto's lock is off every standard rate, so the whole take,
/// the cards at its end, the charts' points and its Positions Test table
/// give no rate in s/d and name the caveat `off_standard_rate`, as
/// `analyze` and a saved take's reading do (#2253).
#[test]
fn an_off_standard_watch_opened_on_auto_has_no_rate_in_s_per_day() {
    use timegrapher_core::measure::off_standard;
    use timegrapher_core::synth::{generate, SynthConfig};
    let base = std::env::temp_dir().join(format!("tg-off-standard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let wav = base.join("off.wav");
    let cfg = SynthConfig {
        bph: 17_570,
        duration_s: 60.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    timegrapher_core::audio::write_wav(&wav, &audio).unwrap();
    // At a set rate: the take with its session file naming 18,000.
    let take = base.join("set");
    std::fs::create_dir_all(&take).unwrap();
    timegrapher_core::audio::write_wav(&take.join("off.wav"), &audio).unwrap();
    std::fs::write(
        take.join("session.toml"),
        "bph = 18000\n\n[[recording]]\nfile = 'off.wav'\nposition = 'DU'\n",
    )
    .unwrap();
    let opened = |bph: Option<u32>| {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.open_recording(if bph.is_some() { &take } else { &wav }, false);
        let t = std::time::Instant::now();
        while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 120 {
            app.poll();
        }
        assert!(app.bench.recording.batch.is_none(), "{:?}", app.message);
        app
    };
    // Set to 18,000, the whole take, the cards and the table read the
    // rate against it, with no caveat.
    let mut set = opened(Some(18_000));
    let w = set.whole_take().expect("the whole take at 18,000");
    let rate =
        w.m.rate_s_per_day
            .unwrap_or_else(|| panic!("{:?}", w.m.caveats));
    assert!((rate + 2059.1).abs() < 1.0, "{rate}");
    assert!(off_standard(&w.m.caveats).is_none(), "{:?}", w.m.caveats);
    set.bench.take.average_s = 10.0;
    assert!(set.reading().and_then(|r| r.rate_s_per_day).is_some());
    let t = set.free_table(&egui::Context::default()).expect("a table");
    assert!(t.mean[0].is_some(), "{t:?}");
    assert_eq!(set.take_manifest().bph, Some(18_000));
    let mut app = opened(None);
    let _ = std::fs::remove_dir_all(&base);
    let w = app.whole_take().expect("the whole take");
    assert_eq!(w.m.rate_s_per_day, None, "{:?}", w.m.caveats);
    let o = off_standard(&w.m.caveats).unwrap_or_else(|| panic!("{:?}", w.m.caveats));
    assert!((o.bph_measured - 17_570.0).abs() < 2.0, "{o:?}");
    for average in [10.0, 60.0] {
        app.bench.take.average_s = average;
        let r = app.reading().expect("a reading at the end");
        assert_eq!(r.rate_s_per_day, None, "over {average} s: {r:?}");
        assert!(
            off_standard(&r.caveats).is_some(),
            "over {average} s: {:?}",
            r.caveats
        );
    }
    let rates: Vec<f64> = (app.bench.results.trend.iter())
        .filter_map(|p| p.rate_s_per_day)
        .collect();
    assert!(rates.is_empty(), "the charts' rates: {rates:?}");
    let t = app.free_table(&egui::Context::default()).expect("a table");
    assert!(t.read > 0, "{t:?}");
    assert_eq!((t.mean[0], t.largest[0]), (None, None), "{t:?}");
    assert!(t.mean[1].is_some(), "amplitude stays: {t:?}");
    // Over 2 s and 4 s the Rate card waits for no more beats: no number
    // of them brings a rate in s/d here, and the chip says why (#2357).
    let ctx = themed();
    for average in [2.0, 4.0] {
        app.bench.take.average_s = average;
        let mut out = ctx.run(super::tests::screen(), |ctx| app.panels(ctx));
        for _ in 0..2 {
            out = ctx.run(super::tests::screen(), |ctx| app.panels(ctx));
        }
        let shown = painted_text(&out);
        let wait = timegrapher_core::measure::WAITING_FOR_MORE_BEATS;
        assert!(!shown.contains(wait), "over {average} s:\n{shown}");
        assert!(!shown.contains("More Beats"), "over {average} s:\n{shown}");
    }
}

/// A take recorded on Auto, opened (Peter on #2357): its `session.json`
/// says `bph: null`, so the 17,280 its `session.toml` keeps is Auto's
/// guess. It opens on Auto, as `timegrapher session` reads it, whether
/// its session file names its one sound file or the folder holds two,
/// and its lock off every standard rate gives no rate in s/d. Recorded at
/// 17,280, it reads at 17,280.
#[test]
fn a_take_recorded_on_auto_opens_on_auto() {
    use timegrapher_core::measure::off_standard;
    use timegrapher_core::synth::{generate, SynthConfig};
    let base = std::env::temp_dir().join(format!("tg-as-recorded-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let cfg = SynthConfig {
        bph: 17_570,
        duration_s: 40.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    // One sound file named in the session file, or the folder's two.
    let opened = |info: &str, parts: bool| {
        let take = base.join(format!("{info}-{parts}"));
        std::fs::create_dir_all(&take).unwrap();
        let file = if parts {
            let half = audio.samples.len() / 2;
            for (i, s) in [&audio.samples[..half], &audio.samples[half..]]
                .iter()
                .enumerate()
            {
                let part = timegrapher_core::audio::Audio {
                    samples: s.to_vec(),
                    sample_rate: audio.sample_rate,
                };
                let name = format!("off-{:03}.wav", i + 1);
                timegrapher_core::audio::write_wav(&take.join(name), &part).unwrap();
            }
            "."
        } else {
            timegrapher_core::audio::write_wav(&take.join("off.wav"), &audio).unwrap();
            "off.wav"
        };
        std::fs::write(
            take.join("session.toml"),
            format!("bph = 17280\n\n[[recording]]\nfile = '{file}'\nposition = 'DU'\n"),
        )
        .unwrap();
        let json = format!(r#"{{"watch": "", "position": "DU", "lift_deg": 52.0, "bph": {info}}}"#);
        std::fs::write(take.join(timegrapher_core::take::RECORD), json).unwrap();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.open_recording(&take, false);
        let t = std::time::Instant::now();
        while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 120 {
            app.poll();
        }
        assert!(app.bench.recording.batch.is_none(), "{:?}", app.message);
        app
    };
    let (auto, parts) = (opened("null", false), opened("null", true));
    let set = opened("17280", false);
    let _ = std::fs::remove_dir_all(&base);
    for (auto, how) in [(auto, "one file"), (parts, "a folder of two")] {
        assert_eq!(auto.bench.take.bph, None, "{how}");
        let w = auto.whole_take().expect("the whole take on Auto");
        assert_eq!(w.m.rate_s_per_day, None, "{how}: {:?}", w.m.caveats);
        assert!(
            off_standard(&w.m.caveats).is_some(),
            "{how}: {:?}",
            w.m.caveats
        );
    }
    assert_eq!(set.bench.take.bph, Some(17_280));
    let w = set.whole_take().expect("the whole take at 17,280");
    assert!(w.m.rate_s_per_day.is_some_and(|r| r > 1_000.0), "{:?}", w.m);
}

/// A take's Positions Test table, its six and its upright stops, read by
/// `take_reads::read_table` (#2253): on Auto, in a lock off every
/// standard rate, no row of either and no value over them is in s/d, and
/// each row names the caveat; at the rate set, every row reads its rate.
#[test]
fn an_off_standard_takes_table_and_upright_stops_have_no_rate_on_auto() {
    use timegrapher_core::measure::off_standard;
    use timegrapher_core::session::Position;
    use timegrapher_core::synth::{generate, SynthConfig};
    let cfg = SynthConfig {
        bph: 17_570,
        duration_s: 90.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let stream = timegrapher_core::stream::StreamConfig::default();
    let log = timegrapher_core::stream::analyze_samples(&audio.samples, 48_000, &stream);
    let stretches = [
        (Position::CH, 0.0, 30.0),
        (Position::H9, 30.0, 60.0),
        (Position::H7h30, 60.0, 90.0),
    ];
    for bph_set in [None, Some(log.bph)] {
        let t = super::take_reads::read_table(&log, None, &[], &[], &stretches, bph_set);
        let rows: Vec<_> = (t.six.0.iter()).chain(t.upright.0.iter()).collect();
        assert!(t.upright.0.len() >= 2 && t.six.0.len() >= 2, "{t:?}");
        for r in rows {
            let m = &r.measurement;
            let at = format!("{bph_set:?} {:?}", r.position);
            assert_eq!(m.rate_s_per_day.is_none(), bph_set.is_none(), "{at}: {m:?}");
            assert_eq!(
                off_standard(&m.caveats).is_some(),
                bph_set.is_none(),
                "{at}"
            );
            assert!(m.amplitude_deg.is_some(), "{at}");
        }
        assert_eq!(t.mean[0].is_none(), bph_set.is_none(), "{bph_set:?}: {t:?}");
    }
}

/// A take recorded on Auto, moved into a watch that sets a beat rate
/// (#1283): the move marks the take's `session.json`, so the watch's rate
/// stands (Peter on #2357), even where it is the rate Auto guessed and
/// `session.toml` does not change, and the take on show is read again at
/// it. Before the move it opens on Auto.
#[test]
fn a_take_recorded_on_auto_moved_into_a_watch_takes_its_rate() {
    use timegrapher_core::{library, synth::generate, synth::SynthConfig};
    let base = std::env::temp_dir().join(format!("tg-app-move-auto-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let pepsi = lib.add(&library::Watch::new("Pepsi")).unwrap();
    let mut own = library::Watch::new("Own");
    own.bph = Some(17_280);
    lib.add(&own).unwrap();
    let own_id = lib.find("Own").unwrap().watch.id.clone();
    drop(lib);
    let take = pepsi.join(library::TAKES).join("2026-11-26 10.00 Dial Up");
    std::fs::create_dir_all(&take).unwrap();
    let cfg = SynthConfig {
        bph: 17_570,
        duration_s: 40.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    timegrapher_core::audio::write_wav(&take.join("take_part01-001.wav"), &audio).unwrap();
    std::fs::write(
        take.join(timegrapher_core::take::MANIFEST),
        "watch = \"Pepsi\"\nbph = 17280\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let record = r#"{"watch": "Pepsi", "position": "DU", "lift_deg": 52.0, "bph": null}"#;
    std::fs::write(take.join(timegrapher_core::take::RECORD), record).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    let open = |app: &mut TimegrapherApp| {
        let t = std::time::Instant::now();
        while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 60 {
            app.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    app.take_action(Some(watches::TakeAction::Open(take.clone())));
    open(&mut app);
    assert_eq!(app.bench.take.bph, None, "opened on Auto");
    app.watches_view.take_readings.wait();
    app.move_take(&take, &own_id);
    let at = root
        .join("Own")
        .join(library::TAKES)
        .join(take.file_name().unwrap());
    assert!(!timegrapher_core::set_rate::recorded_on_auto(&at));
    assert!(app.bench.recording.batch.is_some(), "read again");
    open(&mut app);
    assert_eq!(app.take_on_show().as_ref(), Some(&at));
    assert_eq!(app.bench.take.bph, Some(17_280));
    let w = app.whole_take().expect("the whole take at 17,280");
    assert!(w.m.rate_s_per_day.is_some_and(|r| r > 1_000.0), "{:?}", w.m);
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// The other two opens of a take recorded on Auto (Vernon 56's P2 and
/// P4): played (Play, the folder of sound files on the strip) and kept
/// as its analysis alone then opened. Each opens on Auto, as File > Open
/// does (`apply_take_manifest`).
#[test]
fn a_take_recorded_on_auto_plays_and_opens_kept_on_auto() {
    use timegrapher_core::synth::{generate, SynthConfig};
    let base = std::env::temp_dir().join(format!("tg-as-recorded-play-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let take = base.join("take");
    std::fs::create_dir_all(&take).unwrap();
    let cfg = SynthConfig {
        bph: 17_570,
        duration_s: 40.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    timegrapher_core::audio::write_wav(&take.join("take_part01-001.wav"), &audio).unwrap();
    std::fs::write(
        take.join(timegrapher_core::take::MANIFEST),
        "bph = 17280\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let record = r#"{"watch": "", "position": "DU", "lift_deg": 52.0, "bph": null}"#;
    std::fs::write(take.join(timegrapher_core::take::RECORD), record).unwrap();
    // Played.
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.bench.take.bph = Some(18_000);
    app.start_replay(&take);
    assert!(
        app.message.as_ref().is_none_or(|(_, e)| !e),
        "{:?}",
        app.message
    );
    assert_eq!(app.bench.take.bph, None, "played");
    app.stop();
    // Kept as its analysis alone, then opened.
    let plan = timegrapher_core::analysis_take::plan_keep(&take, &Default::default()).unwrap();
    let kept =
        timegrapher_core::analysis_take::write_kept(&take, &plan, serde_json::Value::Null, |_| {})
            .unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.bench.take.bph = Some(18_000);
    app.open_recording(&kept, false);
    let t = std::time::Instant::now();
    while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 120 {
        app.poll();
    }
    let _ = std::fs::remove_dir_all(&base);
    assert!(
        app.message.as_ref().is_none_or(|(_, e)| !e),
        "{:?}",
        app.message
    );
    assert_eq!(app.bench.take.bph, None, "kept");
}

/// A ticking watch Stopped at every second from 1 to 12 s, and either
/// side of 10 s: under 10 s heard, live waits and the stopped take (read
/// as its saved take opens) reads no figures and says "Under 10 s Heard"
/// on every card, at every Average Over, never a rate from a few beats;
/// from 10 s both read. No Stop raises a fault, even one so short that
/// its clock log has two lines (Norm on #2333; #2334).
#[test]
fn an_early_stop_reads_under_ten_seconds_heard_and_no_fault() {
    use timegrapher_core::measure::{HeardSince, Wait};
    use timegrapher_core::synth::{generate, SynthConfig};
    let ctx = themed();
    let stops = [
        1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 9.9, 10.1, 10.5, 11.0, 12.0,
    ];
    let short = Wait::ShortHeard {
        since: HeardSince::Start,
    };
    for stop_s in stops {
        let base =
            std::env::temp_dir().join(format!("tg-early-stop-{}-{stop_s}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::create_dir_all(&base);
        let cfg = SynthConfig {
            bph: 28_800,
            duration_s: stop_s,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 0.0);
        let mut stopped_rate = None;
        let mut app = stopped_listened(&base, &audio, |_| {});
        assert_eq!(app.message, None, "{stop_s} s: no fault");
        for average in timegrapher_core::live::AVERAGES {
            app.bench.take.average_s = average;
            let r = app.card_reading().expect("a reading");
            let at = format!("{stop_s} s at {average} s");
            if stop_s < 10.0 {
                assert!(
                    r.rate_s_per_day.is_none()
                        && r.amplitude_deg.is_none()
                        && r.beat_error_ms.is_none(),
                    "{at}: {r:?}"
                );
                assert_eq!(r.listening, Some(HeardSince::Start), "{at}: {r:?}");
                assert_eq!(
                    app.listening(Some(&r)),
                    Some((short.title(), short.text())),
                    "{at}"
                );
                let text = cards_text(&app, &ctx);
                assert_eq!(painted(&text, "Under 10 s"), 3, "{at}: {text}");
                assert!(!text.contains("s/d"), "{at}: {text}");
            } else {
                assert!(
                    r.rate_s_per_day.is_some() && r.listening.is_none(),
                    "{at}: {r:?}"
                );
                stopped_rate = r.rate_s_per_day;
            }
        }
        assert_eq!(stopped_rate.is_some(), stop_s >= 10.0, "{stop_s} s");
        let _ = std::fs::remove_dir_all(&base);
    }
}
