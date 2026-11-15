//! The watch's wind on the Position card and in its takes (#1665).

use super::super::tests::{painted_text, queued_capture, save_failure_fixture};
use super::*;

/// An app with a library holding the watch `name`, picked, in `base`.
fn with_watch(tag: &str, name: &str) -> (TimegrapherApp, PathBuf) {
    let base = std::env::temp_dir().join(format!("tg-app-wind-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new(name)).unwrap();
    let id = lib.find(name).unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root));
    app.pick_watch(&id);
    (app, base)
}

/// The Set menu's Wind part sets the watch's full wind, kept with it in
/// Watches, and Not Given clears it: the card says so to a tenth.
#[test]
fn the_set_menu_sets_and_clears_the_watchs_wind() {
    let (mut app, base) = with_watch("set", "Pepsi");
    assert_eq!(app.wind_words(), "Wind not given");
    assert!(app.wind_settable());
    app.pick_wind(WindPick::HoursAgo(2.0));
    assert_eq!(app.wind_words(), "Full wind 2.0 h ago");
    let kept = app.open_watch().unwrap().watch.full_wind.clone();
    assert!(
        kept.as_deref().is_some_and(|t| t.ends_with('Z')),
        "{kept:?}"
    );
    // Kept in Watches: read again from the folder.
    let root = base.join("Timegrapher Data");
    let again = library::Library::read(&root).unwrap();
    assert_eq!(again.find("Pepsi").unwrap().watch.full_wind, kept);
    app.pick_wind(WindPick::Now);
    assert_eq!(app.wind_words(), "Full wind 0.0 h ago");
    app.pick_wind(WindPick::NotGiven);
    assert_eq!(app.wind_words(), "Wind not given");
    assert_eq!(app.open_watch().unwrap().watch.full_wind, None);
    let _ = std::fs::remove_dir_all(&base);
}

/// On a Quick Listen the wind lasts for the session, and picking a watch
/// lets it go for the watch's own.
#[test]
fn a_quick_listens_wind_lasts_until_a_watch_is_picked() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.pick_wind(WindPick::HoursAgo(5.0));
    assert_eq!(app.wind_words(), "Full wind 5.0 h ago");
    app.forget_quick_wind();
    assert_eq!(app.wind_words(), "Wind not given");
}

/// A take is saved with its hours from full wind to its start, to a
/// tenth, in every entry; with no wind given, with none.
#[test]
fn a_take_saves_its_hours_since_full_wind() {
    let (mut app, base) = save_failure_fixture("wind");
    app.pick_wind(WindPick::HoursAgo(3.04));
    let m = app.take_manifest();
    assert!(!m.recordings.is_empty());
    assert!(
        m.recordings.iter().all(|r| r.wind_h == Some(3.0)),
        "{:?}",
        m.recordings
    );
    app.pick_wind(WindPick::NotGiven);
    assert!(app
        .take_manifest()
        .recordings
        .iter()
        .all(|r| r.wind_h.is_none()));
    app.session.discard_recording();
    let _ = std::fs::remove_dir_all(base);
}

/// Start on the Positions Test's full-wind step, and on the Power Reserve
/// Test, sets Full Wind Now by itself (Peter, 14 Nov).
#[test]
fn start_at_full_wind_sets_full_wind_now() {
    let (mut app, base) = save_failure_fixture("wind-start");
    let _tx = queued_capture(&mut app, &base);
    app.pick_wind(WindPick::HoursAgo(30.0));
    app.start_guided(None);
    assert!(app.guided.is_some(), "{:?}", app.message);
    assert_eq!(app.wind_words(), "Full wind 0.0 h ago");
    // Its readings are saved at full wind.
    assert!(app
        .take_manifest()
        .recordings
        .iter()
        .all(|r| r.wind_h == Some(0.0)));
    app.session.discard_recording();
    let _ = std::fs::remove_dir_all(base);

    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.pick_wind(WindPick::HoursAgo(30.0));
    app.begin_reserve();
    assert_eq!(app.wind_words(), "Full wind 0.0 h ago");
}

/// A take on show, a reopened 24 hour half among them, shows its own wind
/// on the card, which can't be set there (Hugh); a take with none says
/// so; Set Wind gives it one, written into its session file.
#[test]
fn a_take_on_show_shows_its_own_wind() {
    let (mut app, base) = with_watch("shown", "Yacht-Master");
    app.pick_wind(WindPick::Now);
    let take = base.join("24h half");
    std::fs::create_dir_all(&take).unwrap();
    std::fs::write(
        take.join(MANIFEST),
        "[[recording]]\nfile = \".\"\nposition = \"DU\"\nwind_h = 24.2\n\n\
         [[recording]]\nfile = \".\"\nposition = \"DD\"\nwind_h = 24.2\n",
    )
    .unwrap();
    app.opened_path = take.display().to_string();
    app.session.analyse();
    assert_eq!(app.wind_words(), "24.2 h after full wind");
    assert!(!app.wind_settable(), "the Set menu's wind is closed");
    // On show, Set Wind is refused: its next save would undo it (Felix).
    let before = std::fs::read_to_string(take.join(MANIFEST)).unwrap();
    app.set_take_wind(&take, None);
    assert_eq!(
        std::fs::read_to_string(take.join(MANIFEST)).unwrap(),
        before
    );
    assert!(app
        .message
        .as_ref()
        .is_some_and(|(m, e)| *e && m.contains("on the Bench")));
    // Once it isn't, it is written into its session file.
    app.opened_path.clear();
    app.set_take_wind(&take, None);
    assert!(!std::fs::read_to_string(take.join(MANIFEST))
        .unwrap()
        .contains("wind_h"));
    app.set_take_wind(&take, Some(1.25));
    app.opened_path = take.display().to_string();
    assert_eq!(app.wind_words(), "1.3 h after full wind");
    let _ = std::fs::remove_dir_all(&base);
}

/// A Positions Test's heading names its hours after full wind only where
/// it was given one: never full wind for a wind not given (Miles).
#[test]
fn a_tests_heading_never_calls_an_unset_wind_full_wind() {
    assert_eq!(guided::test_title(None), "This Test");
    assert_eq!(
        guided::test_title(Some(24.24)),
        "This Test, 24.2 h After Full Wind"
    );
}

/// The card shows the wind whole, under the code on a compact card and
/// on its own row under the name and codes on a wide one, at 6 and at
/// 10, clear of the buttons (Hugh).
#[test]
fn the_card_shows_the_wind_whole() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.pick_wind(WindPick::HoursAgo(24.2));
    let want = "Full wind 24.2 h ago";
    for (w, compact, ten) in [
        (130.0, true, false),
        (POSITION_W, false, false),
        (POSITION_W, false, true),
        (330.0, false, false),
        (330.0, false, true),
    ] {
        app.ten_positions = ten;
        let mut out = None;
        let mut card = egui::Rect::NOTHING;
        for _ in 0..3 {
            // As tall as the row of cards is, taller at 10 once its title
            // row has been drawn.
            let high = app.readings_card_h(&ctx);
            card = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), Vec2::new(w, high));
            out = Some(ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let inner = card.shrink(f32::from(theme::CARD_PADDING));
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
                    app.position_card(&mut child, compact);
                });
            }));
        }
        let out = out.unwrap();
        let (t, clip) = out
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == want => Some((t, c.clip_rect)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{want} not drawn at {w}: {}", painted_text(&out)));
        let drawn = t.galley.rect.translate(t.pos.to_vec2());
        assert!(
            clip.contains_rect(drawn),
            "{w} {compact} {ten}: {drawn:?} in {clip:?}"
        );
        assert!(
            card.contains_rect(drawn),
            "{w} {compact} {ten}: {drawn:?} in {card:?}"
        );
        // On its own row: under every other line of the card's text above
        // the buttons that it would overlap, and over the buttons.
        if !compact {
            let rows = CardRows::of(card);
            // At 10, clear of them, and of the title row, by the gap
            // between the buttons' two rows (Hugh).
            let buttons = if ten {
                rows.controls.bottom()
                    - 2.0 * POSITION_ROW_TEN
                    - 2.0 * listen_card::POSITION_TEN_GAP
            } else {
                rows.controls.top()
            };
            if ten {
                title_clear(&out, app.position_card_title, w);
                clock_centred(&out, app.shown_position().name(), drawn, buttons);
            }
            assert!(
                drawn.bottom() <= buttons + 0.5,
                "{w} {ten}: {drawn:?} over {buttons}"
            );
            for c in &out.shapes {
                if let egui::Shape::Text(o) = &c.shape {
                    let r = o.galley.rect.translate(o.pos.to_vec2());
                    let over = r.x_range().intersects(drawn.x_range());
                    if o.galley.text() != want && over && r.top() < buttons - 0.5 {
                        assert!(
                            r.bottom() <= drawn.top() + 0.5,
                            "{w} {ten}: {r:?} {drawn:?}"
                        );
                    }
                }
            }
        }
    }
    // Beside the codes where it doesn't fit whole, and on a compact card
    // too narrow for it at 9 pt, it is in the hover alone: never cut,
    // wrapped or too small to read (Hugh).
    for (w, compact, ten) in [(150.0, false, true), (95.0, true, false)] {
        app.ten_positions = ten;
        let card = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), Vec2::new(w, 110.0));
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let inner = card.shrink(f32::from(theme::CARD_PADDING));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
                app.position_card(&mut child, compact);
            });
        });
        assert!(
            !out.shapes.iter().any(|c| matches!(&c.shape,
                egui::Shape::Text(t) if t.galley.text().starts_with("Full wind"))),
            "{w}: {}",
            painted_text(&out)
        );
    }
}

/// The 10 card's clock picture is centred on its name and wind lines,
/// not high beside them (Hugh, #1745).
fn clock_centred(out: &egui::FullOutput, name: &str, wind: egui::Rect, buttons: f32) {
    let name = out
        .shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::Shape::Text(t) if t.galley.text() == name => {
                Some(t.galley.rect.translate(t.pos.to_vec2()))
            }
            _ => None,
        })
        .expect("the position's name");
    // The picture: what is drawn left of the name, beside its lines.
    let pic = out
        .shapes
        .iter()
        .filter(|c| !matches!(c.shape, egui::Shape::Text(_)))
        .map(|c| c.shape.visual_bounding_rect())
        .filter(|r| r.is_positive() && r.right() < name.left() && r.top() > name.top() - 30.0)
        .filter(|r| r.bottom() < buttons)
        .fold(egui::Rect::NOTHING, |a, r| a.union(r));
    assert!(pic.is_positive(), "no picture left of {name:?}");
    let lines = (name.top() + wind.bottom()) / 2.0;
    assert!(
        (pic.center().y - lines).abs() <= 1.5,
        "picture {pic:?} not centred on {name:?}..{wind:?}"
    );
}

/// The 10 card's name, codes and wind start 3 pt under the title row as
/// drawn: under its 6 | 10 switch's frame, which runs past the row, and
/// its caption (Hugh, Walter's walk of #1745).
fn title_clear(out: &egui::FullOutput, caption: &str, w: f32) {
    let texts: Vec<(&str, egui::Rect)> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => {
                Some((t.galley.text(), t.galley.rect.translate(t.pos.to_vec2())))
            }
            _ => None,
        })
        .collect();
    let at = |name: &str| texts.iter().find(|t| t.0 == name).map(|t| t.1);
    let ten = at("10").expect("the 6 | 10 switch");
    // The switch's frame: the filled rects round its "10".
    let frame = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(r) if r.rect.contains_rect(ten) && r.rect.height() < 40.0 => {
                Some(r.rect)
            }
            _ => None,
        })
        .reduce(|a, b| a.union(b))
        .expect("the switch's frame");
    let title = frame.union(at(caption).expect("the caption"));
    for (text, r) in &texts {
        // The title row's own words, and the picture's tiny numerals.
        if ["6", "10", "?", caption].contains(text) || r.width() < 4.0 {
            continue;
        }
        assert!(
            r.top() >= title.bottom() + listen_card::POSITION_TEN_GAP - 0.5,
            "{w}: {text:?} {r:?} under {title:?}"
        );
    }
}

/// Too wide for its row at its size, the wind shrinks to fit, not under
/// 9 pt (Walter's walk of #1745): on a 6 card whose row is a little
/// narrower than the words.
#[test]
fn a_row_too_narrow_shrinks_the_wind() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    // Its fonts, for measuring.
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.pick_wind(WindPick::HoursAgo(24.2));
    let want = "Full wind 24.2 h ago";
    let at = |pt: f32| {
        ctx.fonts_mut(|f| {
            f.layout_no_wrap(
                want.into(),
                egui::FontId::proportional(pt),
                egui::Color32::WHITE,
            )
            .size()
            .x
        })
    };
    let (w11, w9) = (at(11.0), at(9.0));
    assert!(w9 + 4.0 < w11);
    app.ten_positions = false;
    // The card's padding, the picture and the gap beside it, and a row
    // between the words' two widths.
    let w = 2.0 * f32::from(theme::CARD_PADDING) + POSITION_PIC + theme::GAP + (w11 + w9) / 2.0;
    let card =
        egui::Rect::from_min_size(egui::pos2(20.0, 20.0), Vec2::new(w, theme::READINGS_CARD_H));
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let inner = card.shrink(f32::from(theme::CARD_PADDING));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            app.position_card(&mut child, false);
        });
    });
    let t = out
        .shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::Shape::Text(t) if t.galley.text() == want => Some(t.galley.rect.width()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{want} not drawn at {w}: {}", painted_text(&out)));
    assert!(t < w11 && t >= w9 - 0.5, "{t} between {w9} and {w11}");
}

/// While listening, each position button's code sits clear above the bar
/// filling under it, at 6 and at 10 (#1752, Hugh).
#[test]
fn a_buttons_code_clears_its_bar() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.live = Some(timegrapher_core::live::LiveAnalyzer::new(
        48_000,
        timegrapher_core::live::LiveConfig::default(),
    ));
    for ten in [false, true] {
        app.ten_positions = ten;
        let mut out = None;
        for _ in 0..3 {
            let high = app.readings_card_h(&ctx);
            let card = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), Vec2::new(330.0, high));
            out = Some(ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let inner = card.shrink(f32::from(theme::CARD_PADDING));
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
                    app.position_card(&mut child, false);
                });
            }));
        }
        let out = out.unwrap();
        // The bars' tracks: flat rects under 6 pt tall.
        let bars: Vec<egui::Rect> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Rect(r) if r.rect.height() < 6.0 && r.rect.width() > 10.0 => {
                    Some(r.rect)
                }
                _ => None,
            })
            .collect();
        let codes = if ten {
            &["DU", "CR", "1H30"][..]
        } else {
            &["DU", "CR"][..]
        };
        for code in codes {
            let drawn = out
                .shapes
                .iter()
                .find_map(|c| match &c.shape {
                    egui::Shape::Text(t) if t.galley.text() == *code => {
                        Some(t.galley.rect.translate(t.pos.to_vec2()))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{code} not drawn"));
            let bar = bars
                .iter()
                .find(|b| b.x_range().contains(drawn.center().x) && b.top() > drawn.top())
                .unwrap_or_else(|| panic!("{ten} {code}: no bar under {drawn:?} in {bars:?}"));
            // The glyphs' foot: the galley's, less its line's spare below.
            let foot = drawn.bottom() - 2.0;
            assert!(foot <= bar.top(), "{ten} {code}: {drawn:?} over {bar:?}");
        }
    }
}
