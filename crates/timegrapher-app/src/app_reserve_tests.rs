//! The Power Reserve test, headless: where it runs, what it keeps and
//! how its take is filed.

use super::tests::painted_text;
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

/// Quick Listen asks before unsaved sound goes, then shows Unsorted
/// and listens with no watch picked; Stop keeps nothing, Save… saves
/// the take into Unsorted, which the list shows at its top, and File
/// Under a Watch moves it into a watch. Picking a watch ends it.
#[test]
fn a_power_reserve_test_holds_its_position_and_keeps_its_take_with_its_run() {
    let base = std::env::temp_dir().join(format!("tg-app-reserve-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.pick_watch(&pepsi);
    // 20 s of sound kept, as a run would keep it (no device is opened).
    app.session.listen();
    let info = app.info("", 48000, 16);
    let mut r = Recorder::start(&base.join("unsaved"), info).unwrap();
    r.write(&timegrapher_core::capture::Block {
        samples: vec![0.0; 48000 * 20],
        at: std::time::SystemTime::now(),
    })
    .unwrap();
    app.session.recorder = Some(r);
    app.position = Position::CrownLeft;
    let mut t = reserve::Test::new(
        Position::CrownLeft,
        true,
        "2026-10-22T16:15:00Z".into(),
        0.0,
    );
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
    assert!(!app.position_editable());
    assert!(app.position_lock_words().contains("Stop Test"));
    // One position for the whole run.
    app.change_position(Position::DialUp);
    assert_eq!(app.position, Position::CrownLeft);

    // Stop ends the test and keeps the take in the watch's history.
    app.stop_listening();
    assert!(!app.reserve_running());
    assert!(app.reserve.as_ref().unwrap().saving);
    app.wait_for_save();
    app.reserve_saved();
    let saved = app.session.saved_to().unwrap().to_path_buf();
    assert_eq!(app.reserve.as_ref().unwrap().saved.as_ref(), Some(&saved));
    assert!(
        saved.starts_with(root.join("Pepsi").join(library::TAKES)),
        "{saved:?}"
    );
    assert!(file_name(&saved).ends_with(" Power Reserve"), "{saved:?}");
    let r = power_reserve::Results::from_json(
        &std::fs::read_to_string(saved.join(power_reserve::FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(r.ending, power_reserve::Ending::ByHand);
    assert_eq!(r.position, "CL");
    assert!((r.run_s - 99.0 * 0.125).abs() < 1e-6, "{}", r.run_s);
    let take = app.open_watch().unwrap().takes[0].clone();
    assert!(watches::take_line(&take, false, None)
        .starts_with("Power Reserve 12 s · Stopped by hand while the watch still ran"));
    // Opened again, the take brings its run back to the pane.
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.start_batch(&saved);
    let back = again.reserve.as_ref().expect("its results");
    assert!(!again.reserve_running());
    assert_eq!(back.results.as_ref(), Some(&r));
    assert_eq!(back.position, Position::CrownLeft);
    let _ = std::fs::remove_dir_all(&base);
}

/// A Power Reserve Test started with a take open runs in the position
/// the card and the pane named, not the one listening had before the
/// take was opened; and the status bar names the watch the Calibre
/// card names.
#[test]
fn a_power_reserve_test_over_an_opened_take_keeps_the_cards_position_and_watch() {
    let base = std::env::temp_dir().join(format!("tg-app-reserve-pos-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Daytona")).unwrap();
    let id = lib.find("Daytona").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root));
    app.pick_watch(&id);
    // A take opened in Dial Down over listening's Dial Up.
    app.reset_session(48000, "take".into());
    app.input = Input::File;
    app.opened_path = "/x/take.wav".into();
    app.mic_setup = Some(WatchSetup::default());
    app.position = Position::DialDown;
    // The test's own session, short of opening a real input.
    app.reserve_session();
    assert_eq!(app.position, Position::DialDown);
    assert_eq!(app.input, Input::Microphone);
    // The take's own watch name gone, as after a test's Stop.
    app.watch.clear();
    let words = app.status_words();
    assert!(words.contains("Daytona"), "{words}");
    assert!(!words.contains("No watch"), "{words}");
    let _ = std::fs::remove_dir_all(&base);
}

/// A Power Reserve run longer than the beats the Bench keeps (about a
/// day) still starts at its first beat: the engine keeps every beat of
/// the run, so its run time and hours aren't read from its last day.
#[test]
fn a_power_reserve_run_keeps_its_first_beat_past_the_benchs_limit() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "syn".into());
    // The Bench's limit made small, as a day's beats are to a long run.
    app.live.as_mut().unwrap().set_max_beats(100);
    app.begin_reserve();
    let cfg = SynthConfig {
        duration_s: 40.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let live = app.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        live.push(b);
    }
    let beats = live.beats().to_vec();
    let now = live.duration_s();
    assert!(beats.len() > 200, "{}", beats.len());
    let t = app.reserve.as_mut().unwrap();
    assert!(t.update(&beats, 0.125, now).is_none());
    let first = t.heard.first_s.unwrap();
    assert!(first < 3.0, "{first}");
    assert!(t.run_so_far_s() > 35.0, "{}", t.run_so_far_s());
}

#[test]
fn a_power_reserve_test_runs_in_the_cards_position() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.change_position(Position::CrownUp);
    app.begin_reserve();
    assert_eq!(app.reserve.as_ref().unwrap().position, Position::CrownUp);
    assert_eq!(app.position, Position::CrownUp);
    // The pane names that position and has no picker of its own.
    let mut texts = Vec::new();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    app.reserve = None;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| app.reserve_settings(ui));
        });
    });
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| app.reserve_settings(ui));
        });
    });
    for c in &out.shapes {
        if let egui::Shape::Text(t) = &c.shape {
            texts.push(t.galley.text().to_string());
        }
    }
    assert!(
        texts
            .iter()
            .any(|t| t == "Runs in Crown Up (CU), set on the Current Position card"),
        "{texts:?}"
    );
    assert!(!texts.iter().any(|t| t == "Position"), "{texts:?}");
    // A compact card is titled Position: the words name it so.
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.position_card(ui, true));
    });
    assert!(painted_text(&out).contains("Position"));
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| app.reserve_settings(ui));
        });
    });
    assert!(
        painted_text(&out).contains("Runs in Crown Up (CU), set on the Position card"),
        "{}",
        painted_text(&out)
    );
    // And the steps before a run.
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            reserve::pane(ui, None, None, app.position_card_title, "", true);
        });
    });
    assert!(
        painted_text(&out)
            .contains("set the position on the Position card, then press Start Test."),
        "{}",
        painted_text(&out)
    );
}
