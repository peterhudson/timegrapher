//! The Complication Test and its pane (#1576): set up in the position the
//! card shows, led through Off, On, Off, and drawn with core's On less Off
//! and findings.

use super::tests::{queued_capture, synthetic_swinging, themed};
use super::*;

/// The Watches view's Complication Test sets the take up on the Bench in
/// the card's position: its pane shows, and nothing starts until Start
/// Test.
#[test]
fn a_complication_test_is_set_up_in_the_cards_position() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.view = View::Watches;
    app.set_position(Position::CrownUp);
    assert!(!pane_visible(&app.panes.tiles, Pane::Complication));
    app.new_take(NewTake::Complication, false);
    assert_eq!(app.view, View::Bench);
    assert_eq!(app.ready_test(), Some(NewTake::Complication));
    assert!(pane_visible(&app.panes.tiles, Pane::Complication));
    assert_eq!(app.position, Position::CrownUp, "the card stays");
    assert!(app.guided.is_none() && !app.running());
    assert_eq!(NewTake::Complication.name(), "Complication Test");
}

/// The test's steps are Off, On, Off in the one position, named so on
/// its buttons and chips.
#[test]
fn the_steps_are_off_on_off_in_one_position() {
    let t = guided::Test::complication(Position::DialUp);
    assert_eq!(t.order(), [Position::DialUp; 3]);
    let codes: Vec<&str> = (0..3).map(|i| t.step_code(i)).collect();
    assert_eq!(codes, ["Off", "On", "Off"]);
    assert_eq!(t.step_name(1), "Complication On");
    assert!(
        t.kept(&guided::About::default()).is_none(),
        "no 24 hour half"
    );
}

/// Each stretch's length on a synthetic take, seconds, and when the run
/// starts.
const T0: f64 = 5.0;
const EACH: f64 = 62.0;
const SETTLE: f64 = 2.0;

/// A Complication Test run through its three stretches on a synthetic
/// take whose amplitude drops 20° and rate 10 s/d while the complication
/// is on; with its capture folder and channel.
fn measured(name: &str) -> (TimegrapherApp, std::path::PathBuf, impl Sized) {
    let base =
        std::env::temp_dir().join(format!("tg-app-complication-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let on = |t: f64| (T0 + EACH..T0 + 2.0 * EACH).contains(&t);
    let amp = move |t: f64| if on(t) { 260.0 } else { 280.0 };
    let extra = move |t: f64| if on(t) { -10.0 } else { 0.0 };
    let mut app = synthetic_swinging(T0 + 3.0 * EACH + 5.0, 4.0, amp, extra);
    app.session.listen();
    let tx = queued_capture(&mut app, &base);
    let mut test = guided::Test::complication(app.position).timed(SETTLE, EACH - SETTLE);
    let mut t = T0;
    test.tick(t);
    for i in 0..3 {
        t += EACH;
        assert_eq!(test.tick(t), guided::Phase::Turn, "stretch {i}");
        if i < 2 {
            assert!(test.advance(t).is_some());
        }
    }
    app.guided = Some(test);
    assert!(app.test_listening());
    let ctx = egui::Context::default();
    app.guided_tick(&ctx);
    (app, base, tx)
}

/// A run measures Off, On, Off and ends by itself. Core's On less Off
/// finds the drop the take was made with, and the Copy text gives the
/// table and the findings.
#[test]
fn a_complication_test_finds_the_drop_with_it_on() {
    use timegrapher_core::complication;
    use timegrapher_core::steadiness::SeriesKind;
    let (app, base, tx) = measured("run");
    assert!(!app.guided_running(), "ended after the third");
    let kept = app.complication_test().expect("the table stays");
    let readings = kept.readings.as_ref().unwrap();
    assert_eq!(readings.len(), 3);
    let m: Vec<_> = readings.iter().map(|r| r.measurement.clone()).collect();
    let amp = complication::change(SeriesKind::Amplitude, &m).unwrap();
    assert!(
        (amp.on_less_off.value + 20.0).abs() < 3.0,
        "{:?}",
        amp.on_less_off
    );
    assert!(complication::differs(amp.on_less_off));
    let rate = complication::change(SeriesKind::Rate, &m).unwrap();
    assert!(
        (rate.on_less_off.value + 10.0).abs() < 1.5,
        "{:?}",
        rate.on_less_off
    );
    let text = guided::export_text(kept, &app.guided_about(), '\t');
    assert!(
        text.contains("Stretch\tAmplitude (°)\tRate (s/d)\tBeat Error (ms)"),
        "{text}"
    );
    assert!(
        text.contains("\nOff\t") && text.contains("\nOn\t"),
        "{text}"
    );
    assert!(text.contains("\nOn less Off\t−"), "{text}");
    assert!(
        text.contains("\nAmplitude: 20 ± 1° lower with the complication on\n"),
        "{text}"
    );
    // The synthetic take's rate has no ±, so it gets no verdict either
    // way: the figure, and why (Hugh).
    assert!(
        text.contains("\nRate: −10.0 s/d, no ± yet (too few readings)\n"),
        "{text}"
    );
    drop(tx);
    let _ = std::fs::remove_dir_all(&base);
}

/// The pane after a run: the table's On less Off row, the three plots'
/// titles with core's findings, amplitude first, and the track naming the
/// stretches. The titles are the names' last showing, after the table's
/// headings.
#[test]
fn the_pane_draws_the_table_and_three_plots_amplitude_first() {
    let (mut app, base, tx) = measured("pane");
    let drawn = draw_at(&mut app, Vec2::new(1280.0, 900.0));
    let texts: Vec<&str> = drawn.iter().map(|(t, _)| t.as_str()).collect();
    assert!(texts.contains(&"On less Off"), "{texts:?}");
    let title = |name: &str| {
        drawn
            .iter()
            .rfind(|(t, _)| t == name)
            .map(|(_, r)| r.top())
            .unwrap_or_else(|| panic!("{name} in {texts:?}"))
    };
    let (a, r, b) = (title("Amplitude"), title("Rate"), title("Beat Error"));
    assert!(a < r && r < b, "amplitude first: {a} {r} {b}");
    assert!(
        texts
            .iter()
            .any(|t| t.contains("lower with the complication on")),
        "{texts:?}"
    );
    assert_eq!(
        texts.iter().filter(|t| **t == "Off").count(),
        4,
        "{texts:?}"
    );
    drop(tx);
    let _ = std::fs::remove_dir_all(&base);
}

/// Between stretches the banner says to turn the complication on, and
/// its button starts the On measurement.
#[test]
fn the_banner_says_to_turn_the_complication_on() {
    let mut app = synthetic_swinging(30.0, 0.0, |_| 280.0, |_| 0.0);
    let mut test = guided::Test::complication(app.position).timed(1.0, 5.0);
    test.tick(0.0);
    assert_eq!(test.tick(10.0), guided::Phase::Turn);
    app.guided = Some(test);
    let drawn = draw_at(&mut app, Vec2::new(1280.0, 900.0));
    let texts: Vec<&str> = drawn.iter().map(|(t, _)| t.as_str()).collect();
    assert!(texts.contains(&"Turn the complication on"), "{texts:?}");
    assert!(texts.contains(&"Start On Measurement"), "{texts:?}");
}

/// The pane drawn twice at `size`: each text and where it is.
fn draw_at(app: &mut TimegrapherApp, size: Vec2) -> Vec<(String, egui::Rect)> {
    let ctx = themed();
    let mut drawn = Vec::new();
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.pane_tools(ui, Pane::Complication);
                app.complication_pane(ui);
            });
        });
        drawn = out
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
    drawn
}
