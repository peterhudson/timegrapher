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

#[test]
fn a_drop_beat_error_caveat_belongs_only_on_beat_error() {
    let caveats = [Caveat::FROM_DROP];
    let be = reading_limits("Beat Error", &caveats, 52.0, 30);
    assert_eq!(be.len(), 1);
    assert_eq!(be[0].0, "beat error from the drop");
    assert!(be[0].1.contains("unlock was not found"));
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

/// A card too narrow for its caption (800 pt) names its reading by the
/// short form the axes use, the whole caption on a hover, never a cut
/// "Ampli…" (#1061); with room, the whole caption.
#[test]
fn a_narrow_cards_title_is_short_not_cut() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let title = |caption: &str, w: f32| {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(w, 24.0));
        let mut texts = Vec::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let short = readouts::cards_short(ui, &[(rect, caption)]);
                    readouts::card_title(ui, rect, caption, &["Help."], &[], (None, short));
                });
            });
            texts = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .filter(|t| t != "?")
                .collect::<Vec<_>>();
        }
        texts
    };
    // A card's title row at 800 pt, as Walter walked it.
    for (caption, short) in [
        ("Amplitude", "Amp."),
        ("Beat Error", "B.E."),
        ("Rate", "Rate"),
    ] {
        assert_eq!(title(caption, 76.0), [short], "{caption}");
        assert_eq!(title(caption, 300.0), [caption]);
    }
}

/// The readings cards switch to their short names together (Hugh, #1061):
/// one too narrow for its caption takes the others with it, so a row
/// never mixes "Amplitude" with "B.E."; widened again, they are whole on
/// the very next pass, with nothing held over (Vernon).
#[test]
fn the_cards_name_their_readings_short_together() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let rect = |x: f32, w: f32| egui::Rect::from_min_size(egui::pos2(x, 10.0), egui::vec2(w, 24.0));
    let row = |beat_error_w: f32| {
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let titles = [
                    (rect(10.0, 300.0), "Amplitude"),
                    (rect(400.0, beat_error_w), "Beat Error"),
                ];
                let short = readouts::cards_short(ui, &titles);
                for (r, caption) in titles {
                    readouts::card_title(ui, r, caption, &["Help."], &[], (None, short));
                }
            });
        });
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .filter(|t| t != "?")
            .collect::<Vec<_>>()
    };
    for _ in 0..3 {
        assert_eq!(row(76.0), ["Amp.", "B.E."]);
    }
    assert_eq!(row(300.0), ["Amplitude", "Beat Error"]);
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
        // Where the ± fits beside the figure made half its size, it
        // stays whole on its line, as before #825.
        if !compact {
            let (fig, side) = (texts[0].1.width(), texts[1].1.width());
            let w = 0.5 * fig + 6.0 + side + 1.0;
            let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(w, 48.0));
            let near = figure_texts(rect, "+12.3", "± 0.04 s/d", compact);
            let words: Vec<&str> = near.iter().map(|t| t.0.as_str()).collect();
            assert_eq!(words, whole, "{w}");
            assert!(
                near.iter().all(|t| t.1.right() <= rect.right() + 0.5),
                "{near:?}"
            );
            // The floor is the figure's least size, 12 pt of 32: just
            // above it the ± stays on its line; just below, it doesn't.
            let at = |pt: f32, pad: f32| {
                let w = pt / theme::FIGURE_PT * fig + 6.0 + side + pad;
                let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(w, 48.0));
                figure_texts(rect, "+12.3", "± 0.04 s/d", compact)
                    .iter()
                    .map(|t| t.0.clone())
                    .collect::<Vec<_>>()
            };
            assert_eq!(at(12.0, 1.0), whole, "at the floor");
            assert_ne!(at(11.0, -1.0), whole, "below the floor");
        }
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
    let span_s = app.average_s;
    app.sound_history = vec![(end, scope_panes::Kept { span_s, sides })];
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
    for (_, k) in &mut app.sound_history {
        for p in k.sides.iter_mut().flatten() {
            let top = p.p90.iter().copied().fold(0.0f32, f32::max);
            let n = p.median.len() / 5;
            p.median[..n].fill(0.9 * top);
        }
    }
    app.sound_opts.side_by_side = true;
    let (_, rings, bars, facts) = run(&mut app, 1440.0, 788.0);
    let plots: Vec<egui::Rect> = (0..2usize)
        .filter_map(|k| {
            ctx.data(|d| d.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new(("profile plot", k))))
                .map(|p| p.0)
        })
        .collect();
    assert_eq!(plots.len(), 2);
    for p in &plots {
        let here: Vec<_> = facts.iter().filter(|f| p.contains_rect(**f)).collect();
        assert_eq!(here.len(), 2, "one row: {here:?}");
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
    // Over a trace high under both top corners, the box is still drawn
    // in each plot, in a bottom corner or else top left: a fact is never
    // dropped (Hugh, #1626).
    for (_, k) in &mut app.sound_history {
        for p in k.sides.iter_mut().flatten() {
            let top = p.p90.iter().copied().fold(0.0f32, f32::max);
            let n = p.median.len();
            p.median[n - n / 4..].fill(0.9 * top);
        }
    }
    let (_, _, _, facts) = run(&mut app, 1440.0, 788.0);
    for p in &plots {
        assert!(
            facts.iter().any(|f| p.contains_rect(*f)),
            "{p:?}: {facts:?}"
        );
    }
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

/// The Rate figure's hover says what its ± waits for, until the take
/// holds the 30 values it takes its scatter from, and then its exact
/// size (#1470).
#[test]
fn the_rate_hover_says_what_its_plus_minus_waits_for() {
    let app = synthetic(60.0, 12.0);
    let r = app.reading().unwrap();
    assert!(r.rate_s_per_day.is_some() && r.rate_error_s_per_day.is_none());
    let words = readouts::rate_pm_words(&r).unwrap();
    let has = format!(
        "± after 5 min of beats: it needs 30 rates of 10 s each, and has {}.",
        r.rate_noise_values
    );
    assert_eq!(words, has);
    assert!((3..30).contains(&r.rate_noise_values), "{words}");
    let mut with = r.clone();
    with.rate_error_s_per_day = Some(0.1234);
    let words = readouts::rate_pm_words(&with).unwrap();
    assert!(
        words.starts_with("± 0.2 s/d is one standard error (0.123, rounded up)."),
        "{words}"
    );
}

/// The Amplitude figure's hover says why it has no ±: too few readings
/// in the take yet, or too few of its own in the average (#1470).
#[test]
fn the_amplitude_hover_says_why_it_has_no_plus_minus() {
    let app = synthetic(60.0, 12.0);
    let r = app.reading().unwrap();
    let mut early = r.clone();
    (early.amplitude_error_deg, early.amplitude_noise_values) = (None, 12);
    assert_eq!(
        readouts::amplitude_pm_words(&early).as_deref(),
        Some("± after 1 min of beats: it needs 30 readings of 2 s each, and has 12.")
    );
    let mut few = r.clone();
    (
        few.amplitude_error_deg,
        few.amplitude_noise_values,
        few.amplitude_values,
    ) = (None, 30, 2);
    assert_eq!(
        readouts::amplitude_pm_words(&few).as_deref(),
        Some(
            "No ± now: 2 of the 2 s readings in this average found the amplitude, \
             and a ± needs 3."
        )
    );
    let mut with = r.clone();
    with.amplitude_error_deg = Some(12.4);
    let words = readouts::amplitude_pm_words(&with).unwrap();
    assert!(
        words.starts_with("± 13° is one standard error (12.40, rounded up)."),
        "{words}"
    );
    // The live reading counts them as core does.
    assert!(
        r.amplitude_values > 0 && r.amplitude_noise_values <= 30,
        "{r:?}"
    );
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
    let shown = timegrapher_core::figure_text::true_minus(&format!(
        "{:+.1}",
        clockstore::correct_rate(card, 20.0)
    ));
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

/// A long run keeps the tick and tock sound every 2 s for its last hour
/// and one a minute before that (#483): 80 h of it is under 7,000
/// sounds, about 45 MB, where every 2 s was 144,000, about 1 GB.
#[test]
fn a_long_runs_sound_is_kept_thinned_before_its_last_hour() {
    use scope_panes::{kept_sound, thin_sound};
    use timegrapher_core::profile::{SOUND_FULL_S, SOUND_OLD_EVERY_S};
    let sides: Profiles = [None, None];
    let mut h: Vec<(f64, Profiles)> = Vec::new();
    let mut t = 0.0;
    while t <= 80.0 * 3600.0 {
        h.push((t, sides.clone()));
        thin_sound(&mut h);
        t += SOUND_EVERY_S;
    }
    assert!(h.len() < 7_000, "{}", h.len());
    let last = h.last().unwrap().0;
    let full: Vec<f64> = h
        .iter()
        .map(|e| e.0)
        .filter(|&t| t >= last - SOUND_FULL_S)
        .collect();
    assert_eq!(full.len(), 1801);
    assert!(full
        .windows(2)
        .all(|w| (w[1] - w[0] - SOUND_EVERY_S).abs() < 1e-9));
    assert!(h
        .windows(2)
        .filter(|w| w[1].0 < last - SOUND_FULL_S)
        .all(|w| (w[1].0 - w[0].0 - SOUND_OLD_EVERY_S).abs() < 1e-6));
    assert_eq!(h[0].0, 0.0);

    // A moment of the last hour shows the sound of its 2 s; an older one
    // the sound of its minute, at the time it was kept.
    assert_eq!(kept_sound(&h, last - 1.0).map(|e| e.0), Some(last - 2.0));
    assert_eq!(kept_sound(&h, 3600.0 + 59.0).map(|e| e.0), Some(3600.0));
    assert_eq!(kept_sound(&h, 3600.0 + 61.0).map(|e| e.0), Some(3660.0));
    // A moment beyond what was kept shows none.
    assert!(kept_sound(&h, last + 3.0).is_none());
    assert!(kept_sound(&h, -1.0).is_none());
}

/// Within the last hour a gap in the sound kept is not filled by the
/// sound before it, as before (#483): only an older moment reaches back
/// up to a minute.
#[test]
fn a_recent_gap_in_the_sound_kept_shows_none() {
    use scope_panes::kept_sound;
    let sides: Profiles = [None, None];
    let h: Vec<(f64, Profiles)> = [0.0, 2.0, 4.0, 30.0, 32.0]
        .into_iter()
        .map(|t| (t, sides.clone()))
        .collect();
    assert_eq!(kept_sound(&h, 5.0).map(|e| e.0), Some(4.0));
    assert!(kept_sound(&h, 20.0).is_none());
    assert_eq!(kept_sound(&h, 31.0).map(|e| e.0), Some(30.0));
}

/// A reading card's sparkline reads the card's own figure each second over
/// the last minute, the figure itself last, and a second look from the
/// seconds it kept gives the same (Reading Card Sparklines).
#[test]
fn a_cards_sparkline_is_its_figure_over_the_last_minute() {
    let ctx = themed();
    let app = synthetic(90.0, 20.0);
    let r = app.reading().expect("a reading");
    let pts = app.spark_trend(&ctx, &r);
    let last = pts.last().expect("points");
    assert_eq!(last.t, r.time_s);
    assert_eq!(last.rate_s_per_day, r.rate_s_per_day);
    assert!(pts.len() >= 59 && pts.len() <= 61, "{}", pts.len());
    assert!(
        pts.first().unwrap().t > r.time_s - 61.0,
        "{:?}",
        pts.first()
    );
    let rates: Vec<f64> = pts.iter().filter_map(|p| p.rate_s_per_day).collect();
    assert!(rates.len() > 40, "{rates:?}");
    assert!(rates.iter().all(|v| (v - 20.0).abs() < 2.0), "{rates:?}");
    // Read again, the kept seconds give the same points.
    assert_eq!(app.spark_trend(&ctx, &r), pts);
}

/// With the strip's newest edge in a masked stretch, the Scope says
/// "Masked at" as the Tick Tock Profile does, by the same rule, and draws
/// no beats there.
#[test]
fn the_scope_says_masked_where_the_profile_does() {
    let mut app = synthetic(30.0, 0.0);
    let end = app.latest_s() - 5.0;
    app.strip_end = Some(end);
    let end = app.strip_end_s();
    app.masked = vec![(end - 15.0, end + 1.0)];
    let at = length_text(end);
    let line = Some(format!("Masked at {at}"));
    assert_eq!(app.profiles_at(end).line, line);
    let ctx = themed();
    let out = ctx.run(screen(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.scope_pane(ui));
    });
    let scope = painted_text(&out);
    assert!(scope.contains(&format!("Masked at {at}")), "{scope}");
}

/// Tick's and Tock's amplitudes are at the amplitude's place, 1°, as
/// the figure beside them and the CLI write them, whatever the ± (#1470).
#[test]
fn tick_and_tock_take_the_amplitudes_decimals() {
    assert_eq!(
        readouts::sides_text(279.494, 279.571, Some(0.04)),
        "Tick 279°, Tock 280°: each side's own amplitude."
    );
    // With no ±, the amplitude's base decimals, as before.
    let base = |x| {
        timegrapher_core::library::fixed_text(x, timegrapher_core::library::base_digits::AMPLITUDE)
    };
    assert_eq!(
        readouts::sides_text(279.494, 279.571, None),
        format!(
            "Tick {}°, Tock {}°: each side's own amplitude.",
            base(279.494),
            base(279.571)
        )
    );
}

/// The Amplitude card's hover gives Tick and Tock at the amplitude's
/// place, as its figure (#1114, #1470).
#[test]
fn the_amplitude_hover_gives_tick_and_tock_the_live_decimals() {
    // Past the 30 amplitude windows the ± takes its scatter from (#1470).
    let mut app = synthetic(90.0, 5.0);
    let r = app.reading().expect("a reading");
    let (Some(a), Some(b), Some(se)) = (
        r.amplitude_even_deg,
        r.amplitude_odd_deg,
        r.amplitude_error_deg,
    ) else {
        panic!("the synthetic take reads both sides and a ±: {r:?}");
    };
    let want = readouts::sides_text(a, b, Some(se));
    // The ± never moves the place (#1470).
    assert_eq!(want, readouts::sides_text(a, b, None));
    let figure = readouts::amplitude_figure(&r).expect("a figure");
    let ctx = themed();
    ctx.style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    let mut at = None;
    let mut text = String::new();
    for _ in 0..6 {
        let input = egui::RawInput {
            events: at.map(egui::Event::PointerMoved).into_iter().collect(),
            ..screen()
        };
        let out = ctx.run(input, |ctx| app.panels(ctx));
        at = at.or_else(|| {
            out.shapes.iter().find_map(|c| match &c.shape {
                egui::Shape::Text(t) if figure.starts_with(t.galley.text()) => {
                    Some(t.galley.rect.translate(t.pos.to_vec2()).center())
                }
                _ => None,
            })
        });
        text = painted_text(&out);
    }
    assert!(at.is_some(), "{figure} drawn: {text}");
    assert!(text.contains(&want), "{want} in: {text}");
}

/// The timed clock's hint writes the ppm and its ± at the clock's place,
/// 0.1 ppm, the ± rounded up there (#1470), and the sign is said in words
/// after it.
#[test]
fn a_timed_clocks_figures_are_at_its_place() {
    use super::readouts::timed_clock_text;
    assert_eq!(timed_clock_text(0.84, 0.06), "0.8 ± 0.1");
    assert_eq!(timed_clock_text(-20.24, 0.3), "20.2 ± 0.3");
    assert_eq!(timed_clock_text(0.84, 0.0004), "0.8 ± 0.1");
}
