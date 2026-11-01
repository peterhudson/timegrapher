//! Keeping takes: the Watches pane, keeping a recording or a Quick Listen
//! in a watch, moving and deleting takes, and a quit that can't ask.

use super::tests::synthetic;
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

/// Quitting without the close being asked first (on macOS, Cmd+Q and
/// Quit in the app menu or the Dock) keeps the unsaved sound, and the
/// next start offers it back; a quit the user answered throws it away
/// as before. A folder another copy is recording into is never offered.
/// The Watches view lists the library's watches, adds one (making
/// the library for the first), shows a watch's details and takes, and
/// Save… then saves into that watch with no question.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn the_watches_pane_adds_a_watch_and_saves_its_takes_into_it() {
    let base = std::env::temp_dir().join(format!("tg-app-watches-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut app = synthetic(5.0, 0.0);
    app.use_library(Some(root.clone()));
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let pane = |ctx: &egui::Context, app: &mut TimegrapherApp| {
        let mut all = String::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.pane_tools(ui, Pane::Watch);
                    app.watch_pane(ui);
                });
                app.watch_form_dialog(ctx);
            });
            all = format!("{:?} {:?}", out.shapes, theme::tools_drawn(ctx));
        }
        all
    };
    let drawn = pane(&ctx, &mut app);
    assert!(
        drawn.contains("No watches yet") && drawn.contains("New Watch…"),
        "{drawn}"
    );
    // Nothing is made on disk until the first watch.
    assert!(!root.exists());

    let mut w = library::Watch::new("Pepsi");
    w.lift = Some(53.0);
    w.bph = Some(28800);
    w.notes = Some(String::new());
    app.watch_form = Some(watches::Form {
        watch: w,
        new: true,
        error: None,
    });
    let form = pane(&ctx, &mut app);
    for t in ["New Watch", "Add Field", "Add Watch", "Lift Angle"] {
        assert!(form.contains(t), "{t}: {form}");
    }
    assert!(!form.contains("Archived"), "no Archived mark: {form}");
    // Nothing on show: the new watch's details are the next take's.
    // (Readings on show keep theirs: see
    // a_recording_on_show_is_kept_in_a_new_watch_with_its_charts.)
    app.new_session();
    app.save_watch_form();
    assert!(app.watch_form.is_none());
    assert!(library::Library::is_library(&root));
    assert_eq!(
        (app.watch.as_str(), app.lift_deg, app.bph),
        ("Pepsi", 53.0, Some(28800))
    );
    let drawn = pane(&ctx, &mut app);
    for t in [
        "Pepsi",
        "No takes yet",
        "New Take",
        "Edit…",
        "Add Photos…",
        "Listen",
        "Positions Test",
        "Power Reserve Test",
    ] {
        assert!(drawn.contains(t), "{t}: {drawn}");
    }
    assert_eq!(app.status_words(), "New Take · Pepsi · Not Started");

    // Save… with the watch picked saves into it, named by its start.
    let scratch = base.join("unsaved");
    app.session.listen();
    let info = app.info("Pepsi", 48000, 16);
    let mut r = Recorder::start(&scratch, info).unwrap();
    r.write(&timegrapher_core::capture::Block {
        samples: vec![0.0; 48000 * 20],
        at: std::time::SystemTime::now(),
    })
    .unwrap();
    let started = watches::local_minute(r.started_utc()).unwrap();
    app.session.recorder = Some(r);
    app.session.set_state(session::State::Paused {
        since: Instant::now(),
    });
    app.save_again();
    app.wait_for_save();
    let takes = root.join("Pepsi").join(library::TAKES);
    let saved: Vec<PathBuf> = std::fs::read_dir(&takes)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(saved.len(), 1, "{saved:?}");
    let name = saved[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with(&started), "{name}");
    assert!(saved[0].join(MANIFEST).is_file());
    assert_eq!(app.session.saved_to(), Some(saved[0].as_path()));
    let drawn = pane(&ctx, &mut app);
    // Its row says when it started, as its folder's name does.
    let row = watches::started_words(&app.open_group().unwrap().takes[0]);
    assert!(drawn.contains(&row), "{row}: {drawn}");
    // Save Again saves into the same take.
    app.save_again();
    app.wait_for_save();
    assert_eq!(std::fs::read_dir(&takes).unwrap().count(), 1);

    // The list says so; a name already in use is refused.
    app.watch_open = None;
    let drawn = pane(&ctx, &mut app);
    assert!(drawn.contains("1 take"), "{drawn}");
    app.watch_form = Some(watches::Form {
        watch: library::Watch::new("pepsi"),
        new: true,
        error: None,
    });
    app.save_watch_form();
    let refused = app.watch_form.as_ref().and_then(|f| f.error.clone());
    assert!(refused.is_some_and(|e| e.contains("already")));
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// A take whose sound is open while it is read for the Watches list
/// isn't moved, renamed or deleted until it is done (Windows refuses
/// to while a file in it is open): the app says to wait.
#[test]
fn a_take_being_read_for_the_list_waits_to_be_moved() {
    let app = TimegrapherApp::with_devices(Vec::new(), None);
    let take = std::env::temp_dir().join("Takes").join("2026-10-20 14.32");
    assert_eq!(app.take_busy(&take, false), None);
    app.take_readings.set_now(Some(take.clone()));
    for keep in [false, true] {
        let why = app.take_busy(&take, keep).unwrap();
        assert!(why.contains("being read for the Watches list"), "{why}");
    }
    app.take_readings
        .set_now(Some(take.with_file_name("another")));
    assert_eq!(app.take_busy(&take, false), None);
}

/// A take's row in the Watches list shows the figures core's
/// `library::reading` gives for that take, worked out from its sound:
/// the app's path to them, not only a reading handed to the table.
#[test]
fn a_takes_row_shows_its_own_reading_from_core() {
    use timegrapher_core::synth::{generate, SynthConfig};
    let base = std::env::temp_dir().join(format!("tg-app-take-reading-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let dir = lib.add(&library::Watch::new("Pepsi")).unwrap();
    let id = lib.watches[0].watch.id.clone();
    drop(lib);
    // A take as the app saves one: 40 s of a watch 10 s/d fast at
    // 270°, its session file, and other figures in session.json.
    let take = dir.join(library::TAKES).join("2026-10-20 14.32 Dial Up");
    std::fs::create_dir_all(&take).unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "bph = 28800\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 40.0,
            rate_s_per_day: 10.0,
            beat_error_ms: 0.6,
            ..Default::default()
        },
        |_| 270.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&take.join("take_part01-001.wav"), &audio).unwrap();
    std::fs::write(
        take.join("session.json"),
        r#"{"started_utc":"2026-10-20T21:32:00Z","duration_s":40.0,
            "result":{"rate_s_per_day":4.2,"amplitude_deg":285.0,"beat_error_unlock_ms":-0.6}}"#,
    )
    .unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.pick_watch(&id);
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let pane = |app: &mut TimegrapherApp| {
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.watch_pane(ui));
        });
        format!("{:?}", out.shapes)
    };
    pane(&mut app);
    app.take_readings.wait();
    let drawn = pane(&mut app);
    let want = library::reading(&take).unwrap();
    assert_eq!(app.take_readings.reading(&take), Some(&want));
    let t = app.open_watch().unwrap().takes[0].clone();
    let cells = watches::take_cells(&t, None, Some(&want));
    for c in &cells[3..6] {
        assert!(!c.is_empty() && drawn.contains(c.as_str()), "{c}: {drawn}");
    }
    // Not the figures the take was saved with.
    assert!(!cells[3..6].contains(&"+4.2".to_string()), "{cells:?}");
    // A report written into the take's folder (as `timegrapher
    // session` leaves one) changes its size but not what its reading
    // reads: the figures stay on show while it is asked again.
    let report = take.join(timegrapher_core::take::REPORT_DIR);
    std::fs::create_dir_all(&report).unwrap();
    std::fs::write(report.join("summary.json"), "{}").unwrap();
    app.take_readings.want(&take, t.bytes + 2, &ctx);
    assert_eq!(app.take_readings.reading(&take), Some(&want));
    app.take_readings.wait();
    assert_eq!(app.take_readings.reading(&take), Some(&want));
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// A watch's photos lead its row in the list and show as a strip in
/// its details, read in the background; one that can't be read says
/// so by name. Delete… asks, then deletes only that photo.
#[test]
fn a_watchs_photos_show_as_thumbnails_and_one_can_be_deleted() {
    let base = std::env::temp_dir().join(format!("tg-app-photos-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let dir = lib.add(&library::Watch::new("Pepsi")).unwrap();
    let id = lib.watches[0].watch.id.clone();
    let data = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("photos");
    let broken = base.join("broken.jpg");
    std::fs::write(&broken, b"not a photo").unwrap();
    lib.add_photos(&id, &[data.join("plain.heic"), broken])
        .unwrap();
    drop(lib);
    let photos = dir.join(library::PHOTOS);
    let (good, bad) = (photos.join("plain.heic"), photos.join("broken.jpg"));
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let pane = |ctx: &egui::Context, app: &mut TimegrapherApp| {
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.pane_tools(ui, Pane::Watch);
                app.watch_pane(ui);
            });
            app.take_ask_dialog(ctx);
        });
        format!("{:?} {:?}", out.shapes, theme::tools_drawn(ctx))
    };
    // The list asks for the first photo (by name: broken.jpg) only.
    pane(&ctx, &mut app);
    app.pick_watch(&id);
    let ready = |app: &mut TimegrapherApp, p: &Path| {
        matches!(app.thumbs.get(&ctx, p), photos::Thumb::Ready(_))
    };
    let failed = |app: &mut TimegrapherApp, p: &Path| {
        matches!(app.thumbs.get(&ctx, p), photos::Thumb::Failed(_))
    };
    for _ in 0..200 {
        pane(&ctx, &mut app);
        if ready(&mut app, &good) && failed(&mut app, &bad) {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(ready(&mut app, &good) && failed(&mut app, &bad));
    let drawn = pane(&ctx, &mut app);
    for t in ["Add Photos…", "broken.jpg", "Can't be shown"] {
        assert!(drawn.contains(t), "{t}: {drawn}");
    }
    app.take_ask = Some(watches::Ask::DeletePhoto { photo: bad.clone() });
    pane(&ctx, &mut app);
    let asked = pane(&ctx, &mut app);
    assert!(asked.contains("Delete \\\"broken.jpg\\\"?"), "{asked}");
    let ask = app.take_ask.take().unwrap();
    app.answer_take_ask(ask);
    assert!(!bad.exists() && good.exists());
    assert_eq!(app.open_watch().unwrap().photos, [good]);
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn open_adds_a_recording_to_a_watch_asking_only_for_what_is_missing() {
    let base = std::env::temp_dir().join(format!("tg-app-open-into-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let wav = |p: &Path| {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(p, spec).unwrap();
        for _ in 0..8000 {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
    };
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.pick_watch(&pepsi);

    // A lone file: its position is in its name, the rest is asked for.
    let lone = base.join("outside").join("pepsi_DU.wav");
    wav(&lone);
    let mut a = app.open_ask_for(&lone).unwrap();
    assert!(a.add && a.watch.as_deref() == Some(pepsi.as_str()));
    let need = app.open_need(&a);
    assert_eq!(
        need,
        OpenNeed {
            bph: true,
            lift: true,
            position: false,
            started: true
        }
    );
    let ctx = egui::Context::default();
    theme::install(&ctx);
    app.open_ask = Some(a.clone());
    let mut text = String::new();
    for _ in 0..2 {
        let out = ctx.run(egui::RawInput::default(), |ctx| app.open_ask_dialog(ctx));
        text = format!("{:?}", out.shapes);
    }
    for asked in ["Beat Rate", "Lift Angle", "Started", "Add and Open"] {
        assert!(text.contains(asked), "{asked}");
    }
    assert!(!text.contains("Choose a Position"));
    app.open_ask = None;
    a.bph = Some(21600);
    a.started = "2026-10-22 14:30".into();
    app.answer_open_ask(a, need);
    assert!(app.open_ask.is_none());
    let Ok(Some(lib)) = &app.library else {
        panic!()
    };
    let takes = &lib.find("Pepsi").unwrap().takes;
    assert_eq!(takes.len(), 1);
    assert!(takes[0].name.starts_with("2026-10-22 14.30"));
    let take = lib.find("Pepsi").unwrap().take_dir(&takes[0]);
    let m: Manifest =
        toml::from_str(&std::fs::read_to_string(take.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!((m.bph, m.lift_known), (Some(21600), Some(false)));
    assert_eq!(m.watch.as_deref(), Some("Pepsi"));
    assert_eq!(m.recordings[0].position.as_deref(), Some("DU"));
    assert!(takes[0].started_utc.is_some() && take.join("pepsi_DU.wav").is_file());
    assert!(lone.is_file());

    // A take folder: only what its session file leaves out is written,
    // and the rest of the file is kept as it was.
    let folder = base.join("outside take");
    wav(&folder.join("take_part01-001.wav"));
    std::fs::write(
        folder.join(MANIFEST),
        "# by hand\nlift = 50.0\n\n[[recording]]\nfile = \"take_part01-001.wav\"\n",
    )
    .unwrap();
    let mut a = app.open_ask_for(&folder).unwrap();
    let need = app.open_need(&a);
    assert_eq!(
        need,
        OpenNeed {
            bph: true,
            lift: false,
            position: true,
            started: true
        }
    );
    a.bph = Some(28800);
    a.position = Some(Position::CrownLeft);
    a.started = "2026-10-22 15:00".into();
    app.answer_open_ask(a, need);
    let Ok(Some(lib)) = &app.library else {
        panic!()
    };
    let entry = lib.find("Pepsi").unwrap();
    assert_eq!(entry.takes.len(), 2);
    let take = entry.dir.join("takes").join("outside take");
    let text = std::fs::read_to_string(take.join(MANIFEST)).unwrap();
    assert!(text.starts_with("# by hand"));
    let m: Manifest = toml::from_str(&text).unwrap();
    assert_eq!((m.bph, m.lift), (Some(28800), Some(50.0)));
    assert_eq!(m.recordings[0].position.as_deref(), Some("CL"));
    assert!(started_in(&take).is_some());
    // What is in the watches already opens straight away.
    assert!(app.open_ask_for(&take).is_none());
    let _ = std::fs::remove_dir_all(&base);
}

/// Keep in a Watch… of a recording on show asks only what the take
/// on show doesn't say: not its position, beat rate or lift angle,
/// which are written with it. And Keep stays greyed out until what is
/// asked is given, for an existing watch as for a new one.
#[test]
fn keeping_a_recording_asks_only_what_is_not_on_show() {
    let base = std::env::temp_dir().join(format!("tg-app-keep-shown-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    // No position in its name, no session file: nothing it says.
    let lone = base.join("outside").join("bench.wav");
    std::fs::create_dir_all(lone.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&lone, spec).unwrap();
    for _ in 0..8000 {
        w.write_sample(0i16).unwrap();
    }
    w.finalize().unwrap();
    let mut app = synthetic(5.0, 0.0);
    app.use_library(Some(root.clone()));
    app.input = Input::File;
    app.opened_path = lone.display().to_string();
    app.position = Position::CrownLeft;
    app.bph = Some(28800);
    app.lift_deg = 50.0;

    // Opening it from outside asks for all of it, and Add and Open
    // waits for it even with a watch picked.
    let mut a = app.open_ask_for(&lone).unwrap();
    a.watch = Some(pepsi.clone());
    a.new_watch = false;
    a.add = true;
    // Its start, guessed from the file, left out to see it asked for.
    a.started.clear();
    let need = app.open_need(&a);
    assert!(need.bph && need.position && need.started);
    assert!(!TimegrapherApp::open_ready(&a, need));
    a.bph = Some(21600);
    a.position = Some(Position::DialUp);
    assert!(!TimegrapherApp::open_ready(&a, need));
    a.started = "2026-10-22 14:30".into();
    assert!(TimegrapherApp::open_ready(&a, need));

    // Keep in a Watch: only when it started is asked (guessed from
    // the file, to be checked).
    app.keep_into(&pepsi);
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut text = String::new();
    for _ in 0..2 {
        let out = ctx.run(egui::RawInput::default(), |ctx| app.open_ask_dialog(ctx));
        text = format!("{:?}", out.shapes);
    }
    assert!(text.contains("Started"), "{text}");
    for not_asked in ["Choose a Position", "Choose a Beat Rate", "Lift Angle"] {
        assert!(!text.contains(not_asked), "{not_asked}");
    }
    // The watch shows while asked; Esc (as Cancel) puts back what was
    // picked before, here no watch.
    assert_eq!(app.watch_open.as_deref(), Some(pepsi.as_str()));
    let esc = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let _ = ctx.run(esc, |ctx| app.open_ask_dialog(ctx));
    assert!(app.open_ask.is_none());
    assert_eq!(app.watch_open, None);
    app.keep_into(&pepsi);
    let mut a = app.open_ask.take().expect("asked when it started");
    a.started.clear();
    assert!(!TimegrapherApp::open_ready(&a, app.open_need(&a)));
    a.started = "2026-10-22 14:30".into();
    let need = app.open_need(&a);
    assert!(TimegrapherApp::open_ready(&a, need));
    app.answer_open_ask(a, need);
    let Ok(Some(lib)) = &app.library else {
        panic!()
    };
    let e = lib.find("Pepsi").unwrap();
    let take = e.take_dir(&e.takes[0]);
    let m: Manifest =
        toml::from_str(&std::fs::read_to_string(take.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!((m.bph, m.lift), (Some(28800), Some(50.0)));
    assert_eq!(m.recordings[0].position.as_deref(), Some("CL"));
    let _ = std::fs::remove_dir_all(&base);
}

/// One part of a take of several, opened alone, is kept in a watch
/// alone, with what is on show; a take of one part goes in whole.
#[test]
fn one_part_of_a_take_is_kept_alone() {
    let base = std::env::temp_dir().join(format!("tg-app-keep-part-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let take = base.join("outside").join("run");
    std::fs::create_dir_all(&take).unwrap();
    std::fs::write(take.join("session.json"), "{}").unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let part = |n: usize| {
        let p = take.join(format!("run_part{n:02}.wav"));
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        for _ in 0..8000 {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
        p
    };
    let first = part(1);
    let mut app = synthetic(5.0, 0.0);
    app.use_library(Some(root.clone()));
    app.input = Input::File;
    app.opened_path = first.display().to_string();
    // Alone in its folder, the take goes in whole.
    app.keep_into(&pepsi);
    assert!(
        matches!(&app.take_ask, Some(watches::Ask::Add { from, .. }) if *from == take),
        "{:?}",
        app.take_ask
    );
    app.take_ask = None;
    // One of two: it goes in alone, asked as a recording.
    part(2);
    app.keep_into(&pepsi);
    assert!(app.take_ask.is_none(), "{:?}", app.take_ask);
    let shown = app.open_ask.as_ref().map(|a| a.path.clone());
    assert!(shown.is_none() || shown == Some(first.clone()), "{shown:?}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_recording_on_show_is_kept_in_a_new_watch_with_its_charts() {
    let base = std::env::temp_dir().join(format!("tg-app-keep-new-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let lone = base.join("outside").join("meteorite_DU.wav");
    std::fs::create_dir_all(lone.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&lone, spec).unwrap();
    for _ in 0..8000 {
        w.write_sample(0i16).unwrap();
    }
    w.finalize().unwrap();
    // A recording opened from outside the watches, its readings on show.
    let mut app = synthetic(5.0, 0.0);
    app.use_library(Some(root.clone()));
    app.input = Input::File;
    app.opened_path = lone.display().to_string();
    let shown = (app.trend.len(), app.bph, app.lift_deg, app.watch.clone());
    assert!(shown.0 > 0);
    assert_eq!(app.addable_take(), Some(lone.clone()));

    // Keep in a Watch… > New Watch…, filled in, then Done.
    app.open_new_watch_form();
    app.keep_after_form = true;
    let f = app.watch_form.as_mut().unwrap();
    f.watch.name = "Meteorite".into();
    f.watch.bph = Some(21600);
    f.watch.lift = Some(50.0);
    app.save_watch_form();
    assert!(app.watch_form.is_none() && !app.keep_after_form);
    // The readings on show keep the settings they were measured with.
    assert_eq!(
        (app.trend.len(), app.bph, app.lift_deg, app.watch.clone()),
        shown
    );
    let id = app.open_watch().unwrap().watch.id.clone();
    // Then it goes into the new watch, asking only what neither says.
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut text = String::new();
    for _ in 0..2 {
        let out = ctx.run(egui::RawInput::default(), |ctx| app.open_ask_dialog(ctx));
        text = format!("{:?}", out.shapes);
    }
    assert!(
        text.contains("Keep \\\"meteorite_DU.wav\\\" in a Watch"),
        "{text}"
    );
    assert!(
        text.contains("Started") && !text.contains("Just Open It"),
        "{text}"
    );
    let mut a = app.open_ask.take().expect("asked what is missing");
    assert!(a.add && !a.new_watch && a.watch.as_deref() == Some(id.as_str()));
    a.started = "2026-10-23 10:50".into();
    let need = app.open_need(&a);
    app.answer_open_ask(a, need);
    let Ok(Some(lib)) = &app.library else {
        panic!()
    };
    let e = lib.find("Meteorite").unwrap();
    assert_eq!(e.takes.len(), 1);
    let take = e.take_dir(&e.takes[0]);
    assert!(take.join("meteorite_DU.wav").is_file() && lone.is_file());
    assert!(Path::new(&app.opened_path).starts_with(&root));
    assert_eq!(app.addable_take(), None);

    // File > New Session lets go of the recording: a new take follows.
    app.new_session();
    assert!(app.input == Input::Microphone && app.opened_path.is_empty());
    assert_eq!(app.status_words(), "New Take · Meteorite · Not Started");
    // And the watch's settings apply to it (set up as New Take does,
    // without opening a real input).
    app.take_setup();
    assert_eq!(
        (app.watch.as_str(), app.bph, app.lift_deg),
        ("Meteorite", Some(21600), 50.0)
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[expect(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn quick_listen_keeps_its_take_in_unsorted_until_filed() {
    let base = std::env::temp_dir().join(format!("tg-app-quick-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.pick_watch(&pepsi);
    assert_eq!(app.watch, "Pepsi");
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let pane = |ctx: &egui::Context, app: &mut TimegrapherApp| {
        let mut all = String::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.pane_tools(ui, Pane::Watch);
                    app.watch_pane(ui);
                });
            });
            all = format!("{:?} {:?}", out.shapes, theme::tools_drawn(ctx));
        }
        all
    };
    // A session with 20 s of sound not yet saved, paused.
    let record = |app: &mut TimegrapherApp, scratch: &Path| {
        app.session.listen();
        let info = app.info("", 48000, 16);
        let mut r = Recorder::start(scratch, info).unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 48000 * 20],
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        app.session.recorder = Some(r);
        app.session.set_state(session::State::Paused {
            since: Instant::now(),
        });
    };
    record(&mut app, &base.join("unsaved-1"));
    assert!(!app.quick_listen_ready(false));
    assert!(matches!(app.confirm, Some(Confirm::QuickListen)));
    assert!(!app.quick_listen && app.watch_open.is_some(), "nothing yet");
    // Answered (no input is opened here: the test touches no device).
    app.confirm = None;
    assert!(app.quick_listen_ready(true));
    assert!(app.session.recorder.is_none(), "the unsaved session went");
    assert!(app.quick_listen && app.unsorted_open && app.watch_open.is_none());
    assert_eq!(app.watch, "");
    assert!(!app.running());
    // The list, Unsorted Takes marked; its row shows its takes.
    assert!(app.list_shown());
    let drawn = pane(&ctx, &mut app);
    for t in ["Unsorted Takes", "Quick Listen, no watch", "Pepsi"] {
        assert!(drawn.contains(t), "{t}: {drawn}");
    }
    app.list_for = None;
    let drawn = pane(&ctx, &mut app);
    for t in ["Unsorted Takes", "No takes here", "Takes"] {
        assert!(drawn.contains(t), "{t}: {drawn}");
    }

    // Stop keeps nothing; Save… saves the take into Unsorted, and a
    // Stop after that adds the sound since to it.
    record(&mut app, &base.join("unsaved-2"));
    app.stop_listening();
    app.wait_for_save();
    let unsorted = root.join(library::UNSORTED).join(library::TAKES);
    let none = std::fs::read_dir(&unsorted).map_or(0, |d| d.count());
    assert_eq!(none, 0, "a Quick Listen take isn't kept at Stop");
    // Nor asked about: it goes without a question unless saved.
    assert!(!app.needs_save_confirmation());
    assert!(app.quick_unsaved());
    // The Listen card's Save offers Unsorted Takes until it is saved.
    assert!(app.saves_into_unsorted());
    app.save_again();
    app.wait_for_save();
    let saved: Vec<PathBuf> = std::fs::read_dir(&unsorted)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert_eq!(app.session.saved_to(), Some(saved[0].as_path()));
    assert!(!app.saves_into_unsorted());
    assert!(!app.needs_save_confirmation());
    app.stop_listening();
    app.wait_for_save();
    assert_eq!(std::fs::read_dir(&unsorted).unwrap().count(), 1);
    let name = file_name(&saved[0]);
    let drawn = pane(&ctx, &mut app);
    let row = watches::started_words(&app.open_group().unwrap().takes[0]);
    assert!(drawn.contains(&row), "{row}: {drawn}");

    // The list shows Unsorted at its top, with Quick Listen.
    app.unsorted_open = false;
    let drawn = pane(&ctx, &mut app);
    for t in ["Unsorted Takes", "Quick Listen, no watch · 1 take", "Pepsi"] {
        assert!(drawn.contains(t), "{t}: {drawn}");
    }
    app.unsorted_open = true;
    let e = app.open_group().unwrap();
    assert!(e.watch.id.is_empty() && e.takes.len() == 1);

    // Filed under Pepsi while stopped: Save Again follows it there.
    app.take_readings.wait();
    app.move_take(&saved[0], &pepsi);
    let filed = root.join("Pepsi").join(library::TAKES).join(&name);
    assert!(filed.join(MANIFEST).is_file() && !saved[0].exists());
    assert!(app
        .message
        .as_ref()
        .is_some_and(|(m, _)| m.contains("Filed") && m.contains("Pepsi")));
    assert_eq!(app.session.saved_to(), Some(filed.as_path()));
    assert!(app.open_group().unwrap().takes.is_empty());
    // Deleted while the stopped session is saved into it: the session
    // lets go of it first, rather than refusing.
    app.answer_take_ask(watches::Ask::Delete {
        take: filed.clone(),
        bytes: 0,
    });
    assert!(!filed.exists(), "{:?}", app.message);
    assert_eq!(app.session.saved_to(), None);

    // Unsorted Takes shows its takes at once, not a list left over.
    app.list_for = Some((None, true));
    app.pick_unsorted();
    assert!(!app.list_shown());

    // Picking a watch ends Quick Listen: Save… saves into the watch.
    app.list_for = Some((None, true));
    app.pick_watch(&pepsi);
    assert!(!app.quick_listen && !app.unsorted_open);
    assert!(app.list_for.is_none());
    assert_eq!(
        app.watch_takes_dir(),
        Some(root.join("Pepsi").join(library::TAKES))
    );
    app.watch_open = None;
    assert_eq!(app.watch_takes_dir(), None, "no watch: Save… asks where");
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// A take of the watch picked is saved into it at Stop; New Take asks
/// before unsaved sound goes; Keep in a Watch… saves a Quick Listen
/// take into the watch picked from its menu.
#[test]
fn a_watchs_take_is_kept_at_stop_and_quick_listen_is_kept_in_a_watch() {
    let base = std::env::temp_dir().join(format!("tg-app-new-take-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let takes = root.join("Pepsi").join(library::TAKES);
    let count = || std::fs::read_dir(&takes).map_or(0, |d| d.count());
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    // 20 s of sound as a run would keep it (no device is opened).
    let record = |app: &mut TimegrapherApp, scratch: &Path| {
        app.session.listen();
        let info = app.info("", 48000, 16);
        let mut r = Recorder::start(scratch, info).unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 48000 * 20],
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        app.session.recorder = Some(r);
    };
    app.pick_watch(&pepsi);
    record(&mut app, &base.join("unsaved-1"));
    app.stop_listening();
    app.wait_for_save();
    assert_eq!(count(), 1, "kept in the watch at Stop");
    assert!(!app.needs_save_confirmation());

    // Quick Listen: nothing kept at Stop, and nothing asked: a new take
    // lets it go.
    app.confirm = None;
    assert!(app.quick_listen_ready(true));
    record(&mut app, &base.join("unsaved-2"));
    app.stop_listening();
    app.wait_for_save();
    assert_eq!(count(), 1);
    assert!(!app.needs_save_confirmation() && app.quick_unsaved());
    assert!(
        app.session.recorder.is_some(),
        "until something replaces it"
    );
    // Keep in a Watch… saves it into Pepsi, as a take of Pepsi.
    app.keep_in_watch(&pepsi);
    app.wait_for_save();
    assert_eq!(count(), 2);
    assert!(!app.quick_listen && app.watch_open.as_deref() == Some(pepsi.as_str()));
    assert_eq!(app.watch, "Pepsi");
    assert!(!app.needs_save_confirmation());
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// A Quick Listen take kept in a watch is saved with the watch's
/// calibre, lift angle and beat rate; a take an older app kept there
/// without them opens with the watch's.
#[test]
fn a_take_kept_in_a_watch_takes_the_watchs_calibre() {
    let base = std::env::temp_dir().join(format!("tg-app-keep-cal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let c = timegrapher_core::calibres::find("2824-2").expect("ETA 2824-2 in the table");
    let mut diver = library::Watch::new("Diver");
    diver.calibre = Some(c.calibre.clone());
    diver.lift = Some(55.0);
    lib.add(&diver).unwrap();
    let id = lib.find("Diver").unwrap().watch.id.clone();
    drop(lib);
    let takes = root.join("Diver").join(library::TAKES);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    assert!(app.quick_listen_ready(true));
    assert!(app.calibre.is_empty() && app.bph.is_none());
    app.session.listen();
    let info = app.info("", 48000, 16);
    let mut r = Recorder::start(&base.join("unsaved"), info).unwrap();
    r.write(&timegrapher_core::capture::Block {
        samples: vec![0.0; 48000 * 20],
        at: std::time::SystemTime::now(),
    })
    .unwrap();
    app.session.recorder = Some(r);
    app.stop_listening();
    app.wait_for_save();
    app.keep_in_watch(&id);
    app.wait_for_save();
    let take = std::fs::read_dir(&takes)
        .unwrap()
        .next()
        .expect("a take in the watch")
        .unwrap()
        .path();
    let (m, _) = read_manifest(&take).unwrap().expect("its session file");
    assert_eq!(m.calibre.as_deref(), Some(c.calibre.as_str()));
    // The calibre's lift angle over the watch's own, where it gives one.
    let lift = c.lift_angle_deg.expect("the table gives the 2824-2's lift");
    assert_ne!(lift, 55.0);
    assert_eq!(m.lift, Some(lift));
    assert_eq!(m.bph, c.bph);
    drop(app);

    // One kept without them opens with the watch's.
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    let bare = Manifest {
        lift: Some(DEFAULT_LIFT_DEG),
        ..Default::default()
    };
    app.apply_manifest(&bare, &[], None);
    app.watch_fills_in(&take, &bare);
    assert_eq!(app.calibre, c.calibre);
    assert_eq!(app.lift_deg, lift);
    assert_eq!(app.bph, c.bph);
    // A take outside the library keeps what its file says.
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.apply_manifest(&bare, &[], None);
    app.watch_fills_in(&base.join("elsewhere"), &bare);
    assert!(app.calibre.is_empty() && app.bph.is_none());
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// A watch's takes are kept as their analysis alone, moved
/// and deleted from the Watches view, and a take from outside is added
/// to a watch; the recording opened follows a take that moves.
#[test]
#[expect(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn a_watchs_takes_are_kept_moved_deleted_and_added() {
    let base = std::env::temp_dir().join(format!("tg-app-take-ops-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let a = lib.add(&library::Watch::new("Pepsi")).unwrap();
    lib.add(&library::Watch::new("Daytona")).unwrap();
    drop(lib);
    // A take with its sound, as the app saves one.
    let take_at = |dir: &Path| {
        std::fs::create_dir_all(dir).unwrap();
        let audio = generate(
            &SynthConfig {
                duration_s: 40.0,
                rate_s_per_day: 4.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        timegrapher_core::audio::write_wav(&dir.join("take_part01-001.wav"), &audio).unwrap();
        std::fs::write(
            dir.join(MANIFEST),
            "# kept as written\nbph = 28800\nlift = 52\n\n[[recording]]\nfile = \".\"\n\
             position = \"DU\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("session.json"),
            r#"{"started_utc":"2026-10-20T17:00:00Z","duration_s":40.0}"#,
        )
        .unwrap();
    };
    let take = a.join(library::TAKES).join("2026-10-20 10.00 Dial Up");
    take_at(&take);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    let pepsi = app
        .library
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()
        .find("Pepsi")
        .unwrap();
    let daytona_id = app
        .library
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()
        .find("Daytona")
        .unwrap()
        .watch
        .id
        .clone();
    let pepsi_id = pepsi.watch.id.clone();
    app.pick_watch(&pepsi_id);

    // Keep Analysis Only: the sound goes only once the analysis is in place.
    app.take_readings.wait();
    app.start_take_keep(take.clone());
    assert!(app.take_busy(&take, false).is_some());
    let t = Instant::now();
    while app.take_keep.is_some() && t.elapsed().as_secs() < 120 {
        app.poll_take_keep();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.take_keep.is_none(), "it finished");
    assert!(analysis_take::is_analysis_only(&take), "{:?}", app.message);
    assert!(!take.join("take_part01-001.wav").exists());
    let toml = std::fs::read_to_string(take.join(MANIFEST)).unwrap();
    assert!(
        toml.contains("# kept as written") && toml.contains("audio = false"),
        "{toml}"
    );
    assert!(take.join("session.json").is_file());
    let e = app.open_watch().unwrap();
    assert!(!e.takes[0].audio && !e.takes[0].incomplete);
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.watch_side(ui));
    });
    assert!(format!("{:?}", out.shapes).contains("Analysis Only"));
    // A click picks the take without opening it.
    app.take_action(Some(watches::TakeAction::Pick(take.clone())));
    assert_eq!(app.take_picked.as_ref(), Some(&take));
    assert!(app.batch.is_none() && app.live.is_none());
    // Its opened analysis reads back.
    app.take_action(Some(watches::TakeAction::Open(take.clone())));
    assert!(app.batch.is_some());
    let t = Instant::now();
    while app.batch.is_some() && t.elapsed().as_secs() < 60 {
        app.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.live.is_some());
    assert_eq!(app.take_on_show().as_ref(), Some(&take));
    // Opened again while on show, it is not read again.
    app.take_action(Some(watches::TakeAction::Open(take.clone())));
    assert!(app.batch.is_none());

    let name = file_name(&take);

    // Moved to Daytona, then deleted there.
    app.take_readings.wait();
    app.move_take(&take, &daytona_id);
    let d = root.join("Daytona").join(library::TAKES).join(&name);
    assert!(d.is_dir() && !take.exists());
    assert_eq!(Path::new(&app.opened_path), d);
    app.answer_take_ask(watches::Ask::Delete {
        take: d.clone(),
        bytes: 0,
    });
    assert!(!d.exists());
    // The take was on show: the window is empty again, as at the start.
    assert!(app.live.is_none() && app.opened_path.is_empty());
    assert!(app.take_on_show().is_none());

    // A take from outside, opened, is added by a move.
    let outside = base.join("From a USB Stick");
    take_at(&outside);
    app.input = Input::File;
    app.opened_path = outside.display().to_string();
    assert_eq!(app.addable_take(), Some(outside.clone()));
    app.answer_take_ask(watches::Ask::Add {
        from: outside.clone(),
        watch: Some(pepsi_id.clone()),
        keep_original: false,
    });
    let added = a.join(library::TAKES).join("From a USB Stick");
    assert!(added.join("take_part01-001.wav").is_file() && !outside.exists());
    assert_eq!(Path::new(&app.opened_path), added);
    assert_eq!(app.addable_take(), None);
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_quit_that_cannot_ask_keeps_unsaved_sound_to_offer_back() {
    let base = std::env::temp_dir().join(format!("tg-app-kept-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let listening = |base: &Path| {
        let mut app = synthetic(5.0, 0.0);
        app.session.listen();
        let info = app.info("Kept", 48000, 16);
        let mut r = Recorder::start(base, info).unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 48000 * 20],
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        let dir = r.dir().to_path_buf();
        app.session.recorder = Some(r);
        app.session.set_state(session::State::Paused {
            since: Instant::now(),
        });
        (app, dir)
    };
    let (mut app, dir) = listening(&base);
    // Being recorded into, it is not offered.
    assert!(kept_sounds(&base).is_empty());
    eframe::App::on_exit(&mut app, None);
    let kept = kept_sounds(&base);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].dir, dir);
    assert!((kept[0].duration_s.unwrap() - 20.0).abs() < 0.5, "{kept:?}");
    let about: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("session.json")).unwrap()).unwrap();
    let local = watches::local_minute(about["started_utc"].as_str().unwrap());
    assert!(local.is_some());
    assert_eq!(kept[0].started, local, "in this computer's time: {kept:?}");
    assert!(kept[0].bytes > 48000 * 2 * 19);

    // The next start says so, with the ways to keep it.
    let mut next = TimegrapherApp::with_devices(Vec::new(), None);
    next.kept = kept;
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut drawn = String::new();
    for _ in 0..2 {
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| next.kept_notice(ui));
        });
        drawn = format!("{:?}", out.shapes);
    }
    assert!(drawn.contains("wasn't saved"), "{drawn}");
    for b in ["Open", "Move To…", "Delete…", "Later"] {
        assert!(drawn.contains(b), "{b}");
    }
    // Open reads it as a recording.
    next.open_recording(&dir, true);
    let t = Instant::now();
    while next.batch.is_some() && t.elapsed().as_secs() < 60 {
        next.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    let live = next.live.as_ref().expect("opened");
    assert!(
        (live.duration_s() - 20.0).abs() < 0.5,
        "{}",
        live.duration_s()
    );
    drop(next);
    // Move To… puts the folder where the user says.
    let mine = base.join("mine");
    std::fs::create_dir_all(&mine).unwrap();
    let moved = move_kept(&dir, &mine).unwrap();
    assert!(!dir.exists() && moved.join("audio-001.wav").is_file());
    assert!(kept_sounds(&base).is_empty());

    // A quit the user answered ("Quit Without Saving") throws it away.
    let (mut app, dir) = listening(&base);
    app.may_close = true;
    eframe::App::on_exit(&mut app, None);
    assert!(!dir.exists());
    let _ = std::fs::remove_dir_all(&base);
}

/// A position as a word of its own in a file's name, and only one.
#[test]
fn positions_are_read_from_file_names() {
    let p = |n: &str| position_in_name(Path::new(n));
    assert_eq!(p("reverso_DU_01.flac"), Some(Position::DialUp));
    assert_eq!(p("/x/ym42_dd_30min/ym42-cl.wav"), Some(Position::CrownLeft));
    assert_eq!(p("ducks.flac"), None);
    assert_eq!(p("du_then_dd.flac"), None);
}

/// Wait for the analysis being kept, then for the take being opened.
fn wait_for_jobs(app: &mut TimegrapherApp) {
    let t = Instant::now();
    while (app.analysis_save.is_some() || app.batch.is_some()) && t.elapsed().as_secs() < 180 {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        app.analysis_save.is_none() && app.batch.is_none(),
        "still working"
    );
}

/// A session kept as its analysis alone has no sound in it, and opens
/// with the same beats, readings and charts as the take with its sound,
/// the Tick Tock profiles it kept, and no Replay or waveform.
#[test]
#[expect(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn a_session_kept_as_its_analysis_opens_as_the_sound_does() {
    let base = std::env::temp_dir().join(format!("tg-app-analysis-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.bph = Some(28800);
    app.reset_session(48000, "test".into());
    app.session.listen();
    let audio = generate(
        &SynthConfig {
            duration_s: 60.0,
            beat_error_ms: 0.6,
            odd_gains: Some([0.9, 0.25, 1.0]),
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let info = app.info("test", 48000, 16);
    app.session.recorder = Some(Recorder::start(&base.join("live"), info).unwrap());
    // A second of sound at a time, each arriving as its last frame is
    // heard, as the capture delivers it, so the clock log reads as a
    // real take's does.
    let began = std::time::SystemTime::now();
    for (k, second) in audio.samples.chunks(48000).enumerate() {
        let block = timegrapher_core::capture::Block {
            samples: second.to_vec(),
            at: began + std::time::Duration::from_secs(k as u64 + 1),
        };
        app.session
            .recorder
            .as_mut()
            .unwrap()
            .write(&block)
            .unwrap();
    }
    for samples in audio.samples.chunks(960) {
        app.live.as_mut().unwrap().push(samples);
    }
    app.masked = vec![(30.0, 32.0)];
    assert!(app.can_save_analysis());
    app.start_analysis_save(&base.join("kept"));
    assert!(app.analysis_save_busy(), "nothing else while it reads");
    wait_for_jobs(&mut app);
    assert!(
        !app.message.as_ref().is_some_and(|m| m.1),
        "{:?}",
        app.message
    );
    let (kept, _) = app.analysis_kept_to.clone().expect("kept");
    assert!(app.session.saved_to().is_none(), "the sound isn't saved");
    assert!(
        file_name(&kept).ends_with("_analysis"),
        "{}",
        kept.display()
    );
    assert!(analysis_take::is_analysis_only(&kept));
    assert!(recordings_in(&kept).is_err(), "no sound in it");
    let m: Manifest =
        toml::from_str(&std::fs::read_to_string(kept.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(m.audio, Some(false));
    assert!(kept.join("session.json").is_file() && kept.join("clock.csv").is_file());
    // The same session with its sound, to compare.
    app.save_into(&base.join("full"));
    app.wait_for_save();
    let full = app
        .session
        .saved_to()
        .map(Path::to_path_buf)
        .expect("saved");
    let open = |path: &Path| {
        let mut a = TimegrapherApp::with_devices(Vec::new(), None);
        a.start_batch(path);
        wait_for_jobs(&mut a);
        a
    };
    let (mut a, b) = (open(&kept), open(&full));
    assert!(a.no_sound && !b.no_sound);
    assert_eq!(a.masked, b.masked);
    assert_eq!(a.masked, vec![(30.0, 32.0)]);
    let (la, lb) = (a.live.as_ref().unwrap(), b.live.as_ref().unwrap());
    assert_eq!(la.beats().len(), lb.beats().len());
    assert_eq!(la.amplitude_windows().len(), lb.amplitude_windows().len());
    let (ra, rb) = (la.reading(20.0), lb.reading(20.0));
    let close = |x: Option<f64>, y: Option<f64>, tol: f64| match (x, y) {
        (Some(x), Some(y)) => (x - y).abs() <= tol,
        (None, None) => true,
        _ => false,
    };
    assert!(
        close(ra.rate_s_per_day, rb.rate_s_per_day, 0.05),
        "{ra:?} {rb:?}"
    );
    assert!(close(ra.amplitude_deg, rb.amplitude_deg, 1e-9));
    assert!(close(ra.beat_error_ms, rb.beat_error_ms, 0.001));
    assert_eq!(a.trend.len(), b.trend.len());
    // The profiles kept for each 2 seconds stand in for the sound.
    assert!(a.sound_history.len() >= 29, "{}", a.sound_history.len());
    assert!(a
        .sound_history
        .iter()
        .all(|(_, p)| p[0].is_some() && p[1].is_some()));
    // Masks changed on it are saved into its session file, which still
    // says it has no sound.
    a.masked = vec![(30.0, 32.0), (50.0, 52.0)];
    a.save_marks();
    let m: Manifest =
        toml::from_str(&std::fs::read_to_string(kept.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(m.audio, Some(false));
    assert_eq!(m.recordings[0].masked, vec![[30.0, 32.0], [50.0, 52.0]]);
    // The folder it was kept in opens it too.
    assert_eq!(take_folder(&base.join("kept")).unwrap(), kept);
    // A session file saying no sound needs the analysis to read, as
    // `timegrapher session` needs it: refused with the sound beside it
    // or without.
    let odd = base.join("odd");
    std::fs::create_dir_all(&odd).unwrap();
    for f in std::fs::read_dir(&full).unwrap() {
        let f = f.unwrap().path();
        if f.is_file() {
            std::fs::copy(&f, odd.join(f.file_name().unwrap())).unwrap();
        }
    }
    let text = std::fs::read_to_string(odd.join(MANIFEST)).unwrap();
    std::fs::write(odd.join(MANIFEST), format!("audio = false\n{text}")).unwrap();
    let mut d = TimegrapherApp::with_devices(Vec::new(), None);
    d.open_recording(&odd, false);
    let (msg, error) = d.message.clone().unwrap();
    assert!(error && msg.contains("audio = false"), "{msg}");
    // Opened through the folder it sits in, refused before the take on
    // show loses its masks.
    let nest = base.join("nest").join("odd");
    std::fs::create_dir_all(&nest).unwrap();
    for f in std::fs::read_dir(&odd).unwrap() {
        let f = f.unwrap().path();
        std::fs::copy(&f, nest.join(f.file_name().unwrap())).unwrap();
    }
    a.open_recording(&base.join("nest"), false);
    let (msg, error) = a.message.clone().unwrap();
    assert!(error && msg.contains("audio = false"), "{msg}");
    assert_eq!(a.masked, vec![(30.0, 32.0), (50.0, 52.0)]);
    for f in recordings_in(&odd).unwrap() {
        std::fs::remove_file(f).unwrap();
    }
    let mut d = TimegrapherApp::with_devices(Vec::new(), None);
    d.open_recording(&odd, false);
    let (msg, error) = d.message.clone().unwrap();
    assert!(error && msg.contains("audio = false"), "{msg}");
    let _ = std::fs::remove_dir_all(&base);
}
