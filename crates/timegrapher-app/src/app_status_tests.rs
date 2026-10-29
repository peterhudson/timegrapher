//! The status bar, headless: notices never move the readings, the left
//! side stops short of them, a long take name ends in "…", and the take's
//! USB lines and clock log apart say so.

use super::tests::{queued_capture, save_failure_fixture, synthetic, themed};
use super::*;

/// No banner ever pushes the window down: a crash last time, sound
/// kept from an earlier session, a newer release and an error are each
/// a word in the status bar, and the readings stay where they were.
/// The status bar repeats nothing the cards show and carries no links
/// the Help menu has.
#[test]
fn notices_sit_in_the_status_bar_and_never_move_the_readings() {
    let ctx = themed();
    let mut app = synthetic(15.0, 0.0);
    let draw = |app: &mut TimegrapherApp| {
        let mut found = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1440.0, 800.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| app.panels(ctx));
            found.clear();
            for c in &out.shapes {
                if let egui::Shape::Text(t) = &c.shape {
                    found.push((t.galley.text().to_string(), t.visual_bounding_rect()));
                }
            }
        }
        found
    };
    let top = |found: &[(String, egui::Rect)], text: &str| {
        found
            .iter()
            .find(|(t, _)| t == text)
            .map(|(_, r)| r.top())
            .unwrap_or_else(|| panic!("{text}: {found:?}"))
    };
    let before = draw(&mut app);
    let rate = top(&before, "Rate");
    for gone in ["Check for Updates…", "Report a Problem…", "DU dial up"] {
        assert!(!before.iter().any(|(t, _)| t == gone), "{gone}: {before:?}");
    }
    assert!(
        !before
            .iter()
            .any(|(t, _)| t.ends_with("beats per hour") || t.starts_with("Showing")),
        "{before:?}"
    );
    let crash = std::env::temp_dir().join(format!("tg-crash-{}.log", std::process::id()));
    std::fs::write(&crash, "panicked").unwrap();
    app.crash = Some(crash.clone());
    app.message = Some(("The microphone couldn't be opened.".into(), true));
    let after = draw(&mut app);
    assert_eq!(top(&after, "Rate"), rate, "the readings didn't move");
    let bar = top(&after, "Closed Unexpectedly…");
    assert!(bar > 700.0, "in the status bar: {bar}");
    assert!(top(&after, "The microphone couldn't be opened.…") > 700.0);
    let _ = std::fs::remove_file(crash);
}

#[test]
fn the_status_bar_cuts_its_left_side_short_of_the_notices() {
    let ctx = themed();
    let (mut app, base) = save_failure_fixture("status-bar");
    let crash = base.join("crash.log");
    std::fs::write(&crash, "panicked").unwrap();
    app.crash = Some(crash);
    // A passing message, which is cut short on the left, not a notice.
    app.message = Some((
        "Saved the take and its analysis into the watch's folder in the library".into(),
        false,
    ));
    // Listening to an input with a long name, whose words can be cut.
    app.source_label = "USB PnP Sound Device Analog Stereo Microphone Input".into();
    let _tx = queued_capture(&mut app, &base);
    // Every few pixels from 800 to 1000, where the passing message comes
    // and goes, and wide.
    for w in (800..=1000).step_by(4).map(|w| w as f32).chain([1280.0]) {
        let mut found = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(w, 700.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| app.panels(ctx));
            found.clear();
            for c in &out.shapes {
                if let egui::Shape::Text(t) = &c.shape {
                    let r = t.visual_bounding_rect().intersect(c.clip_rect);
                    if r.top() > 650.0 && r.is_positive() {
                        found.push((t.galley.text().to_string(), r));
                    }
                }
            }
        }
        // A bare "…" in the bar's small text, once the fonts are loaded.
        let small = egui::TextStyle::Small.resolve(&ctx.style());
        let bare = ctx.fonts_mut(|f| f.layout_no_wrap("…".into(), small, Color32::WHITE).size().x);
        let notices = ["Closed Unexpectedly…"];
        let edge = found
            .iter()
            .filter(|(t, _)| notices.contains(&t.as_str()))
            .map(|(_, r)| r.left())
            .fold(f32::INFINITY, f32::min);
        assert!(edge.is_finite(), "{w}: {found:?}");
        for (t, r) in found.iter().filter(|(t, _)| !notices.contains(&t.as_str())) {
            assert!(
                r.right() <= edge,
                "{w}: {t:?} {r:?} runs under the notices at {edge}"
            );
        }
        // Words with no room for any of them aren't drawn as a bare "…".
        assert!(
            !found
                .iter()
                .any(|(t, r)| t.starts_with('…') || r.width() <= bare + 1.0),
            "{w}: a bare \"…\" ({bare} px): {found:?}"
        );
        // The take's state and whether it is saved always show whole.
        for whole in ["Not saved", "Listening"] {
            assert!(
                found
                    .iter()
                    .any(|(t, _)| t == whole || t.ends_with(&format!("· {whole}"))),
                "{w}: {whole}: {found:?}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// A take whose name is wider than the status bar has room for beside a
/// notice ends its name in "…" and keeps its watch and state, drawn whole
/// and not clipped.
#[test]
fn a_long_take_name_ends_in_an_ellipsis() {
    let ctx = themed();
    let mut app = synthetic(15.0, 0.0);
    app.input = Input::File;
    app.opened_path =
        "a take with a name far wider than the status bar has room for at 800 by 600.wav".into();
    let crash = std::env::temp_dir().join(format!("tg-crash-name-{}.log", std::process::id()));
    std::fs::write(&crash, "panicked").unwrap();
    app.crash = Some(crash.clone());
    let words = app.status_words();
    let (_, tail) = words.split_once(" · ").unwrap();
    let mut found = Vec::new();
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(800.0, 600.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| app.panels(ctx));
        found.clear();
        for c in &out.shapes {
            if let egui::Shape::Text(t) = &c.shape {
                let r = t.visual_bounding_rect();
                if r.top() > 550.0 && t.galley.text().starts_with("a take with") {
                    found.push((t.galley.text().to_string(), r, r.intersect(c.clip_rect)));
                }
            }
        }
    }
    let [(text, whole, shown)] = found.as_slice() else {
        panic!("{found:?}");
    };
    assert!(text.ends_with(&format!("… · {tail}")), "{text}");
    assert_eq!(whole, shown, "{text} is clipped");
    let _ = std::fs::remove_file(crash);
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "a test's clock fixture, not a take being opened"
)]
fn usb_lines_and_a_good_log_apart_say_so_in_the_status_bar() {
    // An exact log, an entry a minute for half an hour, the card
    // 20 ppm slow; USB lines 100 ppm off that, then within 5 ppm.
    let log: Vec<(f64, f64)> = (0..31)
        .map(|i| {
            let audio = f64::from(i) * 60.0;
            (audio, 1.7e9 + audio * (1.0 + 20e-6))
        })
        .collect();
    let rated = |usb: f64| {
        let mut fit = timegrapher_core::clock::ClockFit::for_rates(&log).unwrap();
        fit.rate_from_usb(usb);
        fit
    };
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    assert_eq!(app.clock_disagreement(), None, "no take open");
    app.put_take_clock(Some(rated(120.0)), None);
    let (short, long) = app.clock_disagreement().expect("100 ppm apart");
    assert_eq!(short, "USB lines and clock log 100 ppm apart");
    assert!(
        long.contains("+120.0 ppm") && long.contains("+20 ± 0 ppm"),
        "{long}"
    );
    app.put_take_clock(Some(rated(24.0)), None);
    assert_eq!(app.clock_disagreement(), None, "4 ppm apart");
}
