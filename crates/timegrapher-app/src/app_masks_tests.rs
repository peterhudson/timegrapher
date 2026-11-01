//! Masks and marks: masked stretches, the position brush, painting on a
//! take and saving what was painted, and averages across a mask.

use super::tests::{
    opened_recording, painted_text, queued_capture, reopened, screen, synthetic, themed,
};
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

#[test]
fn fit_shows_the_whole_session() {
    let mut app = synthetic(40.0, 0.0);
    app.strip_end = Some(10.0);
    app.fit_strip();
    assert!(app.strip_fit);
    assert_eq!(app.strip_end, None);
    assert!((app.strip.span_s - app.latest_s()).abs() < 1e-9);
    assert!(app.latest_s() > 35.0);
}

/// Painting on a take opened lights Save, whose menu offers Save
/// Changes first, and the status bar says the changes aren't saved.
#[test]
fn painting_on_an_opened_take_lights_save() {
    let base = std::env::temp_dir().join(format!("tg-app-paint-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = opened_recording(&base, "reverso_DU.wav", 30.0);
    app.brush = Some(Position::CrownLeft);
    app.paint_position(10.0, 20.0);
    assert!(app.marks_unsaved());
    let ctx = themed();
    let status = ctx.run(screen(), |ctx| {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| app.status_bar(ui));
    });
    assert!(painted_text(&status).contains("Changes not saved"));
    let mut time = 0.0;
    let mut pass = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        time += 0.25;
        let input = egui::RawInput {
            time: Some(time),
            events,
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.save_menu(ui, 32.0));
        });
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((
                    t.galley.text().to_string(),
                    t.galley.rect.translate(t.pos.to_vec2()).center(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let click = |at: egui::Pos2| {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![egui::Event::PointerMoved(at), button(true), button(false)]
    };
    pass(&mut app, Vec::new());
    pass(&mut app, click(egui::pos2(8.0 + 16.0, 8.0 + 16.0)));
    let drawn = pass(&mut app, Vec::new());
    let at = drawn
        .iter()
        .find(|t| t.0 == "Save Changes")
        .unwrap_or_else(|| panic!("{drawn:?}"))
        .1;
    pass(&mut app, click(at));
    pass(&mut app, Vec::new());
    assert!(!app.marks_unsaved(), "{:?}", app.message);
    let _ = std::fs::remove_dir_all(&base);
}

/// After a Positions Test, Start a New Test starts a new take, asking
/// first about sound not saved; it never carries on the stopped one.
#[test]
fn a_new_test_after_one_is_a_new_take() {
    let base = std::env::temp_dir().join(format!("tg-app-new-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "test".into());
    app.session.listen();
    let info = app.info("", 48000, 16);
    let mut r = Recorder::start(&base.join("unsaved"), info).unwrap();
    r.write(&timegrapher_core::capture::Block {
        samples: vec![0.0; 48000 * 20],
        at: std::time::SystemTime::now(),
    })
    .unwrap();
    app.session.recorder = Some(r);
    app.session.set_state(session::State::Paused {
        since: Instant::now(),
    });
    app.guided = Some(guided::Test::new());
    app.positions_take = true;
    assert!(app.can_resume());
    app.start_guided(None);
    assert!(!app.running(), "the stopped take isn't carried on");
    assert!(matches!(
        app.confirm,
        Some(Confirm::NewTake(NewTake::Positions))
    ));
    assert!(app.guided.is_some(), "nothing let go before the answer");
    let _ = std::fs::remove_dir_all(&base);
}

/// Masks on a lone recording are asked about, and saved to a new
/// session file that makes its folder a take, written as a take
/// recorded here with the same settings would be; they come back when
/// it is opened again.
#[test]
fn masks_on_a_lone_recording_are_saved_beside_it() {
    let base = std::env::temp_dir().join(format!("tg-app-mask-lone-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = opened_recording(&base, "reverso_DD.wav", 30.0);
    assert!(!app.needs_save_confirmation(), "nothing changed yet");
    app.mask(10.0, 14.0);
    assert!(app.marks_unsaved());
    app.open_recording(&base.join("reverso_DD.wav"), true);
    assert!(
        matches!(app.confirm, Some(Confirm::Open { .. })),
        "asked first"
    );
    app.confirm = None;
    app.save_marks();
    assert!(!app.marks_unsaved());
    let text = std::fs::read_to_string(base.join(MANIFEST)).unwrap();
    let m: Manifest = toml::from_str(&text).unwrap();
    // What the recorder writes for a take with these settings: the
    // default lift is written as assumed, not chosen.
    let mut want = app.manifest_header();
    want.recordings = vec![RecordingEntry {
        file: ".".into(),
        position: Some("DD".into()),
        settle_s: Some(0.0),
        masked: vec![[10.0, 14.0]],
        ..Default::default()
    }];
    assert_eq!(text, toml::to_string_pretty(&want).unwrap());
    assert_eq!(m.lift_known, Some(false));
    assert_eq!(m.calibre, None);
    // The app reads the folder, and the file in it, as a take.
    for open in [base.clone(), base.join("reverso_DD.wav")] {
        let again = reopened(&open);
        assert_eq!(again.masked, [(10.0, 14.0)]);
        assert_eq!(again.position, Position::DialDown);
        assert!(!again.needs_save_confirmation());
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// Starting the microphone, or a Positions Test, over a recording whose
/// masks changed asks first, and keeps them until listening starts.
#[test]
fn listening_asks_before_unsaved_masks_go() {
    let base = std::env::temp_dir().join(format!("tg-app-mask-mic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = opened_recording(&base, "a.wav", 12.0);
    app.mask(2.0, 5.0);
    app.start_microphone();
    assert!(matches!(app.confirm, Some(Confirm::Microphone)));
    assert_eq!(app.masked, [(2.0, 5.0)], "nothing let go yet");
    // A Positions Test asked for waits on the same answer.
    app.confirm = None;
    app.start_guided(None);
    assert!(matches!(app.confirm, Some(Confirm::Microphone)));
    assert_eq!(app.guided_after_ask, Some(None));
    // Answered, with an input that can't be opened: the masks stay on
    // show, still to save.
    app.devices_listed = true;
    app.device = Some("no such input".into());
    app.confirm = None;
    app.listen_after_ask();
    assert!(!app.running());
    assert_eq!(app.masked, [(2.0, 5.0)]);
    assert!(app.marks_unsaved());
    assert_eq!(app.guided_after_ask, None);
    assert!(!app.masks_let_go);
    let _ = std::fs::remove_dir_all(&base);
}

/// A lone recording beside others isn't made a take of: saving its
/// masks is refused and nothing is written.
#[test]
fn masks_are_not_saved_into_a_folder_of_other_recordings() {
    let base = std::env::temp_dir().join(format!("tg-app-mask-others-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    opened_recording(&base, "a.wav", 12.0);
    let mut app = opened_recording(&base, "b.wav", 12.0);
    app.mask(2.0, 5.0);
    app.save_marks();
    assert!(app.marks_unsaved(), "still unsaved");
    assert!(!base.join(MANIFEST).exists());
    let said = app
        .message
        .as_ref()
        .map(|m| m.0.clone())
        .unwrap_or_default();
    assert!(said.contains("1 other recording"), "{said}");
    let _ = std::fs::remove_dir_all(&base);
}

/// Saving masks to a take changes only `masked` on the entries for what
/// was opened: comments, keys this app doesn't read and other entries
/// stay, and the file still reads as the take it was.
#[test]
fn masks_saved_to_a_take_leave_the_rest_of_its_file() {
    let base = std::env::temp_dir().join(format!("tg-app-mask-take-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let original = "# A hand-written note\n\
        watch = \"Test\"\n\
        lift = 52.0\n\
        owner = \"kept\" # and its note\n\n\
        [[recording]]\n\
        file = \"take.wav\"\n\
        position = \"CU\"\n\
        masked = [[1.0, 2.0]]\n\n\
        [[recording]]\n\
        file = \"other.wav\"\n\
        position = \"CD\"\n";
    std::fs::write(base.join(MANIFEST), original).unwrap();
    let mut app = opened_recording(&base, "take.wav", 20.0);
    assert_eq!(app.masked, [(1.0, 2.0)]);
    app.mask(6.0, 9.5);
    app.save_marks();
    assert!(!app.marks_unsaved());
    let text = std::fs::read_to_string(base.join(MANIFEST)).unwrap();
    assert_eq!(
        text,
        original.replace("[[1.0, 2.0]]", "[[1.0, 2.0], [6.0, 9.5]]"),
        "only the masks changed"
    );
    let (_, entries) = read_manifest(&base.join("take.wav")).unwrap().unwrap();
    assert_eq!(entries[0].masked, [[1.0, 2.0], [6.0, 9.5]]);
    // Every mask brought back: the key goes.
    app.set_masked(Vec::new());
    app.save_marks();
    let text = std::fs::read_to_string(base.join(MANIFEST)).unwrap();
    assert_eq!(text, original.replace("masked = [[1.0, 2.0]]\n", ""));
    assert!(!base.join(".session.toml.saving").exists());
    let _ = std::fs::remove_dir_all(&base);
}

/// The Position Brush on a recording opened from disk: a stretch
/// painted is asked about before it goes, and Save puts one entry per
/// stretch in its session file in place of the entries for what was
/// opened, each keeping the rest of what the first said; the take opens
/// with them again. Comments, keys this app doesn't read and the other
/// recording's entry stay.
/// Positions painted on a take in a watch and saved show in the
/// Watches view's row for it at once.
#[test]
fn a_painted_take_in_a_watch_shows_what_was_saved() {
    let base = std::env::temp_dir().join(format!("tg-app-paint-row-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    drop(lib);
    let take = root
        .join("Pepsi")
        .join(library::TAKES)
        .join("2026-10-25 10.00 Dial Up");
    std::fs::create_dir_all(&take).unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "lift = 52.0\n\n[[recording]]\nfile = \"take.wav\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let mut app = opened_recording(&take, "take.wav", 80.0);
    app.use_library(Some(root.clone()));
    let positions = |app: &TimegrapherApp| match &app.library {
        Ok(Some(l)) => l.find("Pepsi").unwrap().takes[0].positions.clone(),
        _ => panic!("no library"),
    };
    assert_eq!(positions(&app), ["DU"]);
    app.brush = Some(Position::CrownUp);
    app.paint_position(10.04, 40.0);
    app.save_marks();
    assert!(!app.marks_unsaved(), "{:?}", app.message);
    assert_eq!(positions(&app), ["DU", "CU"]);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn positions_painted_on_a_take_are_saved_to_it() {
    let base = std::env::temp_dir().join(format!("tg-app-paint-take-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let original = "# A hand-written note\n\
        watch = \"Test\"\n\
        lift = 52.0\n\n\
        [[recording]]\n\
        file = \"take.wav\"\n\
        position = \"DU\"\n\
        notes = \"kept\" # and its note\n\n\
        [[recording]]\n\
        file = \"other.wav\"\n\
        position = \"CD\"\n";
    std::fs::write(base.join(MANIFEST), original).unwrap();
    let mut app = opened_recording(&base, "take.wav", 80.0);
    assert_eq!(app.position, Position::DialUp);
    assert!(!app.marks_unsaved());
    app.brush = Some(Position::CrownUp);
    app.paint_position(10.04, 40.0);
    assert!(app.marks_unsaved());
    assert_eq!(app.position, Position::DialUp, "Dial Up again after it");
    assert_eq!(
        app.session.stretches(),
        [
            (0.0, Position::DialUp),
            (10.0, Position::CrownUp),
            (40.0, Position::DialUp)
        ]
    );
    // The readings are cut at each change: none reaches across one.
    let breaks = app.live.as_ref().unwrap().breaks().to_vec();
    assert!(
        breaks.contains(&10.0) && breaks.contains(&40.0),
        "{breaks:?}"
    );
    app.open_recording(&base.join("take.wav"), true);
    assert!(
        matches!(app.confirm, Some(Confirm::Open { .. })),
        "asked first"
    );
    app.confirm = None;
    app.save_marks();
    assert!(!app.marks_unsaved());
    let text = std::fs::read_to_string(base.join(MANIFEST)).unwrap();
    let entry = |pos: &str, times: &str, comment: bool| {
        format!(
            "[[recording]]\nfile = \"take.wav\"\nposition = \"{pos}\"\nnotes = \"kept\"{}\n{times}\n",
            if comment { " # and its note" } else { "" }
        )
    };
    // Painted, so nothing settles at either edge; what was recorded,
    // Dial Up throughout, is kept under it. The take's own settling
    // at its start is cut where the paint begins (the design guide's
    // mark 51), so its Dial Up before the paint is all settling and
    // nothing of it is read.
    let want = format!(
        "# A hand-written note\nwatch = \"Test\"\nlift = 52.0\n\n{}\n{}\n{}\n\
         [[recording]]\nfile = \"other.wav\"\nposition = \"CD\"\n\n\
         [[recorded]]\nposition = \"DU\"\n",
        entry("DU", "start_s = 10.0\nend_s = 10.0\nsettle_s = 0.0", true),
        entry("CU", "start_s = 10.0\nend_s = 40.0\nsettle_s = 0.0", false),
        entry("DU", "start_s = 40.0\nsettle_s = 0.0", false),
    );
    assert_eq!(text, want);
    // That Dial Up has nothing to read, which every surface leaves
    // out of the take's readings rather than failing on it.
    let saved: Manifest = toml::from_str(&text).unwrap();
    let read: Vec<bool> = saved
        .recordings
        .iter()
        .filter(|e| e.file == "take.wav")
        .map(timegrapher_core::take::reads_nothing)
        .collect();
    assert_eq!(read, [true, false, false]);
    let mut again = reopened(&base.join("take.wav"));
    assert_eq!(again.session.stretches(), app.session.stretches());
    assert_eq!(again.session.settles(), app.session.settles());
    assert_eq!(
        again.session.painted_spans(again.position),
        [(10.0, 40.0, Position::CrownUp)]
    );
    assert!(!again.marks_unsaved());
    // Remove Painting, opened again: Dial Up throughout, as recorded,
    // and saved so, with nothing kept under it.
    again.remove_painting(20.0);
    assert_eq!(again.session.stretches(), [(0.0, Position::DialUp)]);
    assert!(again.marks_unsaved());
    again.save_marks();
    let text = std::fs::read_to_string(base.join(MANIFEST)).unwrap();
    assert!(!text.contains("[[recorded]]"), "{text}");
    assert_eq!(text.matches("take.wav").count(), 1, "{text}");
    let _ = std::fs::remove_dir_all(&base);
}

/// A change of position dragged moves with its settling; painting to
/// the newest beat puts the watch in that position from then on; the
/// brush is put down by Esc (as Mask is) and by the Mask tool, and isn't offered
/// during a Positions Test.
#[test]
fn the_position_brush_paints_and_moves_changes() {
    let mut app = synthetic(60.0, 0.0);
    app.brush = Some(Position::CrownLeft);
    app.paint_position(20.0, 30.0);
    app.move_position_change(20.0, 15.0);
    assert_eq!(app.session.stretches()[1], (15.0, Position::CrownLeft));
    assert!(
        app.session.settles().is_empty(),
        "painting starts no settling"
    );
    let latest = app.latest_s();
    app.paint_position(40.0, latest - 0.5);
    assert_eq!(app.position, Position::CrownLeft, "to the end");
    assert_eq!(
        app.session.stretches().last(),
        Some(&(40.0, Position::CrownLeft))
    );
    // Esc puts it down.
    let ctx = themed();
    let esc = |pressed| egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Default::default(),
    };
    let _ = ctx.run(
        egui::RawInput {
            events: vec![esc(true), esc(false)],
            ..Default::default()
        },
        |ctx| app.panels(ctx),
    );
    assert_eq!(app.brush, None);
    // Esc puts the Mask tool down as well.
    app.masking = true;
    let _ = ctx.run(
        egui::RawInput {
            events: vec![esc(true), esc(false)],
            ..Default::default()
        },
        |ctx| app.panels(ctx),
    );
    assert!(!app.masking, "Esc puts Mask down");
    // Picking the Mask tool puts it down too.
    app.brush = Some(Position::DialUp);
    app.masking = true;
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            theme::pane_toolbar(ui, "Paper Strip", &[], |ui| app.strip_settings(ui))
        });
    });
    assert_eq!(app.brush, None);
    assert!(app.masking);
    // Not during a Positions Test.
    app.masking = false;
    app.brush = Some(Position::DialUp);
    app.guided_measured = vec![(0.0, 10.0)];
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            theme::pane_toolbar(ui, "Paper Strip", &[], |ui| app.strip_settings(ui))
        });
    });
    assert_eq!(app.brush, None);
}

/// Full-wind results loaded from a file stay while the test waits, the
/// minutes passing, with no watch as with a watch's own tests on offer.
#[test]
fn results_loaded_from_a_file_stay_while_the_test_waits() {
    let full = guided::FullWind {
        format: guided::RESULTS_FORMAT.to_string(),
        began_utc: "2026-10-25T09:00:00.000Z".into(),
        began_unix: 1_792_918_800.0,
        watch: "Pepsi".into(),
        calibre: String::new(),
        bph: None,
        lift_angle_deg: None,
        settle_s: None,
        measure_s: None,
        positions: vec![guided::KeptRow {
            position: "DU".into(),
            rate_s_per_day: Some(4.0),
            amplitude_deg: None,
            beat_error_ms: None,
        }],
    };
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.new_take(NewTake::Positions, false);
    let now = guided::unix_now();
    app.find_full_wind_at(now);
    // As Load Full-Wind Results… leaves it.
    app.guided_earlier = Some(full.clone());
    app.guided_earlier_file = true;
    for later in [60.0, 120.0, 3600.0] {
        app.find_full_wind_at(now + later);
        assert_eq!(app.guided_earlier.as_ref(), Some(&full), "{later} s on");
    }
}

/// Cancel Test leaves nothing of the test set up: the full-wind
/// results loaded for it, with no watch, are gone from the Bench.
#[test]
fn a_cancelled_test_leaves_no_full_wind_results() {
    let full = guided::FullWind {
        format: guided::RESULTS_FORMAT.to_string(),
        began_utc: "2026-10-25T09:00:00.000Z".into(),
        began_unix: 1_792_918_800.0,
        watch: "Daytona".into(),
        calibre: String::new(),
        bph: None,
        lift_angle_deg: None,
        settle_s: None,
        measure_s: None,
        positions: Vec::new(),
    };
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.new_take(NewTake::Positions, false);
    app.find_full_wind_at(guided::unix_now());
    app.guided_earlier = Some(full.clone());
    app.guided_earlier_file = true;
    app.leave_ready_test();
    assert_eq!(app.test_ready, None);
    assert_eq!(app.guided_earlier, None);
    assert!(!app.guided_earlier_file);
    // Set up again, it starts with nothing loaded.
    app.new_take(NewTake::Positions, false);
    app.find_full_wind_at(guided::unix_now());
    assert_eq!(app.guided_earlier, None);
}

/// Load Full-Wind Results… reads the results the test just after
/// winding saved, and the next test runs set against them; a file
/// that isn't such results says so and loads nothing.
#[test]
fn full_wind_results_load_and_the_next_test_is_set_against_them() {
    let base = std::env::temp_dir().join(format!("tg-app-full-wind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let full = guided::FullWind {
        format: guided::RESULTS_FORMAT.to_string(),
        began_utc: "2026-10-25T09:00:00.000Z".into(),
        began_unix: 1_792_918_800.0,
        watch: "Pepsi".into(),
        calibre: "ETA 2824-2".into(),
        bph: Some(28_800),
        lift_angle_deg: Some(52.0),
        settle_s: Some(10.0),
        measure_s: Some(60.0),
        positions: ["DU", "DD", "CD", "CL", "CU", "CR"]
            .into_iter()
            .map(|p| guided::KeptRow {
                position: p.into(),
                rate_s_per_day: Some(4.0),
                amplitude_deg: Some(290.0),
                beat_error_ms: Some(0.3),
            })
            .collect(),
    };
    let path = base.join(guided::RESULTS_FILE);
    std::fs::write(&path, full.to_json()).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let read = app.read_full_wind(&path);
    assert_eq!(read.as_ref(), Some(&full));
    // Not results: said, and nothing loaded.
    let bad = base.join("other.json");
    std::fs::write(&bad, "{}").unwrap();
    assert!(app.read_full_wind(&bad).is_none());
    assert!(app
        .message
        .as_ref()
        .is_some_and(|(m, error)| *error && m.contains("other.json")));
    // Loaded, the next test runs against them.
    app.guided_earlier = read;
    app.reset_session(48000, "test".into());
    app.session.listen();
    let _tx = queued_capture(&mut app, &base);
    app.start_guided(app.guided_earlier.clone());
    let test = app.guided.as_ref().expect("the test runs");
    assert_eq!(test.full.as_ref().map(|f| &f.0), Some(&full));
    app.capture = None;
    let _ = std::fs::remove_dir_all(&base);
}

/// A Positions Test's take, run here or opened later, takes no
/// Position Brush: the test set its positions. Another take opened
/// takes it again.
#[test]
fn a_positions_test_take_takes_no_brush() {
    let mut app = synthetic(60.0, 0.0);
    let ctx = themed();
    let draw = |app: &mut TimegrapherApp| {
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                theme::pane_toolbar(ui, "Paper Strip", &[], |ui| app.strip_settings(ui))
            });
        });
    };
    let dir = std::env::temp_dir().join(format!("tg-app-positions-brush-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let take = dir.join("take.flac");
    // A plain take: the brush stays in the hand.
    app.load_reserve(&take);
    assert!(!app.positions_take);
    app.brush = Some(Position::DialUp);
    draw(&mut app);
    assert_eq!(app.brush, Some(Position::DialUp));
    // A Positions Test's results beside it: put down, and greyed.
    std::fs::write(dir.join(guided::RESULTS_FILE), "{}").unwrap();
    app.load_reserve(&take);
    assert!(app.positions_take);
    assert!(!app.in_test(), "the test itself is over");
    draw(&mut app);
    assert_eq!(app.brush, None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The Position Brush is put down and greyed while a Power Reserve
/// Test runs, as for a Positions Test: the run keeps one position.
#[test]
fn the_brush_waits_for_a_power_reserve_run() {
    let mut app = synthetic(60.0, 0.0);
    let ctx = themed();
    let draw = |app: &mut TimegrapherApp| {
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                theme::pane_toolbar(ui, "Paper Strip", &[], |ui| app.strip_settings(ui))
            });
        });
    };
    app.brush = Some(Position::DialUp);
    draw(&mut app);
    assert_eq!(app.brush, Some(Position::DialUp));
    let mut t = reserve::Test::new(Position::DialUp, true, String::new(), 0.0);
    let beats: Vec<timegrapher_core::beats::Beat> = (0..100)
        .map(|i| timegrapher_core::beats::Beat {
            index: i,
            time: 1.0 + i as f64 * 0.125,
            quality: 1.0,
        })
        .collect();
    t.update(&beats, 0.125, 20.0);
    app.reserve = Some(t);
    assert!(app.reserve_running());
    draw(&mut app);
    assert_eq!(app.brush, None);
    app.brush = Some(Position::CrownUp);
    draw(&mut app);
    assert_eq!(app.brush, None, "it stays down while the run goes on");
}

/// After a masked stretch the charts stay empty until a whole
/// averaging time has been heard again: no reading over the few beats
/// just past the mask, and no line drawn across it.
#[test]
fn readings_resume_a_whole_average_after_a_mask() {
    let mut app = synthetic(60.0, 0.0);
    app.mask(20.0, 30.0);
    let after: Vec<_> = app
        .trend
        .iter()
        .filter(|p| p.t > 20.0 && p.rate.is_some())
        .map(|p| p.t)
        .collect();
    assert!(!after.is_empty());
    assert!(
        after.iter().all(|&t| t >= 30.0 + app.average_s - 1e-9),
        "{after:?}"
    );
    let points: Vec<[f64; 2]> = app
        .trend
        .iter()
        .filter_map(|p| Some([p.t, p.rate?]))
        .collect();
    let parts = segments(&points);
    assert_eq!(parts.len(), 2, "one line each side of the mask");
    assert!(parts[0].last().unwrap()[0] < 20.0);
}

/// On a slow beat (12,000 bph) and a short average the beats' span falls up to a
/// beat short of the averaging time; that still counts as covered, so
/// clean readings aren't left off the chart.
#[test]
fn slow_beats_fill_a_short_average() {
    let cfg = SynthConfig {
        duration_s: 30.0,
        bph: 12000,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let mut live = LiveAnalyzer::new(audio.sample_rate, LiveConfig::default());
    for b in audio.samples.chunks(960) {
        live.push(b);
    }
    let points = trend_over(&live, 2.0);
    // The last point, at the newest sound, isn't settled yet.
    let end = live.duration_s() - 1.0;
    let late: Vec<_> = points.iter().filter(|p| p.t > 8.0 && p.t < end).collect();
    assert!(!late.is_empty());
    // None left off that the engine read.
    let left_off: Vec<f64> = late
        .iter()
        .filter(|p| p.rate.is_none() && live.reading_at(p.t, 2.0).rate_s_per_day.is_some())
        .map(|p| p.t)
        .collect();
    assert!(left_off.is_empty(), "{left_off:?} of {}", late.len());
}

#[test]
fn a_line_breaks_where_its_points_stop() {
    let pts = [[0.0, 1.0], [1.0, 1.0], [2.0, 1.0], [10.0, 1.0], [11.0, 1.0]];
    let parts = segments(&pts);
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[1], [[10.0, 1.0], [11.0, 1.0]]);
    assert_eq!(segments(&[]).len(), 0);
}

/// A take listened to for more than a day keeps its first hours: the
/// app keeps as many beats as a power reserve run does (a 26 h take at
/// 28,800 bph lost its first 1 h 40 min to the engine's day), and the
/// strip can be that long.
#[test]
fn a_long_take_keeps_its_first_hours() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let kept = app.live_config().max_beats;
    assert!(kept >= reserve::RUN_BEATS_KEPT);
    assert!(kept as f64 >= 80.0 * 3600.0 * 8.0, "{kept}");
    app.strip.set_span(26.0 * 3600.0);
    assert_eq!(app.strip.span_s, 26.0 * 3600.0);
    app.strip.set_span(1e9);
    assert_eq!(app.strip.span_s, power_reserve::LIMIT_S);
}

/// A long take's charts worked out again keep a reading every 20 s or
/// so, and those read live since come every half second: the line runs
/// on through both, and the value under the pointer is found in both,
/// where the line used to break at every one of the sparse readings.
#[test]
fn a_line_runs_on_where_its_spacing_changes() {
    let mut pts: Vec<[f64; 2]> = (0..4000).map(|i| [i as f64 * 20.0, 1.0]).collect();
    let t0 = pts.last().unwrap()[0];
    pts.extend((1..20_000).map(|i| [t0 + i as f64 * 0.5, 2.0]));
    assert_eq!(segments(&pts).len(), 1);
    assert_eq!(value_at(&pts, 1009.0), Some(1.0));
    assert_eq!(value_at(&pts, t0 + 100.2), Some(2.0));
    // A stretch with nothing read is still a gap, in either.
    let holed: Vec<[f64; 2]> = pts
        .iter()
        .copied()
        .filter(|p| !(1000.0..1100.0).contains(&p[0]))
        .collect();
    assert_eq!(segments(&holed).len(), 2);
    assert_eq!(value_at(&holed, 1050.0), None);
}

/// Masked stretches are saved in the take's own seconds, which leave
/// out pauses, and come back from its session file on the strip.
#[test]
fn masked_stretches_are_saved_with_the_take() {
    let base = std::env::temp_dir().join(format!("tg-app-masked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.session.listen();
    let info = app.info("test", 100, 16);
    app.session.recorder = Some(Recorder::start(&base, info).unwrap());
    let write = |app: &mut TimegrapherApp, s: usize, at: u64| {
        app.session
            .recorder
            .as_mut()
            .unwrap()
            .write(&timegrapher_core::capture::Block {
                samples: vec![0.0; s * 100],
                at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(at),
            })
            .unwrap();
    };
    write(&mut app, 60, 60);
    app.session.pause(60.0).unwrap().unwrap();
    app.session.resume("test").unwrap();
    app.session.resumed_at(120.0);
    write(&mut app, 10, 130);
    let before = app.save_metadata();
    app.set_masked(vec![(125.0, 128.0), (10.0, 20.0)]);
    assert_ne!(app.save_metadata(), before, "masking is an edit to save");
    let m = app.take_manifest();
    // Every entry lists every mask, so none is lost outside a stretch.
    assert!(!m.recordings.is_empty());
    for r in &m.recordings {
        assert_eq!(r.masked, [[10.0, 20.0], [65.0, 68.0]]);
    }
    let text = toml::to_string_pretty(&m).unwrap();
    let again: Manifest = toml::from_str(&text).unwrap();
    let mut opened = TimegrapherApp::with_devices(Vec::new(), None);
    opened.apply_manifest(&again, &again.recordings, None);
    assert_eq!(opened.masked, [(10.0, 20.0), (65.0, 68.0)]);
    // Nothing masked: no masked line in the file.
    app.set_masked(Vec::new());
    assert!(!toml::to_string_pretty(&app.take_manifest())
        .unwrap()
        .contains("masked"));
    app.session.discard_recording();
    let _ = std::fs::remove_dir_all(&base);
}
