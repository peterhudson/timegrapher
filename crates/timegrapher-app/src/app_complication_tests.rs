//! The Complication Test and its pane (#1576): set up in the position the
//! card shows, led through Off, On, Off, and drawn with core's On less Off
//! and findings.

use super::tests::{queued_capture, synthetic_swinging, themed};
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

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

/// A Complication Test saved with its take and the take opened again in
/// a fresh app (#1620): the session file says which stretch is Off and
/// which On, the stretches are read again from the sound, and the pane
/// shows the table and core's On less Off as at the end of the run.
#[test]
fn a_saved_complication_take_opened_again_shows_its_results() {
    use timegrapher_core::complication;
    use timegrapher_core::steadiness::SeriesKind;
    let base =
        std::env::temp_dir().join(format!("tg-app-complication-reopen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let on = |t: f64| (T0 + EACH..T0 + 2.0 * EACH).contains(&t);
    let amp = move |t: f64| if on(t) { 260.0 } else { 280.0 };
    let extra = move |t: f64| if on(t) { -10.0 } else { 0.0 };
    let mut app = synthetic_swinging(0.0, 4.0, |_| 280.0, |_| 0.0);
    app.session.listen();
    app.session.recorder =
        Some(Recorder::start(&base.join("live"), app.info("test", 48000, 16)).unwrap());
    let cfg = SynthConfig {
        duration_s: T0 + 3.0 * EACH + 5.0,
        rate_s_per_day: 4.0,
        ..Default::default()
    };
    let audio = generate(&cfg, amp, extra);
    // A second at a time, each stamped, so the clock log can be read.
    for (k, s) in audio.samples.chunks(48_000).enumerate() {
        let block = timegrapher_core::capture::Block {
            samples: s.to_vec(),
            at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + k as u64),
        };
        app.session
            .recorder
            .as_mut()
            .unwrap()
            .write(&block)
            .unwrap();
        let live = app.live.as_mut().unwrap();
        for b in s.chunks(960) {
            live.push(b);
        }
    }
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
    app.guided_tick(&egui::Context::default());
    let before = app.complication_test().expect("finished");
    let first: Vec<_> = before
        .readings
        .as_ref()
        .unwrap()
        .iter()
        .map(|r| r.measurement.clone())
        .collect();
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    let saved = app.session.saved_to().expect("saved").to_path_buf();
    drop(tx);
    let text = std::fs::read_to_string(saved.join(MANIFEST)).unwrap();
    assert_eq!(text.matches("complication = \"off\"").count(), 2, "{text}");
    assert_eq!(text.matches("complication = \"on\"").count(), 1, "{text}");

    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.start_batch(&saved);
    let clock = Instant::now();
    while again.batch.is_some() && clock.elapsed().as_secs() < 300 {
        again.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(again.live.is_some(), "{:?}", again.message);
    assert!(again.guided.is_none(), "no test on the Bench");
    let drawn = draw_at(&mut again, Vec2::new(1280.0, 900.0));
    let texts: Vec<&str> = drawn.iter().map(|(t, _)| t.as_str()).collect();
    assert!(texts.contains(&"On less Off"), "{texts:?}");
    assert!(
        texts
            .iter()
            .any(|t| t.contains("lower with the complication on")),
        "{texts:?}"
    );
    // The same stretches read the same way as at the end of the run.
    let kept = again.complication_saved.as_ref().expect("its test");
    let back: Vec<_> = kept
        .readings
        .as_ref()
        .unwrap()
        .iter()
        .map(|r| r.measurement.clone())
        .collect();
    for q in [SeriesKind::Amplitude, SeriesKind::Rate] {
        let (a, b) = (
            complication::change(q, &first).unwrap().on_less_off.value,
            complication::change(q, &back).unwrap().on_less_off.value,
        );
        assert!((a - b).abs() < 0.05, "{q:?}: {a} then {b}");
    }
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

/// The watch page's New Take offers every take, each with its own
/// caption (#1634): the Complication Test's says off, on, off, and the
/// Power Reserve Test is still offered. The tests that were there keep
/// their order; the Complication Test comes last.
#[test]
fn new_take_offers_every_take_with_its_own_caption() {
    let order: Vec<NewTake> = NEW_TAKES.iter().map(|t| t.0).collect();
    assert_eq!(
        order,
        [
            NewTake::Listen,
            NewTake::Positions,
            NewTake::Poise,
            NewTake::PowerReserve,
            NewTake::Complication
        ]
    );
    let caption = |k: NewTake| NEW_TAKES.iter().find(|t| t.0 == k).map(|t| t.3);
    assert_eq!(
        caption(NewTake::Complication),
        Some("Off, on, then off again")
    );
    assert_eq!(
        caption(NewTake::PowerReserve),
        Some("From full wind to a stop")
    );
    let names: Vec<(&str, &str, &str)> = NEW_TAKES.iter().map(|t| (t.1, t.2, t.3)).collect();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            watches::new_take_buttons(ui, &names, None);
        });
    });
    let drawn = format!("{:?}", out.shapes);
    for (name, _, caption) in &names {
        assert!(drawn.contains(name), "{name}");
        assert!(drawn.contains(caption), "{caption}");
    }
}

/// Which of New Take's buttons a click on `name` presses, with the page
/// `width` wide.
fn new_take_pressed(width: f32, name: &str) -> Option<usize> {
    let names: Vec<(&str, &str, &str)> = NEW_TAKES.iter().map(|t| (t.1, t.2, t.3)).collect();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 600.0));
    let mut pressed = None;
    let mut frame = |events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let spacing = ui.spacing().item_spacing;
                pressed = pressed.or(watches::new_take_buttons(ui, &names, None));
                assert_eq!(ui.spacing().item_spacing, spacing, "the page's own spacing");
            });
        })
    };
    let out = frame(Vec::new());
    let at = out
        .shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::Shape::Text(t) if t.galley.text() == name => {
                Some(t.visual_bounding_rect().center())
            }
            _ => None,
        })
        .expect("the button is drawn");
    let click = |down| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: down,
        modifiers: egui::Modifiers::NONE,
    };
    frame(vec![egui::Event::PointerMoved(at)]);
    frame(vec![click(true)]);
    frame(vec![click(false)]);
    pressed
}

/// A click on New Take's last button starts the last take, whether the
/// five sit in one row or wrap (#1634), and New Take leaves the page's
/// spacing as it found it.
#[test]
fn new_takes_last_button_starts_the_last_take_in_one_row_or_more() {
    let last = NEW_TAKES.len() - 1;
    for width in [1400.0, 420.0] {
        assert_eq!(
            new_take_pressed(width, NEW_TAKES[last].1),
            Some(last),
            "{width}"
        );
    }
    assert_eq!(new_take_pressed(420.0, NEW_TAKES[1].1), Some(1));
}

/// New Take's names and captions are whole at every width: in one row
/// at 1280, three then two at 1000, and at 800, where the watch's side
/// is narrowest and "Power Reserve Test" is too wide for a third, two to
/// a row (#1634).
#[test]
fn new_take_rows_keep_every_name_and_caption_whole() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let names = new_take_names();
    // The window less the list, its gap and the margins.
    let side = |window: f32| window - WATCH_LIST_W - 17.0 - 2.0 * 12.0 - 16.0;
    let tile = |window: f32| move |n: usize| (side(window) - (n as f32 - 1.0) * 10.0) / n as f32;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            assert_eq!(watches::new_take_row(ui, &names, tile(1280.0)), 5, "1280");
            assert_eq!(watches::new_take_row(ui, &names, tile(800.0)), 2, "800");
            // Wide enough for three, not five: three then two.
            assert_eq!(watches::new_take_row(ui, &names, tile(1000.0)), 3, "1000");
            assert!(watches::new_take_fits(ui, &names, tile(800.0)(2)));
            assert!(
                !watches::new_take_fits(ui, &names, tile(800.0)(3)),
                "a third"
            );
        });
    });
}
