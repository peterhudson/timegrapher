//! The watch's wind on the Listen card and in its takes (#1665, #1915).

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

/// The Wind menu sets the watch's full wind, kept with it in Watches,
/// and Not Given clears it: the field says so to a tenth.
#[test]
fn the_set_menu_sets_and_clears_the_watchs_wind() {
    let (mut app, base) = with_watch("set", "Pepsi");
    assert_eq!(app.wind_field().words.value, "Not given");
    assert!(app.wind_field().settable);
    app.pick_wind(WindPick::HoursAgo(2.0));
    assert_eq!(app.wind_field().words.value, "Full 2.0 h ago");
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
    assert_eq!(app.wind_field().words.value, "Full 0.0 h ago");
    app.pick_wind(WindPick::NotGiven);
    assert_eq!(app.wind_field().words.value, "Not given");
    assert_eq!(app.open_watch().unwrap().watch.full_wind, None);
    let _ = std::fs::remove_dir_all(&base);
}

/// On a Quick Listen the wind lasts for the session, and picking a watch
/// lets it go for the watch's own.
#[test]
fn a_quick_listens_wind_lasts_until_a_watch_is_picked() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.pick_wind(WindPick::HoursAgo(5.0));
    assert_eq!(app.wind_field().words.value, "Full 5.0 h ago");
    app.forget_quick_wind();
    assert_eq!(app.wind_field().words.value, "Not given");
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
    // Fixed from the take's start, in the Wind field (#1915).
    assert_eq!(app.wind_field().words.value, "Began 0.0 h after full");
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
    assert_eq!(app.wind_field().words.value, "Full 0.0 h ago");
}

/// A take on show, a reopened 24 hour half among them, shows its own wind
/// in the Wind field, which can't be set there (Hugh); a take with none says
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
    assert_eq!(app.wind_field().words.value, "Began 24.2 h after full");
    assert!(!app.wind_field().settable, "the Wind menu is closed");
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
    assert_eq!(app.wind_field().words.value, "Began 1.3 h after full");
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

/// Draw the Position card, `w` wide and as tall as the row, three times
/// so a 10 card's title row is measured first.
fn position_card_out(
    app: &mut TimegrapherApp,
    ctx: &egui::Context,
    w: f32,
    compact: bool,
) -> (egui::FullOutput, egui::Rect) {
    let mut out = None;
    let mut card = egui::Rect::NOTHING;
    for _ in 0..3 {
        let high = app.readings_card_h(ctx);
        card = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), Vec2::new(w, high));
        out = Some(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let inner = card.shrink(f32::from(theme::CARD_PADDING));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
                app.position_card(&mut child, compact);
            });
        }));
    }
    (out.unwrap(), card)
}

/// The Position card shows the position alone, as on 24 Oct, at every
/// width, at 6 and at 10: the wind is the Listen card's (#1915). At 10
/// the name clears the title row and the picture is centred on it.
#[test]
fn the_position_card_shows_no_wind() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    for wind in [Some(WindPick::HoursAgo(24.2)), Some(WindPick::NotGiven)] {
        app.pick_wind(wind.unwrap());
        for (w, compact, ten) in [
            (130.0, true, false),
            (130.0, true, true),
            (POSITION_W, false, false),
            (POSITION_W, false, true),
            (330.0, false, false),
            (330.0, false, true),
        ] {
            app.ten_positions = ten;
            let (out, card) = position_card_out(&mut app, &ctx, w, compact);
            let words = painted_text(&out);
            assert!(
                !words.contains("wind") && !words.contains("Wind") && !words.contains("given"),
                "{w} {compact} {ten}: {words}"
            );
            if ten && !compact {
                let rows = CardRows::of(card);
                let buttons = rows.controls.bottom()
                    - 2.0 * POSITION_ROW_TEN
                    - 2.0 * listen_card::POSITION_TEN_GAP;
                title_clear(&out, app.position_card_title, w);
                clock_centred(&out, app.shown_position().name(), buttons);
            }
        }
    }
}

/// The 10 card's clock picture is centred on its name line, not high
/// beside it (Hugh, #1745).
fn clock_centred(out: &egui::FullOutput, name: &str, buttons: f32) {
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
    // The picture: what is drawn left of the name, beside its line.
    let pic = out
        .shapes
        .iter()
        .filter(|c| !matches!(c.shape, egui::Shape::Text(_)))
        .map(|c| c.shape.visual_bounding_rect())
        .filter(|r| r.is_positive() && r.right() < name.left() && r.top() > name.top() - 30.0)
        .filter(|r| r.bottom() < buttons)
        .fold(egui::Rect::NOTHING, |a, r| a.union(r));
    assert!(pic.is_positive(), "no picture left of {name:?}");
    assert!(
        (pic.center().y - name.center().y).abs() <= 1.5,
        "picture {pic:?} not centred on {name:?}"
    );
}

/// The 10 card's name and codes start 3 pt under the title row as
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

/// Draw the Listen card's rows, `w` wide, in a card as tall as the row.
fn listen_card_out(
    app: &mut TimegrapherApp,
    ctx: &egui::Context,
    w: f32,
    compact: bool,
) -> (egui::FullOutput, egui::Rect) {
    let card = egui::Rect::from_min_size(
        egui::pos2(20.0, 20.0),
        Vec2::new(w, app.readings_card_h(ctx)),
    );
    let rows = CardRows::of(card);
    let mut out = None;
    // Twice, as egui sizes a box from its last frame.
    for _ in 0..2 {
        out = Some(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let area = rows.figure.union(rows.controls);
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
                app.listen_buttons(&mut child, compact);
            });
        }));
    }
    (out.unwrap(), card)
}

/// Where `text` is drawn in `out`.
fn drawn(out: &egui::FullOutput, text: &str) -> Option<egui::Rect> {
    out.shapes.iter().find_map(|c| match &c.shape {
        egui::Shape::Text(t) if t.galley.text() == text => {
            Some(t.galley.rect.translate(t.pos.to_vec2()))
        }
        _ => None,
    })
}

/// The Listen card's Average Over and Wind share its foot row, the card
/// no taller (Hugh on #1915), in the widest form that fits whole: never
/// cut or set smaller, stepping down as the card narrows, before a take
/// and from its start alike.
#[test]
fn the_listen_card_shows_the_wind_beside_average_over() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.pick_wind(WindPick::HoursAgo(24.2));
    assert_eq!(
        app.readings_card_h(&ctx),
        theme::READINGS_CARD_H,
        "no taller"
    );
    let live = || {
        timegrapher_core::live::LiveAnalyzer::new(
            48_000,
            timegrapher_core::live::LiveConfig::default(),
        )
    };
    let mut forms = Vec::new();
    for begun in [false, true] {
        app.live = begun.then(live);
        let words = app.wind_field().words;
        for (w, compact) in [
            (500.0, false),
            (330.0, false),
            (LISTEN_W, false),
            (150.0, true),
        ] {
            let (out, card) = listen_card_out(&mut app, &ctx, w, compact);
            let all = painted_text(&out);
            let inner = card.shrink(f32::from(theme::CARD_PADDING));
            let avg_now = fields::duration(app.average_s);
            let form = listen_card::FIELD_FORMS
                .iter()
                .copied()
                .find(|f| {
                    drawn(&out, &f.wind(&words)).is_some()
                        && drawn(&out, &f.average(&avg_now)).is_some()
                })
                .unwrap_or_else(|| panic!("{w} {begun}: no Wind box in {all}"));
            let avg = drawn(&out, &form.average(&fields::duration(app.average_s)))
                .unwrap_or_else(|| panic!("{w}: no Average Over box in {all}"));
            let wind = drawn(&out, &form.wind(&words)).unwrap();
            for r in [avg, wind] {
                assert!(
                    inner.contains_rect(r),
                    "{w} {begun}: {r:?} cut by {inner:?}"
                );
            }
            assert!(
                (avg.center().y - wind.center().y).abs() < 1.0,
                "{w}: one row"
            );
            assert!(wind.left() > avg.right(), "{w}: Wind after Average Over");
            if form.labels {
                assert!(drawn(&out, "Average Over").is_some(), "{w}: {all}");
            }
            forms.push((w, begun, form));
        }
    }
    // The widest card says it all beside its labels; the forms step down.
    for begun in [false, true] {
        let mine: Vec<listen_card::FieldForm> =
            forms.iter().filter(|f| f.1 == begun).map(|f| f.2).collect();
        assert_eq!(mine[0], listen_card::FIELD_FORMS[0], "{begun}");
        let at = |f: &listen_card::FieldForm| {
            listen_card::FIELD_FORMS
                .iter()
                .position(|g| g == f)
                .unwrap()
        };
        assert!(mine.windows(2).all(|p| at(&p[0]) <= at(&p[1])), "{mine:?}");
    }
}

/// The window at `w` x `h`: the Wind box's form as drawn, and where.
fn window_wind(app: &mut TimegrapherApp, w: f32, h: f32) -> (listen_card::FieldForm, egui::Rect) {
    let ctx = super::super::tests::themed();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h));
    let mut out = None;
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        out = Some(ctx.run(input, |ctx| app.panels(ctx)));
    }
    let out = out.unwrap();
    assert_eq!(
        app.readings_card_h(&ctx),
        theme::READINGS_CARD_H,
        "no taller"
    );
    let words = app.wind_field().words;
    let avg_now = fields::duration(app.average_s);
    listen_card::FIELD_FORMS
        .iter()
        .find_map(|f| {
            let wind = drawn(&out, &f.wind(&words))?;
            let avg = drawn(&out, &f.average(&avg_now))?;
            assert!((wind.center().y - avg.center().y).abs() < 1.0, "one row");
            Some((*f, wind))
        })
        .unwrap_or_else(|| panic!("{w}: no Wind box: {}", painted_text(&out)))
}

/// The cards' row keeps its height in every window (Hugh on #1915); the
/// Listen card widens for the Wind box in words where the reading cards
/// keep their full width: its second form at 1920 x 1080, its third at
/// 1600 x 900; at 1280 x 800
/// and 800 x 560, where they can't, it keeps today's width.
#[test]
fn the_listen_card_widens_for_the_wind_where_there_is_room() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.pick_wind(WindPick::HoursAgo(24.2));
    let forms = &listen_card::FIELD_FORMS;
    assert_eq!(window_wind(&mut app, 1920.0, 1080.0).0, forms[1]);
    assert_eq!(window_wind(&mut app, 1600.0, 900.0).0, forms[2]);
    // From a take's start, the second form still fits at 1920 (Walter and
    // Hugh on #1926): "Wind: 54.1 h at start".
    app.pick_wind(WindPick::HoursAgo(54.1));
    app.live = Some(timegrapher_core::live::LiveAnalyzer::new(
        48_000,
        timegrapher_core::live::LiveConfig::default(),
    ));
    assert_eq!(window_wind(&mut app, 1920.0, 1080.0).0, forms[1]);
    app.live = None;
    for (w, h) in [(1280.0, 800.0), (800.0, 560.0)] {
        let (form, _) = window_wind(&mut app, w, h);
        assert!(
            !form.labels && form.wind == listen_card::WindForm::Name,
            "{w}: {form:?}"
        );
    }
}

/// A wider Listen card never takes a reading card under its full width
/// (Hugh on #1915).
#[test]
fn a_wider_listen_card_leaves_the_readings_whole() {
    let mut seen = [false; 2];
    for total in (600..2600).step_by(10) {
        let w = readouts::six_widths(total as f32, theme::GAP);
        if w[1] > LISTEN_W {
            assert!(w[2] >= READING_FULL_W, "{total}: {w:?}");
            seen[usize::from(w[1] == readouts::LISTEN_WIDE_W)] = true;
        }
    }
    assert_eq!(seen, [true; 2], "both wider forms are used");
}

/// Hovering a box in `out` at `text`: what the window then says.
fn hovered_at(app: &mut TimegrapherApp, ctx: &egui::Context, text: &str, w: f32) -> String {
    let (out, _) = listen_card_out(app, ctx, w, true);
    let at = drawn(&out, text).unwrap_or_else(|| panic!("{text}: {}", painted_text(&out)));
    let pass = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let card = egui::Rect::from_min_size(
                    egui::pos2(20.0, 20.0),
                    Vec2::new(w, app.readings_card_h(ctx)),
                );
                let rows = CardRows::of(card);
                let area = rows.figure.union(rows.controls);
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
                app.listen_buttons(&mut child, true);
            });
        })
    };
    pass(app, vec![egui::Event::PointerMoved(at.center())]);
    pass(app, Vec::new());
    painted_text(&pass(app, Vec::new()))
}

/// The compact card's boxes have no arrows, so their hovers say a click
/// opens them (Hugh on #1915).
#[test]
fn the_compact_cards_boxes_say_a_click_opens_them() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    ctx.style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.pick_wind(WindPick::HoursAgo(2.0));
    let w = readouts::six_widths(600.0, theme::GAP)[1];
    let wind = hovered_at(&mut app, &ctx, "Wind", w);
    assert!(wind.contains("Click to set the wind."), "{wind}");
    let avg = hovered_at(&mut app, &ctx, "10 s", w);
    assert!(
        avg.contains("Average Over: 10 s. Click to change."),
        "{avg}"
    );
}

/// From a take's start the Wind field is fixed at the hours from full
/// wind to its start, and can't be set until the next take (#1915).
#[test]
fn the_wind_is_fixed_from_a_takes_start() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.pick_wind(WindPick::HoursAgo(2.0));
    let before = app.wind_field();
    assert_eq!(before.words.value, "Full 2.0 h ago");
    assert!(before.settable && before.given);
    assert!(
        before.hover.starts_with("Full wind 2.0 h ago. "),
        "{}",
        before.hover
    );
    app.live = Some(timegrapher_core::live::LiveAnalyzer::new(
        48_000,
        timegrapher_core::live::LiveConfig::default(),
    ));
    let during = app.wind_field();
    assert_eq!(during.words.value, "Began 2.0 h after full");
    assert!(!during.settable);
    assert!(
        during
            .hover
            .starts_with("Full wind 2.0 h before it began. "),
        "{}",
        during.hover
    );
    app.live = None;
    app.pick_wind(WindPick::NotGiven);
    let none = app.wind_field();
    assert_eq!(none.words.value, "Not given");
    assert!(!none.given, "muted");
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
