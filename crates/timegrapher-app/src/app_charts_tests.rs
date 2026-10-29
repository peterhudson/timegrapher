//! The Charts and Distributions panes, headless: what each plots and
//! where its values come from.

use super::tests::{painted_text, synthetic, themed};
use super::*;

/// A reading's beat error is core's (`beat_error_signed`): from the
/// unlock where it was timed, else from the drop, so a stretch with only
/// a drop still charts.
#[test]
fn a_readings_beat_error_falls_back_to_the_drop() {
    let point = |unlock, drop| TrendPoint {
        t: 0.0,
        rate: None,
        amplitude: None,
        amplitude_a: None,
        amplitude_b: None,
        beat_error_unlock: unlock,
        beat_error_drop: drop,
        heard: true,
    };
    assert_eq!(point(Some(-0.4), Some(0.5)).beat_error(), Some(-0.4));
    assert_eq!(point(None, Some(0.5)).beat_error(), Some(0.5));
    assert_eq!(point(None, None).beat_error(), None);
}

/// The rate distribution's 2 s values are the rate over each 2 seconds
/// of beats, none across a break or into a masked stretch.
#[test]
fn rate_distribution_counts_two_second_stretches() {
    let mut app = synthetic(30.0, 12.0);
    let live = app.live.as_mut().unwrap();
    live.set_breaks(vec![15.0]);
    let mut r = RateTwoS::default();
    r.refresh(live, &[(5.0, 7.5)]);
    assert!(r.values.len() >= 10, "{}", r.values.len());
    for &(a, v) in &r.values {
        let b = a + RateTwoS::STEP_S;
        assert!(!(a < 15.0 && 15.0 < b), "across the break at {a}");
        assert!(!(a < 7.5 && b > 5.0), "into the mask at {a}");
        assert!((v - 12.0).abs() < 20.0, "{v} s/day at {a}");
    }
    // Refreshing again adds nothing; a change of mask starts again.
    let n = r.values.len();
    r.refresh(live, &[(5.0, 7.5)]);
    assert_eq!(r.values.len(), n);
    r.refresh(live, &[]);
    assert!(r.values.len() > n);
    // The pane follows Values for rate too.
    app.hist_readings = false;
    let ctx = egui::Context::default();
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.histogram(ui, SeriesKind::Rate));
    });
    assert!(!app.rate_2s.values.is_empty());
}

/// The Charts and Distributions panes draw a plot for each reading
/// turned on, and its switches keep at least one on.
#[test]
fn the_charts_and_distributions_draw_the_readings_turned_on() {
    let mut app = synthetic(30.0, 20.0);
    for p in [Pane::Charts, Pane::Distributions] {
        set_pane_visible(&mut app.panes.tiles, p, true);
    }
    // Room for every plot: the strip, the profile and the Scope out of
    // the way.
    for p in [Pane::Strip, Pane::Sound, Pane::Scope] {
        set_pane_visible(&mut app.panes.tiles, p, false);
    }
    let ctx = themed();
    // Each pane in turn: they are tabs side by side, one on show.
    let draw = |app: &mut TimegrapherApp| {
        let mut all = String::new();
        for p in [Pane::Charts, Pane::Distributions] {
            for q in [Pane::Charts, Pane::Distributions] {
                set_pane_visible(&mut app.panes.tiles, q, q == p);
            }
            let mut text = String::new();
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1440.0, 1000.0),
                    )),
                    ..Default::default()
                };
                text = painted_text(&ctx.run(input, |ctx| app.panels(ctx)));
            }
            all.push_str(&text);
        }
        all
    };
    // At first only the rate: no amplitude or beat error plot in
    // either pane.
    assert_eq!((app.chart_series, app.dist_series), (RATE_ONLY, RATE_ONLY));
    let text = draw(&mut app);
    assert!(text.contains("Rate"), "{text}");
    for t in ["Beat Error:", "Amplitude:", "From the Unlock", "From Tick"] {
        assert!(!text.contains(t), "{t}: {text}");
    }
    // All three turned on.
    app.chart_series = [true; 3];
    app.dist_series = [true; 3];
    let text = draw(&mut app);
    for t in [
        "Charts",
        "Distributions",
        "s/day",
        "Beat Error:",
        "Amplitude:",
    ] {
        assert!(text.contains(t), "{t}: {text}");
    }
    assert!(!text.contains("Match Strip"), "{text}");
    app.chart_series = RATE_ONLY;
    // The last one on stays on; another comes on beside it.
    steady::flip_series(&mut app.chart_series, 0);
    assert_eq!(app.chart_series, [true, false, false]);
    steady::flip_series(&mut app.chart_series, 2);
    assert_eq!(app.chart_series, [true, false, true]);
    steady::flip_series(&mut app.chart_series, 0);
    assert_eq!(app.chart_series, [false, false, true]);
}
