//! Every pane drawn whole, headless: each shown pane names itself, and
//! nothing a pane draws lands outside its card. The window is drawn at a
//! few sizes and in the states a user meets first (the start screen, a
//! take still waiting for a regular beat, a take with readings), with
//! microphone names as long as macOS gives them, which the Linux and
//! Windows test machines don't have.

use super::tests::painted_text;
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

/// The sizes drawn: the size the other tests use, a 14-inch MacBook's
/// window, and a small one.
const SIZES: [(f32, f32); 3] = [(1280.0, 820.0), (1470.0, 830.0), (1000.0, 680.0)];

/// Microphones as macOS names them: a Continuity Camera iPhone, the
/// built-in one and a USB microphone, with cpal's `coreaudio:` ids.
fn mac_devices() -> Vec<InputDevice> {
    [
        (
            "coreaudio:A3210A76-4B8D-4039-8B7E-2C1F0D9E5A44",
            "Owner’s iPhone 15 Pro Max Microphone",
            true,
        ),
        (
            "coreaudio:BuiltInMicrophoneDevice",
            "MacBook Pro Microphone",
            false,
        ),
        (
            "coreaudio:AppleUSBAudioEngine:Generic:USB Audio Device:14200000:1",
            "USB Audio Device Microphone",
            false,
        ),
    ]
    .into_iter()
    .map(|(id, name, is_default)| InputDevice {
        id: id.into(),
        name: name.into(),
        manufacturer: None,
        is_default,
        default_config: None,
        supported: Vec::new(),
    })
    .collect()
}

/// A rectangle with room for anti-aliasing and rounding.
fn holds(outer: egui::Rect, inner: egui::Rect) -> bool {
    outer.expand(1.5).contains_rect(inner)
}

/// Draw the window at `size` a few times, so the layout settles, and
/// check the last frame. Returns what went wrong, one line each.
fn check(
    app: &mut TimegrapherApp,
    ctx: &egui::Context,
    size: (f32, f32),
    what: &str,
) -> Vec<String> {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(size.0, size.1));
    let mut out = None;
    for _ in 0..4 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        out = Some(ctx.run(input, |ctx| app.panels(ctx)));
    }
    let out = out.unwrap();
    let at = format!("{what} at {}×{}", size.0, size.1);
    let card = theme::palette(ctx.style().visuals.dark_mode).card;

    // The panes' cards, as `PaneBehavior::pane_ui` paints them, and the cards inside them left out.
    let mut cards: Vec<egui::Rect> = Vec::new();
    for c in &out.shapes {
        if let egui::Shape::Rect(r) = &c.shape {
            if r.fill == card && r.corner_radius == egui::CornerRadius::same(theme::CARD_RADIUS) {
                cards.push(r.rect);
            }
        }
    }
    let outer: Vec<egui::Rect> = cards
        .iter()
        .copied()
        .filter(|r| !cards.iter().any(|o| o != r && holds(*o, *r)))
        .collect();
    let mut wrong = Vec::new();
    if outer.is_empty() {
        wrong.push(format!("{at}: no pane drawn"));
        return wrong;
    }
    for (i, a) in outer.iter().enumerate() {
        if !holds(screen, *a) {
            wrong.push(format!("{at}: a pane at {a:?} runs off the window"));
        }
        for b in &outer[i + 1..] {
            if a.shrink(1.5).intersects(b.shrink(1.5)) {
                wrong.push(format!("{at}: panes at {a:?} and {b:?} overlap"));
            }
        }
    }
    // Anything that crosses a pane's left or right edge has spilled out
    // of it: a pane scrolls up and down, never sideways.
    let spills = |r: egui::Rect| {
        outer.iter().copied().find(|o| {
            let beside = r.top() < o.bottom() - 4.0 && r.bottom() > o.top() + 4.0;
            let crosses = (r.left() < o.left() - 1.5 && r.right() > o.left() + 1.5)
                || (r.left() < o.right() - 1.5 && r.right() > o.right() + 1.5);
            beside && crosses
        })
    };
    // What is drawn, as it shows: the part left after clipping. A pane
    // clips what is too wide for it, so this is what spills out where no
    // clip holds it, as before there is anything to show.
    for c in &out.shapes {
        if let egui::Shape::Rect(r) = &c.shape {
            if r.fill == card {
                continue;
            }
        }
        let seen = c.shape.visual_bounding_rect().intersect(c.clip_rect);
        // The window's and the panes' backgrounds.
        if !seen.is_positive() || outer.iter().any(|o| holds(seen, *o)) {
            continue;
        }
        if let Some(o) = spills(seen) {
            let what = match &c.shape {
                egui::Shape::Text(t) => format!("\"{}\"", t.galley.text()),
                _ => "a drawing".to_string(),
            };
            wrong.push(format!(
                "{at}: {what} at {seen:?} runs out of the pane at {o:?}"
            ));
        }
    }
    // Each pane on show names itself, in its tab or its own title: the
    // panes laid out, the front tab of each tab bar.
    let text: Vec<String> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect();
    for &p in Pane::ALL.iter() {
        let laid_out = || {
            let tiles = &app.bench.panes_view.panes.tiles;
            tiles.find_pane(&p).and_then(|t| tiles.rect(t)).is_some()
        };
        let drawn = pane_visible(&app.bench.panes_view.panes.tiles, p) && laid_out();
        if drawn && !text.iter().any(|t| t.starts_with(p.title())) {
            wrong.push(format!(
                "{at}: the {} pane shows without its title",
                p.title()
            ));
        }
    }
    wrong
}

/// Every pane whole on the start screen, before and after the microphones
/// are listed, and with Show Every Input's longer names.
#[test]
fn every_pane_is_whole_on_the_start_screen() {
    let ctx = {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx
    };
    let mut wrong = Vec::new();
    for size in SIZES {
        // Not listed yet: the menu names the id kept from last time.
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.bench.recording.devices_listed = false;
        app.bench.recording.device = Some(mac_devices()[0].id.clone());
        wrong.extend(check(&mut app, &ctx, size, "start, microphones not listed"));

        let mut app = TimegrapherApp::with_devices(mac_devices(), None);
        wrong.extend(check(&mut app, &ctx, size, "start, microphones listed"));
        app.bench.recording.all_inputs = true;
        wrong.extend(check(&mut app, &ctx, size, "start, Show Every Input"));
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

/// Every pane whole while a take waits for a regular beat, and once it
/// has readings.
#[test]
fn every_pane_is_whole_during_a_take() {
    let ctx = {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx
    };
    let mut quiet = TimegrapherApp::with_devices(mac_devices(), None);
    quiet.reset_session(48000, "syn".into());
    // Fifteen seconds of room noise, with no watch on the microphone.
    let live = quiet.bench.recording.live.as_mut().unwrap();
    let mut seed = 1u32;
    for _ in 0..750 {
        let block: Vec<f32> = (0..960)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 * 0.02 - 0.01
            })
            .collect();
        live.push(&block);
    }
    quiet.poll();
    assert!(quiet.no_watch_yet());

    let mut ticking = TimegrapherApp::with_devices(mac_devices(), None);
    ticking.reset_session(48000, "syn".into());
    let audio = generate(
        &SynthConfig {
            duration_s: 12.0,
            rate_s_per_day: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let live = ticking.bench.recording.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        if live.push(b) {
            let r = live.reading(ticking.bench.take.average_s);
            ticking
                .bench
                .results
                .trend
                .push(TrendPoint::of(r.time_s, &r));
        }
    }
    ticking.poll();

    let mut wrong = Vec::new();
    for size in SIZES {
        wrong.extend(check(&mut quiet, &ctx, size, "a take with no beat yet"));
        wrong.extend(check(&mut ticking, &ctx, size, "a take with readings"));
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

/// Every pane whole with a recording opened, analysed whole: the Time
/// slider shows under the cards, and the readings carry their whole-take
/// lines.
#[test]
fn every_pane_is_whole_with_a_recording_opened() {
    let ctx = {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx
    };
    let dir = std::env::temp_dir().join(format!("tg-app-layout-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.wav");
    let audio = generate(
        &SynthConfig {
            duration_s: 30.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&path, &audio).unwrap();
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.start_batch(&path);
    let t = std::time::Instant::now();
    while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 120 {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(app.bench.recording.live.is_some(), "{:?}", app.message);
    let mut wrong = Vec::new();
    for size in SIZES {
        wrong.extend(check(&mut app, &ctx, size, "a recording opened"));
    }
    // With the charts and the distributions open too, no pane is crushed:
    // each laid out keeps room for a plot.
    for p in [Pane::Charts, Pane::Distributions] {
        set_pane_visible(&mut app.bench.panes_view.panes.tiles, p, true);
    }
    for size in [(1000.0, 700.0), (1440.0, 900.0)] {
        wrong.extend(check(&mut app, &ctx, size, "charts and distributions open"));
        for &p in Pane::ALL.iter() {
            let tiles = &app.bench.panes_view.panes.tiles;
            let Some(r) = tiles.find_pane(&p).and_then(|t| tiles.rect(t)) else {
                continue;
            };
            // The least is the pane with its tab bar (24 pt).
            if pane_visible(tiles, p) && r.height() < MIN_PANE_H - 24.0 {
                wrong.push(format!(
                    "{} at {}×{}: {:.0} pt tall",
                    p.title(),
                    size.0,
                    size.1,
                    r.height()
                ));
            }
        }
    }
    // Each in front in turn has room for its toolbar and one readable
    // plot, a distribution's about 60 pt.
    for front in [Pane::Charts, Pane::Distributions] {
        app.show_pane(front, true);
        for size in [(1000.0, 700.0), (1440.0, 900.0)] {
            wrong.extend(check(&mut app, &ctx, size, "charts and distributions open"));
            let tiles = &app.bench.panes_view.panes.tiles;
            let r = tiles.find_pane(&front).and_then(|t| tiles.rect(t));
            let need = app.bench.panes_view.pane_need.get(&front).copied();
            match (r, need) {
                (Some(r), Some(need)) if r.height() + 1.0 >= need => {}
                _ => wrong.push(format!(
                    "{} at {}×{}: {:?} for a need of {need:?}",
                    front.title(),
                    size.0,
                    size.1,
                    r.map(|r| r.height())
                )),
            }
        }
    }
    drop(app);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

/// The Set Up the Microphone sheet, where the microphone menu now lives
/// in the window, holds macOS's long names: nothing it draws runs out of
/// it, at each size, with and without Show Every Input.
#[test]
fn the_set_up_the_microphone_sheet_holds_long_microphone_names() {
    let ctx = {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx
    };
    let mut wrong = Vec::new();
    for size in SIZES {
        for all in [false, true] {
            let mut app = TimegrapherApp::with_devices(mac_devices(), None);
            app.bench.recording.all_inputs = all;
            app.setup_sheet = true;
            // Already tried: a test never opens an input.
            app.setup_tried = true;
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(size.0, size.1));
            let mut out = None;
            for _ in 0..4 {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                };
                out = Some(ctx.run(input, |ctx| {
                    app.panels(ctx);
                    app.setup_sheet_dialog(ctx);
                }));
            }
            assert!(!app.running(), "the sheet opened an input");
            let out = out.unwrap();
            let at = format!("Show Every Input {all} at {}×{}", size.0, size.1);
            // What the sheet draws comes after its backdrop, which covers
            // the window; its title starts its 440-point column.
            let Some(backdrop) = out.shapes.iter().rposition(
                |c| matches!(&c.shape, egui::Shape::Rect(r) if r.rect.contains_rect(screen)),
            ) else {
                wrong.push(format!("{at}: no sheet"));
                continue;
            };
            let Some(left) = out.shapes[backdrop + 1..]
                .iter()
                .find_map(|c| match &c.shape {
                    egui::Shape::Text(t) if t.galley.text() == "Set Up the Microphone" => {
                        Some(c.shape.visual_bounding_rect().left())
                    }
                    _ => None,
                })
            else {
                wrong.push(format!("{at}: no sheet"));
                continue;
            };
            let sheet = egui::Rect::from_x_y_ranges(left..=left + 440.0, screen.y_range());
            let mut texts = Vec::new();
            for c in &out.shapes[backdrop + 1..] {
                let seen = c.shape.visual_bounding_rect().intersect(c.clip_rect);
                if let egui::Shape::Text(t) = &c.shape {
                    texts.push(t.galley.text().to_string());
                    if seen.is_positive() && !holds(sheet, seen) {
                        wrong.push(format!(
                            "{at}: \"{}\" at {seen:?} runs out of the sheet at {sheet:?}",
                            t.galley.text()
                        ));
                    }
                }
            }
            if !texts.iter().any(|t| t.contains("iPhone 15 Pro Max")) {
                wrong.push(format!(
                    "{at}: the microphone's name isn't shown: {texts:?}"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

/// Each pane's toolbar keeps to one row inside its pane, as wide as the
/// window at 1000 pt and as narrow as 420 pt: what doesn't fit folds into
/// a "…" menu at the row's end, which shows only then, and every control
/// is either in the row or in the menu, never both. The Watches view's
/// New Watch… never folds.
#[test]
#[expect(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn every_toolbar_keeps_to_one_row_and_folds_only_when_narrow() {
    let ctx = {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx
    };
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.reset_session(48000, "syn".into());
    let audio = generate(
        &SynthConfig {
            duration_s: 12.0,
            rate_s_per_day: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let live = app.bench.recording.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        if live.push(b) {
            let r = live.reading(app.bench.take.average_s);
            app.bench.results.trend.push(TrendPoint::of(r.time_s, &r));
        }
    }
    app.poll();
    // The Positions Test's table of the take is read in the background:
    // wait for it, so its toolbar is checked with its Series switches on
    // every machine, however fast.
    for _ in 0..500 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::splat(1000.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.pane_tools(ui, Pane::PositionsTest);
            });
        });
        if app.free_table_shown(&ctx).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        app.free_table_shown(&ctx).is_some(),
        "the take's table was never read"
    );

    let mut wrong = Vec::new();
    let mut folded_at_narrow = Vec::new();
    for &p in Pane::ALL.iter() {
        // Wide, narrow, then wide again: what folded comes back out.
        for w in [1000.0, 420.0, 1000.0] {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, 700.0));
            let mut out = None;
            let mut drew = false;
            for _ in 0..4 {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                };
                out = Some(ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        drew = app.pane_tools(ui, p);
                    });
                }));
            }
            if !drew {
                continue;
            }
            let out = out.unwrap();
            let at = format!("{} at {w} pt", p.title());
            let prefix = format!("{}/", p.title());
            let tools: Vec<(String, egui::Rect)> = theme::tools_drawn_at(&ctx)
                .into_iter()
                .filter_map(|(k, r)| k.strip_prefix(&prefix).map(|k| (k.to_string(), r)))
                .collect();
            if tools.is_empty() {
                wrong.push(format!("{at}: nothing drawn"));
                continue;
            }
            // One row: everything painted lies in one band no taller than
            // a control, inside the window's width.
            let mut band = egui::Rect::NOTHING;
            for c in &out.shapes {
                // An empty text, as a field with nothing in it leaves, shows
                // nothing whatever its bounds.
                if matches!(&c.shape, egui::Shape::Text(t) if t.galley.text().is_empty()) {
                    continue;
                }
                let seen = c.shape.visual_bounding_rect().intersect(c.clip_rect);
                if !seen.is_positive() || seen.contains_rect(screen.shrink(20.0)) {
                    continue;
                }
                if seen.left() < screen.left() - 0.5 || seen.right() > screen.right() + 0.5 {
                    wrong.push(format!("{at}: a drawing at {seen:?} is outside the pane"));
                }
                band = band.union(seen);
            }
            if band.height() > 34.0 {
                wrong.push(format!(
                    "{at}: the toolbar is {} pt tall, more than one row: {tools:?}",
                    band.height()
                ));
            }
            for (k, r) in &tools {
                if (r.center().y - tools[0].1.center().y).abs() > 3.0 {
                    wrong.push(format!("{at}: {k} at {r:?} is off the row"));
                }
            }
            let keys: Vec<&str> = tools.iter().map(|(k, _)| k.as_str()).collect();
            let more = keys.contains(&"…");
            if w >= 1000.0 && more {
                wrong.push(format!(
                    "{at}: a \"…\" menu where everything fits: {keys:?}"
                ));
            }
            if w < 1000.0 && more {
                folded_at_narrow.push(p);
            }
            // The Watches view's main action keeps its words in the row
            // however narrow, before the search: the empty list names it.
            if p == Pane::Watch && !keys.contains(&"New Watch…") {
                wrong.push(format!("{at}: New Watch… is not in the row: {keys:?}"));
            }
            // Export is an action, and actions never fold.
            if p == Pane::Strip && !keys.contains(&"Export…") {
                wrong.push(format!("{at}: Export… is not in the row: {keys:?}"));
            }
            // Search stays in the row too, at 1000 pt and narrower: Sort
            // folds before it.
            if p == Pane::Watch && !keys.contains(&"Search") {
                wrong.push(format!("{at}: Search is not in the row: {keys:?}"));
            }
            if p == Pane::Watch && more && keys.contains(&"Sort") {
                wrong.push(format!("{at}: something folded but Sort didn't: {keys:?}"));
            }
            let mut seen_keys = keys.clone();
            seen_keys.sort_unstable();
            seen_keys.dedup();
            if seen_keys.len() != keys.len() {
                wrong.push(format!("{at}: a control drawn twice: {keys:?}"));
            }
        }
    }
    // The pane with the most settings folds some at 420 pt. (The Tick Tock
    // Profile's fit at 420 since its own Average Over went.)
    if !folded_at_narrow.contains(&Pane::Strip) {
        wrong.push(format!("{} at 420 pt: no \"…\" menu", Pane::Strip.title()));
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

/// A choice between two settings is one switch in a toolbar, an icon's
/// width, not a pair of icons: Show on the distributions, Layout on the
/// profile, and the rest. (The strip's Direction is the one labelled
/// two-part control, Down | Across: see
/// `the_strip_direction_is_down_or_across_in_words`.)
#[test]
fn a_two_way_choice_is_one_icon_wide() {
    let ctx = {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx
    };
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.reset_session(48000, "syn".into());
    let mut seen = Vec::new();
    for &p in Pane::ALL.iter() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1440.0, 700.0));
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.pane_tools(ui, p);
                });
            });
        }
        let prefix = format!("{}/", p.title());
        for (k, r) in theme::tools_drawn_at(&ctx) {
            if let Some(name) = k.strip_prefix(&prefix) {
                if ["Show", "Values", "Layout", "dB Scale", "Time From", "Keep"].contains(&name) {
                    assert!(r.width() < 30.0, "{k} is {} pt wide", r.width());
                    seen.push(name.to_string());
                }
            }
        }
    }
    for name in ["Show", "Values", "Layout", "dB Scale", "Time From", "Keep"] {
        assert!(seen.iter().any(|s| s == name), "{name} not drawn: {seen:?}");
    }
}

/// The Paper Strip's toolbar, per the Bench design: Rate, Amplitude and
/// Beat Error at its left end, then Width and Length; Direction is one
/// arrow, the way the paper runs, that a click turns (Hugh on #2170, in
/// place of Down | Across in words, so Time of Day fits), Still Paper
/// right after it and Time of Day after that; Unmask All is gone (Edit ▸
/// Remove All Masks does it).
#[test]
fn the_strip_direction_is_one_arrow_before_the_papers_switches() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.reset_session(48000, "syn".into());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1440.0, 700.0));
    let mut out = None;
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        out = Some(ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.pane_tools(ui, Pane::Strip);
            });
        }));
    }
    let words: Vec<String> = out
        .unwrap()
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect();
    for w in ["Width", "Length"] {
        assert!(words.iter().any(|t| t == w), "{w} not drawn: {words:?}");
    }
    for w in ["Direction", "Down", "Across"] {
        assert!(!words.iter().any(|t| t == w), "{w} drawn: {words:?}");
    }
    let keys: Vec<String> = theme::tools_drawn_at(&ctx)
        .into_iter()
        .filter_map(|(k, _)| k.strip_prefix("Paper Strip/").map(str::to_string))
        .filter(|k| !k.starts_with('|'))
        .collect();
    assert_eq!(
        keys.first().map(String::as_str),
        Some("Draw Over the Strip"),
        "{keys:?}"
    );
    assert_eq!(keys.get(1).map(String::as_str), Some("Width"), "{keys:?}");
    assert!(!keys.iter().any(|k| k == "Unmask All"), "{keys:?}");
    let at = |k: &str| keys.iter().position(|x| x == k).unwrap();
    assert!(
        at("Centre") < at("Rate Line") && at("Rate Line") < at("Mask"),
        "{keys:?}"
    );
    assert!(at("Mask") < at("Direction"), "{keys:?}");
    assert_eq!(at("Still Paper"), at("Direction") + 1, "{keys:?}");
    assert_eq!(at("Time of Day"), at("Still Paper") + 1, "{keys:?}");
}

/// At 1000 pt the Paper Strip's row holds both of the paper's switches,
/// Still Paper and Time of Day, with nothing folded into "…" (Hugh on
/// #2170).
#[test]
fn the_strip_row_holds_still_paper_and_time_of_day_at_1000_pt() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.reset_session(48000, "syn".into());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1000.0, 700.0));
    for _ in 0..4 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.pane_tools(ui, Pane::Strip);
            });
        });
    }
    let keys: Vec<String> = theme::tools_drawn_at(&ctx)
        .into_iter()
        .filter_map(|(k, _)| k.strip_prefix("Paper Strip/").map(str::to_string))
        .collect();
    for k in ["Still Paper", "Time of Day", "Export…"] {
        assert!(keys.iter().any(|x| x == k), "{k} not in the row: {keys:?}");
    }
    assert!(!keys.iter().any(|x| x == "…"), "something folded: {keys:?}");
}

/// The strip's Width and Length fields show their widest values whole,
/// unit and all: Alt and the wheel leaves widths such as ±128.4 ms.
#[test]
fn the_width_and_length_fields_show_their_units_whole() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.reset_session(48000, "syn".into());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1440.0, 700.0));
    for (width, length) in [(888.8, 8888.0), (58.8, 35_999.0)] {
        app.bench.panes_view.strip.set_half_width(width);
        app.bench.panes_view.strip.set_span(length);
        let mut out = None;
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            out = Some(ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.pane_tools(ui, Pane::Strip);
                });
            }));
        }
        let out = out.unwrap();
        for want in [width_label(width), fields::duration(length)] {
            let (t, clip) = out
                .shapes
                .iter()
                .find_map(|c| match &c.shape {
                    egui::Shape::Text(t) if t.galley.text() == want => Some((t, c.clip_rect)),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{want} not drawn: {}", tests::painted_text(&out)));
            let right = t.pos.x + t.galley.rect.right();
            assert!(
                right <= clip.right(),
                "{want} cut at {} of {right}",
                clip.right()
            );
        }
    }
}

/// The compact Listen card's two boxes, at 1000 and 800 wide, read
/// "10 s" and "Wind" whole, as app.md says (Hugh on #2347): "Avg 10 s"
/// has no room there, and neither box's words are cut, run into the
/// other's or leave the card.
#[test]
fn the_compact_listen_boxes_read_10_s_and_wind_whole() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let card = theme::palette(ctx.style().visuals.dark_mode).card;
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    for w in [1000.0, 800.0] {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, 800.0));
        let mut out = None;
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            out = Some(ctx.run(input, |ctx| app.panels(ctx)));
        }
        let out = out.unwrap();
        let texts = tests::painted_text(&out);
        let drawn = |want: &str| {
            let found: Vec<(egui::Rect, egui::Rect)> = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) if t.galley.text() == want => {
                        Some((t.galley.rect.translate(t.pos.to_vec2()), c.clip_rect))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(found.len(), 1, "{want} once at {w}: {texts}");
            let (r, clip) = found[0];
            assert!(holds(clip, r), "{want} cut at {w}: {r:?} in {clip:?}");
            r
        };
        let avg = drawn("10 s");
        let wind = drawn("Wind");
        assert!(
            avg.right() < wind.left(),
            "the boxes run together at {w}: {avg:?}, {wind:?}"
        );
        // The Listen card: the card that holds the average's box.
        let listen = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Rect(r) if r.fill == card && r.rect.contains_rect(avg) => Some(r.rect),
                _ => None,
            })
            .min_by(|a, b| a.area().total_cmp(&b.area()))
            .unwrap_or_else(|| panic!("no card holds the average at {w}"));
        assert!(
            listen.contains_rect(wind),
            "Wind leaves the Listen card at {w}: {wind:?} in {listen:?}"
        );
        assert!(!texts.contains("Avg 10 s"), "{texts}");
    }
}

/// Edit ▸ Remove All Masks asks before it brings every masked stretch
/// back: the masks stay while the question is up.
#[test]
fn removing_all_masks_asks_first() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.reset_session(48000, "syn".into());
    app.set_masked(vec![(10.0, 12.0), (30.0, 33.0)]);
    assert!(
        app.masked_words().contains("2 stretches masked"),
        "{}",
        app.masked_words()
    );
    app.bench.take.remove_masks_asked = true;
    let mut words = Vec::new();
    for _ in 0..2 {
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            app.remove_masks_dialog(ctx)
        });
        words = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
    }
    assert!(words.iter().any(|w| w == "Remove all masks?"), "{words:?}");
    assert_eq!(
        app.bench.take.masked.len(),
        2,
        "nothing removed before the yes"
    );
}

/// The moment the profile shows when the strip is moved back is said once,
/// on the strip that sets it: not in the profile's toolbar, whose controls
/// stand where they do at the newest beats (#1068).
#[test]
fn the_profiles_toolbar_leaves_the_moment_to_the_strip() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = super::tests::synthetic(20.0, 0.0);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1440.0, 700.0));
    let drawn_with = |app: &mut TimegrapherApp| {
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.pane_tools(ui, Pane::Sound);
                });
            });
        }
        theme::tools_drawn_at(&ctx)
    };
    let live = drawn_with(&mut app);
    app.bench.panes_view.strip_end = Some(10.0);
    let drawn = drawn_with(&mut app);
    let moment = app.sound_moment().expect("moved back");
    assert!(
        drawn.iter().all(|(k, _)| !k.ends_with(&moment)),
        "{drawn:?}"
    );
    assert_eq!(live, drawn);
    assert_eq!(
        app.moment_hover(),
        Some(format!("{moment}, set by the Paper Strip."))
    );
}

/// The Tick Tock Profile's plots take the pane's height at 1440 × 1000 in
/// the default layout: short, they take their names in, and with the one
/// time axis under Tock both plots are the same height and Tock's axis is
/// inside the window.
#[test]
fn the_profiles_plots_take_the_panes_height() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.reset_session(48000, "syn".into());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1440.0, 1000.0));
    for _ in 0..4 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.panels(ctx));
    }
    let got = |k: usize| {
        ctx.data(|d| d.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new(("profile plot", k))))
            .expect("the profile is in the default layout")
    };
    let (tick, _) = got(0);
    let (tock, foot) = got(1);
    // About 205 pt for the whole profile here: the plots take two thirds.
    assert!(tick.height() >= 60.0, "{tick:?}");
    assert!(
        (tick.height() - tock.height()).abs() < 1.0,
        "{tick:?} {tock:?}"
    );
    assert!(tock.top() > tick.bottom(), "{tick:?} {tock:?}");
    assert!(
        foot.bottom() <= 1000.0,
        "Tock's axis is in the window: {foot:?}"
    );
}

/// At 800 × 600 the pane is too short to stack Tick and Tock: they go
/// side by side, Tock's plot whole inside the window.
#[test]
fn a_short_profile_puts_tick_and_tock_side_by_side() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.reset_session(48000, "syn".into());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
    for _ in 0..4 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.panels(ctx));
    }
    let got = |k: usize| {
        ctx.data(|d| d.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new(("profile plot", k))))
            .expect("the profile is in the default layout")
    };
    let (tick, _) = got(0);
    let (tock, foot) = got(1);
    assert!(tock.left() > tick.right(), "{tick:?} {tock:?}");
    assert!(tock.height() >= 24.0, "{tock:?}");
    assert!(
        (tick.height() - tock.height()).abs() < 1.0,
        "{tick:?} {tock:?}"
    );
    assert!(foot.bottom() <= 600.0, "{foot:?}");
}

/// At 800 × 600 with every analysis pane open, the tab row's ◂ and ▸
/// stay in their places, greyed at its ends: clicking ▸ again and again
/// scrolls the row to its end and then does nothing, never closing a pane
/// or picking a tab, and ◂ brings it back the same way.
#[test]
fn the_tab_rows_arrows_stay_put_and_grey_at_its_ends() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.reset_session(48000, "syn".into());
    for p in Pane::ALL {
        if !matches!(p, Pane::PositionsTest | Pane::PowerReserve) {
            set_pane_visible(&mut app.bench.panes_view.panes.tiles, p, true);
        }
    }
    let open = |app: &TimegrapherApp| {
        Pane::ALL
            .iter()
            .filter(|&&p| pane_visible(&app.bench.panes_view.panes.tiles, p))
            .count()
    };
    let panes = open(&app);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
    let mut time = 0.0;
    let mut pass = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        time += 0.1;
        let input = egui::RawInput {
            screen_rect: Some(screen),
            time: Some(time),
            events,
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| app.panels(ctx));
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((
                    t.galley.text().to_string(),
                    t.galley.rect.translate(t.pos.to_vec2()).center(),
                    t.fallback_color,
                )),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // The lowest of each arrow (the analysis row's), its place and colour;
    // the last one drawn is ours, over the row's own.
    let arrow = |drawn: &[(String, egui::Pos2, Color32)], glyph: &str| {
        let y = drawn
            .iter()
            .filter(|t| t.0 == glyph)
            .map(|t| t.1.y)
            .fold(f32::MIN, f32::max);
        drawn
            .iter()
            .rev()
            .find(|t| t.0 == glyph && (t.1.y - y).abs() < 4.0)
            .map(|t| (t.1, t.2))
            .unwrap_or_else(|| panic!("a {glyph} on the tab row: {drawn:?}"))
    };
    let mut drawn = Vec::new();
    for _ in 0..6 {
        drawn = pass(&mut app, Vec::new());
    }
    let (left, _) = arrow(&drawn, "⏴");
    let (right, _) = arrow(&drawn, "⏵");
    let picked = |a: &TimegrapherApp| {
        let tiles = &a.bench.panes_view.panes.tiles;
        tiles.iter().find_map(|(_, t)| match t {
            egui_tiles::Tile::Container(egui_tiles::Container::Tabs(t)) if t.children.len() > 4 => {
                t.active
            }
            _ => None,
        })
    };
    let was_picked = picked(&app);
    let mut click = |app: &mut TimegrapherApp, at: egui::Pos2| {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        pass(app, vec![egui::Event::PointerMoved(at), button(true)]);
        pass(app, vec![button(false)]);
        let mut drawn = Vec::new();
        for _ in 0..3 {
            drawn = pass(app, Vec::new());
        }
        drawn
    };
    let mut ends = Vec::new();
    for side in [left, right, left] {
        for _ in 0..12 {
            drawn = click(&mut app, side);
            assert_eq!(arrow(&drawn, "⏴").0, left, "◂ stays put");
            assert_eq!(arrow(&drawn, "⏵").0, right, "▸ stays put");
            assert_eq!(open(&app), panes, "no pane closed");
            assert_eq!(picked(&app), was_picked, "no tab picked");
        }
        ends.push((arrow(&drawn, "⏴").1, arrow(&drawn, "⏵").1));
    }
    // At each end the arrow that went there greys and the other doesn't.
    let (greyed, live) = ends[0];
    assert_ne!(greyed, live, "◂ greyed at the start, ▸ not");
    assert_eq!(ends[1], (live, greyed), "▸ greyed at the end, ◂ not");
    assert_eq!(ends[2], ends[0], "back at the start");
}

/// A Power Reserve Test set up from Watches opens its pane at a pane's
/// least, 140 points with its tab bar, the Paper Strip taking what it
/// gives up.
#[test]
fn the_power_reserve_pane_opens_at_its_least() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    for (w, h) in [(1280.0, 820.0), (800.0, 600.0)] {
        let mut app = TimegrapherApp::with_devices(mac_devices(), None);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h));
        let pass = |app: &mut TimegrapherApp| {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.panels(ctx));
        };
        set_pane_visible(
            &mut app.bench.panes_view.panes.tiles,
            Pane::PowerReserve,
            false,
        );
        for _ in 0..3 {
            pass(&mut app);
        }
        let row = |app: &TimegrapherApp| {
            let id = app
                .bench
                .panes_view
                .panes
                .tiles
                .find_pane(&Pane::PowerReserve)
                .unwrap();
            let tabs = app.bench.panes_view.panes.tiles.parent_of(id).unwrap();
            app.bench
                .panes_view
                .panes
                .tiles
                .rect(tabs)
                .unwrap()
                .height()
        };
        let strip = |app: &TimegrapherApp| {
            let id = app
                .bench
                .panes_view
                .panes
                .tiles
                .find_pane(&Pane::Strip)
                .unwrap();
            let tabs = app.bench.panes_view.panes.tiles.parent_of(id).unwrap();
            app.bench
                .panes_view
                .panes
                .tiles
                .rect(tabs)
                .unwrap()
                .height()
        };
        let (before, was) = (strip(&app), row(&app));
        // At 1280 × 820 the bottom row starts taller; at 800 × 600 it is
        // at its least already.
        assert!(w < 1000.0 || was > MIN_PANE_H + 1.0, "{w}: {was}");
        app.reveal_pane(Pane::PowerReserve);
        for _ in 0..3 {
            pass(&mut app);
        }
        assert!((row(&app) - MIN_PANE_H).abs() < 1.5, "{w}: {}", row(&app));
        assert!(
            strip(&app) >= before + (was - MIN_PANE_H) - 1.5,
            "{w}: {} {before}",
            strip(&app)
        );
    }
}

/// A finished run in a Power Reserve pane at its least, as when a run's
/// take is opened under a tall Paper Strip, gets room for its charts: never
/// a sliver with its axis cut, at the design size or a small window, and
/// never by squeezing the Paper Strip to less than it takes.
#[test]
fn a_finished_runs_pane_is_never_a_sliver() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    for (w, h) in [
        (2560.0, 1440.0),
        (1280.0, 820.0),
        (1000.0, 700.0),
        (800.0, 600.0),
    ] {
        let mut app = TimegrapherApp::with_devices(mac_devices(), None);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h));
        let pass = |app: &mut TimegrapherApp| {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.panels(ctx));
        };
        set_pane_visible(
            &mut app.bench.panes_view.panes.tiles,
            Pane::PowerReserve,
            false,
        );
        for _ in 0..3 {
            pass(&mut app);
        }
        app.reveal_pane(Pane::PowerReserve);
        for _ in 0..3 {
            pass(&mut app);
        }
        let row = |app: &TimegrapherApp| {
            let id = app
                .bench
                .panes_view
                .panes
                .tiles
                .find_pane(&Pane::PowerReserve)
                .unwrap();
            let tabs = app.bench.panes_view.panes.tiles.parent_of(id).unwrap();
            app.bench
                .panes_view
                .panes
                .tiles
                .rect(tabs)
                .unwrap()
                .height()
        };
        assert!((row(&app) - MIN_PANE_H).abs() < 1.5, "{w}: {}", row(&app));
        // The run's take opened, as from the Watches view.
        let dir = std::env::temp_dir().join(format!("tg-app-sliver-{}-{w}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = reserve::finished_run(3).results.unwrap();
        std::fs::write(dir.join(power_reserve::FILE), run.to_json()).unwrap();
        app.load_reserve(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        for _ in 0..4 {
            pass(&mut app);
        }
        let need = app
            .bench
            .panes_view
            .pane_need
            .get(&Pane::PowerReserve)
            .copied()
            .unwrap();
        let tall = row(&app);
        // Nor does the Paper Strip above give up its own least for it.
        let strip = {
            let id = app
                .bench
                .panes_view
                .panes
                .tiles
                .find_pane(&Pane::Strip)
                .unwrap();
            let tabs = app.bench.panes_view.panes.tiles.parent_of(id).unwrap();
            app.bench
                .panes_view
                .panes
                .tiles
                .rect(tabs)
                .unwrap()
                .height()
        };
        assert!(strip + 1.0 >= MIN_PANE_H, "{w}: the strip {strip}");
        // A tab bar over the pane's own need, its charts readable; in a
        // window too small for that, all the room the Paper Strip's least
        // leaves.
        assert!(
            tall + 1.0 >= need + 24.0 || strip <= MIN_PANE_H + 1.0,
            "{w}: {tall} for {need}, the strip {strip}"
        );
        // Where the window has room, grown to show the charts and table.
        // Taller than the Paper Strip only as far as both its charts need
        // (Peter, 4 Nov: room for both charts).
        assert!(h < 700.0 || tall > MIN_PANE_H + 40.0, "{w}: {tall}");
        assert!(
            h < 700.0 || tall <= (need + 24.0).max(strip) + 1.0,
            "{w}: {strip} {tall} for {need}"
        );
    }
}

/// A take with readings, `seconds` long, through the live engine.
fn ticking(seconds: f64) -> TimegrapherApp {
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.reset_session(48000, "syn".into());
    let audio = generate(
        &SynthConfig {
            duration_s: seconds,
            rate_s_per_day: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let live = app.bench.recording.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        if live.push(b) {
            let r = live.reading(app.bench.take.average_s);
            app.bench.results.trend.push(TrendPoint::of(r.time_s, &r));
        }
    }
    app.poll();
    app
}

/// The height of the row holding `pane`: its tabs, or the pane itself.
fn row_h(app: &TimegrapherApp, pane: Pane) -> f32 {
    let id = app.bench.panes_view.panes.tiles.find_pane(&pane).unwrap();
    let row = match app.bench.panes_view.panes.tiles.parent_of(id) {
        Some(p)
            if matches!(
                app.bench.panes_view.panes.tiles.get(p),
                Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(_)))
            ) =>
        {
            p
        }
        _ => id,
    };
    app.bench.panes_view.panes.tiles.rect(row).unwrap().height()
}

/// A pane opened under the Paper Strip, a map among them, takes only the
/// room left: the strip keeps its least, 100 pt of dots, at the design
/// size and in a small window alike.
#[test]
fn the_paper_strip_keeps_its_dots_under_a_map() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    for pane in [Pane::SteadyStft, Pane::SteadyWavelet, Pane::SteadyPeriods] {
        for (w, h) in [(1280.0, 820.0), (1000.0, 700.0), (800.0, 600.0)] {
            let mut app = ticking(12.0);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h));
            let pass = |app: &mut TimegrapherApp| {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                };
                let _ = ctx.run(input, |ctx| app.panels(ctx));
            };
            app.reveal_pane(pane);
            for _ in 0..4 {
                pass(&mut app);
            }
            let need = app
                .bench
                .panes_view
                .pane_need
                .get(&Pane::Strip)
                .copied()
                .unwrap();
            assert!(need >= STRIP_MIN_PLOT_H, "{pane:?} {w}: {need}");
            let strip = row_h(&app, Pane::Strip);
            assert!(strip + 1.0 >= need, "{pane:?} {w}x{h}: {strip} for {need}");
        }
    }
}

/// A pane left too short for its plot under the Paper Strip, as at
/// 800 x 560, says so in one line in its place instead of drawing a
/// sliver; with room again it draws its plot.
#[test]
fn a_pane_too_short_for_its_plot_says_so() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    for pane in [Pane::SteadyWavelet, Pane::Sound] {
        let mut app = ticking(12.0);
        app.reveal_pane(pane);
        let run = |app: &mut TimegrapherApp, w: f32, h: f32| {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h));
            let mut out = None;
            for _ in 0..4 {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                };
                out = Some(ctx.run(input, |ctx| app.panels(ctx)));
            }
            out.unwrap()
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let words = format!("{} not shown: no room.", pane.title());
        let text = run(&mut app, 800.0, 560.0);
        assert!(text.contains(&words), "{pane:?}: {text}");
        assert!(app.bench.panes_view.panes_short.contains(&pane));
        let text = run(&mut app, 1280.0, 1000.0);
        assert!(!text.contains(&words), "{pane:?}: {text}");
        assert!(!app.bench.panes_view.panes_short.contains(&pane));
    }
}

/// A divider the user drags stands, and is kept with the layout, short of
/// the Paper Strip's least, which no drag takes it under (#1976): dragged
/// to a sliver, the strip keeps its least and the map takes the rest.
/// Showing or hiding a pane in its row shares the room out afresh.
#[test]
fn a_dragged_divider_stands_until_a_pane_in_its_row_changes() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1000.0, 700.0));
    let pass = |app: &mut TimegrapherApp| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.panels(ctx));
    };
    let mut app = ticking(12.0);
    app.reveal_pane(Pane::SteadyStft);
    for _ in 0..3 {
        pass(&mut app);
    }
    // The strip's row (its tabs, if any), and the column it is in.
    let tiles = &app.bench.panes_view.panes.tiles;
    let mut strip_id = tiles.find_pane(&Pane::Strip).unwrap();
    let mut column = tiles.parent_of(strip_id).unwrap();
    if matches!(
        app.bench.panes_view.panes.tiles.get(column),
        Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(_)))
    ) {
        strip_id = column;
        column = app.bench.panes_view.panes.tiles.parent_of(column).unwrap();
    }
    // As a drag leaves it: the strip a sliver, the map the rest.
    let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(l))) =
        app.bench.panes_view.panes.tiles.get_mut(column)
    else {
        panic!("the strip's column");
    };
    let total: f32 = l.children.iter().map(|&c| l.shares[c]).sum();
    let others: Vec<TileId> = l
        .children
        .iter()
        .copied()
        .filter(|&c| c != strip_id)
        .collect();
    l.shares.set_share(strip_id, total * 0.05);
    for &c in &others {
        l.shares.set_share(c, total * 0.95 / others.len() as f32);
    }
    app.bench.panes_view.panes_dragged.insert(column);
    for _ in 0..3 {
        pass(&mut app);
    }
    let low = row_h(&app, Pane::Strip);
    let need = app
        .bench
        .panes_view
        .pane_need
        .get(&Pane::Strip)
        .copied()
        .unwrap();
    assert!(need >= STRIP_MIN_H, "{need}");
    assert!(low + 1.0 >= need, "{low} under the strip's least {need}");
    // The drag stands as far as the strip's least, under its tab bar:
    // none of the room goes back to the strip past it.
    assert!(
        low <= need + TAB_BAR_H + 2.0,
        "{low} past the strip's least {need}"
    );
    // Dragged the other way, the map a sliver under its own least: that
    // drag stands, only the strip's least is kept (Trevor on #2026).
    let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(l))) =
        app.bench.panes_view.panes.tiles.get_mut(column)
    else {
        panic!("the strip's column");
    };
    l.shares.set_share(strip_id, total * 0.95);
    for &c in &others {
        l.shares.set_share(c, total * 0.05 / others.len() as f32);
    }
    for _ in 0..3 {
        pass(&mut app);
    }
    let map = row_h(&app, Pane::SteadyStft);
    assert!(map < MIN_PANE_H, "the map's drag stands: {map}");
    assert!(row_h(&app, Pane::Strip) + 1.0 >= need);
    // Kept with the layout.
    let saved = app.settings();
    assert_eq!(saved.panes_dragged, vec![column]);
    // A pane shown in its row: every pane at its least again.
    app.show_pane(Pane::SteadyStft, false);
    app.show_pane(Pane::SteadyStft, true);
    assert!(app.bench.panes_view.panes_dragged.is_empty());
    for _ in 0..4 {
        pass(&mut app);
    }
    let need = app
        .bench
        .panes_view
        .pane_need
        .get(&Pane::Strip)
        .copied()
        .unwrap();
    assert!(row_h(&app, Pane::Strip) + 1.0 >= need);
}

/// The STFT and Wavelet Maps show one map at a time, Rate first, and
/// share the choice: picking Amplitude on one picks it on the other.
/// Every other steadiness pane keeps its own switches.
#[test]
fn the_maps_show_one_map_at_a_time_and_share_it() {
    use steady::View;
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    assert_eq!(app.series_shown(View::Stft), [true, false, false]);
    assert_eq!(app.series_shown(View::Wavelet), [true, false, false]);
    assert_eq!(app.series_shown(View::Fft), [true, false, false]);
    app.bench
        .panes_view
        .steady_series
        .insert(View::Stft, [false, true, false]);
    assert_eq!(app.series_shown(View::Wavelet), [false, true, false]);
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut keys = Vec::new();
    for _ in 0..3 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.pane_tools(ui, Pane::SteadyWavelet);
            });
        });
        keys = theme::tools_drawn_at(&ctx)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
    }
    assert!(keys.iter().any(|k| k == "Wavelet Map/Series"), "{keys:?}");
    assert_eq!(
        app.bench.panes_view.steady_series[&View::Stft],
        [false, true, false]
    );
}

/// Settings > Microphone puts the menu on its own line under its name, the
/// group's width, for every device: a long name shows whole where it fits,
/// and one too long is cut in the middle, keeping its end.
#[test]
fn the_microphone_menu_has_its_own_line_and_cuts_a_name_in_the_middle() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let drawn = |app: &mut TimegrapherApp| {
        let mut all = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1100.0, 700.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.microphone_page(ui));
            });
            all = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    // What is drawn, as cut.
                    egui::Shape::Text(t) => Some((
                        t.galley
                            .rows
                            .iter()
                            .flat_map(|r| r.glyphs.iter().map(|g| g.chr))
                            .collect::<String>(),
                        t.pos,
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
        }
        all
    };
    let mut devices = mac_devices();
    devices[0].name = "Playback/recording through the PulseAudio sound server".into();
    let mut app = TimegrapherApp::with_devices(devices.clone(), None);
    let name = app.picked_label();
    let all = drawn(&mut app);
    let at = |t: &str| all.iter().find(|(s, _)| s == t).map(|(_, p)| *p);
    let menu = at(&name).unwrap_or_else(|| panic!("{name} whole: {all:?}"));
    let label = at("Microphone").expect("its name");
    assert!(
        menu.y > label.y + 20.0,
        "under its name: {menu:?} {label:?}"
    );
    devices[0].name = format!(
        "{} USB Audio Interface (2)",
        "Very Long Maker Name ".repeat(4)
    );
    let mut app = TimegrapherApp::with_devices(devices, None);
    let all = drawn(&mut app);
    let cut = all
        .iter()
        .map(|(s, _)| s)
        .find(|s| s.contains('…'))
        .unwrap_or_else(|| panic!("{all:?}"));
    assert!(
        cut.starts_with("Very Long") && cut.ends_with("(default)"),
        "{cut}"
    );
}

/// The strip's Length field holds the longest length whole: Fit's
/// 79:59:59, inside the field and clear of its ⏷ (#1976; it read
/// "67:28:1" at 92 pt).
#[test]
fn the_length_field_holds_eighty_hours_whole() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    // Fit's length to the latest beat, a second short of the 80 hours:
    // the widest it reads (whole hours read "80 h").
    let longest = StripView::MAX_SPAN_S - 1.0;
    app.bench.panes_view.strip.set_span(longest);
    let shown = fields::duration(longest);
    assert_eq!(shown, "79:59:59");
    let mut drawn = None;
    for _ in 0..2 {
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let pal = theme::pal(ui);
                    app.strip_span_fields(ui, pal);
                });
            });
        });
        let mut shapes = Vec::new();
        fn flat(s: &egui::Shape, into: &mut Vec<egui::Shape>) {
            match s {
                egui::Shape::Vec(v) => v.iter().for_each(|s| flat(s, into)),
                s => into.push(s.clone()),
            }
        }
        out.shapes.iter().for_each(|c| flat(&c.shape, &mut shapes));
        let text = shapes.iter().find_map(|s| match s {
            egui::Shape::Text(t) if t.galley.text() == shown => {
                Some(t.galley.rect.translate(t.pos.to_vec2()))
            }
            _ => None,
        });
        // The field's plate: the one LENGTH_FIELD_W wide round the text.
        let field = text.and_then(|t| {
            shapes.iter().find_map(|s| match s {
                egui::Shape::Rect(r)
                    if r.rect.contains(t.center())
                        && (r.rect.width() - super::paper_strip::LENGTH_FIELD_W).abs() < 0.5 =>
                {
                    Some(r.rect)
                }
                _ => None,
            })
        });
        drawn = text.zip(field);
    }
    let (text, field) = drawn.expect("79:59:59 drawn in its field");
    // The ⏷ takes the field's last 24 pt.
    let arrow_left = field.right() - 24.0;
    assert!(
        text.right() <= arrow_left,
        "{shown} ends at {}, the ⏷ starts at {arrow_left}",
        text.right()
    );
}

/// Charts keeps room for every chart turned on, not one alone: in the
/// starting layout at 1920 x 1080, as Peter's screen, Beat Error had no
/// room beside Rate and Amplitude while the strip had plenty to spare
/// (#2342 item 1). The strip keeps its least.
#[test]
fn charts_has_room_for_all_three_at_1920_by_1080() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = ticking(30.0);
    app.reveal_pane(Pane::Charts);
    app.bench.panes_view.chart_series = [true; 3];
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1920.0, 1080.0));
    for _ in 0..6 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.panels(ctx));
    }
    assert_eq!(app.bench.panes_view.charts_drawn, [true; 3]);
    let need = app.bench.panes_view.pane_need[&Pane::Strip];
    assert!(row_h(&app, Pane::Strip) + 1.0 >= need);
}

/// The panes' tiles as a line: each container its kind and its shown
/// children, each pane its title.
fn layout_text(tree: &Tree<Pane>, id: TileId) -> String {
    let t = &tree.tiles;
    match t.get(id) {
        Some(egui_tiles::Tile::Pane(p)) => p.title().to_string(),
        Some(egui_tiles::Tile::Container(c)) => {
            let shown: Vec<String> = c
                .children()
                .filter(|&&k| t.is_visible(k))
                .map(|&k| layout_text(tree, k))
                .collect();
            format!("{:?}[{}]", c.kind(), shown.join(", "))
        }
        None => "?".into(),
    }
}

/// Whether a tab row holds anything but panes: such a tab is titled by
/// its kind, "Vertical", "Horizontal" or "Tabs".
fn a_tab_holds_a_container(tree: &Tree<Pane>) -> bool {
    let t = &tree.tiles;
    t.iter().any(|(_, tile)| match tile {
        egui_tiles::Tile::Container(egui_tiles::Container::Tabs(tabs)) => {
            tabs.children.iter().any(|&c| t.get_container(c).is_some())
        }
        _ => false,
    })
}

/// A tab dropped on the lower half of the Tick Tock Profile, whose tab
/// row also holds the Scope and the hidden panes, lands under the profile
/// as the preview shows, never as a tab titled "Vertical" beside the
/// Scope (#2213). Before, egui_tiles split the profile inside its tab
/// row, which the hidden panes kept from folding away.
#[test]
fn a_tab_dropped_under_a_pane_lands_as_its_own_row() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.reset_session(48000, "syn".into());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1920.0, 1080.0));
    let mut time = 0.0;
    let mut pass = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        time += 1.0 / 60.0;
        let input = egui::RawInput {
            screen_rect: Some(screen),
            time: Some(time),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.panels(ctx));
    };
    let button = |pressed, pos| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    for _ in 0..2 {
        pass(&mut app, Vec::new());
    }
    // A pane's tab pressed, moved to `to` in six steps and let go.
    let mut drag = |app: &mut TimegrapherApp, pane: Pane, to: egui::Pos2| {
        for _ in 0..4 {
            pass(app, Vec::new());
        }
        let id = app.bench.panes_view.panes.tiles.find_pane(&pane).unwrap();
        let tab: egui::Rect = ctx
            .data(|d| d.get_temp(egui::Id::new(("tab rect", id))))
            .unwrap();
        let from = tab.center();
        pass(
            app,
            vec![egui::Event::PointerMoved(from), button(true, from)],
        );
        for k in 1..=6 {
            let at = from.lerp(to, k as f32 / 6.0);
            pass(app, vec![egui::Event::PointerMoved(at)]);
        }
        pass(app, vec![button(false, to)]);
        for _ in 0..3 {
            pass(app, Vec::new());
        }
    };
    let tree = |app: &TimegrapherApp| app.bench.panes_view.panes.clone();
    let area = |app: &TimegrapherApp| {
        let t = tree(app);
        t.tiles.rect(t.root().unwrap()).unwrap()
    };
    // Distributions beside the strip, then the Positions Test opened as a
    // tab behind the profile.
    set_pane_visible(
        &mut app.bench.panes_view.panes.tiles,
        Pane::Distributions,
        true,
    );
    let a = area(&app);
    drag(
        &mut app,
        Pane::Distributions,
        egui::pos2(a.right() - 5.0, a.top() + 150.0),
    );
    set_pane_visible(
        &mut app.bench.panes_view.panes.tiles,
        Pane::PositionsTest,
        true,
    );
    let row_of = |t: &Tree<Pane>, p: Pane| t.tiles.parent_of(t.tiles.find_pane(&p).unwrap());
    let profile_row = row_of(&tree(&app), Pane::Sound);
    // 45 pt above the bottom edge is nearer the profile's lower half than
    // the drop zone past the last row.
    let a = area(&app);
    drag(
        &mut app,
        Pane::PositionsTest,
        egui::pos2(a.center().x, a.bottom() - 45.0),
    );
    let t = tree(&app);
    let text = layout_text(&t, t.root().unwrap());
    assert!(!a_tab_holds_a_container(&t), "{text}");
    assert_eq!(
        text,
        "Vertical[Horizontal[Tabs[Paper Strip], Tabs[Distributions]], \
         Tabs[Tick Tock Profile, Scope], Tabs[Positions Test]]",
    );
    assert_eq!(
        row_of(&t, Pane::Sound),
        profile_row,
        "the profile keeps its tab row, the hidden panes still in it"
    );
    assert!(layout_is_whole(&t));
}

/// A tab row wrapped round a whole line of panes, as a drop on the whole
/// layout leaves it, is mended to the line with the dropped pane past its
/// end; tabs inside tabs join the row (#2213).
#[test]
fn a_tab_row_round_a_line_of_panes_is_mended() {
    let mut tiles = Tiles::default();
    let strip = tiles.insert_pane(Pane::Strip);
    let strip = tiles.insert_tab_tile(vec![strip]);
    let sound = tiles.insert_pane(Pane::Sound);
    let scope = tiles.insert_pane(Pane::Scope);
    let inner = tiles.insert_tab_tile(vec![sound, scope]);
    let profile = tiles.insert_tab_tile(vec![inner]);
    let line = tiles.insert_vertical_tile(vec![strip, profile]);
    let positions = tiles.insert_pane(Pane::PositionsTest);
    let root = tiles.insert_tab_tile(vec![line, positions]);
    let mut tree = Tree::new("panes", root, tiles);
    mend_tabs(&mut tree, Some(positions));
    assert!(!a_tab_holds_a_container(&tree));
    assert_eq!(tree.root(), Some(line));
    assert_eq!(
        layout_text(&tree, line),
        "Vertical[Tabs[Paper Strip], Tabs[Tick Tock Profile, Scope], Tabs[Positions Test]]"
    );
}

/// A tab row's tabs and the one it shows.
fn row_tabs(tree: &Tree<Pane>, row: TileId) -> (Vec<TileId>, Option<TileId>) {
    match tree.tiles.get(row) {
        Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(t))) => {
            (t.children.clone(), t.active)
        }
        _ => panic!("{row:?} is not a tab row"),
    }
}

/// A line's children and their shares.
fn line_of(tree: &Tree<Pane>, line: TileId) -> (Vec<TileId>, Vec<f32>) {
    match tree.tiles.get(line) {
        Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(l))) => (
            l.children.clone(),
            l.children.iter().map(|&c| l.shares[c]).collect(),
        ),
        _ => panic!("{line:?} is not a line"),
    }
}

/// Tabs inside a tab row join it in their place. With no drop, the row
/// keeps the tab it showed, or shows the inner tabs' own if they were
/// shown; a drop among them shows the tab dropped (Trevor on #2535).
#[test]
fn tabs_inside_tabs_join_the_row_in_their_place() {
    let build = |shown_inner: bool| {
        let mut tiles = Tiles::default();
        let sound = tiles.insert_pane(Pane::Sound);
        let scope = tiles.insert_pane(Pane::Scope);
        let inner = tiles.insert_tab_tile(vec![sound, scope]);
        if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(t))) =
            tiles.get_mut(inner)
        {
            t.active = Some(scope);
        }
        let strip = tiles.insert_pane(Pane::Strip);
        let row = tiles.insert_tab_tile(vec![inner, strip]);
        if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(t))) =
            tiles.get_mut(row)
        {
            t.active = Some(if shown_inner { inner } else { strip });
        }
        (Tree::new("panes", row, tiles), row, [sound, scope, strip])
    };
    // A saved layout, the Paper Strip shown: it stays shown.
    let (mut tree, row, [sound, scope, strip]) = build(false);
    mend_tabs(&mut tree, None);
    assert_eq!(
        row_tabs(&tree, row),
        (vec![sound, scope, strip], Some(strip))
    );
    // The inner tabs shown: their own shown tab, the Scope.
    let (mut tree, row, [sound, scope, strip]) = build(true);
    mend_tabs(&mut tree, None);
    assert_eq!(
        row_tabs(&tree, row),
        (vec![sound, scope, strip], Some(scope))
    );
    // A drop among them: the tab dropped.
    let (mut tree, row, [sound, scope, strip]) = build(false);
    mend_tabs(&mut tree, Some(sound));
    assert_eq!(
        row_tabs(&tree, row),
        (vec![sound, scope, strip], Some(sound))
    );
}

/// A split of a pane in a tab row takes the row's place and keeps its
/// height, and the row takes the pane's place in the split and keeps the
/// pane's share, showing the pane (Trevor on #2535).
#[test]
fn a_split_in_a_tab_row_keeps_each_share() {
    let mut tiles = Tiles::default();
    let strip = tiles.insert_pane(Pane::Strip);
    let strip = tiles.insert_tab_tile(vec![strip]);
    let sound = tiles.insert_pane(Pane::Sound);
    let positions = tiles.insert_pane(Pane::PositionsTest);
    let mut split = Linear::new(LinearDir::Vertical, vec![sound, positions]);
    split.shares.set_share(sound, 3.0);
    let split = tiles.insert_container(split);
    let scope = tiles.insert_pane(Pane::Scope);
    let row = tiles.insert_tab_tile(vec![split, scope]);
    let mut root = Linear::new(LinearDir::Vertical, vec![strip, row]);
    root.shares.set_share(row, 2.0);
    let root = tiles.insert_container(root);
    let mut tree = Tree::new("panes", root, tiles);
    mend_tabs(&mut tree, Some(positions));
    assert!(!a_tab_holds_a_container(&tree));
    assert_eq!(line_of(&tree, root), (vec![strip, split], vec![1.0, 2.0]));
    assert_eq!(
        line_of(&tree, split),
        (vec![row, positions], vec![3.0, 1.0])
    );
    assert_eq!(row_tabs(&tree, row), (vec![sound, scope], Some(sound)));
}

/// A row wrapped round a line shows the tab dropped; an emptied row goes;
/// and a hidden tab beside the line is no reason to wrap (Trevor on
/// #2535).
#[test]
fn a_wrapped_line_shows_the_drop_and_leaves_no_empty_row() {
    // The drop shown, not the row's first other tab.
    let mut tiles = Tiles::default();
    let strip = tiles.insert_pane(Pane::Strip);
    let sound = tiles.insert_pane(Pane::Sound);
    let line = tiles.insert_vertical_tile(vec![strip, sound]);
    let charts = tiles.insert_pane(Pane::Charts);
    let positions = tiles.insert_pane(Pane::PositionsTest);
    let row = tiles.insert_tab_tile(vec![line, charts, positions]);
    let mut tree = Tree::new("panes", row, tiles);
    mend_tabs(&mut tree, Some(positions));
    assert_eq!(tree.root(), Some(line));
    assert_eq!(line_of(&tree, line).0, vec![strip, sound, row]);
    assert_eq!(
        row_tabs(&tree, row),
        (vec![charts, positions], Some(positions))
    );
    // A row holding only an empty line: the line takes its place, the
    // row goes.
    let mut tiles = Tiles::default();
    let line = tiles.insert_container(Linear::new(LinearDir::Vertical, Vec::new()));
    let row = tiles.insert_tab_tile(vec![line]);
    let mut tree = Tree::new("panes", row, tiles);
    mend_tabs(&mut tree, None);
    assert_eq!(tree.root(), Some(line));
    assert!(tree.tiles.get(row).is_none());
    // The only other tab hidden: the line's first pane takes the line's
    // place in the row, as a split.
    let mut tiles = Tiles::default();
    let sound = tiles.insert_pane(Pane::Sound);
    let scope = tiles.insert_pane(Pane::Scope);
    let line = tiles.insert_vertical_tile(vec![sound, scope]);
    let positions = tiles.insert_pane(Pane::PositionsTest);
    let row = tiles.insert_tab_tile(vec![line, positions]);
    let mut tree = Tree::new("panes", row, tiles);
    tree.tiles.set_visible(positions, false);
    mend_tabs(&mut tree, None);
    assert_eq!(tree.root(), Some(line));
    assert_eq!(line_of(&tree, line).0, vec![row, scope]);
    assert_eq!(row_tabs(&tree, row), (vec![sound, positions], Some(sound)));
}

/// A mended tile put in a tab row's place there is shown if the row was.
#[test]
fn a_tile_put_in_a_shown_tabs_place_is_shown() {
    let mut tiles = Tiles::default();
    let strip = tiles.insert_pane(Pane::Strip);
    let sound = tiles.insert_pane(Pane::Sound);
    let parent = tiles.insert_tab_tile(vec![strip, sound]);
    let scope = tiles.insert_pane(Pane::Scope);
    let mut tree = Tree::new("panes", parent, tiles);
    put_in_place(&mut tree, Some(parent), strip, scope);
    assert_eq!(row_tabs(&tree, parent), (vec![scope, sound], Some(scope)));
}

/// A saved layout whose tab row holds a split is drawn mended from its
/// first frame: no tab titled "Vertical" (#2213).
#[test]
fn a_saved_split_in_a_tab_row_is_never_drawn_as_a_tab() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.reset_session(48000, "syn".into());
    set_pane_visible(&mut app.bench.panes_view.panes.tiles, Pane::Charts, true);
    let tiles = &mut app.bench.panes_view.panes.tiles;
    let sound = tiles.find_pane(&Pane::Sound).unwrap();
    let charts = tiles.find_pane(&Pane::Charts).unwrap();
    let row = tiles.parent_of(sound).unwrap();
    let split = tiles.insert_vertical_tile(vec![sound, charts]);
    if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(t))) = tiles.get_mut(row) {
        t.children.retain(|&c| c != charts);
        for c in &mut t.children {
            if *c == sound {
                *c = split;
            }
        }
        t.active = Some(split);
    }
    assert!(a_tab_holds_a_container(&app.bench.panes_view.panes));
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1920.0, 1080.0),
        )),
        time: Some(0.1),
        ..Default::default()
    };
    let out = ctx.run(input, |ctx| app.panels(ctx));
    let text = painted_text(&out);
    assert!(!text.lines().any(|l| l == "Vertical"), "{text}");
    assert!(text.lines().any(|l| l == "Charts"), "{text}");
    assert!(!a_tab_holds_a_container(&app.bench.panes_view.panes));
}
