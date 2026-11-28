//! Inputs and their gain, what the cards and readings say, the profiles,
//! exporting a pane, and the empty and error states.

use super::readouts::{beats_were_regular, no_watch_hover, reading_limits, take_limits};
use super::tests::{frames, painted_text, screen, synthetic, synthetic_set, themed};
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
    // A short stretch bears on every card of the reading shown: said once,
    // in the status bar, and on no card (#1975).
    for title in ["Rate", "Amplitude", "Beat Error"] {
        assert!(
            reading_limits(title, &caveats, 52.0, 30, false).is_empty(),
            "{title}"
        );
    }
    let take = take_limits(&caveats, 52.0, 30, false);
    let short: Vec<&str> = take.iter().map(|f| f.0.as_str()).collect();
    // The lift angle and the input's clock are said elsewhere.
    assert_eq!(short, ["4 s of beats"]);
    let missing = take_limits(&[Caveat::BeatsMissing { fraction: 0.9 }], 52.0, 90, false);
    assert_eq!(missing[0].0, "90 of 100 beats");
    for (_, long) in take.iter().chain(&missing) {
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
    let be = reading_limits("Beat Error", &caveats, 52.0, 30, false);
    assert_eq!(be.len(), 1);
    assert_eq!(be[0].0, "sides named by count");
    assert!(be[0].1.contains("not told apart by their sound"));
    // It doesn't touch the rate or the amplitude.
    assert!(reading_limits("Rate", &caveats, 52.0, 30, false).is_empty());
    assert!(reading_limits("Amplitude", &caveats, 52.0, 30, false).is_empty());
}

#[test]
fn a_drop_beat_error_caveat_belongs_only_on_beat_error() {
    let caveats = [Caveat::FROM_DROP];
    let be = reading_limits("Beat Error", &caveats, 52.0, 30, false);
    assert_eq!(be.len(), 1);
    assert_eq!(be[0].0, "beat error from the drop");
    assert!(be[0].1.contains("unlock was not found"));
    assert!(reading_limits("Rate", &caveats, 52.0, 30, false).is_empty());
    assert!(reading_limits("Amplitude", &caveats, 52.0, 30, false).is_empty());
}

/// Sides too faint to read give neither an amplitude nor an unlock beat
/// error (#1738), so they are said once, in the status bar (Norm,
/// #1975); but where they leave the Amplitude card without a figure,
/// they are said there, the reason for its dash, and not again in the
/// status bar (Hugh's rule 3).
#[test]
fn faint_sides_are_named_where_they_empty_a_card_else_for_the_take() {
    let caveats = [Caveat::FaintSidesWithheld {
        sides: 3,
        windows: 2,
    }];
    for empty in [false, true] {
        for title in ["Rate", "Beat Error"] {
            assert!(
                reading_limits(title, &caveats, 52.0, 30, empty).is_empty(),
                "{title}"
            );
        }
    }
    assert!(reading_limits("Amplitude", &caveats, 52.0, 30, false).is_empty());
    let take = take_limits(&caveats, 52.0, 30, false);
    assert_eq!(take.len(), 1);
    let facts = reading_limits("Amplitude", &caveats, 52.0, 30, true);
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].0, "3 faint sides left out");
    assert!(
        facts[0].1.starts_with("3 sides in 2 windows"),
        "{}",
        facts[0].1
    );
    assert!(take_limits(&caveats, 52.0, 30, true).is_empty());
    // A take-wide caveat on an empty card is not its reason: no chip.
    let short = [Caveat::ShortSpan { span_s: 4.0 }];
    assert!(reading_limits("Amplitude", &short, 52.0, 30, true).is_empty());
}

/// A caveat that bears on more than one card, or on the take, is said
/// once, in the status bar's chip, and on no card: breaks, clipping, a
/// set rate unlike the one heard (#1975). One that bears on one card
/// alone is on that card and not in the status bar.
#[test]
fn a_take_wide_caveat_is_said_once_for_the_take() {
    let caveats = [
        Caveat::Breaks { count: 2 },
        Caveat::Clipped { fraction: 0.03 },
        Caveat::SetRateDisagrees { heard: 21600 },
        Caveat::SidesByCount { count: 1 },
    ];
    for title in ["Rate", "Amplitude"] {
        assert!(
            reading_limits(title, &caveats, 52.0, 30, false).is_empty(),
            "{title}"
        );
    }
    let be = reading_limits("Beat Error", &caveats, 52.0, 30, false);
    assert_eq!(be.len(), 1);
    assert_eq!(be[0].0, "sides named by count");
    let take = take_limits(&caveats, 52.0, 30, false);
    assert_eq!(take.len(), 3, "{take:?}");
    assert!(take.iter().all(|t| t.0 != "sides named by count"));
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
                    readouts::figure_row(ui, rect, None, "s/d", Some(&status), "", (compact, None));
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

/// A card too narrow for its caption with its "?" (800 pt) goes without
/// the "?" and keeps its whole caption (Hugh on #1954); narrower still,
/// than the window can be made, it names its reading by the short form
/// the axes use, never a cut "Ampli…" (#1061); with room, the whole
/// caption and its "?".
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
                .collect::<Vec<_>>();
        }
        texts
    };
    // A card's title row at 800 pt, as Walter walked it, and narrower.
    // "Rate" fits beside its "?" even there.
    assert_eq!(title("Rate", 76.0), ["?", "Rate"]);
    assert_eq!(title("Rate", 300.0), ["?", "Rate"]);
    for (caption, short) in [("Amplitude", "Amp."), ("Beat Error", "B.E.")] {
        assert_eq!(title(caption, 76.0), [caption], "{caption}");
        assert_eq!(title(caption, 40.0), [short], "{caption}");
        assert_eq!(title(caption, 300.0), ["?", caption]);
    }
}

/// The readings cards go without their "?" together (Hugh, #1061 and
/// #1954): one too narrow for its caption beside its "?" takes the
/// others with it, so a row never mixes a "?" on one card with none on
/// the next; widened again, they have it back on the very next pass, with
/// nothing held over (Vernon).
#[test]
fn the_cards_go_without_their_help_marks_together() {
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
            .collect::<Vec<_>>()
    };
    for _ in 0..3 {
        assert_eq!(row(76.0), ["Amplitude", "Beat Error"]);
    }
    assert_eq!(row(300.0), ["?", "Amplitude", "?", "Beat Error"]);
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
                readouts::figure_row(ui, rect, Some(value), unit, None, "", (compact, None));
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
    let sides = app
        .bench
        .recording
        .live
        .as_ref()
        .unwrap()
        .tick_profiles()
        .clone();
    let span_s = app.bench.take.average_s;
    app.bench.results.sound_history = vec![(end, scope_panes::Kept { span_s, sides })];
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
    let kept = app.bench.results.sound_history.clone();
    for (_, k) in &mut app.bench.results.sound_history {
        for p in k.sides.iter_mut().flatten() {
            let top = p.p90.iter().copied().fold(0.0f32, f32::max);
            let n = p.median.len() / 5;
            p.median[..n].fill(0.9 * top);
        }
    }
    app.bench.panes_view.sound_opts.side_by_side = true;
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
    for (_, k) in &mut app.bench.results.sound_history {
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
    app.bench.results.sound_history = kept;
    app.bench.panes_view.sound_opts.side_by_side = false;
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
    assert_eq!(app.bench.take.bph, None);
    let heard = app.bench.recording.live.as_ref().unwrap().beats().len();
    let trend = app.bench.results.trend.len();
    assert!(heard > 100 && trend > 5, "{heard} {trend}");
    assert_eq!(app.cal.bph("ETA 2824-2"), Some(28_800));
    app.set_calibre("ETA 2824-2".into());
    assert_eq!(app.bench.take.bph, Some(28_800));
    assert_eq!(
        app.bench.recording.live.as_ref().unwrap().beats().len(),
        heard
    );
    assert_eq!(app.bench.results.trend.len(), trend);
    // Auto again, with no calibre: still all of it.
    app.set_calibre(String::new());
    assert_eq!(app.bench.take.bph, None);
    assert_eq!(
        app.bench.recording.live.as_ref().unwrap().beats().len(),
        heard
    );
    assert_eq!(app.bench.results.trend.len(), trend);
    let other = ["Seiko 7S26", "Seiko 7S26A", "Miyota 8215", "Seiko NH35"]
        .into_iter()
        .find(|c| app.cal.bph(c).is_some_and(|b| b != 28_800))
        .expect("a calibre at another rate");
    app.set_calibre(other.into());
    assert!(app
        .bench
        .recording
        .live
        .as_ref()
        .unwrap()
        .beats()
        .is_empty());
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
    let live = app.bench.recording.live.as_mut().unwrap();
    for b in x.chunks(960) {
        if live.push(b) {
            let r = live.reading(app.bench.take.average_s);
            app.bench.results.trend.push(TrendPoint::of(r.time_s, &r));
        }
    }
    app.poll();
    assert!(app.no_watch_yet());
    assert!(!app
        .bench
        .recording
        .live
        .as_ref()
        .unwrap()
        .candidates()
        .is_empty());
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.strip_pane(ui));
    });
    let drawn = format!("{:?}", out.shapes);
    // Led by the cards' own wait, as no beat is found at all (#2044).
    let note = "No Beats Heard. The faint dots are sounds that might be beats.";
    assert!(drawn.contains(note), "{note} in {drawn}");
    assert!(drawn.contains("Circle"), "the possible beats are drawn");
}

/// Noise while the watch settles is still no watch: the settle hides
/// nothing heard, so the strip still says no regular beat (#1944).
#[test]
fn noise_while_settling_is_still_no_watch() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "noise".into());
    let base = std::env::temp_dir().join(format!("tg-settle-noise-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    let _tx = super::tests::queued_capture(&mut app, &base);
    let mut seed = 54_321_u32;
    let x: Vec<f32> = (0..48_000 * 20)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32 * 0.02 - 0.01
        })
        .collect();
    let live = app.bench.recording.live.as_mut().unwrap();
    for b in x.chunks(960) {
        live.push(b);
    }
    app.session.set_settle_s(30.0);
    app.session.start_settle(0.0);
    app.bench.results.trend.clear();
    assert!(app.settling_left().is_some());
    assert!(app.no_watch_yet(), "noise is no watch, settling or not");
    let _ = std::fs::remove_dir_all(base);
}

/// A paused session carries on only with the input it was listening to:
/// another input's clock correction and clock measurement aren't its.
#[test]
fn resuming_on_another_input_is_refused() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.bench.recording.live = Some(LiveAnalyzer::new(48_000, LiveConfig::default()));
    app.bench.recording.clock_track = Some(ClockTrack {
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
    let live = app.bench.recording.live.as_mut().unwrap();
    for b in noise.chunks(960) {
        if live.push(b) {
            let r = live.reading(app.bench.take.average_s);
            app.bench.results.trend.push(TrendPoint::of(r.time_s, &r));
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
    // How many of the 30 are the run's own, from core (#1950).
    with.rate_own_values = 4;
    let words = readouts::rate_pm_words(&with).unwrap();
    assert!(
        words.ends_with(
            "Its scatter comes from 30 rates of 10 s each: 4 of this run's own and the 26 \
             nearest it in this take."
        ),
        "{words}"
    );
    with.rate_own_values = 42;
    let words = readouts::rate_pm_words(&with).unwrap();
    assert!(
        words.ends_with("Its scatter comes from this run's own 42 rates of 10 s each."),
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
        readouts::amplitude_pm_words(&early, 10.0).as_deref(),
        Some("± after 1 min of beats: it needs 30 readings of 2 s each, and has 12.")
    );
    let mut few = r.clone();
    (
        few.amplitude_error_deg,
        few.amplitude_noise_values,
        few.amplitude_values,
    ) = (None, 30, 2);
    assert_eq!(
        readouts::amplitude_pm_words(&few, 10.0).as_deref(),
        Some(
            "No ± now: 2 of the 2 s readings in this average found the amplitude, \
             and a ± needs 3."
        )
    );
    // Under 10 s the readings are the last 10 s's, as the figure's (#1951).
    assert!(readouts::amplitude_pm_words(&few, 2.0)
        .is_some_and(|w| w.starts_with("No ± now: 2 of the 2 s readings in the last 10 s")));
    let mut with = r.clone();
    with.amplitude_error_deg = Some(12.4);
    let words = readouts::amplitude_pm_words(&with, 10.0).unwrap();
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

/// Readings that repeat exactly say so in the Rate and Amplitude hovers,
/// in core's words with no count, before the waiting texts (#1908).
#[test]
fn a_figure_whose_readings_repeat_says_so() {
    use timegrapher_core::confidence::NoPm;
    let app = synthetic(60.0, 12.0);
    let r = app.reading().unwrap();
    let mut rate = r.clone();
    (rate.rate_error_s_per_day, rate.rate_no_pm) = (None, Some(NoPm::Repeats));
    assert_eq!(
        readouts::rate_pm_words(&rate).as_deref(),
        Some(NoPm::REPEATS_TEXT)
    );
    let mut amp = r.clone();
    (
        amp.amplitude_error_deg,
        amp.amplitude_noise_values,
        amp.amplitude_values,
        amp.amplitude_no_pm,
    ) = (None, 30, 30, Some(NoPm::Repeats));
    assert_eq!(
        readouts::amplitude_pm_words(&amp, 10.0).as_deref(),
        Some(NoPm::REPEATS_TEXT)
    );
    // The waiting reasons keep their words.
    amp.amplitude_no_pm = Some(NoPm::TooFew);
    assert!(readouts::amplitude_pm_words(&amp, 10.0)
        .is_some_and(|w| w.starts_with("No ± now: 30 of the 2 s readings")));
}

/// Lay out and paint frames without a window, in both strip directions,
/// with a synthetic watch run through the live engine.
#[test]
fn draws_a_synthetic_watch_headless() {
    let mut app = synthetic(12.0, 20.0);
    assert!(app.bench.recording.anchor.is_some(), "strip not anchored");
    let r = app.reading().unwrap();
    let rate = r.rate_s_per_day.unwrap();
    assert!((rate - 20.0).abs() < 2.0, "rate {rate}");

    let ctx = themed();
    for horizontal in [false, true] {
        app.bench.panes_view.strip.horizontal = horizontal;
        app.bench.panes_view.panes = default_layout(horizontal);
        app.bench.panes_view.follow = horizontal;
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
    assert!(
        app.bench.panes_view.export.is_none(),
        "export still waiting"
    );
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
        .bench
        .panes_view
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
    assert!(
        app.bench.panes_view.export.is_none(),
        "export still waiting"
    );
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
    assert!(app.bench.panes_view.export.is_none());
    assert!(matches!(&app.message, Some((_, true))));
}

/// Clipped sound, which bears on the amplitude and the beat error, shows
/// once, in the status bar's chip (#1975), with nothing pushed in over
/// the readings; a stored clock error is
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
    let live = app.bench.recording.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        let hot: Vec<f32> = b
            .iter()
            .map(|v| (v * 4.0 / peak).clamp(-1.0, 1.0))
            .collect();
        live.push(&hot);
    }
    // The figures read as a file are in, however busy the machine (#1987).
    live.read_as_file_now();
    app.bench.recording.clock = Some(clockstore::DeviceClock {
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
    assert_eq!(
        text.lines().filter(|l| l.contains("% Clipped")).count(),
        1,
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
    app.bench.recording.input = Input::File;
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
    app.bench.panes_view.strip_end = Some(end);
    let end = app.strip_end_s();
    app.bench.take.masked = vec![(end - 15.0, end + 1.0)];
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

/// The Tick Tock Profile withholds while the reading cards do: it
/// describes the same beats as the readings, so it never shows a shape
/// and its facts beside cards that say there is no figure (#1965, #2123).
/// A masked moment still says so.
#[test]
fn the_profile_withholds_while_the_cards_do() {
    let sides = synthetic(30.0, 0.0)
        .bench
        .recording
        .live
        .as_ref()
        .unwrap()
        .tick_profiles()
        .clone();
    assert!(sides.iter().all(Option::is_some), "a watch's profiles");
    let kept = |k: usize| {
        let sides = sides.clone();
        let span_s = 10.0;
        (2.0 * k as f64, scope_panes::Kept { span_s, sides })
    };
    let mut app = synthetic(30.0, 0.0);
    app.bench.results.sound_history = (5..=15).map(kept).collect();
    let end = app.strip_end_s();
    // Figures on the cards: the profiles.
    assert_eq!(app.cards_unheard(), None);
    let s = app.profiles_at(end);
    assert!(s.line.is_none() && s.profiles.iter().all(Option::is_some));
    // Silence after the shape was kept: the cards say no beats were
    // heard, and the Profile shows no shape and no facts, though one was
    // kept.
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "quiet".into());
    let live = app.bench.recording.live.as_mut().unwrap();
    for b in vec![0.0_f32; 48_000 * 30].chunks(960) {
        live.push(b);
    }
    app.bench.results.sound_history = (5..=15).map(kept).collect();
    let end = app.strip_end_s();
    let (title, _) = app.cards_unheard().expect("the cards wait");
    assert_eq!(title, "No Beats Heard");
    let s = app.profiles_at(end);
    assert!(s.profiles.iter().all(Option::is_none) && !s.masked);
    let ctx = themed();
    let out = ctx.run(screen(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.sound_pane(ui));
    });
    let drawn = painted_text(&out);
    assert!(
        !drawn.contains("Unlock"),
        "no facts beside the wait: {drawn}"
    );
    // Masked there: said as masked, as the strip's hatching says it.
    app.bench.take.masked = vec![(end - 15.0, end + 1.0)];
    let s = app.profiles_at(end);
    assert_eq!(s.line, Some(format!("Masked at {}", length_text(end))));
    assert!(s.masked);
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

/// A card with limits always shows a chip, shortened in steps where it
/// doesn't fit and never dropped (Hugh, #1808): the first limit and how
/// many more, then how many, then a bare warning chip.
#[test]
fn a_cards_limits_chip_shortens_and_is_never_dropped() {
    let two = [
        (
            "breaks from the clock log".to_string(),
            "Breaks.".to_string(),
        ),
        ("1 faint side left out".to_string(), "Faint.".to_string()),
    ];
    let one = &two[..1];
    let any = |_: &str| true;
    assert_eq!(
        readouts::limits_chip_words(&two, any),
        "Breaks From the Clock Log +1"
    );
    assert_eq!(
        readouts::limits_chip_words(one, any),
        "Breaks From the Clock Log"
    );
    let short = |w: &str| w.len() <= 8;
    assert_eq!(readouts::limits_chip_words(&two, short), "2 limits");
    assert_eq!(readouts::limits_chip_words(one, short), "1 limit");
    assert_eq!(readouts::limits_chip_words(&two, |_| false), "");
    // At its narrowest, the bare chip is drawn: the warning's triangle,
    // and none of the limits' words.
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let drawn = |limits: &[(String, String)]| {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(120.0, 24.0));
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                readouts::card_title(ui, rect, "Amplitude", &["Help."], limits, (None, false));
            });
        });
        let paths = out
            .shapes
            .iter()
            .filter(|c| matches!(&c.shape, egui::Shape::Path(p) if p.closed && p.points.len() == 3))
            .count();
        (paths, painted_text(&out))
    };
    let (none, _) = drawn(&[]);
    let (bare, text) = drawn(&two);
    assert_eq!((none, bare), (0, 1), "{text}");
    assert!(
        !text.contains("limit") && !text.contains("Breaks"),
        "{text}"
    );
}

/// At 1600 wide, the Amplitude card's title with Walter's two limits
/// from #1800 ("breaks from the clock log" and "1 faint side left out")
/// has no room for the first limit's words, and still shows a chip, its
/// count (#1808).
#[test]
fn two_limits_show_a_chip_at_1600() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    let ctx = themed();
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1600.0, 900.0),
        )),
        ..Default::default()
    };
    // The Amplitude card's title row, as the window lays it out: from its
    // caption to the end of its "?".
    let mut out = ctx.run(input(), |ctx| app.panels(ctx));
    for _ in 0..2 {
        out = ctx.run(input(), |ctx| app.panels(ctx));
    }
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
    let caption = texts
        .iter()
        .find(|t| t.0 == "Amplitude")
        .expect("its caption")
        .1;
    let help = texts
        .iter()
        .filter(|t| t.0 == "?" && t.1.min.x > caption.max.x)
        .map(|t| t.1)
        .min_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .expect("its ?");
    let rect = egui::Rect::from_min_max(
        egui::pos2(caption.min.x, caption.min.y - 4.0),
        egui::pos2(help.max.x + 4.0, caption.max.y + 4.0),
    );
    let limits = [
        (
            "breaks from the clock log".to_string(),
            "Breaks.".to_string(),
        ),
        ("1 faint side left out".to_string(), "Faint.".to_string()),
    ];
    let title = egui::Context::default();
    theme::install(&title);
    let mut text = String::new();
    for _ in 0..2 {
        text = painted_text(&title.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                readouts::card_title(ui, rect, "Amplitude", &["Help."], &limits, (None, false));
            });
        }));
    }
    assert!(
        !text.contains("Breaks From the Clock Log"),
        "{rect:?}: {text}"
    );
    assert!(text.lines().any(|l| l == "2 limits"), "{rect:?}: {text}");
}

/// A watch heard while it settles at the start of a take: nothing is read
/// yet, but the strip shows its beats, not "no regular beat" (Peter,
/// 2026-11-18, a 30 s settle at the start said so for all 30 s while the
/// Scope and Tick Tock Profile showed the beats). Noise while settling is
/// still no watch: see [`noise_while_settling_is_still_no_watch`].
#[test]
fn a_watch_heard_while_it_settles_is_no_missing_watch() {
    let mut app = synthetic(20.0, 0.0);
    let base = std::env::temp_dir().join(format!("tg-settle-heard-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    // Listening, as the watch settles live.
    let _tx = super::tests::queued_capture(&mut app, &base);
    app.session.set_settle_s(30.0);
    app.session.start_settle(0.0);
    // Nothing read while settling: no reading kept as heard.
    app.bench.results.trend.clear();
    assert!(app.settling_left().is_some());
    assert!(app.reading().is_none(), "nothing is read while settling");
    assert!(!app.no_watch_yet(), "its beats are heard");
    let _ = std::fs::remove_dir_all(base);
}

/// The reading cards' title rows and figures across a window `w` wide:
/// each caption's text, where it is drawn and the "?"s right of the first
/// reading card's left edge; and each figure's size.
struct CardRow {
    captions: Vec<(String, egui::Rect)>,
    marks: usize,
    figures: Vec<f32>,
    /// The right edge of the rate's unit, "s/d", whole.
    rate_unit_right: Option<f32>,
    /// Where limits chips are drawn: rounded plates 18 pt tall.
    chips: Vec<egui::Rect>,
    /// Where each trend line's last point is drawn.
    last_points: Vec<egui::Pos2>,
}

/// Every shape in `shape`, those in a list one by one.
fn flat(shape: &egui::Shape, into: &mut Vec<egui::Shape>) {
    match shape {
        egui::Shape::Vec(v) => v.iter().for_each(|s| flat(s, into)),
        s => into.push(s.clone()),
    }
}

/// Draws the bench, its readings pinned, `w` wide, after `pointer` (if
/// any) has rested on a spot.
fn card_row(
    app: &mut TimegrapherApp,
    ctx: &egui::Context,
    w: f32,
    pointer: Option<egui::Pos2>,
) -> (CardRow, String) {
    let r = app.reading().expect("a reading");
    // The figures there are (a watch with no unlock gives no amplitude).
    let figures: Vec<String> = [
        readouts::rate_figure(&r),
        readouts::amplitude_figure(&r),
        readouts::beat_error_figure(&r),
    ]
    .into_iter()
    .flatten()
    .collect();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, 800.0));
    let mut out = None;
    for pass in 0..4 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events: pointer
                .filter(|_| pass > 0)
                .map(egui::Event::PointerMoved)
                .into_iter()
                .collect(),
            ..Default::default()
        };
        out = Some(ctx.run(input, |ctx| app.panels(ctx)));
    }
    let out = out.expect("drawn");
    let mut texts = Vec::new();
    for c in &out.shapes {
        if let egui::Shape::Text(t) = &c.shape {
            let size = t
                .galley
                .job
                .sections
                .first()
                .map_or(0.0, |s| s.format.font_id.size);
            texts.push((
                t.galley.text().to_string(),
                t.galley.rect.translate(t.pos.to_vec2()),
                size,
            ));
        }
    }
    let captions: Vec<(String, egui::Rect)> = ["Rate", "Amplitude", "Beat Error", "Amp.", "B.E."]
        .iter()
        .filter_map(|c| texts.iter().find(|t| t.0 == *c && t.1.top() < 120.0))
        .map(|t| (t.0.clone(), t.1))
        .collect();
    let left = captions.first().map_or(f32::MAX, |c| c.1.left()) - 1.0;
    let marks = texts
        .iter()
        .filter(|t| t.0 == "?" && t.1.left() > left && t.1.top() < 120.0)
        .count();
    let figures = figures
        .iter()
        .map(|f| {
            texts
                .iter()
                .find(|t| t.0.contains(f.trim_start_matches(['-', '+', '−'])))
                .unwrap_or_else(|| panic!("{w}: no {f} drawn in {texts:?}"))
                .2
        })
        .collect();
    let rate_unit_right = texts
        .iter()
        .find(|t| t.0 == "s/d" && t.1.top() < 120.0)
        .map(|t| t.1.right());
    let mut shapes = Vec::new();
    out.shapes.iter().for_each(|c| flat(&c.shape, &mut shapes));
    let chips = shapes
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Rect(r)
                if (r.rect.height() - 18.0).abs() < 0.5
                    && r.corner_radius == egui::CornerRadius::same(9)
                    && r.rect.top() < 160.0 =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .collect();
    let last_points = shapes
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Circle(c) if (c.radius - 2.5).abs() < 0.01 && c.center.y < 160.0 => {
                Some(c.center)
            }
            _ => None,
        })
        .collect();
    (
        CardRow {
            captions,
            marks,
            figures,
            rate_unit_right,
            chips,
            last_points,
        },
        painted_text(&out),
    )
}

/// At 800 the reading cards go without their "?": each caption is whole
/// and carries the help the "?" gave, on a hover (Hugh on #1954).
#[test]
fn at_800_the_reading_cards_captions_are_whole_and_carry_the_help() {
    let mut app = synthetic(60.0, 20.0);
    app.bench.panes_view.readings_pinned = true;
    let ctx = themed();
    ctx.style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    let (row, _) = card_row(&mut app, &ctx, 800.0, None);
    let names: Vec<&str> = row.captions.iter().map(|c| c.0.as_str()).collect();
    assert_eq!(names, ["Rate", "Amplitude", "Beat Error"]);
    assert_eq!(row.marks, 0, "no \"?\" on the reading cards");
    for (caption, help) in [
        ("Rate", help::RATE[0]),
        ("Amplitude", help::AMPLITUDE[0]),
        ("Beat Error", help::BEAT_ERROR[0]),
    ] {
        let at = row
            .captions
            .iter()
            .find(|c| c.0 == caption)
            .expect("drawn")
            .1
            .center();
        let (_, shown) = card_row(&mut app, &ctx, 800.0, Some(at));
        let start: String = help.chars().take(40).collect();
        assert!(shown.contains(&start), "{caption}'s hover: {shown}");
    }
    // With room, the "?"s are back.
    let (wide, _) = card_row(&mut app, &ctx, 1000.0, None);
    assert_eq!(wide.marks, 3);
}

/// At 800 a reading card's limits chip sits at the right end of its trend
/// row, under the figure, so its caption stays whole: inside the card and
/// clear of the trend's last point (Hugh on #2026; Walter found "B.E."
/// with a caveat on a stopped watch). Every reading card places it by the
/// one `readouts::trend_chip`.
#[test]
fn at_800_a_caveat_leaves_the_reading_cards_captions_whole() {
    // A watch whose unlock makes no sound: its beat error is timed from
    // the drop, a caveat on the Beat Error card alone.
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "syn".into());
    let mut cfg = SynthConfig {
        duration_s: 120.0,
        rate_s_per_day: 20.0,
        ..Default::default()
    };
    cfg.sounds[0].gain = 0.0;
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let live = app.bench.recording.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        if live.push(b) {
            let r = live.reading(app.bench.take.average_s);
            app.bench.results.trend.push(TrendPoint::of(r.time_s, &r));
        }
    }
    app.poll();
    app.refresh_trend();
    app.bench.panes_view.readings_pinned = true;
    let caveats = app.reading().expect("a reading").caveats;
    assert!(
        reading_limits("Rate", &caveats, 52.0, 30, false).is_empty()
            && !reading_limits("Beat Error", &caveats, 52.0, 30, false).is_empty(),
        "a Beat Error caveat alone: {caveats:?}"
    );
    let ctx = themed();
    let (row, text) = card_row(&mut app, &ctx, 800.0, None);
    let names: Vec<&str> = row.captions.iter().map(|c| c.0.as_str()).collect();
    assert_eq!(names, ["Rate", "Amplitude", "Beat Error"], "{text}");
    assert_eq!(
        row.chips.len(),
        1,
        "Beat Error's chip alone: {:?}",
        row.chips
    );
    let inner = readouts::six_widths(800.0 - 24.0 - 52.0, theme::GAP)[2] - 2.0 * 16.0;
    let rect = row.captions[2].1;
    let card = rect.left()..=rect.left() + inner + 0.5;
    let chip = row.chips[0];
    assert!(
        card.contains(&chip.left()) && card.contains(&chip.right()),
        "the chip {chip:?} inside Beat Error's card {card:?}"
    );
    assert!(chip.top() > rect.bottom(), "the chip in the title row");
    let last = row
        .last_points
        .iter()
        .find(|p| card.contains(&p.x))
        .expect("Beat Error's trend drawn");
    assert!(
        !chip.expand(2.5).contains(*last),
        "the chip {chip:?} on the trend's last point {last:?}"
    );
}

/// The three reading figures share one size at every width, the largest
/// at which each card's figure and unit fit whole; a long rate takes the
/// others down with it rather than its unit being cut (Hugh on #1976).
#[test]
fn the_reading_figures_share_one_size() {
    for rate in [20.0, -2174.4] {
        // Set to 28,800: on Auto, −2174.4 s/d is past every standard
        // rate's reach and reads no rate (#2223).
        let mut app = synthetic_set(60.0, rate, Some(28_800), |_| 280.0, |_| 0.0);
        app.bench.panes_view.readings_pinned = true;
        let ctx = themed();
        for w in [800.0, 1000.0, 1920.0] {
            let (row, _) = card_row(&mut app, &ctx, w, None);
            let f = &row.figures;
            assert!(
                f.len() == 3 && f[0] > 0.0 && f.iter().all(|s| *s == f[0]),
                "{rate} at {w}: {f:?}"
            );
            // The unit whole, inside the rate card: left of the next
            // card's caption less the gap between cards.
            let next = row.captions[1].1.left() - theme::GAP - 16.0;
            let unit = row.rate_unit_right.expect("the unit drawn whole");
            assert!(
                unit <= next + 8.0,
                "{rate} at {w}: s/d ends at {unit}, the card at {next}"
            );
        }
    }
}

/// With under 10 s of sound behind the reading, the cards show no figure,
/// and say "Listening" with what they wait for in its place: not empty,
/// and never a rate read from the stand's noise (#2014, Hugh). From 10 s
/// the figures are back.
#[test]
fn the_cards_say_listening_until_ten_seconds_are_heard() {
    use timegrapher_core::measure::{listening_text, HeardSince, LISTENING_TITLE};
    let ctx = themed();
    // A watch's beats opened with no snippets: their sound can't be
    // judged, so under 10 s of sound there is no verdict yet.
    let mut app = synthetic(30.0, 20.0);
    let live = app.bench.recording.live.as_ref().expect("live");
    let log = timegrapher_core::stream::BeatLog {
        lock_template_kept: false,
        sample_rate: 48_000,
        duration_s: live.duration_s(),
        bph: live.bph().expect("bph"),
        lift_deg: 52.0,
        escapement: Default::default(),
        beats: live.beats().to_vec(),
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    app.bench.recording.live = Some(timegrapher_core::live::LiveAnalyzer::from_log(
        log,
        timegrapher_core::live::LiveConfig::default(),
    ));
    let cards = |app: &TimegrapherApp| {
        let out = ctx.run(screen(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let _ = app.readouts(ui);
            });
        });
        painted_text(&out)
    };
    app.bench.panes_view.strip_end = Some(6.0);
    let r = app.card_reading().expect("a reading");
    assert_eq!(r.listening, Some(HeardSince::Start), "{r:?}");
    assert_eq!(r.rate_s_per_day, None);
    let early = cards(&app);
    assert!(early.contains(LISTENING_TITLE), "{early}");
    assert!(!early.contains("s/d"), "{early}");
    assert!(
        listening_text(HeardSince::Start).contains("since Listen started"),
        "{}",
        listening_text(HeardSince::Start)
    );
    app.bench.panes_view.strip_end = Some(25.0);
    let r = app.card_reading().expect("a reading");
    assert_eq!(r.listening, None);
    assert!(r.rate_s_per_day.is_some(), "{r:?}");
    assert!(!cards(&app).contains(LISTENING_TITLE));
}

/// The reading cards' painted text, as one string.
pub(super) fn cards_text(app: &TimegrapherApp, ctx: &egui::Context) -> String {
    let out = ctx.run(screen(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let _ = app.readouts(ui);
        });
    });
    painted_text(&out)
}

/// How many times `word` is painted in `text`, one shape a line.
pub(super) fn painted(text: &str, word: &str) -> usize {
    text.lines().filter(|l| *l == word).count()
}

/// While the watch settles after a move, the Rate, Amplitude and Beat
/// Error cards wait alike: each counts the settle down in the figure's
/// place, not a bare "—" on two of them. The signal stays in the status
/// bar all the while (#2007).
#[test]
fn every_reading_card_counts_the_settle_down_and_the_signal_stays() {
    use timegrapher_core::measure::Wait;
    let ctx = themed();
    let base = std::env::temp_dir().join(format!("tg-settle-wait-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "settle".into());
    let _tx = super::tests::queued_capture(&mut app, &base);
    let cfg = SynthConfig {
        duration_s: 52.0,
        rate_s_per_day: 5.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let (before, after) = audio.samples.split_at(30 * 48_000);
    let (during, rest) = after.split_at(5 * 48_000);
    let push = |app: &mut TimegrapherApp, x: &[f32]| {
        let live = app.bench.recording.live.as_mut().expect("live");
        x.chunks(960).for_each(|b| {
            live.push(b);
        });
    };
    push(&mut app, before);
    app.session.set_settle_s(20.0);
    app.bench
        .recording
        .live
        .as_mut()
        .expect("live")
        .new_stretch();
    app.session.start_settle(30.0);
    push(&mut app, during);
    let left = app.settling_left().expect("settling");
    assert!(app.reading().is_none(), "nothing is read while settling");
    let text = cards_text(&app, &ctx);
    // "Settling," over "15 s", on each of the three cards.
    let title = Wait::Settling { left_s: left }.title();
    let (head, secs) = title.split_once(' ').expect("two words");
    assert_eq!(painted(&text, head), 3, "{text}");
    assert_eq!(painted(&text, secs), 3, "{text}");
    let why = app.waiting(None).expect("a wait").1;
    assert!(
        why.starts_with(&Wait::Settling { left_s: left }.text()),
        "{why}"
    );
    // The signal is the sound's own, not withheld with the readings.
    assert!(app.signal().is_some(), "signal while settling");
    let out = ctx.run(screen(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| app.status_facts(ui));
        });
    });
    assert!(
        painted_text(&out)
            .lines()
            .any(|l| l.starts_with("signal ") && l.ends_with('×')),
        "{}",
        painted_text(&out)
    );
    // Settled: the figures, and no settle words.
    push(&mut app, rest);
    assert!(app.settling_left().is_none());
    let text = cards_text(&app, &ctx);
    assert_eq!(painted(&text, head), 0, "{text}");
    assert!(text.contains("s/d"), "{text}");
    drop(_tx);
    let _ = std::fs::remove_dir_all(base);
}

/// Listening to silence past the first 10 s, every reading card says
/// "Waiting for Beats", with why in its hover, never a bare "—" (#2007).
/// The status bar keeps "signal" in its place with a "—". Stopped, they
/// say "No Beats Heard" instead.
#[test]
fn silence_waits_for_beats_on_every_card() {
    use timegrapher_core::measure::WAITING_FOR_BEATS;
    let ctx = themed();
    let base = std::env::temp_dir().join(format!("tg-silence-wait-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "silence".into());
    let _tx = super::tests::queued_capture(&mut app, &base);
    let quiet = vec![0.0_f32; 960];
    for _ in 0..15 * 50 {
        app.bench
            .recording
            .live
            .as_mut()
            .expect("live")
            .push(&quiet);
    }
    let r = app.reading().expect("a reading");
    assert!(r.bph.is_none() && r.listening.is_none(), "{r:?}");
    let text = cards_text(&app, &ctx);
    // "Waiting" over "for Beats" on each of the three cards.
    let (head, tail) = WAITING_FOR_BEATS.split_once(' ').expect("words");
    assert_eq!(painted(&text, head), 3, "{text}");
    assert_eq!(painted(&text, tail), 3, "{text}");
    // Why, in the hover: no beat at all, not a figure short of beats.
    let why = app.waiting(Some(&r)).expect("a wait").1;
    assert_eq!(why, "The figures need a watch's beats: none are heard yet.");
    let out = ctx.run(screen(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| app.status_facts(ui));
        });
    });
    assert!(
        painted_text(&out).lines().any(|l| l.starts_with("signal ")),
        "{}",
        painted_text(&out)
    );
    // Stopped after the silence: a fact, not a wait (Hugh on #2036).
    app.stop();
    assert!(!app.running() && app.bench.recording.live.is_some());
    let r = app.card_reading().expect("a reading");
    assert!(r.bph.is_none(), "{r:?}");
    let text = cards_text(&app, &ctx);
    assert_eq!(painted(&text, "No Beats"), 3, "{text}");
    assert_eq!(painted(&text, "Heard"), 3, "{text}");
    assert!(!text.contains("Waiting") && !text.contains("Not"), "{text}");
    assert_eq!(
        app.waiting(Some(&r)).expect("a wait"),
        (
            "No Beats Heard".to_string(),
            "The figures need a watch's beats: none were heard in this stretch.".to_string()
        )
    );
    drop(_tx);
    let _ = std::fs::remove_dir_all(base);
}

/// A test set up and waiting for Start Test: every reading card says it
/// waits for the Start, not for beats no one is listening for (#2007).
#[test]
fn a_test_set_up_waits_for_its_start_on_every_card() {
    use timegrapher_core::measure::Wait;
    let ctx = themed();
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.bench.recording.live = None;
    app.bench.test.test_ready = Some(NewTake::Positions);
    assert!(app.ready_test().is_some());
    let text = cards_text(&app, &ctx);
    let title = Wait::TestStart.title();
    assert_eq!(title, "Waiting for Start");
    assert_eq!(painted(&text, "Waiting"), 3, "{text}");
    assert_eq!(painted(&text, "for Start"), 3, "{text}");
    assert!(!text.contains("Beats"), "{text}");
    assert_eq!(
        app.waiting(None).expect("a wait").1,
        "The figures need the test started: Start Test on the Listen card starts it."
    );
}

/// A card whose figure hasn't come yet on a stretch still short of beats
/// waits for more beats; on a long enough stretch, or where its own limit
/// says why it is empty, it doesn't claim more beats will bring it. The
/// cards' shared wait comes first (#2007).
#[test]
fn a_card_waits_for_more_beats_only_on_a_short_stretch() {
    use super::readouts::card_wait;
    use timegrapher_core::measure::Wait;
    let app = synthetic(30.0, 5.0);
    let mut r = app.reading().expect("a reading");
    let more = Some((Wait::MoreBeats.title(), Wait::MoreBeats.text()));
    r.caveats = vec![Caveat::ShortSpan { span_s: 6.0 }];
    assert_eq!(card_wait(None, Some(&r), false, false), more);
    assert_eq!(
        card_wait(None, Some(&r), true, false),
        None,
        "its figure is in"
    );
    assert_eq!(
        card_wait(None, Some(&r), false, true),
        None,
        "said on the card"
    );
    assert_eq!(card_wait(None, None, false, false), None);
    r.caveats.clear();
    assert_eq!(card_wait(None, Some(&r), false, false), None, "long enough");
    let shared = ("Listening".to_string(), "why".to_string());
    assert_eq!(
        card_wait(Some(&shared), Some(&r), true, false),
        Some(shared)
    );
}

/// After New Session while stopped nothing is listened to: every reading
/// card says "Not Listening", with why in its hover, never that it waits
/// for beats. Once Listen runs on silence, they say "Waiting for Beats"
/// (Hugh on #2036).
#[test]
fn after_new_session_while_stopped_the_cards_say_not_listening() {
    let ctx = themed();
    let mut app = synthetic(30.0, 5.0);
    assert!(!app.running());
    app.new_session();
    assert!(app.bench.recording.live.is_none() && !app.running());
    let text = cards_text(&app, &ctx);
    // "Not" over "Listening" on each of the three cards.
    assert_eq!(painted(&text, "Not"), 3, "{text}");
    assert_eq!(painted(&text, "Listening"), 3, "{text}");
    assert!(!text.contains("Beats"), "{text}");
    let why = app.waiting(app.card_reading().as_ref()).expect("a wait");
    assert_eq!(
        why,
        (
            "Not Listening".to_string(),
            "The figures need sound: nothing is being listened to.".to_string()
        )
    );
    let base = std::env::temp_dir().join(format!("tg-idle-wait-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    app.reset_session(48000, "silence".into());
    let _tx = super::tests::queued_capture(&mut app, &base);
    assert!(app.running());
    let quiet = vec![0.0_f32; 960];
    for _ in 0..15 * 50 {
        app.bench
            .recording
            .live
            .as_mut()
            .expect("live")
            .push(&quiet);
    }
    let text = cards_text(&app, &ctx);
    assert_eq!(painted(&text, "Waiting"), 3, "{text}");
    assert_eq!(painted(&text, "for Beats"), 3, "{text}");
    drop(_tx);
    let _ = std::fs::remove_dir_all(base);
}

/// "Waiting for More Beats" in a reading card's figure place at 800, 1000
/// and 1920 wide: drawn whole at the card's own size, or as "More Beats"
/// where the whole words don't fit, never cut or made smaller (Hugh on
/// #2036).
#[test]
fn waiting_for_more_beats_shows_whole_at_every_width() {
    use timegrapher_core::measure::{Wait, WAITING_FOR_MORE_BEATS};
    let mut app = synthetic(30.0, 5.0);
    let ctx = themed();
    for w in [800.0_f32, 1000.0, 1920.0] {
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(w, 800.0),
            )),
            ..Default::default()
        };
        // Where the Amplitude and Beat Error cards are, as drawn.
        let at = |name: &str, out: &egui::FullOutput| {
            out.shapes.iter().find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
                _ => None,
            })
        };
        let mut out = ctx.run(input(), |ctx| app.panels(ctx));
        for _ in 0..2 {
            out = ctx.run(input(), |ctx| app.panels(ctx));
        }
        let a = at("Amplitude", &out).expect("Amplitude");
        let b = at("Beat Error", &out).expect("Beat Error");
        let pad = f32::from(theme::CARD_PADDING);
        let card_w = b.x - a.x - theme::GAP;
        let h = app.readings_card_h(&ctx);
        let card = egui::Rect::from_min_size(egui::pos2(a.x - pad, 0.0), Vec2::new(card_w, h));
        let rows = CardRows::of(card);
        let (fig, _, _) = crate::sparkline::lay(rows.inner, rows.figure, rows.controls);
        let compact = card_w < READING_FULL_W;
        let wait = Wait::MoreBeats;
        let status = (wait.title(), wait.text());
        let out = ctx.run(input(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                super::readouts::figure_row(
                    ui,
                    fig,
                    None,
                    "°",
                    Some(&status),
                    "",
                    (compact, Some(1)),
                );
            });
        });
        let mut words = Vec::new();
        for c in &out.shapes {
            if let egui::Shape::Text(t) = &c.shape {
                if t.galley.text() == "—" {
                    continue;
                }
                let size = t.galley.job.sections[0].format.font_id.size;
                assert_eq!(size, if compact { 12.0 } else { 15.0 }, "{w}");
                words.push(t.galley.rows.iter().map(|r| r.text()).collect::<String>());
            }
        }
        let shown = words.join(" ");
        assert!(
            shown == WAITING_FOR_MORE_BEATS || shown == "More Beats",
            "{w}: {shown:?}"
        );
    }
    // Too narrow for the whole words at its size: the shorter words, at
    // that size, before any smaller font.
    let status = (Wait::MoreBeats.title(), Wait::MoreBeats.text());
    let out = ctx.run(screen(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let fig = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(55.0, 40.0));
            super::readouts::figure_row(ui, fig, None, "°", Some(&status), "", (true, Some(1)));
        });
    });
    let mut words = Vec::new();
    for c in &out.shapes {
        if let egui::Shape::Text(t) = &c.shape {
            assert_eq!(t.galley.job.sections[0].format.font_id.size, 12.0);
            words.push(t.galley.text().to_string());
        }
    }
    assert_eq!(words.join(" "), "More Beats");
}

/// A Positions Test's opening settle, in its first position, which no
/// move starts: every reading card counts it down as a move's settle,
/// not "Listening", and shows no figure in it (Walter on #2036).
#[test]
fn a_tests_opening_settle_counts_down_on_every_card() {
    let ctx = themed();
    let base = std::env::temp_dir().join(format!("tg-open-settle-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "opening".into());
    let _tx = super::tests::queued_capture(&mut app, &base);
    let cfg = SynthConfig {
        duration_s: 17.0,
        rate_s_per_day: 5.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let live = app.bench.recording.live.as_mut().expect("live");
    audio.samples.chunks(960).for_each(|b| {
        live.push(b);
    });
    // Started at 2 s with a 20 s settle, which ends at 22 s.
    let mut test = guided::Test::new().timed(20.0, 60.0);
    test.tick(2.0);
    app.bench.test.guided = Some(test);
    let now = app.latest_s();
    assert!(app.session.settling_left(now).is_none(), "no move");
    let left = app.test_settle_left(now).expect("settling");
    assert!((left - (22.0 - now)).abs() < 1e-9, "{left} at {now}");
    let title = timegrapher_core::measure::Wait::Settling { left_s: left }.title();
    let (head, secs) = title.split_once(' ').expect("two words");
    let text = cards_text(&app, &ctx);
    assert_eq!(painted(&text, head), 3, "{text}");
    assert_eq!(painted(&text, secs), 3, "{text}");
    assert!(
        !text.contains("Listening") && !text.contains("s/d"),
        "{text}"
    );
    // From 22 s the test measures: its settle is over.
    assert!(app.test_settle_left(22.0).is_none());
    // Not listening, a test's settle says nothing to the cards.
    app.stop();
    assert!(app.test_settle_left(now).is_none());
    drop(_tx);
    let _ = std::fs::remove_dir_all(base);
}

/// Paused inside a test's opening settle, nothing settles: the cards
/// stop counting and `settling_left()` agrees. Resume settles again
/// from the start, its whole length (Trevor on #2036).
#[test]
fn pausing_in_a_tests_opening_settle_stops_its_count() {
    let ctx = themed();
    let base = std::env::temp_dir().join(format!("tg-pause-settle-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "pause".into());
    let _tx = super::tests::queued_capture(&mut app, &base);
    let cfg = SynthConfig {
        duration_s: 17.0,
        rate_s_per_day: 5.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let live = app.bench.recording.live.as_mut().expect("live");
    audio.samples.chunks(960).for_each(|b| {
        live.push(b);
    });
    let mut test = guided::Test::new().timed(20.0, 60.0);
    test.tick(2.0);
    app.bench.test.guided = Some(test);
    let now = app.latest_s();
    assert!(app.settling_left().is_some(), "settling before the pause");
    app.bench
        .test
        .guided
        .as_mut()
        .expect("test")
        .pause(now - 1.0);
    assert!(app
        .bench
        .test
        .guided
        .as_ref()
        .expect("test")
        .settling_left(now)
        .is_none());
    assert!(app.settling_left().is_none(), "paused, nothing settles");
    let text = cards_text(&app, &ctx);
    assert!(!text.contains("Settling"), "{text}");
    app.bench.test.guided.as_mut().expect("test").resume(now);
    let left = app.settling_left().expect("settling again");
    assert!((left - 20.0).abs() < 1e-9, "{left}");
    drop(_tx);
    let _ = std::fs::remove_dir_all(base);
}

/// A guided test's pane counts its settle in the cards' seconds, from the
/// one `settling_left`, though the test moves on with the sound's length
/// and the cards read to the last beat (Walter on #2051).
#[test]
fn a_tests_pane_counts_the_settle_as_the_cards_do() {
    let base = std::env::temp_dir().join(format!("tg-pane-settle-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "pane".into());
    let _tx = super::tests::queued_capture(&mut app, &base);
    let cfg = SynthConfig {
        duration_s: 17.0,
        rate_s_per_day: 5.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let live = app.bench.recording.live.as_mut().expect("live");
    audio.samples.chunks(960).for_each(|b| {
        live.push(b);
    });
    let mut test = guided::Test::new().timed(20.0, 60.0);
    test.tick(2.0);
    app.bench.test.guided = Some(test);
    let sound = app
        .bench
        .recording
        .live
        .as_ref()
        .expect("live")
        .duration_s();
    assert!(sound > app.latest_s(), "{sound} after the last beat");
    let ticked = app.bench.test.guided.as_mut().expect("test").tick(sound);
    let cards = app.settling_left().expect("settling");
    assert_eq!(ticked, guided::Phase::Settling(22.0 - sound));
    assert_eq!(
        app.shown_phase(Some(ticked)),
        Some(guided::Phase::Settling(cards))
    );
    drop(_tx);
    let _ = std::fs::remove_dir_all(base);
}

/// An opened recording with no beat in it says "No Beats Heard" on every
/// card, not "No Regular Beat", which is about beats that are there
/// (Walter on #2036).
#[test]
fn an_opened_recording_of_silence_says_no_beats_heard() {
    let ctx = themed();
    let mut app = synthetic(30.0, 5.0);
    let log = timegrapher_core::stream::BeatLog {
        lock_template_kept: false,
        sample_rate: 48_000,
        duration_s: 60.0,
        bph: 21_600,
        lift_deg: 52.0,
        escapement: Default::default(),
        beats: Vec::new(),
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    app.bench.recording.live = Some(timegrapher_core::live::LiveAnalyzer::from_log(
        log,
        timegrapher_core::live::LiveConfig::default(),
    ));
    app.bench.panes_view.strip_end = Some(40.0);
    assert!(!app.running());
    let r = app.card_reading().expect("a reading");
    assert_eq!(r.beats_used, 0, "{r:?}");
    let text = cards_text(&app, &ctx);
    assert_eq!(painted(&text, "No Beats"), 3, "{text}");
    assert_eq!(painted(&text, "Heard"), 3, "{text}");
    assert!(!text.contains("Regular"), "{text}");
}

/// After New Session no figure is shown, so the sound card's clock line
/// a cleared take left behind isn't in the Rate card's hover; with a
/// figure it is (Walter on #2036, #1721).
#[test]
fn a_wait_carries_no_clock_line() {
    let mut app = synthetic(30.0, 5.0);
    app.new_session();
    app.bench.recording.live_usb = Some(super::live_usb::LiveUsb::new(48_000));
    assert!(app.live_usb_not_yet().is_some());
    assert_eq!(app.clock_not_yet(None), None);
    assert_eq!(app.clock_not_yet(Some(5.0)), app.live_usb_not_yet());
}

/// The strip's words in its place when no regular beat is drawn: the
/// whole pane with `whole`, else its no-watch card alone.
fn strip_text(app: &mut TimegrapherApp, ctx: &egui::Context, whole: bool) -> String {
    let out = ctx.run(screen(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            if whole {
                app.strip_pane(ui);
            } else {
                app.no_watch(ui);
            }
        });
    });
    painted_text(&out).replace('\n', " ")
}

/// Silence gives the strip the cards' own verdict in the cards' own
/// words, never "No regular beat found" beside them: "Waiting for Beats"
/// while listening, and "No Beats Heard" on an opened recording of
/// silence (Walter on #2036). The Rate hover says nothing of beats behind
/// a figure there is none of (#2044).
#[test]
fn silence_gives_the_strip_and_the_cards_one_verdict() {
    use timegrapher_core::measure::Wait;
    let ctx = themed();
    let base = std::env::temp_dir().join(format!("tg-silence-strip-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "silence".into());
    let _tx = super::tests::queued_capture(&mut app, &base);
    let quiet = vec![0.0_f32; 960];
    for _ in 0..15 * 50 {
        app.bench
            .recording
            .live
            .as_mut()
            .expect("live")
            .push(&quiet);
    }
    assert!(app.no_watch_yet());
    let r = app.reading().expect("a reading");
    assert_eq!(app.rate_beats_behind(Some(&r)), "");
    let said = |text: &str, wait: Wait| {
        assert!(text.contains(&wait.title()), "{text}");
        assert!(text.contains(&wait.text()), "{text}");
        assert!(!text.contains("regular"), "{text}");
    };
    said(&strip_text(&mut app, &ctx, false), Wait::NoBeats);
    // With the one faint dot (the peak as the microphone opens), the
    // strip's note leads with the same wait, listening and once stopped
    // (Walter on #2051), and says one dot (#2053).
    let note = |wait: Wait| {
        format!(
            "{}. The faint dot is a sound that might be a beat.",
            wait.title()
        )
    };
    let whole = strip_text(&mut app, &ctx, true);
    assert!(whole.contains(&note(Wait::NoBeats)), "{whole}");
    assert!(!whole.contains("regular"), "{whole}");
    app.stop();
    let whole = strip_text(&mut app, &ctx, true);
    assert!(whole.contains(&note(Wait::NoBeatsHeard)), "{whole}");
    assert!(!whole.contains("regular"), "{whole}");
    let _ = std::fs::remove_dir_all(base);
    // An opened recording of silence, as Walter opened it.
    let mut app = synthetic(30.0, 5.0);
    let log = timegrapher_core::stream::BeatLog {
        lock_template_kept: false,
        sample_rate: 48_000,
        duration_s: 60.0,
        bph: 21_600,
        lift_deg: 52.0,
        escapement: Default::default(),
        beats: Vec::new(),
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    app.bench.recording.live = Some(timegrapher_core::live::LiveAnalyzer::from_log(
        log,
        timegrapher_core::live::LiveConfig::default(),
    ));
    app.bench.results.trend.clear();
    assert!(app.no_watch_yet());
    said(&strip_text(&mut app, &ctx, true), Wait::NoBeatsHeard);
}

/// The Rate hover of an opened take's whole reading at its end, shorter
/// than Average Over: its beats are the whole take's after settling, each
/// side of a break fitted apart, not the latest run's (Hugh 5 and Norm on
/// #2075).
#[test]
fn a_whole_readings_rate_hover_says_its_beats_are_the_whole_takes() {
    let mut app = synthetic(30.0, 5.0);
    app.bench.take.average_s = 60.0;
    let mut r = app.reading().expect("a reading");
    r.whole_take = true;
    let said = app.rate_beats_behind(Some(&r));
    let first = said.split("\n\n").next().expect("a line");
    assert_eq!(
        first,
        format!(
            "{} beats in {} of 1 min: the beats the readings are fitted to, fewer seconds \
             than Average Over asks for. They are the whole take after settling, fitted \
             apart at each break, masked stretch or gap of more than 2 s with no beats, \
             with any piece under 10 s left out.",
            r.beats_used,
            crate::fields::duration(r.span_s.round())
        )
    );
}

/// A short stretch's Rate hover, whole, as Hugh ruled it on #2044: the
/// beats the readings are fitted to, and every cause that cuts the
/// stretch, the gap's seconds from core's `clean::GAP_S`.
#[test]
fn a_short_stretch_says_which_beats_the_readings_are_fitted_to() {
    let mut app = synthetic(30.0, 5.0);
    app.bench.take.average_s = 60.0;
    let r = app.reading().expect("a reading");
    assert!(r.span_s + 1.0 < app.bench.take.average_s, "{r:?}");
    let gap = crate::fields::duration(timegrapher_core::clean::GAP_S);
    assert_eq!(gap, "2 s");
    let said = app.rate_beats_behind(Some(&r));
    let first = said.split("\n\n").next().expect("a line");
    assert_eq!(
        first,
        format!(
            "{} beats in {} of 1 min: the beats the readings are fitted to, fewer seconds \
             than Average Over asks for. They are every beat since the sound began, or \
             since the latest break, masked stretch or gap of more than 2 s with no beats.",
            r.beats_used,
            crate::fields::duration(r.span_s.round())
        )
    );
}

/// A listen stopped before 10 s with nothing heard is no longer
/// listening: every card says "No Beats Heard", as the status bar says
/// Stopped, not "Listening" (Walter on #2053).
#[test]
fn a_listen_stopped_early_on_silence_says_no_beats_heard() {
    use timegrapher_core::measure::Wait;
    let ctx = themed();
    let base = std::env::temp_dir().join(format!("tg-stop-early-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "early".into());
    let _tx = super::tests::queued_capture(&mut app, &base);
    let quiet = vec![0.0_f32; 960];
    for _ in 0..6 * 50 {
        app.bench
            .recording
            .live
            .as_mut()
            .expect("live")
            .push(&quiet);
    }
    let text = cards_text(&app, &ctx);
    assert_eq!(painted(&text, "Listening"), 3, "listening first: {text}");
    app.stop();
    let text = cards_text(&app, &ctx);
    assert!(!text.contains("Listening"), "{text}");
    assert_eq!(painted(&text, "No Beats"), 3, "{text}");
    assert_eq!(painted(&text, "Heard"), 3, "{text}");
    let r = app.card_reading();
    assert_eq!(
        app.waiting(r.as_ref()),
        Some((Wait::NoBeatsHeard.title(), Wait::NoBeatsHeard.text()))
    );
    drop(_tx);
    let _ = std::fs::remove_dir_all(base);
}

/// A watch heard for under 10 s, its beats' sound not to be judged, and
/// the sound over (stopped, or an opened take at its newest moment): the
/// figures are still held, and no more will come, so every card says
/// "Under 10 s Heard", the fact, not "Listening" or "Waiting for More
/// Beats" (Trevor and Hugh on #2066).
#[test]
fn under_ten_seconds_heard_and_over_says_so_on_every_card() {
    use timegrapher_core::measure::{HeardSince, Wait};
    let ctx = themed();
    let app = synthetic(30.0, 20.0);
    let live = app.bench.recording.live.as_ref().expect("live");
    let log = timegrapher_core::stream::BeatLog {
        lock_template_kept: false,
        sample_rate: 48_000,
        duration_s: 6.0,
        bph: live.bph().expect("bph"),
        lift_deg: 52.0,
        escapement: Default::default(),
        beats: live
            .beats()
            .iter()
            .filter(|b| b.time <= 6.0)
            .cloned()
            .collect(),
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    let mut app = synthetic(30.0, 20.0);
    app.bench.recording.live = Some(timegrapher_core::live::LiveAnalyzer::from_log(
        log,
        timegrapher_core::live::LiveConfig::default(),
    ));
    app.bench.panes_view.strip_end = None;
    assert!(!app.running());
    let r = app.card_reading().expect("a reading");
    assert_eq!(r.listening, Some(HeardSince::Start), "{r:?}");
    let text = cards_text(&app, &ctx);
    let title = Wait::ShortHeard {
        since: HeardSince::Start,
    }
    .title();
    assert_eq!(title, "Under 10 s Heard");
    let (head, tail) = title.rsplit_once(' ').expect("words");
    assert_eq!(painted(&text, head), 3, "{text}");
    assert_eq!(painted(&text, tail), 3, "{text}");
    assert!(
        !text.contains("Listening") && !text.contains("More") && !text.contains("Waiting"),
        "{text}"
    );
    let says = |since| {
        let mut r = r.clone();
        r.listening = Some(since);
        app.listening(Some(&r)).expect("a word")
    };
    assert_eq!(
        says(HeardSince::Start),
        (
            "Under 10 s Heard".to_string(),
            "The figures need 10 s of sound: less than 10 s was heard since the start.".to_string()
        )
    );
    assert_eq!(
        says(HeardSince::Break),
        (
            "Under 10 s Heard".to_string(),
            "The figures need 10 s of sound: less than 10 s was heard since the break.".to_string()
        )
    );
    // Moved back into it, the take goes on past there: still Listening.
    app.bench.panes_view.strip_end = Some(5.0);
    assert!(cards_text(&app, &ctx).contains("Listening"));
}

/// A watch heard past its first seconds while the figures are read as a
/// file (#1987): before a read has figures of it, the cards say the beat
/// is being found, never a word for no beats nor live's own figures
/// (Hugh 5 and Norm on #2133).
#[test]
fn a_watch_heard_never_waits_for_beats_while_its_figures_are_read() {
    use timegrapher_core::measure::FINDING_TITLE;
    use timegrapher_core::synth::{generate, SynthConfig};
    let ctx = themed();
    let base = std::env::temp_dir().join(format!("tg-read-gap-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "watch".into());
    let _tx = super::tests::queued_capture(&mut app, &base);
    let cfg = SynthConfig {
        duration_s: 12.0,
        ..Default::default()
    };
    let sound = generate(&cfg, |_| 280.0, |_| 0.0).samples;
    // Heard all at once, as the app's feed takes a backlog: the first
    // read starts on all of it, so none has landed yet.
    let live = app.bench.recording.live.as_mut().expect("live");
    live.push_without_waiting(&sound);
    assert_eq!(live.read_as_file_to(), None, "no read has landed");
    let r = app.reading().expect("a reading");
    assert!(r.finding_beat && !r.watch_heard, "{r:?}");
    assert!(r.rate_s_per_day.is_none(), "not live's own figures: {r:?}");
    let text = cards_text(&app, &ctx);
    for word in ["Waiting", "No Beats"] {
        assert!(!text.contains(word), "{word}: {text}");
    }
    let (finding, _) = FINDING_TITLE.split_once(' ').expect("words");
    assert!(text.contains(finding), "{text}");
    // Once the read lands, its figures.
    app.bench
        .recording
        .live
        .as_mut()
        .expect("live")
        .read_as_file_now();
    let r = app.reading().expect("a reading");
    assert!(r.watch_heard && r.rate_s_per_day.is_some(), "{r:?}");
    let _ = std::fs::remove_dir_all(base);
}

/// The cards after a splice, each 0.25 s from it: Beat Error is a figure
/// or worded, never a bare "—" beside figures on the other cards (Walter
/// 17's hold of #2133). Where live has lost its sides the read's beat
/// error shows (Norm); a reading with figures but no sides told apart says
/// "Waiting for Tick and Tock" (Hugh 5). Two splices: the watch slipping
/// half a beat, and one at 28800 followed by one at 21600, as Walter
/// spliced the ETA 2824 into the Reverso. Returns, per splice, the frames
/// that painted the read's beat error with live's sides lost, and those
/// that painted the wait.
pub(super) fn beat_error_after(first: &[f32], splice: &[f32]) -> (usize, usize) {
    use timegrapher_core::measure::Wait;
    let ctx = themed();
    let base = std::env::temp_dir().join(format!(
        "tg-relock-be-{}-{}",
        splice.len(),
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "watch".into());
    let _tx = super::tests::queued_capture(&mut app, &base);
    let live = app.bench.recording.live.as_mut().expect("live");
    for c in first.chunks(960) {
        live.push(c);
    }
    let (mut read_sides, mut worded, mut bare) = (0, 0, Vec::new());
    for (k, c) in splice.chunks(12_000).enumerate() {
        // As at 1x, where each read lands well within the next 0.25 s.
        let live = app.bench.recording.live.as_mut().expect("live");
        live.push(c);
        live.read_as_file_now();
        let Some(r) = app.reading() else { continue };
        let text = cards_text(&app, &ctx);
        let at = first.len() as f64 / 48_000.0 + 0.25 * (k + 1) as f64;
        let live = app.bench.recording.live.as_ref().expect("live");
        if r.rate_s_per_day.is_some()
            && r.beat_error_ms.is_none()
            && r.beat_error_unlock_ms.is_none()
        {
            assert!(
                text.contains(&Wait::TickAndTock.title()),
                "{at} s: no beat error and no wait: {text}"
            );
            worded += 1;
        }
        if r.rate_s_per_day.is_some() && painted(&text, "—") > 0 {
            bare.push(format!("{at} s: {text}"));
        }
        let shown = r.beat_error_unlock_ms.or(r.beat_error_ms);
        if shown.is_some() && !live.sides_known_at(live.duration_s()) {
            read_sides += 1;
        }
    }
    assert!(bare.is_empty(), "a bare dash beside figures: {bare:?}");
    let _ = std::fs::remove_dir_all(base);
    (read_sides, worded)
}

/// A synthetic watch at `bph` for `seconds`.
fn relock_watch(bph: u32, seconds: f64) -> Vec<f32> {
    use timegrapher_core::synth::{generate, SynthConfig};
    let cfg = SynthConfig {
        duration_s: seconds,
        bph,
        ..Default::default()
    };
    generate(&cfg, |_| 280.0, |_| 0.0).samples
}

#[test]
fn beat_error_after_a_splice_is_a_figure_or_worded() {
    // Half a beat (62.5 ms at 28800) slipped: the same watch, later.
    let first = relock_watch(28_800, 25.0);
    let slipped: Vec<f32> = relock_watch(28_800, 8.0)[3_000..].to_vec();
    let slip = beat_error_after(&first, &slipped);
    let relock = beat_error_after(&first, &relock_watch(21_600, 8.0));
    eprintln!("slip {slip:?}, relock {relock:?} (read's sides with live's lost, worded)");
}

/// The Beat Error card with a reading that has figures but neither beat
/// error, as a read that hasn't told the Tick from the Tock gives: it says
/// "Waiting for Tick and Tock", never a bare dash; with either beat error
/// it shows the figure; the cards' shared wait comes first (Norm and Hugh 5
/// on #2133).
#[test]
fn a_beat_error_without_sides_waits_for_the_tick_and_the_tock() {
    use super::readouts::beat_error_card_wait;
    use timegrapher_core::measure::Wait;
    let app = synthetic(30.0, 5.0);
    let mut r = app.reading().expect("a reading");
    assert_eq!(beat_error_card_wait(None, Some(&r)), None, "its figure");
    r.beat_error_unlock_ms = None;
    assert_eq!(beat_error_card_wait(None, Some(&r)), None, "from the drop");
    r.beat_error_ms = None;
    let wait = Some((Wait::TickAndTock.title(), Wait::TickAndTock.text()));
    assert_eq!(beat_error_card_wait(None, Some(&r)), wait);
    let shared = ("Listening".to_string(), "why".to_string());
    assert_eq!(beat_error_card_wait(Some(&shared), Some(&r)), Some(shared));
}

/// The Beat Error card's BE angle of a reading off its set rate is at
/// the balance's own period, from the reading's rate, not the set rate's
/// (Trevor on #2252): 2.0 ms at 280° on 18,000 that reads −2059 s/d.
#[test]
fn the_be_angle_line_is_at_the_readings_own_period() {
    use timegrapher_core::be_angle::{card_words, Angle};
    let app = synthetic(60.0, 12.0);
    let mut r = app.reading().unwrap();
    r.bph = Some(18_000);
    r.rate_s_per_day = Some(-2059.1);
    r.amplitude_deg = Some(280.0);
    r.amplitude_error_deg = None;
    r.beat_error_ms = Some(2.0);
    r.beat_error_unlock_ms = None;
    r.beat_error_unlock_error_ms = None;
    let words = |period: f64| {
        let angle = Angle::at((280.0, None), (2.0, None), period).unwrap();
        card_words(&angle, 280.0, 52.0)
    };
    let own = words(timegrapher_core::timing::period_s(0.2, -2059.1));
    assert_ne!(own, words(0.2), "the set rate's angle is told apart");
    assert_eq!(
        readouts::be_angle_words(&r, 52.0).as_deref(),
        Some(own.as_str())
    );
}

/// Beats regular but no run long enough to read (#2257): the cards'
/// hover is core's reason alone, and its beats are a watch's; scattered
/// beats keep the hover about scatter as before (Hugh on #2311).
#[test]
fn a_run_too_short_is_not_called_scattered() {
    use timegrapher_core::measure::NoWatchReason;
    let mut r = LiveAnalyzer::new(48_000, LiveConfig::default()).reading(2.0);
    r.caveats = vec![Caveat::NoWatch {
        reason: NoWatchReason::TooShort,
    }];
    assert_eq!(
        no_watch_hover(Some(&r)),
        "No regular beat found (the beats were regular, but no run of them lasted the 10 s \
         a reading needs)."
    );
    assert!(beats_were_regular(&r));
    r.caveats = vec![Caveat::NoWatch {
        reason: NoWatchReason::Scatter,
    }];
    assert!(no_watch_hover(Some(&r)).ends_with(*help::NO_REGULAR_BEAT));
    assert!(!beats_were_regular(&r));
}

/// A synthetic watch at 28,800 heard `first` s, then `gap` s of noise
/// peaking at 0.75 times its RMS, then `back` s of the watch again,
/// listened to through the capture as the window hears it, stopped, and
/// its Stop read in.
fn stopped_short_return(base: &Path, spans: (f64, f64, f64)) -> TimegrapherApp {
    stopped_short_return_as(base, spans, |_| {})
}

/// [`stopped_short_return`], the app set up by `prep` first.
pub(super) fn stopped_short_return_as(
    base: &Path,
    (first, gap, back): (f64, f64, f64),
    prep: impl FnOnce(&mut TimegrapherApp),
) -> TimegrapherApp {
    let cfg = SynthConfig {
        bph: 28_800,
        duration_s: first + gap + back,
        ..Default::default()
    };
    let mut audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let span = (first * 48_000.0) as usize..((first + gap) * 48_000.0) as usize;
    let rms = (audio.samples[span.clone()]
        .iter()
        .map(|v| v * v)
        .sum::<f32>()
        / span.len() as f32)
        .sqrt();
    let mut x: u64 = 12_345;
    for s in &mut audio.samples[span] {
        x = x
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *s = 1.5 * rms * ((x >> 33) as f32 / (1u64 << 31) as f32 - 0.5);
    }
    stopped_listened(base, &audio, prep)
}

/// `audio` at 48 kHz listened to through the capture, the app set up by
/// `prep` first (its beat rate, say), as the window hears it, stopped,
/// and its Stop read in.
pub(super) fn stopped_listened(
    base: &Path,
    audio: &timegrapher_core::audio::Audio,
    prep: impl FnOnce(&mut TimegrapherApp),
) -> TimegrapherApp {
    use timegrapher_core::capture::{Block, Event};
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    prep(&mut app);
    app.reset_session(48000, "short return".into());
    app.session.listen();
    let info = app.info("short return", 48000, 16);
    app.session.recorder = Some(Recorder::start(&base.join("live"), info).unwrap());
    let tx = super::tests::queued_capture(&mut app, base);
    for (k, samples) in audio.samples.chunks(960).enumerate() {
        let us = 3e6 + (k * 960) as f64 / 48_000.0 * 1e6;
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_micros(us as u64);
        let samples = samples.to_vec();
        tx.send(Event::Audio(Block { samples, at })).unwrap();
    }
    while app.poll_for(std::time::Duration::from_secs(60)) > 0 {}
    app.stop();
    let t = std::time::Instant::now();
    while app.stop_read_waits().is_some() {
        app.poll_stop_read();
        assert!(
            t.elapsed().as_secs() < 120,
            "the Stop read is still under way"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    app.poll_stop_read();
    assert!(
        app.stopped_take().is_some(),
        "the take stopped is read whole"
    );
    app
}

/// A watch put back for 9 s after noise, stopped, after 7 s of it heard
/// first or none: at every Average Over its cards read the whole take's
/// verdict, too short, never Finding the Beat, whose wait can't end once
/// the take has; and the strip shows the run's beats, saying what a
/// reading needs, never "no regular beat found yet" (Walter 46 on #2311).
#[test]
fn a_stopped_short_return_reads_too_short_at_every_average() {
    use timegrapher_core::measure::{no_watch_reason_in, too_short_need_text, NoWatchReason};
    let ctx = themed();
    for (n, take) in [(7.0, 20.0, 9.0), (0.0, 12.0, 9.0)].into_iter().enumerate() {
        let base = std::env::temp_dir().join(format!("tg-short-return-{}-{n}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);
        let mut app = stopped_short_return(&base, take);
        for average in timegrapher_core::live::AVERAGES {
            app.bench.take.average_s = average;
            let _ = app.whole_take();
            let r = app.card_reading().expect("a reading");
            assert!(
                !r.finding_beat && !r.watch_heard,
                "{take:?} at {average} s: {r:?}"
            );
            assert_eq!(
                no_watch_reason_in(&r.caveats),
                Some(NoWatchReason::TooShort),
                "{take:?} at {average} s: {r:?}"
            );
            let strip = strip_text(&mut app, &ctx, true);
            assert!(
                strip.contains(&too_short_need_text()),
                "{take:?} at {average} s: {strip}"
            );
            assert!(!strip.contains("yet"), "{take:?} at {average} s: {strip}");
        }
        let _ = std::fs::remove_dir_all(base);
    }
}

/// 120 s of watch, 20 s of noise, then 12 s back, stopped: the whole take
/// hears the watch, so a card whose Average Over is shorter than the take
/// reads its own last seconds as the saved read gives them: never the
/// whole take's figures, never Finding the Beat (Norm's ruling on #2311).
#[test]
fn a_stopped_card_reads_its_own_span_after_a_long_run() {
    let base = std::env::temp_dir().join(format!("tg-own-span-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    let mut app = stopped_short_return(&base, (120.0, 20.0, 12.0));
    for average in timegrapher_core::live::AVERAGES {
        app.bench.take.average_s = average;
        let _ = app.whole_take();
        let r = app.card_reading().expect("a reading");
        assert!(!r.whole_take && !r.finding_beat, "{average} s: {r:?}");
        assert!(r.span_s <= average + 1e-6, "{average} s: {r:?}");
    }
    let _ = std::fs::remove_dir_all(base);
}

/// 30 s of watch, 20 s of noise, then 11 s back, stopped: at the watch's
/// last beat live is still finding the beat at every Average Over, and
/// the whole take hears the watch, so each card reads as the saved read
/// of its own last seconds gives it (the take's reader over that span,
/// as its Watches row reads), never Finding the Beat (Norm's ruling on
/// #2311, case 3).
#[test]
fn a_stopped_card_still_finding_reads_its_own_span() {
    use timegrapher_core::measure::no_watch_reason_in;
    let base = std::env::temp_dir().join(format!("tg-finding-span-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    let mut app = stopped_short_return(&base, (30.0, 20.0, 11.0));
    for average in timegrapher_core::live::AVERAGES {
        app.bench.take.average_s = average;
        assert!(app.whole_take().expect("a whole take").m.watch_heard);
        let s = app.stopped_take().expect("stopped");
        let at = timegrapher_core::beats::last_watch_beat_s(s.live.beats()).expect("a last beat");
        assert!(s.live.reading_at(at, average).finding_beat, "{average} s");
        let own = app.own_span_read(at - average, at).expect("its span read");
        assert!(own.start_s >= at - average && own.end_s == at, "{own:?}");
        let r = app.card_reading().expect("a reading");
        assert!(!r.finding_beat && !r.whole_take, "{average} s: {r:?}");
        assert_eq!(r.watch_heard, own.watch_heard, "{average} s: {r:?}");
        assert_eq!(r.rate_s_per_day, own.rate_s_per_day, "{average} s");
        assert_eq!(r.caveats, own.caveats, "{average} s");
        assert_eq!(
            no_watch_reason_in(&r.caveats),
            no_watch_reason_in(&own.caveats),
            "{average} s"
        );
    }
    let _ = std::fs::remove_dir_all(base);
}

/// A watch beating 17,570 bph listened to on Auto and stopped (#2327):
/// Auto's lock is off every standard rate, so at Stop the whole take and
/// the cards at every Average Over give no rate in s/d and name the
/// caveat `off_standard_rate`, as while it listened and as `analyze`
/// reads its sound (#2253). Set to 18,000, the same take reads its rate
/// against it.
#[test]
fn an_off_standard_watch_stopped_on_auto_has_no_rate_in_s_per_day() {
    use timegrapher_core::measure::off_standard;
    let base = std::env::temp_dir().join(format!("tg-off-standard-stop-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    let cfg = SynthConfig {
        bph: 17_570,
        duration_s: 60.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let _ = std::fs::create_dir_all(base.join("set"));
    let set = stopped_listened(&base.join("set"), &audio, |app| {
        app.bench.take.bph = Some(18_000);
    });
    let w = set.whole_take().expect("the whole take at 18,000");
    let rate =
        w.m.rate_s_per_day
            .unwrap_or_else(|| panic!("{:?}", w.m.caveats));
    assert!((rate + 2059.1).abs() < 1.0, "{rate}");
    assert!(off_standard(&w.m.caveats).is_none(), "{:?}", w.m.caveats);
    let r = set.card_reading().expect("a reading");
    assert!(r.rate_s_per_day.is_some(), "{r:?}");
    let mut app = stopped_listened(&base, &audio, |_| {});
    let w = app.whole_take().expect("the whole take");
    assert_eq!(w.m.rate_s_per_day, None, "{:?}", w.m.caveats);
    assert!(off_standard(&w.m.caveats).is_some(), "{:?}", w.m.caveats);
    for average in timegrapher_core::live::AVERAGES {
        app.bench.take.average_s = average;
        let _ = app.whole_take();
        let r = app.card_reading().expect("a reading");
        assert!(r.watch_heard, "over {average} s: {r:?}");
        assert_eq!(r.rate_s_per_day, None, "over {average} s: {r:?}");
        assert!(
            off_standard(&r.caveats).is_some(),
            "over {average} s: {:?}",
            r.caveats
        );
    }
    let _ = std::fs::remove_dir_all(&base);
}
