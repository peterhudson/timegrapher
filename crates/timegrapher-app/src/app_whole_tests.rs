//! The whole take's reading, headless: it is session's reading of the
//! same stretch, at any length session reads.

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
    let mut log = steady::log_of(app.live.as_ref().unwrap()).unwrap();
    log.duration_s = log.beats.last().unwrap().time;
    app.live = Some(LiveAnalyzer::from_log(log, LiveConfig::default()));
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
