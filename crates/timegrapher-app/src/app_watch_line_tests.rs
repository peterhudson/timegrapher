//! The Watches view and the watch line: the list beside the watch, the
//! line's menu, picking a watch over a take, and the card's fields.

use super::tests::{painted_text, screen, synthetic, themed};
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

/// The Watches view is the Watches view's toolbar and content, moved
/// whole: everything the pane drew, the view draws, and the view adds
/// only the window's own frame (the menus, the mode switch and the
/// status bar). The pane icons and the readings cards are the Bench's
/// and don't show.
/// The Watches view per its design: in a wide window the list sits
/// beside the watch, whose side stops at about 1100 pt, with Takes
/// straight after New Take and Details and Photos after them; in a
/// narrow one the watch shows alone, with ‹ All Watches back to the
/// list.
#[test]
fn the_watches_view_puts_the_list_beside_the_watch_when_wide() {
    let base = std::env::temp_dir().join(format!("tg-app-wv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(base.join("Timegrapher Data")));
    for name in ["Rolex", "Pepsi"] {
        let mut w = library::Watch::new(name);
        w.notes = Some(String::new());
        app.watch_form = Some(watches::Form {
            watch: w,
            new: true,
            error: None,
        });
        app.save_watch_form();
    }
    assert_eq!(
        app.open_watch().map(|e| e.watch.name.as_str()),
        Some("Pepsi")
    );
    let ctx = themed();
    let tops = |app: &mut TimegrapherApp, w: f32| {
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(w, 900.0),
            )),
            ..Default::default()
        };
        let mut at: Vec<(String, egui::Rect)> = Vec::new();
        for _ in 0..3 {
            let out = ctx.run(input(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.pane_tools(ui, Pane::Watch);
                    app.watch_pane(ui);
                });
            });
            at = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => {
                        Some((t.galley.text().to_string(), c.shape.visual_bounding_rect()))
                    }
                    _ => None,
                })
                .collect();
        }
        at
    };
    let find =
        |at: &[(String, egui::Rect)], t: &str| at.iter().find(|(s, _)| s == t).map(|(_, r)| *r);
    let wide = tops(&mut app, 2560.0);
    for t in [
        "Rolex",
        "Pepsi",
        "New Take",
        "Takes",
        "Details",
        "Photos",
        "Edit…",
        "Add Photos…",
    ] {
        assert!(find(&wide, t).is_some(), "{t} not drawn: {wide:?}");
    }
    assert!(find(&wide, "All Watches").is_none());
    let y = |t: &str| find(&wide, t).unwrap().top();
    assert!(y("New Take") < y("Takes") && y("Takes") < y("Details") && y("Details") < y("Photos"));
    let right = find(&wide, "Add Photos…").unwrap().right();
    assert!(
        right <= WATCH_LIST_W + 17.0 + WATCH_SIDE_MAX + 20.0,
        "the watch's side reaches {right}"
    );
    let narrow = tops(&mut app, 800.0);
    assert!(
        find(&narrow, "Rolex").is_none(),
        "the list is hidden: {narrow:?}"
    );
    assert!(find(&narrow, "New Take").is_some());
    assert!(
        theme::tools_drawn(&ctx)
            .iter()
            .any(|k| k.contains("All Watches")),
        "{:?}",
        theme::tools_drawn(&ctx)
    );
    // Nothing on the Bench: New Take and Open are free.
    assert_eq!(app.watches_busy(), None);
    assert!(find(&wide, "Stop the take on the Bench first.").is_none());
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn the_watches_view_is_the_watches_content_in_the_whole_window() {
    let base = std::env::temp_dir().join(format!("tg-app-view-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(base.join("Timegrapher Data")));
    let mut w = library::Watch::new("Pepsi");
    w.notes = Some(String::new());
    app.watch_form = Some(watches::Form {
        watch: w,
        new: true,
        error: None,
    });
    app.save_watch_form();
    let ctx = themed();
    let wide = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(2560.0, 1440.0),
        )),
        ..Default::default()
    };
    let lines = |t: String| -> std::collections::BTreeSet<String> {
        t.lines().map(str::to_string).collect()
    };
    for listed in [false, true] {
        if listed {
            app.list_for = Some((app.watch_open.clone(), false));
        }
        let mut content = String::new();
        for _ in 0..3 {
            content = painted_text(&ctx.run(wide(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.pane_tools(ui, Pane::Watch);
                    app.watch_pane(ui);
                });
            }));
        }
        let mut status = String::new();
        for _ in 0..2 {
            status = painted_text(&ctx.run(wide(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.status_bar(ui));
            }));
        }
        app.view = View::Watches;
        let mut window = String::new();
        for _ in 0..3 {
            window = painted_text(&ctx.run(wide(), |ctx| app.panels(ctx)));
        }
        let (content, status, window) = (lines(content), lines(status), lines(window));
        let missing: Vec<_> = content.difference(&window).collect();
        assert!(missing.is_empty(), "lost {missing:?}");
        let frame: std::collections::BTreeSet<String> =
            ["File", "Edit", "View", "Watches", "Help", "Bench"]
                .map(str::to_string)
                .into_iter()
                .chain(status)
                .collect();
        let added: Vec<_> = window
            .difference(&content)
            .filter(|t| !frame.contains(*t))
            .collect();
        assert!(added.is_empty(), "added {added:?}");
        assert!(!window.contains("Analysis"), "pane icons shown");
        assert!(!window.contains("Average Over"), "cards shown");
        assert_eq!(app.view, View::Watches, "the view stays put");
        app.view = View::Bench;
    }
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// Find in Watches… shows the Watches view's list with the search
/// field ready to type in, what it held selected so typing replaces
/// it; Open on a take there goes to the Bench.
#[test]
fn find_in_watches_readies_the_search_and_open_goes_to_the_bench() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let ctx = themed();
    let _ = ctx.run(screen(), |ctx| app.panels(ctx));
    app.watch_search = "Pepsi".into();
    app.find_in_watches();
    assert_eq!(app.view, View::Watches);
    assert!(app.list_shown());
    for _ in 0..2 {
        let _ = ctx.run(screen(), |ctx| app.panels(ctx));
    }
    assert!(!app.watch_search_focus);
    assert!(ctx.wants_keyboard_input(), "search field focused");
    let typed = egui::RawInput {
        events: vec![egui::Event::Text("S".into())],
        ..screen()
    };
    let _ = ctx.run(typed, |ctx| app.panels(ctx));
    assert_eq!(app.watch_search, "S", "the old search was selected");
    let missing = std::env::temp_dir().join("tg-app-no-such-take");
    app.take_action(Some(watches::TakeAction::Open(missing)));
    assert_eq!(app.view, View::Bench);
}

/// A library in a folder of its own holding these watches, the first
/// with `calibre`, for the Watch line's tests.
fn line_app(tag: &str, names: &[&str], calibre: Option<&str>) -> (TimegrapherApp, PathBuf) {
    let base = std::env::temp_dir().join(format!("tg-app-line-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(base.join("Timegrapher Data")));
    for (i, name) in names.iter().enumerate() {
        let mut w = library::Watch::new(name);
        if i == 0 {
            w.calibre = calibre.map(str::to_string);
        }
        app.watch_form = Some(watches::Form {
            watch: w,
            new: true,
            error: None,
        });
        app.save_watch_form();
    }
    // Saving a new watch picks it: start from No Watch.
    app.pick_unsorted();
    (app, base)
}

fn watch_id(app: &TimegrapherApp, name: &str) -> String {
    match &app.library {
        Ok(Some(l)) => l
            .watches
            .iter()
            .find(|e| e.watch.name == name)
            .unwrap()
            .watch
            .id
            .clone(),
        _ => panic!("no library"),
    }
}

/// A watch picked in the list over a Quick Listen still listening asks
/// about its unsaved sound first, or stops at once with nothing to ask
/// about; over a watch's take listening it goes to `pick_watch`, which
/// refuses; with nothing listening it is picked.
#[test]
fn a_watch_picked_over_a_quick_listen_asks_then_stops() {
    assert_eq!(list_pick_route(true, true, true), ListPick::Ask);
    assert_eq!(list_pick_route(true, true, false), ListPick::StopThenPick);
    assert_eq!(list_pick_route(true, false, true), ListPick::Pick);
    assert_eq!(list_pick_route(false, true, true), ListPick::Pick);
    // Asked and answered with nothing listening, it picks the watch.
    let (mut app, base) = line_app("list-pick", &["Pepsi"], None);
    let pepsi = watch_id(&app, "Pepsi");
    app.list_pick(pepsi.clone());
    assert_eq!(app.watch_open.as_deref(), Some(pepsi.as_str()));
    app.pick_unsorted();
    app.confirmed(
        Confirm::PickWatch(Some(pepsi.clone())),
        &egui::Context::default(),
    );
    assert_eq!(app.watch_open.as_deref(), Some(pepsi.as_str()));
    let _ = std::fs::remove_dir_all(base);
}

/// List Shows is kept between runs. A field no watch has shows as the
/// calibre, the choice still kept.
#[test]
fn list_shows_is_kept_between_runs() {
    let (mut app, base) = line_app("list-shows", &["Pepsi"], None);
    app.watch_list_shows = Some("Customer".into());
    assert_eq!(app.list_shows(), None, "no watch has Customer");
    if let Ok(Some(l)) = &mut app.library {
        l.watches[0].watch.fields.push(library::Field {
            label: "Customer".into(),
            kind: library::FieldKind::Text,
            value: "Ada".into(),
        });
    }
    assert_eq!(app.list_shows(), Some("Customer"));
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.apply_settings(app.settings());
    assert_eq!(again.watch_list_shows.as_deref(), Some("Customer"));
    let _ = std::fs::remove_dir_all(base);
}

/// What the Watch line's menu draws, in order.
fn line_menu(app: &mut TimegrapherApp, rates_here: bool) -> Vec<String> {
    let ctx = themed();
    let mut text = String::new();
    for _ in 0..2 {
        text = painted_text(&ctx.run(screen(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.watch_line_menu(ui, rates_here));
        }));
    }
    text.lines().map(str::to_string).collect()
}

/// The Calibre card's title menu: No Watch (Quick Listen), then the
/// watches under Recent, the newest take first, then New Watch… and
/// Find in Watches…. Quick Listen is no longer in the Watches menu.
#[test]
fn the_watch_line_menu_offers_no_watch_the_recent_watches_new_and_find() {
    let (mut app, base) = line_app("menu", &["Alpha", "Beta", "Gamma"], None);
    if let Ok(Some(l)) = &mut app.library {
        let take = |when: &str| library::Take {
            name: when.into(),
            bytes: 0,
            audio: true,
            incomplete: false,
            started_utc: Some(when.into()),
            duration_s: None,
            positions: Vec::new(),
            at_end: None,
            power_reserve: None,
        };
        for e in &mut l.watches {
            match e.watch.name.as_str() {
                "Gamma" => e.takes.push(take("2026-10-20T10:00:00Z")),
                "Beta" => e.takes.push(take("2026-10-24T10:00:00Z")),
                _ => {}
            }
        }
    }
    let menu = line_menu(&mut app, false);
    assert_eq!(
        menu,
        [
            format!("✓ {NO_WATCH}").as_str(),
            "Recent",
            "Beta",
            "Gamma",
            "Alpha",
            "New Watch…",
            "Find in Watches…"
        ],
    );
    let ctx = themed();
    let mut watches = String::new();
    for _ in 0..2 {
        watches = painted_text(&ctx.run(screen(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.watches_menu(ui));
        }));
    }
    assert!(!watches.contains("Quick Listen"), "{watches}");
    assert!(watches.contains("Find in Watches…"), "{watches}");
    app.pick_watch(&watch_id(&app, "Gamma"));
    assert_eq!(app.open_watch().unwrap().watch.name, "Gamma");
    let menu = line_menu(&mut app, false);
    assert_eq!(menu[0], NO_WATCH);
    assert!(menu.contains(&"✓ Gamma".to_string()), "{menu:?}");
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// For a watch with no calibre on a card too narrow for its Beat Rate,
/// the beat rates head the title menu, the one in use ticked; a rate
/// picked there is the card's and saved to the watch.
#[test]
fn the_title_menu_leads_with_the_hidden_beat_rate_for_a_watch_with_no_calibre() {
    let (mut app, base) = line_app("rates", &["Seiko"], None);
    app.pick_watch(&watch_id(&app, "Seiko"));
    assert!(app.watch_sets_here());
    let menu = line_menu(&mut app, true);
    assert_eq!(
        menu[..4],
        [
            "Beat Rate (hidden at this width)",
            "✓ Auto",
            "Other Rates",
            NO_WATCH
        ]
    );
    app.bph = Some(21600);
    let menu = line_menu(&mut app, true);
    assert_eq!(menu[1..3], ["✓ 21600 bph", "Auto"]);
    assert_eq!(
        line_menu(&mut app, false)[0],
        NO_WATCH,
        "only where it is hidden"
    );
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// A watch picked on the Watch line while a stopped take is on show
/// puts the take away, so the card shows the watch picked at once:
/// its calibre, lift and beat rate, never the old take's.
#[test]
fn picking_on_the_watch_line_over_a_take_shows_the_watch_picked() {
    let c = timegrapher_core::calibres::find("2824-2").expect("ETA 2824-2 in the table");
    let (mut app, base) = line_app("over", &["Pepsi", "Seiko"], Some(&c.calibre));
    let seiko = watch_id(&app, "Seiko");
    if let Ok(Some(l)) = &mut app.library {
        let e = l.watches.iter().find(|e| e.watch.id == seiko).unwrap();
        let mut w = e.watch.clone();
        w.lift = Some(53.0);
        l.update(&w).unwrap();
    }
    // A take on show measured with other settings.
    let on_show = |app: &mut TimegrapherApp| {
        app.set_lift(56.0);
        app.change_bph(Some(19800));
        app.reset_session(48000, "syn".into());
        let audio = generate(
            &SynthConfig {
                duration_s: 4.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let live = app.live.as_mut().unwrap();
        for b in audio.samples.chunks(960) {
            live.push(b);
        }
        assert!(app.live.is_some() && !app.running());
    };
    on_show(&mut app);
    app.line_pick(Some(watch_id(&app, "Pepsi")), false);
    assert!(app.live.is_none(), "the take is put away");
    assert_eq!(app.calibre, c.calibre);
    assert_eq!(app.lift_deg, c.lift_angle_deg.unwrap());
    assert_eq!(app.bph, c.bph);
    on_show(&mut app);
    app.line_pick(Some(seiko.clone()), false);
    assert!(app.calibre.is_empty());
    assert_eq!((app.lift_deg, app.bph), (53.0, None));
    assert!(app.watch_sets_here());
    on_show(&mut app);
    app.line_pick(None, false);
    assert!(app.open_watch().is_none() && app.live.is_none());
    assert_eq!((app.lift_deg, app.bph), (DEFAULT_LIFT_DEG, None));
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// Unsaved sound, Quick Listen's included, is asked about before the
/// Watch line puts its take away: Cancel leaves everything as it was,
/// Don't Save picks the watch.
/// The status bar names a take with no watch as Quick Listen's, however
/// No Watch was reached: picked on the Watch line over a watch's take
/// that was put away, or there from the start.
#[test]
fn the_status_bar_names_no_watch_quick_listen_however_it_was_reached() {
    let (mut app, base) = line_app("status", &["Pepsi"], None);
    assert_eq!(app.status_words(), "Quick Listen · No watch · Not Started");
    let pepsi = watch_id(&app, "Pepsi");
    app.line_pick(Some(pepsi), false);
    assert_eq!(app.status_words(), "New Take · Pepsi · Not Started");
    // A take on show, put away by picking No Watch on the line.
    app.reset_session(48000, "test".into());
    app.session.listen();
    app.session.set_state(session::State::Paused {
        since: Instant::now(),
    });
    app.line_pick(None, true);
    assert_eq!(app.status_words(), "Quick Listen · No watch · Not Started");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn the_watch_line_asks_before_putting_away_unsaved_sound() {
    let (mut app, base) = line_app("ask", &["Pepsi"], None);
    let pepsi = watch_id(&app, "Pepsi");
    app.quick_listen = true;
    app.session.listen();
    let info = app.info("", 48000, 16);
    let mut r = Recorder::start(&base.join("unsaved"), info).unwrap();
    r.write(&timegrapher_core::capture::Block {
        samples: vec![0.0; 48000 * 20],
        at: std::time::SystemTime::now(),
    })
    .unwrap();
    app.session.recorder = Some(r);
    app.session.set_state(session::State::Listening);
    // Stopped as the Listen card's Stop does.
    app.stop_listening();
    app.wait_for_save();
    assert!(app.quick_unsaved() && !app.needs_save_confirmation());
    app.line_pick(Some(pepsi.clone()), false);
    assert_eq!(app.confirm, Some(Confirm::PickWatch(Some(pepsi.clone()))));
    // The question shows, and stays until answered.
    let ctx = themed();
    for _ in 0..2 {
        let _ = ctx.run(screen(), |ctx| app.confirm_dialog(ctx));
    }
    assert_eq!(app.confirm, Some(Confirm::PickWatch(Some(pepsi.clone()))));
    assert!(app.open_watch().is_none() && app.session.recorder.is_some());
    // Cancel: nothing changes.
    app.confirm = None;
    assert!(app.open_watch().is_none() && app.session.recorder.is_some());
    // Don't Save.
    app.line_pick(Some(pepsi), true);
    assert!(
        app.session.recorder.is_none(),
        "the sound went, as answered"
    );
    assert_eq!(app.open_watch().unwrap().watch.name, "Pepsi");
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// The lift and beat rate set on the card are saved to a watch with no
/// calibre, as Edit Watch would, and never to one whose calibre sets
/// them.
#[test]
fn the_card_saves_its_lift_and_beat_rate_to_a_watch_with_no_calibre() {
    let c = timegrapher_core::calibres::find("2824-2").expect("ETA 2824-2 in the table");
    let (mut app, base) = line_app("save", &["Pepsi", "Seiko"], Some(&c.calibre));
    let saved = |name: &str| {
        let lib = library::Library::open(&base.join("Timegrapher Data")).unwrap();
        let w = lib.watches.iter().find(|e| e.watch.name == name).unwrap();
        (w.watch.lift, w.watch.bph)
    };
    app.pick_watch(&watch_id(&app, "Seiko"));
    app.set_lift(47.0);
    app.change_bph(Some(21600));
    app.save_card_to_watch();
    assert_eq!(saved("Seiko"), (Some(47.0), Some(21600)));
    app.pick_watch(&watch_id(&app, "Pepsi"));
    assert!(!app.watch_sets_here());
    let before = saved("Pepsi");
    app.lift_deg = 44.0;
    app.save_card_to_watch();
    assert_eq!(saved("Pepsi"), before, "the calibre's to set");
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// The title is cut before its ⏷, never the ⏷, and the narrowest card
/// says No Watch.
#[test]
fn the_watch_line_cuts_the_name_and_keeps_its_arrow() {
    let (mut app, base) = line_app("cut", &["Seiko 6R15 Diver Bench Movement"], None);
    let ctx = themed();
    let title = |app: &mut TimegrapherApp, w: f32| {
        let mut text = String::new();
        for _ in 0..2 {
            text = painted_text(&ctx.run(screen(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(Vec2::new(w, theme::TITLE_ROW_H), |ui| {
                        ui.set_max_width(w);
                        app.watch_line(ui, &[]);
                    });
                });
            }));
        }
        let line = text.lines().find(|l| l.ends_with('⏷'));
        line.unwrap_or_default().to_string()
    };
    assert_eq!(title(&mut app, 100.0), "No Watch ⏷");
    assert_eq!(title(&mut app, 230.0), format!("{NO_WATCH} ⏷"));
    app.pick_watch(&watch_id(&app, "Seiko 6R15 Diver Bench Movement"));
    let cut = title(&mut app, 100.0);
    assert!(cut.starts_with("Seiko") && cut.ends_with("… ⏷"), "{cut}");
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// The Settings gear is in the same place in both views: in Watches
/// the pane icons hide but keep their room.
#[test]
fn the_gear_stays_where_it_is_in_the_watches_view() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let ctx = themed();
    let wide = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(2560.0, 1440.0),
        )),
        ..Default::default()
    };
    let bar = |app: &mut TimegrapherApp| {
        let (mut right, mut text) = (0.0, String::new());
        for _ in 0..2 {
            text = painted_text(&ctx.run(wide(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.menu_bar(ui);
                    right = ui.min_rect().right();
                });
            }));
        }
        (right, text)
    };
    let (bench, shown) = bar(&mut app);
    app.view = View::Watches;
    let (watches, hidden) = bar(&mut app);
    assert_eq!(bench, watches);
    assert!(shown.contains("Analysis") && !hidden.contains("Analysis"));
}

/// A layout saved while Watches was a pane comes back without it, the
/// rest as it was left.
#[test]
fn a_layout_saved_with_the_watches_pane_comes_back_without_it() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    set_pane_visible(&mut app.panes.tiles, Pane::Charts, true);
    let mut saved = app.settings();
    let mut panes = saved.panes.take().unwrap();
    add_missing(&mut panes, Pane::Watch);
    let w = panes.tiles.find_pane(&Pane::Watch).unwrap();
    assert!(panes.tiles.is_visible(w));
    saved.panes = Some(panes);
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.apply_settings(saved);
    assert!(again.panes.tiles.find_pane(&Pane::Watch).is_none());
    assert!(layout_is_whole(&again.panes));
    assert!(pane_visible(&again.panes.tiles, Pane::Charts), "kept");
}

/// A Quick Listen still on show when a watch is picked, as New Watch…
/// picks the watch it adds, is asked about before a take of that watch
/// starts: Listen asks, and a Power Reserve Test waits for it to be
/// saved or set aside.
#[test]
fn a_take_of_a_watch_asks_about_quick_listens_sound() {
    let base = std::env::temp_dir().join(format!("tg-app-quick-ask-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let mut app = synthetic(5.0, 0.0);
    app.use_library(Some(root.clone()));
    app.session.listen();
    let info = app.info("Quick", 48000, 16);
    let mut r = Recorder::start(&base.join("scratch"), info).unwrap();
    r.write(&timegrapher_core::capture::Block {
        samples: vec![0.0; 48000 * 15],
        at: std::time::SystemTime::now(),
    })
    .unwrap();
    app.session.recorder = Some(r);
    app.session.set_state(session::State::Paused {
        since: Instant::now(),
    });
    app.quick_listen = true;
    app.pick_watch(&pepsi);
    assert!(app.quick_unsaved(), "picking keeps the readings on show");
    // New Watch… then Add Watch picks the watch it adds.
    app.watch_form = Some(watches::Form {
        watch: library::Watch::new("Speedy"),
        new: true,
        error: None,
    });
    app.save_watch_form();
    assert_eq!(
        app.open_watch().map(|e| e.watch.name.clone()).as_deref(),
        Some("Speedy")
    );
    assert!(
        app.quick_unsaved(),
        "the readings on show are Quick Listen's"
    );
    app.new_take(NewTake::Listen, false);
    assert_eq!(app.confirm, Some(Confirm::NewTake(NewTake::Listen)));
    // The question shows, rather than carrying on unasked.
    let ctx = themed();
    for _ in 0..2 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.confirm_dialog(ctx));
    }
    assert_eq!(app.confirm, Some(Confirm::NewTake(NewTake::Listen)));
    assert!(app.session.recorder.is_some(), "nothing let go yet");
    app.confirm = None;
    app.start_reserve();
    assert!(app.session.recorder.is_some() && app.reserve.is_none());
    // Start Test asks the same question.
    assert_eq!(app.confirm, Some(Confirm::ReserveStart));
    for _ in 0..2 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.confirm_dialog(ctx));
    }
    assert_eq!(
        app.confirm,
        Some(Confirm::ReserveStart),
        "shown, not passed"
    );
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// Quick Listen's sound that wasn't saved goes without a question at a
/// quit or a new take, and isn't kept to offer back.
#[test]
fn quick_listen_sound_goes_without_asking() {
    let base = std::env::temp_dir().join(format!("tg-app-quick-go-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = synthetic(5.0, 0.0);
    app.session.listen();
    let info = app.info("Quick", 48000, 16);
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
    app.quick_listen = true;
    assert!(app.unsaved_s() > 5.0);
    assert!(!app.needs_save_confirmation());
    eframe::App::on_exit(&mut app, None);
    assert!(kept_sounds(&base).is_empty());
    let _ = std::fs::remove_dir_all(&base);
}

/// The Calibre card's controls stay inside their rows: the Calibre
/// pop-up on the figure row, and under it the lift alone on the
/// narrowest card, the beat rate beside it on a roomier one and on the
/// full card.
#[test]
fn the_calibre_controls_fit_a_narrow_card() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.bph = Some(28800);
    let ctx = themed();
    let inner = |card: f32| card - 2.0 * f32::from(theme::CARD_PADDING);
    for w in [
        inner(CALIBRE_COMPACT_W),
        inner(CALIBRE_ROOMY_W),
        inner(CALIBRE_W),
    ] {
        let mut used = egui::Rect::NOTHING;
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let rect =
                        egui::Rect::from_min_size(ui.cursor().min, Vec2::new(w, theme::CONTROL_H));
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    child.spacing_mut().item_spacing.x = 8.0;
                    child.spacing_mut().interact_size.y = theme::CONTROL_H;
                    app.calibre_control(&mut child);
                    used = child.min_rect();
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect.translate(Vec2::new(0.0, 40.0)))
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    child.spacing_mut().item_spacing.x = 8.0;
                    child.spacing_mut().interact_size.y = theme::CONTROL_H;
                    app.rate_and_lift(&mut child);
                    let row = child.min_rect();
                    used = egui::Rect::from_min_size(
                        used.min,
                        Vec2::new(
                            used.width().max(row.width()),
                            used.height().max(row.height()),
                        ),
                    );
                });
            });
        }
        assert!(
            used.width() <= w + 0.5 && used.height() <= theme::CONTROL_H + 0.5,
            "{:?} in a {w} pt row",
            used.size()
        );
    }
}

/// A card's limits show in a chip in its title row, in Title Case, the
/// first of them with how many more; the whole sentences are its
/// hover. Where the words would push the card's caption off it, the
/// chip gives how many limits there are.
#[test]
fn a_cards_limits_show_in_a_chip_with_the_whole_in_its_hover() {
    assert_eq!(chip_words("9 s of beats"), "9 s of Beats");
    assert_eq!(chip_words("3% clipped"), "3% Clipped");
    assert_eq!(chip_words("28800 bph heard"), "28800 bph Heard");
    assert_eq!(chip_words("sides named by count"), "Sides Named by Count");
    let ctx = themed();
    // Fewer seconds of beats than an analysis asks for: a limit on
    // every reading.
    let noise = synthetic(6.0, 0.0);
    let r = noise.reading().expect("a reading");
    let limits = reading_limits("Beat Error", &r.caveats, noise.lift_deg, r.beats_used);
    assert!(!limits.is_empty(), "{:?}", r.caveats);
    let chip = match limits.len() {
        1 => chip_words(&limits[0].0),
        n => format!("{} +{}", chip_words(&limits[0].0), n - 1),
    };
    let drawn = |w: f32| {
        let mut texts = Vec::new();
        for _ in 0..2 {
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(w, 820.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let _ = noise.readouts(ui);
                    });
                },
            );
            texts = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>();
        }
        texts
    };
    let wide = drawn(1610.0);
    assert!(wide.contains(&chip), "{chip}: {wide:?}");
    for caption in ["Rate", "Amplitude", "Beat Error"] {
        assert!(wide.iter().any(|t| t == caption), "{caption}: {wide:?}");
    }
    // Too narrow for the words beside the caption, they go to the
    // caption's hover: never a bare count.
    let narrow = drawn(800.0);
    let count = limits.len().to_string();
    assert!(!narrow.contains(&count), "{count}: {narrow:?}");
    assert!(narrow.iter().any(|t| t == "Rate"), "{narrow:?}");
}

/// Hiss with the beat rate set: a reading of whatever peaks there are,
/// with its limits named.
fn noisy_app() -> TimegrapherApp {
    let mut noise = TimegrapherApp::with_devices(Vec::new(), None);
    noise.bph = Some(28_800);
    noise.reset_session(48000, "noise".into());
    let mut seed = 12_345_u32;
    let hiss: Vec<f32> = (0..48_000 * 20)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32 * 0.02 - 0.01
        })
        .collect();
    let live = noise.live.as_mut().unwrap();
    for b in hiss.chunks(960) {
        live.push(b);
    }
    noise.poll();
    assert!(noise.reading().is_some_and(|r| !r.watch_heard));
    noise
}

/// The reading cards are one height at every width, whatever they
/// show: waiting, reading, settling, or with their limits named.
#[test]
fn the_cards_keep_their_height_whatever_they_show() {
    let ctx = themed();
    let height = |app: &TimegrapherApp, w: f32| {
        let mut h = 0.0;
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(w, 820.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let top = ui.cursor().top();
                    let _ = app.readouts(ui);
                    h = ui.cursor().top() - top;
                });
            });
        }
        h
    };
    // A take opened.
    let waiting = synthetic(1.0, 0.0);
    assert!(waiting.reading().is_none_or(|r| r.rate_s_per_day.is_none()));
    let reading = synthetic(60.0, 0.0);
    assert!(reading.reading().is_some());
    let mut settling = synthetic(60.0, 0.0);
    settling
        .session
        .load(&[(0.0, Position::DialUp), (55.0, Position::DialDown)], &[]);
    settling.position = Position::DialDown;
    settling.session.start_settle(55.0);
    let mut masked = synthetic(60.0, 0.0);
    masked.mask(5.0, 30.0);
    let noise = noisy_app();
    for w in [800.0, 1280.0, 1900.0] {
        let h = height(&waiting, w);
        assert_eq!(
            h,
            theme::READINGS_CARD_H + ctx.style().spacing.item_spacing.y
        );
        for app in [&reading, &settling, &masked, &noise] {
            assert_eq!(height(app, w), h, "at {w} pt wide");
        }
    }
}

/// The watch form's lift angle and beat rate are the calibre's where it
/// gives them, shown and not set; with no calibre both are set there,
/// the beat rate Auto until picked, and there is no Clear.
#[test]
fn the_watch_form_takes_lift_and_beat_rate_from_the_calibre_or_from_you() {
    let c = timegrapher_core::calibres::find("2824-2").expect("ETA 2824-2 in the table");
    let lift = c.lift_angle_deg.expect("its lift");
    let bph = c.bph.expect("its beat rate");
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let ctx = themed();
    let drawn = |app: &mut TimegrapherApp| {
        let mut all = String::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                app.watch_form_dialog(ctx);
            });
            all = format!("{:?}", out.shapes);
        }
        all
    };
    let mut w = library::Watch::new("Diver");
    w.calibre = Some(c.calibre.clone());
    w.lift = Some(55.0);
    app.watch_form = Some(watches::Form {
        watch: w,
        new: true,
        error: None,
    });
    let d = drawn(&mut app);
    let from = format!("{}° from the calibre", fields::plain(lift));
    assert!(d.contains(&from), "{d}");
    assert!(d.contains(&format!("{bph} bph from the calibre")), "{d}");
    assert!(!d.contains("Clear"), "{d}");
    let w = &app.watch_form.as_ref().unwrap().watch;
    assert_eq!((w.lift, w.bph), (Some(lift), Some(bph)), "the calibre's");

    let mut bare = library::Watch::new("Pepsi");
    app.watch_form = Some(watches::Form {
        watch: bare.clone(),
        new: true,
        error: None,
    });
    let d = drawn(&mut app);
    assert!(d.contains("52°") && d.contains("(default)"), "{d}");
    bare.lift = Some(48.0);
    app.watch_form = Some(watches::Form {
        watch: bare,
        new: true,
        error: None,
    });
    let d = drawn(&mut app);
    assert!(d.contains("48°") && d.contains("\"Auto\""), "{d}");
    assert!(
        !d.contains("from the calibre") && !d.contains("Clear"),
        "{d}"
    );
}

/// With no watch picked, or a watch with no calibre, the Calibre card
/// asks for the beat rate and the lift angle under the Calibre pop-up;
/// a watch with a calibre the table knows names it instead.
#[test]
fn the_cards_ask_for_beat_rate_and_lift_only_without_a_calibre() {
    let base = std::env::temp_dir().join(format!("tg-app-cards-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let c = timegrapher_core::calibres::find("2824-2").expect("ETA 2824-2 in the table");
    let mut known = library::Watch::new("Diver");
    known.calibre = Some(c.calibre.clone());
    lib.add(&known).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let diver = lib.find("Diver").unwrap().watch.id.clone();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    let ctx = themed();
    let drawn = |app: &mut TimegrapherApp| {
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_width(CALIBRE_W - 2.0 * f32::from(theme::CARD_PADDING));
                ui.horizontal(|ui| app.calibre_control(ui));
                ui.horizontal(|ui| app.rate_and_lift(ui));
            });
        });
        // The lift angle's field: the one control a control's height.
        let field = out.shapes.iter().any(|c| match &c.shape {
            egui::Shape::Rect(r) => (r.rect.height() - theme::CONTROL_H).abs() < 0.5,
            _ => false,
        });
        format!("{:?}{}", out.shapes, if field { "LIFT FIELD" } else { "" })
    };
    // The beat rate's pop-up reads Auto, and the lift angle has its
    // field; a calibre's angle is a label.
    let asks = |d: &str| d.contains("\"Auto\"") && d.contains("LIFT FIELD");
    let none = drawn(&mut app);
    assert!(asks(&none) && none.contains("Not Set"), "{none}");
    app.pick_watch(&diver);
    let named = drawn(&mut app);
    assert!(!asks(&named), "{named}");
    assert!(named.contains(&c.calibre), "the calibre named: {named}");
    app.pick_watch(&pepsi);
    assert!(
        app.calibre.is_empty(),
        "not the calibre of the watch before"
    );
    let bare = drawn(&mut app);
    assert!(asks(&bare), "{bare}");
    assert!(
        !bare.contains("Not Set"),
        "no calibre menu for a watch: {bare}"
    );
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}
