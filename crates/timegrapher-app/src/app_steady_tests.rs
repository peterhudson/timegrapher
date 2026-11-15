//! The Steadiness panes, headless: where the pane goes when moved, the
//! breaks it stops at, and the series each pane keeps.

use super::tests::synthetic;
use super::*;

#[test]
fn moving_steadiness_out_leaves_the_profile_showing() {
    let app = synthetic(40.0, 0.0);
    let mut tree = app.panes.clone();
    let steady = tree.tiles.find_pane(&Pane::SteadyReadings).unwrap();
    let sound = tree.tiles.find_pane(&Pane::Sound).unwrap();
    let tabs = tree.tiles.parent_of(steady).unwrap();
    // What a drag leaves behind: Steadiness active in the profile's
    // tab group but moved into a tab group of its own.
    if let Some(egui_tiles::Tile::Container(c)) = tree.tiles.get_mut(tabs) {
        c.remove_child(steady);
        if let egui_tiles::Container::Tabs(t) = c {
            t.active = Some(steady);
        }
    }
    let own = tree.tiles.insert_tab_tile(vec![steady]);
    let root = tree.root().unwrap();
    if let Some(egui_tiles::Tile::Container(c)) = tree.tiles.get_mut(root) {
        c.add_child(own);
    }
    repair_tabs(&mut tree);
    let Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(t))) = tree.tiles.get(tabs)
    else {
        panic!("the profile's tab group is gone");
    };
    assert_eq!(t.active, Some(sound));
}

#[test]
fn steadiness_stops_at_a_takes_breaks_with_their_gaps() {
    use timegrapher_core::breaks::Break;
    let log = BeatLog {
        sample_rate: 48000,
        duration_s: 600.0,
        bph: 28800,
        lift_deg: 52.0,
        beats: Vec::new(),
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: vec![200.0, 400.0],
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let cut = |audio_s, gap_s| Break { audio_s, gap_s };
    let breaks = |app: &TimegrapherApp| app.steady_log(log.clone(), &Default::default(), None).1;
    // Heard here: each jump, with no time.
    assert_eq!(breaks(&app), [cut(200.0, 0.0), cut(400.0, 0.0)]);
    // Opened: the take's own break keeps the 30 s its gap held, as
    // `series` reads it; the other jump has none.
    app.take_breaks = Some(vec![cut(200.0, 30.0)]);
    assert_eq!(breaks(&app), [cut(200.0, 30.0), cut(400.0, 0.0)]);
}

/// A take with two stretches in crown right, the second settling from
/// 337 s to 377 s (ym42_CR_long's third file): the app reads them as one
/// run with the settling a hole in it (#1156).
fn two_entries_in_one_position() -> TimegrapherApp {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.show_log(
        super::masks_tests::long_log(0.25),
        "two entries".into(),
        vec![(0.0, Position::CrownRight), (337.0, Position::CrownRight)],
        vec![(0.0, 20.0), (337.0, 377.0)],
    );
    app
}

/// The steadiness tests and the distributions' readings leave a hole in
/// a run out and stop at both its ends, as `series` does, rather than
/// joining the run on either side of it (#1156).
#[test]
fn steadiness_leaves_a_hole_out_and_stops_at_its_ends() {
    use timegrapher_core::breaks::Break;
    let app = two_entries_in_one_position();
    let runs = app.current_runs();
    assert_eq!(runs.holes, [(337.0, 377.0)]);
    let log = steady::log_of(app.live.as_ref().unwrap()).unwrap();
    let (joined, breaks) = app.steady_log(log, &runs, None);
    // Joined from 20 s, the run's start: the hole is 317 s to 357 s.
    assert!(
        !joined
            .beats
            .iter()
            .any(|b| b.time > 317.0 && b.time < 357.0),
        "a beat in the hole is read"
    );
    for at in [317.0, 357.0] {
        assert!(
            breaks.contains(&Break {
                audio_s: at,
                gap_s: 0.0
            }),
            "no cut at {at}: {breaks:?}"
        );
    }
}

/// The distributions' 2 s values take no window in a hole in a run
/// (#1156).
#[test]
fn the_distributions_take_no_window_in_a_hole() {
    let app = two_entries_in_one_position();
    let windows = app.position_windows();
    assert!(!windows.is_empty());
    assert!(
        windows
            .iter()
            .all(|w| w.end_s <= 337.0 || w.start_s >= 377.0),
        "a window in the hole is counted"
    );
}

/// Each Steadiness pane's series come back next time, Rate alone for a
/// pane not set and for every series off, and the one choice an older
/// app kept for its single Steadiness pane given to every view, unless
/// that was all three, which older apps wrote whatever was chosen.
#[test]
fn each_steady_panes_series_are_kept() {
    use steady::View;
    let mut app = synthetic(5.0, 0.0);
    app.steady_series.insert(View::Fft, [false, true, false]);
    let s = app.settings();
    let mut back = TimegrapherApp::with_devices(Vec::new(), None);
    back.apply_settings(s.clone());
    assert_eq!(back.steady_series[&View::Fft], [false, true, false]);
    assert_eq!(back.series_shown(View::Cycle), RATE_ONLY);
    let mut none = s;
    none.steady_shown = vec![(View::Fft, [false; 3])];
    back.apply_settings(none);
    assert_eq!(back.series_shown(View::Fft), RATE_ONLY);
    let mut all = app.settings();
    all.steady_shown.clear();
    all.steady_series = [true; 3];
    back.apply_settings(all);
    assert_eq!(back.series_shown(View::Cycle), RATE_ONLY);
    let mut older = app.settings();
    older.steady_shown.clear();
    older.steady_series = [true, false, false];
    back.apply_settings(older);
    for p in Pane::STEADY {
        assert_eq!(
            back.steady_series[&p.steady_view().unwrap()],
            [true, false, false]
        );
    }
}

/// The FFT pane's Timing changes only how rate is drawn, so it can be
/// used only while the pane draws rate: with Rate off it is greyed and a
/// click does nothing.
#[test]
fn the_fft_timing_switch_needs_rate_shown() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = synthetic(40.0, 0.0);
    app.reveal_pane(Pane::SteadyFft);
    let pass = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1600.0, 1000.0),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.panels(ctx));
        theme::tools_drawn_at(&ctx)
            .into_iter()
            .find(|(k, _)| k == "FFT/Timing")
            .map(|(_, r)| r)
    };
    let click = |at: egui::Pos2| {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![egui::Event::PointerMoved(at), button(true), button(false)]
    };
    for (series, flips) in [(RATE_ONLY, true), ([false, true, false], false)] {
        app.steady_series.insert(steady::View::Fft, series);
        app.fft_timing = false;
        pass(&mut app, Vec::new());
        let at = pass(&mut app, Vec::new()).expect("Timing drawn").center();
        pass(&mut app, click(at));
        pass(&mut app, Vec::new());
        assert_eq!(app.fft_timing, flips, "{series:?}");
    }
}

/// Outside a toolbar, as in the "…" menu, a dimmed switch still says
/// why in its hover, on its name and on the switch.
#[test]
fn a_dimmed_menu_switch_says_why_in_its_hover() {
    let ctx = super::tests::themed();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(300.0, 200.0));
    let why = "Turn Rate on to draw it as the beat timing's spectrum.";
    let mut on = false;
    let mut row = egui::Rect::NOTHING;
    // A second a frame, so a still pointer is past the hover's delay.
    let mut time = 0.0;
    let mut pass = |events: Vec<egui::Event>, on: &mut bool, row: &mut egui::Rect| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            time: Some(time),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let drawn = ui.scope(|ui| {
                    let clicked = theme::tool_switch_unless(
                        ui,
                        Icon::Stopwatch,
                        egui::Color32::WHITE,
                        "Timing",
                        on,
                        "The hint",
                        Some(why),
                    );
                    assert!(!clicked);
                });
                *row = drawn.response.rect;
            });
        });
        time += 1.0;
        super::tests::painted_text(&out)
    };
    for _ in 0..2 {
        pass(Vec::new(), &mut on, &mut row);
    }
    // The name at the row's left, then the switch at its right.
    for at in [
        egui::pos2(row.left() + 8.0, row.center().y),
        egui::pos2(row.right() - 8.0, row.center().y),
    ] {
        pass(vec![egui::Event::PointerMoved(at)], &mut on, &mut row);
        pass(Vec::new(), &mut on, &mut row);
        let hovered = pass(Vec::new(), &mut on, &mut row);
        assert!(hovered.contains(why), "{at:?}: {hovered}");
        assert!(!hovered.contains("The hint"), "{hovered}");
    }
    assert!(!on);
}

/// A switch in a pane's toolbar changes that pane only: Wheel Periods
/// clicked on FFT leaves Periods as it was, and each pane's own choice
/// comes back next time. Settings from before each pane kept its own
/// give every pane the one old choice.
#[test]
fn a_panes_wheel_periods_switch_changes_that_pane_only() {
    use steady::View;
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = synthetic(40.0, 0.0);
    app.reveal_pane(Pane::SteadyPeriods);
    app.reveal_pane(Pane::SteadyFft);
    let pass = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1600.0, 1000.0),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.panels(ctx));
        theme::tools_drawn_at(&ctx)
            .into_iter()
            .find(|(k, _)| k == "FFT/Wheel Periods")
            .map(|(_, r)| r)
    };
    let (fft, periods) = (
        app.show_marks.wheels.on(View::Fft),
        app.show_marks.wheels.on(View::Periods),
    );
    pass(&mut app, Vec::new());
    let at = pass(&mut app, Vec::new())
        .expect("Wheel Periods drawn on FFT")
        .center();
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    pass(
        &mut app,
        vec![egui::Event::PointerMoved(at), button(true), button(false)],
    );
    pass(&mut app, Vec::new());
    assert_eq!(app.show_marks.wheels.on(View::Fft), !fft);
    assert_eq!(app.show_marks.wheels.on(View::Periods), periods);
    // Only the pane switched is noted: Periods, drawn too, keeps the
    // default, as do the panes' Info and dB Scale.
    assert_eq!(app.show_marks.wheels.saved(), vec![(View::Fft, !fft)]);
    assert!(app.steady_info.saved().is_empty() && app.maps_db.saved().is_empty());
    // Mesh is its own switch: Wheel Periods leaves it as it was (#1764).
    assert!(app.show_marks.meshes.saved().is_empty());
    app.show_marks.meshes.set(View::Stft, false);
    app.maps_db.set(View::Wavelet, true);
    app.steady_info.set(View::Cusum, true);

    let mut back = TimegrapherApp::with_devices(Vec::new(), None);
    back.apply_settings(app.settings());
    for (switch, on, off) in [
        (&back.show_marks.wheels, View::Periods, View::Fft),
        (&back.maps_db, View::Wavelet, View::Stft),
        (&back.steady_info, View::Cusum, View::Allan),
        (&back.show_marks.meshes, View::Wavelet, View::Stft),
    ] {
        assert!(switch.on(on) && !switch.on(off), "{switch:?}");
    }

    let mut older = app.settings();
    older.marks.show_wheels = false;
    older.marks.show_wheels_set.clear();
    back.apply_settings(older);
    for p in Pane::STEADY {
        assert!(
            !back.show_marks.wheels.on(p.steady_view().unwrap()),
            "{p:?}"
        );
    }
    // Settings kept before Mesh had a switch mark the meshes, as the app
    // did then, and keep their Wheel Periods as they were.
    let mut v = serde_json::to_value(app.settings()).unwrap();
    let kept = v.as_object_mut().unwrap();
    kept.remove("show_meshes");
    kept.remove("show_meshes_set");
    let s: crate::settings::Settings = serde_json::from_value(v).unwrap();
    assert!(s.marks.show_meshes && s.marks.show_meshes_set.is_empty());
    assert_eq!(s.marks.show_wheels_set, vec![(View::Fft, !fft)]);
}
