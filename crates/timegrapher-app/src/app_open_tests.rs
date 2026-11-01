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
        ("card_ppm=-1000000", "card_ppm"),
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
    let valid = "lift_angle=52\nlift_known=false\ncard_ppm=0\n[[recording]]\nfile=\"watch.wav\"\nposition=\"DU\"\n";
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
        // Existing exact-path applicability does not attach child-file
        // metadata to the folder view itself.
        assert!(read_manifest(&root).unwrap().is_none());
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
