//! The Dynamic Poise test and its pane (#1474): set up at 9H with the
//! card at 10, led through the eight upright stops, kept out of the
//! Positions Test's pane, and drawn with core's fit.

use super::poise_pane::PoiseView;
use super::tests::{queued_capture, synthetic_varying, themed};
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

/// The Watches view's Dynamic Poise Test sets the take up on the Bench:
/// its pane shows, the card turns to 10 at 9H, and nothing starts until
/// Start Test.
#[test]
fn a_dynamic_poise_test_is_set_up_at_9h_with_the_card_at_10() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.view = View::Watches;
    assert!(!pane_visible(&app.panes.tiles, Pane::DynamicPoise));
    assert!(!app.ten_positions);
    app.new_take(NewTake::Poise, false);
    assert_eq!(app.view, View::Bench);
    assert_eq!(app.ready_test(), Some(NewTake::Poise));
    assert!(pane_visible(&app.panes.tiles, Pane::DynamicPoise));
    assert_eq!(app.position, Position::CrownDown);
    assert_eq!(app.position.witschi(), "9H");
    assert!(
        app.ten_positions,
        "Start was pressed: the card shows the eight"
    );
    assert!(app.guided.is_none() && !app.running());
    assert_eq!(NewTake::Poise.name(), "Dynamic Poise Test");
}

/// The test turns the watch 45° at a time, the same way round, through
/// the eight upright stops, and names them by the hour at the top.
#[test]
fn the_poise_order_is_the_eight_upright_stops_45_degrees_apart() {
    let order = guided::Kind::Poise.order();
    let hours: Vec<&str> = order.iter().map(|p| p.witschi()).collect();
    assert_eq!(
        hours,
        ["9H", "7H30", "6H", "4H30", "3H", "1H30", "12H", "10H30"]
    );
    for w in order.windows(2) {
        let (a, b) = (w[0].core().top_deg(), w[1].core().top_deg());
        let step = (a.unwrap() - b.unwrap()).rem_euclid(360.0);
        assert_eq!(step, 45.0, "{:?} to {:?}", w[0], w[1]);
    }
    let core: Vec<_> = order.iter().map(|p| p.core()).collect();
    assert_eq!(core, core_session::Position::UPRIGHT, "core's order");
}

/// A rate that goes once round as the watch turns, fastest with 6H at the
/// top: the eight stops are measured, the test ends by itself after the
/// eighth, core's fit names 6H, and the Positions Test's pane and its
/// 24 hour results take no part in it.
fn a_poise_test_measures_eight_stops_and_core_fits_the_wave(app: &TimegrapherApp) {
    assert!(!app.guided_running(), "ended after the eighth");
    let kept = app.poise_test().expect("the table stays");
    let readings = kept.readings.as_ref().unwrap();
    assert_eq!(readings.len(), 8);
    // The Mean row is over all eight, the 45° stops too.
    let table = guided::table(readings, guided::Kind::Poise);
    let rates: Vec<f64> = table.rows.iter().filter_map(|r| r.1).collect();
    assert_eq!(rates.len(), 8);
    let mean = rates.iter().sum::<f64>() / 8.0;
    assert!(
        (table.mean_rate.unwrap() - mean).abs() < 1e-9,
        "{:?}",
        table.mean_rate
    );
    assert!(kept.kept(&app.guided_about()).is_none(), "no 24 hour half");
    let fit = guided::poise_fit(readings).expect("fitted from eight");
    assert_eq!(fit.positions, 8);
    assert!(fit.wave_clear, "{}", fit.text);
    assert!(
        fit.short_text.contains("fastest near 6H at the top"),
        "{}",
        fit.short_text
    );
    assert!((fit.wave.size.value - 30.0).abs() < 3.0, "{}", fit.text);
    // Copy: the table in the pane's order, Mean, no Largest Difference,
    // then core's sentence.
    let text = guided::export_text(kept, &app.guided_about(), '\t');
    assert!(text.contains("Crown Down (CD, 9H)\t"), "{text}");
    assert!(
        text.contains("\nMean\t") && text.contains("From Mean (s/d)"),
        "{text}"
    );
    assert!(!text.contains("Largest Difference") && !text.contains("Vertical less"));
    assert!(text.trim_end().ends_with(&fit.text), "{text}");
}

/// The measured Dynamic Poise test's checks, each of them a test of its
/// own until #1757, on one measured take: measuring it was nearly all of
/// each one's time, about 12 s. Each check starts from the pane's view
/// and Info as measured; Save as Text…, which takes the app, runs last.
#[test]
fn a_measured_poise_test_passes_each_check() {
    let (mut app, base, tx) = measured("measured-checks");
    let fresh = (app.poise_views, app.poise_info);
    let reset = |app: &mut TimegrapherApp| {
        app.poise_views = fresh.0;
        app.poise_info = fresh.1;
    };
    a_poise_test_measures_eight_stops_and_core_fits_the_wave(&app);
    the_picture_has_its_key_and_mean_and_info_says_each_fact_once(&mut app);
    reset(&mut app);
    the_heavy_spot_is_in_a_facts_box_and_the_notes_in_info(&mut app);
    reset(&mut app);
    the_table_tightens_beside_the_picture_then_goes_under_it(&mut app);
    reset(&mut app);
    both_views_sit_side_by_side_where_there_is_room(&mut app);
    reset(&mut app);
    only_the_table_scrolls_and_a_short_pane_scrolls_the_picture(&mut app);
    reset(&mut app);
    the_dial_has_its_watch_case_behind_it(&mut app);
    reset(&mut app);
    save_as_text_writes_the_poise_tests_report(app, &base);
    drop(tx);
    let _ = std::fs::remove_dir_all(&base);
}

/// The checks of a take of the eight upright stops saved and opened
/// again, each of them a test of its own until #1757, on one such take:
/// saving and opening it was nearly all of each one's time, about 13 s.
/// Save as Text…, which takes the app, runs last.
#[test]
fn a_saved_and_opened_poise_take_passes_each_check() {
    let (mut app, base) = saved_and_opened("saved-checks", guided::Kind::Poise.order());
    a_saved_poise_take_opened_again_shows_its_stops_and_fit(&mut app);
    a_reopened_poise_takes_copy_copies_the_table_it_shows(&mut app);
    save_as_text_writes_an_opened_poise_takes_report(app, &base);
    let _ = std::fs::remove_dir_all(&base);
}

/// A Dynamic Poise test run through its eight stops on a synthetic take,
/// 62 s each, whose rate goes 30 s/d each way once round, fastest at 6H
/// at the top; with its capture folder and channel.
fn measured(name: &str) -> (TimegrapherApp, std::path::PathBuf, impl Sized) {
    let base = std::env::temp_dir().join(format!("tg-app-poise-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let (t0, each, settle) = (5.0, 62.0, 2.0);
    let order = guided::Kind::Poise.order();
    // 30 s/d each way, fastest at 180° (6H at the top).
    let extra = move |t: f64| {
        let k = ((t - t0) / each).floor();
        if !(0.0..8.0).contains(&k) {
            return 0.0;
        }
        let deg = order[k as usize].core().top_deg().unwrap();
        30.0 * (deg - 180.0).to_radians().cos()
    };
    let mut app = synthetic_varying(t0 + 8.0 * each + 5.0, 4.0, extra);
    app.session.listen();
    let tx = queued_capture(&mut app, &base);
    let mut test = guided::Test::poise().timed(settle, each - settle);
    let mut t = t0;
    test.tick(t);
    for i in 0..order.len() {
        t += each;
        assert_eq!(test.tick(t), guided::Phase::Turn, "stop {i}");
        if i + 1 < order.len() {
            assert_eq!(test.advance(t), Some(order[i + 1]));
        }
    }
    app.guided = Some(test);
    assert!(app.test_listening());
    assert!(app.positions_test().is_none(), "not the Positions Test's");
    let ctx = egui::Context::default();
    app.guided_tick(&ctx);
    (app, base, tx)
}

/// A take still being listened to, with `stops` marked by hand in turn,
/// `each` s apart, its rate 30 s/d each way round the dial (fastest with
/// 6H at the top, as `measured`, and no change dial up or down), its
/// sound kept; and its folder.
fn listened(name: &str, stops: &[Position]) -> (TimegrapherApp, std::path::PathBuf) {
    listened_each(name, stops, 62.0)
}

/// [`listened`] with stops `each` seconds long.
fn listened_each(
    name: &str,
    stops: &[Position],
    each: f64,
) -> (TimegrapherApp, std::path::PathBuf) {
    let base = std::env::temp_dir().join(format!("tg-app-poise-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let tops: Vec<Option<f64>> = stops.iter().map(|p| p.core().top_deg()).collect();
    let extra = move |t: f64| {
        let top = tops[((t / each).floor() as usize).min(tops.len() - 1)];
        top.map_or(0.0, |deg| 30.0 * (deg - 180.0).to_radians().cos())
    };
    let mut app = synthetic_varying(0.0, 0.0, |_| 0.0);
    app.session.listen();
    app.session.recorder =
        Some(Recorder::start(&base.join("live"), app.info("test", 48000, 16)).unwrap());
    let cfg = SynthConfig {
        duration_s: each * stops.len() as f64,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, extra);
    let per_stop = (each * 48_000.0) as usize;
    let mut second = 1_700_000_000;
    for (p, sound) in stops.iter().zip(audio.samples.chunks(per_stop)) {
        app.session.set_state(session::State::Listening);
        app.set_position(*p);
        // A second at a time, each stamped, so the clock log can be read.
        for s in sound.chunks(48_000) {
            let block = timegrapher_core::capture::Block {
                samples: s.to_vec(),
                at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(second),
            };
            second += 1;
            let r = app.session.recorder.as_mut().unwrap();
            r.write(&block).unwrap();
            let live = app.live.as_mut().unwrap();
            for b in s.chunks(960) {
                live.push(b);
            }
        }
    }
    (app, base)
}

/// [`listened`], saved, then opened in a fresh app as a saved take is
/// opened (#1619); and the folder to remove.
pub(super) fn saved_and_opened(
    name: &str,
    stops: &[Position],
) -> (TimegrapherApp, std::path::PathBuf) {
    saved_and_opened_each(name, stops, 62.0)
}

/// [`saved_and_opened`] with stops `each` seconds long.
fn saved_and_opened_each(
    name: &str,
    stops: &[Position],
    each: f64,
) -> (TimegrapherApp, std::path::PathBuf) {
    let (mut app, base) = listened_each(name, stops, each);
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    let saved = app.session.saved_to().expect("saved").to_path_buf();
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.start_batch(&saved);
    let t = Instant::now();
    while again.batch.is_some() && t.elapsed().as_secs() < 300 {
        again.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(again.live.is_some(), "{:?}", again.message);
    assert!(again.guided.is_none(), "no test on the Bench");
    (again, base)
}

/// A take of the eight upright stops, saved and opened again (#1619):
/// the pane reads each stop from the take's saved marks, as the
/// Positions pane reads its six, so the table and core's fit come back.
fn a_saved_poise_take_opened_again_shows_its_stops_and_fit(app: &mut TimegrapherApp) {
    let t = app.free_table(&egui::Context::default()).expect("a table");
    let readings = &t.upright.0;
    assert_eq!(readings.len(), 8, "{t:?}");
    // The synthetic take has no noise, so core states no ± and no
    // fastest hour; the wave it fits is the one put in.
    let fit = guided::poise_fit(readings).expect("fitted from eight");
    assert!((fit.wave.size.value - 30.0).abs() < 1.0, "{}", fit.text);
    assert!(
        (fit.wave.peak_deg.value - 180.0).abs() < 3.0,
        "{}",
        fit.text
    );
    // The six positions' own table is as it was: only its six.
    assert_eq!(t.rows.len(), 6);
    let drawn = draw(app, Pane::DynamicPoise);
    for label in ["Crown Down (CD, 9H)", "7:30 Up (7H30)", "10:30 Up (10H30)"] {
        assert!(drawn.contains(label), "{label}: {drawn}");
    }
    assert!(drawn.contains(&fit.short_text), "{drawn}");
    assert!(!drawn.contains(&poise_pending()), "{drawn}");
}

/// A reopened Dynamic Poise take's Copy copies the rows its table
/// shows: it is not greyed out as "Nothing measured yet", and the text
/// has the eight stops and the fit (#1657, from an external audit).
fn a_reopened_poise_takes_copy_copies_the_table_it_shows(app: &mut TimegrapherApp) {
    let t = app.free_table(&egui::Context::default()).expect("a table");
    let fit = guided::poise_fit(&t.upright.0).expect("fitted from eight");
    let text = guided::kept_rows_text(&t.upright.0, guided::Kind::Poise, &app.guided_about(), '\t');
    for label in ["Crown Down (CD, 9H)", "7:30 Up (7H30)", "10:30 Up (10H30)"] {
        assert!(text.contains(label), "{label}: {text}");
    }
    assert!(text.contains(&fit.text), "{text}");
    let size = Vec2::new(1280.0, 820.0);
    let drawn = draw_at(app, Pane::DynamicPoise, size);
    let (_, copy) = drawn
        .iter()
        .find(|(t, _)| t == "Copy")
        .unwrap_or_else(|| panic!("Copy in {drawn:?}"));
    let copied = click(app, Pane::DynamicPoise, size, copy.center());
    assert_eq!(
        copied.as_deref(),
        Some(text.as_str()),
        "what the table shows"
    );
    let said = app.message.as_ref().map(|(m, _)| m.as_str());
    assert_eq!(said, Some("Copied the Dynamic Poise table"));
}

/// A saved take of only the four vertical positions, as a Positions Test
/// leaves them, opened again: the pane shows its four and says what the
/// fit lacks, not that it waits for a stop that won't come (Hugh on
/// #1622).
#[test]
fn a_saved_take_of_four_stops_says_it_needs_five() {
    let four = [
        Position::CrownDown,
        Position::CrownLeft,
        Position::CrownUp,
        Position::CrownRight,
    ];
    let (mut app, base) = saved_and_opened("four", &four);
    let t = app.free_table(&egui::Context::default()).expect("a table");
    assert_eq!(t.upright.0.len(), 4, "{t:?}");
    let drawn = draw(&mut app, Pane::DynamicPoise);
    let lacks = timegrapher_core::poise::unfitted_text(4, true);
    assert_eq!(lacks, "Needs 5 upright positions to fit; this take has 4");
    assert!(drawn.contains(&lacks), "{drawn}");
    assert!(!drawn.contains(&poise_pending()), "{drawn}");
    assert!(drawn.contains("Crown Left (CL, 6H)"), "{drawn}");
    let _ = std::fs::remove_dir_all(&base);
}

/// The same four stops marked by hand while still listening: more may
/// come, so the pane waits for five (Trevor on #1622).
#[test]
fn a_take_still_listening_with_four_stops_waits_for_five() {
    let four = [
        Position::CrownDown,
        Position::CrownLeft,
        Position::CrownUp,
        Position::CrownRight,
    ];
    let (mut app, base) = listened("four-live", &four);
    // Sound still coming in.
    let tx = queued_capture(&mut app, &base);
    assert!(app.running() && app.guided.is_none());
    let t = app.free_table(&egui::Context::default()).expect("a table");
    assert_eq!(t.upright.0.len(), 4, "{t:?}");
    let drawn = draw(&mut app, Pane::DynamicPoise);
    assert!(drawn.contains(&poise_pending()), "{drawn}");
    assert!(!drawn.contains("this take has"), "{drawn}");
    drop(tx);
    let _ = std::fs::remove_dir_all(&base);
}

/// The pane draws a test part way, before five stops are in: the table's
/// rows in the stand's order, core's words for the fit to come, and in
/// either view no wave yet. The Positions Test's pane keeps to its own.
#[test]
fn the_pane_says_the_fit_waits_for_five_stops_in_both_views() {
    let mut app = synthetic_varying(30.0, 0.0, |_| 0.0);
    app.guided = Some(guided::Test::poise().timed(1.0, 5.0));
    for view in [PoiseView::Plot, PoiseView::Dial] {
        app.poise_views = [view == PoiseView::Plot, view == PoiseView::Dial];
        let drawn = draw(&mut app, Pane::DynamicPoise);
        assert!(drawn.contains(&poise_pending()), "{view:?}: {drawn}");
        for label in ["Crown Down (CD, 9H)", "7:30 Up (7H30)", "10:30 Up (10H30)"] {
            assert!(drawn.contains(label), "{label}");
        }
        assert!(drawn.contains("Hour at the top") == (view == PoiseView::Plot));
        assert!(!drawn.contains("Fastest"), "no wave yet");
    }
    let positions = draw(&mut app, Pane::PositionsTest);
    assert!(!positions.contains("Crown Down (CD, 9H)"), "{positions}");
}

/// Info opens core's note on the band near 220°.
#[test]
fn info_shows_cores_note_on_the_band() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let note = timegrapher_core::poise::null_band_note();
    assert!(!draw(&mut app, Pane::DynamicPoise).contains(&note));
    app.poise_info = true;
    assert!(draw(&mut app, Pane::DynamicPoise).contains(&note));
}

/// Hugh's review of #1499 on a measured run: the plot's key and its
/// dotted Mean, named as the table's Mean row; the dial's Mean ring named
/// too; and Info says 219.5° once, in the band's note.
fn the_picture_has_its_key_and_mean_and_info_says_each_fact_once(app: &mut TimegrapherApp) {
    let readings = app.poise_test().unwrap().readings.clone().unwrap();
    let (_, mean) = guided::poise_mean(&readings).unwrap();
    let row = guided::table(&readings, guided::Kind::Poise)
        .mean_rate
        .unwrap();
    assert!(mean.starts_with("Mean ") && mean.contains(&format!("{:.1}", row.abs())));
    app.poise_views = [true, false];
    let plot = draw(app, Pane::DynamicPoise);
    for word in ["Upright positions (8)", "Fitted wave", mean.as_str()] {
        assert!(plot.contains(word), "{word}: {plot}");
    }
    // The Mean's dotted line reaches the screen: short level dashes
    // across the plot, apart from the grid's full-width lines.
    let dashes = plot_dashes(app);
    assert!(dashes > 20, "{dashes} dashes");
    app.poise_views = [false, true];
    let dial = draw(app, Pane::DynamicPoise);
    assert!(
        dial.contains(&mean) && dial.contains("Fastest 6H"),
        "{dial}"
    );
    app.poise_info = true;
    let info = draw(app, Pane::DynamicPoise);
    assert_eq!(info.matches("least at 219.5°").count(), 1, "{info}");
}

/// The heavy spot is a facts box's one row on the picture, in both views,
/// and nothing else of it is over the picture; its caveats and the
/// table's notes are in Info, once (Hugh on #1537). On the dial the box
/// sits above the hours' names.
fn the_heavy_spot_is_in_a_facts_box_and_the_notes_in_info(app: &mut TimegrapherApp) {
    let readings = app.poise_test().unwrap().readings.clone().unwrap();
    let fit = guided::poise_fit(&readings).expect("a fit");
    let (name, value) = fit.heavy_spot_row();
    let notes = guided::poise_table_notes(&readings);
    let caveat = fit.caveat_line();
    for view in [PoiseView::Plot, PoiseView::Dial] {
        app.poise_views = [view == PoiseView::Plot, view == PoiseView::Dial];
        app.poise_info = false;
        let texts = draw_at(app, Pane::DynamicPoise, Vec2::new(1280.0, 820.0));
        let at = |want: &str| texts.iter().find(|(t, _)| t == want).map(|(_, r)| *r);
        let n = at(&name).unwrap_or_else(|| panic!("{view:?}: {texts:?}"));
        let v = at(&value).unwrap_or_else(|| panic!("{value} {view:?}: {texts:?}"));
        assert!(
            (n.center().y - v.center().y).abs() < 1.0,
            "one row: {n:?} {v:?}"
        );
        assert!(at(&fit.heavy_spot_line()).is_none(), "{view:?}");
        assert!(caveat.is_empty() || at(&caveat).is_none(), "{view:?}");
        for note in &notes {
            assert!(at(note).is_none(), "{note} out of Info");
        }
        if view == PoiseView::Dial {
            let hour = at(timegrapher_core::poise::hour_code(0.0)).expect("12H");
            assert!(v.bottom() < hour.top(), "{v:?} over {hour:?}");
        }
        app.poise_info = true;
        let info = draw(app, Pane::DynamicPoise);
        for note in &notes {
            assert_eq!(info.matches(note.as_str()).count(), 1, "{note}: {info}");
        }
    }
}

/// The dial has the watch case behind it (#1762, Peter's version A, from
/// core's `poise_case`): its bezel just outside the outer ring, the whole
/// case inside the pane, lugs and crown included, and the 3H name past
/// the crown; in both themes, and in a short wide pane as in a tall one.
fn the_dial_has_its_watch_case_behind_it(app: &mut TimegrapherApp) {
    use timegrapher_core::poise_case;
    app.poise_views = [false, true];
    for (dark, size) in [
        (true, Vec2::new(1280.0, 820.0)),
        (false, Vec2::new(1280.0, 820.0)),
        (true, Vec2::new(1000.0, 520.0)),
        // Short and wide: the lugs, not the crown, set the dial's size.
        (true, Vec2::new(2400.0, 760.0)),
    ] {
        let ctx = themed();
        ctx.set_theme(if dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let mut out = None;
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            out = Some(ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.pane_tools(ui, Pane::DynamicPoise);
                    app.poise_pane(ui);
                });
            }));
        }
        let out = out.unwrap();
        let palette = timegrapher_core::palette::palette(dark);
        let colour = |c: timegrapher_core::palette::Rgb| egui::Color32::from_rgb(c.r, c.g, c.b);
        let (_, line) = palette.case_over(palette.window);
        let line = colour(line);
        let card = colour(palette.window);
        assert_eq!(card, ctx.style().visuals.panel_fill, "the dial's back");
        // The bezel: a circle filled with the window, the colour behind
        // the dial, outlined in the case's line (Hugh on #1772).
        let bezel = out
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::Shape::Circle(b) if b.fill == card && b.stroke.color == line => Some(*b),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{dark} {size:?}: no bezel"));
        let ring = f64::from(bezel.radius) / poise_case::RING_TO_BEZEL;
        // Every outline of the case whole inside the dial's room.
        case_uncut(&out, line, screen, (dark, size));
        let face = colour(palette.case_over(palette.window).0);
        faces_convex(&out, face);
        // Each lug filled right up to its straight inner edge, as egui
        // paints it (Peter, via Hugh on #1772).
        lugs_filled(&ctx, &out, face, (bezel.center, ring), (dark, size));
        // The 3H name past the crown's end.
        let three = timegrapher_core::poise::hour_code(90.0);
        let name = out
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == three => {
                    Some(t.galley.rect.translate(t.pos.to_vec2()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{dark} {size:?}: no {three}"));
        let crown_end = bezel.center.x + poise_case::reach(ring) as f32;
        assert!(name.left() > crown_end, "{name:?} {crown_end}");
        // The fastest hour's note clear of that hour's name (Hugh on
        // #1772).
        let texts: Vec<(String, egui::Rect)> = out
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
        let note = texts
            .iter()
            .find(|t| t.0.starts_with("Fastest "))
            .unwrap_or_else(|| panic!("{dark} {size:?}: no note"));
        let hour = note.0.trim_start_matches("Fastest ");
        let named = texts.iter().find(|t| t.0 == hour).expect("its hour");
        assert!(!note.1.intersects(named.1), "{note:?} {named:?}");
        if let Ok(d) = std::env::var("SHOT_DIR") {
            let back = ctx.style().visuals.window_fill;
            let svg = crate::export::svg(&out.shapes, screen, back);
            let theme = if dark { "dark" } else { "light" };
            let name = format!("{d}/dial-case-{theme}-{}.svg", size.x);
            std::fs::write(name, svg).unwrap();
        }
    }
}

/// Every piece of the case's face, drawn in `face`, convex, as egui fills
/// only convex shapes (Max on #1772): the lugs filled from core's convex
/// pieces, not whole.
fn faces_convex(out: &egui::FullOutput, face: egui::Color32) {
    let faces: Vec<&Vec<egui::Pos2>> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Path(p) if p.closed && p.fill == face => Some(&p.points),
            _ => None,
        })
        .collect();
    assert!(!faces.is_empty(), "no face");
    for f in faces {
        let n = f.len();
        let turns: Vec<f32> = (0..n)
            .map(|i| {
                let (a, b, c) = (f[i], f[(i + 1) % n], f[(i + 2) % n]);
                (b - a).x * (c - b).y - (b - a).y * (c - b).x
            })
            .filter(|t| t.abs() > 1e-3)
            .collect();
        let convex = turns.iter().all(|t| *t > 0.0) || turns.iter().all(|t| *t < 0.0);
        assert!(convex, "a concave fill: {f:?}");
    }
}

/// The inside of each of the four lugs filled with the case's `face` as
/// egui paints it, right up to the lug's straight inner edge and no
/// further (Peter saw the 1H30 and 7H30 lugs' edges missing; Hugh on
/// #1772): past the bezel, points 2 px inside each inner edge under a
/// triangle of the face in egui's own tessellation of the frame, and
/// points 2 px outside it under none. No face reaches off the case
/// either. The dial is centred at
/// `centre`, its outer ring `ring`.
fn lugs_filled(
    ctx: &egui::Context,
    out: &egui::FullOutput,
    face: egui::Color32,
    (centre, ring): (egui::Pos2, f64),
    (dark, size): (bool, egui::Vec2),
) {
    let case = timegrapher_core::poise_case::case(ring);
    let shapes: Vec<Vec<egui::Pos2>> = case
        .shapes
        .iter()
        .map(|s| {
            s.iter()
                .map(|p| centre + egui::vec2(p[0] as f32, p[1] as f32))
                .collect()
        })
        .collect();
    // Within `px` of the case: inside a shape or that near its edge.
    let near_case = |q: egui::Pos2, px: f32| {
        shapes.iter().any(|s| {
            let n = s.len();
            let mut on = false;
            for i in 0..n {
                let (a, b) = (s[i], s[(i + 1) % n]);
                if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) * (b.x - a.x) / (b.y - a.y)
                {
                    on = !on;
                }
                let t = ((q - a).dot(b - a) / (b - a).length_sq().max(1e-6)).clamp(0.0, 1.0);
                if (a + t * (b - a)).distance(q) <= px {
                    return true;
                }
            }
            on
        })
    };
    let mut tris: Vec<[egui::Pos2; 3]> = Vec::new();
    for p in ctx.tessellate(out.shapes.clone(), 1.0) {
        let egui::epaint::Primitive::Mesh(m) = &p.primitive else {
            continue;
        };
        for t in m.indices.chunks(3) {
            let v = [0, 1, 2].map(|i| &m.vertices[t[i] as usize]);
            if v.iter().all(|v| v.color == face) {
                tris.push(v.map(|v| v.pos));
            }
            // The face's feathered edge stays at the case's edge: a
            // sliver's needle tip feathered out past it as a hairline
            // (#1780).
            if v.iter().any(|v| v.color == face) {
                for q in v.map(|v| v.pos) {
                    assert!(
                        near_case(q, 2.0),
                        "{dark} {size:?}: the face reaches {q:?}, off the case"
                    );
                }
            }
        }
    }
    let inside = |t: &[egui::Pos2; 3], q: egui::Pos2| {
        let side = |a: egui::Pos2, b: egui::Pos2| (b - a).x * (q - a).y - (b - a).y * (q - a).x;
        let s = [side(t[0], t[1]), side(t[1], t[2]), side(t[2], t[0])];
        s.iter().all(|x| *x >= 0.0) || s.iter().all(|x| *x <= 0.0)
    };
    // The body first, then the four lugs; each lug's inner edge runs from
    // its second point to its third.
    let mut checked = 0;
    for lug in &case.shapes[1..5] {
        let (a, b) = (lug[1], lug[2]);
        for i in 0..=20 {
            let t = f64::from(i) / 20.0;
            // 2 px either side of the edge: the lug's side filled, the
            // other bare, so the edge's line shows (the old fill spilled
            // the face over it, from the dial's centre).
            let at = |off: f64| {
                let p = [
                    a[0] + t * (b[0] - a[0]) + a[0].signum() * off,
                    a[1] + t * (b[1] - a[1]),
                ];
                (
                    p[0].hypot(p[1]),
                    centre + egui::vec2(p[0] as f32, p[1] as f32),
                )
            };
            let ((r, q), (_, beyond)) = (at(2.0), at(-2.0));
            if r < case.bezel_r + 3.0 {
                continue;
            }
            assert!(
                tris.iter().any(|t| inside(t, q)),
                "{dark} {size:?}: the lug at {q:?} isn't filled to its inner edge"
            );
            assert!(
                !tris.iter().any(|t| inside(t, beyond)),
                "{dark} {size:?}: the face spills past the lug's inner edge to {beyond:?}"
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 20,
        "{dark} {size:?}: only {checked} points past the bezel"
    );
}

/// Every outline of the dial's case, drawn in `line`, whole inside the
/// dial's room, none of it cut off at the room's edge: lugs, guards and
/// crown (Trevor on #1772).
fn case_uncut(
    out: &egui::FullOutput,
    line: egui::Color32,
    screen: egui::Rect,
    (dark, size): (bool, Vec2),
) {
    let outlines: Vec<(egui::Rect, egui::Pos2)> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Path(p)
                if p.closed && p.stroke.color == egui::epaint::ColorMode::Solid(line) =>
            {
                Some(
                    p.points
                        .iter()
                        .map(|q| (c.clip_rect, *q))
                        .collect::<Vec<_>>(),
                )
            }
            _ => None,
        })
        .flatten()
        .collect();
    assert!(!outlines.is_empty(), "{dark} {size:?}: no case");
    let cut: Vec<_> = outlines
        .iter()
        .filter(|(clip, q)| !clip.intersect(screen).contains(*q))
        .collect();
    assert!(cut.is_empty(), "{dark} {size:?}: the case cut: {cut:?}");
}

/// The table stays beside the picture, tight, while the picture keeps its
/// least width, and the tight table fits the room it is given; narrower
/// still the picture, the result, goes over the table (Hugh, #1499).
fn the_table_tightens_beside_the_picture_then_goes_under_it(app: &mut TimegrapherApp) {
    let at = |app: &mut TimegrapherApp, w: f32| {
        let texts = draw_at(app, Pane::DynamicPoise, Vec2::new(w, 820.0));
        let find = |want: &str| {
            texts
                .iter()
                .find(|(t, _)| t.starts_with(want))
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("{want} at {w}"))
        };
        (
            find("Crown Down (CD, 9H)"),
            find("Once-round wave"),
            find("Beat Error"),
        )
    };
    // Wide: the full table, beside.
    let (row, title, _) = at(app, 1280.0);
    assert!(
        title.left() > row.left() + guided::TABLE_W,
        "{row:?} {title:?}"
    );
    // Narrower: tight, still beside, and within its width.
    let (row, title, last) = at(app, 900.0);
    assert!(
        title.left() > row.left() + guided::TIGHT_TABLE_W,
        "{row:?} {title:?}"
    );
    assert!(
        title.left() < row.left() + guided::TABLE_W,
        "tight: {title:?}"
    );
    assert!(
        last.right() <= row.left() + guided::TIGHT_TABLE_W,
        "{last:?}"
    );
    // Narrow: the picture over the table.
    let (row, title, _) = at(app, 780.0);
    assert!(title.bottom() < row.top(), "{row:?} {title:?}");
}

/// The level dashes drawn in the pane at 1280 by 820: the Plot's dotted
/// lines, which only the Mean's is.
fn plot_dashes(app: &mut TimegrapherApp) -> usize {
    let ctx = themed();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1280.0, 820.0),
        )),
        ..Default::default()
    };
    let out = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.poise_pane(ui));
    });
    out.shapes
        .iter()
        .filter(|c| match &c.shape {
            egui::Shape::LineSegment { points: [a, b], .. } => {
                a.y == b.y && (b.x - a.x).abs() > 1.5 && (b.x - a.x).abs() < 4.0
            }
            _ => false,
        })
        .count()
}

fn poise_pending() -> String {
    timegrapher_core::poise::pending_text()
}

/// The pane's toolbar and body drawn twice at 1280 by 820, its text.
fn draw(app: &mut TimegrapherApp, pane: Pane) -> String {
    draw_at(app, pane, Vec2::new(1280.0, 820.0))
        .into_iter()
        .map(|(t, _)| t)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The pane drawn twice at `size`: each text and where it is.
fn draw_at(app: &mut TimegrapherApp, pane: Pane, size: Vec2) -> Vec<(String, egui::Rect)> {
    let ctx = themed();
    let mut drawn = Vec::new();
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.pane_tools(ui, pane);
                match pane {
                    Pane::DynamicPoise => app.poise_pane(ui),
                    _ => app.positions_pane(ui),
                }
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
        // A picture of both views for review (#1729).
        if let (Ok(d), [true, true]) = (std::env::var("SHOT_DIR"), app.poise_views) {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let back = ctx.style().visuals.window_fill;
            let svg = crate::export::svg(&out.shapes, screen, back);
            std::fs::write(format!("{d}/poise-both-{}.svg", size.x), svg).unwrap();
        }
    }
    drawn
}

/// The pane's Start button goes through the guided tests' one handler
/// (#1563): pressed after the first stop, it turns the card to the second,
/// the strip's measured and not-used stretches follow the test, and the
/// engine is given the new masks.
#[test]
fn the_panes_start_button_turns_to_the_next_stop() {
    let base = std::env::temp_dir().join(format!("tg-app-poise-next-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let (t0, each) = (5.0, 20.0);
    let order = guided::Kind::Poise.order();
    let mut app = synthetic_varying(t0 + 2.0 * each, 4.0, |_| 0.0);
    app.session.listen();
    let _tx = queued_capture(&mut app, &base);
    let mut test = guided::Test::poise().timed(2.0, each - 2.0);
    test.tick(t0);
    assert_eq!(test.tick(t0 + each), guided::Phase::Turn);
    app.guided = Some(test);
    app.set_position(order[0]);
    app.guided_measured.clear();
    app.guided_unused.clear();
    app.live.as_mut().unwrap().set_masked(&[]);
    let size = Vec2::new(1280.0, 900.0);
    let drawn = draw_at(&mut app, Pane::DynamicPoise, size);
    let start = format!("Start {} Measurement", order[1].witschi());
    let (_, button) = drawn
        .iter()
        .find(|(t, _)| t.contains(&start))
        .unwrap_or_else(|| panic!("{start} in {drawn:?}"));
    click(&mut app, Pane::DynamicPoise, size, button.center());
    assert_eq!(app.position, order[1]);
    assert_eq!(app.poise_test().map(|t| t.step), Some(1));
    assert!(
        !app.guided_measured.is_empty(),
        "the strip follows the test"
    );
    assert!(
        app.guided_unused
            .iter()
            .any(|&(a, b, _)| a == 0.0 && b == t0),
        "the time before the test reads as not used: {:?}",
        app.guided_unused
    );
    let engine = app.live.as_ref().unwrap().masked().to_vec();
    assert!(!engine.is_empty(), "the engine leaves the unused time out");
    assert_eq!(engine, app.engine_masked());
    let _ = std::fs::remove_dir_all(&base);
}

/// The pane drawn with a press and release at `at`; the text it put on
/// the clipboard, if any.
fn click(app: &mut TimegrapherApp, pane: Pane, size: Vec2, at: egui::Pos2) -> Option<String> {
    let ctx = themed();
    let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size));
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let frames = [
        vec![egui::Event::PointerMoved(at)],
        vec![press(true)],
        vec![press(false)],
        vec![],
    ];
    let mut copied = None;
    for events in frames {
        let input = egui::RawInput {
            screen_rect: screen,
            events,
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.pane_tools(ui, pane);
                app.poise_pane(ui);
            });
        });
        for c in out.platform_output.commands {
            if let egui::OutputCommand::CopyText(t) = c {
                copied = Some(t);
            }
        }
    }
    copied
}

/// A saved Dynamic Poise take whose stops start between the 2 s windows
/// of its whole sound (61 s each), so the window over each stop's start
/// holds the end of the stop before. The app, reading the take's stops,
/// and `timegrapher session`, reading its whole sound, give each stop
/// the same figures and ±s: a stop takes only the windows wholly inside
/// it (Norm on #1624).
#[test]
fn a_saved_poise_take_reads_the_same_in_the_app_and_the_cli() {
    let (app, base) = saved_and_opened_each("edge", guided::Kind::Poise.order(), 61.0);
    assert_eq!(reads_as_the_cli(&app).len(), 8);
    let _ = std::fs::remove_dir_all(base);
}

/// A take of one stop is a plain take, not a test: its edge falls inside
/// a window, and the app keeps that window by the midpoint rule and
/// borrows from the take around it, as the CLI does (Norm and Trevor on
/// #1700).
#[test]
fn a_saved_one_stop_take_reads_the_same_in_the_app_and_the_cli() {
    let one = &guided::Kind::Poise.order()[..1];
    let (app, base) = saved_and_opened_each("one", one, 61.0);
    let read = reads_as_the_cli(&app);
    assert_eq!(read.len(), 1);
    let c = read[0].confidence.as_ref().expect("a watch heard");
    assert!(c.amplitude.se.is_some(), "amplitude ± kept");
    let _ = std::fs::remove_dir_all(base);
}

/// Each stop of the opened take `app` read by the app and by
/// `take::measure_entry` gives the same figures and ±s; the stops
/// compared, as the app read them.
fn reads_as_the_cli(app: &TimegrapherApp) -> Vec<timegrapher_core::session::Measurement> {
    let t = app.free_table(&egui::Context::default()).expect("a table");
    let dir = app.session.saved_to().map(Path::to_path_buf);
    let dir = dir.unwrap_or_else(|| PathBuf::from(&app.opened_path));
    let text = std::fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let m: timegrapher_core::session::Manifest = toml::from_str(&text).unwrap();
    let settings = timegrapher_core::take::EntrySettings {
        stream: timegrapher_core::stream::StreamConfig::default(),
        card_clock: false,
        settle_s: None,
    };
    let mut compared = Vec::new();
    for e in &m.recordings {
        let cli = timegrapher_core::take::measure_entry(
            &dir,
            &m,
            e,
            &settings,
            |_| Ok(None),
            |_| Ok(None),
        )
        .expect("read")
        .measurement;
        let label = e.position.as_deref().unwrap();
        let Some(r) = t.upright.0.iter().find(|r| r.position.common() == label) else {
            continue;
        };
        let (a, b) = (&r.measurement, &cli);
        let near = |x: Option<f64>, y: Option<f64>, tol: f64| match (x, y) {
            (Some(x), Some(y)) => (x - y).abs() <= tol,
            (x, y) => x.is_none() && y.is_none(),
        };
        assert!(
            near(a.amplitude_deg, b.amplitude_deg, 1e-6),
            "{label} amplitude"
        );
        assert!(
            near(a.beat_error_unlock_ms, b.beat_error_unlock_ms, 1e-6),
            "{label} beat error"
        );
        let (ca, cb) = (a.confidence.unwrap(), b.confidence.unwrap());
        assert!(
            near(ca.amplitude.se, cb.amplitude.se, 1e-6),
            "{label} amplitude ±"
        );
        assert!(
            near(ca.beat_error.se, cb.beat_error.se, 1e-6),
            "{label} beat error ±"
        );
        assert!(near(ca.rate.se, cb.rate.se, 1e-3), "{label} rate ±");
        compared.push(a.clone());
    }
    compared
}

/// Press the Dynamic Poise pane's Save as Text…, found where the
/// toolbar drew it, then pick `path` as the dialog would: what it wrote.
fn press_save_as_text(mut app: TimegrapherApp, path: &std::path::Path) -> String {
    let ctx = themed();
    let size = Vec2::new(1280.0, 820.0);
    let run = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.pane_tools(ui, Pane::DynamicPoise);
                app.poise_pane(ui);
            });
        });
    };
    run(&mut app, vec![]);
    run(&mut app, vec![]);
    let tools = theme::tools_drawn_at(&ctx);
    let at = tools
        .iter()
        .find(|(k, _)| k.ends_with("/Save as Text…"))
        .unwrap_or_else(|| panic!("Save as Text… in {tools:?}"))
        .1
        .center();
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    run(&mut app, vec![egui::Event::PointerMoved(at)]);
    run(&mut app, vec![press(true)]);
    run(&mut app, vec![press(false)]);
    assert!(app.picking(), "Save as Text… asks where");
    saved_text(app, path).1
}

/// What Save as Text… writes, picked to `path` as the dialog would.
fn saved_text(app: TimegrapherApp, path: &std::path::Path) -> (TimegrapherApp, String) {
    let app = std::sync::Mutex::new(app);
    assert!(crate::pick::answer(&app, |_, pick| {
        assert_eq!(pick, crate::pick::Pick::Save);
        vec![path.to_path_buf()]
    }));
    let text = std::fs::read_to_string(path).expect("the report written");
    (app.into_inner().unwrap(), text)
}

/// Save as Text… on the Dynamic Poise pane, pressed with its test on the
/// Bench, writes core's poise report of the test (#1509): the watch, how
/// it was read with Settle and Each Position, the table and the fit. The
/// Positions Test's Save as Text… asks nothing for it.
fn save_as_text_writes_the_poise_tests_report(mut app: TimegrapherApp, base: &std::path::Path) {
    let about = app.guided_about();
    app.save_test_text(&about, guided::Kind::Positions, None, None);
    assert!(!app.picking(), "no Positions Test on the Bench");
    let t = app.poise_test().expect("the test");
    let take = guided::began_name(t);
    let readings = t.readings_so_far().to_vec();
    let header = timegrapher_core::text_report::Header {
        watch: Some(about.watch.clone()),
        date: timegrapher_core::text_report::take_date(&take),
        calibre: Some(about.calibre.clone()),
        bph: about.bph,
        // No calibre gives it, so the lift is in the heading.
        lift_angle_deg: Some(about.lift_angle_deg),
        settle_s: Some(t.settle_s),
        measure_s: Some(t.measure_s),
    };
    assert!(!about.calibre.contains('°'), "{}", about.calibre);
    let text = press_save_as_text(app, &base.join("poise.txt"));
    assert_eq!(readings.len(), 8);
    assert_eq!(
        text,
        timegrapher_core::text_report::poise(&header, &readings)
    );
}

/// Save as Text… on the Dynamic Poise pane, pressed for a take opened
/// again with no test on the Bench, writes core's poise report of the upright stops
/// the pane shows, with no Settle or Each Position (#1509).
fn save_as_text_writes_an_opened_poise_takes_report(app: TimegrapherApp, base: &std::path::Path) {
    // The pane's table is read off the window's thread: wait for it.
    let ctx = egui::Context::default();
    let start = Instant::now();
    let kept = loop {
        if let Some(k) = app.poise_kept(&ctx) {
            break k;
        }
        assert!(start.elapsed().as_secs() < 300, "never read");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(kept.len(), 8);
    let about = app.guided_about();
    let take = app
        .take_on_show()
        .and_then(|p| Some(p.file_name()?.to_string_lossy().into_owned()));
    let header = timegrapher_core::text_report::Header {
        watch: Some(about.watch.clone()),
        date: take
            .as_deref()
            .and_then(timegrapher_core::text_report::take_date),
        calibre: Some(about.calibre.clone()),
        bph: about.bph,
        lift_angle_deg: Some(about.lift_angle_deg),
        settle_s: None,
        measure_s: None,
    };
    let text = press_save_as_text(app, &base.join("kept.txt"));
    assert_eq!(text, timegrapher_core::text_report::poise(&header, &kept));
}

/// With no stops at all, as on a fresh Quick Listen, the pane says how a
/// test fits, not that the take lacks positions (Hugh on #1622).
#[test]
fn a_take_with_no_stops_says_how_the_test_fits() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let drawn = draw(&mut app, Pane::DynamicPoise);
    assert!(
        drawn.contains(&timegrapher_core::poise::pending_text()),
        "{drawn}"
    );
    assert!(!drawn.contains("Needs 5"), "{drawn}");
}

/// A Dynamic Poise test run live with the pane's Next on one continuous
/// sound (no handling between stops), settling 5 s and measuring 30 s at
/// each, saved and opened again: every stop reads the same live as
/// reopened (#1720, Norm: the reopened table is right).
#[test]
fn a_poise_test_reads_the_same_live_as_reopened() {
    let base = std::env::temp_dir().join(format!("tg-app-poise-live1720-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let (settle, measure) = (5.0, 30.0);
    let mut app = synthetic_varying(0.0, 0.0, |_| 0.0);
    app.session.listen();
    app.session.recorder =
        Some(Recorder::start(&base.join("live"), app.info("test", 48000, 16)).unwrap());
    let cfg = SynthConfig {
        duration_s: 8.0 * (settle + measure) + 1.0,
        snr_db: 30.0,
        rate_s_per_day: 9.0,
        seed: 1720,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 310.0, |_| 0.0);
    let tx = queued_capture(&mut app, &base);
    app.guided = Some(guided::Test::poise().timed(settle, measure));
    // The pane's Settle and Each Position, as each frame gives the session.
    (app.guided_settle_s, app.guided_measure_s) = (settle, measure);
    app.session.set_settle_s(settle);
    app.session.set_state(session::State::Listening);
    let ctx = egui::Context::default();
    let mut k = 0u64;
    for s in audio.samples.chunks(48_000) {
        // The 20 ms blocks a microphone sends, taken as listening takes
        // them (saved, read, settling and stretches as the app goes).
        for b in s.chunks(960) {
            tx.send(Event::Audio(timegrapher_core::capture::Block {
                samples: b.to_vec(),
                at: std::time::UNIX_EPOCH
                    + std::time::Duration::from_millis(1_700_000_000_000 + 20 * k),
            }))
            .unwrap();
            k += 1;
        }
        let want = k as f64 * 0.02;
        let t = Instant::now();
        while app.live.as_ref().unwrap().duration_s() < want - 1e-6 && t.elapsed().as_secs() < 30 {
            app.poll();
        }
        let now = app.live.as_ref().unwrap().duration_s();
        app.guided_tick(&ctx);
        // Next, as the pane's button gives it, once the stop is read.
        let next = app.guided.as_mut().is_some_and(|t| {
            t.tick(now) == guided::Phase::Turn && !t.needs_reading() && t.next_position().is_some()
        });
        if next {
            app.apply_guided_action(guided::Action::Next, now);
        }
    }
    let live = app
        .poise_test()
        .and_then(|t| t.readings.clone())
        .expect("finished live");
    assert_eq!(live.len(), 8);
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    let saved = app.session.saved_to().expect("saved").to_path_buf();
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.start_batch(&saved);
    let t = Instant::now();
    while again.batch.is_some() && t.elapsed().as_secs() < 300 {
        again.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let table = again.free_table(&ctx).expect("a table");
    let reopened = &table.upright.0;
    assert_eq!(reopened.len(), 8);
    for (l, r) in live.iter().zip(reopened) {
        let (a, b) = (&l.measurement, &r.measurement);
        // Read whole both ways: no settling or turn culls part of a stop.
        assert!(
            a.beats.abs_diff(b.beats) <= 8,
            "{} beats {} live, {} reopened",
            l.label,
            a.beats,
            b.beats
        );
        let (ra, rb) = (a.rate_s_per_day.unwrap(), b.rate_s_per_day.unwrap());
        let (aa, ab) = (a.amplitude_deg.unwrap(), b.amplitude_deg.unwrap());
        assert!(
            (ra - rb).abs() < 0.01,
            "{} rate {ra} live, {rb} reopened",
            l.label
        );
        assert!(
            (aa - ab).abs() < 1.0,
            "{} amplitude {aa} live, {ab} reopened",
            l.label
        );
    }
    drop(tx);
    let _ = std::fs::remove_dir_all(&base);
}

/// With Plot and Dial both on (#1729), the two sit side by side where
/// there is room, the Plot left of the Dial, and one over the other
/// where there is not.
fn both_views_sit_side_by_side_where_there_is_room(app: &mut TimegrapherApp) {
    app.poise_views = [true, true];
    // The plot names the fastest hour too: the dial's is the other one.
    let at = |app: &mut TimegrapherApp, w: f32| {
        let texts = draw_at(app, Pane::DynamicPoise, Vec2::new(w, 820.0));
        let all = |want: &str| -> Vec<egui::Rect> {
            texts
                .iter()
                .filter(|(t, _)| t.starts_with(want))
                .map(|(_, r)| *r)
                .collect()
        };
        let hour = *all("Hour at the top").first().expect("the plot");
        let fastest = all("Fastest 6H");
        assert_eq!(fastest.len(), 2, "both views at {w}: {fastest:?}");
        // The heavy spot's plate once, on the plot (Hugh on #1794).
        assert_eq!(all("Heavy spot").len(), 1, "one plate at {w}");
        (hour, fastest)
    };
    let (plot, fastest) = at(app, 2000.0);
    assert!(
        fastest.iter().any(|d| d.left() > plot.right()),
        "side by side: {plot:?} {fastest:?}"
    );
    let (plot, fastest) = at(app, 1100.0);
    assert!(
        fastest.iter().any(|d| d.top() > plot.bottom()),
        "one over the other: {plot:?} {fastest:?}"
    );
}

/// The pane at `size` after a wheel turn of `dy` points with the pointer
/// at `at`: each text and where it is.
fn wheeled(
    app: &mut TimegrapherApp,
    size: Vec2,
    (at, dy): (egui::Pos2, f32),
) -> Vec<(String, egui::Rect)> {
    let ctx = themed();
    let mut drawn = Vec::new();
    for frame in 0..5 {
        let mut events = vec![egui::Event::PointerMoved(at)];
        if frame == 2 {
            events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -dy),
                modifiers: Default::default(),
            });
        }
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.pane_tools(ui, Pane::DynamicPoise);
                app.poise_pane(ui);
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

/// #1729's rule 2: a wheel over the table scrolls the table and leaves
/// the fit's words and the picture where they were; in a pane too short
/// for the picture, a wheel over it scrolls it on its own, so none of it
/// is cut off (Trevor and Hugh on #1794).
fn only_the_table_scrolls_and_a_short_pane_scrolls_the_picture(app: &mut TimegrapherApp) {
    let find = |texts: &[(String, egui::Rect)], want: &str| {
        texts
            .iter()
            .find(|(t, _)| t.starts_with(want))
            .map(|(_, r)| *r)
    };
    // Short enough that neither the table nor the picture fits.
    let size = Vec2::new(1280.0, 250.0);
    let still = wheeled(app, size, (egui::pos2(-10.0, -10.0), 0.0));
    let row = find(&still, "Crown Down (CD, 9H)").expect("the first row");
    let words = find(&still, "Once-round wave").expect("the fit's words");
    // Over the table.
    let rolled = wheeled(app, size, (row.center(), 200.0));
    let row_now = find(&rolled, "Crown Down (CD, 9H)");
    assert!(
        row_now.is_none_or(|r| r.top() < row.top() - 20.0),
        "the table scrolled: {row:?} {row_now:?}"
    );
    let words_now = find(&rolled, "Once-round wave").expect("the words stay");
    assert_eq!(words_now.top(), words.top(), "the words stay put");
    // The picture's foot is below the short pane until it is scrolled.
    let hour = find(&still, "Hour at the top");
    assert!(
        hour.is_none_or(|r| r.bottom() > size.y),
        "cut at first: {hour:?}"
    );
    let rolled = wheeled(app, size, (words.center() + Vec2::new(0.0, 120.0), 400.0));
    let hour = find(&rolled, "Hour at the top").expect("the plot's foot");
    assert!(hour.bottom() <= size.y, "scrolled into view: {hour:?}");
    let row_now = find(&rolled, "Crown Down (CD, 9H)").expect("the table");
    assert_eq!(row_now.top(), row.top(), "the table stays put");
}
