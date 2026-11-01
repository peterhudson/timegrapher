//! The Positions Test: marking positions, the positions table, the full
//! wind search, and the power reserve test waiting on the bench.

use super::tests::{frames, queued_capture, synthetic, themed, SETTLE_S};
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

/// The settling's hint names the settling the app uses, the Positions
/// Test pane's Settle, in a free take as in a test: 20 s unless set.
#[test]
fn the_settle_hint_names_the_settle_in_use() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    assert_eq!(app.session.settle_len(), guided::SETTLE_S);
    let hint = settle_hint(app.session.settle_len());
    assert!(hint.starts_with("For the first 20 seconds"), "{hint}");
    app.guided_settle_s = 30.0;
    app.session.set_settle_s(app.guided_settle_s);
    let hint = settle_hint(app.session.settle_len());
    assert!(hint.starts_with("For the first 30 seconds"), "{hint}");
}

/// Record, save, carry on, pause, change position, resume, save again
/// and reopen: sound after a save still counts as unsaved, Save Again
/// keeps it, and the saved take keeps the position it started in.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn a_take_saved_twice_across_a_move_keeps_everything() {
    let base = std::env::temp_dir().join(format!("tg-app-twice-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.bph = Some(28800);
    app.reset_session(48000, "test".into());
    app.session.listen();
    let audio = generate(
        &SynthConfig {
            duration_s: 90.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let third = audio.samples.len() / 3;
    let began = std::time::SystemTime::now();
    let feed = |app: &mut TimegrapherApp, i: usize| {
        let block = timegrapher_core::capture::Block {
            samples: audio.samples[i * third..(i + 1) * third].to_vec(),
            // Blocks arrive at their final frame, with a 60 s pause
            // between the second and third blocks.
            at: began + std::time::Duration::from_secs([30, 60, 150][i]),
        };
        app.session
            .recorder
            .as_mut()
            .unwrap()
            .write(&block)
            .unwrap();
        // Like poll(), send the same sound to the recorder and the
        // live analyser so their clocks agree before each transition.
        for samples in block.samples.chunks(960) {
            app.live.as_mut().unwrap().push(samples);
        }
    };
    let info = app.info("test", 48000, 16);
    app.session.recorder = Some(Recorder::start(&base.join("kept"), info).unwrap());
    feed(&mut app, 0);
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    assert_eq!(app.unsaved_s(), 0.0);
    // Carrying on after the save: that sound isn't saved yet.
    feed(&mut app, 1);
    assert!((app.unsaved_s() - 30.0).abs() < 0.1, "{}", app.unsaved_s());
    // Paused, moved to dial down, resumed through the same session
    // lifecycle as pause()/resume_microphone(), without opening a mic.
    let paused_at = app.live.as_ref().unwrap().duration_s();
    assert_eq!(paused_at, 60.0);
    assert_eq!(app.session.recorder.as_ref().unwrap().duration_s(), 60.0);
    app.session.pause(paused_at).unwrap().unwrap();
    app.session.set_settle_s(SETTLE_S);
    app.change_position(Position::DialDown);
    assert!(
        app.session.settles().is_empty(),
        "settling waits for Resume"
    );
    app.session.resume("test").unwrap();
    // Inject a deterministic elapsed pause instead of waiting for the
    // wall-clock duration returned by resume().
    let live = app.live.as_mut().unwrap();
    live.skip(60.0);
    app.session.resumed_at(live.duration_s());
    assert_eq!(app.session.settles(), [(120.0, 120.0 + SETTLE_S)]);
    for t in [60.0, 90.0, 120.0] {
        assert_eq!(app.session.audio_of(t), Some(60.0));
    }
    feed(&mut app, 2);
    assert_eq!(app.live.as_ref().unwrap().duration_s(), 150.0);
    assert_eq!(app.session.recorder.as_ref().unwrap().duration_s(), 90.0);
    assert_eq!(app.session.audio_of(120.0 + SETTLE_S), Some(70.0));
    assert_eq!(app.session.audio_of(150.0), Some(90.0));
    assert!((app.unsaved_s() - 60.0).abs() < 0.1, "{}", app.unsaved_s());
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    assert_eq!(app.unsaved_s(), 0.0);
    let saved = app
        .session
        .saved_to()
        .map(Path::to_path_buf)
        .expect("saved");
    let text = std::fs::read_to_string(saved.join(MANIFEST)).unwrap();
    let m: Manifest = toml::from_str(&text).unwrap();
    let got: Vec<_> = m
        .recordings
        .iter()
        .map(|e| (e.position.clone().unwrap(), e.start_s, e.end_s))
        .collect();
    assert_eq!(
        got,
        [
            ("DU".to_string(), Some(0.0), Some(60.0)),
            ("DD".to_string(), Some(60.0 + SETTLE_S), None)
        ],
        "{text}"
    );
    // The pause is a break, its sound in a file of its own.
    let session: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(saved.join("session.json")).unwrap())
            .unwrap();
    assert_eq!(session["breaks"].as_array().map(Vec::len), Some(1));
    assert_eq!(session["breaks"][0]["segment"], "audio-002.wav");
    assert_eq!(session["breaks"][0]["audio_s"], 60.0);
    assert_eq!(session["breaks"][0]["gap_s"], 60.0);
    assert_eq!(session["duration_s"], 90.0);
    // Reopened, all 90 s are there in both positions.
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.start_batch(&saved);
    let t = Instant::now();
    while again.batch.is_some() && t.elapsed().as_secs() < 120 {
        again.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let live = again.live.as_ref().expect("analysed");
    assert!(
        (live.duration_s() - 90.0).abs() < 0.5,
        "{}",
        live.duration_s()
    );
    assert_eq!(
        again.session.stretches(),
        [(0.0, Position::DialUp), (60.0, Position::DialDown)]
    );
    let _ = std::fs::remove_dir_all(&base);
}

/// A Positions Test begun with the watch already dial up: the strip
/// marks Dial Up from the start, with the time before the test named,
/// though the watch never moved.
#[test]
fn a_positions_test_is_marked_from_its_first_position() {
    let mut app = synthetic(20.0, 0.0);
    assert!(app.position_bands().is_empty(), "one position, no test");
    let mut test = guided::Test::new();
    test.tick(5.0);
    app.guided_unused = test.unused(10.0);
    app.guided_measured = test.measured(10.0);
    let bands = app.position_bands();
    assert_eq!(bands.len(), 1);
    assert_eq!(bands[0].2, "Dial Up (DU)");
    let settles = app.strip_settles();
    assert_eq!(settles[0], (0.0, 5.0, guided::BEFORE_TEST));
    assert_eq!(settles[1], (5.0, 10.0, guided::SETTLING));
}

/// A new position keeps the strip's beats and marks the stretch; the
/// readings and the distributions take only the position the watch is
/// in, and going back to an earlier position joins its stretches for
/// the steadiness tests.
/// Each position button's bar fills with the seconds heard in that
/// position: settled and not masked, other positions' time left out.
#[test]
fn each_position_counts_its_own_seconds() {
    let mut app = synthetic(20.0, 0.0);
    app.session.listen();
    app.session.set_state(session::State::Paused {
        since: Instant::now(),
    });
    // One position so far: every second is Dial Up's.
    let end = app.latest_s();
    assert!(
        (app.recorded_in(Position::DialUp) - end).abs() < 0.5,
        "{end}"
    );
    assert_eq!(app.recorded_in(Position::DialDown), 0.0);
    let cfg = SynthConfig {
        duration_s: 40.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let moved = app.live.as_ref().unwrap().duration_s();
    app.set_position(Position::DialDown);
    // Paused, the settling starts at Resume: here, at once.
    app.session.start_settle(moved);
    let live = app.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        live.push(b);
    }
    let du = app.recorded_in(Position::DialUp);
    let dd = app.recorded_in(Position::DialDown);
    assert!(
        (du - moved).abs() < 0.5,
        "Dial Up keeps its {du} s of {moved}"
    );
    // Dial Down's time to the latest beat, less its settling.
    let want = app.latest_s() - moved - app.guided_settle_s;
    assert!((dd - want).abs() < 1.0, "Dial Down {dd} s, not {want}");
    // A mask comes off the position it lies in.
    app.masked.push((moved + 25.0, moved + 30.0));
    assert!((app.recorded_in(Position::DialDown) - (want - 5.0)).abs() < 1.0);
    assert_eq!(app.recorded_in(Position::CrownUp), 0.0);
}

/// With no guided test, the Positions Test pane reads every position
/// marked in the take: each one's settled duration and readings, the
/// positions never marked, and the largest rate difference.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn the_positions_table_reads_a_free_take() {
    let mut app = synthetic(40.0, 0.0);
    app.session.listen();
    app.session.set_state(session::State::Paused {
        since: Instant::now(),
    });
    let cfg = SynthConfig {
        duration_s: 40.0,
        rate_s_per_day: 60.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    // Moved while listening, so the settling starts at once.
    let moved = |app: &mut TimegrapherApp, p: Position| {
        app.session.set_state(session::State::Listening);
        app.set_position(p);
        app.session.set_state(session::State::Paused {
            since: Instant::now(),
        });
    };
    moved(&mut app, Position::DialDown);
    let live = app.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        live.push(b);
    }
    let t = app.free_table(&egui::Context::default()).expect("a table");
    let row = |label: &str| t.rows.iter().find(|r| r.0 == label).unwrap().clone();
    let du = row("Dial Up (DU)");
    let dd = row("Dial Down (DD)");
    let (guided::FreeCell::Read(du_rate, ..), guided::FreeCell::Read(dd_rate, ..)) = (&du.2, &dd.2)
    else {
        panic!("{t:?}");
    };
    assert!(du_rate.unwrap().abs() < 5.0, "{du_rate:?}");
    assert!((dd_rate.unwrap() - 60.0).abs() < 5.0, "{dd_rate:?}");
    // Dial Down's settling is left out of its duration.
    assert!(
        (dd.1 - (40.0 - app.guided_settle_s)).abs() < 1.0,
        "{}",
        dd.1
    );
    assert_eq!(row("Crown Up (CU, 3H)").2, guided::FreeCell::NotMarked);
    assert_eq!(t.read, 2);
    assert!((t.largest[0].unwrap() - 60.0).abs() < 8.0);
    // The mean and the largest differences are core's, over the two
    // positions read.
    let (
        guided::FreeCell::Read(Some(du_rate), Some(du_amp), Some(du_be)),
        guided::FreeCell::Read(Some(dd_rate), Some(dd_amp), Some(dd_be)),
    ) = (&du.2, &dd.2)
    else {
        panic!("{t:?}");
    };
    let near = |a: Option<f64>, b: f64| (a.unwrap() - b).abs() < 1e-9;
    assert!(near(t.mean[0], (du_rate + dd_rate) / 2.0));
    assert!(near(t.mean[1], (du_amp + dd_amp) / 2.0));
    assert!(near(t.mean[2], (du_be + dd_be) / 2.0));
    assert!(near(t.largest[0], (du_rate - dd_rate).abs()));
    assert!(near(t.largest[1], (du_amp - dd_amp).abs()));
    assert!(near(t.largest[2], (du_be - dd_be).abs()));

    // The pane draws it, with Each Position's word for a free take.
    let ctx = themed();
    let mut drawn = String::new();
    for _ in 0..2 {
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.pane_tools(ui, Pane::PositionsTest);
                app.positions_pane(ui);
            });
        });
        drawn = format!("{:?} {:?}", out.shapes, theme::tools_drawn(&ctx));
    }
    for want in [
        "This Take",
        "Duration",
        "Not marked",
        "Average",
        "Largest Difference",
        "2 of 6",
        "Each Position applies only to the guided test.",
    ] {
        assert!(drawn.contains(want), "{want}: {drawn}");
    }

    // A position too short to read says so.
    moved(&mut app, Position::CrownUp);
    let audio = generate(
        &SynthConfig {
            duration_s: app.guided_settle_s + 5.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let live = app.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        live.push(b);
    }
    let t = app.free_table(&egui::Context::default()).unwrap();
    let cu = t.rows.iter().find(|r| r.0 == "Crown Up (CU, 3H)").unwrap();
    assert_eq!(cu.2, guided::FreeCell::Short, "{t:?}");
    // Seven seconds more: 12 s measured.
    let audio = generate(
        &SynthConfig {
            duration_s: 7.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let live = app.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        live.push(b);
    }
    let t = app.free_table(&egui::Context::default()).unwrap();
    let cu = t.rows.iter().find(|r| r.0 == "Crown Up (CU, 3H)").unwrap();
    assert!(
        matches!(cu.2, guided::FreeCell::Read(Some(_), Some(_), Some(_))),
        "{t:?}"
    );
    // A second masked in its middle leaves 11 s with no 10 s clear to
    // read: the row says so, never a row of dashes.
    let end = app.latest_s();
    app.masked.push((end - 7.0, end - 6.0));
    let t = app.free_table(&egui::Context::default()).unwrap();
    let cu = t.rows.iter().find(|r| r.0 == "Crown Up (CU, 3H)").unwrap();
    assert_eq!(cu.2, guided::FreeCell::Unread, "{t:?}");
}

#[test]
fn the_positions_table_reads_again_every_second_in_the_background_while_listening() {
    let base = std::env::temp_dir().join(format!("tg-free-table-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let mut app = synthetic(20.0, 0.0);
    app.session.listen();
    app.session.set_state(session::State::Paused {
        since: Instant::now(),
    });
    let feed = |app: &mut TimegrapherApp, s: f64| {
        let cfg = SynthConfig {
            duration_s: s,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 0.0);
        let live = app.live.as_mut().unwrap();
        for b in audio.samples.chunks(960) {
            live.push(b);
        }
    };
    feed(&mut app, 30.0);
    let _tx = queued_capture(&mut app, &base);
    assert!(app.running());
    let ctx = egui::Context::default();
    let du = |t: &guided::FreeTable| t.rows.iter().find(|r| r.0 == "Dial Up (DU)").unwrap().1;
    // The first table is read at once, to the last whole second.
    let first = app.free_table(&ctx).expect("a table");
    assert_eq!(du(&first), app.latest_s().floor());
    // Under a second more: the same second, the same table, nothing read.
    feed(&mut app, 0.5);
    assert_eq!(app.latest_s().floor(), du(&first));
    assert_eq!(du(&app.free_table(&ctx).unwrap()), du(&first));
    assert!(app.free_job.borrow().is_none());
    // Into the next seconds: the table on show stays while the next is
    // read in the background, then the next one shows.
    feed(&mut app, 2.0);
    assert_eq!(du(&app.free_table(&ctx).unwrap()), du(&first));
    assert!(app.free_job.borrow().is_some());
    let since = Instant::now();
    let next = loop {
        let t = app.free_table(&ctx).unwrap();
        if du(&t) != du(&first) {
            break t;
        }
        assert!(since.elapsed().as_secs() < 30, "never read again");
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert_eq!(du(&next), app.latest_s().floor());
    assert!(
        du(&next) >= du(&first) + 2.0,
        "{} {}",
        du(&first),
        du(&next)
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn the_guided_test_reads_the_position_under_way_as_it_is_measured() {
    let mut app = synthetic(20.0, 0.0);
    let start = app.live.as_ref().unwrap().duration_s();
    let mut test = guided::Test::new().timed(5.0, 60.0);
    test.tick(start);
    app.guided = Some(test);
    let ctx = egui::Context::default();
    // One stretch of sound at +30 s/d, heard in two parts.
    let audio = generate(
        &SynthConfig {
            duration_s: 40.0,
            rate_s_per_day: 30.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let (first, rest) = audio.samples.split_at(12 * 48_000);
    let feed = |app: &mut TimegrapherApp, samples: &[f32]| {
        let live = app.live.as_mut().unwrap();
        for b in samples.chunks(960) {
            live.push(b);
        }
    };
    // Settled, but under 10 s measured: nothing to show yet.
    feed(&mut app, first);
    assert_eq!(app.guided_live(&ctx), None);
    // 35 s measured: its figures so far.
    feed(&mut app, rest);
    let Some(guided::FreeCell::Read(Some(rate), Some(_), _)) = app.guided_live(&ctx) else {
        panic!("{:?}", app.guided_live(&ctx));
    };
    assert!((rate - 30.0).abs() < 5.0, "{rate}");
}

/// While a take is read, the status bar's bar says how far, not the
/// take's name again: that is already at the bar's left.
#[test]
fn the_analysing_bar_says_how_far_not_the_name() {
    assert_eq!(batch_words(0.374, true), "Analysing, 37%");
    assert_eq!(batch_words(0.0, false), "Analysing…");
}

/// Stop Test… on the Listen card, answered Stop Test, keeps the
/// positions measured in the table, stops listening, and leaves the
/// position free to set again.
#[test]
fn a_positions_tests_stop_keeps_its_rows_and_stops_listening() {
    let base = std::env::temp_dir().join(format!("tg-app-guided-stop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let mut app = synthetic(20.0, 0.0);
    app.session.listen();
    let tx = queued_capture(&mut app, &base);
    let start = app.live.as_ref().unwrap().duration_s();
    let mut test = guided::Test::new().timed(1.0, 5.0);
    test.tick(start);
    app.guided = Some(test);
    assert!(app.guided_running());
    assert!(app.test_listening());
    app.stop_test();
    assert!(app.session.paused(), "listening stopped");
    let kept = app.guided.as_ref().expect("the table stays");
    assert!(kept.readings.is_some(), "finished");
    assert!(!app.guided_running());
    assert!(app.capture.is_none());
    // A finished test sets nothing while listening goes on.
    let _tx2 = queued_capture(&mut app, &base);
    app.session.listen();
    assert!(app.running() && app.position_editable());
    drop(tx);
    let _ = std::fs::remove_dir_all(&base);
}

/// New Test sets the Positions Test up on the Bench and starts
/// nothing: the pane shows, the Position card reads Dial Up, and the
/// status bar says the test is ready, not started.
#[test]
fn a_new_positions_test_waits_ready_on_the_bench() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.view = View::Watches;
    set_pane_visible(&mut app.panes.tiles, Pane::PositionsTest, false);
    app.set_position(Position::CrownDown);
    app.new_take(NewTake::Positions, false);
    assert_eq!(app.ready_test(), Some(NewTake::Positions));
    assert!(app.guided.is_none() && !app.running());
    assert_eq!(app.view, View::Bench);
    assert!(pane_visible(&app.panes.tiles, Pane::PositionsTest));
    assert_eq!(app.position, Position::DialUp);
    let words = app.status_words();
    assert!(
        words.contains("Positions Test") && words.contains("Ready, not started"),
        "{words}"
    );
    // With no watch, nothing holds Start Test back.
    assert_eq!(app.start_test_why_not(), None);
}

/// A Positions Test set up for 24 Hours After is set against one of
/// the picked watch's own full-wind tests, picked in Compare With:
/// picking another watch while it waits offers that watch's, and
/// Start Test waits for the pick.
#[test]
fn a_ready_test_finds_the_picked_watchs_full_wind() {
    let base = std::env::temp_dir().join(format!("tg-app-ready-wind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Seiko 5")).unwrap();
    lib.add(&library::Watch::new("Daytona")).unwrap();
    let seiko = lib.find("Seiko 5").unwrap().watch.id.clone();
    let daytona = lib.find("Daytona").unwrap();
    let (daytona, take) = (
        daytona.watch.id.clone(),
        daytona.dir.join(library::TAKES).join("full wind"),
    );
    drop(lib);
    // Daytona's full-wind test, begun 31 h ago.
    std::fs::create_dir_all(&take).unwrap();
    let full = guided::FullWind {
        format: guided::RESULTS_FORMAT.to_string(),
        began_utc: String::new(),
        began_unix: guided::unix_now() - 31.0 * 3600.0,
        watch: "Daytona".into(),
        calibre: String::new(),
        bph: None,
        lift_angle_deg: None,
        settle_s: None,
        measure_s: None,
        positions: vec![guided::KeptRow {
            position: "DU".into(),
            rate_s_per_day: Some(4.0),
            amplitude_deg: Some(290.0),
            beat_error_ms: Some(0.3),
        }],
    };
    std::fs::write(take.join(guided::RESULTS_FILE), full.to_json()).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root));
    app.pick_watch(&seiko);
    app.guided_after = true;
    app.new_take(NewTake::Positions, false);
    assert_eq!(app.ready_test(), Some(NewTake::Positions));
    let why = app.start_test_why_not();
    assert!(
        why.as_deref()
            .is_some_and(|w| w.starts_with("Seiko 5 has no")),
        "{why:?}"
    );
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let frame = |app: &mut TimegrapherApp| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.panels(ctx));
    };
    app.pick_watch(&daytona);
    frame(&mut app);
    assert_eq!(app.ready_test(), Some(NewTake::Positions));
    // Offered, not picked for you.
    assert_eq!(app.guided_choices.len(), 1);
    assert_eq!(app.guided_earlier, None);
    let why = app.start_test_why_not();
    assert!(
        why.as_deref()
            .is_some_and(|w| w.starts_with("Pick the full-wind test")),
        "{why:?}"
    );
    app.pick_full_wind(0);
    assert_eq!(app.start_test_why_not(), None);
    assert_eq!(
        app.guided_earlier.as_ref().map(|f| f.watch.as_str()),
        Some("Daytona")
    );
    // And back: Seiko 5 has none.
    app.pick_watch(&seiko);
    frame(&mut app);
    assert!(app.start_test_why_not().is_some());
    let _ = std::fs::remove_dir_all(&base);
}

/// A test set up just before the full-wind test's 12 hour mark can
/// start once the mark passes, and a full-wind take removed is no
/// longer found: the search sees the time and the takes, not only the
/// watch and the Wind setting.
#[test]
fn a_ready_tests_full_wind_search_sees_the_time_and_the_takes() {
    let base = std::env::temp_dir().join(format!("tg-app-wind-time-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Seiko 5")).unwrap();
    let entry = lib.find("Seiko 5").unwrap();
    let (seiko, take) = (
        entry.watch.id.clone(),
        entry.dir.join(library::TAKES).join("full wind"),
    );
    drop(lib);
    // Its full-wind test began 11 h 30 min ago.
    let now = guided::unix_now();
    std::fs::create_dir_all(&take).unwrap();
    let full = guided::FullWind {
        format: guided::RESULTS_FORMAT.to_string(),
        began_utc: String::new(),
        began_unix: now - 11.5 * 3600.0,
        watch: "Seiko 5".into(),
        calibre: String::new(),
        bph: None,
        lift_angle_deg: None,
        settle_s: None,
        measure_s: None,
        positions: vec![guided::KeptRow {
            position: "DU".into(),
            rate_s_per_day: Some(4.0),
            amplitude_deg: Some(290.0),
            beat_error_ms: Some(0.3),
        }],
    };
    std::fs::write(take.join(guided::RESULTS_FILE), full.to_json()).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root));
    app.pick_watch(&seiko);
    app.guided_after = true;
    app.new_take(NewTake::Positions, false);
    app.find_full_wind_at(now);
    assert!(app.start_test_why_not().is_some(), "too soon");
    // An idle window still draws at the next minute, to search again.
    let at = (now / 60.0).floor() * 60.0 + 15.0;
    let wait = app
        .full_wind_recheck(at)
        .expect("waits on its full-wind test");
    assert!((wait.as_secs_f64() - 45.05).abs() < 1e-6, "{wait:?}");
    // An hour on, past the mark, with nothing else changed: offered,
    // and once picked, the test can start.
    app.find_full_wind_at(now + 3600.0);
    assert_eq!(app.guided_choices.len(), 1);
    assert!(app.start_test_why_not().is_some(), "not yet picked");
    app.pick_full_wind(0);
    assert_eq!(app.start_test_why_not(), None);
    // The take removed, in the same minute: no longer found, even with
    // the takes folder's time as it was, as Windows can leave it.
    let takes = take.parent().unwrap().to_path_buf();
    let was = std::fs::metadata(&takes).unwrap().modified().unwrap();
    std::fs::remove_dir_all(&take).unwrap();
    // Set back by hand where a folder can be opened to set it.
    #[cfg(unix)]
    {
        std::fs::File::open(&takes)
            .unwrap()
            .set_modified(was)
            .unwrap();
        assert_eq!(std::fs::metadata(&takes).unwrap().modified().unwrap(), was);
    }
    #[cfg(not(unix))]
    let _ = was;
    app.find_full_wind_at(now + 3600.0);
    assert!(app.start_test_why_not().is_some(), "removed");
    assert_eq!(app.guided_earlier, None, "the pick goes with its take");
    // With 24 Hours After off, nothing waits on the minute.
    app.guided_after = false;
    assert_eq!(app.full_wind_recheck(at), None);
    let _ = std::fs::remove_dir_all(&base);
}

/// The sixth position measured and read, the Positions Test ends by
/// itself: listening stops and the table keeps all six.
#[test]
fn a_positions_test_ends_by_itself_after_the_sixth_position() {
    let base = std::env::temp_dir().join(format!("tg-app-guided-end-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let mut app = synthetic(20.0, 0.0);
    app.session.listen();
    let tx = queued_capture(&mut app, &base);
    let end = app.live.as_ref().unwrap().duration_s();
    let mut test = guided::Test::new().timed(0.5, 2.0);
    let mut t = end - 18.0;
    test.tick(t);
    for i in 0..guided::ORDER.len() {
        t += 2.75;
        assert_eq!(test.tick(t), guided::Phase::Turn, "position {i}");
        if i + 1 < guided::ORDER.len() {
            test.advance(t);
        }
    }
    app.guided = Some(test);
    assert!(app.test_listening());
    let ctx = egui::Context::default();
    app.guided_tick(&ctx);
    assert!(!app.guided_running(), "ended");
    assert!(app.session.paused(), "listening stopped");
    let kept = app.guided.as_ref().expect("the table stays");
    assert_eq!(kept.readings.as_ref().map(|r| r.len()), Some(6));
    drop(tx);
    let _ = std::fs::remove_dir_all(&base);
}

/// The Watches view's Power Reserve Test only sets the take up: the
/// pane shows on the Bench, and nothing starts until its Start Test,
/// the position set first on the card.
#[test]
fn the_watches_power_reserve_test_waits_on_the_bench() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.view = View::Watches;
    set_pane_visible(&mut app.panes.tiles, Pane::PowerReserve, false);
    app.new_take(NewTake::PowerReserve, false);
    assert!(app.reserve.is_none() && !app.running());
    assert_eq!(app.view, View::Bench);
    assert!(pane_visible(&app.panes.tiles, Pane::PowerReserve));
    assert!(app.position_editable(), "the card sets it");
}

#[test]
fn a_new_position_marks_a_stretch_and_keeps_the_strip() {
    let mut app = synthetic(20.0, 0.0);
    app.session.listen();
    app.session.set_state(session::State::Paused {
        since: Instant::now(),
    });
    let feed = |app: &mut TimegrapherApp, rate: f64| {
        let cfg = SynthConfig {
            duration_s: 20.0,
            rate_s_per_day: rate,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 0.0);
        let live = app.live.as_mut().unwrap();
        for b in audio.samples.chunks(960) {
            if live.push(b) {
                let r = live.reading(app.average_s);
                app.trend.push(TrendPoint::of(r.time_s, &r));
            }
        }
    };
    let first = app.live.as_ref().unwrap().beats().len();
    app.set_position(Position::DialDown);
    assert_eq!(app.live.as_ref().unwrap().beats().len(), first, "kept");
    feed(&mut app, 60.0);
    app.set_position(Position::DialUp);
    feed(&mut app, 0.0);
    let live = app.live.as_ref().unwrap();
    assert!(live.beats().len() > 2 * first);
    assert_eq!(app.session.stretches().len(), 3);
    let bands = app.position_bands();
    assert_eq!(
        bands.iter().map(|b| b.2).collect::<Vec<_>>(),
        ["Dial Up (DU)", "Dial Down (DD)", "Dial Up (DU)"]
    );
    assert!(app.in_position(10.0) && !app.in_position(30.0) && app.in_position(50.0));
    // The readings since the move are Dial Up's alone.
    let r = app.reading().unwrap();
    assert!(
        r.rate_s_per_day.unwrap().abs() < 5.0,
        "{:?}",
        r.rate_s_per_day
    );
    // Dial Up's two stretches joined, without Dial Down's beats.
    let spans = app.current_spans();
    assert_eq!(spans.len(), 2);
    let log = steady::log_of(live).unwrap();
    let joined = steady::join(&log, &spans, &[], None).0;
    let dd = log
        .beats
        .iter()
        .filter(|b| b.time > spans[0].1 && b.time < spans[1].0)
        .count();
    assert!(dd > 100);
    assert!(joined.beats.len() + dd <= log.beats.len());
    assert!(joined.beats.len() + dd + 40 > log.beats.len());
    assert!(joined
        .beats
        .windows(2)
        .all(|w| w[1].time > w[0].time && w[1].index > w[0].index));
    let want = steady::joined_duration(&spans, live.duration_s(), None);
    assert!((joined.duration_s - want).abs() < 1e-9);
    assert!((want - (live.duration_s() - 20.0 + timegrapher_core::joined::JOIN_S)).abs() < 1.0);
    let fit = timegrapher_core::timing::fit(&joined.beats, joined.bph).unwrap();
    assert!(fit.rate_s_per_day.abs() < 5.0, "{}", fit.rate_s_per_day);
    // And it all draws, bands and all.
    let ctx = themed();
    for p in [Pane::Charts, Pane::Distributions] {
        set_pane_visible(&mut app.panes.tiles, p, true);
    }
    frames(&mut app, &ctx, 2);
    app.relayout(false);
    frames(&mut app, &ctx, 2);
}

/// A stretch dragged over on the strip is masked out of the readings
/// and the charts; a click on it brings it back.
#[test]
fn a_masked_stretch_leaves_the_readings_and_comes_back() {
    let mut app = synthetic(60.0, 0.0);
    let heard = |app: &TimegrapherApp| {
        app.trend
            .iter()
            .filter(|p| p.t > 30.0 && p.t < 40.0 && p.rate.is_some())
            .count()
    };
    // The 2-second stretches the distributions count, too.
    let stretches = |app: &TimegrapherApp| {
        app.unmasked_windows()
            .iter()
            .filter(|w| w.end_s > 30.0 && w.start_s < 45.0)
            .count()
    };
    assert!(heard(&app) > 0);
    assert!(stretches(&app) > 0);
    app.mask(30.2, 30.4);
    assert!(app.masked.is_empty(), "a slip of the mouse");
    app.mask(40.0, 30.0);
    app.mask(35.0, 45.0);
    assert_eq!(app.masked, [(30.0, 45.0)], "joined");
    assert_eq!(app.live.as_ref().unwrap().masked(), app.masked);
    assert_eq!(heard(&app), 0);
    assert_eq!(stretches(&app), 0);
    app.unmask_at(10.0);
    assert_eq!(app.masked.len(), 1, "nothing masked there");
    app.unmask_at(33.0);
    assert!(app.masked.is_empty());
    assert!(heard(&app) > 0);
    assert!(stretches(&app) > 0);
}

/// A masked stretch's edges move, and one moved over another joins it
/// for good; one moved to nothing comes back.
#[test]
fn a_masked_stretch_can_be_reshaped() {
    let mut app = synthetic(60.0, 0.0);
    app.set_masked(vec![(10.0, 15.0), (30.0, 40.0)]);
    app.reshape_mask((30.0, 40.0), (25.0, 40.0));
    assert_eq!(app.masked, [(10.0, 15.0), (25.0, 40.0)]);
    app.reshape_mask((10.0, 15.0), (10.0, 28.0));
    assert_eq!(app.masked, [(10.0, 40.0)], "joined");
    app.reshape_mask((99.0, 100.0), (1.0, 2.0));
    assert_eq!(app.masked, [(10.0, 40.0)], "no such stretch");
    app.reshape_mask((10.0, 40.0), (10.0, 10.2));
    assert!(app.masked.is_empty());
}
