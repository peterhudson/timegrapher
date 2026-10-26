//! Every pane drawn whole, headless: each shown pane names itself, and
//! nothing a pane draws lands outside its card. The window is drawn at a
//! few sizes and in the states a user meets first (the start screen, a
//! take still waiting for a regular beat, a take with readings), with
//! microphone names as long as macOS gives them, which the Linux and
//! Windows test machines don't have.

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
            let tiles = &app.panes.tiles;
            tiles.find_pane(&p).and_then(|t| tiles.rect(t)).is_some()
        };
        let drawn = pane_visible(&app.panes.tiles, p) && laid_out();
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
        app.devices_listed = false;
        app.device = Some(mac_devices()[0].id.clone());
        wrong.extend(check(&mut app, &ctx, size, "start, microphones not listed"));

        let mut app = TimegrapherApp::with_devices(mac_devices(), None);
        wrong.extend(check(&mut app, &ctx, size, "start, microphones listed"));
        app.all_inputs = true;
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
    let live = quiet.live.as_mut().unwrap();
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
    let live = ticking.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        if live.push(b) {
            let r = live.reading(ticking.average_s);
            ticking.trend.push(TrendPoint::of(r.time_s, &r));
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
    while app.batch.is_some() && t.elapsed().as_secs() < 120 {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(app.live.is_some(), "{:?}", app.message);
    let mut wrong = Vec::new();
    for size in SIZES {
        wrong.extend(check(&mut app, &ctx, size, "a recording opened"));
    }
    // With the charts and the distributions open too, no pane is crushed:
    // each laid out keeps room for a plot.
    for p in [Pane::Charts, Pane::Distributions] {
        set_pane_visible(&mut app.panes.tiles, p, true);
    }
    for size in [(1000.0, 700.0), (1440.0, 900.0)] {
        wrong.extend(check(&mut app, &ctx, size, "charts and distributions open"));
        for &p in Pane::ALL.iter() {
            let tiles = &app.panes.tiles;
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
            let tiles = &app.panes.tiles;
            let r = tiles.find_pane(&front).and_then(|t| tiles.rect(t));
            let need = app.pane_need.get(&front).copied();
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
            app.all_inputs = all;
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
    let live = app.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        if live.push(b) {
            let r = live.reading(app.average_s);
            app.trend.push(TrendPoint::of(r.time_s, &r));
        }
    }
    app.poll();

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
/// Beat Error at its left end, then Width and Length; Direction is a
/// labelled Down | Across control; Unmask All is gone (Edit ▸ Remove All
/// Masks does it).
#[test]
fn the_strip_direction_is_down_or_across_in_words() {
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
    for w in ["Direction", "Down", "Across", "Width", "Length"] {
        assert!(words.iter().any(|t| t == w), "{w} not drawn: {words:?}");
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
    app.remove_masks_asked = true;
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
    assert_eq!(app.masked.len(), 2, "nothing removed before the yes");
}

/// The moment the profile shows ("At 10:00") is at the right end of its
/// toolbar, before the "?", clear of the controls.
#[test]
fn the_profiles_moment_is_at_the_toolbars_right_end() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = TimegrapherApp::with_devices(mac_devices(), None);
    app.reset_session(48000, "syn".into());
    app.sound_note = Some("At 10:00".into());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1440.0, 700.0));
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
    let drawn = theme::tools_drawn_at(&ctx);
    let title = Pane::Sound.title();
    let at = |k: &str| {
        drawn
            .iter()
            .find(|(d, _)| d == &format!("{title}/{k}"))
            .map(|d| d.1)
            .unwrap_or_else(|| panic!("no {k}: {drawn:?}"))
    };
    let note = at("At 10:00");
    assert!(note.right() > 1440.0 - 60.0, "{note:?}");
    for (k, r) in &drawn {
        if k.starts_with(&format!("{title}/")) && !k.ends_with("At 10:00") {
            assert!(r.right() < note.left(), "{k} at {r:?} runs into the note");
        }
    }
    assert!(at("dB Scale").width() < 30.0);
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
