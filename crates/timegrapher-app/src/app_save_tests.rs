//! Saving: folders of segments, saves that fail, and the questions
//! before unsaved sound goes.

use super::tests::{
    append_save_test_second, queued_capture, save_failure_fixture, synthetic, SETTLE_S,
};
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

/// After a move the watch settles for SETTLE_S: nothing is read then,
/// its beats are left out of the position's spans, and a move during a
/// settling starts it again.
#[test]
fn a_move_settles_before_it_is_read() {
    let mut app = synthetic(60.0, 0.0);
    app.session.set_settle_s(SETTLE_S);
    app.session
        .load(&[(0.0, Position::DialUp), (30.0, Position::DialDown)], &[]);
    app.position = Position::DialDown;
    app.session.start_settle(30.0);
    assert_eq!(app.session.settles(), [(30.0, 30.0 + SETTLE_S)]);
    assert!(!app.in_position(35.0) && app.in_position(45.0));
    assert_eq!(app.current_spans(), [(30.0 + SETTLE_S, f64::INFINITY)]);
    assert!(app.session.settling_left(35.0).is_some() && app.reading_at_time(35.0).is_none());
    assert!(app.session.settling_left(50.0).is_none() && app.reading_at_time(50.0).is_some());
    // Moved again 4 s in: the first settling ends there.
    app.session.start_settle(34.0);
    assert_eq!(
        app.session.settles(),
        [(30.0, 34.0), (34.0, 34.0 + SETTLE_S)]
    );
    assert_eq!(app.session.settle_to(), Some(34.0 + SETTLE_S));
}

/// The folder a take was saved into opens the take inside it; with two
/// takes inside, the message names them.
#[test]
fn a_save_folder_opens_the_take_inside() {
    let base = std::env::temp_dir().join(format!("tg-app-take-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let take = base.join("2026-10-11_DD3235");
    std::fs::create_dir_all(&take).unwrap();
    let tone = timegrapher_core::audio::Audio {
        samples: vec![0.0; 800],
        sample_rate: 8000,
    };
    timegrapher_core::audio::write_wav(&take.join("audio-001.wav"), &tone).unwrap();
    assert_eq!(take_folder(&base).unwrap(), take);
    assert_eq!(take_folder(&take).unwrap(), take);
    let other = base.join("2026-10-12_DD3235");
    std::fs::create_dir_all(&other).unwrap();
    timegrapher_core::audio::write_wav(&other.join("audio-001.wav"), &tone).unwrap();
    let e = take_folder(&base).unwrap_err();
    assert!(
        e.contains("2 takes") && e.contains("2026-10-12_DD3235"),
        "{e}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

/// A folder of segments is analysed in name order as one recording,
/// and its Steadiness tests cover the whole of it.
#[test]
fn a_folder_of_segments_is_analysed_as_one_recording() {
    let dir = std::env::temp_dir().join(format!("tg-app-folder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 360.0,
            rate_s_per_day: 8.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    // Three 2-minute segments, written out of name order, plus a file
    // that isn't a recording.
    let n = audio.samples.len() / 3;
    for (i, name) in [(2, "seg-03.wav"), (0, "seg-01.wav"), (1, "seg-02.wav")] {
        let part = timegrapher_core::audio::Audio {
            samples: audio.samples[i * n..(i + 1) * n].to_vec(),
            sample_rate: audio.sample_rate,
        };
        timegrapher_core::audio::write_wav(&dir.join(name), &part).unwrap();
    }
    std::fs::write(dir.join("notes.txt"), "not audio").unwrap();
    let files = recordings_in(&dir).unwrap();
    let names: Vec<_> = files
        .iter()
        .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["seg-01.wav", "seg-02.wav", "seg-03.wav"]);

    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    // Cancelled, the analysis stops without an answer.
    app.start_batch(&dir);
    let b = app.batch.as_ref().unwrap();
    assert_eq!((b.files, b.first.as_str()), (3, "seg-01.wav"));
    b.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let stopped = b.rx.recv_timeout(std::time::Duration::from_secs(60));
    assert!(
        matches!(
            stopped,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        ),
        "{stopped:?}"
    );
    app.batch = None;
    app.start_batch(&dir);
    let t = Instant::now();
    while app.batch.is_some() && t.elapsed().as_secs() < 120 {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let live = app.live.as_ref().expect("analysed");
    assert!(
        (live.duration_s() - 360.0).abs() < 1.0,
        "{}",
        live.duration_s()
    );
    assert!(app.source_label.contains("3 files"), "{}", app.source_label);
    let rate = live.reading(60.0).rate_s_per_day.unwrap();
    assert!((rate - 8.0).abs() < 2.0, "rate {rate}");
    // An empty folder says so instead of failing quietly.
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(recordings_in(&empty).is_err());
    assert!(take_folder(&empty).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Numbers in names count as numbers; a session file's list comes
/// first; files written out of the joining order are pointed out.
#[test]
fn folder_files_join_in_the_agreed_order() {
    use std::cmp::Ordering::Less;
    assert_eq!(natural_order("part2.wav", "part10.wav"), Less);
    assert_eq!(natural_order("take_part01-009", "take_part01-010"), Less);
    assert_eq!(natural_order("take_part01-099", "take_part02-001"), Less);
    assert_eq!(natural_order("A.wav", "b.wav"), Less);

    let dir = std::env::temp_dir().join(format!("tg-order-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let part = |name: &str| {
        let a = timegrapher_core::audio::Audio {
            samples: vec![0.0; 8000],
            sample_rate: 8000,
        };
        timegrapher_core::audio::write_wav(&dir.join(name), &a).unwrap();
    };
    for n in ["b-1.wav", "a-2.wav", "a-10.wav", "a-1.wav"] {
        part(n);
    }
    let names = |d: &Path| -> Vec<String> {
        recordings_in(d)
            .unwrap()
            .iter()
            .map(|f| file_name(f))
            .collect()
    };
    assert_eq!(names(&dir), ["a-1.wav", "a-2.wav", "a-10.wav", "b-1.wav"]);
    // Written within a second of each other, one second long each.
    assert_eq!(order_warning(&recordings_in(&dir).unwrap()), None);
    std::fs::write(
        dir.join(MANIFEST),
        "[[recording]]\nfile = \"b-1.wav\"\n\n[[recording]]\nfile = \"a-2.wav\"\n",
    )
    .unwrap();
    assert_eq!(names(&dir), ["b-1.wav", "a-2.wav", "a-1.wav", "a-10.wav"]);
    // a-2 written an hour before b-1, which is joined ahead of it.
    let f = std::fs::File::options()
        .write(true)
        .open(dir.join("a-2.wav"))
        .unwrap();
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    let w = order_warning(&recordings_in(&dir).unwrap()).unwrap();
    assert!(w.starts_with("a-2.wav was written before b-1.wav"), "{w}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn saved_file_bytes(dir: &Path) -> std::collections::BTreeMap<std::ffi::OsString, Vec<u8>> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().unwrap().is_file())
        .map(|e| (e.file_name(), std::fs::read(e.path()).unwrap()))
        .collect()
}

fn queue_segment_open_failure(
    app: &mut TimegrapherApp,
    base: &Path,
) -> std::sync::mpsc::Sender<Event> {
    let now = app.live.as_ref().unwrap().duration_s();
    app.session.pause(now).unwrap().unwrap();
    app.session.resume("test").unwrap();
    app.session.resumed_at(now);
    // A real, deterministic failure before writing the next segment.
    // This does not simulate a mid-sample write or a full disk.
    std::fs::create_dir(
        app.session
            .recorder
            .as_ref()
            .unwrap()
            .dir()
            .join("audio-002.wav"),
    )
    .unwrap();
    let tx = queued_capture(app, base);
    tx.send(Event::Audio(timegrapher_core::capture::Block {
        samples: vec![0.0; 960],
        at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(2),
    }))
    .unwrap();
    tx
}

#[test]
#[expect(
    clippy::cognitive_complexity,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn recording_failure_stops_capture_and_retains_scratch_with_or_without_input_error() {
    for input_error in [false, true] {
        let (mut app, base) = save_failure_fixture(if input_error {
            "recording-input"
        } else {
            "recording-only"
        });
        app.save_into(&base.join("saved"));
        app.wait_for_save();
        let saved = app.session.saved_to().unwrap().to_path_buf();
        let saved_bytes = saved_file_bytes(&saved);
        let scratch = app.session.recorder.as_ref().unwrap().dir().to_path_buf();
        let tx = queue_segment_open_failure(&mut app, &base);
        let before = app.live.as_ref().unwrap().duration_s();
        if input_error {
            tx.send(Event::Error("queued input failure".into()))
                .unwrap();
        }
        app.poll();
        assert!(!app.running());
        assert!(app.session.paused());
        assert!(app.session.recorder.is_some());
        assert!(!app.can_resume());
        assert!(!app.session.allows(Action::Save));
        assert_eq!(app.live.as_ref().unwrap().duration_s(), before);
        assert_eq!(app.session.recorder.as_ref().unwrap().duration_s(), 1.0);
        let fault = app.session.recording_fault().unwrap();
        assert_eq!(fault.recovery_dir, scratch);
        assert!(!fault.reason.contains("queued input failure"));
        assert!(fault.marker_error.is_none());
        assert!(scratch.join("RECORDING-ERROR.txt").is_file());
        assert_eq!(
            hound::WavReader::open(scratch.join("audio-001.wav"))
                .unwrap()
                .duration(),
            48000
        );
        assert!(
            app.needs_save_confirmation(),
            "even a saved, short faulted take needs a retention warning"
        );
        // Direct methods must not reach hardware, a file dialog, or a save.
        app.devices_listed = false;
        app.start_microphone();
        app.resume_microphone();
        assert!(!app.devices_listed);
        assert!(!app.running());
        app.save_into(&base.join("must-not-exist"));
        app.wait_for_save();
        app.save_again();
        app.wait_for_save();
        app.save_recording();
        app.wait_for_save();
        assert!(!base.join("must-not-exist").exists());
        assert_eq!(saved_file_bytes(&saved), saved_bytes);
        app.info_msg("Another notice".into());
        assert!(
            app.session.recording_fault().is_some(),
            "ordinary messages cannot dismiss the fault"
        );
        app.poll();
        assert!(!app.running());
        drop(tx);
        if input_error {
            eframe::App::on_exit(&mut app, None);
        } else {
            app.new_session();
        }
        assert!(scratch.join("audio-001.wav").is_file());
        assert_eq!(saved_file_bytes(&saved), saved_bytes);
        assert!(app.session.recorder.is_none());
        assert!(app.session.recording_fault().is_none());
        std::fs::remove_dir_all(base).unwrap();
    }
}

#[test]
fn input_only_failure_keeps_the_existing_resume_and_save_contract() {
    let (mut app, base) = save_failure_fixture("input-only");
    let tx = queued_capture(&mut app, &base);
    tx.send(Event::Error("input disconnected".into())).unwrap();
    app.poll();
    assert!(!app.running());
    assert!(app.can_resume());
    assert!(app.session.allows(Action::Save));
    assert!(app.session.recording_fault().is_none());
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    assert!(app.session.saved_to().unwrap().join(MANIFEST).is_file());
    app.new_session();
    drop(tx);
    std::fs::remove_dir_all(base).unwrap();
}

/// Sound that stops while listening says so in Settings > Microphone,
/// as an input that won't open does.
#[test]
fn sound_that_stops_while_listening_says_so_in_the_microphone_row() {
    let (mut app, base) = save_failure_fixture("input-fault");
    let tx = queued_capture(&mut app, &base);
    assert!(app.input_fault.is_none());
    tx.send(Event::Error("input disconnected".into())).unwrap();
    app.poll();
    assert!(!app.running());
    let fault = app.input_fault.clone().unwrap();
    assert!(fault.starts_with("Input stopped."), "{fault}");
    assert_eq!(app.message.as_ref().unwrap().0, fault);
    app.new_session();
    drop(tx);
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn pause_and_input_stop_close_failures_retain_scratch_and_report_marker_failure() {
    for input_stop in [false, true] {
        let (mut app, base) = save_failure_fixture(if input_stop {
            "input-close"
        } else {
            "pause-close"
        });
        let scratch = app.session.recorder.as_ref().unwrap().dir().to_path_buf();
        std::fs::remove_file(scratch.join("session.json")).unwrap();
        std::fs::create_dir(scratch.join("session.json")).unwrap();
        std::fs::create_dir(scratch.join("RECORDING-ERROR.txt")).unwrap();
        let tx = queued_capture(&mut app, &base);
        if input_stop {
            tx.send(Event::Error("input disconnected".into())).unwrap();
            app.poll();
        } else {
            app.pause();
        }
        assert!(!app.running());
        assert!(!app.can_resume());
        assert!(!app.session.allows(Action::Save));
        let fault = app.session.recording_fault().unwrap();
        assert!(fault.reason.contains("Couldn't close"));
        assert!(fault.marker_error.is_some());
        assert!(fault
            .description()
            .contains("Couldn't write the recording-error marker"));
        let message = &app.message.as_ref().unwrap().0;
        assert!(message.contains("Recording stopped"));
        if input_stop {
            assert!(message.contains("input disconnected"));
        }
        app.new_session();
        assert!(scratch.join("audio-001.wav").is_file());
        drop(tx);
        std::fs::remove_dir_all(base).unwrap();
    }
}

#[test]
fn persisted_lift_context_only_edit_stays_dirty_until_successful_save() {
    let (mut app, base) = save_failure_fixture("lift-context");
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    let dest = app.session.saved_to().unwrap().to_path_buf();
    let old = std::fs::read(dest.join(MANIFEST)).unwrap();
    let explicit: Manifest = toml::from_str("lift = 52\nlift_known = true").unwrap();
    app.apply_manifest(&explicit, &[], None);
    assert!(app.lift_known());
    assert!(
        app.metadata_unsaved(),
        "same value, different persisted context"
    );
    std::fs::remove_file(dest.join(MANIFEST)).unwrap();
    std::fs::create_dir(dest.join(MANIFEST)).unwrap();
    app.save_again();
    app.wait_for_save();
    assert!(app.metadata_unsaved());
    assert!(app.message.as_ref().is_some_and(|(_, error)| *error));
    std::fs::remove_dir(dest.join(MANIFEST)).unwrap();
    std::fs::write(dest.join(MANIFEST), old).unwrap();
    app.save_again();
    app.wait_for_save();
    assert!(!app.metadata_unsaved());
    let m: Manifest =
        toml::from_str(&std::fs::read_to_string(dest.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(m.lift_known, Some(true));
    app.session.discard_recording();
    std::fs::remove_dir_all(base).unwrap();
}

/// Save hands the copy to a thread of its own and returns: the session
/// isn't marked saved until the copy is written, listening carries on
/// meanwhile, and nothing that would start another save or throw the
/// sound away runs until it is done. The copy holds the sound up to
/// the Save.
#[test]
fn save_copies_in_the_background_while_listening_carries_on() {
    let (mut app, base) = save_failure_fixture("background");
    append_save_test_second(&mut app, 2);
    app.save_into(&base.join("saved"));
    assert!(
        app.saving.is_some(),
        "the copy is written on its own thread"
    );
    assert_eq!(
        app.session.saved_to(),
        None,
        "not saved until it is written"
    );
    append_save_test_second(&mut app, 3);
    app.save_again();
    app.new_session();
    assert!(
        app.session.recorder.is_some(),
        "the sound being copied is kept"
    );
    assert!(app.saving.is_some());
    app.wait_for_save();
    let dest = app.session.saved_to().unwrap().to_path_buf();
    let audio = timegrapher_core::audio::load(&dest.join("audio-001.wav")).unwrap();
    assert_eq!(audio.samples.len(), 2 * 48000);
    assert_eq!(app.unsaved_s(), 1.0);
    app.save_again();
    app.wait_for_save();
    assert_eq!(app.session.saved_to(), Some(dest.as_path()));
    let audio = timegrapher_core::audio::load(&dest.join("audio-001.wav")).unwrap();
    assert_eq!(audio.samples.len(), 3 * 48000);
    app.session.discard_recording();
    std::fs::remove_dir_all(base).unwrap();
}

/// The question before a new session, answered Save…, waits for the
/// save it started, then carries on.
#[test]
fn a_save_from_the_question_carries_on_once_it_is_written() {
    let (mut app, base) = save_failure_fixture("confirm-wait");
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    let dest = app.session.saved_to().unwrap().to_path_buf();
    for second in 2..=8 {
        append_save_test_second(&mut app, second);
    }
    assert!(app.needs_save_confirmation());
    app.confirm = Some(Confirm::NewSession);
    // What the question's Save… does.
    app.save_again();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let _ = ctx.run(Default::default(), |ctx| app.confirm_dialog(ctx));
    assert!(app.confirm.is_some(), "still waiting for the save");
    assert!(app.session.recorder.is_some());
    app.wait_for_save();
    let _ = ctx.run(Default::default(), |ctx| app.confirm_dialog(ctx));
    assert!(app.confirm.is_none());
    assert!(app.session.recorder.is_none(), "the new session started");
    let audio = timegrapher_core::audio::load(&dest.join("audio-001.wav")).unwrap();
    assert_eq!(audio.samples.len(), 8 * 48000);
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn saved_metadata_edits_warn_without_new_sound_and_reverts_are_clean() {
    let (mut app, base) = save_failure_fixture("metadata-edits");
    app.watch = "original".into();
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    app.session
        .pause(app.live.as_ref().unwrap().duration_s())
        .unwrap()
        .unwrap();
    assert!(!app.needs_save_confirmation());
    let original = app.save_metadata();
    app.watch = "renamed".into();
    assert!(app.metadata_unsaved());
    assert!(app.needs_save_confirmation());
    assert_eq!(app.unsaved_s(), 0.0);
    app.open_recording(&base.join("other.wav"), true);
    assert!(matches!(app.confirm, Some(Confirm::Open { .. })));
    assert!(app.session.recorder.is_some());
    app.confirm = None;
    app.watch = original.watch.clone();
    assert!(!app.needs_save_confirmation());
    app.bph = Some(21600);
    assert!(app.metadata_unsaved());
    app.bph = original.bph;
    app.set_lift(original.lift_deg + 1.0);
    assert!(app.metadata_unsaved());
    app.set_lift(original.lift_deg);
    app.calibre = "another calibre".into();
    assert!(app.metadata_unsaved());
    app.calibre = original.calibre.clone();
    app.hist_cumulative = !app.hist_cumulative;
    assert!(
        !app.metadata_unsaved(),
        "view preferences are not saved take edits"
    );
    app.change_position(Position::DialDown);
    app.change_position(Position::DialUp);
    assert!(
        app.metadata_unsaved(),
        "returning to DU retains appended position notes"
    );
    app.save_again();
    app.wait_for_save();
    assert!(!app.needs_save_confirmation());
    app.session.discard_recording();
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn short_audio_tail_does_not_become_an_unsaved_metadata_warning() {
    let (mut app, base) = save_failure_fixture("metadata-audio");
    app.change_position(Position::DialDown);
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    let ranges = app.take_manifest().recordings;
    append_save_test_second(&mut app, 2);
    assert_eq!(app.unsaved_s(), 1.0);
    assert_ne!(
        app.take_manifest().recordings.last().unwrap().start_s,
        ranges.last().unwrap().start_s
    );
    assert!(!app.metadata_unsaved());
    assert!(!app.needs_save_confirmation());
    app.session.discard_recording();
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn failed_metadata_only_save_retains_dirty_state_and_the_saved_bytes() {
    let (mut app, base) = save_failure_fixture("metadata-failure");
    let parent = base.join("saved");
    app.save_into(&parent);
    app.wait_for_save();
    let dest = app.session.saved_to().unwrap().to_path_buf();
    app.session
        .pause(app.live.as_ref().unwrap().duration_s())
        .unwrap()
        .unwrap();
    app.watch = "Updated watch".into();
    let previous = std::fs::read(dest.join(MANIFEST)).unwrap();
    std::fs::remove_file(dest.join(MANIFEST)).unwrap();
    std::fs::create_dir(dest.join(MANIFEST)).unwrap();
    let before = saved_file_bytes(&dest);
    app.save_again();
    app.wait_for_save();
    assert!(app.message.as_ref().is_some_and(|(_, error)| *error));
    assert!(app.metadata_unsaved());
    assert!(
        app.needs_save_confirmation(),
        "the confirmation must not continue after failure"
    );
    assert_eq!(app.unsaved_s(), 0.0);
    assert_eq!(app.session.saved_to(), Some(dest.as_path()));
    assert_eq!(saved_file_bytes(&dest), before);
    std::fs::remove_dir(dest.join(MANIFEST)).unwrap();
    std::fs::write(dest.join(MANIFEST), previous).unwrap();
    app.save_again();
    app.wait_for_save();
    assert!(!app.needs_save_confirmation());
    let manifest: Manifest =
        toml::from_str(&std::fs::read_to_string(dest.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(manifest.watch.as_deref(), Some("Updated watch"));
    let info: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dest.join("session.json")).unwrap()).unwrap();
    assert_eq!(info["watch"], "Updated watch");
    app.session.discard_recording();
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn completed_guided_metadata_is_unsaved_until_the_required_file_succeeds() {
    let (mut app, base) = save_failure_fixture("metadata-guided");
    let parent = base.join("saved");
    app.save_into(&parent);
    app.wait_for_save();
    let mut test = guided::Test::new();
    test.readings = Some(Vec::new());
    app.guided = Some(test);
    assert!(app.metadata_unsaved());
    let dest = app.session.saved_to().unwrap().to_path_buf();
    std::fs::create_dir(dest.join(guided::RESULTS_FILE)).unwrap();
    app.save_again();
    app.wait_for_save();
    assert!(app.metadata_unsaved());
    std::fs::remove_dir(dest.join(guided::RESULTS_FILE)).unwrap();
    app.save_again();
    app.wait_for_save();
    assert!(!app.metadata_unsaved());
    assert!(dest.join(guided::RESULTS_FILE).is_file());
    app.session.discard_recording();
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn successful_save_keeps_the_callers_path_for_messages_and_save_again() {
    let (mut app, base) = save_failure_fixture("caller-path");
    let parent = base.join("saved").join("..").join("saved");
    app.save_into(&parent);
    app.wait_for_save();
    let dest = app.session.saved_to().unwrap();
    assert_eq!(dest.parent().unwrap().as_os_str(), parent.as_os_str());
    assert_eq!(
        app.message.as_ref(),
        Some(&(
            format!("Saved {} of sound in {}", length_text(1.0), dest.display()),
            false
        ))
    );
    assert_eq!(app.save_dir, parent.display().to_string());
    assert!(dest.join(MANIFEST).is_file());
    app.session.discard_recording();
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn failed_required_manifest_does_not_report_or_mark_a_successful_save() {
    let (mut app, base) = save_failure_fixture("manifest");
    let parent = base.join("saved");
    // Prepare the destination name without marking an app save. A directory
    // at the required file path deterministically rejects the file write.
    let dest = app
        .session
        .recorder
        .as_mut()
        .unwrap()
        .save_copy(&parent, "", None)
        .unwrap();
    std::fs::create_dir(dest.join(MANIFEST)).unwrap();
    let before = saved_file_bytes(&dest);
    app.save_into(&parent);
    app.wait_for_save();
    assert!(app
        .message
        .as_ref()
        .is_some_and(|(m, error)| *error && m.contains(MANIFEST)));
    assert_eq!(app.session.saved_to(), None);
    assert_eq!(app.unsaved_s(), 1.0);
    assert_eq!(saved_file_bytes(&dest), before);
    std::fs::remove_dir(dest.join(MANIFEST)).unwrap();
    app.save_into(&parent);
    app.wait_for_save();
    assert_eq!(app.session.saved_to(), Some(dest.as_path()));
    assert_eq!(app.unsaved_s(), 0.0);
    let manifest: Manifest =
        toml::from_str(&std::fs::read_to_string(dest.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(manifest.recordings.len(), 1);
    app.session.discard_recording();
    assert_eq!(
        timegrapher_core::audio::load(&dest.join("audio-001.wav"))
            .unwrap()
            .samples
            .len(),
        48000
    );
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn failed_required_guided_result_preserves_the_previous_save_and_unsaved_sound() {
    let (mut app, base) = save_failure_fixture("guided");
    let parent = base.join("saved");
    app.save_into(&parent);
    app.wait_for_save();
    let dest = app.session.saved_to().unwrap().to_path_buf();
    let before = saved_file_bytes(&dest);
    append_save_test_second(&mut app, 2);
    // Only the finished-state marker is needed here: this test covers
    // required result persistence, not the measurement of guided positions.
    let mut test = guided::Test::new();
    test.readings = Some(Vec::new());
    app.guided = Some(test);
    std::fs::create_dir(dest.join(guided::RESULTS_FILE)).unwrap();
    app.save_into(&parent);
    app.wait_for_save();
    assert!(app
        .message
        .as_ref()
        .is_some_and(|(m, error)| *error && m.contains(guided::RESULTS_FILE)));
    assert_eq!(app.session.saved_to(), Some(dest.as_path()));
    assert_eq!(app.unsaved_s(), 1.0);
    assert_eq!(saved_file_bytes(&dest), before);
    std::fs::remove_dir(dest.join(guided::RESULTS_FILE)).unwrap();
    app.save_into(&parent);
    app.wait_for_save();
    assert_eq!(app.unsaved_s(), 0.0);
    let kept = std::fs::read(dest.join(guided::RESULTS_FILE)).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&kept).unwrap();
    assert_eq!(json["format"], guided::RESULTS_FORMAT);
    // A later unfinished test (like a 24-hour half) supplies no new full-wind
    // result. Preserve the earlier result as a separate comparison artifact.
    app.guided = Some(guided::Test::new());
    app.save_into(&parent);
    app.wait_for_save();
    assert_eq!(
        std::fs::read(dest.join(guided::RESULTS_FILE)).unwrap(),
        kept
    );
    app.session.discard_recording();
    assert_eq!(
        timegrapher_core::audio::load(&dest.join("audio-001.wav"))
            .unwrap()
            .samples
            .len(),
        96000
    );
    let manifest: Manifest =
        toml::from_str(&std::fs::read_to_string(dest.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(manifest.recordings.len(), 1);
    std::fs::remove_dir_all(base).unwrap();
}

/// A microphone session's sound is kept as it goes, can be saved at
/// any point, and goes with a new session.
#[test]
fn sessions_keep_save_and_discard_their_sound() {
    let base = std::env::temp_dir().join(format!("tg-app-save-{}", std::process::id()));
    let mut app = synthetic(12.0, 20.0);
    app.session.listen();
    let info = app.info("test", 48000, 16);
    let mut r = Recorder::start(&base.join("kept"), info).unwrap();
    r.write(&timegrapher_core::capture::Block {
        samples: vec![0.0; 48000 * 6],
        at: std::time::SystemTime::now(),
    })
    .unwrap();
    let kept = r.dir().to_path_buf();
    app.session.recorder = Some(r);
    assert!(app.unsaved_s() > 5.0, "unsaved sound to ask about");
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    let saved = app
        .session
        .saved_to()
        .map(Path::to_path_buf)
        .expect("saved");
    assert!(saved.join("audio-001.wav").is_file());
    assert_eq!(app.unsaved_s(), 0.0);
    // Paused, a session can carry on; a new session forgets it.
    app.session.set_state(session::State::Paused {
        since: Instant::now(),
    });
    assert!(app.can_resume());
    app.new_session();
    assert!(app.live.is_none() && app.session.recorder.is_none() && !app.can_resume());
    assert!(!kept.exists(), "the unsaved copy is thrown away");
    assert!(
        saved.join("audio-001.wav").is_file(),
        "the saved copy stays"
    );
    let _ = std::fs::remove_dir_all(&base);
}

/// Opening a recording over a stopped microphone session with unsaved
/// sound asks first and keeps the sound; Recording chosen sets the
/// session aside instead of showing it as a recording.
#[test]
fn opening_a_recording_asks_before_unsaved_sound_goes() {
    let base = std::env::temp_dir().join(format!("tg-app-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = synthetic(5.0, 0.0);
    app.session.listen();
    let info = app.info("test", 48000, 16);
    let mut r = Recorder::start(&base, info).unwrap();
    r.write(&timegrapher_core::capture::Block {
        samples: vec![0.0; 48000 * 20],
        at: std::time::SystemTime::now(),
    })
    .unwrap();
    app.session.recorder = Some(r);
    app.session.set_state(session::State::Paused {
        since: Instant::now(),
    });
    app.input = Input::File;
    assert!(app.mic_session_set_aside());
    let elsewhere = base.join("other.wav");
    app.open_recording(&elsewhere, true);
    assert!(matches!(app.confirm, Some(Confirm::Open { .. })));
    assert!(app.session.recorder.is_some() && app.unsaved_s() > 19.0);
    app.input = Input::Microphone;
    assert!(!app.mic_session_set_aside());
    let _ = std::fs::remove_dir_all(&base);
}
