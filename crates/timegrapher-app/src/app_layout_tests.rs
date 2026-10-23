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
