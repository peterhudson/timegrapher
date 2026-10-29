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
    let breaks = |app: &TimegrapherApp| app.steady_log(log.clone(), &[], None).1;
    // Heard here: each jump, with no time.
    assert_eq!(breaks(&app), [cut(200.0, 0.0), cut(400.0, 0.0)]);
    // Opened: the take's own break keeps the 30 s its gap held, as
    // `series` reads it; the other jump has none.
    app.take_breaks = Some(vec![cut(200.0, 30.0)]);
    assert_eq!(breaks(&app), [cut(200.0, 30.0), cut(400.0, 0.0)]);
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
