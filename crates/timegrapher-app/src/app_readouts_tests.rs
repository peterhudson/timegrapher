//! Inputs and their gain, what the cards and readings say, the profiles,
//! exporting a pane, and the empty and error states.

use super::tests::{frames, painted_text, screen, synthetic, themed};
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

/// An input that hangs as it closes doesn't hang the app: the wait
/// ends and the app carries on, as it does at once when closing is
/// quick.
#[test]
fn a_hung_input_closes_without_hanging_the_app() {
    struct Hangs(std::sync::mpsc::Receiver<()>);
    impl Drop for Hangs {
        fn drop(&mut self) {
            // As a dead sound server's close: no answer comes.
            let _ = self.0.recv();
        }
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let started = Instant::now();
    assert!(!drop_within(Hangs(rx), Duration::from_millis(100)));
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(tx);
    assert!(drop_within(String::from("quick"), Duration::from_secs(2)));
}

/// On macOS the input's automatic gain is the USB microphone's whose
/// name it carries; the system default takes the only one there is.
#[test]
fn an_input_finds_its_usb_microphones_automatic_gain() {
    let mic = |name: &str| mixer::UsbAgc {
        name: name.into(),
        vendor_id: 0x0d8c,
        product_id: 0x013c,
        control: timegrapher_core::usbagc::AgcControl {
            interface: 0,
            unit: 2,
            channel: 0,
            volume: true,
        },
        before: Some(true),
        after: None,
        volume_db: None,
        error: None,
    };
    let name = |m: Option<mixer::UsbAgc>| m.map(|m| m.name);
    let one = || vec![mic("USB PnP Sound Device")];
    let two = || vec![mic("USB PnP Sound Device"), mic("Blue Yeti")];
    assert_eq!(
        name(usb_agc_for(one(), Some("USB PnP Sound Device"))).as_deref(),
        Some("USB PnP Sound Device")
    );
    assert_eq!(
        name(usb_agc_for(two(), Some("Blue Yeti Stereo Microphone"))).as_deref(),
        Some("Blue Yeti")
    );
    assert_eq!(
        name(usb_agc_for(one(), None)).as_deref(),
        Some("USB PnP Sound Device")
    );
    // The built-in microphone has no such switch, and the default
    // with two USB microphones is neither.
    assert!(usb_agc_for(one(), Some("MacBook Pro Microphone")).is_none());
    assert!(usb_agc_for(two(), None).is_none());
    assert!(usb_agc_for(Vec::new(), None).is_none());
}

/// Which inputs keep their automatic gain on is kept with the settings.
#[test]
fn the_automatic_gain_choice_is_kept_per_input() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.agc_on = vec!["hw:2,0".into()];
    let s = app.settings();
    assert_eq!(s.agc_on, vec!["hw:2,0".to_string()]);
    let mut again = TimegrapherApp::with_devices(Vec::new(), None);
    again.apply_settings(s);
    assert_eq!(again.agc_on, vec!["hw:2,0".to_string()]);
    assert!(
        Settings::default().agc_on.is_empty(),
        "off for every input at first"
    );
}

#[test]
fn errors_read_without_codes() {
    assert_eq!(
        plain_error("Can't read /x/y.flac: No such file or directory (os error 2)"),
        "Can't read /x/y.flac: nothing is there"
    );
    assert_eq!(plain_error("Saving failed"), "Saving failed");
}

#[test]
fn a_reading_says_what_limits_it_as_facts() {
    let caveats = [
        Caveat::ShortSpan { span_s: 4.0 },
        Caveat::LiftUnknown,
        Caveat::ClockUnknown,
    ];
    let amp = reading_limits("Amplitude", &caveats, 52.0, 30);
    let short: Vec<&str> = amp.iter().map(|f| f.0.as_str()).collect();
    // The lift angle and the input's clock are said elsewhere.
    assert_eq!(short, ["4 s of beats"]);
    let rate = reading_limits("Rate", &caveats, 52.0, 30);
    let short: Vec<&str> = rate.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(short, ["4 s of beats"]);
    let missing = reading_limits("Rate", &[Caveat::BeatsMissing { fraction: 0.9 }], 52.0, 90);
    assert_eq!(missing[0].0, "90 of 100 beats");
    for (_, long) in amp.iter().chain(&rate) {
        for word in [
            "Confidence",
            "Low",
            "High",
            "Medium",
            "trust",
            "pick",
            "Turn",
        ] {
            assert!(!long.contains(word), "{long}");
        }
    }
}

#[test]
fn a_beat_error_says_where_its_sides_were_named_by_count() {
    let caveats = [Caveat::SidesByCount { count: 2 }];
    let be = reading_limits("Beat Error", &caveats, 52.0, 30);
    assert_eq!(be.len(), 1);
    assert_eq!(be[0].0, "sides named by count");
    assert!(be[0].1.contains("not told apart by their sound"));
    // It doesn't touch the rate or the amplitude.
    assert!(reading_limits("Rate", &caveats, 52.0, 30).is_empty());
    assert!(reading_limits("Amplitude", &caveats, 52.0, 30).is_empty());
}

/// Words in a reading's place on a narrow card show whole: "No
/// Regular Beat" takes the dash's room rather than being cut.
#[test]
fn a_narrow_cards_words_show_whole() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let status = ("No Regular Beat".to_string(), String::new());
    for compact in [true, false] {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(96.0, 40.0));
        let mut texts = Vec::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    readouts::figure_row(ui, rect, None, "s/d", Some(&status), "", compact);
                });
            });
            texts = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some((
                        t.galley.text().to_string(),
                        t.galley.rect.translate(t.pos.to_vec2()),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
        }
        let words: String = texts
            .iter()
            .filter(|t| t.0 != "—")
            .map(|t| t.0.clone())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(words, "No Regular Beat", "{texts:?}");
        for t in &texts {
            assert!(t.1.right() <= rect.right() + 0.5, "{compact}: {t:?} is cut");
        }
    }
}

/// The texts a readings card's figure row paints in `rect`, and where.
fn figure_texts(
    rect: egui::Rect,
    value: &str,
    unit: &str,
    compact: bool,
) -> Vec<(String, egui::Rect)> {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut texts = Vec::new();
    for _ in 0..2 {
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                readouts::figure_row(ui, rect, Some(value), unit, None, "", compact);
            });
        });
        texts = out
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
    texts
}

/// A card's ± shows whole beside its figure where there is room; where
/// there isn't, it stacks over its unit, then leaves the card, and is
/// never cut into a number that reads as zero (#825).
#[test]
fn a_cards_plus_minus_shows_whole_or_not_at_all() {
    for compact in [false, true] {
        let wide = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(400.0, 48.0));
        let texts = figure_texts(wide, "+12.3", "± 0.04 s/d", compact);
        let words: Vec<&str> = texts.iter().map(|t| t.0.as_str()).collect();
        let whole = if compact {
            vec!["+12.3", "± 0.04", "s/d"]
        } else {
            vec!["+12.3", "± 0.04 s/d"]
        };
        assert_eq!(words, whole, "{compact}");
        for w in [150.0, 120.0, 96.0, 80.0] {
            let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(w, 48.0));
            for (unit, rest) in [("± 0.04 s/d", "s/d"), ("± 0.4°", "")] {
                let texts = figure_texts(rect, "+12.3", unit, compact);
                for t in &texts {
                    assert!(
                        t.1.right() <= rect.right() + 0.5,
                        "{w} {compact}: {t:?} is cut"
                    );
                }
                let side: Vec<&str> = texts.iter().skip(1).map(|t| t.0.as_str()).collect();
                let (pm, u) = unit
                    .rsplit_once(' ')
                    .filter(|(a, _)| a.contains(' '))
                    .unwrap_or((unit, ""));
                let allowed: [Vec<&str>; 3] = [vec![unit], vec![pm, u], rest_lines(rest)];
                assert!(allowed.contains(&side), "{w} {compact}: {side:?}");
            }
        }
    }
}

fn rest_lines(rest: &str) -> Vec<&str> {
    if rest.is_empty() {
        Vec::new()
    } else {
        vec![rest]
    }
}

/// On the Tick Tock Profile an edge's bar and a sound's ring are drawn
/// on the trace, the ring hollow; on a short plot, with the edges'
/// names inside it, no bar or ring crosses a name.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn the_profiles_marks_keep_off_the_names() {
    let mut app = synthetic(30.0, 0.0);
    // Stopped, the profile is the one kept at the end.
    let end = app.latest_s();
    let sides = app.live.as_ref().unwrap().tick_profiles().clone();
    app.sound_history = vec![(end, sides)];
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let run = |app: &mut TimegrapherApp, w: f32, h: f32| {
        let mut shapes = Vec::new();
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(w, h),
                )),
                ..Default::default()
            };
            shapes = ctx.run(input, |ctx| app.panels(ctx)).shapes;
        }
        let names: Vec<egui::Rect> = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) if ["Unlock", "Drop", "Peak"].contains(&t.galley.text()) => {
                    Some(t.galley.rect.translate(t.pos.to_vec2()))
                }
                _ => None,
            })
            .collect();
        let rings: Vec<egui::Rect> = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Circle(r) if r.fill == egui::Color32::TRANSPARENT => Some(
                    egui::Rect::from_center_size(r.center, egui::Vec2::splat(2.0 * r.radius)),
                ),
                _ => None,
            })
            .collect();
        let bars: Vec<egui::Rect> = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::LineSegment { points, stroke }
                    if stroke.width == 2.0 && points[0].y == points[1].y =>
                {
                    Some(egui::Rect::from_two_pos(points[0], points[1]))
                }
                _ => None,
            })
            .collect();
        let facts: Vec<egui::Rect> = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t)
                    if ["Amplitude", "Unlock to Drop"].contains(&t.galley.text())
                        || t.galley.text().ends_with('°')
                        || t.galley.text().ends_with(" ms") =>
                {
                    Some(t.galley.rect.translate(t.pos.to_vec2()))
                }
                _ => None,
            })
            .collect();
        (names, rings, bars, facts)
    };
    // Where the facts box is two rows, over a trace that rises under
    // the left corner and is low on the right, it takes the right, and
    // keeps off the marks.
    let kept = app.sound_history.clone();
    for (_, sides) in &mut app.sound_history {
        for p in sides.iter_mut().flatten() {
            let top = p.p90.iter().copied().fold(0.0f32, f32::max);
            let n = p.median.len() / 5;
            p.median[..n].fill(0.9 * top);
        }
    }
    app.sound_opts.side_by_side = true;
    let (_, rings, bars, facts) = run(&mut app, 1440.0, 820.0);
    let plots: Vec<egui::Rect> = (0..2usize)
        .filter_map(|k| {
            ctx.data(|d| d.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new(("profile plot", k))))
                .map(|p| p.0)
        })
        .collect();
    assert_eq!(plots.len(), 2);
    for p in &plots {
        let here: Vec<_> = facts.iter().filter(|f| p.contains_rect(**f)).collect();
        assert_eq!(here.len(), 4, "two rows: {here:?}");
        assert!(
            here.iter().all(|f| f.left() > p.center().x),
            "{p:?}: {here:?}"
        );
    }
    for m in rings.iter().chain(&bars) {
        assert!(
            facts.iter().all(|f| !f.intersects(*m)),
            "{m:?} under the facts: {facts:?}"
        );
    }
    // Over a trace high under both corners, the box keeps no rows: the
    // trace and its marks come first, and the hover has the facts.
    for (_, sides) in &mut app.sound_history {
        for p in sides.iter_mut().flatten() {
            let top = p.p90.iter().copied().fold(0.0f32, f32::max);
            let n = p.median.len();
            p.median[n - n / 4..].fill(0.9 * top);
        }
    }
    let (_, _, _, facts) = run(&mut app, 1440.0, 820.0);
    assert!(
        facts
            .iter()
            .all(|f| plots.iter().all(|p| !p.contains_rect(*f))),
        "{facts:?}"
    );
    app.sound_history = kept;
    app.sound_opts.side_by_side = false;
    let (names, rings, bars, _) = run(&mut app, 1440.0, 1000.0);
    assert!(!names.is_empty(), "the edges named");
    assert!(!rings.is_empty() && !bars.is_empty(), "{rings:?} {bars:?}");
    // A short window that still draws the plot: at 800 x 600 it
    // says there is no room instead.
    let (names, rings, bars, _) = run(&mut app, 800.0, 640.0);
    assert!(!names.is_empty(), "the edges named in the short plot");
    for m in rings.iter().chain(&bars) {
        assert!(
            names.iter().all(|n| !n.intersects(*m)),
            "{m:?} crosses a name: {names:?}"
        );
    }
}

/// In Quick Listen, a calibre picked while listening, at the beat rate
/// Auto already found, adds to the take: the beats and the strip's
/// readings heard so far stay. A calibre at another rate starts the
/// beats again.
#[test]
fn a_calibre_picked_while_listening_keeps_what_was_heard() {
    let mut app = synthetic(30.0, 0.0);
    assert_eq!(app.bph, None);
    let heard = app.live.as_ref().unwrap().beats().len();
    let trend = app.trend.len();
    assert!(heard > 100 && trend > 5, "{heard} {trend}");
    assert_eq!(app.cal.bph("ETA 2824-2"), Some(28_800));
    app.set_calibre("ETA 2824-2".into());
    assert_eq!(app.bph, Some(28_800));
    assert_eq!(app.live.as_ref().unwrap().beats().len(), heard);
    assert_eq!(app.trend.len(), trend);
    // Auto again, with no calibre: still all of it.
    app.set_calibre(String::new());
    assert_eq!(app.bph, None);
    assert_eq!(app.live.as_ref().unwrap().beats().len(), heard);
    assert_eq!(app.trend.len(), trend);
    let other = ["Seiko 7S26", "Seiko 7S26A", "Miyota 8215", "Seiko NH35"]
        .into_iter()
        .find(|c| app.cal.bph(c).is_some_and(|b| b != 28_800))
        .expect("a calibre at another rate");
    app.set_calibre(other.into());
    assert!(app.live.as_ref().unwrap().beats().is_empty());
}

/// Noise with no regular beat: the strip shows the peaks heard,
/// faintly and said to be possible beats, rather than an empty card.
#[test]
fn noise_shows_its_possible_beats_on_the_strip() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "noise".into());
    let mut seed = 12_345_u32;
    let x: Vec<f32> = (0..48_000 * 20)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32 * 0.02 - 0.01
        })
        .collect();
    let live = app.live.as_mut().unwrap();
    for b in x.chunks(960) {
        if live.push(b) {
            let r = live.reading(app.average_s);
            app.trend.push(TrendPoint::of(r.time_s, &r));
        }
    }
    app.poll();
    assert!(app.no_watch_yet());
    assert!(!app.live.as_ref().unwrap().candidates().is_empty());
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.strip_pane(ui));
    });
    let drawn = format!("{:?}", out.shapes);
    assert!(drawn.contains("No regular beat found yet"), "{drawn}");
    assert!(drawn.contains("Circle"), "the possible beats are drawn");
}

/// A paused session carries on only with the input it was listening to:
/// another input's clock correction and clock measurement aren't its.
#[test]
fn resuming_on_another_input_is_refused() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.live = Some(LiveAnalyzer::new(48_000, LiveConfig::default()));
    app.clock_track = Some(ClockTrack {
        device: "Mic A".into(),
        shown: "Mic A (default)".into(),
        tracker: timegrapher_core::clock::ClockTracker::new(48_000),
        next_s: CLOCK_MEASURE_S,
        fit: None,
    });
    // Compared by the input's name, named as the microphone menu shows it.
    assert_eq!(app.resume_refusal("Mic A", "Mic A (direct)", 48_000), None);
    let why = app
        .resume_refusal("Mic B", "Mic B (direct)", 48_000)
        .expect("another input refused");
    assert!(
        why.contains("Mic A (default)") && why.contains("Mic B (direct)"),
        "{why}"
    );
    assert!(app.resume_refusal("Mic A", "Mic A", 44_100).is_some());
}

/// Stopped after the watch is taken off the stand, the readings stay on
/// the last stretch with a watch heard, and a long enough take shows
/// its whole-take reading.
#[test]
fn stopping_keeps_the_last_heard_readings() {
    let mut app = synthetic(60.0, 12.0);
    let before = app.reading().unwrap();
    let whole = app.whole_take().expect("whole take");
    let rate = whole.m.rate_s_per_day.unwrap();
    assert!((rate - 12.0).abs() < 2.0, "{rate}");
    assert!(whole.span > 40.0, "{}", whole.span);
    // Then 20 s of hiss with the watch gone.
    let mut seed = 1u32;
    let noise: Vec<f32> = (0..48000 * 20)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 0.02 - 0.01
        })
        .collect();
    let live = app.live.as_mut().unwrap();
    for b in noise.chunks(960) {
        if live.push(b) {
            let r = live.reading(app.average_s);
            app.trend.push(TrendPoint::of(r.time_s, &r));
        }
    }
    let after = app.reading().unwrap();
    let (a, b) = (
        before.rate_s_per_day.unwrap(),
        after.rate_s_per_day.unwrap(),
    );
    assert!((a - b).abs() < 1.0, "{a} then {b}");
    assert!(after.time_s <= 62.0, "{}", after.time_s);
}

/// Lay out and paint frames without a window, in both strip directions,
/// with a synthetic watch run through the live engine.
#[test]
fn draws_a_synthetic_watch_headless() {
    let mut app = synthetic(12.0, 20.0);
    assert!(app.anchor.is_some(), "strip not anchored");
    let r = app.reading().unwrap();
    let rate = r.rate_s_per_day.unwrap();
    assert!((rate - 20.0).abs() < 2.0, "rate {rate}");

    let ctx = themed();
    for horizontal in [false, true] {
        app.strip.horizontal = horizontal;
        app.panes = default_layout(horizontal);
        app.follow = horizontal;
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1280.0, 820.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| app.panels(ctx));
            assert!(!out.shapes.is_empty());
        }
    }
}

fn export_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tg-app-export-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Export as SVG: the next frame draws the pane with its name in place
/// of its toolbar and writes what it painted.
#[test]
fn a_pane_is_exported_as_an_svg_drawing() {
    let mut app = synthetic(12.0, 20.0);
    let ctx = themed();
    let _ = ctx.run(screen(), |ctx| app.panels(ctx));
    let dir = export_dir("svg");
    let path = dir.join("Strip.svg");
    app.start_export(Pane::Strip, path.clone());
    let _ = ctx.run(screen(), |ctx| {
        app.export_tick(ctx);
        app.panels(ctx)
    });
    assert!(app.export.is_none(), "export still waiting");
    let doc = std::fs::read_to_string(&path).unwrap();
    assert!(doc.starts_with("<svg "), "{doc}");
    assert!(doc.contains(">Paper Strip<") || doc.contains(&format!(">{}<", Pane::Strip.title())));
    assert!(
        doc.contains("<polyline") || doc.contains("<line"),
        "no lines"
    );
    // Its controls aren't in the picture, and the window's frame goes on
    // as before: the profile below keeps its toolbar (its Layout
    // setting, first in the row, stands for it).
    assert!(!doc.contains("Width"), "toolbar drawn into the picture");
    assert!(!doc.contains("Export…"), "toolbar drawn into the picture");
    assert!(
        theme::tools_drawn(&ctx).contains(&format!("{}/Layout", Pane::Sound.title())),
        "other panes lost their toolbars"
    );
    assert!(matches!(&app.message, Some((m, false)) if m.starts_with("Saved ")));
    let _ = std::fs::remove_dir_all(dir);
}

/// Export as PNG: a screenshot is asked for, and the pane is cut from it.
#[test]
fn a_pane_is_exported_as_a_png_from_a_screenshot() {
    let mut app = synthetic(12.0, 20.0);
    let ctx = themed();
    let _ = ctx.run(screen(), |ctx| app.panels(ctx));
    let dir = export_dir("png");
    let path = dir.join("Strip.png");
    app.start_export(Pane::Strip, path.clone());
    let out = ctx.run(screen(), |ctx| {
        app.export_tick(ctx);
        app.panels(ctx)
    });
    let asked = out
        .viewport_output
        .values()
        .flat_map(|v| &v.commands)
        .any(|c| matches!(c, egui::ViewportCommand::Screenshot(_)));
    assert!(asked, "no screenshot asked for");
    let rect = app
        .export
        .as_ref()
        .and_then(|e| e.rect)
        .expect("pane drawn");
    // The window as a screen of one colour, the pane's corner marked.
    let mut image = egui::ColorImage::filled([1280, 820], Color32::from_gray(40));
    let corner = rect.min.round();
    image.pixels[corner.y as usize * 1280 + corner.x as usize] = Color32::RED;
    let mut input = screen();
    input.events.push(egui::Event::Screenshot {
        viewport_id: egui::ViewportId::ROOT,
        user_data: Default::default(),
        image: std::sync::Arc::new(image),
    });
    let _ = ctx.run(input, |ctx| {
        app.export_tick(ctx);
        app.panels(ctx)
    });
    assert!(app.export.is_none(), "export still waiting");
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    assert_eq!(info.width, rect.width().round() as u32);
    assert_eq!(info.height, rect.height().round() as u32);
    assert_eq!(&buf[..4], &[255, 0, 0, 255], "not cut at the pane");
    let _ = std::fs::remove_dir_all(dir);
}

/// A pane that's never drawn gives up with a message, not a wait.
#[test]
fn an_export_that_cannot_be_drawn_gives_up() {
    let mut app = synthetic(2.0, 0.0);
    let ctx = themed();
    app.start_export(Pane::Strip, export_dir("never").join("x.png"));
    for _ in 0..61 {
        let _ = ctx.run(screen(), |ctx| app.export_tick(ctx));
    }
    assert!(app.export.is_none());
    assert!(matches!(&app.message, Some((_, true))));
}

/// Clipped sound shows in a chip on the Amplitude and Beat Error cards,
/// with nothing pushed in over the readings; a stored clock error is
/// in Settings, not on the Rate card, the corrected rate in the Rate
/// figure's hover.
#[test]
fn clipping_shows_in_a_chip_and_the_true_clock_in_the_rate_hover() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "syn".into());
    let audio = generate(
        &SynthConfig {
            duration_s: 12.0,
            rate_s_per_day: 10.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let peak = audio.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
    let live = app.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        let hot: Vec<f32> = b
            .iter()
            .map(|v| (v * 4.0 / peak).clamp(-1.0, 1.0))
            .collect();
        live.push(&hot);
    }
    app.clock = Some(clockstore::DeviceClock {
        device: "syn".into(),
        ppm: 20.0,
        ppm_sd: None,
        span_s: 600.0,
        measured_utc: "2026-10-10T13:00:00Z".into(),
        source: "measure".into(),
    });
    let ctx = themed();
    let mut text = String::new();
    for w in [400.0, 400.0, 1610.0, 1610.0, 1610.0] {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(w, 820.0),
            )),
            ..Default::default()
        };
        text = painted_text(&ctx.run(input, |ctx| app.panels(ctx)));
    }
    assert!(!text.contains("Clipping on"), "no banner: {text}");
    assert!(
        text.lines().filter(|l| l.ends_with("% Clipped")).count() >= 2,
        "{text}"
    );
    // The clock kept is Settings' to show.
    assert!(!text.contains("20.0 ppm slow"), "{text}");
    // The rate on it, in the Rate figure's hover: 20 ppm slow takes
    // 1.7 s/d off a rate read on the card.
    let card = app.reading().unwrap().rate_s_per_day.unwrap();
    let shown = format!("{:+.1}", clockstore::correct_rate(card, 20.0)).replace('-', "−");
    let hover = app.true_clock_words(card).expect("a clock kept");
    assert!(hover.starts_with(&format!("True Clock {shown}")), "{hover}");
}

/// The window before anything is open, with a problem showing, in both
/// themes.
#[test]
fn draws_the_empty_and_error_states() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.error("Can't read take.flac".into());
    let ctx = themed();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.set_theme(theme);
        frames(&mut app, &ctx, 2);
    }
    app.message = None;
    app.input = Input::File;
    frames(&mut app, &ctx, 1);
}
