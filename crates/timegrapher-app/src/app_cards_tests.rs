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
                    fit = stacked(ui, [true; 3], STACKED_MIN_H, |ui, k| {
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
    for p in Position::ALL {
        match p.lying() {
            Lying::Hour(h) => assert_eq!(format!("{h}H"), p.witschi()),
            Lying::DialUp => assert_eq!(p, Position::DialUp),
            Lying::DialDown => assert_eq!(p, Position::DialDown),
        }
    }
}

/// Every pane has one place in the icon bar's groups: the core panes
/// with Watches first, Analysis (the charts, the distributions, the
/// Steadiness views and Isochronism) and Tests. At 1000 points every
/// icon fits; narrower, the largest group folds into a menu, and the
/// Tests still show.
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
    // At 1000 points every icon fits, with the Charts and
    // Distributions groups gone into Analysis.
    let mid = window_texts(&mut app, &ctx, 1000.0);
    assert!(mid.iter().any(|t| t == "Analysis"), "{mid:?}");
    assert!(mid.iter().any(|t| t == "Tests"), "{mid:?}");
    // No group folded (the Watch line's ⏷ aside).
    let folded = |t: &String| t.contains('⏷') && !t.starts_with(NO_WATCH_SHORT);
    assert!(!mid.iter().any(folded), "{mid:?}");
    let narrow = window_texts(&mut app, &ctx, 760.0);
    assert!(
        narrow
            .iter()
            .any(|t| t.starts_with("Analysis") && t.contains('⏷')),
        "{narrow:?}"
    );
    assert!(narrow.iter().any(|t| t == "Tests"), "{narrow:?}");
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
        // Listen goes back to the strip as it was, unless it was
        // changed by hand since.
        assert_eq!(
            (app.live_view().span_s, app.live_view().half_width_ms),
            (10.0, 1.0)
        );
        app.strip.set_half_width(5.0);
        assert_eq!(app.live_view(), app.strip);
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
        // keep their captions whole beside it.
        wants.extend(["0:00", "Rate", "Amplitude", "Beat Error"]);
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
            assert!(
                texts.iter().any(|t| t == caption),
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
            "Amplitude",
            "Beat Error",
        ];
        for pair in order.windows(2) {
            assert!(at(pair[0]) < at(pair[1]), "{pair:?} at {w}");
        }
    }
}

/// Keys 1 to 6 put the watch in each position in turn, as the
/// Position card lists them; not while a field is typed in.
#[test]
fn keys_one_to_six_pick_the_position() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let ctx = themed();
    let keys = [
        egui::Key::Num1,
        egui::Key::Num2,
        egui::Key::Num3,
        egui::Key::Num4,
        egui::Key::Num5,
        egui::Key::Num6,
    ];
    for (n, key) in keys.into_iter().enumerate().rev() {
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
        assert_eq!(app.position, Position::ALL[n], "key {}", n + 1);
    }
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
