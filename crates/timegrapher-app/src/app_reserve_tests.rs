//! The Power Reserve test, headless: where it runs, what it keeps and
//! how its take is filed.

use super::tests::{painted_text, queued_capture};
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

/// A 46 h run in a 1000 x 700 or 800 x 600 window: the Power Reserve pane
/// takes room enough to show both its charts whole, each naming 0 h on
/// its hours' axis, the table scrolling under them. The Rate chart's zero
/// line reads "0 s/d", never "-0 s/d" (#991). At 1000 x 700 the Rate
/// chart names more than its zero line.
#[test]
fn a_long_runs_two_charts_show_whole_in_a_small_window() {
    for (w, h) in [(1000.0f32, 700.0f32), (800.0, 600.0)] {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let mut r = reserve::finished_run(47).results.unwrap();
        r.run_s = 46.5 * 3600.0;
        for hr in &mut r.hours {
            let x = f64::from(hr.hour);
            hr.rate_s_per_day = Some(2.0 - 0.004 * x * x);
            hr.amplitude_deg = Some(295.0 - 2.1 * x);
        }
        let ctx = super::tests::themed();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h));
        let pass = |app: &mut TimegrapherApp| {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            ctx.run(input, |ctx| app.panels(ctx))
        };
        for _ in 0..3 {
            pass(&mut app);
        }
        // The run's take opened, as from the Watches view.
        let dir = std::env::temp_dir().join(format!("tg-app-small-{}-{w}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(power_reserve::FILE), r.to_json()).unwrap();
        app.load_reserve(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        let mut out = None;
        for _ in 0..4 {
            out = Some(pass(&mut app));
        }
        // The text shown: inside its clip and the window.
        let shown: Vec<String> = out
            .unwrap()
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => {
                    let r = t.galley.rect.translate(t.pos.to_vec2());
                    let clip = c.clip_rect.intersect(screen);
                    (clip.contains_rect(r)).then(|| t.galley.text().to_string())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            shown.iter().filter(|t| *t == "0 h").count(),
            2,
            "{w}: {shown:?}"
        );
        assert!(shown.iter().any(|t| t == "0 s/d"), "{w}: {shown:?}");
        let rate_labels = shown
            .iter()
            .filter(|t| {
                t.strip_suffix(" s/d")
                    .is_some_and(|v| !v.contains('.') && v.parse::<i32>().is_ok())
            })
            .count();
        assert!(w < 1000.0 || rate_labels >= 2, "{w}: {shown:?}");
        assert!(!shown.iter().any(|t| t.starts_with("-0")), "{w}: {shown:?}");
        // The rate falls below zero: its scale's negative values carry a
        // true minus, never a hyphen (#1402).
        assert!(
            shown
                .iter()
                .any(|t| t.starts_with('\u{2212}') && t.ends_with(" s/d")),
            "{w}: {shown:?}"
        );
        assert!(
            !shown
                .iter()
                .any(|t| t.starts_with('-') && t[1..].starts_with(|c: char| c.is_ascii_digit())),
            "{w}: {shown:?}"
        );
    }
}

/// The text drawn whole inside its clip and the window, below the Power
/// Reserve pane's run line: the pane's charts and what shows of its table.
fn under_run_line(out: &egui::FullOutput, screen: egui::Rect) -> Vec<String> {
    let shown: Vec<(String, egui::Rect)> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => {
                let r = t.galley.rect.translate(t.pos.to_vec2());
                let clip = c.clip_rect.intersect(screen);
                clip.contains_rect(r)
                    .then(|| (t.galley.text().to_string(), r))
            }
            _ => None,
        })
        .collect();
    let line = shown
        .iter()
        .find(|t| t.0.starts_with("Ran "))
        .map_or(f32::INFINITY, |t| t.1.bottom());
    shown
        .into_iter()
        .filter(|t| t.1.top() >= line)
        .map(|t| t.0)
        .collect()
}

/// How many of `shown` are whole-number axis labels in `unit`.
fn axis_labels(shown: &[String], unit: &str) -> usize {
    shown
        .iter()
        .filter(|t| {
            // A screen's minus is the true one (#1402).
            t.strip_suffix(unit)
                .is_some_and(|v| v.replace('\u{2212}', "-").parse::<i32>().is_ok())
        })
        .count()
}

/// A 46 h run's take opened with its sound, as from the command line, in
/// a 1000 x 700 window: the Paper Strip shows the sound's beats and keeps
/// its least, and the Power Reserve pane still shows both charts whole,
/// each naming at least two values and 0 h (#991, the #1043 walkthrough).
#[test]
fn a_long_runs_charts_read_with_its_take_open_in_a_small_window() {
    let dir = std::env::temp_dir().join(format!("tg-app-run-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("take.wav");
    let audio = generate(
        &SynthConfig {
            duration_s: 60.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&path, &audio).unwrap();
    let mut r = reserve::finished_run(47).results.unwrap();
    r.run_s = 46.5 * 3600.0;
    for hr in &mut r.hours {
        let x = f64::from(hr.hour);
        hr.rate_s_per_day = Some(2.0 - 0.004 * x * x);
        hr.amplitude_deg = Some(295.0 - 2.1 * x);
    }
    std::fs::write(dir.join(power_reserve::FILE), r.to_json()).unwrap();
    let ctx = super::tests::themed();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1000.0, 700.0));
    let pass = |app: &mut TimegrapherApp| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        ctx.run(input, |ctx| app.panels(ctx))
    };
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    for _ in 0..3 {
        pass(&mut app);
    }
    app.start_batch(&path);
    let t = std::time::Instant::now();
    while app.batch.is_some() && t.elapsed().as_secs() < 120 {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(app.live.is_some(), "{:?}", app.message);
    assert!(app.reserve.is_some(), "{:?}", app.message);
    let mut out = None;
    for _ in 0..6 {
        out = Some(pass(&mut app));
    }
    let _ = std::fs::remove_dir_all(&dir);
    if let Ok(d) = std::env::var("SHOT_DIR") {
        let o = out.as_ref().unwrap();
        let svg = crate::export::svg(&o.shapes, screen, ctx.style().visuals.window_fill);
        std::fs::write(format!("{d}/open-1000.svg"), svg).unwrap();
    }
    let shown = under_run_line(out.as_ref().unwrap(), screen);
    // Each chart's first and last hour drawn whole, the right one clear
    // of the pane's edge and its scroll bar.
    for h in ["0 h", "48 h"] {
        assert_eq!(shown.iter().filter(|t| *t == h).count(), 2, "{shown:?}");
    }
    let last_right = out
        .as_ref()
        .unwrap()
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) if t.galley.text() == "48 h" => {
                Some(t.galley.rect.translate(t.pos.to_vec2()).right())
            }
            _ => None,
        })
        .fold(0.0f32, f32::max);
    // The pane's 12 pt edge and its scroll bar, with room to spare.
    assert!(last_right <= screen.right() - 36.0, "{last_right}");
    assert!(axis_labels(&shown, " s/d") >= 2, "{shown:?}");
    assert!(axis_labels(&shown, "°") >= 2, "{shown:?}");
}

/// Each text drawn, its rect, and the part of it its clip shows.
fn clipped_texts(out: &egui::FullOutput) -> Vec<(String, egui::Rect, egui::Rect)> {
    out.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => {
                let r = t.galley.rect.translate(t.pos.to_vec2());
                Some((t.galley.text().to_string(), r, r.intersect(c.clip_rect)))
            }
            _ => None,
        })
        .collect()
}

/// The Power Reserve plots' frames: drawn in the plot's `fill` below the
/// run's line `ran`, at least 40 pt each way.
fn plot_frames(out: &egui::FullOutput, fill: egui::Color32, ran: egui::Rect) -> Vec<egui::Rect> {
    out.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(r)
                if r.fill == fill
                    && r.rect.top() > ran.bottom()
                    && r.rect.height() >= 40.0
                    && r.rect.width() >= 40.0
                    && r.rect.left() >= ran.left() =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .collect()
}

/// The hours' rows cut by the band under the table's pinned titles, drawn
/// in `fill`: a row half under it would show as a sliver of text.
fn rows_cut(out: &egui::FullOutput, fill: egui::Color32) -> Vec<String> {
    let texts = clipped_texts(out);
    // "Read" is the table's alone; the cards have a Rate.
    let Some(title) = texts.iter().find(|t| t.0 == "Read") else {
        return Vec::new();
    };
    let band = out.shapes.iter().find_map(|c| match &c.shape {
        egui::Shape::Rect(r)
            if r.fill == fill
                // From the pane's top: the titles, or the key over them.
                && r.rect.top() <= title.1.top() + 4.0
                && title.1.top() - r.rect.top() < 30.0
                && r.rect.bottom() >= title.1.bottom()
                && r.rect.width() > 100.0 =>
        {
            Some(r.rect)
        }
        _ => None,
    });
    let Some(band) = band else {
        return Vec::new();
    };
    texts
        .into_iter()
        .filter(|t| {
            t.0.ends_with(" h")
                && t.0.contains('–')
                && t.1.top() < band.bottom()
                && t.1.bottom() > band.bottom()
        })
        .map(|t| t.0)
        .collect()
}

/// Scrolled down the table, with every beat error dimmed: the titles are
/// pinned whole, the grey cells' key is not over them (it is in the Info
/// panel, #1528), and a pinned title shows its hover (Walter's walk of
/// #1084).
fn pinned_titles_and_hovers(
    ctx: &egui::Context,
    app: &mut TimegrapherApp,
    at: egui::Pos2,
    screen: egui::Rect,
) {
    let w = screen.width();
    ctx.style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    let run = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| app.panels(ctx))
    };
    let out = run(app, vec![egui::Event::PointerMoved(at)]);
    let texts = clipped_texts(&out);
    let whole = |name: &str| {
        texts
            .iter()
            .filter(|t| t.0 == name && t.2 == t.1)
            .map(|t| t.1)
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .unwrap_or_else(|| panic!("{w}: {name} not whole: {texts:?}"))
    };
    assert!(
        !texts.iter().any(|t| t.0 == reserve::DIM_KEY_TEXT),
        "{w}: the key over the table: {texts:?}"
    );
    let read = whole("Read");
    // The pinned titles sit tight over the first whole row, as in their
    // own place (Walter's walk of #1097).
    let unit = whole("%");
    let next_row = texts
        .iter()
        .filter(|t| t.2 == t.1 && t.0.ends_with(" h") && t.1.top() > unit.bottom())
        .map(|t| t.1.top())
        .fold(f32::INFINITY, f32::min);
    assert!(next_row - unit.bottom() < 10.0, "{w}: {unit:?} {next_row}");
    // The table has scrolled: its first hour is out of sight.
    assert!(!texts.iter().any(|t| t.0 == "0–1 h" && t.2 == t.1), "{w}");
    let mut out = run(app, vec![egui::Event::PointerMoved(read.center())]);
    for _ in 0..2 {
        out = run(app, Vec::new());
    }
    let hint = reserve::HOUR_COLUMNS[5].2;
    assert!(
        clipped_texts(&out).iter().any(|t| t.0 == hint),
        "{w}: no hover over the pinned title"
    );
}

/// A 46 h run's take opened in `app`, as from the Watches view, its rate
/// falling away and its amplitude with it; `tag` names its folder.
fn open_long_run(app: &mut TimegrapherApp, tag: &str) {
    let mut r = reserve::finished_run(47).results.unwrap();
    r.run_s = 46.5 * 3600.0;
    for hr in &mut r.hours {
        let x = f64::from(hr.hour);
        hr.rate_s_per_day = Some(2.0 - 0.04 * x * x);
        hr.amplitude_deg = Some(295.0 - 2.1 * x);
    }
    let dir = std::env::temp_dir().join(format!("tg-app-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(power_reserve::FILE), r.to_json()).unwrap();
    app.load_reserve(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A 46 h run's table starts at its charts' plot frame, the value labels
/// alone in the margin to its left, stacked at 2560 x 1440 and side by
/// side at 1000 x 700 and 800 x 600; its units are in its titles
/// (Peter's macOS screenshot, 11-04; Hugh's rules).
#[test]
fn a_runs_table_starts_at_its_charts_plot_edge() {
    for (w, h) in [(2560.0f32, 1440.0f32), (1000.0, 700.0), (800.0, 600.0)] {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let ctx = super::tests::themed();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h));
        let pass = |app: &mut TimegrapherApp, wheel: Option<egui::Pos2>| {
            let mut events = Vec::new();
            if let Some(at) = wheel {
                events.push(egui::Event::PointerMoved(at));
                events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -120.0),
                    modifiers: Default::default(),
                });
            }
            let input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            ctx.run(input, |ctx| app.panels(ctx))
        };
        for _ in 0..3 {
            pass(&mut app, None);
        }
        open_long_run(&mut app, &format!("table-{w}"));
        let mut out = pass(&mut app, None);
        for _ in 0..3 {
            out = pass(&mut app, None);
        }
        let ran = clipped_texts(&out)
            .iter()
            .find(|t| t.0.starts_with("Ran "))
            .map(|t| t.1)
            .unwrap_or_else(|| panic!("{w}: no run line"));
        let frames = plot_frames(&out, ctx.style().visuals.extreme_bg_color, ran);
        assert_eq!(frames.len(), 2, "{w}: {frames:?}");
        let plot_left = frames
            .iter()
            .map(|r| r.left())
            .fold(f32::INFINITY, f32::min);
        let plot_right = frames.iter().map(|r| r.right()).fold(0.0f32, f32::max);
        // Scroll the pane down its table, a step at a time, gathering what
        // shows: under the charts at 1000 and 800.
        let at = egui::pos2(plot_left + 20.0, frames[0].center().y);
        // Wherever it is scrolled to, rows that show have their titles in
        // view, so each figure keeps its unit.
        let whole = |t: &(String, egui::Rect, egui::Rect)| t.2 == t.1;
        let mut shown = clipped_texts(&out);
        for step in 0..14 {
            let o = pass(&mut app, Some(at));
            let cut = rows_cut(&o, ctx.style().visuals.extreme_bg_color);
            assert!(cut.is_empty(), "{w}, step {step}: {cut:?}");
            let now = clipped_texts(&o);
            let rows = now
                .iter()
                .any(|t| whole(t) && t.0.ends_with(" h") && t.0.contains('–'));
            let titled = now.iter().any(|t| whole(t) && t.0 == "Read");
            assert!(!rows || titled, "{w}, step {step}: {now:?}");
            shown.extend(now);
        }
        pinned_titles_and_hovers(&ctx, &mut app, at, screen);
        let head = |name: &str| {
            shown
                .iter()
                .find(|t| t.0 == name)
                .map(|t| (t.1, t.2))
                .unwrap_or_else(|| panic!("{w}: no {name}: {shown:?}"))
        };
        let hours = head("Hours").0;
        assert!(
            (hours.left() - plot_left).abs() < 1.0,
            "{w}: table at {}, plots at {plot_left}",
            hours.left()
        );
        // Each hour's row starts there too.
        let rows: Vec<f32> = shown
            .iter()
            .filter(|t| t.0.ends_with(" h") && t.0.contains('–'))
            .map(|t| t.1.left())
            .collect();
        assert!(!rows.is_empty(), "{w}: {shown:?}");
        assert!(
            rows.iter().all(|x| (x - plot_left).abs() < 1.0),
            "{w}: {rows:?} {plot_left}"
        );
        // No wider than the plots, scrolling sideways when it would be;
        // units in the titles, not the cells.
        let read = head("%").1;
        assert!(
            read.right() <= plot_right + 1.0,
            "{w}: {read:?} {plot_right}"
        );
        for title in ["Rate", "s/d", "Amplitude", "°", "Beat Error", "ms"] {
            head(title);
        }
        assert!(
            !shown.iter().any(|t| t.1.top() > ran.bottom()
                && t.1.left() >= plot_left - 1.0
                && t.0 != "%"
                && (t.0.ends_with(" ms") || t.0.ends_with('%'))),
            "{w}: {shown:?}"
        );
    }
}

/// Quick Listen asks before unsaved sound goes, then shows Unsorted
/// and listens with no watch picked; Stop keeps nothing, Save… saves
/// the take into Unsorted, which the list shows at its top, and File
/// Under a Watch moves it into a watch. Picking a watch ends it.
#[test]
fn a_power_reserve_test_holds_its_position_and_keeps_its_take_with_its_run() {
    let base = std::env::temp_dir().join(format!("tg-app-reserve-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.pick_watch(&pepsi);
    // 20 s of sound kept, as a run would keep it (no device is opened).
    app.session.listen();
    let info = app.info("", 48000, 16);
    let mut r = Recorder::start(&base.join("unsaved"), info).unwrap();
    r.write(&timegrapher_core::capture::Block {
        samples: vec![0.0; 48000 * 20],
        at: std::time::SystemTime::now(),
    })
    .unwrap();
    app.session.recorder = Some(r);
    app.position = Position::CrownLeft;
    let mut t = reserve::Test::new(
        Position::CrownLeft,
        true,
        "2026-10-22T16:15:00Z".into(),
        0.0,
    );
    let beats: Vec<timegrapher_core::beats::Beat> = (0..100)
        .map(|i| timegrapher_core::beats::Beat {
            index: i,
            time: 1.0 + i as f64 * 0.125,
            quality: 1.0,
        })
        .collect();
    t.update(&beats, 0.125, 20.0);
    app.reserve = Some(t);
    assert!(app.reserve_running());
    assert!(!app.position_editable());
    assert!(app.position_lock_words().contains("Stop Test"));
    // One position for the whole run.
    app.change_position(Position::DialUp);
    assert_eq!(app.position, Position::CrownLeft);

    // Stop ends the test and keeps the take in the watch's history.
    app.stop_listening();
    assert!(!app.reserve_running());
    assert!(app.reserve.as_ref().unwrap().saving);
    app.wait_for_save();
    app.reserve_saved();
    let saved = app.session.saved_to().unwrap().to_path_buf();
    assert_eq!(app.reserve.as_ref().unwrap().saved.as_ref(), Some(&saved));
    assert!(
        saved.starts_with(root.join("Pepsi").join(library::TAKES)),
        "{saved:?}"
    );
    assert!(file_name(&saved).ends_with(" Power Reserve"), "{saved:?}");
    let r = power_reserve::Results::from_json(
        &std::fs::read_to_string(saved.join(power_reserve::FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(r.ending, power_reserve::Ending::ByHand);
    assert_eq!(r.position, "CL");
    assert!((r.run_s - 99.0 * 0.125).abs() < 1e-6, "{}", r.run_s);
    let take = app.open_watch().unwrap().takes[0].clone();
    assert!(watches::take_line(&take, false, None)
        .starts_with("Power Reserve 12 s · Stopped by hand while the watch still ran"));
    // Opened again, the take brings its run back to the pane.
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.start_batch(&saved);
    let back = again.reserve.as_ref().expect("its results");
    assert!(!again.reserve_running());
    assert_eq!(back.results.as_ref(), Some(&r));
    assert_eq!(back.position, Position::CrownLeft);
    let _ = std::fs::remove_dir_all(&base);
}

/// A test in Crown Left with 100 beats heard.
fn crown_left_test() -> reserve::Test {
    let mut t = reserve::Test::new(
        Position::CrownLeft,
        false,
        "2026-10-22T16:15:00Z".into(),
        0.0,
    );
    let beats: Vec<timegrapher_core::beats::Beat> = (0..100)
        .map(|i| timegrapher_core::beats::Beat {
            index: i,
            time: 1.0 + i as f64 * 0.125,
            quality: 1.0,
        })
        .collect();
    t.update(&beats, 0.125, 20.0);
    t
}

/// The save under way takes 300 ms longer to finish.
fn slow_save(app: &mut TimegrapherApp) {
    let mut s = app.saving.take().expect("a save into its folder");
    let copy = s.thread;
    s.thread = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        copy.join().unwrap()
    });
    app.saving = Some(s);
}

/// A Power Reserve Test kept as its analysis only, recording into its
/// folder as it goes and stopped by hand, has its sound removed once its
/// take is written, with no input: the window draws again while the keep
/// runs, so it is collected (#1221, Walter's find).
#[test]
fn a_power_reserve_test_kept_as_analysis_only_lets_its_sound_go() {
    let base = std::env::temp_dir().join(format!("tg-app-reserve-ao-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.pick_watch(&pepsi);
    app.session.listen();
    let info = app.info("", 48000, 16);
    let mut r = Recorder::start(&base.join("unsaved"), info).unwrap();
    let start = std::time::SystemTime::now();
    // A watch ticking, so its analysis has beats to keep.
    let cfg = SynthConfig {
        duration_s: 28.0,
        ..Default::default()
    };
    let ticks = generate(&cfg, |_| 280.0, |_| 0.0).samples;
    let block = |s: u64, at: u64| timegrapher_core::capture::Block {
        samples: ticks[48000 * at as usize..48000 * (at + s) as usize].to_vec(),
        at: start + std::time::Duration::from_secs(at),
    };
    r.write(&block(20, 0)).unwrap();
    app.session.recorder = Some(r);
    let _tx = super::tests::queued_capture(&mut app, &base);
    app.position = Position::CrownLeft;
    app.reserve = Some(crown_left_test());
    assert!(app.streams(), "a test of a watch picked records as it goes");
    app.stream_tick();
    app.wait_for_save();
    let take = app
        .session
        .saved_to()
        .expect("recording into its folder")
        .to_path_buf();
    app.session
        .recorder
        .as_mut()
        .unwrap()
        .write(&block(6, 20))
        .unwrap();
    // The next save into its folder still being written at the Stop.
    app.stream_tick();
    slow_save(&mut app);
    let rec = app.session.recorder.as_mut().unwrap();
    rec.write(&block(2, 26)).unwrap();
    app.stop_listening();
    // As the frames run: the end save written, then the sound let go.
    for _ in 0..500 {
        app.poll_saving();
        app.reserve_saved();
        if app.take_keep.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        app.take_keep.is_some(),
        "keeping its analysis only, {:?}",
        app.message
    );
    assert_eq!(app.session.saved_to(), Some(take.as_path()));
    assert!(!app.running());
    assert!(
        app.repaint_every().is_some(),
        "an idle window draws again to collect the keep"
    );
    for _ in 0..1000 {
        app.poll_take_keep();
        if app.take_keep.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let left: Vec<_> = std::fs::read_dir(&take)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(
        !left
            .iter()
            .any(|n| n.to_string_lossy().starts_with("audio-")),
        "{left:?} {:?}",
        app.message
    );
    app.session.discard_recording();
    let _ = std::fs::remove_dir_all(&base);
}

/// A Power Reserve Test started with a take open runs in the position
/// the card and the pane named, not the one listening had before the
/// take was opened; and the status bar names the watch the Calibre
/// card names.
#[test]
fn a_power_reserve_test_over_an_opened_take_keeps_the_cards_position_and_watch() {
    let base = std::env::temp_dir().join(format!("tg-app-reserve-pos-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Daytona")).unwrap();
    let id = lib.find("Daytona").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root));
    app.pick_watch(&id);
    // A take opened in Dial Down over listening's Dial Up.
    app.reset_session(48000, "take".into());
    app.input = Input::File;
    app.opened_path = "/x/take.wav".into();
    app.mic_setup = Some(WatchSetup::default());
    app.position = Position::DialDown;
    // The test's own session, short of opening a real input.
    app.reserve_session();
    assert_eq!(app.position, Position::DialDown);
    assert_eq!(app.input, Input::Microphone);
    // The take's own watch name gone, as after a test's Stop.
    app.watch.clear();
    let words = app.status_words();
    assert!(words.contains("Daytona"), "{words}");
    assert!(!words.contains("No watch"), "{words}");
    let _ = std::fs::remove_dir_all(&base);
}

/// A Power Reserve run longer than the beats the Bench keeps (about a
/// day) still starts at its first beat: the engine keeps every beat of
/// the run, so its run time and hours aren't read from its last day.
#[test]
fn a_power_reserve_run_keeps_its_first_beat_past_the_benchs_limit() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "syn".into());
    // The Bench's limit made small, as a day's beats are to a long run.
    app.live.as_mut().unwrap().set_max_beats(100);
    app.begin_reserve();
    let cfg = SynthConfig {
        duration_s: 40.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let live = app.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        live.push(b);
    }
    let beats = live.beats().to_vec();
    let now = live.duration_s();
    assert!(beats.len() > 200, "{}", beats.len());
    let t = app.reserve.as_mut().unwrap();
    assert!(t.update(&beats, 0.125, now).is_none());
    let first = t.heard.first_s.unwrap();
    assert!(first < 3.0, "{first}");
    assert!(t.run_so_far_s() > 35.0, "{}", t.run_so_far_s());
}

#[test]
fn a_power_reserve_test_runs_in_the_cards_position() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.change_position(Position::CrownUp);
    app.begin_reserve();
    assert_eq!(app.reserve.as_ref().unwrap().position, Position::CrownUp);
    assert_eq!(app.position, Position::CrownUp);
    // The pane names that position and has no picker of its own.
    let mut texts = Vec::new();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    app.reserve = None;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| app.reserve_settings(ui));
        });
    });
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| app.reserve_settings(ui));
        });
    });
    for c in &out.shapes {
        if let egui::Shape::Text(t) = &c.shape {
            texts.push(t.galley.text().to_string());
        }
    }
    assert!(
        texts
            .iter()
            .any(|t| t == "Runs in Crown Up (CU), set on the Current Position card"),
        "{texts:?}"
    );
    assert!(!texts.iter().any(|t| t == "Position"), "{texts:?}");
    // A compact card is titled Position: the words name it so.
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.position_card(ui, true));
    });
    assert!(painted_text(&out).contains("Position"));
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| app.reserve_settings(ui));
        });
    });
    assert!(
        painted_text(&out).contains("Runs in Crown Up (CU), set on the Position card"),
        "{}",
        painted_text(&out)
    );
    // And the steps before a run.
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            reserve::pane(ui, None, None, (app.position_card_title, ""), true, false);
        });
    });
    assert!(
        painted_text(&out)
            .contains("set the position on the Position card, then press Start Test."),
        "{}",
        painted_text(&out)
    );
}

/// The chart measured again as the run goes on keeps its whole hours and
/// measures the rest on core's grid of true hours, so after several
/// refreshes it holds the hours a fresh read of the run gives, across a
/// pause ending on an hour or inside one (#642; the outside review of
/// #876 found 30 min left out).
#[test]
fn the_live_hours_match_a_fresh_read_after_each_refresh() {
    use timegrapher_core::{beats::Beat, breaks::Break, stream::BeatLog};
    let log = |duration_s: f64| BeatLog {
        sample_rate: 48_000,
        duration_s,
        bph: 28_800,
        lift_deg: 52.0,
        beats: (0..(duration_s * 8.0) as i64)
            .map(|index| Beat {
                index,
                time: index as f64 * 0.125,
                quality: 1.0,
            })
            .collect(),
        amplitude_windows: vec![],
        clipped_beats: vec![],
        clipped_samples: 0,
        jumps: vec![],
        sided: vec![],
        by_count: vec![],
        snippets: vec![],
    };
    for gap_s in [1800.0, 2700.0] {
        let breaks = vec![Break {
            audio_s: 1800.0,
            gap_s,
        }];
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.reserve = Some(reserve::Test::new(
            Position::DialUp,
            true,
            String::new(),
            0.0,
        ));
        app.reserve_read_s = 0.0;
        app.reserve_hours_s = f64::NEG_INFINITY;
        app.put_take_clock(None, None);
        app.put_take_breaks(Some(breaks.clone()), None);
        let ctx = egui::Context::default();
        let mut last = 0.0;
        for duration_s in [5000.0, 5060.0, 6800.0, 6860.0] {
            app.live = Some(LiveAnalyzer::from_log(
                log(duration_s),
                LiveConfig::default(),
            ));
            app.reserve_tick(&ctx);
            last = duration_s;
        }
        let t = app.reserve.as_ref().unwrap();
        let fresh = power_reserve::hours(
            &log(last),
            None,
            &breaks,
            &[],
            t.heard.first_s.unwrap(),
            t.heard.last_s.unwrap(),
        );
        let live: Vec<_> = t.so_far.iter().map(|h| (h.hour, h.span_s)).collect();
        let want: Vec<_> = fresh.iter().map(|h| (h.hour, h.span_s)).collect();
        assert_eq!(live.len(), want.len(), "gap {gap_s}: {live:?} vs {want:?}");
        for (a, b) in live.iter().zip(&want) {
            assert!(
                a.0 == b.0 && (a.1 - b.1).abs() < 1e-3,
                "gap {gap_s}: {live:?} vs {want:?}"
            );
        }
    }
}

/// A take of `run_s` of beats at 3,600 bph (few, so the test is quick),
/// then a minute of silence.
fn long_log(run_s: f64) -> BeatLog {
    let n = run_s as i64;
    BeatLog {
        sample_rate: 48000,
        duration_s: run_s + 60.0,
        bph: 3600,
        lift_deg: 52.0,
        beats: (0..n)
            .map(|index| timegrapher_core::beats::Beat {
                index,
                time: index as f64,
                quality: 1.0,
            })
            .collect(),
        amplitude_windows: vec![],
        clipped_beats: vec![],
        clipped_samples: 0,
        jumps: vec![],
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    }
}

/// A long take that isn't a test fills the Power Reserve pane with its
/// run, hour by hour, saying where it came from and keeping nothing; a
/// short one leaves the pane as before a test.
#[test]
fn a_long_take_shows_its_run_in_the_power_reserve_pane() {
    let run = power_reserve::LONG_TAKE_S + 1800.0;
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.show_log(long_log(run), "long".into(), vec![], vec![]);
    let t = app.reserve.as_ref().expect("the run of a long take");
    assert!(!app.reserve_running());
    assert!(t.saved.is_none(), "nothing kept");
    let r = t.results.as_ref().unwrap();
    assert_eq!(r.source, power_reserve::Source::LongTake);
    assert_eq!(r.ending, power_reserve::Ending::TakeEnded);
    assert_eq!(r.hours.len(), 7);
    let text = reserve::results_text(r, "", ',');
    assert!(text.contains("Read from,\"Read from a long take, not a Power Reserve Test\""));
    assert!(!text.contains("Started (UTC)"), "no start known");
    // A run from the take's start, never a power reserve.
    assert!(text.contains("Ran from the take's start,at least 6 h 29 min"));
    assert!(!text.contains("Power reserve,"));
    assert_eq!(r.ran(), "Ran at least 6 h 29 min from the take's start");
    // Its pane, shown later, grows to show the charts and table, as an
    // opened run's take does: never a sliver.
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(1000.0, 700.0));
    let pass = |app: &mut TimegrapherApp| {
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.panels(ctx));
        }
    };
    pass(&mut app);
    app.reveal_pane(Pane::PowerReserve);
    pass(&mut app);
    let id = app.panes.tiles.find_pane(&Pane::PowerReserve).unwrap();
    let tabs = app.panes.tiles.parent_of(id).unwrap();
    let tall = app.panes.tiles.rect(tabs).unwrap().height();
    assert!(tall > MIN_PANE_H + 40.0, "{tall}");

    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.show_log(
        long_log(power_reserve::LONG_TAKE_S - 600.0),
        "short".into(),
        vec![],
        vec![],
    );
    assert!(app.reserve.is_none());
}

/// A long take lands without its charts or its run read on the window's
/// thread: the landing reads neither; the window's next frames read
/// both in the background, and what comes is what reading them on the
/// spot gives (#1014).
#[test]
fn a_long_take_lands_with_its_run_and_charts_read_in_the_background() {
    let run = power_reserve::LONG_TAKE_S + 1800.0;
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.land_log(long_log(run), "long".into(), vec![], vec![]);
    assert!(
        app.reserve.is_none(),
        "the run isn't read as the take lands"
    );
    assert!(app.trend.is_empty(), "nor are the charts");
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let start = Instant::now();
    while app.reserve.is_none() || app.bg.trend.is_some() || app.trend.is_empty() {
        app.take_reads_shown(&ctx);
        assert!(start.elapsed() < Duration::from_secs(60), "never came");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.bg.reserve.is_none());
    let mut here = TimegrapherApp::with_devices(Vec::new(), None);
    here.show_log(long_log(run), "long".into(), vec![], vec![]);
    assert_eq!(app.trend, here.trend);
    let text = |a: &TimegrapherApp| {
        let r = a.reserve.as_ref().and_then(|t| t.results.as_ref()).unwrap();
        reserve::results_text(r, "", ',')
    };
    assert_eq!(text(&app), text(&here));
}

/// A long take's run, once it has come, is read again when what it
/// rests on changes: every beat masked, it goes, as a fresh reading
/// gives none; unmasked, it comes back (#1644, from an external audit).
#[test]
fn a_long_takes_run_is_read_again_when_its_masks_change() {
    let run = power_reserve::LONG_TAKE_S + 1800.0;
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.land_log(long_log(run), "long".into(), vec![], vec![]);
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let settle = |app: &mut TimegrapherApp, shown: bool| {
        let start = Instant::now();
        loop {
            app.take_reads_shown(&ctx);
            if app.bg.reserve.is_none() && app.reserve.is_some() == shown {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(60), "never came");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    let text = |a: &TimegrapherApp| {
        let r = a.reserve.as_ref().and_then(|t| t.results.as_ref()).unwrap();
        reserve::results_text(r, "", ',')
    };
    settle(&mut app, true);
    let before = text(&app);
    app.mask(0.0, run + 60.0);
    assert!(app.long_take_reader().map(|read| read()).unwrap().is_none());
    settle(&mut app, false);
    app.set_masked(Vec::new());
    settle(&mut app, true);
    assert_eq!(text(&app), before);
}

/// An idle window draws only when asked; one listening or reading a
/// batch draws every 40 ms; work on a thread of its own, each kind on its
/// own, every 100 ms, so what it gives shows with no input (Trevor's HOLD
/// on #1232); and once more on the frame it ended (Walter's).
#[test]
fn the_window_draws_again_only_while_work_is_under_way() {
    use std::sync::mpsc::channel;
    use std::time::Duration;
    let idle = || TimegrapherApp::with_devices(Vec::new(), None);
    assert_eq!(idle().repaint_every(), None, "idle");
    // Once more at once on the frame work ended, then no more.
    assert_eq!(idle().draw_next(true), Some(Duration::ZERO));
    assert_eq!(idle().draw_next(false), None);
    let analysis = || AnalysisSave {
        rx: channel().1,
        progress: Default::default(),
        cancel: Default::default(),
        total_s: 1.0,
    };
    type Set = Box<dyn Fn(&mut TimegrapherApp)>;
    let slow: [(&str, Set); 6] = [
        (
            "sound",
            Box::new(|a| a.sound_job = Some((0.0, 0, channel().1))),
        ),
        ("scope", Box::new(|a| a.scope_job = Some(channel().1))),
        (
            "steady",
            Box::new(|a| a.steady_job = Some((Revision::default(), channel().1))),
        ),
        (
            "save",
            Box::new(|a| {
                a.saving = Some(Saving {
                    thread: std::thread::spawn(|| Err(std::io::Error::other("held"))),
                    dest: PathBuf::new(),
                    parent: PathBuf::new(),
                    secs: 0.0,
                    metadata: a.session.metadata_snapshot(a.save_metadata()),
                    rescan: false,
                    streamed: Streamed::No,
                })
            }),
        ),
        (
            "keep",
            Box::new(move |a| a.take_keep = Some((PathBuf::new(), analysis()))),
        ),
        (
            "analysis",
            Box::new(move |a| a.analysis_save = Some(analysis())),
        ),
    ];
    for (name, set) in slow {
        let mut a = idle();
        set(&mut a);
        assert_eq!(
            a.repaint_every(),
            Some(Duration::from_millis(100)),
            "{name}"
        );
        assert_eq!(a.draw_next(true), Some(Duration::from_millis(100)));
    }
    let mut a = idle();
    a.batch = Some(Batch {
        rx: channel().1,
        progress: Default::default(),
        cancel: Default::default(),
        duration_s: None,
        label: String::new(),
        started: Instant::now(),
        files: 1,
        first: String::new(),
        last: String::new(),
        stretches: Vec::new(),
        settles: Vec::new(),
        order_note: None,
    });
    assert_eq!(a.repaint_every(), Some(Duration::from_millis(40)), "batch");
    let base = std::env::temp_dir().join(format!("tg-app-repaint-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let mut a = idle();
    let _tx = super::tests::queued_capture(&mut a, &base);
    assert!(a.running());
    assert_eq!(
        a.repaint_every(),
        Some(Duration::from_millis(40)),
        "listening"
    );
    drop(a);
    let _ = std::fs::remove_dir_all(&base);
}

/// A Power Reserve Test whose recording fails (a full disk) ends there:
/// the pane no longer says it runs, its results are written into the take
/// it was recording into with that ending, no failed save is promised
/// again, and the take's row in Watches reads all its sound (#1380,
/// Walter's find).
#[test]
fn a_power_reserve_test_whose_recording_fails_keeps_its_run_so_far() {
    let base = std::env::temp_dir().join(format!("tg-app-reserve-fail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Reverso")).unwrap();
    let reverso = lib.find("Reverso").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.pick_watch(&reverso);
    app.session.listen();
    let info = app.info("", 48000, 16);
    let mut r = Recorder::start(&base.join("unsaved"), info).unwrap();
    let block = |s: usize| timegrapher_core::capture::Block {
        samples: vec![0.0; 48000 * s],
        at: std::time::SystemTime::now(),
    };
    r.write(&block(20)).unwrap();
    app.session.recorder = Some(r);
    let _tx = super::tests::queued_capture(&mut app, &base);
    app.position = Position::CrownLeft;
    app.reserve = Some(crown_left_test());
    // Recording into its folder: a first save, then a second.
    app.stream_tick();
    app.wait_for_save();
    let take = app
        .session
        .saved_to()
        .expect("recording into its folder")
        .to_path_buf();
    let rec = app.session.recorder.as_mut().unwrap();
    rec.write(&block(6)).unwrap();
    app.stream_tick();
    app.wait_for_save();
    // The third fails, as on a full disk, and is promised again.
    let rec = app.session.recorder.as_mut().unwrap();
    rec.write(&block(6)).unwrap();
    app.stream_tick();
    let mut s = app.saving.take().expect("a save into its folder");
    let copy = s.thread;
    s.thread = std::thread::spawn(move || {
        let _ = copy.join();
        Err(std::io::Error::other(
            "No space left on device (os error 28)",
        ))
    });
    app.saving = Some(s);
    app.wait_for_save();
    assert!(app.stream_wait.is_some());
    assert!(app.reserve_running());

    // Then the recording fails.
    app.recording_failed("No space left on device (os error 28)".into());
    assert!(!app.reserve_running(), "the pane no longer says Running");
    assert!(app.stream_wait.is_none(), "nothing is tried again");
    let t = app.reserve.as_ref().unwrap();
    assert_eq!(t.ending, Some(power_reserve::Ending::RecordingFailed));
    assert_eq!(t.saved.as_ref(), Some(&take));
    let kept = power_reserve::Results::from_json(
        &std::fs::read_to_string(take.join(power_reserve::FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(t.results.as_ref(), Some(&kept));
    assert_eq!(kept.ending, power_reserve::Ending::RecordingFailed);
    assert!(kept.sound_kept, "no keep as its analysis alone follows");
    assert!(kept
        .sentence("CL")
        .starts_with("Power reserve at least 12 s in CL."));
    // Its row reads the take as its folder holds it, not as first saved.
    let row = app.open_watch().unwrap().takes[0].clone();
    let length = row.duration_s.expect("a length");
    // The failing save's thread wrote its copy before failing here.
    assert!(length >= 26.0 - 0.01, "{length}");
    assert!(watches::take_line(&row, false, None).starts_with(
        "Power Reserve at least 12 s · The recording failed while the watch still ran"
    ));

    // Opened again, the take brings its run back to the pane.
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.start_batch(&take);
    let back = again.reserve.as_ref().expect("its results");
    assert!(!again.reserve_running());
    assert_eq!(back.results.as_ref(), Some(&kept));
    let _ = std::fs::remove_dir_all(&base);
}

/// A failed save into the take, once its recording has failed, promises
/// nothing more (#1380).
#[test]
fn a_save_failing_after_the_recording_failed_promises_no_retry() {
    let base = std::env::temp_dir().join(format!("tg-app-reserve-noretry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.session.listen();
    let info = app.info("", 48000, 16);
    let r = Recorder::start(&base.join("unsaved"), info).unwrap();
    app.session.recorder = Some(r);
    app.session
        .recording_failed(0.0, "No space left on device (os error 28)".into());
    app.save_failed_for_test(&base.join("take"), "No space left".into());
    assert!(app.stream_wait.is_none());
    let said = app.message.clone().unwrap().0;
    assert!(said.contains("recovery folder"), "{said}");
    assert!(!said.contains("tried again"), "{said}");
    let _ = std::fs::remove_dir_all(&base);
}

/// Results the full disk kept out of the take are written a minute
/// later, and until then tried again each minute (#1380).
#[test]
fn a_failed_runs_results_are_written_again_a_minute_later() {
    let base = std::env::temp_dir().join(format!("tg-app-reserve-again-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let take = base.join("take");
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let mut t = crown_left_test();
    t.ending = Some(power_reserve::Ending::RecordingFailed);
    t.finish(|s| s);
    let long_ago = Instant::now() - STREAM_RETRY;
    t.unwritten = Some((take.clone(), long_ago));
    app.reserve = Some(t);
    let ctx = egui::Context::default();
    // Still no room (here, no folder): tried again a minute on.
    app.reserve_tick(&ctx);
    let t = app.reserve.as_ref().unwrap();
    let (_, at) = t.unwritten.clone().expect("still to write");
    assert!(at > long_ago);
    assert!(t.saved.is_none());
    // An idle window draws for the next try on its own (#1405).
    let next = app.repaint_every().expect("a draw for the next try");
    assert!(next > STREAM_RETRY / 2 && next <= STREAM_RETRY, "{next:?}");
    // Not tried again before the minute is up.
    app.reserve_tick(&ctx);
    let (_, again) = app.reserve.as_ref().unwrap().unwritten.clone().unwrap();
    assert_eq!(again, at, "no try before the minute");
    // Three quarters of the minute gone: the draw comes at the time left.
    app.reserve.as_mut().unwrap().unwritten =
        Some((take.clone(), Instant::now() - STREAM_RETRY * 3 / 4));
    let left = app.repaint_every().expect("a draw for the next try");
    assert!(left <= STREAM_RETRY / 4, "{left:?}");
    // Room again: written.
    std::fs::create_dir_all(&take).unwrap();
    app.reserve.as_mut().unwrap().unwritten = Some((take.clone(), long_ago));
    app.reserve_tick(&ctx);
    let t = app.reserve.as_ref().unwrap();
    assert!(t.unwritten.is_none());
    assert_eq!(t.saved.as_ref(), Some(&take));
    assert_eq!(app.repaint_every(), None, "nothing left to try");
    let kept = power_reserve::Results::from_json(
        &std::fs::read_to_string(take.join(power_reserve::FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(kept.ending, power_reserve::Ending::RecordingFailed);
    let _ = std::fs::remove_dir_all(&base);
}

/// A run on an automatic rate that reaches the watch's stop keeps its
/// hour table: the silence before the stop lets the rate go, and the
/// hours are read at the rate the run ran at (#1394, Walter's find).
#[test]
fn a_run_on_an_automatic_rate_keeps_its_hours_at_the_stop() {
    let cfg = SynthConfig {
        duration_s: 90.0,
        bph: 21_600,
        ..Default::default()
    };
    let ticks = generate(&cfg, |_| 280.0, |_| 0.0).samples;
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let auto = LiveConfig::default();
    assert!(auto.analysis.bph.is_none(), "the rate on Auto");
    app.live = Some(LiveAnalyzer::new(48_000, auto));
    app.reserve = Some(reserve::Test::new(
        Position::DialUp,
        true,
        String::new(),
        0.0,
    ));
    app.reserve_read_s = 0.0;
    let ctx = egui::Context::default();
    // Then silence, in a few long blocks so the test is quick.
    let silence = vec![0.0f32; 48_000 * 20];
    let sound = ticks.chunks(48_000).chain(std::iter::repeat_n(
        silence.as_slice(),
        (power_reserve::STOP_AFTER_S / 20.0) as usize + 3,
    ));
    for s in sound {
        app.live.as_mut().unwrap().push(s);
        app.reserve_tick(&ctx);
        if !app.reserve_running() {
            break;
        }
    }
    assert_eq!(app.live.as_ref().unwrap().bph(), None, "the rate let go");
    let t = app.reserve.as_ref().unwrap();
    assert_eq!(t.ending, Some(power_reserve::Ending::Stopped));
    assert_eq!(t.bph, Some(21_600));
    let r = t.results.as_ref().expect("its results");
    assert!(!r.hours.is_empty(), "its hour table");
}

/// With the window not drawing (minimised, a locked screen), the
/// listening thread alone ends a Power Reserve Test at the watch's stop:
/// no frame is drawn, and the test still ends there (#1647, from an
/// external audit).
#[test]
fn the_listening_thread_ends_a_power_reserve_test_without_frames() {
    let base = std::env::temp_dir().join(format!("tg-app-reserve-pump-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let cfg = SynthConfig {
        duration_s: 90.0,
        bph: 21_600,
        ..Default::default()
    };
    let ticks = generate(&cfg, |_| 280.0, |_| 0.0).samples;
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.live = Some(LiveAnalyzer::new(48_000, LiveConfig::default()));
    let tx = queued_capture(&mut app, &base);
    app.reserve = Some(reserve::Test::new(
        Position::DialUp,
        true,
        String::new(),
        0.0,
    ));
    app.reserve_read_s = 0.0;
    let silence = vec![0.0f32; 48_000 * 20];
    let sound = ticks.chunks(48_000).chain(std::iter::repeat_n(
        silence.as_slice(),
        (power_reserve::STOP_AFTER_S / 20.0) as usize + 3,
    ));
    for s in sound {
        app.live.as_mut().unwrap().push(s);
        // Pushed straight in, so mark it as arrived: otherwise a slow run
        // finds the input silent for 3 s and stops listening first.
        app.audio_at = Some(Instant::now());
        // The listening thread's turn; never a frame's.
        app.pump();
        if !app.reserve_running() {
            break;
        }
    }
    let t = app.reserve.as_ref().unwrap();
    assert_eq!(t.ending, Some(power_reserve::Ending::Stopped));
    assert!(t.results.is_some(), "its results");
    drop(tx);
    let _ = std::fs::remove_dir_all(&base);
}

/// While listening, a long take that isn't a test fills the Power Reserve
/// pane as an opened one does, read in the background and again a
/// minute of take later, never written into the take or naming it as a
/// test; a short one leaves the pane as before a test. Where listening
/// stops, it is read once more.
#[test]
fn a_long_quick_listen_shows_its_run_so_far_in_the_power_reserve_pane() {
    let base = std::env::temp_dir().join(format!("tg-live-reserve-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let settle = |app: &mut TimegrapherApp| {
        let start = Instant::now();
        app.take_reads_shown(&ctx);
        while app.bg.reserve_live.is_some() {
            assert!(start.elapsed() < Duration::from_secs(60), "never came");
            std::thread::sleep(Duration::from_millis(5));
            app.take_reads_shown(&ctx);
        }
    };

    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.land_log(
        long_log(power_reserve::LONG_TAKE_S - 600.0),
        "short".into(),
        vec![],
        vec![],
    );
    let _tx = queued_capture(&mut app, &base);
    assert!(app.running());
    settle(&mut app);
    assert!(app.reserve.is_none(), "a short take leaves the pane");

    let run = power_reserve::LONG_TAKE_S + 1800.0;
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.land_log(long_log(run), "long".into(), vec![], vec![]);
    let _tx = queued_capture(&mut app, &base);
    settle(&mut app);
    assert!(!app.reserve_running());
    assert!(!app.reserve_is_test());
    let r = app
        .reserve
        .as_ref()
        .and_then(|t| t.results.as_ref())
        .unwrap();
    assert_eq!(r.source, power_reserve::Source::LongTake);
    assert_eq!(r.hours.len(), 7);
    // A run so far: the take hasn't ended (Walter's HOLD on #1475).
    assert_eq!(r.ending, power_reserve::Ending::Listening);
    assert_eq!(
        app.reserve.as_ref().unwrap().ending,
        Some(power_reserve::Ending::Listening)
    );
    assert!(!r.sentence("DU").contains("The take ended"));
    let mut here = TimegrapherApp::with_devices(Vec::new(), None);
    here.show_log(long_log(run), "long".into(), vec![], vec![]);
    let text = |a: &TimegrapherApp| {
        let r = a.reserve.as_ref().and_then(|t| t.results.as_ref()).unwrap();
        reserve::results_text(r, "", ',')
    };
    let so_far = |a: &TimegrapherApp| {
        let r = a.reserve.as_ref().and_then(|t| t.results.as_ref()).unwrap();
        reserve::results_text(&r.clone().still_listening(), "", ',')
    };
    assert_eq!(text(&app), so_far(&here), "as the opened take reads it");
    // Nothing of it goes into the take.
    assert!(app.results_files().unwrap().is_empty());
    // Not read again until a minute of take has come.
    app.take_reads_shown(&ctx);
    assert!(app.bg.reserve_live.is_none());
    app.bg.reserve_live_s -= power_reserve::READ_AGAIN_S;
    app.take_reads_shown(&ctx);
    assert!(app.bg.reserve_live.is_some(), "read again a minute on");
    settle(&mut app);
    assert_eq!(text(&app), so_far(&here));
    // Stopped: read once more, and kept.
    app.capture = None;
    assert!(app.bg.reserve.is_some());
    let start = Instant::now();
    while app.bg.reserve.is_some() {
        assert!(start.elapsed() < Duration::from_secs(60), "never came");
        std::thread::sleep(Duration::from_millis(5));
        app.take_reads_shown(&ctx);
    }
    assert_eq!(text(&app), text(&here));
    let _ = std::fs::remove_dir_all(&base);
}

/// The Power Reserve pane's Info stays as it was left, through the
/// settings, as the Steadiness panes' does (Trevor's review of #1538).
#[test]
fn the_reserve_info_is_kept_in_the_settings() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    assert!(!app.reserve_info);
    app.reserve_info = true;
    let mut back = TimegrapherApp::with_devices(Vec::new(), None);
    back.apply_settings(app.settings());
    assert!(back.reserve_info);
    app.reserve_info = false;
    back.apply_settings(app.settings());
    assert!(!back.reserve_info);
    // The Positions Test pane's Info, the same way (#1528).
    assert!(!back.positions_info);
    app.positions_info = true;
    back.apply_settings(app.settings());
    assert!(back.positions_info && !back.reserve_info);
}

/// A Power Reserve Test with Analysis Only picked, its 28 s of a ticking
/// watch sent from the microphone, then `end` (Stop, or the recording
/// failing): the take it keeps in the watch's takes.
fn analysis_only_run(tag: &str, end: fn(&mut TimegrapherApp)) -> (PathBuf, PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("tg-app-reserve-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.pick_watch(&pepsi);
    app.reset_session(48000, "test".into());
    app.session.listen();
    let recording = base.join("unsaved");
    let info = app.info("", 48000, 16);
    app.session.recorder = Some(Recorder::start(&recording, info).unwrap());
    let tx = queued_capture(&mut app, &base);
    app.position = Position::CrownLeft;
    app.reserve_keep_sound = false;
    app.begin_reserve();
    assert!(app.kept_running(), "{:?}", app.message);
    assert!(!app.reserve.as_ref().unwrap().keep_sound);
    assert!(!app.streams(), "no sound goes into the take's folder");
    let cfg = SynthConfig {
        duration_s: 28.0,
        ..Default::default()
    };
    let ticks = generate(&cfg, |_| 280.0, |_| 0.0).samples;
    // In the 20 ms blocks a microphone sends.
    for (k, s) in ticks.chunks(960).enumerate() {
        tx.send(Event::Audio(timegrapher_core::capture::Block {
            samples: s.to_vec(),
            at: std::time::UNIX_EPOCH + std::time::Duration::from_millis(2000 + 20 * k as u64),
        }))
        .unwrap();
    }
    for _ in 0..600 {
        app.poll_for(std::time::Duration::from_millis(200));
        // The test reads its beats as the window's frames would.
        app.reserve_follow();
        if app.heard_s() >= 27.9 {
            break;
        }
    }
    assert!(app.heard_s() >= 27.9, "{} {:?}", app.heard_s(), app.message);
    end(&mut app);
    for _ in 0..3000 {
        app.reserve_follow();
        if app.reserve.as_ref().is_some_and(|t| t.saved.is_some()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let take = app
        .reserve
        .as_ref()
        .and_then(|t| t.saved.clone())
        .unwrap_or_else(|| panic!("its take kept, {:?}", app.message));
    assert!(!app.kept_running());
    assert_eq!(app.session.saved_to(), Some(take.as_path()));
    drop(tx);
    (base, recording, take)
}

/// Every file under `dir` that is sound.
fn sound_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            found.extend(sound_files(&p));
        } else if timegrapher_core::take::is_sound(&p)
            || p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("audio-")
        {
            found.push(p);
        }
    }
    found
}

/// A Power Reserve Test kept as its analysis only reads its sound as it
/// comes and never writes it: stopped, its take holds its analysis, its
/// results and a session file saying it keeps no sound, and no sound is
/// written anywhere; opened again, it brings its run back (#1514).
#[test]
fn a_power_reserve_test_kept_as_analysis_only_never_writes_its_sound() {
    let (base, recording, take) = analysis_only_run("ao-live", TimegrapherApp::stop_listening);
    assert!(take
        .join(analysis_take::DIR)
        .join(analysis_take::HEADER)
        .is_file());
    assert!(take.join(power_reserve::FILE).is_file());
    let manifest = std::fs::read_to_string(take.join(MANIFEST)).unwrap();
    assert!(manifest.contains("audio = false"), "{manifest}");
    assert_eq!(sound_files(&take), Vec::<PathBuf>::new());
    assert_eq!(sound_files(&recording), Vec::<PathBuf>::new());
    assert!(!timegrapher_core::checkpoint::exists(&recording));
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.start_batch(&take);
    let back = again.reserve.as_ref().expect("its results");
    assert!(!back.results.as_ref().unwrap().sound_kept);
    let _ = std::fs::remove_dir_all(&base);
}

/// A Power Reserve Test kept as its analysis only whose recording fails
/// still keeps its take as its analysis, read to the failure, not its
/// sound (#1514).
#[test]
fn a_failed_analysis_only_test_still_keeps_its_analysis() {
    let (base, _, take) = analysis_only_run("ao-fail", |a| {
        a.recording_failed("No space left on device (os error 28)".into());
    });
    assert!(take
        .join(analysis_take::DIR)
        .join(analysis_take::HEADER)
        .is_file());
    let kept = power_reserve::Results::from_json(
        &std::fs::read_to_string(take.join(power_reserve::FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(kept.ending, power_reserve::Ending::RecordingFailed);
    assert!(!kept.sound_kept);
    assert_eq!(sound_files(&take), Vec::<PathBuf>::new());
    let _ = std::fs::remove_dir_all(&base);
}
