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
    const { assert!(wind::START_SETS_FULL_WIND) };
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
/// so; Set Wind… gives it one, written into its session file.
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
    assert_eq!(app.wind_words(), "Full wind 24.2 h before it began");
    assert!(!app.wind_settable(), "the Set menu's wind is closed");
    app.set_take_wind(&take, None);
    assert_eq!(app.wind_words(), "Wind not given");
    app.set_take_wind(&take, Some(1.25));
    assert_eq!(app.wind_words(), "Full wind 1.3 h before it began");
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
/// beside the codes on a wide one, at 6 and at 10.
#[test]
fn the_card_shows_the_wind_whole() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.pick_wind(WindPick::HoursAgo(24.2));
    let want = "Full wind 24.2 h ago";
    for (w, compact, ten) in [
        (130.0, true, false),
        (330.0, false, false),
        (330.0, false, true),
    ] {
        app.ten_positions = ten;
        let card = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), Vec2::new(w, 110.0));
        let mut out = None;
        for _ in 0..2 {
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
    }
    // Beside the codes where it doesn't fit whole, it is in the hover
    // alone: never shrunk, cut or wrapped (Hugh).
    app.ten_positions = true;
    let card = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), Vec2::new(150.0, 110.0));
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let inner = card.shrink(f32::from(theme::CARD_PADDING));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            app.position_card(&mut child, false);
        });
    });
    assert!(
        !out.shapes.iter().any(|c| matches!(&c.shape,
            egui::Shape::Text(t) if t.galley.text().starts_with("Full wind"))),
        "{}",
        painted_text(&out)
    );
}
