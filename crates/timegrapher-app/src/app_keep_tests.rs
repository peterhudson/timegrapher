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
    app.watches_view.watch_form = Some(watches::Form {
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
    assert!(app.watches_view.watch_form.is_none());
    assert!(library::Library::is_library(&root));
    assert_eq!(
        (
            app.bench.take.watch.as_str(),
            app.bench.take.lift_deg,
            app.bench.take.bph
        ),
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
    app.watches_view.watch_open = None;
    let drawn = pane(&ctx, &mut app);
    assert!(drawn.contains("1 take"), "{drawn}");
    app.watches_view.watch_form = Some(watches::Form {
        watch: library::Watch::new("pepsi"),
        new: true,
        error: None,
    });
    app.save_watch_form();
    let refused = app
        .watches_view
        .watch_form
        .as_ref()
        .and_then(|f| f.error.clone());
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
    app.watches_view.take_readings.set_now(Some(take.clone()));
    for keep in [false, true] {
        let why = app.take_busy(&take, keep).unwrap();
        assert!(why.contains("being read for the Watches list"), "{why}");
    }
    app.watches_view
        .take_readings
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
    app.watches_view.take_readings.wait();
    let drawn = pane(&mut app);
    let want = library::reading(&take).unwrap();
    assert_eq!(app.watches_view.take_readings.reading(&take), Some(&want));
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
    let Ok(Some(l)) = &mut app.library else {
        panic!("no library")
    };
    l.scan();
    let at = l.readings_revision();
    app.watches_view.take_readings.want(&take, &at, &ctx);
    assert_eq!(app.watches_view.take_readings.reading(&take), Some(&want));
    app.watches_view.take_readings.wait();
    assert_eq!(app.watches_view.take_readings.reading(&take), Some(&want));
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
        matches!(
            app.watches_view.thumbs.get(&ctx, p),
            photos::Thumb::Ready(_)
        )
    };
    let failed = |app: &mut TimegrapherApp, p: &Path| {
        matches!(
            app.watches_view.thumbs.get(&ctx, p),
            photos::Thumb::Failed(_)
        )
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
    app.watches_view.take_ask = Some(watches::Ask::DeletePhoto { photo: bad.clone() });
    pane(&ctx, &mut app);
    let asked = pane(&ctx, &mut app);
    assert!(asked.contains("Delete \\\"broken.jpg\\\"?"), "{asked}");
    let ask = app.watches_view.take_ask.take().unwrap();
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
    app.bench.keeping.open_ask = Some(a.clone());
    let mut text = String::new();
    for _ in 0..2 {
        let out = ctx.run(egui::RawInput::default(), |ctx| app.open_ask_dialog(ctx));
        text = format!("{:?}", out.shapes);
    }
    for asked in ["Beat Rate", "Lift Angle", "Started", "Add and Open"] {
        assert!(text.contains(asked), "{asked}");
    }
    assert!(!text.contains("Choose a Position"));
    app.bench.keeping.open_ask = None;
    a.bph = Some(21600);
    a.started = "2026-10-22 14:30".into();
    app.answer_open_ask(a, need);
    assert!(app.bench.keeping.open_ask.is_none());
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
    app.bench.recording.input = Input::File;
    app.bench.keeping.opened_path = lone.display().to_string();
    app.bench.take.position = Position::CrownLeft;
    app.bench.take.bph = Some(28800);
    app.bench.take.lift_deg = 50.0;

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
    assert_eq!(app.watches_view.watch_open.as_deref(), Some(pepsi.as_str()));
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
    assert!(app.bench.keeping.open_ask.is_none());
    assert_eq!(app.watches_view.watch_open, None);
    app.keep_into(&pepsi);
    let mut a = app
        .bench
        .keeping
        .open_ask
        .take()
        .expect("asked when it started");
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
    app.bench.recording.input = Input::File;
    app.bench.keeping.opened_path = first.display().to_string();
    // Alone in its folder, the take goes in whole.
    app.keep_into(&pepsi);
    assert!(
        matches!(&app.watches_view.take_ask, Some(watches::Ask::Add { from, .. }) if *from == take),
        "{:?}",
        app.watches_view.take_ask
    );
    app.watches_view.take_ask = None;
    // One of two: it goes in alone, asked as a recording.
    part(2);
    app.keep_into(&pepsi);
    assert!(
        app.watches_view.take_ask.is_none(),
        "{:?}",
        app.watches_view.take_ask
    );
    let shown = app.bench.keeping.open_ask.as_ref().map(|a| a.path.clone());
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
    app.bench.recording.input = Input::File;
    app.bench.keeping.opened_path = lone.display().to_string();
    let shown = (
        app.bench.results.trend.len(),
        app.bench.take.bph,
        app.bench.take.lift_deg,
        app.bench.take.watch.clone(),
    );
    assert!(shown.0 > 0);
    assert_eq!(app.addable_take(), Some(lone.clone()));

    // Keep in a Watch… > New Watch…, filled in, then Done.
    app.keep_in_new_watch();
    let f = app.watches_view.watch_form.as_mut().unwrap();
    f.watch.name = "Meteorite".into();
    f.watch.bph = Some(21600);
    f.watch.lift = Some(50.0);
    app.save_watch_form();
    assert!(app.watches_view.watch_form.is_none() && !app.bench.keeping.keep_after_form);
    // The readings on show keep the settings they were measured with.
    assert_eq!(
        (
            app.bench.results.trend.len(),
            app.bench.take.bph,
            app.bench.take.lift_deg,
            app.bench.take.watch.clone()
        ),
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
    let mut a = app
        .bench
        .keeping
        .open_ask
        .take()
        .expect("asked what is missing");
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
    assert!(Path::new(&app.bench.keeping.opened_path).starts_with(&root));
    assert_eq!(app.addable_take(), None);

    // File > New Session lets go of the recording: a new take follows.
    app.new_session();
    assert!(
        app.bench.recording.input == Input::Microphone && app.bench.keeping.opened_path.is_empty()
    );
    assert_eq!(app.status_words(), "New Take · Meteorite · Not Started");
    // And the watch's settings apply to it (set up as New Take does,
    // without opening a real input).
    app.take_setup();
    assert_eq!(
        (
            app.bench.take.watch.as_str(),
            app.bench.take.bph,
            app.bench.take.lift_deg
        ),
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
    assert_eq!(app.bench.take.watch, "Pepsi");
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
    assert!(
        !app.bench.recording.quick_listen && app.watches_view.watch_open.is_some(),
        "nothing yet"
    );
    // Answered (no input is opened here: the test touches no device).
    app.confirm = None;
    assert!(app.quick_listen_ready(true));
    assert!(app.session.recorder.is_none(), "the unsaved session went");
    assert!(
        app.bench.recording.quick_listen
            && app.watches_view.unsorted_open
            && app.watches_view.watch_open.is_none()
    );
    assert_eq!(app.bench.take.watch, "");
    assert!(!app.running());
    // The list, Unsorted Takes marked; its row shows its takes.
    assert!(app.list_shown());
    let drawn = pane(&ctx, &mut app);
    for t in ["Unsorted Takes", "Quick Listen, no watch", "Pepsi"] {
        assert!(drawn.contains(t), "{t}: {drawn}");
    }
    app.watches_view.list_for = None;
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
    app.watches_view.unsorted_open = false;
    let drawn = pane(&ctx, &mut app);
    for t in ["Unsorted Takes", "Quick Listen, no watch · 1 take", "Pepsi"] {
        assert!(drawn.contains(t), "{t}: {drawn}");
    }
    app.watches_view.unsorted_open = true;
    let e = app.open_group().unwrap();
    assert!(e.watch.id.is_empty() && e.takes.len() == 1);

    // Filed under Pepsi while stopped: Save Again follows it there.
    app.watches_view.take_readings.wait();
    app.move_take(&saved[0], &pepsi);
    let filed = root.join("Pepsi").join(library::TAKES).join(&name);
    assert!(filed.join(MANIFEST).is_file() && !saved[0].exists());
    assert!(app
        .message
        .as_ref()
        .is_some_and(|(m, _)| m.contains("Filed") && m.contains("Pepsi")));
    assert_eq!(app.session.saved_to(), Some(filed.as_path()));
    // The Bench and Watches go with it to Pepsi (#1978): Unsorted is left
    // empty, and Pepsi shows the take.
    assert_eq!(app.bench.take.watch, "Pepsi");
    let e = app.open_group().unwrap();
    assert_eq!(e.watch.name, "Pepsi");
    assert!(
        e.takes.iter().any(|t| e.take_dir(t) == filed),
        "{:?}",
        e.takes
    );
    // Deleted while the stopped session is saved into it: the session
    // lets go of it first, rather than refusing.
    app.answer_take_ask(watches::Ask::Delete {
        take: filed.clone(),
        bytes: 0,
    });
    assert!(!filed.exists(), "{:?}", app.message);
    assert_eq!(app.session.saved_to(), None);

    // Unsorted Takes shows its takes at once, not a list left over.
    app.watches_view.list_for = Some(watches_view::Picked::UNSORTED);
    app.pick_unsorted();
    assert!(!app.list_shown());

    // Picking a watch ends Quick Listen: Save… saves into the watch.
    app.watches_view.list_for = Some(watches_view::Picked::UNSORTED);
    app.pick_watch(&pepsi);
    assert!(!app.bench.recording.quick_listen && !app.watches_view.unsorted_open);
    assert!(app.watches_view.list_for.is_none());
    assert_eq!(
        app.watch_takes_dir(),
        Some(root.join("Pepsi").join(library::TAKES))
    );
    app.watches_view.watch_open = None;
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
    assert!(
        !app.bench.recording.quick_listen
            && app.watches_view.watch_open.as_deref() == Some(pepsi.as_str())
    );
    assert_eq!(app.bench.take.watch, "Pepsi");
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
    assert!(app.bench.take.calibre.is_empty() && app.bench.take.bph.is_none());
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
    assert_eq!(app.bench.take.calibre, c.calibre);
    assert_eq!(app.bench.take.lift_deg, lift);
    assert_eq!(app.bench.take.bph, c.bph);
    // A take outside the library keeps what its file says.
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.apply_manifest(&bare, &[], None);
    app.watch_fills_in(&base.join("elsewhere"), &bare);
    assert!(app.bench.take.calibre.is_empty() && app.bench.take.bph.is_none());
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
    // Daytona has its own lift and beat rate: a take kept as its analysis
    // alone, moved there, keeps those it was measured with (#1283).
    let mut daytona = library::Watch::new("Daytona");
    daytona.lift = Some(49.0);
    daytona.bph = Some(21600);
    lib.add(&daytona).unwrap();
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
            "# kept as written\nwatch = \"Pepsi\"\nbph = 28800\nlift = 52\n\n[[recording]]\nfile = \".\"\n\
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
    app.watches_view.take_readings.wait();
    app.start_take_keep(take.clone());
    assert!(app.take_busy(&take, false).is_some());
    let t = Instant::now();
    while app.bench.keeping.take_keep.is_some() && t.elapsed().as_secs() < 120 {
        app.poll_take_keep();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.bench.keeping.take_keep.is_none(), "it finished");
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
    assert_eq!(app.bench.keeping.take_picked.as_ref(), Some(&take));
    assert!(app.bench.recording.batch.is_none() && app.bench.recording.live.is_none());
    // Its opened analysis reads back.
    app.take_action(Some(watches::TakeAction::Open(take.clone())));
    assert!(app.bench.recording.batch.is_some());
    let t = Instant::now();
    while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 60 {
        app.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.bench.recording.live.is_some());
    assert_eq!(app.take_on_show().as_ref(), Some(&take));
    // Opened again while on show, it is not read again.
    app.take_action(Some(watches::TakeAction::Open(take.clone())));
    assert!(app.bench.recording.batch.is_none());

    let name = file_name(&take);

    // Moved to Daytona, then deleted there.
    app.watches_view.take_readings.wait();
    app.move_take(&take, &daytona_id);
    let d = root.join("Daytona").join(library::TAKES).join(&name);
    assert!(d.is_dir() && !take.exists());
    assert_eq!(Path::new(&app.bench.keeping.opened_path), d);
    // On show, it is Daytona's now: the Bench's card and the status line
    // name it (#1280).
    assert_eq!(
        app.watches_view.watch_open.as_deref(),
        Some(daytona_id.as_str())
    );
    assert_eq!(app.bench.take.watch, "Daytona");
    assert!(
        app.status_words().contains("· Daytona ·"),
        "{}",
        app.status_words()
    );
    // Its session file names Daytona too, and, its sound gone, keeps the
    // lift and beat rate it was measured with (#1283).
    let toml = std::fs::read_to_string(d.join(MANIFEST)).unwrap();
    assert!(
        toml.contains("watch = \"Daytona\"")
            && !toml.contains("Pepsi")
            && toml.contains("# kept as written")
            && toml.contains("bph = 28800")
            && toml.contains("lift = 52"),
        "{toml}"
    );
    app.answer_take_ask(watches::Ask::Delete {
        take: d.clone(),
        bytes: 0,
    });
    assert!(!d.exists());
    // The take was on show: the window is empty again, as at the start.
    assert!(app.bench.recording.live.is_none() && app.bench.keeping.opened_path.is_empty());
    assert!(app.take_on_show().is_none());
    // Set for Daytona's next take, as Listen sets it: its calibre, lift
    // and beat rate on the card, and no Quick Listen in the status (#1981).
    let w = app.open_watch().unwrap().watch.clone();
    assert_eq!(
        (
            app.bench.take.watch.as_str(),
            app.bench.recording.quick_listen
        ),
        ("Daytona", false)
    );
    assert_eq!(
        app.bench.take.calibre,
        w.calibre.clone().unwrap_or_default()
    );
    let lift = app.cal.lift(&app.bench.take.calibre).or(w.lift);
    let bph = app.cal.bph(&app.bench.take.calibre).or(w.bph);
    assert_eq!(
        (app.bench.take.lift_deg, app.bench.take.bph),
        (lift.unwrap_or(DEFAULT_LIFT_DEG), bph)
    );
    assert!(
        !app.status_words().contains("Quick Listen"),
        "{}",
        app.status_words()
    );

    // A take from outside, opened, is added by a move.
    let outside = base.join("From a USB Stick");
    take_at(&outside);
    app.bench.recording.input = Input::File;
    app.bench.keeping.opened_path = outside.display().to_string();
    assert_eq!(app.addable_take(), Some(outside.clone()));
    app.answer_take_ask(watches::Ask::Add {
        from: outside.clone(),
        watch: Some(pepsi_id.clone()),
        keep_original: false,
    });
    let added = a.join(library::TAKES).join("From a USB Stick");
    assert!(added.join("take_part01-001.wav").is_file() && !outside.exists());
    assert_eq!(Path::new(&app.bench.keeping.opened_path), added);
    assert_eq!(app.addable_take(), None);
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// The take on show, filed from Unsorted Takes under a watch: the Bench
/// leaves Unsorted Takes for that watch and the status line names it
/// (#1280).
#[test]
fn the_take_on_show_filed_from_unsorted_takes_the_bench_to_its_watch() {
    let base = std::env::temp_dir().join(format!("tg-app-file-shown-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    let take = lib.unsorted_dir().unwrap().join("2026-10-20 10.00 Dial Up");
    drop(lib);
    std::fs::create_dir_all(&take).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&take.join("take_part01-001.wav"), &audio).unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "bph = 28800\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.pick_unsorted();
    app.take_action(Some(watches::TakeAction::Open(take.clone())));
    let t = Instant::now();
    while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 60 {
        app.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(app.take_on_show().as_ref(), Some(&take));
    assert!(app.watches_view.unsorted_open && app.watches_view.watch_open.is_none());

    app.watches_view.take_readings.wait();
    app.move_take(&take, &pepsi);
    assert!(!app.watches_view.unsorted_open);
    assert_eq!(app.watches_view.watch_open.as_deref(), Some(pepsi.as_str()));
    assert_eq!(app.bench.take.watch, "Pepsi");
    assert!(
        app.status_words().contains("· Pepsi ·"),
        "{}",
        app.status_words()
    );
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// A Quick Listen kept in Unsorted Takes, still on the Bench, filed under
/// a watch: the Bench and the status line follow it to that watch, and it
/// is read again with the watch's lift (#1978).
#[test]
fn a_quick_listen_kept_then_filed_takes_the_bench_to_its_watch() {
    let (mut app, base) = super::tests::save_failure_fixture("file-kept");
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let mut ym = library::Watch::new("Yacht-Master 42");
    ym.lift = Some(55.0);
    lib.add(&ym).unwrap();
    let id = lib.find("Yacht-Master 42").unwrap().watch.id.clone();
    let unsorted = lib.unsorted_dir().unwrap();
    drop(lib);
    app.use_library(Some(root.clone()));
    app.pick_unsorted();
    for at in 2..12 {
        super::tests::append_save_test_second(&mut app, at);
    }
    app.save_into(&unsorted);
    app.wait_for_save();
    let kept = app.session.saved_to().expect("kept").to_path_buf();
    assert!(app.take_on_show().is_none(), "the session's own take");
    app.watches_view.take_readings.wait();
    app.move_take(&kept, &id);
    assert!(!app.watches_view.unsorted_open, "{:?}", app.message);
    assert_eq!(app.watches_view.watch_open.as_deref(), Some(id.as_str()));
    assert_eq!(app.bench.take.watch, "Yacht-Master 42");
    assert!(
        app.status_words().contains("Yacht-Master 42") && !app.status_words().contains("No watch"),
        "{}",
        app.status_words()
    );
    let t = Instant::now();
    while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 60 {
        app.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        app.bench.take.lift_deg, 55.0,
        "read again with its watch's lift"
    );
    // The status line names the take by its folder, not New Take
    // (Walter on #1999).
    let name = file_name(&kept);
    assert!(
        app.status_words().contains(&name) && !app.status_words().contains("New Take"),
        "{}",
        app.status_words()
    );
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// A take moved to a watch takes the calibre, lift angle and beat rate
/// that watch has set, the calibre's own where the table gives them, and
/// is read again with them when it is on show; what the watch leaves
/// unset stays as recorded (#1283, Peter's call).
#[test]
fn a_moved_take_takes_its_new_watchs_settings() {
    let base = std::env::temp_dir().join(format!("tg-app-move-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let pepsi = lib.add(&library::Watch::new("Pepsi")).unwrap();
    let mut eta = library::Watch::new("Explorer");
    eta.calibre = Some("ETA 2824-2".into());
    lib.add(&eta).unwrap();
    let mut own = library::Watch::new("Own");
    own.lift = Some(49.0);
    own.bph = Some(21600);
    lib.add(&own).unwrap();
    let id = |name: &str| lib.find(name).unwrap().watch.id.clone();
    let (eta_id, own_id) = (id("Explorer"), id("Own"));
    drop(lib);
    let take = pepsi.join(library::TAKES).join("2026-10-20 10.00 Dial Up");
    std::fs::create_dir_all(&take).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&take.join("take_part01-001.wav"), &audio).unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "# kept as written\nwatch = \"Pepsi\"\ncalibre = \"Old\"\nbph = 28800\nlift = 52\n\
         lift_known = false\n\n[wheels]\nthird = 450.0\n\n[[recording]]\nfile = \".\"\n\
         position = \"DU\"\n",
    )
    .unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    let open = |app: &mut TimegrapherApp| {
        let t = Instant::now();
        while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 60 {
            app.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    app.take_action(Some(watches::TakeAction::Open(take.clone())));
    open(&mut app);
    assert_eq!(app.bench.take.lift_deg, 52.0);

    // Into a watch of a calibre the table knows: its lift and beat rate.
    app.watches_view.take_readings.wait();
    app.move_take(&take, &eta_id);
    let at = root
        .join("Explorer")
        .join(library::TAKES)
        .join(file_name(&take));
    let (lift, bph) = (
        app.cal.lift("ETA 2824-2").unwrap(),
        app.cal.bph("ETA 2824-2").unwrap(),
    );
    let m: Manifest = toml::from_str(&std::fs::read_to_string(at.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(m.watch.as_deref(), Some("Explorer"));
    assert_eq!(m.calibre.as_deref(), Some("ETA 2824-2"));
    assert_eq!(
        (m.lift, m.lift_known, m.bph),
        (Some(lift), Some(true), Some(bph))
    );
    assert!(m.wheels.is_empty(), "the old calibre's wheels go with it");
    assert!(std::fs::read_to_string(at.join(MANIFEST))
        .unwrap()
        .contains("# kept as written"));
    // On show, it is read again with them.
    assert!(app.bench.recording.batch.is_some(), "read again");
    open(&mut app);
    assert_eq!(app.take_on_show().as_ref(), Some(&at));
    assert_eq!(app.bench.take.lift_deg, lift);

    // Into a watch of its own lift and beat rate and no calibre: those
    // two, and the calibre stays.
    app.watches_view.take_readings.wait();
    app.move_take(&at, &own_id);
    let at = root.join("Own").join(library::TAKES).join(file_name(&take));
    let m: Manifest = toml::from_str(&std::fs::read_to_string(at.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(m.watch.as_deref(), Some("Own"));
    assert_eq!(m.calibre.as_deref(), Some("ETA 2824-2"));
    assert_eq!((m.lift, m.bph), (Some(49.0), Some(21600)));
    open(&mut app);
    assert_eq!(app.bench.take.lift_deg, 49.0);
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// A take whose lift its session file says wasn't known, moved to a
/// watch of that same lift: the lift is the watch's now, so known, and
/// the take is read again (#1283).
#[test]
fn a_moved_takes_lift_becomes_known_from_its_watch() {
    let cal = crate::calibre_ui::Calibres::open(None);
    let mut w = library::Watch::new("Own");
    w.lift = Some(52.0);
    let mut doc: toml_edit::DocumentMut = "lift = 52\nlift_known = false\n".parse().unwrap();
    assert!(library::settings_of(&mut doc, &w, &cal.user));
    assert_eq!(doc.get("lift_known").and_then(|i| i.as_bool()), Some(true));
    // Already so: nothing changes, and nothing is read again.
    assert!(!library::settings_of(&mut doc, &w, &cal.user));
}

/// A watch whose calibre's rate, or its own, is outside 12,000 to 72,000
/// bph files a take with no `bph`, so the take opens on Auto (#2224).
#[test]
fn a_watch_rate_outside_the_range_files_a_take_on_auto() {
    let cal = crate::calibre_ui::Calibres::open(None);
    let mut iwc = library::Watch::new("IWC");
    iwc.calibre = Some("IWC 9520".into());
    let mut own = library::Watch::new("Own");
    own.bph = Some(10_800);
    for w in [&iwc, &own] {
        assert_eq!(cal.watch_bph(w), None, "{}", w.name);
        let mut doc = toml_edit::DocumentMut::new();
        library::settings_of(&mut doc, w, &cal.user);
        assert!(doc.get("bph").is_none(), "{}: {doc}", w.name);
    }
    assert_eq!(cal.bph("IWC 9520"), None);
    assert_eq!(cal.bph("IWC 9520 (18,000 bph, 40°)"), Some(18_000));
}

/// A recording opened from outside the Watches folder stays on show, as
/// it was measured, when a watch is picked in Watches, picked on the
/// Watch line, or added: to be kept in it next (#1966).
#[test]
fn picking_or_adding_a_watch_keeps_a_recording_opened_from_outside() {
    let base = std::env::temp_dir().join(format!("tg-app-pick-keep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let file = base.join("outside").join("old take.wav");
    let mut app = super::tests::opened_recording(file.parent().unwrap(), "old take.wav", 20.0);
    app.use_library(Some(root.clone()));
    app.change_position(Position::DialDown);
    let shown = app.take_on_show().expect("on show");
    assert!(app.shown_outside_watches());
    // Picked in Watches.
    app.pick_watch(&pepsi);
    assert_eq!(app.take_on_show(), Some(shown.clone()));
    assert!(app.bench.recording.live.is_some());
    assert_eq!(app.watches_view.watch_open.as_deref(), Some(pepsi.as_str()));
    // Picked on the Watch line: nothing asked, nothing put away, and the
    // position stays.
    drop(app);
    let mut app = super::tests::opened_recording(file.parent().unwrap(), "old take.wav", 20.0);
    app.use_library(Some(root.clone()));
    app.change_position(Position::DialDown);
    app.line_pick(Some(pepsi.clone()), false);
    assert!(app.confirm.is_none());
    assert_eq!(app.take_on_show(), Some(shown.clone()));
    assert_eq!(app.bench.take.position, Position::DialDown);
    // Added with New Watch…: picked, and the take stays.
    let Ok(Some(lib)) = &mut app.library else {
        panic!("no library");
    };
    lib.add(&library::Watch::new("DayDate 36")).unwrap();
    let daydate = lib.find("DayDate 36").unwrap().watch.id.clone();
    app.pick_watch(&daydate);
    assert_eq!(app.take_on_show(), Some(shown));
    assert_eq!(
        app.watches_view.watch_open.as_deref(),
        Some(daydate.as_str())
    );
    let _ = std::fs::remove_dir_all(&base);
}

/// Picking another watch in the Watches list with a take on show: the
/// take is put away, as Unsorted Takes and the Watch line do, so the
/// Bench never names a watch the take isn't in; picking its own watch
/// keeps it, and marks not yet saved are asked about first (#1303).
#[test]
fn picking_another_watch_puts_the_take_on_show_away() {
    let base = std::env::temp_dir().join(format!("tg-app-pick-away-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let pepsi = lib.add(&library::Watch::new("Pepsi")).unwrap();
    lib.add(&library::Watch::new("Daytona")).unwrap();
    let id = |name: &str| lib.find(name).unwrap().watch.id.clone();
    let (pepsi_id, daytona_id) = (id("Pepsi"), id("Daytona"));
    drop(lib);
    let take = pepsi.join(library::TAKES).join("2026-10-20 10.00 Dial Up");
    std::fs::create_dir_all(&take).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&take.join("take_part01-001.wav"), &audio).unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "watch = \"Pepsi\"\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    let open = |app: &mut TimegrapherApp| {
        app.pick_watch(&pepsi_id);
        app.take_action(Some(watches::TakeAction::Open(take.clone())));
        let t = Instant::now();
        while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 60 {
            app.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(app.take_on_show().as_ref(), Some(&take));
    };
    // Its own watch: it stays.
    open(&mut app);
    app.pick_watch(&pepsi_id);
    assert_eq!(app.take_on_show().as_ref(), Some(&take));
    // Another: it is put away, and the Bench names the one picked.
    app.pick_watch(&daytona_id);
    assert!(app.take_on_show().is_none() && app.bench.recording.live.is_none());
    assert_eq!(app.bench.take.watch, "Daytona");
    assert!(
        app.status_words().contains("Daytona"),
        "{}",
        app.status_words()
    );
    // With its marks not yet saved, they are asked about first, as the
    // Watch line asks; until then the take and its own watch stay named.
    open(&mut app);
    app.bench.take.brush = Some(Position::DialDown);
    app.paint_position(0.0, 5.0);
    assert!(app.needs_save_confirmation());
    // Its own watch: nothing to ask, and it stays.
    app.pick_watch(&pepsi_id);
    assert!(app.confirm.is_none());
    assert_eq!(app.take_on_show().as_ref(), Some(&take));
    app.pick_watch(&daytona_id);
    assert!(matches!(&app.confirm, Some(Confirm::PickWatch(Some(i))) if *i == daytona_id));
    assert_eq!(app.take_on_show().as_ref(), Some(&take));
    assert_eq!(
        app.watches_view.watch_open.as_deref(),
        Some(pepsi_id.as_str())
    );
    assert!(
        app.status_words().contains("Pepsi") && !app.status_words().contains("Daytona"),
        "{}",
        app.status_words()
    );
    // Don't Save: it goes, and Daytona is picked.
    app.confirm = None;
    app.line_pick(Some(daytona_id.clone()), true);
    assert!(app.take_on_show().is_none());
    assert_eq!(app.bench.take.watch, "Daytona");

    // A take from Unsorted Takes, in no watch: picking one puts it away too.
    let unsorted = root
        .join(library::UNSORTED)
        .join(library::TAKES)
        .join("2026-10-21 10.00 Dial Up");
    std::fs::create_dir_all(&unsorted).unwrap();
    for f in ["take_part01-001.wav", MANIFEST] {
        std::fs::copy(take.join(f), unsorted.join(f)).unwrap();
    }
    app.use_library(Some(root.clone()));
    app.pick_unsorted();
    app.take_action(Some(watches::TakeAction::Open(unsorted.clone())));
    let t = Instant::now();
    while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 60 {
        app.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(app.take_on_show().as_ref(), Some(&unsorted));
    app.pick_watch(&pepsi_id);
    assert!(app.take_on_show().is_none());
    assert_eq!(app.bench.take.watch, "Pepsi");
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// A take with no session.toml, filed under a watch: it moves without an
/// error, and no session.toml is made for it (#1283).
#[test]
fn a_take_with_no_session_toml_is_filed_without_one() {
    let base = std::env::temp_dir().join(format!("tg-app-file-bare-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    let take = lib.unsorted_dir().unwrap().join("2026-10-20 10.00 Dial Up");
    drop(lib);
    std::fs::create_dir_all(&take).unwrap();
    std::fs::write(
        take.join("session.json"),
        r#"{"started_utc":"2026-10-20T17:00:00Z","duration_s":40.0}"#,
    )
    .unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.watches_view.take_readings.wait();
    app.move_take(&take, &pepsi);
    let filed = root
        .join("Pepsi")
        .join(library::TAKES)
        .join(file_name(&take));
    assert!(filed.join("session.json").is_file() && !take.exists());
    assert!(
        app.message
            .as_ref()
            .is_some_and(|(m, error)| !error && m.contains("Filed")),
        "{:?}",
        app.message
    );
    assert!(!filed.join(MANIFEST).exists());
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
    next.bench.keeping.kept = kept;
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
    while next.bench.recording.batch.is_some() && t.elapsed().as_secs() < 60 {
        next.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    let live = next.bench.recording.live.as_ref().expect("opened");
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
    // The hour codes, in capitals: the 45° stops have no other.
    assert_eq!(p("ym42_7H30_48000.wav"), Some(Position::Up7h30));
    assert_eq!(p("rolex_6H.wav"), Some(Position::CrownLeft));
    assert_eq!(p("run_12h.flac"), None);
}

/// Wait for the analysis being kept, then for the take being opened.
fn wait_for_jobs(app: &mut TimegrapherApp) {
    let t = Instant::now();
    while (app.bench.keeping.analysis_save.is_some() || app.bench.recording.batch.is_some())
        && t.elapsed().as_secs() < 180
    {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        app.bench.keeping.analysis_save.is_none() && app.bench.recording.batch.is_none(),
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
    app.bench.take.bph = Some(28800);
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
        app.bench.recording.live.as_mut().unwrap().push(samples);
    }
    app.bench.take.masked = vec![(30.0, 32.0)];
    // A calibre with its own escape teeth: the kept analysis is read with
    // them, as the take is opened (#1324).
    app.cal
        .user
        .custom
        .push(timegrapher_core::calibre_store::CustomCalibre {
            name: "Test 21".into(),
            maker: None,
            aliases: Vec::new(),
            bph: Some(28800),
            lift_angle_deg: None,
            escape_teeth: Some(21),
            escapement: None,
            wheels: Vec::new(),
            notes: None,
        });
    app.bench.take.calibre = "Test 21".into();
    assert!(app.can_save_analysis());
    app.start_analysis_save(&base.join("kept"));
    assert!(app.analysis_save_busy(), "nothing else while it reads");
    wait_for_jobs(&mut app);
    assert!(
        !app.message.as_ref().is_some_and(|m| m.1),
        "{:?}",
        app.message
    );
    let (kept, _) = app.bench.keeping.analysis_kept_to.clone().expect("kept");
    assert!(app.session.saved_to().is_none(), "the sound isn't saved");
    assert!(
        file_name(&kept).ends_with("_analysis"),
        "{}",
        kept.display()
    );
    assert!(analysis_take::is_analysis_only(&kept));
    assert!(recordings_in(&kept).is_err(), "no sound in it");
    let header = analysis_take::read(&kept).unwrap().header;
    assert_eq!(header.settings["escape_teeth"], 21);
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
    assert!(a.bench.keeping.no_sound.is_some() && b.bench.keeping.no_sound.is_none());
    assert_eq!(a.bench.take.masked, b.bench.take.masked);
    assert_eq!(a.bench.take.masked, vec![(30.0, 32.0)]);
    let (la, lb) = (
        a.bench.recording.live.as_ref().unwrap(),
        b.bench.recording.live.as_ref().unwrap(),
    );
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
    assert_eq!(a.bench.results.trend.len(), b.bench.results.trend.len());
    // The profiles kept for each 2 seconds stand in for the sound.
    let k = a.bench.keeping.no_sound.as_deref().expect("kept");
    assert!(k.len() >= 29, "{}", k.len());
    assert!(k
        .iter()
        .all(|p| p.sides[0].is_some() && p.sides[1].is_some()));
    // Moved back, the Tick Tock Profile averages those in the Average
    // Over window, and says so (#979).
    a.bench.take.average_s = 10.0;
    let shown = a.profiles_at(20.0);
    let hover = shown.hover.expect("what it is made of");
    assert!(
        hover.contains("Tick: average of 5 2 s profiles, 0:10 to 0:20"),
        "{hover}"
    );
    assert!(hover.contains("Tock: average of 5 2 s profiles"), "{hover}");
    assert!(hover.contains("Band: range of the 2 s bands"), "{hover}");
    assert!(shown.line.is_none() && shown.profiles.iter().all(Option::is_some));
    assert_eq!(shown.band, crate::profiles::BAND_AVERAGED);
    // A break in the window leaves out the stretch it falls in.
    a.bench.take.take_breaks = Some(vec![timegrapher_core::breaks::Break {
        audio_s: 15.0,
        gap_s: 0.0,
    }]);
    let hover = a.profiles_at(20.0).hover.expect("what it is made of");
    assert!(
        hover.contains("Tick: average of 4 2 s profiles, 0:10 to 0:20"),
        "{hover}"
    );
    a.bench.take.take_breaks = None;
    // One stretch is the 2 s profile, not an average of one.
    a.bench.take.average_s = 2.0;
    let hover = a.profiles_at(20.0).hover.expect("what it is made of");
    assert!(
        hover.contains("Tick: the 2 s profile, 0:18 to 0:20"),
        "{hover}"
    );
    // Off the 2 s grid, too (Walter).
    let hover = a.profiles_at(19.3).hover.expect("what it is made of");
    assert!(
        hover.contains("Tick: the 2 s profile, 0:18 to 0:20"),
        "{hover}"
    );
    // A masked moment is masked, as with the sound.
    let shown = a.profiles_at(31.0);
    assert!(shown.masked && shown.profiles.iter().all(Option::is_none));
    assert_eq!(shown.line.as_deref(), Some("Masked at 0:31"));
    // Masks changed on it are saved into its session file, which still
    // says it has no sound.
    a.bench.take.masked = vec![(30.0, 32.0), (50.0, 52.0)];
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
    assert_eq!(a.bench.take.masked, vec![(30.0, 32.0), (50.0, 52.0)]);
    for f in recordings_in(&odd).unwrap() {
        std::fs::remove_file(f).unwrap();
    }
    let mut d = TimegrapherApp::with_devices(Vec::new(), None);
    d.open_recording(&odd, false);
    let (msg, error) = d.message.clone().unwrap();
    assert!(error && msg.contains("audio = false"), "{msg}");
    let _ = std::fs::remove_dir_all(&base);
}

/// A library at `base/Timegrapher Data` holding one watch with one take
/// whose `session.json` names a WAV hour, as saved before hours were kept
/// as FLAC: the take's folder and the WAV's samples as read back.
fn wav_library(tag: &str) -> (PathBuf, PathBuf, PathBuf, Vec<f32>) {
    use timegrapher_core::synth::{generate, SynthConfig};
    let base = std::env::temp_dir().join(format!("tg-app-keep-wavs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let dir = lib.add(&library::Watch::new("Pepsi")).unwrap();
    drop(lib);
    let take = dir.join(library::TAKES).join("2026-10-20 14.32 Dial Up");
    std::fs::create_dir_all(&take).unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "bph = 28800\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 5.0,
            ..Default::default()
        },
        |_| 270.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&take.join("audio-001.wav"), &audio).unwrap();
    let heard = timegrapher_core::audio::load(&take.join("audio-001.wav"))
        .unwrap()
        .samples;
    std::fs::write(
        take.join("session.json"),
        r#"{"started_utc":"2026-10-20T21:32:00Z","duration_s":5.0,"segments":["audio-001.wav"]}"#,
    )
    .unwrap();
    (base, root, take, heard)
}

/// Poll the keeping of saved WAVs until it ends, for at most 30 s.
fn wav_keep_ends(app: &mut TimegrapherApp) {
    let until = Instant::now() + std::time::Duration::from_secs(30);
    while app.bench.keeping.wav_keep.is_some() && Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.poll_wav_keep();
    }
    assert!(app.bench.keeping.wav_keep.is_none(), "ended within 30 s");
}

/// At start-up a saved take whose hour is still a WAV (saved before hours
/// were kept as FLAC, #503) is kept as FLAC in the background: the status
/// bar says so while it works, and the library is read again after, its
/// session.json naming the FLAC.
#[test]
fn saved_wav_hours_are_kept_as_flac_at_start_up() {
    let (base, root, take, heard) = wav_library("ok");
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.keep_saved_wavs();
    let words = app
        .bench
        .keeping
        .wav_keep
        .as_ref()
        .expect("one take to keep")
        .words();
    assert!(words.starts_with("Keeping saved takes as FLAC:"), "{words}");
    assert!(words.ends_with("of 1"), "{words}");
    // The library doesn't move while its takes are being converted in
    // place (Duncan's note on #1120).
    app.library_action(settings_window::LibraryAction::Move);
    assert!(app.bench.keeping.library_move_ask.is_none());
    assert_eq!(
        app.message,
        Some((library_folder::WAV_KEEP_WAIT.to_string(), false))
    );
    app.message = None;
    wav_keep_ends(&mut app);
    assert!(take.join("audio-001.flac").is_file());
    assert!(!take.join("audio-001.wav").exists());
    let session = std::fs::read_to_string(take.join("session.json")).unwrap();
    assert!(session.contains("\"audio-001.flac\""), "{session}");
    assert_eq!(
        timegrapher_core::audio::load(&take.join("audio-001.flac"))
            .unwrap()
            .samples,
        heard
    );
    assert!(app.message.is_none(), "{:?}", app.message);
    // The library is read again: its take's size is the FLAC's.
    let lib = app.library.as_ref().unwrap().as_ref().unwrap();
    let flac = std::fs::metadata(take.join("audio-001.flac"))
        .unwrap()
        .len();
    assert!(lib.watches[0].takes[0].bytes >= flac);
    // Nothing is left to do on the next start.
    app.keep_saved_wavs();
    assert!(app.bench.keeping.wav_keep.is_none());
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// The take on show is never converted from under its analysis: its
/// hours stay WAVs, for the next start (Peter's 65 h take, #503).
#[test]
fn the_take_on_show_keeps_its_wavs_for_another_time() {
    let (base, root, take, _) = wav_library("open");
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.bench.keeping.opened_path = take.display().to_string();
    app.session.set_state(session::State::Analysed);
    app.keep_saved_wavs();
    wav_keep_ends(&mut app);
    assert!(take.join("audio-001.wav").is_file());
    assert!(!take.join("audio-001.flac").exists());
    assert!(app.message.is_none(), "{:?}", app.message);
    // A session folder on show holds the take: it is left too (Trevor).
    app.bench.keeping.opened_path = take.parent().unwrap().display().to_string();
    app.keep_saved_wavs();
    wav_keep_ends(&mut app);
    assert!(take.join("audio-001.wav").is_file());
    assert!(!take.join("audio-001.flac").exists());
    // Closed, the next start keeps it.
    app.session.set_state(session::State::Idle);
    app.keep_saved_wavs();
    wav_keep_ends(&mut app);
    assert!(take.join("audio-001.flac").is_file());
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// The take the session saves into is left alone too, recording or
/// stopped: Save Again writes its session file from memory, naming the
/// WAV, so its hours are never converted under it (#1988).
#[test]
fn the_take_saved_into_keeps_its_wavs_for_another_time() {
    let (base, root, take, _) = wav_library("held");
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    let metadata = app.session.metadata_snapshot(app.save_metadata());
    app.session.mark_saved(5.0, take.clone(), metadata);
    assert_eq!(app.take_on_show(), None);
    app.keep_saved_wavs();
    assert!(app.bench.keeping.wav_keep.is_some(), "one take to keep");
    wav_keep_ends(&mut app);
    assert!(take.join("audio-001.wav").is_file());
    assert!(!take.join("audio-001.flac").exists());
    assert!(app.message.is_none(), "{:?}", app.message);
    // Let go of, the next start keeps it.
    app.new_session();
    app.keep_saved_wavs();
    wav_keep_ends(&mut app);
    assert!(take.join("audio-001.flac").is_file());
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// With another copy of the app holding the library (read-only here),
/// nothing is converted: that copy does it, and two never convert the
/// same hour at once.
#[test]
fn a_library_another_copy_has_keeps_its_wavs() {
    let (base, root, take, _) = wav_library("other");
    let other = library::Library::open(&root).unwrap();
    assert!(other.read_only.is_none());
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    let lib = app.library.as_ref().unwrap().as_ref().unwrap();
    assert!(lib.read_only.is_some(), "another copy has it");
    app.keep_saved_wavs();
    assert!(app.bench.keeping.wav_keep.is_none());
    assert!(take.join("audio-001.wav").is_file());
    assert!(!take.join("audio-001.flac").exists());
    drop((app, other));
    let _ = std::fs::remove_dir_all(&base);
}

/// An hour that can't be kept as FLAC stays a whole WAV, and the status
/// line names it; the session file still names the WAV.
#[test]
fn an_hour_that_cant_be_kept_as_flac_stays_a_wav_and_is_named() {
    let (base, root, take, _) = wav_library("bad");
    // Not a sound file under its WAV name.
    std::fs::write(take.join("audio-001.wav"), b"not a wav").unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.keep_saved_wavs();
    assert!(app.bench.keeping.wav_keep.is_some());
    wav_keep_ends(&mut app);
    let (m, bad) = app.message.clone().expect("the failure said");
    assert!(bad && m.contains("audio-001.wav"), "{m}");
    assert_eq!(
        std::fs::read(take.join("audio-001.wav")).unwrap(),
        b"not a wav"
    );
    let session = std::fs::read_to_string(take.join("session.json")).unwrap();
    assert!(session.contains("\"audio-001.wav\""), "{session}");
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// Just Open It opens a recording from outside the watches as no watch's
/// take: the watch picked is let go, so the status line and the cards
/// name the take's own watch, and the next take is a Quick Listen's.
#[test]
fn just_open_it_lets_the_watch_picked_go() {
    let base = std::env::temp_dir().join(format!("tg-app-just-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Alpha")).unwrap();
    let alpha = lib.find("Alpha").unwrap().watch.id.clone();
    drop(lib);
    let take = base.join("outside take");
    std::fs::create_dir_all(&take).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&take.join("take.wav"), &audio).unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "watch = 'Yacht-Master'\ncalibre = '3235'\nbph = 28800\nlift = 55.0\n\n\
         [[recording]]\nfile = 'take.wav'\nposition = 'CR'\n",
    )
    .unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.pick_watch(&alpha);
    // As Open's dialog does (`open_picked`).
    app.bench.recording.input = Input::File;
    let mut a = app.open_ask_for(&take).expect("asked");
    a.add = false;
    let need = app.open_need(&a);
    app.answer_open_ask(a, need);
    let t = Instant::now();
    while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 120 {
        app.poll();
    }
    assert!(app.watches_view.watch_open.is_none() && !app.watches_view.unsorted_open);
    assert_eq!(app.status_words(), "outside take · Yacht-Master · Opened");
    assert_eq!(app.bench.take.calibre, "3235");
    // The next take starts from Quick Listen's settings, not Alpha's.
    app.new_session();
    assert_eq!(app.watch_setup(), WatchSetup::default());
    let _ = std::fs::remove_dir_all(&base);
}

/// A new watch made while adding a take starts with what the take says
/// of its calibre, beat rate and lift, so the two agree (#1266). Where the
/// calibre table gives a beat rate or lift, the watch takes the table's,
/// as Edit… keeps it, not the take's (Vernon on #1273).
#[test]
fn a_new_watch_made_for_a_take_takes_its_calibre_beat_rate_and_lift() {
    let known = timegrapher_core::calibres::find("2824-2").expect("ETA 2824-2 in the table");
    let table_lift = known.lift_angle_deg.expect("a lift for the 2824-2");
    for (n, calibre) in ["Unlisted 1", known.calibre.as_str()]
        .into_iter()
        .enumerate()
    {
        let base =
            std::env::temp_dir().join(format!("tg-app-new-for-take-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        library::Library::create(&root).unwrap();
        let take = base.join("outside take");
        std::fs::create_dir_all(&take).unwrap();
        let audio = generate(
            &SynthConfig {
                duration_s: 20.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        timegrapher_core::audio::write_wav(&take.join("take.wav"), &audio).unwrap();
        std::fs::write(
            take.join(MANIFEST),
            format!(
                "calibre = '{calibre}'\nbph = 28800\nlift = 55.0\n\n\
                 [[recording]]\nfile = 'take.wav'\nposition = 'DU'\n"
            ),
        )
        .unwrap();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        app.bench.recording.input = Input::File;
        let mut a = app.open_ask_for(&take).expect("asked");
        a.add = true;
        a.new_watch = true;
        a.new_name = "YM42 roam".into();
        a.started = "2026-10-22 14:30".into();
        let need = app.open_need(&a);
        app.answer_open_ask(a, need);
        assert!(
            app.bench.keeping.open_ask.is_none(),
            "added: {:?}",
            app.bench
                .keeping
                .open_ask
                .as_ref()
                .and_then(|a| a.error.clone())
        );
        let lib = library::Library::open(&root).unwrap();
        let w = &lib.find("YM42 roam").expect("the new watch").watch;
        assert_eq!(w.calibre.as_deref(), Some(calibre));
        // What a Listen on the watch, and its Edit…, start with.
        if n == 0 {
            assert_eq!((w.bph, w.lift), (Some(28800), Some(55.0)));
            assert_eq!(app.cal.watch_lift(w), Some(55.0));
            assert_eq!(app.cal.watch_bph(w), Some(28800));
        } else {
            assert_eq!(
                (w.bph, w.lift),
                (None, None),
                "the table's, not stored twice"
            );
            assert_eq!(app.cal.watch_lift(w), Some(table_lift));
            assert_eq!(app.cal.watch_bph(w), known.bph);
        }
        let t = Instant::now();
        while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 120 {
            app.poll();
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}

/// A New Watch made for a recording that can't be added is taken back, so
/// it isn't left empty in the watches and a second try can use its name
/// (#1275).
#[cfg(unix)]
#[test]
fn a_new_watch_for_a_recording_that_cant_be_added_is_taken_back() {
    let base = std::env::temp_dir().join(format!("tg-app-add-undo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    library::Library::create(&root).unwrap();
    let take = base.join("outside take");
    std::fs::create_dir_all(&take).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&take.join("take.wav"), &audio).unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "bph = 28800\nlift = 55.0\n\n[[recording]]\nfile = 'take.wav'\nposition = 'DU'\n",
    )
    .unwrap();
    // A link in the take, which can't be copied.
    let link = take.join("notes.txt");
    std::os::unix::fs::symlink(take.join("take.wav"), &link).unwrap();
    // Two watches already there with no takes: one with nothing in it,
    // one with a photo.
    let mut lib = library::Library::open(&root).unwrap();
    lib.add(&library::Watch::new("Alpha")).unwrap();
    let pictured = lib.add(&library::Watch::new("Pictured")).unwrap();
    std::fs::write(pictured.join(library::PHOTOS).join("dial.jpg"), b"jpeg").unwrap();
    let alpha = lib.find("Alpha").unwrap().watch.id.clone();
    let photo = lib.find("Pictured").unwrap().watch.id.clone();
    // One with a photo is never taken back.
    let err = lib.remove_empty(&photo).unwrap_err();
    assert!(err.contains("has takes or photos"), "{err}");
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.bench.recording.input = Input::File;
    // A failed add into a watch already there leaves it, empty or not.
    let mut a = app.open_ask_for(&take).expect("asked");
    a.add = true;
    a.new_watch = false;
    a.watch = Some(alpha);
    a.started = "2026-10-22 14:30".into();
    let need = app.open_need(&a);
    app.answer_open_ask(a, need);
    let asked = app.bench.keeping.open_ask.as_ref();
    assert!(asked.is_some_and(|a| a.error.is_some()));
    app.bench.keeping.open_ask = None;
    let ask = |app: &TimegrapherApp| {
        let mut a = app.open_ask_for(&take).expect("asked");
        a.add = true;
        a.new_watch = true;
        a.new_name = "YM42 roam".into();
        a.started = "2026-10-22 14:30".into();
        a
    };
    let a = ask(&app);
    let need = app.open_need(&a);
    app.answer_open_ask(a, need);
    let asked = app.bench.keeping.open_ask.as_ref();
    let why = asked.and_then(|a| a.error.clone());
    assert!(
        why.as_deref().is_some_and(|w| w.contains("link")),
        "{why:?}"
    );
    let names = |root: &Path| -> Vec<String> {
        library::Library::open(root)
            .unwrap()
            .watches
            .iter()
            .map(|e| e.watch.name.clone())
            .collect()
    };
    assert_eq!(
        names(&root),
        ["Alpha", "Pictured"],
        "only the new one taken back"
    );
    // Fixed, the same name is free to try again.
    std::fs::remove_file(&link).unwrap();
    // As the question does, still open with the reason in it.
    let mut a = app
        .bench
        .keeping
        .open_ask
        .take()
        .expect("the question stays open");
    a.error = None;
    let need = app.open_need(&a);
    app.answer_open_ask(a, need);
    assert!(
        app.bench.keeping.open_ask.is_none(),
        "{:?}",
        app.bench
            .keeping
            .open_ask
            .as_ref()
            .and_then(|a| a.error.clone())
    );
    assert_eq!(names(&root), ["Alpha", "Pictured", "YM42 roam"]);
    let t = Instant::now();
    while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 120 {
        app.poll();
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// Open's question guesses when a recording started from when its file
/// was last written, less its length, to the minute in this computer's
/// time; the minute typed back reads as that time. A minute that isn't
/// one reads as nothing, and no files give no guess.
#[test]
fn an_open_guesses_the_start_from_the_files_time_less_its_length() {
    let dir = std::env::temp_dir().join(format!("tg-app-started-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("take.wav");
    let audio = generate(
        &SynthConfig {
            duration_s: 90.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&wav, &audio).unwrap();
    let ended = parse_local_minute(" 2026-10-22 14:31 ").expect("a minute");
    std::fs::File::options()
        .write(true)
        .open(&wav)
        .unwrap()
        .set_modified(ended)
        .unwrap();
    let guess = started_guess(std::slice::from_ref(&wav)).expect("guessed");
    assert_eq!(
        guess.format("%Y-%m-%d %H:%M:%S").to_string(),
        "2026-10-22 14:29:30"
    );
    assert_eq!(
        parse_local_minute(&guess.format("%Y-%m-%d %H:%M").to_string()),
        parse_local_minute("2026-10-22 14:29")
    );
    // Of several files, the first started the take.
    let later = dir.join("later.wav");
    timegrapher_core::audio::write_wav(&later, &audio).unwrap();
    let two = [wav.clone(), later];
    assert_eq!(started_guess(&two), Some(guess));
    assert!(parse_local_minute("2026-10-22").is_none());
    assert!(parse_local_minute("22 Oct 2026 14:30").is_none());
    assert!(started_guess(&[]).is_none());
    assert!(started_guess(&[dir.join("gone.wav")]).is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

/// A take's size on its row counts every file in its folder, in folders
/// inside it too; a folder that can't be read counts as nothing.
#[test]
fn a_takes_size_counts_every_file_inside_it() {
    let dir = std::env::temp_dir().join(format!("tg-app-folder-size-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("inner").join("deeper")).unwrap();
    std::fs::write(dir.join("a"), [0u8; 10]).unwrap();
    std::fs::write(dir.join("inner").join("b"), [0u8; 200]).unwrap();
    std::fs::write(dir.join("inner").join("deeper").join("c"), [0u8; 3000]).unwrap();
    assert_eq!(folder_size(&dir), 3210);
    assert_eq!(folder_size(&dir.join("inner")), 3200);
    assert_eq!(folder_size(&dir.join("missing")), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

/// Keep Analysis Only keeps every file of the take that isn't its sound,
/// subfolders too (#1323), and reads the sound as core reads the take: a
/// session file that names only its calibre is read at the calibre's
/// beat rate, lift and teeth (#1324). Read at 52° instead of the
/// calibre's 59°, a watch at 108° reads under 100° and every amplitude
/// window is thrown away.
#[test]
fn keep_analysis_only_keeps_subfolders_and_the_calibres_settings() {
    let base = std::env::temp_dir().join(format!("tg-app-keep-nested-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let watch = lib.add(&library::Watch::new("Explorer")).unwrap();
    drop(lib);
    let take = watch.join(library::TAKES).join("2026-11-07 06.00 Dial Up");
    std::fs::create_dir_all(take.join("report/older")).unwrap();
    std::fs::create_dir_all(take.join("lift52")).unwrap();
    std::fs::write(take.join("report/notes.txt"), "kept by hand").unwrap();
    std::fs::write(take.join("report/older/a.txt"), "older").unwrap();
    std::fs::write(take.join("lift52/session.toml"), "lift = 52\n").unwrap();
    std::fs::write(take.join("clock.csv"), "").unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "calibre = \"Test 59\"\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 40.0,
            lift_deg: 59.0,
            ..Default::default()
        },
        |_| 108.0,
        |_| 0.0,
    );
    let wav = take.join("take_part01-001.wav");
    timegrapher_core::audio::write_wav(&wav, &audio).unwrap();

    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.cal
        .user
        .custom
        .push(timegrapher_core::calibre_store::CustomCalibre {
            name: "Test 59".into(),
            maker: None,
            aliases: Vec::new(),
            bph: Some(28800),
            lift_angle_deg: Some(59.0),
            escapement: None,
            escape_teeth: Some(20),
            wheels: Vec::new(),
            notes: None,
        });
    let m: Manifest =
        toml::from_str(&std::fs::read_to_string(take.join(MANIFEST)).unwrap()).unwrap();
    let a = app.keep_config(&m).analysis;
    assert_eq!(
        (a.bph, a.amplitude.lift_deg, a.lift_known, a.escape_teeth),
        (Some(28800), 59.0, true, 20)
    );
    app.use_library(Some(root));
    app.start_take_keep(take.clone());
    let t = Instant::now();
    while app.bench.keeping.take_keep.is_some() && t.elapsed().as_secs() < 120 {
        app.poll_take_keep();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(analysis_take::is_analysis_only(&take), "{:?}", app.message);
    assert!(!wav.exists());
    for (file, text) in [
        ("report/notes.txt", "kept by hand"),
        ("report/older/a.txt", "older"),
        ("lift52/session.toml", "lift = 52\n"),
        ("clock.csv", ""),
    ] {
        assert_eq!(
            std::fs::read_to_string(take.join(file)).ok().as_deref(),
            Some(text),
            "{file}"
        );
    }
    let stored = analysis_take::read(&take).unwrap();
    assert_eq!(stored.header.lift_deg, 59.0);
    assert_eq!(stored.header.bph, 28800);
    let kept = stored
        .log
        .amplitude_windows
        .iter()
        .filter(|w| w.even_deg.is_some())
        .count();
    assert!(kept > 0, "no amplitude window kept");
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// Sound left at the top of a take beside its FLAC is the same sound and
/// goes with it, and sound in a subfolder, which isn't the take's own,
/// is refused before anything changes: no sound is ever kept with an
/// analysis (#1323, Norm).
#[test]
fn keep_analysis_only_keeps_no_sound() {
    let base = std::env::temp_dir().join(format!("tg-app-keep-sound-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let watch = lib.add(&library::Watch::new("Explorer")).unwrap();
    drop(lib);
    let take = watch.join(library::TAKES).join("2026-11-07 06.00 Dial Up");
    std::fs::create_dir_all(take.join("parts")).unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "bph = 28800\nlift = 52\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let wav = take.join("take_part01-001.wav");
    timegrapher_core::audio::write_wav(&wav, &audio).unwrap();
    let nested = take.join("parts/audio-001.wav");
    timegrapher_core::audio::write_wav(&nested, &audio).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root));
    let keep = |app: &mut TimegrapherApp| {
        app.start_take_keep(take.clone());
        let t = Instant::now();
        while app.bench.keeping.take_keep.is_some() && t.elapsed().as_secs() < 120 {
            app.poll_take_keep();
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    keep(&mut app);
    assert!(
        format!("{:?}", app.message).contains("audio-001.wav"),
        "{:?}",
        app.message
    );
    assert!(wav.exists() && nested.exists() && !analysis_take::is_analysis_only(&take));
    // The subfolder's sound moved out; the top WAV kept as FLAC, the WAV
    // left beside it.
    std::fs::remove_dir_all(take.join("parts")).unwrap();
    let flac = timegrapher_core::flac::flac_path(&wav);
    timegrapher_core::flac::encode_wav(&wav, &flac).unwrap();
    keep(&mut app);
    assert!(analysis_take::is_analysis_only(&take), "{:?}", app.message);
    assert!(!wav.exists() && !flac.exists());
    let left: Vec<_> = walk(&take)
        .into_iter()
        .filter(|p| timegrapher_core::take::is_sound(p))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// Every file under `dir`, subfolders too.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// A take holding anything Keep Analysis Only can't keep, such as a
/// link, is refused before anything changes (#1323).
#[cfg(unix)]
#[test]
fn keep_analysis_only_refuses_a_take_with_a_link() {
    let base = std::env::temp_dir().join(format!("tg-app-keep-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let watch = lib.add(&library::Watch::new("Explorer")).unwrap();
    drop(lib);
    let take = watch.join(library::TAKES).join("2026-11-07 06.00 Dial Up");
    std::fs::create_dir_all(take.join("report")).unwrap();
    std::fs::write(base.join("elsewhere.txt"), "outside").unwrap();
    std::os::unix::fs::symlink(base.join("elsewhere.txt"), take.join("report/link.txt")).unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "bph = 28800\nlift = 52\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let audio = generate(&SynthConfig::default(), |_| 280.0, |_| 0.0);
    let wav = take.join("take_part01-001.wav");
    timegrapher_core::audio::write_wav(&wav, &audio).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root));
    app.start_take_keep(take.clone());
    assert!(app.bench.keeping.take_keep.is_none());
    assert!(
        format!("{:?}", app.message).contains("link.txt"),
        "{:?}",
        app.message
    );
    assert!(wav.exists() && !analysis_take::is_analysis_only(&take));
    assert!(take.join("report/link.txt").exists());
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// A take another program adds while the app has the Watches folder open
/// (`timegrapher watches add-take`, which may add without the lock) is
/// listed once the window comes back to the front, without a restart.
#[test]
fn a_take_added_from_outside_shows_when_the_window_is_back_in_front() {
    let base = std::env::temp_dir().join(format!("tg-app-take-outside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    let takes = |app: &TimegrapherApp| match &app.library {
        Ok(Some(l)) => l.find("Pepsi").unwrap().takes.len(),
        _ => unreachable!(),
    };
    assert_eq!(takes(&app), 0);
    // Another program, while the app holds the lock.
    let from = base.join("outside take");
    std::fs::create_dir_all(&from).unwrap();
    std::fs::write(from.join(MANIFEST), "[[recording]]\nfile = \".\"\n").unwrap();
    let mut other = library::Library::open_to_add(&root).unwrap();
    assert!(other.read_only.is_some(), "the app holds the lock");
    let id = other.find("Pepsi").unwrap().watch.id.clone();
    let watch = |lib: &mut library::Library| lib.update(&library::Watch::new("x"));
    assert!(watch(&mut other).is_err(), "nothing but a take is added");
    other.add_take(&from, &id, true).unwrap();
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        app.reread_library_on_focus(ctx)
    });
    assert_eq!(
        takes(&app),
        0,
        "not read again while the window stays in front"
    );
    let back = egui::RawInput {
        events: vec![egui::Event::WindowFocused(true)],
        ..Default::default()
    };
    let _ = ctx.run(back, |ctx| app.reread_library_on_focus(ctx));
    assert_eq!(takes(&app), 1);
    let _ = std::fs::remove_dir_all(&base);
}

/// Stop judges the newest amplitude chunk at once, as a read of the take
/// judges its last (#1738, Trevor on #1800): a side too faint against the
/// same side's in its chunk, which listening withholds only once the
/// chunk closes, is withheld when listening stops.
#[test]
fn a_stop_withholds_the_faint_sides_of_the_open_chunk() {
    let base = std::env::temp_dir().join(format!("tg-stop-settle-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "syn".into());
    // The swing smeared inside each 2 s window from 60 s on, as in core's
    // equivalence test; the take stops 170 s in, in the chunk from 141 s.
    let cfg = SynthConfig {
        duration_s: 170.0,
        snr_db: 26.0,
        seed: 1738,
        ..Default::default()
    };
    let swing = |t: f64| {
        if (60.0..150.0).contains(&t) {
            200.0 + 60.0 * (t * 1.7).sin()
        } else {
            270.0
        }
    };
    let audio = generate(&cfg, swing, |_| 0.0);
    for b in audio.samples.chunks(4800) {
        app.bench.recording.live.as_mut().unwrap().push(b);
    }
    let faint_from = |app: &TimegrapherApp, from_s: f64| {
        let live = app.bench.recording.live.as_ref().unwrap();
        let wins = live.amplitude_windows().iter();
        wins.filter(|w| w.start_s >= from_s && w.faint_withheld().contains(&true))
            .count()
    };
    assert_eq!(faint_from(&app, 141.0), 0, "not judged while listening");
    let _tx = super::tests::queued_capture(&mut app, &base);
    app.pause();
    assert!(faint_from(&app, 141.0) > 0, "judged at the stop");
    let _ = std::fs::remove_dir_all(base);
}

/// A finished test whose take is kept by Save Analysis Only has no sound
/// to be read again from: its table says its figures are the listening's,
/// and its file says `listened` (#2070, Hugh 5's words). A full save never
/// shows that note: its test is read again instead.
#[test]
fn a_test_kept_without_its_sound_says_its_figures_are_the_listenings() {
    let base = std::env::temp_dir().join(format!("tg-app-2070-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.bench.take.bph = Some(28800);
    app.reset_session(48000, "test".into());
    app.session.listen();
    let audio = generate(
        &SynthConfig {
            duration_s: 30.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let info = app.info("test", 48000, 16);
    app.session.recorder = Some(Recorder::start(&base.join("live"), info).unwrap());
    let began = std::time::SystemTime::now();
    for (k, second) in audio.samples.chunks(48000).enumerate() {
        let block = timegrapher_core::capture::Block {
            samples: second.to_vec(),
            at: began + std::time::Duration::from_secs(k as u64 + 1),
        };
        let r = app.session.recorder.as_mut().unwrap();
        r.write(&block).unwrap();
    }
    for samples in audio.samples.chunks(960) {
        app.bench.recording.live.as_mut().unwrap().push(samples);
    }
    // A test not finished is left as it is, with no note (Trevor on
    // #2080).
    app.bench.test.guided = Some(guided::Test::poise());
    assert!(app.can_save_analysis());
    app.start_analysis_save(&base.join("unfinished"));
    wait_for_jobs(&mut app);
    assert!(
        app.bench.keeping.analysis_kept_to.is_some(),
        "{:?}",
        app.message
    );
    let t = app.bench.test.guided.as_ref().unwrap();
    assert_eq!(t.saved_read, guided::SavedRead::NotRead);
    assert_eq!(t.saved_read_note(), None);
    app.bench.keeping.analysis_kept_to = None;
    let mut test = guided::Test::poise();
    test.saved_read = guided::SavedRead::Pending;
    app.bench.test.guided = Some(test);
    assert_eq!(
        app.bench.test.guided.as_ref().unwrap().saved_read_note(),
        None
    );
    assert!(app.can_save_analysis());
    app.start_analysis_save(&base.join("kept"));
    wait_for_jobs(&mut app);
    assert!(
        app.bench.keeping.analysis_kept_to.is_some(),
        "{:?}",
        app.message
    );
    let t = app.bench.test.guided.as_ref().unwrap();
    assert_eq!(t.saved_read, guided::SavedRead::NoSound);
    assert!(t.listened());
    assert_eq!(
        t.saved_read_note().as_deref(),
        Some(
            "These are the figures heard while listening: the take was saved without its \
             sound, so it couldn't be analysed again. Opened again, the take may read slightly \
             differently."
        )
    );
    // Saved in full after: read again from the sound that save keeps, so
    // the note goes (Hugh 5 on #2080).
    app.save_into(&base.join("full"));
    app.wait_for_save();
    assert!(app.session.saved_to().is_some());
    let t = app.bench.test.guided.as_ref().unwrap();
    assert_ne!(t.saved_read, guided::SavedRead::NoSound);
    assert_ne!(t.saved_read, guided::SavedRead::Pending);
    let _ = std::fs::remove_dir_all(&base);
}

/// A test's take kept with Save Analysis Only… is kept: a new session
/// after it leaves the wind its Start set, not the one before (#2001,
/// Trevor on #2136).
#[test]
fn a_test_kept_as_its_analysis_keeps_its_starts_wind() {
    use super::wind::WindPick;
    let base = std::env::temp_dir().join(format!("tg-app-wind-analysis-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.bench.take.bph = Some(28800);
    app.reset_session(48000, "test".into());
    app.session.listen();
    let audio = generate(
        &SynthConfig {
            duration_s: 30.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let info = app.info("test", 48000, 16);
    app.session.recorder = Some(Recorder::start(&base.join("live"), info).unwrap());
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
        app.bench.recording.live.as_mut().unwrap().push(samples);
    }
    app.pick_wind(WindPick::HoursAgo(5.0));
    app.started_from_full_wind(false);
    assert!(app.can_save_analysis());
    app.start_analysis_save(&base.join("kept"));
    wait_for_jobs(&mut app);
    assert!(
        app.bench.keeping.analysis_kept_to.is_some(),
        "{:?}",
        app.message
    );
    app.new_session();
    assert_eq!(app.wind_field().words.value, "Full 0.0 h ago");
    let _ = std::fs::remove_dir_all(&base);
}

/// A Stop read that fails says so, in Hugh's words, and the cards read
/// the live engine, as before (#2164).
#[test]
fn a_stop_read_that_fails_says_so() {
    let base = std::env::temp_dir().join(format!("tg-app-stop-fails-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "test".into());
    app.session.listen();
    let info = app.info("test", 48000, 16);
    app.session.recorder = Some(Recorder::start(&base.join("live"), info).unwrap());
    let tx = super::tests::queued_capture(&mut app, &base);
    let audio = generate(
        &SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    for (k, samples) in audio.samples.chunks(960).enumerate() {
        let at =
            std::time::UNIX_EPOCH + std::time::Duration::from_micros(3_000_000 + k as u64 * 20_000);
        let samples = samples.to_vec();
        tx.send(timegrapher_core::capture::Event::Audio(
            timegrapher_core::capture::Block { samples, at },
        ))
        .unwrap();
    }
    while app.poll_for(std::time::Duration::from_secs(60)) > 0 {}
    app.stop();
    assert!(app.stop_read_waits().is_some(), "the take stopped is read");
    app.break_stop_read();
    app.poll_stop_read();
    assert_eq!(
        app.message,
        Some((
            "The take stopped couldn't be analysed whole\nThe cards show the figures heard \
             while listening (it stopped part way). Saved and opened again, the take may read \
             slightly differently."
                .to_string(),
            true
        ))
    );
    assert!(app.stop_read_waits().is_none() && app.stopped_take().is_none());
    assert!(
        app.card_reading().is_some(),
        "the cards read live, as before"
    );
    let _ = std::fs::remove_dir_all(&base);
}
