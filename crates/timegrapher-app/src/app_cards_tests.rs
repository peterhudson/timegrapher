//! Calibres, settings and layouts, the readings cards and their fields,
//! the icon bar, and the keys that pick a position or a view.

use super::tests::{painted_text, screen, synthetic, themed};
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

#[test]
fn a_calibre_names_the_wheels_and_a_typed_one_is_kept() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    assert!(app.calibre_wheels().is_none());
    // From the table: its train, and its lift angle where it has one.
    let c = timegrapher_core::calibres::find("2824-2").expect("ETA 2824-2 in the table");
    app.set_calibre(c.calibre.clone());
    let wheels = app.calibre_wheels().expect("wheels");
    assert_eq!(wheels.len(), c.wheels.len());
    if let Some(l) = c.lift_angle_deg {
        assert_eq!(app.lift_deg, l);
    }
    // Typed in: tidied for the tests; the recent picks are kept with
    // the settings, the calibre itself in the user calibre file.
    app.cal.user.put_custom(calibre_ui::custom(
        "Bench Watch",
        [
            ("escape wheel", 5.0),
            ("fourth wheel", 60.0),
            ("centre wheel", 3600.0),
            (" Third Wheel ", 450.0),
            ("", 10.0),
        ],
    ));
    app.set_calibre("Bench Watch".into());
    let wheels = app.calibre_wheels().expect("wheels");
    assert_eq!(wheels.len(), 4, "the unnamed wheel is left out");
    assert!(wheels.iter().any(|w| w.name == "third wheel"));
    let mut next = TimegrapherApp::with_devices(Vec::new(), None);
    next.apply_settings(app.settings());
    assert_eq!(
        next.cal.recent,
        ["Bench Watch".to_string(), c.calibre.clone()]
    );
    assert!(app.settings().custom_calibres.is_empty());
    // The pick itself is about the watch on the stand, so it isn't kept.
    assert!(next.calibre.is_empty());
}

#[test]
fn unsaved_legacy_calibres_remain_in_app_settings() {
    let root = std::env::temp_dir().join(format!(
        "tg-app-legacy-calibre-protection-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("calibres.json");
    let original = b"unreadable original catalogue";
    std::fs::write(&path, original).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.cal = calibre_ui::Calibres::open(Some(path.clone()));
    let mut settings = Settings::default();
    settings
        .custom_calibres
        .push(crate::settings::CustomCalibre {
            name: "Legacy Bench".into(),
            wheels: vec![crate::settings::CustomWheel {
                name: " Third Wheel ".into(),
                period_s: 450.0,
            }],
        });
    let originals = settings.custom_calibres.clone();
    app.apply_settings(settings);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    // Actual app settings serialization preserves the original legacy form,
    // rather than only its normalized, temporary catalogue entry.
    let json = serde_json::to_string(&app.settings()).unwrap();
    let persisted: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(persisted.custom_calibres, originals);
    let mut next = TimegrapherApp::with_devices(Vec::new(), None);
    next.apply_settings(persisted.clone()); // no persistent path
    assert_eq!(next.settings().custom_calibres, originals);
    assert!(next.cal.find("Legacy Bench").is_some());
    // A new instance can load a repaired path and finish the migration.
    std::fs::remove_file(&path).unwrap();
    let mut repaired = TimegrapherApp::with_devices(Vec::new(), None);
    repaired.cal = calibre_ui::Calibres::open(Some(path.clone()));
    repaired.apply_settings(persisted);
    assert!(repaired.settings().custom_calibres.is_empty());
    assert!(calibre_ui::Calibres::open(Some(path))
        .find("Legacy Bench")
        .is_some());
    std::fs::remove_dir_all(root).unwrap();
}

/// A first start, with nothing saved, has the strip across and laid
/// out above the charts, as Across lays it out.
#[test]
fn a_first_start_lays_the_strip_across() {
    let fresh = TimegrapherApp::with_devices(Vec::new(), None);
    assert!(fresh.strip.horizontal);
    // The first row or column above the strip: a column (the strip
    // over the charts) for Across, a row for Down.
    let holder = |t: &Tree<Pane>| {
        let mut id = t.tiles.find_pane(&Pane::Strip)?;
        loop {
            id = t.tiles.parent_of(id)?;
            if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(l))) =
                t.tiles.get(id)
            {
                return Some(l.dir);
            }
        }
    };
    assert_eq!(holder(&fresh.panes), holder(&default_layout(true)));
    assert_ne!(holder(&fresh.panes), holder(&default_layout(false)));
}

#[test]
fn settings_come_back_as_they_were_left() {
    // A fresh app opens with the defaults the settings describe.
    let fresh = TimegrapherApp::with_devices(Vec::new(), None);
    let mut first = fresh.settings();
    first.panes = None;
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(Settings::default()).unwrap()
    );
    // Change a few views, keep them, and open a new app with them.
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    // Down, now that Across is the default: a saved Down stays.
    app.strip.horizontal = false;
    app.relayout(false);
    app.show_readings = false;
    app.sound_opts.scale = profiles::Scale::Decibels;
    app.hist_cumulative = true;
    app.folded.insert("Window".into());
    set_pane_visible(&mut app.panes.tiles, Pane::Distributions, true);
    app.set_average(30.0);
    let json = serde_json::to_string(&app.settings()).unwrap();
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.apply_settings(serde_json::from_str(&json).unwrap());
    assert!(!again.strip.horizontal);
    assert!(!again.show_readings);
    assert_eq!(again.sound_opts.scale, profiles::Scale::Decibels);
    assert!(again.hist_cumulative);
    assert!(again.folded.contains("Window"));
    assert_eq!(again.average_s, 30.0);
    assert!(pane_visible(&again.panes.tiles, Pane::Distributions));
    // Settings from an older app, with fields missing, fill in the rest.
    let old: Settings = serde_json::from_str(r#"{"hist_cumulative": true}"#).unwrap();
    assert!(old.hist_cumulative && old.rate_line);
    // A layout missing panes is set aside for the starting one.
    let mut broken = app.settings();
    let mut tiles = Tiles::default();
    let only = tiles.insert_pane(Pane::Strip);
    broken.panes = Some(Tree::new("panes", only, tiles));
    again.apply_settings(broken);
    assert!(layout_is_whole(&again.panes));
    // A layout from before the Steadiness pane keeps its arrangement
    // and gains the pane as a tab beside the profile.
    let mut before = app.settings();
    let mut tree = app.panes.clone();
    let id = tree.tiles.find_pane(&Pane::SteadyReadings).unwrap();
    let tabs = tree.tiles.parent_of(id).unwrap();
    if let Some(egui_tiles::Tile::Container(c)) = tree.tiles.get_mut(tabs) {
        c.remove_child(id);
    }
    tree.tiles.remove(id);
    assert!(!layout_is_whole(&tree));
    before.panes = Some(tree);
    again.strip.horizontal = true;
    again.apply_settings(before);
    assert!(layout_is_whole(&again.panes));
    assert!(!again.strip.horizontal, "the saved arrangement was kept");
    let sound = again.panes.tiles.find_pane(&Pane::Sound).unwrap();
    let steady = again.panes.tiles.find_pane(&Pane::SteadyReadings).unwrap();
    assert_eq!(
        again.panes.tiles.parent_of(sound),
        again.panes.tiles.parent_of(steady)
    );
}

/// Every word one frame of the whole window draws, at `w` × 800.
fn window_texts(app: &mut TimegrapherApp, ctx: &egui::Context, w: f32) -> Vec<String> {
    let mut texts = Vec::new();
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(w, 800.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| app.panels(ctx));
        texts.clear();
        for c in &out.shapes {
            if let egui::Shape::Text(t) = &c.shape {
                texts.push(t.galley.text().to_string());
            }
        }
    }
    texts
}

/// The app opens on Quick Listen's layout: the readings, the Paper
/// Strip with its Rate Line and Parallel Guides, and the Tick Tock
/// Profile with the Scope behind it. Every other pane is hidden, there
/// is no Take or Microphone pane, and Watches is a view, not a pane.
#[test]
fn the_app_opens_on_the_quick_listen_layout() {
    let app = TimegrapherApp::with_devices(Vec::new(), None);
    let shown: Vec<Pane> = Pane::ALL
        .into_iter()
        .filter(|&p| pane_visible(&app.panes.tiles, p))
        .collect();
    assert_eq!(shown, [Pane::Strip, Pane::Sound, Pane::Scope]);
    assert!(app.panes.tiles.find_pane(&Pane::Watch).is_none());
    assert!(app.rate_line && app.rate_guides);
    // The Scope shares the profile's tabs, behind it.
    let sound = app.panes.tiles.find_pane(&Pane::Sound).unwrap();
    let scope = app.panes.tiles.find_pane(&Pane::Scope).unwrap();
    let tabs = app.panes.tiles.parent_of(sound).unwrap();
    assert_eq!(app.panes.tiles.parent_of(scope), Some(tabs));
    let Some(egui_tiles::Container::Tabs(t)) = app.panes.tiles.get_container(tabs) else {
        panic!("the profile sits in tabs");
    };
    assert_eq!(t.active, Some(sound));
    // A pane saved as Microphone by an older app reads back as one the
    // app has, so the rest of the settings aren't lost with it.
    let old: Pane = serde_json::from_str("\"Microphone\"").unwrap();
    assert_eq!(old, Pane::Watch);
}

/// A pane too short for every reading's plot draws the first ones at a
/// readable height and names the rest; a tall one draws them all.
#[test]
fn a_short_pane_names_the_plots_it_has_no_room_for() {
    let run = |height: f32| {
        let ctx = egui::Context::default();
        let mut drawn = Vec::new();
        let mut fit = [false; 3];
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(800.0, height),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    fit = stacked(ui, [true; 3], (STACKED_MIN_H, false), |ui, k, _| {
                        drawn.push((k, ui.max_rect().height()));
                    });
                });
            },
        );
        (drawn, painted_text(&out), fit)
    };
    // What it says it drew, for the key, is what it drew.
    let (drawn, text, fit) = run(140.0);
    assert_eq!(fit, [true, false, false]);
    assert_eq!(drawn.len(), 1, "{drawn:?}");
    assert!(drawn[0].1 >= STACKED_MIN_H, "{drawn:?}");
    assert!(
        text.contains("Amplitude and Beat Error not shown: no room"),
        "{text}"
    );
    let (drawn, text, fit) = run(600.0);
    assert_eq!(fit, [true; 3]);
    assert_eq!(drawn.len(), 3, "{drawn:?}");
    assert!(!text.contains("not shown"), "{text}");
}

/// Settings from before the charts and the distributions were one pane
/// each still read: each old chart is the Charts pane, each old
/// distribution the Distributions pane, the old charts' Match Strip is
/// left, and the layout starts afresh with one of each, hidden, while
/// the other settings are kept.
#[test]
fn an_old_layout_with_a_pane_for_each_chart_reads_back() {
    for (name, pane) in [
        ("Rate", Pane::Charts),
        ("Amplitude", Pane::Charts),
        ("BeatError", Pane::Charts),
        ("RateHistogram", Pane::Distributions),
        ("AmplitudeHistogram", Pane::Distributions),
        ("BeatErrorHistogram", Pane::Distributions),
    ] {
        let old: Pane = serde_json::from_str(&format!("\"{name}\"")).unwrap();
        assert_eq!(old, pane, "{name}");
    }
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.hist_cumulative = true;
    // An older app's file, naming a chart and a distribution the old
    // way, with Match Strip on.
    let json = serde_json::to_string(&app.settings()).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let tree = serde_json::to_string(&value["panes"])
        .unwrap()
        .replacen("\"Charts\"", "\"Rate\"", 1)
        .replacen("\"Distributions\"", "\"AmplitudeHistogram\"", 1);
    value["panes"] = serde_json::from_str(&tree).unwrap();
    value["layout"] = 2.into();
    value["span_of_strip"] = true.into();
    value.as_object_mut().unwrap().remove("chart_series");
    let old: Settings = serde_json::from_value(value).unwrap();
    let mut back = TimegrapherApp::with_devices(Vec::new(), None);
    back.apply_settings(old);
    assert!(back.hist_cumulative, "the rest is kept");
    assert_eq!(back.chart_series, RATE_ONLY);
    for p in [Pane::Charts, Pane::Distributions] {
        let n = back
            .panes
            .tiles
            .tiles()
            .filter(|t| matches!(t, egui_tiles::Tile::Pane(q) if *q == p))
            .count();
        assert_eq!(n, 1, "{p:?}");
        assert!(!pane_visible(&back.panes.tiles, p), "{p:?}");
    }
    // Each pane's readings come back as they were left.
    app.chart_series = [false, true, false];
    app.dist_series = [true, false, true];
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.apply_settings(app.settings());
    assert_eq!(again.chart_series, [false, true, false]);
    assert_eq!(again.dist_series, [true, false, true]);
}

/// A layout saved before Quick Listen's starts afresh once; one saved
/// since comes back as it was left.
#[test]
fn an_old_layout_starts_afresh_on_the_quick_listen_layout() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    set_pane_visible(&mut app.panes.tiles, Pane::Charts, true);
    let mut before = app.settings();
    assert_eq!(before.layout, LAYOUT);
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.apply_settings(before.clone());
    assert!(pane_visible(&again.panes.tiles, Pane::Charts), "kept");
    // A file from before the version was kept reads as the oldest.
    let none: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(none.layout, 0);
    before.layout = LAYOUT - 1;
    let mut old = TimegrapherApp::with_devices(Vec::new(), None);
    old.apply_settings(before);
    assert!(!pane_visible(&old.panes.tiles, Pane::Charts), "afresh");
}

/// The Current Position card's six buttons stay big enough to read and
/// press, in one row, where the window has room for every card's full
/// form (1610). Narrower (1000, 800), the card is captioned Position
/// and one pop-up of the six takes the buttons' place. Either way every
/// card of the row stays inside the window with the Calibre card's
/// caption whole.
#[test]
fn the_position_buttons_keep_their_size() {
    let ctx = themed();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let rects = |app: &mut TimegrapherApp, w: f32| {
        let mut found = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(w, 800.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| app.panels(ctx));
            found.clear();
            for c in &out.shapes {
                if let egui::Shape::Text(t) = &c.shape {
                    found.push((t.galley.text().to_string(), t.visual_bounding_rect()));
                }
            }
        }
        found
    };
    let at = |found: &[(String, egui::Rect)], text: &str| {
        found
            .iter()
            .find(|(t, _)| t == text)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("{text}: {found:?}"))
    };
    for (w, buttons, caption) in [
        (1610.0, true, "Current Position"),
        (1000.0, false, "Position"),
        (800.0, false, "Position"),
    ] {
        let found = rects(&mut app, w);
        at(&found, caption);
        if buttons {
            let (du, cd) = (at(&found, "DU"), at(&found, "CD"));
            assert!((du.center().y - cd.center().y).abs() < 1.0, "{w}");
            // Next to each other, the codes' centres are far enough
            // apart for a button of 28 points or more each.
            let cu = at(&found, "CU");
            assert!(cu.center().x - du.center().x >= 2.0 * 28.0, "{w}");
        } else {
            // The pop-up shows the current position's code alone.
            at(&found, "DU");
            assert!(!found.iter().any(|(t, _)| t == "CR"), "{w}: {found:?}");
        }
        // The Calibre card's title, its Watch line.
        let calibre = found
            .iter()
            .find(|(t, _)| t.starts_with(NO_WATCH_SHORT))
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("the Watch line: {found:?}"));
        assert!(calibre.right() <= w - 12.0, "{w}: {calibre:?}");
    }
}

#[test]
fn the_calibre_cards_fields_end_on_one_right_edge() {
    let ctx = themed();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    for w in [1000.0, 1280.0, 1610.0, 2560.0] {
        let (mut texts, mut frames) = (Vec::new(), Vec::new());
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(w, 900.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| app.panels(ctx));
            texts.clear();
            frames.clear();
            for c in &out.shapes {
                match &c.shape {
                    egui::Shape::Text(t) => {
                        texts.push((t.galley.text().to_string(), t.visual_bounding_rect()))
                    }
                    egui::Shape::Rect(r) => frames.push(r.rect),
                    _ => {}
                }
            }
        }
        // The smallest frame round each field's words.
        let field = |words: &str| {
            let at = texts
                .iter()
                .find(|(t, _)| t == words)
                .map(|(_, r)| r.center())
                .unwrap_or_else(|| panic!("{w}: {words}: {texts:?}"));
            frames
                .iter()
                .filter(|r| r.contains(at) && r.height() < 40.0)
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .copied()
                .unwrap_or_else(|| panic!("{w}: no frame round {words}"))
        };
        let (calibre, bph) = (field("Not Set"), field("Auto"));
        assert!(bph.top() > calibre.bottom(), "{w}: {calibre:?} {bph:?}");
        assert!(
            (bph.right() - calibre.right()).abs() < 0.5,
            "{w}: Auto ends at {} where Not Set ends at {}",
            bph.right(),
            calibre.right()
        );
    }
}

/// The Current Position card sets the position only while it can:
/// before anything is on show its six buttons can be pressed; with a
/// take on show they only show the position, as history, their codes
/// dimmed. Either way the card names the position with both its codes
/// and has all six.
#[test]
fn the_position_card_only_shows_a_takes_position() {
    let ctx = themed();
    // The colour "DD" (never the current position here) is drawn in.
    let ink = |app: &mut TimegrapherApp| {
        let mut colour = None;
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1610.0, 800.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| app.panels(ctx));
            colour = out.shapes.iter().find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == "DD" => {
                    Some(t.galley.job.sections[0].format.color)
                }
                _ => None,
            });
        }
        colour.expect("the DD button")
    };
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    assert!(app.position_editable());
    let settable = ink(&mut app);
    let texts = window_texts(&mut app, &ctx, 1610.0);
    assert!(texts.iter().any(|t| t == "Current Position"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "DU · CH"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Dial Up"), "{texts:?}");
    for p in Position::ALL {
        assert!(
            texts.iter().any(|t| t == p.code()),
            "{}: {texts:?}",
            p.code()
        );
    }
    let mut app = synthetic(20.0, 0.0);
    assert!(!app.position_editable());
    let texts = window_texts(&mut app, &ctx, 1610.0);
    for p in Position::ALL {
        assert!(
            texts.iter().any(|t| t == p.code()),
            "{}: {texts:?}",
            p.code()
        );
    }
    // Compact, the two-letter code beside the picture, once, not the
    // name, which a compact card cuts short.
    let texts = window_texts(&mut app, &ctx, 1000.0);
    assert!(texts.iter().any(|t| t == "Position"), "{texts:?}");
    // The cards are drawn before the panes; the Paper Strip's Position
    // Brush shows a code of its own.
    let cards = texts.iter().take_while(|t| *t != "Paper Strip");
    assert_eq!(cards.filter(|t| *t == "DU").count(), 1, "{texts:?}");
    for gone in ["Dial", "Up", "Dial Up"] {
        assert!(!texts.iter().any(|t| t == gone), "{gone}: {texts:?}");
    }
    let pal = theme::palette(ctx.style().visuals.dark_mode);
    assert_eq!(settable, pal.text);
    assert_eq!(ink(&mut app), pal.text_tertiary);
}

/// The card's 6 | 10 (#1473, Hugh's spec): at 6 the six buttons as
/// before; clicking 10 adds the four 45° stops in a second row, in hour
/// order, under the six, and the name and its one code on one line. A
/// 45° stop on show keeps the card at 10 even with the switch at 6.
#[test]
fn the_position_card_turns_to_ten_and_back() {
    let ctx = themed();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let frame = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        let mut texts = Vec::new();
        for events in [events, Vec::new(), Vec::new()] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1610.0, 800.0),
                )),
                events,
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| app.panels(ctx));
            texts = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some((
                        t.galley.text().to_string(),
                        t.galley.rect.translate(t.pos.to_vec2()).center(),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
        }
        texts
    };
    let at = |texts: &[(String, egui::Pos2)], words: &str| {
        texts.iter().find(|(t, _)| t == words).map(|(_, p)| *p)
    };
    let six = frame(&mut app, Vec::new());
    assert!(!app.ten_positions);
    assert!(Position::HALF_HOURS
        .iter()
        .all(|p| at(&six, p.code()).is_none()));
    let ten = at(&six, "10").expect("the 6 | 10 switch");
    let click = |pos| {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![egui::Event::PointerMoved(pos), button(true), button(false)]
    };
    let texts = frame(&mut app, click(ten));
    assert!(app.ten_positions, "10 clicked");
    let row = |p: Position| at(&texts, p.code()).unwrap_or_else(|| panic!("{p:?}: {texts:?}"));
    let first = row(Position::DialUp).y;
    for pair in Position::HALF_HOURS.windows(2) {
        assert!(row(pair[0]).x < row(pair[1]).x, "{pair:?} in hour order");
    }
    for p in Position::HALF_HOURS {
        assert!(row(p).y > first + 10.0, "{p:?} under the six");
    }
    for p in Position::ALL {
        assert!((row(p).y - first).abs() < 1.0, "{p:?} on the first row");
    }
    // The name and its code on one line.
    let (name, code) = (
        at(&texts, "Dial Up").unwrap(),
        at(&texts, "DU · CH").unwrap(),
    );
    assert!((name.y - code.y).abs() < 3.0 && code.x > name.x);
    // Back to 6.
    // The switch's 6, left of its 10 so it reads 6 | 10: the picture has a
    // 6 of its own.
    let ten_at = at(&texts, "10").expect("the 10");
    let six_at = texts
        .iter()
        .filter(|(t, p)| {
            t == "6" && (p.y - ten_at.y).abs() < 4.0 && p.x < ten_at.x && ten_at.x - p.x < 40.0
        })
        .map(|(_, p)| *p)
        .next()
        .expect("the 6");
    let texts = frame(&mut app, click(six_at));
    assert!(!app.ten_positions);
    assert!(at(&texts, "1H30").is_none());
    // A 45° stop on show keeps the card at 10, its code once.
    app.set_position(Position::Up4h30);
    let texts = frame(&mut app, Vec::new());
    assert!(!app.ten_positions, "showing it doesn't change the setting");
    assert!(at(&texts, "1H30").is_some(), "{texts:?}");
    // The cards are drawn before the panes, whose strip names it too.
    let cards = texts.iter().take_while(|(t, _)| t != "Paper Strip");
    assert_eq!(
        cards.filter(|(t, _)| t == "4H30").count(),
        2,
        "beside the name and on its button: {texts:?}"
    );
    assert!(at(&texts, "4:30 Up").is_some());
}

/// Each position's second code is Witschi's: the hour that points up
/// standing, CH and CB lying flat; and the picture turns the watch so
/// that hour is at the top.
#[test]
fn each_position_has_witschis_code_and_its_hour_up() {
    use crate::icons::Lying;
    let pairs: Vec<(&str, &str)> = Position::ALL
        .iter()
        .map(|p| (p.code(), p.witschi()))
        .collect();
    assert_eq!(
        pairs,
        [
            ("DU", "CH"),
            ("DD", "CB"),
            ("CU", "3H"),
            ("CD", "9H"),
            ("CL", "6H"),
            ("CR", "12H")
        ]
    );
    let halves: Vec<(&str, &str, &str)> = Position::HALF_HOURS
        .iter()
        .map(|p| (p.code(), p.witschi(), p.label()))
        .collect();
    assert_eq!(
        halves,
        [
            ("1H30", "1H30", "1:30 Up (1H30)"),
            ("4H30", "4H30", "4:30 Up (4H30)"),
            ("7H30", "7H30", "7:30 Up (7H30)"),
            ("10H30", "10H30", "10:30 Up (10H30)")
        ]
    );
    // The picture turns the hour at the top up: 30° an hour.
    for p in Position::TEN {
        match p.lying() {
            Lying::Top(deg) => {
                let (h, half) = p
                    .witschi()
                    .split_once('H')
                    .map(|(h, m)| (h.parse::<f32>().unwrap() % 12.0, m == "30"))
                    .unwrap();
                assert_eq!(deg, 30.0 * h + if half { 15.0 } else { 0.0 }, "{p:?}");
            }
            Lying::DialUp => assert_eq!(p, Position::DialUp),
            Lying::DialDown => assert_eq!(p, Position::DialDown),
        }
        assert_eq!(Position::of_core(p.core()), Some(p));
        assert_eq!(Position::from_name(p.code()), Some(p));
    }
}

/// Every pane has one place in the icon bar's groups: the core panes
/// with Watches first, Analysis (the charts, the distributions, the
/// Steadiness views and Isochronism) and Tests. At 1200 points every
/// icon fits beside the menus and the Bench | Watches switch; at 1000
/// and narrower, the largest group, Analysis, folds into a menu (the top
/// of the window spec, #1000), and the Tests still show: as icons at
/// 1000, folded too at 760.
#[test]
fn every_pane_has_one_place_in_the_icon_bar() {
    let mut all: Vec<Pane> = [&Pane::CORE[..], &Pane::ANALYSIS, &Pane::TESTS].concat();
    assert_eq!(Pane::CORE[0], Pane::Strip);
    assert!(!Pane::ALL.contains(&Pane::Watch), "Watches is a view");
    assert!(Pane::ANALYSIS.contains(&Pane::Isochronism));
    assert_eq!(Pane::ANALYSIS[..2], [Pane::Charts, Pane::Distributions]);
    assert_eq!(all.len(), Pane::ALL.len());
    all.sort();
    all.dedup();
    assert_eq!(all.len(), Pane::ALL.len());
    let ctx = themed();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    // At 1200 points every icon fits, with the Charts and
    // Distributions groups gone into Analysis.
    let mid = window_texts(&mut app, &ctx, 1200.0);
    assert!(mid.iter().any(|t| t == "Analysis"), "{mid:?}");
    assert!(mid.iter().any(|t| t == "Tests"), "{mid:?}");
    // No group folded (the Watch line's ⏷ aside).
    let folded = |t: &String| t.contains('⏷') && !t.starts_with(NO_WATCH_SHORT);
    assert!(!mid.iter().any(folded), "{mid:?}");
    for w in [1000.0, 760.0] {
        let narrow = window_texts(&mut app, &ctx, w);
        assert!(
            narrow
                .iter()
                .any(|t| t.starts_with("Analysis") && t.contains('⏷')),
            "{w}: {narrow:?}"
        );
        assert!(
            narrow.iter().any(|t| t.starts_with("Tests")),
            "{w}: {narrow:?}"
        );
    }
    // At 1000 points the Tests keep their icons; at 760 they fold too.
    let mut at = |w| window_texts(&mut app, &ctx, w);
    assert!(at(1000.0).iter().any(|t| t == "Tests"));
    assert!(at(760.0).iter().any(|t| t == "Tests ⏷"));
    let wide = window_texts(&mut app, &ctx, 2000.0);
    assert!(wide.iter().any(|t| t == "Analysis"), "{wide:?}");
}

/// Readings, in the View menu and as the first icon in the bar, hides
/// and shows the cards.
#[test]
fn readings_hides_the_cards() {
    let ctx = themed();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let shown = window_texts(&mut app, &ctx, 1440.0);
    assert!(shown.iter().any(|t| t == "Listen"), "{shown:?}");
    app.show_readings = false;
    let hidden = window_texts(&mut app, &ctx, 1440.0);
    for card in ["Current Position", "Listen", "Beat Rate"] {
        assert!(!hidden.iter().any(|t| t == card), "{card}: {hidden:?}");
    }
}

/// Unsorted Takes puts the app back in Quick Listen: no watch, no
/// calibre, the default lift angle and the beat rate found from the
/// beats; with readings on show they keep their settings until the
/// next Listen. ‹ All Watches shows the list with the watch picked
/// still picked and its row marked.
#[test]
fn unsorted_takes_goes_back_to_quick_listen() {
    let base = std::env::temp_dir().join(format!("tg-app-test-unsorted-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let mut w = library::Watch::new("Pepsi");
    w.lift = Some(55.0);
    w.bph = Some(21600);
    lib.add(&w).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.pick_watch(&pepsi);
    app.set_calibre("ETA 2824-2".into());
    app.set_lift(55.0);
    assert_eq!(app.bph, Some(21600));

    // ‹ All Watches: the list, Pepsi still picked.
    app.list_for = Some((app.watch_open.clone(), false));
    assert!(app.list_shown());
    assert_eq!(app.watch_open.as_deref(), Some(pepsi.as_str()));

    app.pick_unsorted();
    assert!(!app.list_shown(), "Unsorted Takes on show");
    assert!(app.watch_open.is_none() && app.unsorted_open);
    assert!(app.calibre.is_empty() && app.watch.is_empty());
    assert_eq!(app.lift_deg, DEFAULT_LIFT_DEG);
    assert!(!app.lift_known());
    assert_eq!(app.bph, None);

    // With readings on show, the settings wait for the next Listen.
    drop(app);
    let mut shown = synthetic(15.0, 0.0);
    shown.use_library(Some(root.clone()));
    shown.set_lift(55.0);
    shown.pick_unsorted();
    assert_eq!(shown.lift_deg, 55.0, "the readings keep their lift");
    assert!(shown.quick_defaults);
    assert!(shown.quick_listen_ready(true));
    assert_eq!(shown.lift_deg, DEFAULT_LIFT_DEG);
    assert!(!shown.quick_defaults);
    drop(shown);
    let _ = std::fs::remove_dir_all(&base);
}

/// A watch an older version marked Archived is listed and offered
/// like any other, and saving it in the form clears the mark.
#[test]
fn an_archived_watch_is_listed_and_saving_clears_the_mark() {
    let base = std::env::temp_dir().join(format!("tg-app-test-archived-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let mut w = library::Watch::new("Old Seiko");
    w.archived = true;
    lib.add(&w).unwrap();
    let id = lib.find("Old Seiko").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    let watch = {
        let Ok(Some(l)) = &app.library else {
            panic!("no library")
        };
        let listed = watches::shown(l, "", app.watch_sort);
        assert_eq!(listed.len(), 1, "the archived watch is listed");
        l.find("Old Seiko").unwrap().watch.clone()
    };
    app.pick_watch(&id);
    assert_eq!(app.watch_open.as_deref(), Some(id.as_str()));
    app.watch_form = Some(watches::Form {
        watch,
        new: false,
        error: None,
    });
    app.save_watch_form();
    let again = library::Library::open(&root).unwrap();
    assert!(!again.find("Old Seiko").unwrap().watch.archived);
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// Two minutes of beats at 28,800 bph from a watch gaining `gain`
/// (a fraction: 1e-4 is 8.64 s a day), as a take's log.
fn gaining_log(gain: f64) -> BeatLog {
    BeatLog {
        sample_rate: 48000,
        duration_s: 120.0,
        bph: 28800,
        lift_deg: 52.0,
        beats: (0..960)
            .map(|index| timegrapher_core::beats::Beat {
                index,
                time: index as f64 / 8.0 * (1.0 - gain),
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
    }
}

/// A take opened shows whole: Fit along the strip, and the narrowest
/// listed width that holds every beat, with Auto-Centre off or on.
#[test]
fn an_opened_take_fits_the_strip() {
    for follow in [false, true] {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.follow = follow;
        app.strip.set_span(10.0);
        app.strip.set_half_width(1.0);
        // 12 ms gained over the two minutes.
        app.show_log(gaining_log(1e-4), "gaining".into(), vec![], vec![]);
        assert!((app.strip.span_s - app.latest_s()).abs() < 1e-9);
        assert!(app.strip.span_s > 119.0);
        let half = app.strip.half_width_ms;
        // Centred, the spread is ±6 ms; kept on the newest beats, it
        // reaches 12 ms on one side.
        assert_eq!(half, if follow { 20.0 } else { 10.0 }, "follow {follow}");
        let live = app.live.as_ref().unwrap();
        let anchor = app.anchor.unwrap();
        for b in live.beats() {
            let lead = anchor.lead_ms(b, 0.125);
            assert!(lead.abs() < half, "{lead} outside ±{half}");
        }
        // Listen goes back to the strip as it was, each of width and
        // length unless it was set by hand since, which stays (Hugh).
        assert_eq!(
            (app.live_view().span_s, app.live_view().half_width_ms),
            (10.0, 1.0)
        );
        app.strip.set_half_width(5.0);
        assert_eq!(
            (app.live_view().span_s, app.live_view().half_width_ms),
            (10.0, 5.0)
        );
    }
}

/// A test or a new take set up after an opened take starts from the
/// person's own Width and Length from before the take was opened, not
/// the take's fitted view (Walter, Hugh); one of them set by hand while
/// the take was on show stays as set.
#[test]
fn a_new_test_starts_from_the_strip_as_it_was_before_the_open() {
    // How each case starts: a test, Take Setup's Listen, or the Listen
    // button straight on the take; and what the person set by hand.
    let (test, setup, listen) = (Some(true), Some(false), None);
    for (kind, start, width, span) in [
        (NewTake::Positions, test, None, None),
        (NewTake::PowerReserve, test, None, None),
        (NewTake::Positions, test, Some(250.0), None),
        (NewTake::Positions, test, None, Some(60.0)),
        (NewTake::Listen, setup, None, None),
        (NewTake::Listen, listen, None, None),
    ] {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.strip.set_span(30.0);
        app.strip.set_half_width(10.0);
        let mut fit = app.strip_fit;
        app.show_log(gaining_log(1e-4), "gaining".into(), vec![], vec![]);
        assert!(app.strip.span_s > 119.0, "fitted to the take");
        if let Some(w) = width {
            app.strip.set_half_width(w);
        }
        if let Some(s) = span {
            app.strip.set_span(s);
            fit = app.strip_fit;
        }
        match start {
            Some(true) => {
                app.new_take(kind, true);
                assert_eq!(app.ready_test(), Some(kind));
            }
            Some(false) => app.take_setup(),
            None => app.start_microphone(),
        }
        let case = format!("{kind:?} {start:?}, by hand {width:?} {span:?}");
        // The take's pair is gone: a later test has nothing stale to restore.
        assert!(app.live_strip.is_none(), "{case}");
        assert_eq!(
            (app.strip.span_s, app.strip.half_width_ms, app.strip_fit),
            (span.unwrap_or(30.0), width.unwrap_or(10.0), fit),
            "{case}"
        );
    }
}

/// A watch's take on show, then Unsorted Takes: the take is put away
/// and the settings go back to Quick Listen's, not by chance.
#[test]
fn unsorted_takes_after_a_watch_take_resets_the_settings() {
    let base =
        std::env::temp_dir().join(format!("tg-app-test-unsorted-take-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    let mut w = library::Watch::new("Pepsi");
    w.lift = Some(55.0);
    w.bph = Some(28800);
    lib.add(&w).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.pick_watch(&pepsi);
    app.set_calibre("ETA 2824-2".into());
    app.show_log(gaining_log(0.0), "take".into(), vec![], vec![]);
    app.input = Input::File;
    app.opened_path = base.join("take.flac").display().to_string();
    assert!(app.take_on_show().is_some());
    assert_eq!(app.bph, Some(28800));

    app.pick_unsorted();
    assert!(app.take_on_show().is_none(), "the watch's take is put away");
    assert!(app.live.is_none());
    assert!(app.calibre.is_empty());
    assert_eq!(app.bph, None);
    assert_eq!(app.lift_deg, DEFAULT_LIFT_DEG);
    assert!(!app.quick_defaults);
    drop(app);
    let _ = std::fs::remove_dir_all(&base);
}

/// A new beat rate reads the take on show again at that rate, with
/// the settings as they are now, not as the take saved them.
#[test]
fn a_new_beat_rate_reads_the_take_again() {
    let dir = std::env::temp_dir().join(format!("tg-app-test-reread-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let take = dir.join("take.wav");
    timegrapher_core::audio::write_wav(&take, &audio).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let finish = |app: &mut TimegrapherApp| {
        let t = Instant::now();
        while app.batch.is_some() && t.elapsed().as_secs() < 120 {
            app.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    app.start_batch(&take);
    finish(&mut app);
    assert!(app.take_on_show().is_some());
    app.set_lift(49.0);
    app.change_bph(Some(21600));
    assert!(app.batch.is_some(), "read again");
    finish(&mut app);
    assert!(
        app.take_on_show().is_some(),
        "still on show, no Open needed"
    );
    assert_eq!(app.bph, Some(21600));
    assert_eq!(app.live.as_ref().unwrap().bph(), Some(21600));
    assert_eq!(app.lift_deg, 49.0, "the settings as they were");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The readings' cards carry what a take needs: the position, Listen
/// and Save with the take's length (on the card from 1000 up; in its
/// title row below that, the reading cards keeping their captions
/// whole) and Average Over on a card of their own, the lift angle, and the beat rate's menu with the calibre: in
/// full where the window has room (1610), one control each where it
/// hasn't (1000). Nothing says Take, Pause or Resume, and no Replay is
/// offered.
#[test]
fn the_readings_cards_carry_the_take_controls() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let ctx = themed();
    for w in [800.0, 1000.0, 1610.0] {
        let texts = window_texts(&mut app, &ctx, w);
        // The Calibre card's title is its Watch line, No Watch while
        // none is picked.
        let line = if w > 800.0 {
            format!("{NO_WATCH} ⏷")
        } else {
            format!("{NO_WATCH_SHORT} ⏷")
        };
        let mut wants = vec!["DU", "Listen", "Not Set", "Watches", "Lift", line.as_str()];
        if w > 800.0 {
            // At 800 the beat rate heads the Calibre menu, and the lift
            // has the row: nothing finds the lift, Auto finds the rate.
            wants.push("Auto");
        }
        // The take's length shows at every width, and the reading cards
        // keep their captions beside it: whole where they fit, by their
        // short names at 800 (#1061), never cut.
        wants.extend(["0:00", "Rate"]);
        wants.extend(if w > 800.0 {
            ["Amplitude", "Beat Error"]
        } else {
            ["Amp.", "B.E."]
        });
        if w <= 800.0 {
            wants.extend(["Position", "Avg 10 s"]);
        }
        if w > 1000.0 {
            wants.extend([
                "Current Position",
                "Dial Up",
                "10 s",
                "CR",
                "DU · CH",
                "0:00",
                "Not Started",
                "Average Over",
                "Auto",
            ]);
        } else if w > 800.0 {
            // The take's length in the compact Listen card's title row,
            // Average Over's pop-up saying what it is, and the beat
            // rate beside the lift on the Calibre card.
            wants.extend(["Position", "Avg 10 s", "Auto"]);
        }
        for want in wants {
            assert!(texts.iter().any(|t| t == want), "{want} at {w}: {texts:?}");
        }
        // No beats yet: the profile's level axis has no marks.
        assert!(!texts.iter().any(|t| t.starts_with("0.0000")), "{texts:?}");
        for gone in [
            "Input Clock",
            "Beat Rate",
            "Take",
            "New Take",
            "Pause",
            "Resume",
            "Replay",
            "Microphone",
        ] {
            assert!(!texts.iter().any(|t| t == gone), "{gone} at {w}: {texts:?}");
        }
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with("Quick Listen · No watch · ")),
            "the status line: {texts:?}"
        );
    }
}

/// A reading with limits keeps its card's caption whole on the
/// narrowest window: a limit with no room for its chip beside the
/// caption is in the caption's hover.
#[test]
fn a_readings_limits_never_cut_its_caption() {
    let mut app = synthetic(6.0, 0.0);
    let r = app.reading().expect("a reading");
    assert!(!r.caveats.is_empty(), "6 s of beats is short");
    let ctx = themed();
    app.session.listen();
    for w in [740.0, 760.0, 780.0, 800.0, 1000.0] {
        let texts = window_texts(&mut app, &ctx, w);
        for caption in ["Rate", "Amplitude", "Beat Error"] {
            // Whole, or by its short name on a narrow card (#1061).
            let short = timegrapher_core::steady_views::short_name(caption).unwrap_or(caption);
            assert!(
                texts.iter().any(|t| t == caption || t == short),
                "{caption} at {w}: {texts:?}"
            );
        }
    }
}

/// Left to right the cards read Position, Calibre, Listen, Rate,
/// Amplitude, Beat Error, at every width.
#[test]
fn the_cards_read_position_calibre_listen_then_the_readings() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let ctx = themed();
    for w in [800.0, 1000.0, 1610.0] {
        let mut xs = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(w, 800.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| app.panels(ctx));
            xs.clear();
            for c in &out.shapes {
                if let egui::Shape::Text(t) = &c.shape {
                    xs.push((t.galley.text().to_string(), t.pos.x));
                }
            }
        }
        let at = |caption: &str| {
            xs.iter()
                .find(|(t, _)| t == caption)
                .map(|&(_, x)| x)
                .unwrap_or_else(|| panic!("{caption} at {w}: {xs:?}"))
        };
        let position = if w > 1000.0 {
            "Current Position"
        } else {
            "Position"
        };
        // The Calibre card's title is its Watch line.
        let line = if w > 800.0 {
            format!("{NO_WATCH} ⏷")
        } else {
            format!("{NO_WATCH_SHORT} ⏷")
        };
        let order = [
            position,
            line.as_str(),
            "Listen",
            "Rate",
            timegrapher_core::steady_views::short_name("Amplitude")
                .filter(|_| w <= 800.0)
                .unwrap_or("Amplitude"),
            timegrapher_core::steady_views::short_name("Beat Error")
                .filter(|_| w <= 800.0)
                .unwrap_or("Beat Error"),
        ];
        for pair in order.windows(2) {
            assert!(at(pair[0]) < at(pair[1]), "{pair:?} at {w}");
        }
    }
}

/// Each key is the last digit of the hour at the top, U and D lying
/// (Peter's pick, #1473): every one pinned. The 45° stops' keys turn the
/// card to 10, and the six's leave it as it is.
#[test]
fn each_hour_key_picks_its_position_and_a_45_degree_one_turns_the_card_to_10() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let ctx = themed();
    use egui::Key;
    let keys = [
        (Key::U, Position::DialUp),
        (Key::D, Position::DialDown),
        (Key::Num3, Position::CrownUp),
        (Key::Num9, Position::CrownDown),
        (Key::Num6, Position::CrownLeft),
        (Key::Num2, Position::CrownRight),
        (Key::Num1, Position::Up1h30),
        (Key::Num4, Position::Up4h30),
        (Key::Num7, Position::Up7h30),
        (Key::Num0, Position::Up10h30),
    ];
    for (key, p) in keys {
        assert_eq!(p.key(), Some(key), "{p:?}");
    }
    for (key, p) in keys.into_iter().rev() {
        app.ten_positions = false;
        let press = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        };
        let _ = ctx.run(
            egui::RawInput {
                events: vec![press(true), press(false)],
                ..Default::default()
            },
            |ctx| app.panels(ctx),
        );
        assert_eq!(app.position, p, "{key:?}");
        assert_eq!(app.ten_positions, p.is_half_hour(), "{key:?} at 6");
    }
    // Keys 5 and 8 are no one's.
    for key in [Key::Num5, Key::Num8] {
        assert_eq!(Position::of_key(key), None);
    }
}

/// Walter's walk of #1194: with New Watch open and no field typed in,
/// the position keys change nothing behind the dialog.
#[test]
fn keys_one_to_six_wait_behind_a_dialog() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let ctx = themed();
    app.watch_form = Some(crate::watches::Form {
        watch: timegrapher_core::library::Watch::new("Test"),
        new: true,
        error: None,
    });
    let start = app.position;
    assert_ne!(start, Position::ALL[2]);
    let frame = |app: &mut TimegrapherApp, input| {
        let _ = ctx.run(input, |ctx| {
            app.panels(ctx);
            app.watch_form_dialog(ctx);
        });
    };
    frame(&mut app, screen());
    frame(&mut app, screen());
    frame(&mut app, key_frame(egui::Key::Num3, egui::Modifiers::NONE));
    assert!(app.watch_form.is_some(), "the dialog stays open");
    assert_eq!(
        app.position, start,
        "a key moved the watch behind the dialog"
    );
    // Closed, the key works again.
    app.watch_form = None;
    frame(&mut app, screen());
    frame(&mut app, screen());
    frame(&mut app, key_frame(egui::Key::Num3, egui::Modifiers::NONE));
    assert_eq!(app.position, Position::ALL[2]);
}

/// A key press, alone or with modifiers held, for one frame.
fn key_frame(key: egui::Key, modifiers: egui::Modifiers) -> egui::RawInput {
    let press = |pressed| egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    };
    egui::RawInput {
        modifiers,
        events: vec![press(true), press(false)],
        ..screen()
    }
}

/// Cmd+1 and Cmd+2 pick the view and never move the watch. The digits
/// alone still pick a position on the Bench, but not in Watches, nor
/// with a modifier held. The app opens on the Bench.
#[test]
fn cmd_1_and_2_pick_the_view_and_the_digits_alone_the_position() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let ctx = themed();
    let _ = ctx.run(screen(), |ctx| app.panels(ctx));
    assert_eq!(app.view, View::Bench);
    let start = app.position;
    assert_ne!(start, Position::ALL[2]);
    let press = |app: &mut TimegrapherApp, key, m| {
        let _ = ctx.run(key_frame(key, m), |ctx| app.panels(ctx));
    };
    press(&mut app, egui::Key::Num2, egui::Modifiers::COMMAND);
    assert_eq!((app.view, app.position), (View::Watches, start));
    press(&mut app, egui::Key::Num3, egui::Modifiers::NONE);
    assert_eq!(app.position, start, "no positions in Watches");
    press(&mut app, egui::Key::Num1, egui::Modifiers::COMMAND);
    assert_eq!((app.view, app.position), (View::Bench, start));
    for held in [
        egui::Modifiers::SHIFT,
        egui::Modifiers::ALT,
        egui::Modifiers::CTRL,
    ] {
        press(&mut app, egui::Key::Num3, held);
        assert_eq!((app.view, app.position), (View::Bench, start), "{held:?}");
        // Pressed and let go within the frame: only the key event
        // carries the modifier, the frame's are none.
        let quick = egui::RawInput {
            modifiers: egui::Modifiers::NONE,
            ..key_frame(egui::Key::Num3, held)
        };
        let _ = ctx.run(quick, |ctx| app.panels(ctx));
        assert_eq!(app.position, start, "{held:?} on the event only");
    }
    press(&mut app, egui::Key::Num3, egui::Modifiers::NONE);
    assert_eq!(app.position, Position::ALL[2]);
}

/// The top of the window is one row (#1000): the menus, the Bench |
/// Watches switch and the pane icons share it, and the readings start
/// right under it. In Watches the view's toolbar is at the top of its
/// card, under that row.
#[test]
fn the_menus_switch_and_icons_share_one_row() {
    let ctx = themed();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let at = |app: &mut TimegrapherApp| {
        let mut rects: Vec<(String, egui::Rect)> = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1440.0, 800.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| app.panels(ctx));
            rects = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some((
                        t.galley.text().to_string(),
                        t.galley.rect.translate(t.pos.to_vec2()),
                    )),
                    _ => None,
                })
                .collect();
        }
        rects
    };
    let find = |rects: &[(String, egui::Rect)], text: &str| {
        rects
            .iter()
            .find(|(t, _)| t == text)
            .map(|r| r.1)
            .unwrap_or_else(|| panic!("{text}: {rects:?}"))
    };
    let bench = at(&mut app);
    let file = find(&bench, "File");
    for t in ["Bench", "Watches", "Analysis", "Tests"] {
        let r = find(&bench, t);
        assert!(
            (r.center().y - file.center().y).abs() < 3.0,
            "{t}: {r:?} {file:?}"
        );
    }
    // The readings' titles sit just under the one row.
    let listen = find(&bench, "Listen");
    assert!(listen.top() < file.bottom() + 40.0, "{listen:?} {file:?}");
    app.view = View::Watches;
    let watches = at(&mut app);
    let file = find(&watches, "File");
    let sort = find(&watches, "Sort");
    assert!(sort.top() > file.bottom(), "{sort:?} {file:?}");
    assert!(sort.top() < file.bottom() + 40.0, "{sort:?} {file:?}");
}

/// A menu item and its shortcut run one command, in either menu bar:
/// Cmd+, opens Settings as Settings… does, and a dimmed item does
/// nothing, from its shortcut or chosen.
#[test]
fn a_menu_item_and_its_shortcut_run_one_command() {
    use super::menus::MenuCommand;
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let ctx = themed();
    let _ = ctx.run(screen(), |ctx| app.panels(ctx));
    assert_eq!(app.settings_tab, None);
    let _ = ctx.run(
        key_frame(egui::Key::Comma, egui::Modifiers::COMMAND),
        |ctx| app.panels(ctx),
    );
    assert!(app.settings_tab.is_some());
    // Nothing is masked: Remove All Masks… is dimmed and asks nothing.
    assert!(!app.menu_enabled(MenuCommand::RemoveMasks));
    app.run_menu_command(&ctx, MenuCommand::RemoveMasks);
    assert!(!app.remove_masks_asked);
    // Panes are the Bench's: in Watches their items are dimmed.
    app.view = View::Watches;
    let shown = app.show_readings;
    app.run_menu_command(&ctx, MenuCommand::Readings);
    assert_eq!(app.show_readings, shown);
    app.view = View::Bench;
    app.run_menu_command(&ctx, MenuCommand::Readings);
    assert_eq!(app.show_readings, !shown);
    assert_eq!(app.menu_checked(MenuCommand::Readings), !shown);
}

/// Unpinned, the readings are a strip: the position, the watch, the
/// take's length and the three figures, none of the cards' controls. A
/// click on it opens the full cards over the panes, which don't move;
/// Esc closes them. The pin puts the cards back in the layout, and is
/// kept; View ▸ Readings hides the strip too.
#[test]
fn the_readings_strip_opens_its_cards_over_the_panes() {
    let mut app = synthetic(40.0, 5.0);
    let ctx = themed();
    let pointer = |at: egui::Pos2, down: bool| {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut events = vec![egui::Event::PointerMoved(at)];
        if down {
            events.extend([button(true), button(false)]);
        }
        events
    };
    let pass = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        let input = egui::RawInput { events, ..screen() };
        let out = ctx.run(input, |ctx| app.panels(ctx));
        let strip_tools = theme::tools_drawn_at(&ctx)
            .into_iter()
            .find(|(k, _)| k.starts_with("Paper Strip/"))
            .map(|(_, r)| r);
        (super::tests::painted_text(&out), strip_tools)
    };
    pass(&mut app, Vec::new());
    let (pinned, at_pinned) = pass(&mut app, Vec::new());
    assert!(pinned.contains("Avg 10 s"), "{pinned}");
    // The pin, at the row's right end, unpins them.
    let pin = egui::pos2(1280.0 - 33.0, 66.0);
    pass(&mut app, pointer(pin, true));
    assert!(app.menu_checked(super::menus::MenuCommand::CompactReadings));
    pass(&mut app, pointer(pin, false));
    let (strip, at_closed) = pass(&mut app, Vec::new());
    for gone in ["Avg 10 s", "Lift"] {
        assert!(!strip.contains(gone), "{gone} in the strip: {strip}");
    }
    for shown in ["Dial Up", "Rate", "s/d", "ms", "52° · Auto 28800"] {
        assert!(strip.contains(shown), "{shown} not in the strip: {strip}");
    }
    let (closed, pinned_at) = (at_closed.unwrap(), at_pinned.unwrap());
    assert!(
        closed.top() < pinned_at.top(),
        "the strip is shorter than the cards"
    );
    // A click on the strip's Rate opens the cards over the panes.
    let rate = egui::pos2(closed.center().x, 70.0);
    pass(&mut app, pointer(rate, true));
    let (open, at_open) = pass(&mut app, pointer(rate, false));
    assert!(open.contains("Avg 10 s"), "{open}");
    assert_eq!(at_open, Some(closed), "the panes stay where they were");
    // Esc closes them.
    let esc = egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    pass(&mut app, vec![esc]);
    let (shut, _) = pass(&mut app, Vec::new());
    assert!(!shut.contains("Avg 10 s"), "{shut}");
    // The pin is kept.
    let mut back = TimegrapherApp::with_devices(Vec::new(), None);
    back.apply_settings(app.settings());
    assert!(!back.readings_pinned);
    // View ▸ Readings hides the strip.
    app.show_readings = false;
    pass(&mut app, Vec::new());
    let (hidden, _) = pass(&mut app, Vec::new());
    assert!(!hidden.contains("52° · Auto 28800"), "{hidden}");
}

/// Walter's walk of #1491: with the switch stored at 6 and a 45° stop
/// current, the compact card's Set offers all ten, the current one among
/// them, as the full card shows ten.
#[test]
fn the_compact_pop_up_offers_ten_while_a_45_degree_stop_is_current() {
    let ctx = themed();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let _ = window_texts(&mut app, &ctx, 1610.0);
    let _ = ctx.run(key_frame(egui::Key::Num4, egui::Modifiers::NONE), |ctx| {
        app.panels(ctx)
    });
    // The switch put back at 6 by hand: the 45° stop still holds ten.
    app.ten_positions = false;
    let run = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| app.panels(ctx));
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
    let _ = run(&mut app, Vec::new());
    let texts = run(&mut app, Vec::new());
    let set = texts
        .iter()
        .find(|(t, _)| t == "Set")
        .map(|(_, p)| *p)
        .expect("Set");
    let button = |pressed| egui::Event::PointerButton {
        pos: set,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let _ = run(
        &mut app,
        vec![egui::Event::PointerMoved(set), button(true), button(false)],
    );
    let _ = run(&mut app, Vec::new());
    // Each item inside the pop-up's clip, none scrolled out of view:
    // egui still emits a clipped item's text.
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(800.0, 600.0),
        )),
        ..Default::default()
    };
    let out = ctx.run(input, |ctx| app.panels(ctx));
    let items: Vec<(String, bool)> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) if t.galley.text().contains("· key") => {
                let r = t.galley.rect.translate(t.pos.to_vec2());
                Some((t.galley.text().to_string(), c.clip_rect.contains_rect(r)))
            }
            _ => None,
        })
        .collect();
    assert_eq!(items.len(), 10, "{items:?}");
    assert!(items.iter().all(|(_, seen)| *seen), "{items:?}");
    assert!(
        items.iter().any(|(t, _)| t == "4:30 Up (4H30) · key 4"),
        "{items:?}"
    );
}

/// With no watch, every width of the readings strip's watch cell keeps
/// the lift angle beside the beat rate, the narrowest too, the angle
/// first, and the narrowest the beat rate without "Auto " (Peter, #1526;
/// Hugh): a watchmaker checks both at a glance.
#[test]
fn the_strip_keeps_the_lift_beside_the_beat_rate_with_no_watch() {
    let app = synthetic(40.0, 5.0);
    assert!(app.open_watch().is_none());
    let (angle, _, bare) = app.watch_facts();
    for choice in app.watch_pieces() {
        let words: Vec<&str> = choice.iter().map(|(w, _, _)| w.as_str()).collect();
        let text = words.join(" ");
        let a = text.find(&angle).expect("the lift angle at every width");
        let b = text.find(&bare).expect("the beat rate at every width");
        assert!(a < b, "the angle comes first: {text}");
        assert!(
            choice.iter().all(|(_, size, _)| *size >= 13.0),
            "one type size: {text}"
        );
    }
    let narrowest = app.watch_pieces().pop().unwrap();
    assert_eq!(narrowest[1].0, bare, "the narrowest gives the rate alone");
}

/// The beat rate's two parts: "Auto" for a rate found by listening, then
/// the rate, which the strip writes alone where it is narrow (#1526).
#[test]
fn a_found_beat_rate_is_auto_then_the_rate() {
    let mut app = synthetic(40.0, 5.0);
    app.bph = None;
    let found = |rate: &str| (Some("Auto"), rate.to_string());
    assert_eq!(app.bph_parts(false, Some(28800)), found("28800"));
    assert_eq!(app.bph_shown(false, Some(28800)), "Auto 28800");
    assert_eq!(app.bph_parts(false, None), found(""));
    assert_eq!(app.bph_shown(false, None), "Auto");
    app.bph = Some(21600);
    let set = (None, "21600 bph".to_string());
    assert_eq!(app.bph_parts(false, Some(28800)), set);
    assert_eq!(app.bph_parts(true, Some(28800)), set);
}

/// With no watch, hovering the strip's watch cell shows one tooltip: the
/// calibre, lift angle and beat rate in full, then what a click does,
/// never the strip's tooltip on top of a second one (Hugh, #1526).
#[test]
fn the_strip_watch_cell_shows_one_tooltip() {
    let mut app = synthetic(40.0, 5.0);
    app.readings_pinned = false;
    let ctx = themed();
    // A second a frame, so a still pointer is past the hover's delay.
    let mut time = 0.0;
    let mut pass = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            events,
            time: Some(time),
            ..screen()
        };
        time += 1.0;
        let out = ctx.run(input, |ctx| app.panels(ctx));
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    pass(&mut app, Vec::new());
    let (angle, _, _) = app.watch_facts();
    let at = pass(&mut app, Vec::new())
        .into_iter()
        .find(|(t, _)| t.starts_with(&angle))
        .map(|(_, p)| p + egui::vec2(4.0, 6.0))
        .expect("the watch cell's lift angle");
    pass(&mut app, vec![egui::Event::PointerMoved(at)]);
    pass(&mut app, Vec::new());
    let shown = pass(&mut app, Vec::new());
    let tips: Vec<&String> = shown
        .iter()
        .map(|(t, _)| t)
        .filter(|t| t.contains("Open the readings' cards"))
        .collect();
    assert_eq!(tips.len(), 1, "one tooltip: {tips:?}");
    assert!(tips[0].contains(&app.watch_detail()), "{tips:?}");
    let details = shown
        .iter()
        .filter(|(t, _)| t.contains(&app.watch_detail()))
        .count();
    assert_eq!(details, 1, "the full facts once");
}
