//! The Dynamic Poise test and its pane (#1474): set up at 9H with the
//! card at 10, led through the eight upright stops, kept out of the
//! Positions Test's pane, and drawn with core's fit.

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
#[test]
fn a_poise_test_measures_eight_stops_and_core_fits_the_wave() {
    let (app, base, tx) = measured("fit");
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
    drop(tx);
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
#[test]
fn a_saved_poise_take_opened_again_shows_its_stops_and_fit() {
    let (mut app, base) = saved_and_opened("reopen", guided::Kind::Poise.order());
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
    let drawn = draw(&mut app, Pane::DynamicPoise);
    for label in ["Crown Down (CD, 9H)", "7:30 Up (7H30)", "10:30 Up (10H30)"] {
        assert!(drawn.contains(label), "{label}: {drawn}");
    }
    assert!(drawn.contains(&fit.short_text), "{drawn}");
    assert!(!drawn.contains(&poise_pending()), "{drawn}");
    let _ = std::fs::remove_dir_all(&base);
}

/// A reopened Dynamic Poise take's Copy copies the rows its table
/// shows: it is not greyed out as "Nothing measured yet", and the text
/// has the eight stops and the fit (#1657, from an external audit).
#[test]
fn a_reopened_poise_takes_copy_copies_the_table_it_shows() {
    let (mut app, base) = saved_and_opened("copy", guided::Kind::Poise.order());
    let t = app.free_table(&egui::Context::default()).expect("a table");
    let fit = guided::poise_fit(&t.upright.0).expect("fitted from eight");
    let text = guided::kept_rows_text(&t.upright.0, guided::Kind::Poise, &app.guided_about(), '\t');
    for label in ["Crown Down (CD, 9H)", "7:30 Up (7H30)", "10:30 Up (10H30)"] {
        assert!(text.contains(label), "{label}: {text}");
    }
    assert!(text.contains(&fit.text), "{text}");
    let size = Vec2::new(1280.0, 820.0);
    let drawn = draw_at(&mut app, Pane::DynamicPoise, size);
    let (_, copy) = drawn
        .iter()
        .find(|(t, _)| t == "Copy")
        .unwrap_or_else(|| panic!("Copy in {drawn:?}"));
    let copied = click(&mut app, Pane::DynamicPoise, size, copy.center());
    assert_eq!(
        copied.as_deref(),
        Some(text.as_str()),
        "what the table shows"
    );
    let said = app.message.as_ref().map(|(m, _)| m.as_str());
    assert_eq!(said, Some("Copied the Dynamic Poise table"));
    let _ = std::fs::remove_dir_all(&base);
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
        app.poise_view = view;
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
#[test]
fn the_picture_has_its_key_and_mean_and_info_says_each_fact_once() {
    let (mut app, base, tx) = measured("key");
    let readings = app.poise_test().unwrap().readings.clone().unwrap();
    let (_, mean) = guided::poise_mean(&readings).unwrap();
    let row = guided::table(&readings, guided::Kind::Poise)
        .mean_rate
        .unwrap();
    assert!(mean.starts_with("Mean ") && mean.contains(&format!("{:.1}", row.abs())));
    app.poise_view = PoiseView::Plot;
    let plot = draw(&mut app, Pane::DynamicPoise);
    for word in ["Upright positions (8)", "Fitted wave", mean.as_str()] {
        assert!(plot.contains(word), "{word}: {plot}");
    }
    // The Mean's dotted line reaches the screen: short level dashes
    // across the plot, apart from the grid's full-width lines.
    let dashes = plot_dashes(&mut app);
    assert!(dashes > 20, "{dashes} dashes");
    app.poise_view = PoiseView::Dial;
    let dial = draw(&mut app, Pane::DynamicPoise);
    assert!(
        dial.contains(&mean) && dial.contains("Fastest 6H"),
        "{dial}"
    );
    app.poise_info = true;
    let info = draw(&mut app, Pane::DynamicPoise);
    assert_eq!(info.matches("least at 219.5°").count(), 1, "{info}");
    drop(tx);
    let _ = std::fs::remove_dir_all(&base);
}

/// The heavy spot is a facts box's one row on the picture, in both views,
/// and nothing else of it is over the picture; its caveats and the
/// table's notes are in Info, once (Hugh on #1537). On the dial the box
/// sits above the hours' names.
#[test]
fn the_heavy_spot_is_in_a_facts_box_and_the_notes_in_info() {
    let (mut app, base, tx) = measured("facts");
    let readings = app.poise_test().unwrap().readings.clone().unwrap();
    let fit = guided::poise_fit(&readings).expect("a fit");
    let (name, value) = fit.heavy_spot_row();
    let notes = guided::poise_table_notes(&readings);
    let caveat = fit.caveat_line();
    for view in [PoiseView::Plot, PoiseView::Dial] {
        app.poise_view = view;
        app.poise_info = false;
        let texts = draw_at(&mut app, Pane::DynamicPoise, Vec2::new(1280.0, 820.0));
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
        let info = draw(&mut app, Pane::DynamicPoise);
        for note in &notes {
            assert_eq!(info.matches(note.as_str()).count(), 1, "{note}: {info}");
        }
    }
    drop(tx);
    let _ = std::fs::remove_dir_all(&base);
}

/// The table stays beside the picture, tight, while the picture keeps its
/// least width, and the tight table fits the room it is given; narrower
/// still the picture, the result, goes over the table (Hugh, #1499).
#[test]
fn the_table_tightens_beside_the_picture_then_goes_under_it() {
    let (mut app, base, tx) = measured("layout");
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
    let (row, title, _) = at(&mut app, 1280.0);
    assert!(
        title.left() > row.left() + guided::TABLE_W,
        "{row:?} {title:?}"
    );
    // Narrower: tight, still beside, and within its width.
    let (row, title, last) = at(&mut app, 900.0);
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
    let (row, title, _) = at(&mut app, 780.0);
    assert!(title.bottom() < row.top(), "{row:?} {title:?}");
    drop(tx);
    let _ = std::fs::remove_dir_all(&base);
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
