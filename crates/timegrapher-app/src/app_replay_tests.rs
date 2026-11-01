//! Hidden panes and replay: panes hidden and brought back, launching
//! and replaying without an input, and replay keeping a take's history.

use super::tests::{frames, queued_capture, synthetic, themed};
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

#[test]
fn hidden_panes_stay_hidden_through_a_new_direction() {
    let mut app = synthetic(12.0, 20.0);
    let ctx = themed();
    frames(&mut app, &ctx, 2);
    for p in [Pane::Charts, Pane::Distributions, Pane::Strip] {
        set_pane_visible(&mut app.panes.tiles, p, false);
    }
    frames(&mut app, &ctx, 2);
    app.strip.horizontal = true;
    app.relayout(true);
    frames(&mut app, &ctx, 3);
    for p in [Pane::Charts, Pane::Distributions, Pane::Strip] {
        assert!(!pane_visible(&app.panes.tiles, p), "{p:?} came back");
    }
    assert!(pane_visible(&app.panes.tiles, Pane::Sound));
}

#[test]
fn a_new_averaging_time_redraws_the_whole_chart() {
    let mut app = synthetic(30.0, 20.0);
    let before = app.trend.len();
    assert!(before > 20);
    assert_eq!(app.live.as_ref().unwrap().config().profile_s, 10.0);
    app.set_average(4.0);
    // The tick and tock sound covers the same beats as the readings.
    assert_eq!(app.live.as_ref().unwrap().config().profile_s, 4.0);
    // Every point now uses 4 s of beats, back to the start.
    let early = app.trend.iter().find(|p| p.t >= 8.0).unwrap();
    let r = app.live.as_ref().unwrap().reading_at(early.t, 4.0);
    assert_eq!(early.rate, r.rate_s_per_day);
    assert!((app.trend.last().unwrap().t - app.live.as_ref().unwrap().duration_s()).abs() <= 0.5);
}

#[test]
fn values_under_the_pointer() {
    let pts = vec![[0.0, 1.0], [1.0, 2.0], [2.0, 3.0], [10.0, 4.0]];
    assert_eq!(value_at(&pts, 0.9), Some(2.0));
    assert_eq!(value_at(&pts, 1.4), Some(2.0));
    assert_eq!(value_at(&pts, -0.5), Some(1.0));
    // In the gap between 2 s and 10 s, nothing near enough.
    assert_eq!(
        value_at(
            &[[0.0, 1.0], [1.0, 2.0], [2.0, 3.0], [3.0, 3.0], [40.0, 4.0]],
            25.0
        ),
        None
    );
    assert_eq!(value_at(&[], 1.0), None);
}

#[test]
fn hidden_panes_leave_no_empty_frames_and_come_back() {
    let mut tree = default_layout(false);
    // Wrap each pane in its tab bar, as drawing does.
    tree.simplify(&SimplificationOptions {
        all_panes_must_have_tabs: true,
        ..Default::default()
    });
    let tiles = &mut tree.tiles;
    let visible_containers = |t: &Tiles<Pane>| {
        t.tile_ids()
            .filter(|&id| t.get_container(id).is_some() && t.is_visible(id))
            .count()
    };
    // The charts and distributions shown, as they are in use.
    for p in [Pane::Charts, Pane::Distributions] {
        set_pane_visible(tiles, p, true);
    }
    let all = visible_containers(tiles);
    set_pane_visible(tiles, Pane::Charts, false);
    assert!(!pane_visible(tiles, Pane::Charts));
    // They are tabs behind the profile, so its tab bar stays while
    // any pane shows in it,
    assert_eq!(visible_containers(tiles), all);
    set_pane_visible(tiles, Pane::Distributions, false);
    set_pane_visible(tiles, Pane::Scope, false);
    assert_eq!(visible_containers(tiles), all);
    // and goes with the last of them, nothing else.
    set_pane_visible(tiles, Pane::Sound, false);
    assert_eq!(visible_containers(tiles), all - 1);
    for p in Pane::ALL {
        set_pane_visible(tiles, p, false);
    }
    assert_eq!(visible_containers(tiles), 0, "root still shown");
    set_pane_visible(tiles, Pane::Distributions, true);
    assert!(pane_visible(tiles, Pane::Distributions));
    assert!(tiles.is_visible(tree.root.unwrap()));
    assert!(!pane_visible(tiles, Pane::Charts));

    // A new strip direction keeps the choice.
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    set_pane_visible(&mut app.panes.tiles, Pane::Sound, false);
    app.relayout(true);
    assert!(!pane_visible(&app.panes.tiles, Pane::Sound));
    assert!(pane_visible(&app.panes.tiles, Pane::Strip));
}

#[test]
fn histograms_start_hidden_and_keep_their_switch_through_a_relayout() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    assert!(!pane_visible(&app.panes.tiles, Pane::Distributions));
    // The charts start hidden too, in Quick Listen's layout.
    assert!(!pane_visible(&app.panes.tiles, Pane::Charts));
    set_pane_visible(&mut app.panes.tiles, Pane::Distributions, true);
    app.relayout(true);
    assert!(pane_visible(&app.panes.tiles, Pane::Distributions));
    assert!(!pane_visible(&app.panes.tiles, Pane::Charts));
}

#[test]
fn launching_and_replaying_touch_no_input() {
    // `new` lists nothing; a replay never needs to.
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.devices_listed = false;
    app.apply_settings(Settings {
        device: Some("alsa:hw:CARD=Device,DEV=0".into()),
        ..Settings::default()
    });
    assert_eq!(app.device.as_deref(), Some("alsa:hw:CARD=Device,DEV=0"));
    let dir = std::env::temp_dir().join("tg-app-no-input");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("take.wav");
    let audio = generate(
        &SynthConfig {
            duration_s: 4.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&path, &audio).unwrap();
    app.start_replay(&path);
    let ctx = themed();
    frames(&mut app, &ctx, 3);
    assert!(!app.devices_listed);
    // Nor does the microphone view, until its menu opens or Start.
    app.stop();
    app.input = Input::Microphone;
    frames(&mut app, &ctx, 2);
    assert!(!app.devices_listed);
}

fn replay_context_fixture(name: &str, single: bool) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("tg-replay-context-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let audio = timegrapher_core::audio::Audio {
        sample_rate: 100,
        samples: vec![0.0; if single { 9000 } else { 4500 }],
    };
    let first = dir.join("audio-001.wav");
    timegrapher_core::audio::write_wav(&first, &audio).unwrap();
    if !single {
        timegrapher_core::audio::write_wav(&dir.join("audio-002.wav"), &audio).unwrap();
    }
    let file = if single { "audio-001.wav" } else { "." };
    std::fs::write(
        dir.join(MANIFEST),
        format!(
            // Settled at each entry's start, as the app writes its takes.
            "watch = 'Replay Watch'\nbph = 28800\nlift = 49.0\nsettle_s = 0.0\n\n\
         [[recording]]\nfile = '{file}'\nposition = 'DU'\nstart_s = 0.0\nend_s = 30.0\n\n\
         [[recording]]\nfile = '{file}'\nposition = 'DD'\nstart_s = 40.0\nend_s = 60.0\n\n\
         [[recording]]\nfile = '{file}'\nposition = 'DU'\nstart_s = 70.0\n"
        ),
    )
    .unwrap();
    let path = if single { first } else { dir.clone() };
    (dir, path)
}

// Drain the real replay source without analyzing it. End follows file close,
// so later directory cleanup cannot race the reader on Windows.
fn drain_replay_source(app: &TimegrapherApp) -> usize {
    let c = app.capture.as_ref().unwrap();
    let mut samples = 0;
    let started = Instant::now();
    loop {
        assert!(
            started.elapsed().as_secs() < 10,
            "replay source did not end"
        );
        match c
            .rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
        {
            Event::Audio(b) => samples += b.samples.len(),
            Event::End => return samples,
            Event::Error(e) => panic!("{e}"),
        }
    }
}

#[test]
fn whole_recording_viewer_keeps_known_jumps_with_or_without_position_cuts() {
    for with_positions in [false, true] {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.average_s = 60.0;
        let log = BeatLog {
            sample_rate: 48000,
            duration_s: 120.0,
            bph: 28800,
            lift_deg: 52.0,
            beats: (0..960)
                .map(|index| timegrapher_core::beats::Beat {
                    index,
                    time: index as f64 / 8.0 + if index >= 480 { 0.03 } else { 0.0 },
                    quality: if index == 500 { 0.1 } else { 1.0 },
                })
                .collect(),
            amplitude_windows: vec![],
            clipped_beats: vec![],
            clipped_samples: 0,
            jumps: vec![60.03],
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        };
        let original: Vec<_> = log
            .beats
            .iter()
            .map(|b| (b.index, b.time, b.quality))
            .collect();
        let (positions, settles) = if with_positions {
            (
                vec![
                    (0.0, Position::DialUp),
                    (30.0, Position::DialDown),
                    (90.0, Position::DialUp),
                ],
                vec![(30.0, 40.0), (90.0, 100.0)],
            )
        } else {
            (vec![], vec![])
        };
        app.show_log(log, "known phase step".into(), positions, settles);
        let live = app.live.as_ref().unwrap();
        let expected = if with_positions {
            vec![30.0, 40.0, 60.03, 90.0, 100.0]
        } else {
            vec![60.03]
        };
        assert_eq!(
            live.breaks(),
            expected,
            "position cuts cannot erase an intrinsic jump"
        );
        assert_eq!(
            live.beats()
                .iter()
                .map(|b| (b.index, b.time, b.quality))
                .collect::<Vec<_>>(),
            original
        );
        let reading = app.reading_at_time(75.0).unwrap();
        assert!(reading.watch_heard);
        assert!(reading.span_s <= 75.0 - 60.03 + 1e-9, "{reading:?}");
        assert!(reading.rate_s_per_day.unwrap().abs() < 0.001, "{reading:?}");
        assert!(
            app.reading_at_time(20.0)
                .unwrap()
                .rate_s_per_day
                .unwrap()
                .abs()
                < 0.001
        );
    }
}

#[test]
fn replay_restores_saved_history_and_a_position_change_preserves_it() {
    let (dir, path) = replay_context_fixture("folder", false);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.start_replay_at(&path, 100.0);
    assert_eq!(
        drain_replay_source(&app),
        9000,
        "both segments, in recorded-audio time"
    );
    let positions = [
        (0.0, Position::DialUp),
        (30.0, Position::DialDown),
        (60.0, Position::DialUp),
    ];
    let settles = [(30.0, 40.0), (60.0, 70.0)];
    let breaks = [30.0, 40.0, 60.0, 70.0];
    assert_eq!(app.session.stretches(), positions);
    assert_eq!(app.session.settles(), settles);
    assert_eq!(app.live.as_ref().unwrap().breaks(), breaks);
    assert_eq!(
        (app.watch.as_str(), app.lift_deg, app.bph),
        ("Replay Watch", 49.0, Some(28800))
    );
    app.change_position(Position::DialDown);
    assert_eq!(app.position, Position::DialDown);
    assert_eq!(app.session.stretches(), positions);
    assert_eq!(app.session.settles(), settles);
    assert_eq!(app.live.as_ref().unwrap().breaks(), breaks);
    app.stop();
    assert!(app.session.has_replay_context());
    // A segment alone does not match the folder's '.' entries.
    app.start_replay_at(&dir.join("audio-001.wav"), 100.0);
    assert_eq!(drain_replay_source(&app), 4500);
    assert!(!app.session.has_replay_context());
    assert!(app.session.stretches().is_empty());
    assert!(app.session.settles().is_empty());
    assert!(app.live.as_ref().unwrap().breaks().is_empty());
    app.change_position(Position::DialDown);
    assert!(
        !app.session.stretches().is_empty(),
        "plain replay keeps manual moves"
    );
    app.start_replay_at(&path, 100.0);
    assert_eq!(drain_replay_source(&app), 9000);
    assert_eq!(app.session.stretches(), positions);
    assert_eq!(app.live.as_ref().unwrap().breaks(), breaks);
    app.new_session();
    assert!(!app.session.has_replay_context());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn replay_uses_the_same_matching_file_manifest_as_whole_take_analysis() {
    let (dir, path) = replay_context_fixture("file", true);
    let mut batch = TimegrapherApp::with_devices(Vec::new(), None);
    let (manifest, entries) = read_manifest(&path).unwrap().unwrap();
    let (positions, settles) = batch.apply_manifest(&manifest, &entries, None);
    let mut replay = TimegrapherApp::with_devices(Vec::new(), None);
    replay.start_replay_at(&path, 100.0);
    assert_eq!(drain_replay_source(&replay), 9000);
    assert_eq!(replay.session.stretches(), positions);
    assert_eq!(replay.session.settles(), settles);
    assert_eq!(replay.watch, batch.watch);
    assert_eq!((replay.lift_deg, replay.bph), (batch.lift_deg, batch.bph));
    replay.stop();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn replay_poll_does_not_publish_readings_inside_saved_settling() {
    let base = std::env::temp_dir().join(format!("tg-replay-settle-poll-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let mut app = synthetic(12.0, 0.0);
    app.session.replay();
    app.session.load(
        &[(0.0, Position::DialUp), (8.0, Position::DialDown)],
        &[(8.0, 14.0)],
    );
    app.live
        .as_mut()
        .unwrap()
        .set_breaks(app.session.replay_breaks().unwrap());
    app.clear_trends();
    let tx = queued_capture(&mut app, &base);
    app.capture.as_mut().unwrap().is_file = true;
    tx.send(Event::Audio(timegrapher_core::capture::Block {
        samples: vec![0.0; 24000],
        at: std::time::SystemTime::now(),
    }))
    .unwrap();
    app.poll();
    let at = app.live.as_ref().unwrap().reading(app.average_s).time_s;
    assert!(
        at > 8.0 && at <= 14.0,
        "fixture must read within the saved settle: {at}"
    );
    assert!(app.trend.is_empty());
    assert!(app.reading().is_none());
    assert_eq!(app.live.as_ref().unwrap().breaks(), &[8.0, 14.0]);
    app.stop();
    drop(tx);
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn replay_readouts_and_rebuilt_trends_respect_imported_settling() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let log = BeatLog {
        sample_rate: 48000,
        duration_s: 90.0,
        bph: 28800,
        lift_deg: 52.0,
        beats: (0..720)
            .map(|i| timegrapher_core::beats::Beat {
                index: i,
                time: i as f64 / 8.0,
                quality: 1.0,
            })
            .collect(),
        amplitude_windows: vec![],
        clipped_beats: vec![],
        clipped_samples: 0,
        jumps: vec![],
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    app.live = Some(LiveAnalyzer::from_log(log, app.live_config()));
    app.session.replay();
    app.session.load(
        &[(0.0, Position::DialUp), (30.0, Position::DialDown)],
        &[(30.0, 40.0)],
    );
    app.live
        .as_mut()
        .unwrap()
        .set_breaks(app.session.replay_breaks().unwrap());
    app.rebuild_trend();
    assert!(!app.trend.is_empty());
    assert!(app.trend.iter().all(|p| p.t <= 30.0 || p.t > 40.0));
    assert!(app.reading_at_time(35.0).is_none());
    let after = app.reading_at_time(41.0).unwrap();
    assert!(
        after.span_s <= 1.0,
        "no fit crosses the settling end: {}",
        after.span_s
    );
    // Endpoint convention is inherited unchanged: settling_at is (from,to].
    assert!(app.reading_at_time(40.0).is_none());
}
