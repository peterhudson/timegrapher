//! Settings and the sheets: asking for a microphone, status lines and
//! row captions, moving the library, the clock store, and recent calibres.

use super::tests::{
    opened_recording, painted_text, save_failure_fixture, screen, synthetic, themed,
};
use super::*;

/// The first launch asks which microphone; once one is set up (or has
/// opened), every launch starts Quick Listen, unless Listen When the
/// App Opens is off. A recording asked for opens instead.
#[test]
fn the_app_asks_for_a_microphone_once_then_opens_listening() {
    let launch = |s: Settings| {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.apply_settings(s);
        app.at_launch();
        (app.setup_sheet, app.listen_pending)
    };
    assert_eq!(launch(Settings::default()), (true, false), "first launch");
    let set_up = Settings {
        mic_set_up: true,
        ..Settings::default()
    };
    assert_eq!(launch(set_up.clone()), (false, true));
    assert_eq!(
        launch(Settings {
            listen_at_open: false,
            ..set_up.clone()
        }),
        (false, false)
    );
    // Not Now: the sheet doesn't come back, and the app doesn't listen
    // by itself until a microphone is set up.
    assert_eq!(
        launch(Settings {
            setup_declined: true,
            ..Settings::default()
        }),
        (false, false),
        "after Not Now"
    );
    let mut opening = TimegrapherApp::with_devices(Vec::new(), None);
    opening.apply_settings(set_up);
    opening.input = Input::File;
    opening.at_launch();
    assert!(!opening.setup_sheet && !opening.listen_pending);
}

/// Where each text is painted, its top left.
fn painted_at(out: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
    out.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos)),
            _ => None,
        })
        .collect()
}

/// Set Up the Microphone's status line holds a failure in its own two
/// lines: the buttons below it don't move, and the failure is said
/// there, not in a paragraph under the controls. Before anything
/// starts it says "Not listening.".
#[test]
fn the_sheets_status_line_holds_a_failure_without_moving_the_buttons() {
    let ctx = themed();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.setup_sheet = true;
    // Already tried: a test never opens an input.
    app.setup_tried = true;
    let draw = |app: &mut TimegrapherApp| {
        let mut out = None;
        for _ in 0..3 {
            out = Some(ctx.run(screen(), |ctx| app.setup_sheet_dialog(ctx)));
        }
        painted_at(&out.unwrap())
    };
    let quiet = draw(&mut app);
    let at = |drawn: &[(String, egui::Pos2)], t: &str| {
        drawn
            .iter()
            .find(|(s, _)| s == t)
            .map(|d| d.1)
            .unwrap_or_else(|| panic!("no {t}: {drawn:?}"))
    };
    assert!(quiet.iter().any(|(t, _)| t == "Not listening."));
    app.message = Some((
        "Can't open the input. This input is busy or missing. If the desktop sound \
         server is running it is probably holding it: choose the system default instead, \
         or close what is using it. Otherwise check the microphone is plugged in. (The \
         requested device is no longer available. For example, it has been unplugged.)"
            .into(),
        true,
    ));
    let failed = draw(&mut app);
    assert!(failed
        .iter()
        .any(|(t, _)| t.starts_with("Can't open the input")));
    assert!(!failed.iter().any(|(t, _)| t == "Not listening."));
    for button in ["Done", "Not Now"] {
        assert_eq!(at(&quiet, button), at(&failed, button), "{button} moved");
    }
    assert!(app.setup_sheet, "still open");
}

/// A message about a setting takes its row's caption: Keep Awake's
/// fault replaces "While listening…", and the rows below it stay put.
#[test]
fn a_settings_message_takes_its_rows_caption() {
    let ctx = themed();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let draw = |app: &mut TimegrapherApp| {
        let mut out = None;
        for _ in 0..2 {
            out = Some(ctx.run(screen(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.general_settings(ui));
            }));
        }
        painted_at(&out.unwrap())
    };
    let fine = draw(&mut app);
    app.keep_awake = true;
    // As a desktop with no session bus says it: plain words in the
    // caption, on one line, and the system's own words in the hover.
    app.awake_fault = Some(
        "Failed to connect to address `unix:path=/run/user/0/bus`: No such file or \
         directory (os error 2)"
            .into(),
    );
    let fault = draw(&mut app);
    let y = |drawn: &[(String, egui::Pos2)], t: &str| {
        drawn
            .iter()
            .find(|(s, _)| s == t)
            .map(|d| d.1.y)
            .unwrap_or_else(|| panic!("no {t}: {drawn:?}"))
    };
    assert!(fault
        .iter()
        .any(|(t, _)| t == "Not being kept awake: the system didn't allow it."));
    assert!(!fault.iter().any(|(t, _)| t.contains("unix:path")));
    assert!(!fault.iter().any(|(t, _)| t.starts_with("While listening")));
    assert_eq!(
        y(&fine, "Check for Updates at Start"),
        y(&fault, "Check for Updates at Start")
    );
    for title in ["Appearance", "Listening", "Updates"] {
        assert!(fine.iter().any(|(t, _)| t == title), "{title}");
    }
}

/// An input that won't open says so in the Microphone row's caption,
/// on one line, and the row below stays put.
#[test]
fn an_input_that_wont_open_says_so_in_its_rows_caption() {
    let ctx = themed();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let draw = |app: &mut TimegrapherApp| {
        let mut out = None;
        for _ in 0..2 {
            out = Some(ctx.run(screen(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.microphone_page(ui));
            }));
        }
        painted_at(&out.unwrap())
    };
    let fine = draw(&mut app);
    assert!(fine
        .iter()
        .any(|(t, _)| t == "The input the watch sits on."));
    let m = "Can't open the input. A backend-specific error has occurred: ALSA \
             function 'snd_pcm_open' failed with error 'Connection refused (111)'";
    app.input_fault = Some(m.into());
    let failed = draw(&mut app);
    assert!(failed.iter().any(|(t, _)| t == m), "{failed:?}");
    assert!(!failed
        .iter()
        .any(|(t, _)| t == "The input the watch sits on."));
    let y = |drawn: &[(String, egui::Pos2)]| {
        drawn
            .iter()
            .find(|(s, _)| s == "Show Every Input")
            .map(|d| d.1.y)
            .unwrap()
    };
    assert_eq!(y(&fine), y(&failed));
}

/// The panes start below the readings' cards, whatever the cards'
/// controls take: with a watch picked the Calibre card names the
/// calibre rather than offering its menu.
#[test]
fn the_panes_start_below_the_cards_and_their_controls() {
    let base = std::env::temp_dir().join(format!("tg-app-below-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    let ctx = themed();
    let check = |app: &mut TimegrapherApp| {
        let mut top_of = std::collections::HashMap::new();
        let mut bottom_of = std::collections::HashMap::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1000.0, 700.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| app.panels(ctx));
            top_of.clear();
            bottom_of.clear();
            for c in &out.shapes {
                if let egui::Shape::Text(t) = &c.shape {
                    let r = c.shape.visual_bounding_rect();
                    let k = t.galley.text().to_string();
                    // The first drawn of a text: the cards come before
                    // the panes (the Position Brush shows a code too).
                    top_of.entry(k.clone()).or_insert(r.top());
                    bottom_of.entry(k).or_insert(r.bottom());
                }
            }
        }
        let pane = top_of["Paper Strip"];
        // The Calibre card's controls: the Calibre pop-up (or a
        // watch's word for it) and the beat rate's pop-up under it.
        let calibre = ["Not Set", "No Calibre"]
            .into_iter()
            .find(|k| bottom_of.contains_key(*k))
            .expect("the calibre");
        for control in ["DU", calibre, "Auto"] {
            assert!(
                bottom_of[control] < pane,
                "{control} ends at {} under the panes at {pane}",
                bottom_of[control]
            );
        }
    };
    check(&mut app);
    app.pick_watch(&pepsi);
    check(&mut app);
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// The app opens on Quick Listen, whose sound goes without a question;
/// a Positions Test started after it, with no watch picked, is not
/// Quick Listen, so its take is never let go unasked. (A Power Reserve
/// Test clears the same flag before it opens the microphone.)
#[test]
fn a_test_after_quick_listen_is_not_quick_listen() {
    let base = std::env::temp_dir().join(format!("tg-app-test-ql-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = opened_recording(&base, "a.wav", 12.0);
    // Changed masks make the test ask before it opens an input, so no
    // input is opened here.
    app.mask(2.0, 5.0);
    app.quick_listen = true;
    app.start_guided(None);
    assert!(matches!(app.confirm, Some(Confirm::Microphone)));
    assert!(!app.quick_listen);
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// A Positions Test or Power Reserve Test that ends with no watch
/// picked asks where it goes: Keep in Unsorted, Keep in a Watch… or
/// Throw Away. Quick Listen's sound still goes without a question.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn a_test_with_no_watch_asks_where_it_goes() {
    let base = std::env::temp_dir().join(format!("tg-app-test-keep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    // A take as Stop leaves it, with no input opened.
    let take = |app: &mut TimegrapherApp, n: usize| {
        app.new_session();
        app.session.listen();
        let info = app.info("test", 48000, 16);
        let mut r = Recorder::start(&base.join(format!("unsaved-{n}")), info).unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 48000 * 5],
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        app.session.recorder = Some(r);
    };
    let takes_in = |dir: PathBuf| std::fs::read_dir(dir).map_or(0, |d| d.count());
    let unsorted = root.join(library::UNSORTED).join(library::TAKES);
    let in_pepsi = root.join("Pepsi").join(library::TAKES);

    // Quick Listen: no question.
    take(&mut app, 0);
    app.quick_listen = true;
    app.stop_listening();
    assert_eq!(app.test_keep, None);

    // A Positions Test: asked, with the three answers on show.
    take(&mut app, 1);
    app.quick_listen = false;
    app.positions_take = true;
    app.stop_listening();
    assert_eq!(app.test_keep, Some(TestKeep::Positions));
    let ctx = themed();
    assert!(app.confirm.is_none() && app.watch_form.is_none());
    let mut drawn = String::new();
    // Clear of the test's pane, top or foot, leaving its table in view.
    let full = screen().screen_rect.unwrap();
    let (upper, lower) = full.split_top_bottom_at_fraction(0.5);
    for (pane, clear) in [(lower, upper), (upper, lower)] {
        app.results_rects[0] = Some(pane.shrink(8.0));
        let mut title = None;
        for _ in 0..3 {
            let out = ctx.run(screen(), |ctx| app.test_keep_dialog(ctx));
            drawn = painted_text(&out);
            for c in &out.shapes {
                if let egui::Shape::Text(t) = &c.shape {
                    if t.galley.text() == "Keep This Positions Test?" {
                        title = Some(c.shape.visual_bounding_rect());
                    }
                }
            }
        }
        let title = title.expect("the sheet's title");
        assert!(
            clear.contains_rect(title),
            "the sheet's title at {title:?}, the pane at {pane:?}"
        );
    }
    for t in [
        "Keep This Positions Test?",
        "Keep in Unsorted",
        "Keep in a Watch…",
        "Throw Away",
    ] {
        assert!(drawn.contains(t), "{t}: {drawn}");
    }
    // Keep in Unsorted.
    app.keep_test(TestKeep::Positions, None);
    app.wait_for_save();
    assert_eq!(app.test_keep, None);
    assert_eq!(takes_in(unsorted.clone()), 1);

    // Keep in a Watch: Pepsi.
    take(&mut app, 2);
    app.positions_take = true;
    app.watch_open = None;
    app.stop_listening();
    assert_eq!(app.test_keep, Some(TestKeep::Positions));
    app.keep_test(TestKeep::Positions, Some(&pepsi));
    app.wait_for_save();
    assert_eq!(takes_in(in_pepsi.clone()), 1);

    // Throw Away: nothing kept, nothing left to ask about.
    take(&mut app, 3);
    app.positions_take = true;
    app.watch_open = None;
    app.stop_listening();
    assert_eq!(app.test_keep, Some(TestKeep::Positions));
    app.new_session();
    assert_eq!(app.test_keep, None);
    assert!(!app.needs_save_confirmation());
    assert_eq!(takes_in(unsorted.clone()), 1);
    assert_eq!(takes_in(in_pepsi.clone()), 1);

    // A Power Reserve Test with no watch: asked before it is saved,
    // then kept in Unsorted.
    take(&mut app, 4);
    app.watch_open = None;
    let started = app
        .session
        .recorder
        .as_ref()
        .unwrap()
        .started_utc()
        .to_string();
    app.reserve = Some(reserve::Test::new(Position::DialUp, true, started, 0.0));
    app.stop_listening();
    assert_eq!(app.test_keep, Some(TestKeep::PowerReserve));
    assert_eq!(takes_in(unsorted.clone()), 1, "not saved before the answer");
    app.keep_test(TestKeep::PowerReserve, None);
    app.wait_for_save();
    assert_eq!(takes_in(unsorted.clone()), 2);

    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn the_paper_strip_is_the_largest_area_at_1000_by_700() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let ctx = themed();
    let mut top_of = std::collections::HashMap::new();
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1000.0, 700.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| app.panels(ctx));
        for c in &out.shapes {
            if let egui::Shape::Text(t) = &c.shape {
                let r = c.shape.visual_bounding_rect();
                top_of.insert(t.galley.text().to_string(), r.top());
            }
        }
    }
    let cards = top_of["Paper Strip"] - top_of["Beat Error"];
    let strip = top_of["Tick Tock Profile"] - top_of["Paper Strip"];
    let profile = top_of[&app.status_words()] - top_of["Tick Tock Profile"];
    assert!(
        strip > cards && strip > profile,
        "the strip {strip} against the cards {cards} and the profile {profile}"
    );
    // The profile keeps about a third of the room below the cards.
    assert!(
        profile > (strip + profile) * 0.38,
        "the profile {profile} against the strip {strip}"
    );
    // Each toolbar keeps to one row, from its first control to its last
    // (or its "…" menu, when the pane is too narrow for them all).
    let tools = theme::tools_drawn_at(&ctx);
    let row = |p: Pane| {
        let ys: Vec<f32> = tools
            .iter()
            .filter(|(k, _)| k.starts_with(&format!("{}/", p.title())))
            .map(|(_, r)| r.center().y)
            .collect();
        ys.len() > 1 && ys.iter().all(|y| (y - ys[0]).abs() < 4.0)
    };
    assert!(row(Pane::Strip), "{tools:?}");
    assert!(row(Pane::Sound), "{tools:?}");
}

/// A chart's scale holds every point, a glitch too.
#[test]
fn the_scale_holds_every_point() {
    let mut pts: Vec<[f64; 2]> = (0..60).map(|i| [i as f64, 5.0 + (i % 5) as f64]).collect();
    pts.extend((60..66).map(|i| [i as f64, 800.0]));
    let (lo, hi) = data_range(&pts).unwrap();
    assert!(lo < 5.0 && hi > 800.0, "{lo} {hi}");
    assert_eq!(data_range(&[]), None);
    let (lo, hi) = data_range(&[[0.0, 3.0]]).unwrap();
    assert!(lo < 3.0 && hi > 3.0);
}

#[test]
fn tick_and_tock_amplitude_dash_over_their_own_colours() {
    let pal = theme::palette(true);
    let key = chart_key([false, true, false], pal);
    assert_eq!(key[0], ("Average Amplitude", pal.trace_amplitude, None));
    assert_eq!(key[1], ("From Tick", pal.trace_amplitude, Some(pal.tick)));
    assert_eq!(key[2], ("From Tock", pal.trace_amplitude, Some(pal.tock)));
    assert_eq!(key.len(), 3);
    // The key follows the charts shown: none for the rate alone.
    assert!(chart_key([true, false, false], pal).is_empty());
    assert_eq!(chart_key([true; 3], pal).len(), 5);
}

#[test]
fn a_chart_labels_no_time_before_the_start() {
    let r = -30.0..=600.0;
    assert_eq!(chart_time_label(-30.0, 60.0, &r), "");
    assert_eq!(chart_time_label(0.0, 60.0, &r), "0:00");
    assert_eq!(chart_time_label(120.0, 60.0, &r), "2:00");
    // Marks under a second apart don't repeat a label.
    let r = 9.0..=10.0;
    assert_eq!(chart_time_label(9.2, 0.2, &r), "0:09.2");
    assert_eq!(chart_time_label(9.4, 0.2, &r), "0:09.4");
}

/// Settings' Library section reads the library at the folder chosen,
/// draws what it holds, moves it and keeps using it where it went,
/// and the choice is kept between runs.
#[test]
fn settings_moves_the_library_and_keeps_its_place() {
    let base = std::env::temp_dir().join(format!("tg-app-library-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("here").join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.open_settings(Tab::WatchesFolder);
    assert_eq!(
        app.library
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .watches
            .len(),
        1
    );
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let text = |ctx: &egui::Context, app: &mut TimegrapherApp| {
        let mut all = String::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
            all = format!("{:?}", out.shapes);
        }
        all
    };
    let drawn = text(&ctx, &mut app);
    assert!(drawn.contains("1 watch, 0 takes"), "{drawn}");
    assert!(drawn.contains("Move Watches…") && drawn.contains("Use the Default"));
    // The Watches folder is a category of its own in the sidebar, beside
    // every other; nothing is called Library, and the kept clocks are
    // under Microphone, not here.
    for name in ["General", "Microphone", "Watches Folder", "Calibres"] {
        assert!(drawn.contains(name), "{name} not in the sidebar: {drawn}");
    }
    assert!(
        !drawn.contains("Library") && !drawn.contains("Quartz Correction"),
        "{drawn}"
    );
    assert!(!drawn.contains("Sound Card Clock"), "{drawn}");
    // While another copy holds them, this one can only read them, and says so.
    assert!(drawn.contains("can only read them"), "{drawn}");
    drop(lib);
    app.read_library();
    assert!(!text(&ctx, &mut app).contains("can only read them"));
    // Opening Settings again reads it afresh, and this copy's own lock
    // is no other copy.
    app.open_settings(Tab::WatchesFolder);
    assert!(!text(&ctx, &mut app).contains("can only read them"));

    // Move it into another folder, and it is used there.
    let to = base.join("there").join("Timegrapher Data");
    app.library_move_ask = Some(to.clone());
    let asked = text(&ctx, &mut app);
    assert!(asked.contains("Move your watches to"), "{asked}");
    app.library_move_ask = None;
    app.move_library(root.clone(), to.clone());
    let t = Instant::now();
    while app.library_moving.is_some() && t.elapsed().as_secs() < 30 {
        app.poll_library_move(&ctx);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.library_dir.as_deref(), Some(to.as_path()));
    assert!(!root.exists() && library::Library::is_library(&to));
    assert_eq!(
        app.library
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .watches
            .len(),
        1
    );
    assert_eq!(app.settings().library.as_deref(), Some(to.as_path()));
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.apply_settings(app.settings());
    assert_eq!(again.library_root().as_deref(), Some(to.as_path()));

    // A folder with no library yet reads as none, and is left alone.
    app.use_library(Some(base.join("empty")));
    assert!(matches!(app.library, Ok(None)));
    assert!(!base.join("empty").exists());
    let _ = std::fs::remove_dir_all(&base);
}

/// The Rate's hover names where an opened take's clock came from in
/// core's words: its clock log, its USB lines or the correction kept
/// for its input.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "a test's clock fixture, not a take being opened"
)]
fn the_rate_hover_names_where_a_takes_clock_came_from() {
    use timegrapher_core::take::{clock_source_text, ClockFrom};
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let clock = timegrapher_core::clock::ClockFit::fixed(18.9, 600.0);
    let usb = "from 12 USB lines in the recording, 0.1 ppm apart";
    let kept = "stored for 'USB Mic', 2026-10-20T10:00:00Z";
    let log = std::path::Path::new("take/clock.log");
    for from in [ClockFrom::Log, ClockFrom::Usb, ClockFrom::Correction] {
        let source = clock_source_text(from, Some(usb), Some(kept), Some(log));
        app.put_take_clock(Some(clock.clone()), Some(source.clone()));
        let words = app.true_clock_words(0.0).unwrap();
        assert!(
            words.contains(&format!("18.9 ppm slow, {source},")),
            "{words}"
        );
    }
    // The USB lines set the rate over a log kept for its breaks: core's
    // words for both, and what the log measured.
    let mut rated = clock.clone();
    rated.rate_from_usb(18.9);
    let source = clock_source_text(ClockFrom::LogRatedByUsb, Some(usb), None, Some(log));
    app.put_take_clock(Some(rated.clone()), Some(source.clone()));
    let words = app.true_clock_words(0.0).unwrap();
    let note = rated.withheld.unwrap().note();
    assert!(
        words.contains(&format!("18.9 ppm slow, {source},")) && words.contains(&note[1..]),
        "{words}"
    );
}

/// Settings lists each kept clock; Forget removes that one from the
/// file the command line reads, and the input listening loses its
/// True Clock with it. Manage Calibres… opens Settings at Calibres.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "a test's clock fixture, not a take being opened"
)]
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn settings_forgets_a_kept_clock() {
    const MARKER: &str = "TIMEGRAPHER_FORGET_TEST_CHILD";
    if std::env::var_os(MARKER).is_none() {
        let root = std::env::temp_dir().join(format!("tg-app-forget-clock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut store = clockstore::ClockStore::default();
        for (device, ppm) in [("USB Mic", 12.5), ("Built-in", -3.0)] {
            store.set(clockstore::DeviceClock::from_fit(
                device,
                &timegrapher_core::clock::ClockFit::fixed(ppm, 1500.0),
                "2026-10-20T10:00:00Z".into(),
                "measure",
            ));
        }
        store.save_to(&root.join("clocks.json")).unwrap();
        // Isolate the config environment in a child test process.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!(
                    "{}::settings_forgets_a_kept_clock",
                    module_path!().split_once("::").unwrap().1
                ),
                "--nocapture",
            ])
            .env(MARKER, "1")
            .env("TIMEGRAPHER_CONFIG_DIR", &root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        let left = clockstore::ClockStore::load_from(&root.join("clocks.json")).unwrap();
        let names: Vec<&str> = left.devices.iter().map(|d| d.device.as_str()).collect();
        assert_eq!(names, ["Built-in"]);
        std::fs::remove_dir_all(root).unwrap();
        return;
    }
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.clock = clockstore::ClockStore::load()
        .unwrap()
        .get("USB Mic")
        .cloned();
    assert!(app.clock.is_some());
    app.open_settings(Tab::Microphone);
    let listed = app.kept_clocks.as_ref().unwrap();
    assert_eq!(listed.devices.len(), 2);
    assert_eq!(
        settings_window::error_words(listed.get("USB Mic").unwrap()),
        "12.5 ppm slow, +1.08 s/d on every rate"
    );
    // The window draws both, each with its Forget.
    let ctx = egui::Context::default();
    theme::install(&ctx);
    // A window is measured on its first frame and drawn from the next.
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
    let out = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
    let text = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        text.iter().any(|t| t == "USB Mic") && text.iter().any(|t| t == "Built-in"),
        "{text:?}"
    );
    assert_eq!(
        text.iter().filter(|t| *t == "Forget").count(),
        2,
        "{text:?}"
    );
    // Under Microphone, as Quartz Correction, kept per input: the input
    // listening is marked, and a kept clock is under its own input.
    assert!(text.iter().any(|t| t == "Quartz Correction"), "{text:?}");
    app.clock_track = Some(ClockTrack {
        device: "USB Mic".into(),
        shown: "USB Mic".into(),
        tracker: timegrapher_core::clock::ClockTracker::new(48_000),
        next_s: CLOCK_MEASURE_S,
        fit: None,
    });
    let out = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
    let text: Vec<String> = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect();
    assert!(
        text.iter().any(|t| t == "USB Mic  (listening)") && text.iter().any(|t| t == "Built-in"),
        "{text:?}"
    );
    assert!(
        text.iter().any(|t| t.starts_with("3.0 ppm fast, ")),
        "{text:?}"
    );
    // An input listening with none kept says so, beside the others'.
    app.clock_track.as_mut().unwrap().device = "Line In".into();
    let out = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
    let said = out.shapes.iter().any(|s| match &s.shape {
        egui::Shape::Text(t) => t.galley.text().starts_with("None kept for \"Line In\""),
        _ => false,
    });
    assert!(said, "the input listening with no correction kept is named");
    app.clock_track = None;
    // Forget asks first, saying what is lost; nothing goes until it is
    // confirmed.
    app.forget_ask = Some("USB Mic".into());
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
    let out = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
    let asked = out.shapes.iter().any(|s| match &s.shape {
        egui::Shape::Text(t) => {
            t.galley.text()
                == "Forget the clock kept for \"USB Mic\", 12.5 ppm slow, measured 2026-10-20?"
        }
        _ => false,
    });
    assert!(asked, "the question names the input, its error and its day");
    assert!(app.clock.is_some() && app.forget_ask.is_some());
    assert_eq!(app.kept_clocks.as_ref().unwrap().devices.len(), 2);
    app.forget_ask = None;
    app.forget_clock("USB Mic");
    assert!(
        app.clock.is_none(),
        "the input listening loses its True Clock"
    );
    assert_eq!(app.kept_clocks.as_ref().unwrap().devices.len(), 1);
    assert!(!app.message.as_ref().unwrap().1);
    // Manage Calibres… opens Settings at Calibres, and Settings opens
    // where it was left.
    app.settings_tab = None;
    app.cal.open_manager("");
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
    assert_eq!(app.settings_tab, Some(Tab::Calibres));
    assert!(!app.cal.manager_open);
    app.settings_tab = None;
    app.open_settings(app.settings_last);
    assert_eq!(app.settings_tab, Some(Tab::Calibres));
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "a test's clock fixture, not a take being opened"
)]
fn invalid_clock_store_blocks_fresh_listening_and_keep_without_hardware() {
    const MARKER: &str = "TIMEGRAPHER_CLOCK_STORE_TEST_CHILD";
    if std::env::var_os(MARKER).is_none() {
        let root =
            std::env::temp_dir().join(format!("tg-app-invalid-clock-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let original = r#"{"schema":"timegrapher.clocks/1","devices":[{"device":"USB","ppm":-1000000,"ppm_sd":null,"span_s":0,"measured_utc":"legacy","source":"set"}]}"#;
        std::fs::write(root.join("clocks.json"), original).unwrap();
        // Isolate the config environment in a child test process, not parallel app tests.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!(
                    "{}::invalid_clock_store_blocks_fresh_listening_and_keep_without_hardware",
                    module_path!().split_once("::").unwrap().1
                ),
                "--nocapture",
            ])
            .env(MARKER, "1")
            .env("TIMEGRAPHER_CONFIG_DIR", &root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "child must execute the actual entrypoint test"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("clocks.json")).unwrap(),
            original
        );
        std::fs::remove_dir_all(root).unwrap();
        return;
    }
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48_000, "retained context".into());
    let old = clockstore::DeviceClock::from_fit(
        "old",
        &timegrapher_core::clock::ClockFit::fixed(12.0, 60.0),
        "legacy".into(),
        "set",
    );
    app.clock = Some(old.clone());
    app.devices_listed = false;
    app.start_microphone();
    assert!(app.capture.is_none() && app.batch.is_none());
    assert!(app.devices.is_empty());
    assert!(
        !app.devices_listed,
        "invalid store must prevent device enumeration"
    );
    assert_eq!(app.clock, Some(old.clone()));
    assert_eq!(app.live.as_ref().unwrap().sample_rate(), 48_000);
    let (message, error) = app.message.as_ref().unwrap();
    assert!(
        *error && message.contains("clocks.json") && message.contains("ppm"),
        "{message}"
    );
    app.clock_track = Some(ClockTrack {
        device: "USB".into(),
        shown: "USB".into(),
        tracker: timegrapher_core::clock::ClockTracker::new(48_000),
        next_s: CLOCK_MEASURE_S,
        fit: Some(timegrapher_core::clock::ClockFit::fixed(20.0, 1200.0)),
    });
    app.keep_clock();
    assert_eq!(app.clock, Some(old));
    let (message, error) = app.message.as_ref().unwrap();
    assert!(
        *error && message.contains("couldn't be kept") && message.contains("ppm"),
        "{message}"
    );
}

/// In Quick Listen a calibre picked sets the beat rate and, where the
/// table gives one, the lift angle, both then shown as text; Not Set
/// puts back Auto and the default lift angle.
#[test]
fn a_calibre_picked_sets_the_beat_rate_and_not_set_puts_back_auto() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    assert_eq!(app.bph, None);
    app.set_calibre("ETA 2824-2".into());
    let bph = app.cal.bph(&app.calibre).expect("the 2824-2's beat rate");
    assert_eq!(app.bph, Some(bph));
    assert!(app.bph_set_by_calibre());
    if let Some(l) = app.cal.lift(&app.calibre) {
        assert_eq!(app.lift_deg, l);
        assert!(!app.lift_settable());
    }
    app.set_calibre(String::new());
    assert_eq!(app.bph, None);
    assert_eq!(app.lift_deg, DEFAULT_LIFT_DEG);
    assert!(!app.bph_set_by_calibre() && app.lift_settable());
}

#[test]
fn the_input_clock_is_offered_after_twenty_minutes() {
    // A card 20 ppm slow: 48,000 frames arrive every 1.00002 s of
    // the computer's time, in blocks of a tenth of a second.
    let mut t = ClockTrack {
        device: "Test Mic".into(),
        shown: "Test Mic".into(),
        tracker: timegrapher_core::clock::ClockTracker::new(48_000),
        next_s: CLOCK_MEASURE_S,
        fit: None,
    };
    use std::time::Duration;
    let start = std::time::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let mut audio = 0.0;
    while audio < CLOCK_MEASURE_S - 1.0 {
        audio += 0.1;
        t.push(4_800, start + Duration::from_secs_f64(audio * 1.000_02));
    }
    assert!(t.fit.is_none(), "nothing offered before 20 minutes");
    while audio < CLOCK_MEASURE_S + 1.0 {
        audio += 0.1;
        t.push(4_800, start + Duration::from_secs_f64(audio * 1.000_02));
    }
    let f = t.fit.as_ref().expect("timed at 20 minutes");
    assert!((f.ppm - 20.0).abs() < 0.5, "{} ppm", f.ppm);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.clock_track = Some(t);
    assert!(app.clock_measured().is_some());
    assert!(app.clock_offer().is_some(), "offered with none kept");
    // One kept for the same name: offered again only when it is more
    // than a couple of ppm away, as when another card answers to it.
    let kept = |ppm: f64| clockstore::DeviceClock {
        device: "Test Mic".into(),
        ppm,
        ppm_sd: Some(0.1),
        span_s: 1200.0,
        measured_utc: "2026-10-12T20:00:00Z".into(),
        source: "measure".into(),
    };
    app.clock = Some(kept(21.0));
    assert!(app.clock_offer().is_none(), "the kept clock agrees");
    app.clock = Some(kept(-14.0));
    assert!(app.clock_offer().is_some(), "the kept clock is 34 ppm off");
    assert_eq!(day_of("2026-10-12T20:00:00Z"), "2026-10-12");
}

/// One Average Over, on the Listen card, for the readings and every
/// pane: a pane's own time, saved by an older app, is read and left
/// out.
#[test]
fn one_average_over_sets_every_pane() {
    let mut app = synthetic(40.0, 0.0);
    app.set_average(30.0);
    assert!(!app.trend.is_empty());
    assert!(app.trend.last().unwrap().t > 30.0);
    let mut s = app.settings();
    let mut json = serde_json::to_value(&s).unwrap();
    json["pane_average"] = serde_json::json!([["Charts", 5.0]]);
    s = serde_json::from_value(json).unwrap();
    let mut back = TimegrapherApp::with_devices(Vec::new(), None);
    back.apply_settings(s);
    assert_eq!(back.average_s, 30.0);
}

#[test]
fn a_long_take_still_draws_over_a_short_strip() {
    // Opened whole, a take over 5 h 33 min keeps a reading only every
    // take/4000 s, more than 5 s apart: thinned here to the 6.7 s of
    // the 7 h 28 min Pepsi take, the strip works out its own.
    let mut app = synthetic(40.0, 20.0);
    let thin = |app: &mut TimegrapherApp| {
        let step = 26_880.0 / TREND_POINTS;
        let mut t = 0.0;
        let mut thin = Vec::new();
        while t <= 40.0 {
            if let Some(p) = app.trend.iter().find(|p| p.t >= t) {
                thin.push(*p);
            }
            t += step;
        }
        app.trend = thin;
    };
    thin(&mut app);
    let (pts, spacing) = app.strip_trend(5.0, 40.0, app.average_s);
    assert_eq!(spacing, strip_step(35.0));
    assert!(pts.len() >= 60, "{} points", pts.len());
    assert!(pts.iter().filter(|p| p.rate.is_some()).count() >= 40);
    // A new lift angle works the strip's readings out again, so the
    // amplitude line follows it at once.
    let amplitude = |pts: &[TrendPoint]| pts.iter().find_map(|p| p.amplitude).unwrap();
    let before = amplitude(&pts);
    app.set_lift(app.lift_deg + 10.0);
    assert!(app.strip_points.is_none());
    thin(&mut app);
    let (pts, _) = app.strip_trend(5.0, 40.0, app.average_s);
    assert!(amplitude(&pts) != before, "amplitude {before} kept");
    // The kept run serves when it is fine enough.
    let mut app = synthetic(40.0, 20.0);
    let kept = app.trend.len();
    let (pts, _) = app.strip_trend(0.0, 40.0, app.average_s);
    assert!(app.strip_points.is_none());
    assert!(pts.len() <= kept);
}

/// Recent is the six calibres the app was last set to, by a menu pick,
/// a watch opened or a take saved: the latest first, none twice, kept
/// across restarts.
#[test]
fn recent_follows_what_the_app_was_set_to() {
    let names: Vec<String> = timegrapher_core::calibres::all()
        .iter()
        .take(8)
        .map(|c| c.calibre.clone())
        .collect();
    let (mut app, base) = save_failure_fixture("recent");
    // Menu picks: the latest first.
    for n in &names[..3] {
        app.set_calibre(n.clone());
    }
    assert_eq!(
        app.cal
            .recent
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [names[2].as_str(), names[1].as_str(), names[0].as_str()]
    );
    // Set again to a calibre already there: it moves to the front, once.
    app.set_calibre(names[0].clone());
    assert_eq!(
        app.cal
            .recent
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [names[0].as_str(), names[2].as_str(), names[1].as_str()]
    );
    app.set_calibre(names[0].clone());
    assert_eq!(
        app.cal
            .recent
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [names[0].as_str(), names[2].as_str(), names[1].as_str()]
    );
    // A watch opened sets its calibre.
    let mut w = library::Watch::new("Bench");
    w.calibre = Some(names[3].clone());
    app.apply_watch(&w);
    assert_eq!(app.cal.recent[0], names[3]);
    // A take saved puts its calibre first, though nothing else set it.
    app.cal.recent.retain(|r| r != names[3].as_str());
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    assert_eq!(app.cal.recent[0], names[3]);
    // Six at most, the oldest going first; Not Set is never one.
    for n in &names[4..] {
        app.set_calibre(n.clone());
    }
    app.set_calibre(String::new());
    assert_eq!(app.cal.recent.len(), calibre_ui::RECENT);
    assert_eq!(
        app.cal
            .recent
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            names[7].as_str(),
            names[6].as_str(),
            names[5].as_str(),
            names[4].as_str(),
            names[3].as_str(),
            names[0].as_str()
        ]
    );
    // Kept across restarts, with the Settings category last left on.
    app.settings_last = Tab::Calibres;
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.apply_settings(app.settings());
    assert_eq!(again.cal.recent, app.cal.recent);
    assert_eq!(again.settings_last, Tab::Calibres);
    app.new_session();
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn a_take_saved_under_a_former_calibre_name_picks_the_new_one() {
    let c = timegrapher_core::calibres::all()
        .iter()
        .find(|c| !c.former_names.is_empty())
        .expect("a renamed calibre in the table");
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.set_calibre(c.former_names[0].clone());
    assert_eq!(app.calibre, c.calibre);
    assert_eq!(app.cal.recent.first(), Some(&c.calibre));
}
