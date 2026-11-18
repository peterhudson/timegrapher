//! Opening takes: a take's calibre and settings, positions read once
//! settled, failed and refused opens, and manifests.

use super::tests::{reopened, synthetic, SETTLE_S};
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

/// A take is read with its calibre's settings where its session file
/// gives none, as the Watches list and `session` read it.
#[test]
fn a_take_is_read_with_its_calibres_settings() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.cal
        .user
        .custom
        .push(timegrapher_core::calibre_store::CustomCalibre {
            name: "Test 21".into(),
            maker: None,
            aliases: Vec::new(),
            bph: Some(21600),
            lift_angle_deg: Some(49.0),
            escape_teeth: Some(21),
            wheels: Vec::new(),
            notes: None,
        });
    let read = |text: &str| {
        let m: Manifest = toml::from_str(text).unwrap();
        app.entry_settings(&m).stream.analysis
    };
    let entry = "\n[[recording]]\nfile = '.'\nposition = 'DU'\n";
    let a = read(&format!("calibre = 'Test 21'\n{entry}"));
    assert_eq!(a.escape_teeth, 21);
    assert_eq!(a.bph, Some(21600));
    // The session file's own figures come first.
    let a = read(&format!("calibre = 'Test 21'\nescape_teeth = 18\n{entry}"));
    assert_eq!(a.escape_teeth, 18);
    // No calibre, or one not known: core's 15 teeth.
    assert_eq!(read(entry).escape_teeth, 15);
    assert_eq!(
        read(&format!("calibre = 'Unknown'\n{entry}")).escape_teeth,
        15
    );
}

/// A take whose entries say how long each position settles, as
/// `timegrapher record` writes them, is read once each has settled, as
/// `session` reads it: the settling is marked, not read. The app's own
/// takes say `settle_s = 0`, so theirs read from their start.
#[test]
fn a_recorded_takes_positions_are_read_once_settled() {
    let text = "audio = true\nsettle_s = 30.0\n\n\
                [[recording]]\nfile = '.'\nposition = 'DU'\nstart_s = 0.0\nend_s = 90.0\n\n\
                [[recording]]\nfile = '.'\nposition = 'DD'\nstart_s = 90.0\nend_s = 180.0\n\
                measure_s = 40.0\n";
    let m: Manifest = toml::from_str(text).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let (stretches, settles) = app.apply_manifest(&m, &m.recordings, None);
    assert_eq!(
        stretches,
        [(0.0, Position::DialUp), (90.0, Position::DialDown)]
    );
    assert_eq!(settles, [(0.0, 30.0), (90.0, 120.0)]);
    // Settled at once, as the app writes its own takes.
    let text = text.replace("settle_s = 30.0", "settle_s = 0.0");
    let m: Manifest = toml::from_str(&text).unwrap();
    let (_, settles) = app.apply_manifest(&m, &m.recordings, None);
    assert!(settles.is_empty(), "{settles:?}");
    // A take shorter than twice its settling settles to halfway, as
    // session reads it, once its length is known.
    let text = "audio = true\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n";
    let short: Manifest = toml::from_str(text).unwrap();
    let (_, settles) = app.apply_manifest(&short, &short.recordings, Some(25.0));
    assert_eq!(settles, [(0.0, 12.5)]);
    let (_, settles) = app.apply_manifest(&short, &short.recordings, None);
    assert_eq!(settles, [(0.0, timegrapher_core::take::SETTLE_S)]);
}

/// A take of one position whose session file gives no settling (one
/// made by `timegrapher record`) is read once settled, as `session`
/// reads it: its first 20 s are left out of its row.
#[test]
fn a_recorded_take_of_one_position_is_read_once_settled() {
    let dir = std::env::temp_dir().join(format!("tg-app-one-settle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 50.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&dir.join("audio-001.wav"), &audio).unwrap();
    std::fs::write(
        dir.join(MANIFEST),
        "bph = 28800\nlift = 52.0\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n",
    )
    .unwrap();
    let app = reopened(&dir);
    assert_eq!(app.session.settles(), [(0.0, 20.0)]);
    let spans = app.session.settled_spans(app.position, Position::DialUp);
    assert_eq!(spans.len(), 1, "{spans:?}");
    assert_eq!(spans[0].0, 20.0, "{spans:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A take that fails to open leaves the take on show as it was, read
/// on its own clock and breaks.
#[test]
fn a_failed_open_leaves_the_takes_clock() {
    let base = std::env::temp_dir().join(format!("tg-app-failed-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let good = base.join("good");
    std::fs::create_dir_all(&good).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 30.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&good.join("audio-001.wav"), &audio).unwrap();
    std::fs::write(
        good.join(MANIFEST),
        "bph = 28800\nlift = 52.0\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n",
    )
    .unwrap();
    let mut app = reopened(&good);
    let breaks = app.take_breaks.clone();
    assert!(breaks.is_some());
    // Sound that can't be read: found, then refused.
    let bad = base.join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("audio-001.wav"), b"not a wav").unwrap();
    std::fs::write(bad.join(MANIFEST), "[[recording]]\nfile = '.'\n").unwrap();
    app.start_batch(&bad);
    assert!(
        app.message.as_ref().is_some_and(|m| m.1),
        "{:?}",
        app.message
    );
    assert!(app.batch.is_none() && app.live.is_some());
    assert_eq!(app.take_breaks, breaks);
    let _ = std::fs::remove_dir_all(&base);
}

/// A take refused while it is read (here, a clock log of one line)
/// says why and leaves the window no longer opening.
#[test]
fn a_take_refused_while_read_is_no_longer_opening() {
    let base = std::env::temp_dir().join(format!("tg-app-refused-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&base.join("audio-001.wav"), &audio).unwrap();
    std::fs::write(base.join("clock.csv"), "1700000000.000000,0\n").unwrap();
    std::fs::write(
        base.join(MANIFEST),
        "bph = 28800\nlift = 52.0\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n\
         clock = 'clock.csv'\nsettle_s = 0.0\n",
    )
    .unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.start_batch(&base);
    assert!(app.batch.is_some(), "{:?}", app.message);
    assert_eq!(app.session.state(), session::State::Analysing);
    let t = Instant::now();
    while app.batch.is_some() && t.elapsed().as_secs() < 120 {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(app.batch.is_none());
    assert!(
        app.message
            .as_ref()
            .is_some_and(|m| m.1 && m.0.contains("need at least 2")),
        "{:?}",
        app.message
    );
    assert_eq!(app.session.state(), session::State::Idle);
    let _ = std::fs::remove_dir_all(&base);
}

/// A cancelled opening leaves nothing opening: the window no longer says
/// Opening (#1220, Walter's find).
#[test]
fn a_cancelled_open_is_no_longer_opening_and_leaves_nothing() {
    let base = std::env::temp_dir().join(format!("tg-app-cancel-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 600.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let wav = base.join("long.wav");
    timegrapher_core::audio::write_wav(&wav, &audio).unwrap();
    std::fs::write(
        base.join(MANIFEST),
        "calibre = '3235'\nlift = 55.0\n\n[[recording]]\nfile = 'long.wav'\nposition = 'CD'\n",
    )
    .unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let before = (app.status_words(), app.watch_setup());
    app.start_batch(&base);
    assert!(app.batch.is_some(), "{:?}", app.message);
    assert_eq!(app.state_words(), "Opening");
    assert_eq!(app.calibre, "3235");
    app.cancel_open();
    for _ in 0..10 {
        app.poll();
    }
    assert!(app.batch.is_none());
    assert_ne!(app.state_words(), "Opening");
    assert!(
        !app.message.as_ref().is_some_and(|m| m.1),
        "{:?}",
        app.message
    );
    // Nothing of the take is left: the bar and the cards are as they were
    // before, and Save has nothing to keep (#1238).
    assert_eq!((app.status_words(), app.watch_setup()), before);
    assert!(app.source_label.is_empty(), "{}", app.source_label);
    assert!(!app.session.allows(Action::Save));
    let _ = std::fs::remove_dir_all(&base);
}

/// A take is opened whole as the command line opens it (core's
/// `take::whole_entry`), so one whose entries name two clock logs is
/// refused with core's reason, not read on the first entry's (#731).
#[test]
fn a_take_on_two_clock_logs_is_refused_as_the_command_line_refuses_it() {
    let base = std::env::temp_dir().join(format!("tg-app-two-clocks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&base.join("audio-001.wav"), &audio).unwrap();
    std::fs::write(
        base.join(MANIFEST),
        "bph = 28800\nlift = 52.0\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n\
         clock = 'a.csv'\nstart_s = 0.0\nend_s = 10.0\nsettle_s = 0.0\n\n\
         [[recording]]\nfile = '.'\nposition = 'DD'\nclock = 'b.csv'\nstart_s = 10.0\n\
         settle_s = 0.0\n",
    )
    .unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.start_batch(&base);
    assert!(app.batch.is_none());
    assert!(
        app.message
            .as_ref()
            .is_some_and(|m| m.1 && m.0.contains("2 clock logs")),
        "{:?}",
        app.message
    );
    let _ = std::fs::remove_dir_all(&base);
}

/// Opening one recording of a folder of several reads that recording
/// alone, however its session file's entries spell its name (#731).
#[test]
fn one_recording_of_a_folder_is_read_alone_however_its_entries_spell_it() {
    let base = std::env::temp_dir().join(format!("tg-app-one-of-two-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    for name in ["audio-001.wav", "audio-002.wav"] {
        std::fs::write(base.join(name), b"").unwrap();
    }
    std::fs::write(
        base.join(MANIFEST),
        "bph = 28800\n\n[[recording]]\nfile = 'audio-001.wav'\nposition = 'DU'\n\n\
         [[recording]]\nfile = './audio-001.wav'\nposition = 'DD'\n",
    )
    .unwrap();
    let path = base.join("audio-001.wav");
    let manifest = read_manifest(&path).unwrap();
    assert_eq!(manifest.as_ref().map(|(_, e)| e.len()), Some(2));
    let (_, m) = take_entry(&path, manifest.as_ref()).unwrap();
    let files: Vec<&str> = m.recordings.iter().map(|e| e.file.as_str()).collect();
    assert_eq!(files, ["audio-001.wav", "audio-001.wav"]);
    let whole = timegrapher_core::take::whole_take(&m).unwrap();
    assert_eq!(whole.file, "audio-001.wav");
    let _ = std::fs::remove_dir_all(&base);
}

/// Guided elapsed-time endpoints use the recorder timeline even when
/// the watch never changes position and a pause spans an endpoint.
#[test]
fn positions_test_manifest_maps_paused_elapsed_time_to_recorded_sound() {
    let base = std::env::temp_dir().join(format!("tg-app-guided-pause-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.session.listen();
    let info = app.info("test", 100, 16);
    app.session.recorder = Some(Recorder::start(&base, info).unwrap());
    app.session
        .recorder
        .as_mut()
        .unwrap()
        .write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 60 * 100],
            at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(60),
        })
        .unwrap();
    app.session.pause(60.0).unwrap().unwrap();
    app.session.resume("test").unwrap();
    app.session.resumed_at(120.0);
    app.session
        .recorder
        .as_mut()
        .unwrap()
        .write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 10 * 100],
            at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(130),
        })
        .unwrap();
    for (measure, end) in [(60.0, 60.0), (110.0, 70.0)] {
        let mut test = guided::Test::new().timed(20.0, measure);
        test.tick(0.0);
        test.tick(130.0);
        app.guided = Some(test);
        let m = app.take_manifest();
        assert_eq!(m.recordings.len(), 1);
        assert_eq!(m.recordings[0].position.as_deref(), Some("DU"));
        assert_eq!(m.recordings[0].start_s, Some(20.0));
        assert_eq!(m.recordings[0].end_s, Some(end));
        // Its start is after the settling: nothing more to skip.
        assert_eq!(m.recordings[0].settle_s, Some(0.0));
    }
    app.session.discard_recording();
}

/// A take saved during a Positions Test lists the test's own measured
/// stretches, so `timegrapher session` reads the seconds the table did.
#[test]
fn a_positions_test_take_lists_its_measured_stretches() {
    let base = std::env::temp_dir().join(format!("tg-app-guided-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = synthetic(5.0, 0.0);
    app.session.listen();
    let info = app.info("test", 48000, 16);
    app.session.recorder = Some(Recorder::start(&base.join("kept"), info).unwrap());
    let audio = generate(
        &SynthConfig {
            duration_s: 80.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let r = app.session.recorder.as_mut().unwrap();
    r.write(&timegrapher_core::capture::Block {
        samples: audio.samples,
        at: std::time::SystemTime::now(),
    })
    .unwrap();
    let mut test = guided::Test::new();
    test.tick(0.0);
    test.tick(guided::SETTLE_S + guided::MEASURE_S);
    let next = test.advance(80.0).expect("a second position");
    app.session.move_to(80.0, Position::DialUp, next).unwrap();
    test.tick(80.0);
    test.tick(80.0 + guided::SETTLE_S + guided::MEASURE_S);
    app.session
        .recorder
        .as_mut()
        .unwrap()
        .write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 80 * 48000],
            at: std::time::SystemTime::now() + std::time::Duration::from_secs(80),
        })
        .unwrap();
    app.guided = Some(test);
    let m = app.take_manifest();
    let got: Vec<_> = m
        .recordings
        .iter()
        .map(|e| (e.position.clone().unwrap(), e.start_s, e.end_s))
        .collect();
    assert_eq!(
        got,
        [
            ("DU".to_string(), Some(20.0), Some(80.0)),
            ("DD".to_string(), Some(100.0), Some(160.0)),
        ]
    );
    assert!(m.recordings.iter().all(|e| e.wind_h.is_none()), "full wind");
    // Finished, the session's folder keeps its results for the 24
    // hour half.
    app.guided.as_mut().unwrap().readings = Some(Vec::new());
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    let dir = app.session.saved_to().unwrap().to_path_buf();
    let kept: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(guided::RESULTS_FILE)).unwrap())
            .unwrap();
    assert_eq!(kept["format"], guided::RESULTS_FORMAT);
    // The 24 hour half's stretches carry the hours since full wind.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let full = guided::FullWind {
        wind_h: None,
        against: None,
        format: guided::RESULTS_FORMAT.into(),
        began_utc: String::new(),
        began_unix: now - 24.0 * 3600.0,
        watch: String::new(),
        calibre: String::new(),
        bph: None,
        lift_angle_deg: None,
        settle_s: None,
        measure_s: None,
        positions: Vec::new(),
    };
    let mut half = guided::Test::after(full);
    half.tick(0.0);
    half.tick(guided::SETTLE_S + guided::MEASURE_S);
    app.guided = Some(half);
    let m = app.take_manifest();
    assert_eq!(m.recordings.len(), 1);
    assert_eq!(m.recordings[0].wind_h, Some(24.0));
    let _ = std::fs::remove_dir_all(&base);
}

/// A saved take carries a session file with the watch, its calibre,
/// lift angle, beat rate and positions, and opening it takes them back.
#[test]
#[expect(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn a_saved_take_brings_back_its_calibre_and_positions() {
    let base = std::env::temp_dir().join(format!("tg-app-sidecar-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = synthetic(5.0, 0.0);
    app.session.listen();
    app.watch = "Test Watch".into();
    app.cal.user.put_custom(calibre_ui::custom(
        "Bench Calibre",
        [("escape wheel", 5.0), ("fourth wheel", 60.0)],
    ));
    app.calibre = "Bench Calibre".into();
    app.set_lift(49.0);
    app.bph = Some(28800);
    let audio = generate(
        &SynthConfig {
            duration_s: 90.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let info = app.info("test", 48000, 16);
    app.session.recorder = Some(Recorder::start(&base.join("kept"), info).unwrap());
    app.session.set_settle_s(SETTLE_S);
    let third = audio.samples.len() / 3;
    // Each 30 s block arrives as its last frame is heard, as the
    // capture delivers sound, so the clock log reads as a real take's.
    let began = std::time::SystemTime::now();
    let moves = [
        (0, None),
        (1, Some((Position::DialUp, Position::DialDown))),
        (2, Some((Position::DialDown, Position::DialUp))),
    ];
    for (i, pos) in moves {
        if let Some((old, new)) = pos {
            let r = app.session.recorder.as_mut().unwrap();
            r.note(&format!("position {}", new.code())).unwrap();
            app.session.move_to(i as f64 * 30.0, old, new).unwrap();
        }
        let r = app.session.recorder.as_mut().unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: audio.samples[i * third..(i + 1) * third].to_vec(),
            at: began + std::time::Duration::from_secs(30 * (i as u64 + 1)),
        })
        .unwrap();
    }
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    let saved = app
        .session
        .saved_to()
        .map(Path::to_path_buf)
        .expect("saved");
    let text = std::fs::read_to_string(saved.join(MANIFEST)).unwrap();
    let m: Manifest = toml::from_str(&text).unwrap();
    assert_eq!(m.calibre.as_deref(), Some("Bench Calibre"));
    assert_eq!(m.wheels.len(), 2, "{text}");
    let starts: Vec<_> = m.recordings.iter().map(|e| e.start_s).collect();
    // Each later position starts once it has settled.
    assert_eq!(starts, [Some(0.0), Some(40.0), Some(70.0)], "{text}");
    let ends: Vec<_> = m.recordings.iter().map(|e| e.end_s).collect();
    assert_eq!(ends, [Some(30.0), Some(60.0), None], "{text}");
    assert_eq!(m.recordings[2].end_s, None);
    // Each start is already after its settling, and the entry says so,
    // so `timegrapher session` skips nothing more: it reads the seconds
    // the app's table read.
    assert!(
        m.recordings.iter().all(|e| e.settle_s == Some(0.0)),
        "{text}"
    );
    // A mask in the settling gap after the first move is on every
    // entry, though inside none, and comes back when the file is read.
    app.set_masked(vec![(32.0, 38.0)]);
    let m2 = app.take_manifest();
    assert_eq!(m2.recordings.len(), 3);
    for r in &m2.recordings {
        assert_eq!(r.masked, [[32.0, 38.0]]);
    }
    let mut opened = TimegrapherApp::with_devices(Vec::new(), None);
    opened.apply_manifest(&m2, &m2.recordings, None);
    assert_eq!(opened.masked, [(32.0, 38.0)]);
    app.set_masked(Vec::new());
    // Nothing painted: nothing kept as recorded.
    assert!(m.recorded.is_empty(), "{text}");
    // Painted over the last stretch: what was recorded is kept under
    // it, in the sound's seconds, as the recordings are listed; then
    // painted back, it goes.
    app.session
        .paint(75.0, 85.0, Position::CrownLeft, Position::DialUp, 90.0);
    let recorded: Vec<_> = app
        .take_manifest()
        .recorded
        .iter()
        .map(|r| (r.position.clone(), r.start_s, r.end_s))
        .collect();
    assert_eq!(
        recorded,
        [
            ("DU".to_string(), Some(0.0), Some(30.0)),
            ("DD".to_string(), Some(40.0), Some(60.0)),
            ("DU".to_string(), Some(70.0), None)
        ]
    );
    app.session
        .paint(75.0, 85.0, Position::DialUp, Position::DialUp, 90.0);
    assert!(app.take_manifest().recorded.is_empty());

    // Opened on a fresh app, the take brings it all back.
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.start_batch(&saved);
    let t = Instant::now();
    while again.batch.is_some() && t.elapsed().as_secs() < 120 {
        again.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(again.live.is_some(), "analysed");
    assert_eq!(again.watch, "Test Watch");
    assert_eq!(again.calibre, "Bench Calibre");
    // Not known there: added to its own calibres from the wheels.
    assert!(again
        .cal
        .find("Bench Calibre")
        .is_some_and(|e| e.is_custom()));
    assert_eq!(again.calibre_wheels().map(|w| w.len()), Some(2));
    assert_eq!((again.lift_deg, again.bph), (49.0, Some(28800)));
    assert_eq!(
        again.session.stretches(),
        [
            (0.0, Position::DialUp),
            (30.0, Position::DialDown),
            (60.0, Position::DialUp)
        ]
    );
    assert_eq!(again.session.settles(), [(30.0, 40.0), (60.0, 70.0)]);
    assert_eq!(
        again.live.as_ref().unwrap().breaks(),
        &[30.0, 40.0, 60.0, 70.0]
    );
    assert_eq!(again.position, Position::DialUp);
    // A recording with no session file opens as before.
    assert!(read_manifest(&base).unwrap().is_none());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn applicable_invalid_manifest_stops_actual_open_entrypoints_before_audio() {
    let root =
        std::env::temp_dir().join(format!("tg-invalid-manifest-open-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("watch.wav");
    // Deliberately not an audio file: manifest error must win over header error.
    std::fs::write(&path, []).unwrap();
    for (fields, field) in [
        ("bph=0", "bph"),
        ("lift=nan", "lift"),
        ("lift_angle=0", "lift"),
        ("escape_teeth=0", "escape_teeth"),
        ("card_ppm_fast=1000000", "card_ppm_fast"),
    ] {
        std::fs::write(
            root.join(MANIFEST),
            format!("{fields}\n[[recording]]\nfile=\"watch.wav\"\nposition=\"DU\"\n"),
        )
        .unwrap();
        for replay in [true, false] {
            let mut app = TimegrapherApp::with_devices(Vec::new(), None);
            if replay {
                app.start_replay(&path);
            } else {
                app.start_batch(&path);
            }
            assert!(app.capture.is_none() && app.batch.is_none());
            let (message, error) = app.message.as_ref().unwrap();
            assert!(*error);
            assert!(
                message.contains(MANIFEST) && message.contains(&format!("{field}:")),
                "{replay}: {message}"
            );
        }
    }
    // Applicability cannot be established from a syntactically corrupt file.
    std::fs::write(root.join(MANIFEST), "not valid TOML [").unwrap();
    for replay in [true, false] {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        if replay {
            app.start_replay(&path);
        } else {
            app.start_batch(&path);
        }
        assert!(app.capture.is_none() && app.batch.is_none());
        assert!(app
            .message
            .as_ref()
            .is_some_and(|(m, e)| *e && m.contains(MANIFEST)));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn valid_absent_and_unrelated_manifests_keep_actual_open_paths() {
    use std::time::Duration;
    use timegrapher_core::audio;
    let root = std::env::temp_dir().join(format!("tg-valid-manifest-open-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("watch.wav");
    // Empty valid WAV: no acoustic signal or device; enough to admit each path.
    audio::write_wav(
        &path,
        &audio::Audio {
            samples: Vec::new(),
            sample_rate: 48000,
        },
    )
    .unwrap();
    let valid = "lift_angle=52\nlift_known=false\ncard_ppm_fast=0\n[[recording]]\nfile=\"watch.wav\"\nposition=\"DU\"\n";
    let unrelated = "bph=0\nlift=nan\n[[recording]]\nfile=\"other.wav\"\nposition=\"DU\"\n";
    for text in [None, Some(valid), Some(unrelated)] {
        let manifest = root.join(MANIFEST);
        match text {
            Some(text) => std::fs::write(&manifest, text).unwrap(),
            None => {
                let _ = std::fs::remove_file(&manifest);
            }
        }
        if text == Some(unrelated) {
            assert!(read_manifest(&path).unwrap().is_none());
        }
        // A folder reads its own manifest when it names its recording (#655).
        assert_eq!(read_manifest(&root).unwrap().is_some(), text == Some(valid));
        for replay in [true, false] {
            let mut app = TimegrapherApp::with_devices(Vec::new(), None);
            if replay {
                app.start_replay(&path);
                let capture = app.capture.as_ref().expect("replay admitted");
                assert!(matches!(
                    capture.rx.recv_timeout(Duration::from_secs(5)),
                    Ok(Event::End)
                ));
            } else {
                app.start_batch(&path);
                let batch = app.batch.as_ref().expect("analysis admitted");
                // Completion (even a no-beats result) establishes that the
                // source reader closed before deleting the fixture.
                let _result = batch
                    .rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("empty batch completes");
            }
            assert!(!app.message.as_ref().is_some_and(|(_, error)| *error));
            assert_eq!(app.lift_deg, 52.0);
            assert!(!app.lift_known());
            app.stop();
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

/// A take of `seconds` of one watch at 28,800 bph, in a folder of its own
/// named `name`, with its session file.
fn one_position_take(name: &str, seconds: f64) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tg-app-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: seconds,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&dir.join("audio-001.wav"), &audio).unwrap();
    std::fs::write(
        dir.join(MANIFEST),
        "bph = 28800\nlift = 52.0\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n",
    )
    .unwrap();
    dir
}

/// A take's readings are readied off the window's thread with its
/// engine (a long take's took a minute there, #726), and are the ones the
/// window would read itself.
#[test]
fn an_opened_takes_readings_are_readied_off_the_window() {
    let dir = one_position_take("readied", 40.0);
    let mut app = reopened(&dir);
    let readied = app.trend.clone();
    assert!(readied.len() > 10, "{}", readied.len());
    assert_eq!(app.trend_key, (app.rev, app.average_s.to_bits()));
    app.rebuild_trend();
    assert_eq!(app.trend, readied);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A setting changed while a take opens holds once it is in, as it would
/// if changed just after: the readings are read again on it.
#[test]
fn a_setting_changed_while_a_take_opens_holds() {
    let dir = one_position_take("changed-opening", 40.0);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.start_batch(&dir);
    let was = app.average_s;
    let now = if was == 2.0 { 4.0 } else { 2.0 };
    app.set_average(now);
    let t = Instant::now();
    while app.batch.is_some() && t.elapsed().as_secs() < 120 {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // The charts, not readied at the new setting, are read by the next
    // frames in the background (#1014).
    let ctx = egui::Context::default();
    while app.trend_key != (app.rev, now.to_bits()) && t.elapsed().as_secs() < 120 {
        app.take_reads_shown(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let live = app.live.as_ref().expect("opened");
    assert_eq!(live.config().profile_s, now);
    assert_eq!(app.trend_key, (app.rev, now.to_bits()));
    let shown = app.trend.clone();
    app.rebuild_trend();
    assert_eq!(app.trend, shown);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A take opened from a folder of several sound files, whose session file
/// lists each file with its own position and masks in seconds into that
/// file, puts each where its file starts on the strip, as `session`
/// reads each file; masks saved go back to the file they are over, in its
/// own seconds (#1154).
#[test]
fn a_folder_takes_later_files_land_where_they_start() {
    let dir = std::env::temp_dir().join(format!("tg-folder-parts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Two files of 60 s each: the strip runs 0 to 120 s.
    let audio = timegrapher_core::audio::Audio {
        sample_rate: 100,
        samples: vec![0.0; 6000],
    };
    timegrapher_core::audio::write_wav(&dir.join("a.wav"), &audio).unwrap();
    timegrapher_core::audio::write_wav(&dir.join("b.wav"), &audio).unwrap();
    std::fs::write(
        dir.join(MANIFEST),
        "watch = 'W'\nbph = 28800\nlift = 52.0\n\n\
         [[recording]]\nfile = 'a.wav'\nposition = 'DU'\n\n\
         [[recording]]\nfile = 'b.wav'\nposition = 'DD'\nmasked = [[5.0, 10.0]]\n",
    )
    .unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.open_recording(&dir, false);
    let t = Instant::now();
    while app.batch.is_some() && t.elapsed().as_secs() < 60 {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        app.session.stretches(),
        [(0.0, Position::DialUp), (60.0, Position::DialDown)]
    );
    // Each file settles over its own start, as session reads it.
    assert_eq!(app.session.settles(), [(0.0, 20.0), (60.0, 80.0)]);
    assert_eq!(app.masked, [(65.0, 70.0)]);
    // A mask across the join is kept by both files, each its part.
    app.mask(55.0, 62.0);
    app.save_marks();
    let saved: Manifest =
        toml::from_str(&std::fs::read_to_string(dir.join(MANIFEST)).unwrap()).unwrap();
    let masks: Vec<(&str, Vec<[f64; 2]>)> = saved
        .recordings
        .iter()
        .map(|e| (e.file.as_str(), e.masked.clone()))
        .collect();
    assert_eq!(
        masks,
        [
            ("a.wav", vec![[55.0, 60.0]]),
            ("b.wav", vec![[0.0, 2.0], [5.0, 10.0]]),
        ]
    );
    // Opened again, the strip is as it was saved.
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.open_recording(&dir, false);
    let t = Instant::now();
    while again.batch.is_some() && t.elapsed().as_secs() < 60 {
        again.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(again.masked, [(55.0, 62.0), (65.0, 70.0)]);
    drop((app, again));
    std::fs::remove_dir_all(dir).unwrap();
}

/// Opening a recording puts its watch's settings in place before reading
/// it. A calibre among them that changes the beat rate doesn't read the
/// take on show again: that was the take before, whose own calibre read
/// the next one again, and so on until the stack ran out (#1241).
#[test]
fn a_recording_opened_after_a_take_with_a_calibre_opens() {
    let base = std::env::temp_dir().join(format!("tg-app-after-calibre-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let take = base.join("take");
    std::fs::create_dir_all(&take).unwrap();
    let synth = |s: f64| {
        generate(
            &SynthConfig {
                duration_s: s,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        )
    };
    timegrapher_core::audio::write_wav(&take.join("take.wav"), &synth(60.0)).unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "calibre = '3235'\nbph = 28800\nlift = 55.0\nsettle_s = 5.0\n\n\
         [[recording]]\nfile = 'take.wav'\nposition = 'CU'\nend_s = 20.0\n\n\
         [[recording]]\nfile = 'take.wav'\nposition = 'CD'\nstart_s = 20.0\nend_s = 30.0\n\n\
         [[recording]]\nfile = 'take.wav'\nposition = 'CU'\nstart_s = 30.0\n\
         masked = [[15.0, 40.0]]\n",
    )
    .unwrap();
    let wav = base.join("plain.wav");
    timegrapher_core::audio::write_wav(&wav, &synth(30.0)).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let opened = |app: &mut TimegrapherApp, path: &std::path::Path| {
        app.open_recording(path, true);
        let t = Instant::now();
        while app.batch.is_some() && t.elapsed().as_secs() < 120 {
            app.poll();
        }
        assert!(app.batch.is_none(), "{:?}", app.message);
    };
    opened(&mut app, &take);
    assert_eq!(app.calibre, "3235");
    assert_eq!(app.bph, Some(28800));
    opened(&mut app, &wav);
    assert_eq!(app.source_label, "plain.wav", "{:?}", app.message);
    // The plain file starts from Quick Listen's settings, not the take's.
    assert_eq!(app.calibre, "");
    assert_eq!(app.bph, None);
    // Listen putting the microphone's setup back over the take doesn't
    // read it again either.
    opened(&mut app, &take);
    app.stop();
    app.set_watch_setup(WatchSetup::default());
    assert!(app.batch.is_none(), "{:?}", app.message);
    assert_eq!(app.calibre, "");
    // Played through after the take, it is the one on show: the take
    // isn't read again behind it.
    opened(&mut app, &take);
    app.start_replay(&wav);
    assert!(app.batch.is_none(), "{:?}", app.message);
    assert_eq!(app.source_label, "plain.wav", "{:?}", app.message);
    assert_eq!(app.calibre, "");
    app.stop();
    let _ = std::fs::remove_dir_all(&base);
}

/// A take's own file opened after a plain one reads as it does opened
/// first: its watch on the status line and its calibre on the cards, with
/// nothing left of the file before's "No watch settings" (#1240, which
/// #1242's fix for the re-read loop also fixed).
#[test]
fn a_takes_file_opened_after_a_plain_one_names_its_watch() {
    let base = std::env::temp_dir().join(format!("tg-app-after-plain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let take = base.join("take");
    std::fs::create_dir_all(&take).unwrap();
    let synth = |s: f64| {
        generate(
            &SynthConfig {
                duration_s: s,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        )
    };
    timegrapher_core::audio::write_wav(&take.join("take.wav"), &synth(30.0)).unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "watch = 'Yacht-Master'\ncalibre = '3235'\nlift = 55.0\n\n\
         [[recording]]\nfile = 'take.wav'\nposition = 'CU'\n",
    )
    .unwrap();
    let wav = base.join("plain.wav");
    timegrapher_core::audio::write_wav(&wav, &synth(30.0)).unwrap();
    let opened = |app: &mut TimegrapherApp, path: &std::path::Path| {
        app.input = Input::File;
        app.open_recording(path, true);
        let t = Instant::now();
        while app.batch.is_some() && t.elapsed().as_secs() < 120 {
            app.poll();
        }
        assert!(app.batch.is_none(), "{:?}", app.message);
        (app.status_words(), app.watch_setup(), app.message.clone())
    };
    let mut fresh = TimegrapherApp::with_devices(Vec::new(), None);
    let first = opened(&mut fresh, &take.join("take.wav"));
    assert_eq!(first.0, "take.wav · Yacht-Master · Opened");
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    opened(&mut app, &wav);
    assert_eq!(opened(&mut app, &take.join("take.wav")), first);
    let _ = std::fs::remove_dir_all(&base);
}

/// A save or load dialog starts in the folder the take was last saved
/// in, and in the save folder until it has been saved.
#[test]
fn a_dialog_starts_where_the_take_was_saved() {
    let mut app = synthetic(2.0, 0.0);
    assert_eq!(app.save_start(), PathBuf::from(&app.save_dir));
    let saved = std::env::temp_dir().join("tg-app-saved-here").join("take");
    let metadata = app.session.metadata_snapshot(app.save_metadata());
    app.session.mark_saved(1.0, saved.clone(), metadata);
    assert_eq!(app.save_start(), saved.parent().unwrap());
}

/// Open Recording… asks for a file and Open Folder… for a folder. What is
/// picked is opened, unless it is a take outside the library, which
/// asks first; a cancel opens nothing.
#[test]
fn what_opens_dialog_picks_is_opened_or_asked_about() {
    let take = one_position_take("picked", 3.0);
    let root = std::env::temp_dir().join(format!("tg-app-picked-lib-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    drop(library::Library::create(&root).unwrap());
    let answer = |app: TimegrapherApp, want: Pick, picked: Vec<PathBuf>| {
        let app = std::sync::Mutex::new(app);
        assert!(crate::pick::answer(&app, |_, pick| {
            assert_eq!(pick, want);
            picked
        }));
        app.into_inner().unwrap()
    };
    // Cancelled: nothing opens.
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.open_from_menu(false);
    let app = answer(app, Pick::File, Vec::new());
    assert!(app.file_path.is_empty() && app.batch.is_none() && app.open_ask.is_none());
    // A recording already in the library: opened.
    let wav = root.join("audio-001.wav");
    std::fs::copy(take.join("audio-001.wav"), &wav).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.open_from_menu(false);
    let app = answer(app, Pick::File, vec![wav.clone()]);
    assert_eq!(app.file_path, wav.display().to_string());
    assert_eq!(app.input, Input::File);
    assert!(app.open_ask.is_none());
    assert!(app.batch.is_some(), "it is being read");
    // Let go of the library, or the next copy finds it held.
    drop(app);
    // A take's folder outside the library: asked about before it opens.
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.open_from_menu(true);
    let app = answer(app, Pick::Folder, vec![take.clone()]);
    assert_eq!(app.file_path, take.display().to_string());
    assert!(app.open_ask.is_some());
    assert!(app.batch.is_none(), "nothing read until answered");
    std::fs::remove_dir_all(take).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

/// A take whose session file gives a beat rate other than its calibre's
/// (the calibre edited since, or the file written outside the app) opens
/// at the file's rate, and the Watch line says the calibre beats at
/// another: the one way the app reaches that chip (Hugh, #1813). At
/// 1600 wide the line holds "1 limit" beside the watch's whole name,
/// where #1811 left only the bare chip.
#[test]
fn a_take_at_another_rate_than_its_calibre_says_so_on_the_watch_line() {
    let dir = one_position_take("calibre-rate", 40.0);
    std::fs::write(
        dir.join(MANIFEST),
        "watch = 'Pepsi'\ncalibre = 'Unitas 6497-2'\nbph = 28800\nlift = 52.0\n\n\
         [[recording]]\nfile = '.'\nposition = 'DU'\n",
    )
    .unwrap();
    let mut app = reopened(&dir);
    assert_eq!(app.calibre, "Unitas 6497-2");
    assert_eq!(app.bph, Some(28800));
    assert_eq!(app.cal.bph(&app.calibre), Some(21600));
    let ctx = super::tests::themed();
    let mut text = String::new();
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1600.0, 900.0),
            )),
            ..Default::default()
        };
        text = super::tests::painted_text(&ctx.run(input, |ctx| app.panels(ctx)));
    }
    assert!(
        text.lines().any(|l| l == "1 limit") && text.lines().any(|l| l == "Pepsi ⏷"),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An analysis whose thread ends without answering is a failure, said in
/// the status line, not a window left saying Analysing (#1891).
#[test]
fn an_analysis_that_stops_without_answering_says_it_failed() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let (tx, rx) = std::sync::mpsc::channel();
    app.batch = Some(Batch {
        rx,
        progress: Default::default(),
        cancel: Default::default(),
        duration_s: None,
        label: String::new(),
        started: Instant::now(),
        files: 1,
        first: String::new(),
        last: String::new(),
        stretches: Vec::new(),
        settles: Vec::new(),
        order_note: None,
    });
    app.poll_batch();
    assert!(app.batch.is_some(), "still analysing");
    drop(tx);
    app.poll_batch();
    assert!(app.batch.is_none(), "Analysing is down");
    let (said, bad) = app.message.clone().expect("a message");
    assert_eq!(said, "Analysis failed: it stopped part way");
    assert!(bad);
}
