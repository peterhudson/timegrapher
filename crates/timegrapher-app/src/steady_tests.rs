//! The Steadiness views' tests: folds, joins, breaks and what each
//! view draws from a log.

/// A fold names a wheel only when the finding folded at is that
/// wheel's: a 46 s fold doesn't borrow the escape wheel of the beat
/// timing's 5.0 s line.
#[test]
fn a_fold_names_only_its_own_findings_wheel() {
    let escape = Some("escape wheel");
    assert_eq!(fold_wheel(&[(46.0, None), (5.0, escape)], 46.0), None);
    assert_eq!(fold_wheel(&[(5.0, escape)], 5.0), escape);
    assert_eq!(fold_wheel(&[(46.0, None), (46.2, escape)], 46.0), escape);
    assert_eq!(fold_wheel(&[], 46.0), None);
}

#[test]
fn a_spectrum_climbing_at_its_end_labels_its_own_peak_not_the_end() {
    // log10 period against log10 size: a hump at 60 s, then a climb
    // to the longest period that ends higher than the hump.
    let pts = [[1.0, 0.0], [1.78, 1.0], [2.2, 0.4], [3.0, 1.5], [3.3, 2.0]];
    assert_eq!(super::tallest_peak(&pts), Some([1.78, 1.0]));
    assert_eq!(
        super::tallest_peak(&[[1.0, 0.0], [2.0, 1.0], [3.0, 2.0]]),
        None
    );
}

#[test]
fn the_cycle_columns_give_their_quartiles_and_the_line_its_swing() {
    let nan = f64::NAN;
    let f = super::Fold {
        period_s: 60.0,
        profile: vec![1.0, -1.0, nan],
        counts: vec![5, 5, 1],
        raster: vec![
            vec![0.0, -3.0, 9.0],
            vec![1.0, -2.0, nan],
            vec![2.0, -1.0, nan],
            vec![3.0, 0.0, nan],
            vec![4.0, 1.0, nan],
        ],
        t0: 0.0,
    };
    let c = steady_views::fold_columns(&f);
    assert_eq!(c[0], Some([1.0, 2.0, 3.0]));
    assert_eq!(c[1], Some([-2.0, -1.0, 0.0]));
    assert_eq!(c[2], None);
    assert_eq!(steady_views::fold_spread(&f, &c), Some((2.0, 2.0)));
}

use super::*;

/// The Periods scores in dB read down from 0 dB at the tallest, every
/// 10 dB to the floor, −30 dB.
#[test]
fn the_periods_db_scale_reads_down_from_0_db() {
    let marks = |lo, hi| -> Vec<f64> {
        db_grid(GridInput {
            bounds: (lo, hi),
            base_step_size: 1.0,
        })
        .into_iter()
        .map(|m| m.value)
        .collect()
    };
    assert_eq!(marks(-30.0, 4.5), [0.0, -10.0, -20.0, -30.0]);
    assert_eq!(db_label(0.0), "0 dB");
    assert_eq!(db_label(-10.0), "−10 dB");
}

/// A period axis is marked at times a watchmaker reads, in their own
/// units, not at 100 s and "17 min".
#[test]
fn a_period_axis_is_marked_in_watchmakers_times() {
    let marks = period_grid(900.0)(GridInput {
        bounds: (4f64.log10(), 60_000f64.log10()),
        base_step_size: 0.001,
    });
    let labels: Vec<String> = marks.iter().map(|m| period_label(m.value)).collect();
    assert!(labels.contains(&"1 min".to_string()), "{labels:?}");
    assert!(labels.contains(&"1 h".to_string()), "{labels:?}");
    assert!(
        labels.iter().all(|l| !l.is_empty() && !l.contains("17")),
        "{labels:?}"
    );
    // Narrow, fewer marks, the main ones kept.
    let few = period_grid(200.0)(GridInput {
        bounds: (4f64.log10(), 60_000f64.log10()),
        base_step_size: 0.001,
    });
    assert!(few.len() < marks.len() && few.len() >= 3);
    assert_eq!(period_label(1000f64.log10()), "");
}
use timegrapher_core::beats::Beat;
use timegrapher_core::live::LiveConfig;
use timegrapher_core::synth::{generate, SynthConfig};

/// The plots carry only their titles and a key saying what a dot is;
/// the Info panel holds the view's description, each series' finding
/// whole and its statistics, which say what they counted, and what was
/// tested: beside the plots in a wide pane, over them in a narrow one.
#[test]
fn the_info_panel_holds_the_findings_and_the_plots_say_what_a_dot_is() {
    let cfg = SynthConfig {
        duration_s: 600.0,
        rate_s_per_day: 5.0,
        ..Default::default()
    };
    let audio = generate(
        &cfg,
        |_| 270.0,
        |t| 20.0 * (std::f64::consts::TAU * t / 60.0).sin(),
    );
    let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
    for b in audio.samples.chunks(4800) {
        live.push(b);
    }
    let s = compute(&log_of(&live).expect("beat rate"), None);
    let rate = s.report.series[0].headline.clone();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    fn texts(shape: &egui::Shape, into: &mut Vec<(String, f32)>) {
        match shape {
            egui::Shape::Text(t) => into.push((t.galley.text().to_string(), t.pos.x)),
            egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, into)),
            _ => {}
        }
    }
    let drawn = |w: f32, info: bool| {
        let mut all = Vec::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(w, 700.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let opts = Opts {
                        info,
                        ..Default::default()
                    };
                    draw(
                        ui,
                        Some(&s),
                        "Tested over 10:00 of beats",
                        View::Readings,
                        &[true, true, false],
                        opts,
                    );
                });
            });
            all.clear();
            out.shapes.iter().for_each(|c| texts(&c.shape, &mut all));
        }
        all
    };
    let has = |all: &[(String, f32)], t: &str| all.iter().any(|(s, _)| s.contains(t));
    let closed = drawn(900.0, false);
    assert!(has(&closed, "10 s readings ("), "{closed:?}");
    assert!(has(&closed, "2 s windows ("), "{closed:?}");
    for gone in [
        rate.as_str(),
        "Tested on",
        "Tested over",
        "Every reading in time order",
    ] {
        assert!(!has(&closed, gone), "{gone}");
    }
    let open = drawn(900.0, true);
    for t in [
        rate.as_str(),
        "Tested on ",
        "Tested over 10:00 of beats",
        "Every reading in time order",
        "Readings",
    ] {
        assert!(has(&open, t), "{t}: {open:?}");
    }
    // Beside the plots: the panel's words start in its third.
    let x = open
        .iter()
        .find(|(s, _)| s.contains("Tested on "))
        .unwrap()
        .1;
    assert!(x >= 900.0 - INFO_W - 16.0, "{x}");
    assert!(!has(&open, "…"), "{open:?}");
    // Narrow: over the plots, the same words.
    let narrow = drawn(500.0, true);
    assert!(has(&narrow, rate.as_str()) && has(&narrow, "Tested on "));
}

/// A series with too few readings says so once, in its row, not again
/// in the open Info panel.
#[test]
fn too_few_readings_is_said_once() {
    let cfg = SynthConfig {
        duration_s: 60.0,
        rate_s_per_day: 5.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 270.0, |_| 0.0);
    let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
    for b in audio.samples.chunks(4800) {
        live.push(b);
    }
    let s = compute(&log_of(&live).expect("beat rate"), None);
    let few: Vec<String> = s
        .report
        .series
        .iter()
        .filter(|c| c.autocorrelation.is_none())
        .map(|c| c.headline.clone())
        .collect();
    assert!(!few.is_empty(), "a minute leaves a series too few readings");
    let ctx = egui::Context::default();
    theme::install(&ctx);
    fn texts(shape: &egui::Shape, into: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => into.push(t.galley.text().to_string()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, into)),
            _ => {}
        }
    }
    let mut all = Vec::new();
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, 700.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let opts = Opts {
                    info: true,
                    ..Default::default()
                };
                draw(
                    ui,
                    Some(&s),
                    "Tested over 1:00 of beats",
                    View::Readings,
                    &[true, true, true],
                    opts,
                );
            });
        });
        all.clear();
        out.shapes.iter().for_each(|c| texts(&c.shape, &mut all));
    }
    for h in &few {
        let n = all.iter().filter(|t| t.contains(h.as_str())).count();
        assert_eq!(n, 1, "{h}: {all:?}");
    }
}

/// In a short pane the Info panel scrolls inside it rather than running
/// off its bottom, and Cycle's note sits above each plot, inside its
/// column, clear of the panel.
#[test]
fn a_short_panes_info_scrolls_and_the_cycle_note_stays_in_its_column() {
    let cfg = SynthConfig {
        duration_s: 600.0,
        rate_s_per_day: 5.0,
        ..Default::default()
    };
    let audio = generate(
        &cfg,
        |_| 270.0,
        |t| 20.0 * (std::f64::consts::TAU * t / 60.0).sin(),
    );
    let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
    for b in audio.samples.chunks(4800) {
        live.push(b);
    }
    let s = compute(&log_of(&live).expect("beat rate"), None);
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let (w, h) = (1440.0, 300.0);
    let mut out = None;
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h))),
            ..Default::default()
        };
        out = Some(ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let opts = Opts {
                    info: true,
                    ..Default::default()
                };
                draw(
                    ui,
                    Some(&s),
                    "Tested over 10:00 of beats",
                    View::Cycle,
                    &[true, true, true],
                    opts,
                );
            });
        }));
    }
    let out = out.unwrap();
    let mut texts = Vec::new();
    for c in &out.shapes {
        if let egui::Shape::Text(t) = &c.shape {
            let r = t.galley.rect.translate(t.pos.to_vec2());
            texts.push((t.galley.text().to_string(), r, c.clip_rect));
        }
    }
    // The pane's inner edge is 8 pt in from the window's.
    let panel_left = w - 8.0 - INFO_W;
    // The panel's words are clipped at the pane's foot: below it they
    // scroll.
    let panel: Vec<_> = texts.iter().filter(|t| t.1.left() >= panel_left).collect();
    assert!(
        panel.iter().any(|t| t.0.contains("Tested on ")),
        "{texts:?}"
    );
    assert!(panel.iter().all(|t| t.2.bottom() <= h), "{panel:?}");
    let notes: Vec<_> = texts.iter().filter(|t| t.0.contains("cycles")).collect();
    assert!(!notes.is_empty(), "{texts:?}");
    for n in notes {
        assert!(n.1.left() >= 0.0 && n.1.right() < panel_left, "{n:?}");
    }
    // No axis label runs past the line before the panel.
    let line = panel_left - 12.0 / 2.0;
    for t in texts.iter().filter(|t| t.1.left() < panel_left) {
        assert!(t.1.right() <= line, "{t:?}");
    }
    // The plots take the room left under their notes.
    let axis: Vec<_> = texts.iter().filter(|t| t.0.ends_with(" s/d")).collect();
    assert!(!axis.is_empty(), "{texts:?}");
    assert!(axis.iter().all(|t| t.1.bottom() <= h), "{axis:?}");
}

/// Each view's Info panel opens at its top, with its heading in view:
/// one scrolled down and closed, or another view's, does not open
/// scrolled.
#[test]
fn the_info_panel_opens_at_its_top() {
    let cfg = SynthConfig {
        duration_s: 600.0,
        rate_s_per_day: 5.0,
        ..Default::default()
    };
    let audio = generate(
        &cfg,
        |_| 270.0,
        |t| 20.0 * (std::f64::consts::TAU * t / 60.0).sin(),
    );
    let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
    for b in audio.samples.chunks(4800) {
        live.push(b);
    }
    let s = compute(&log_of(&live).expect("beat rate"), None);
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let (w, h) = (1440.0, 300.0);
    let panel_left = w - 8.0 - INFO_W;
    let at = egui::pos2(w - INFO_W / 2.0, h / 2.0);
    // One pass: the panel's heading, if in view, with `wheel` scrolling
    // the panel, or no pane at all.
    let pass = |view: Option<View>, wheel: bool| {
        let mut events = vec![egui::Event::PointerMoved(at)];
        if wheel {
            events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: Vec2::new(0.0, -400.0),
                modifiers: Default::default(),
            });
        }
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h))),
            events,
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(view) = view {
                    let opts = Opts {
                        info: true,
                        ..Default::default()
                    };
                    draw(ui, Some(&s), "", view, &[true, true, true], opts);
                }
            });
        });
        let label = view.map(View::label).unwrap_or_default();
        out.shapes.iter().any(|c| match &c.shape {
            egui::Shape::Text(t) => {
                let r = t.galley.rect.translate(t.pos.to_vec2());
                t.galley.text() == label && r.left() >= panel_left && r.top() >= c.clip_rect.top()
            }
            _ => false,
        })
    };
    assert!(pass(Some(View::Cycle), false), "opens at its top");
    for _ in 0..20 {
        pass(Some(View::Cycle), true);
    }
    assert!(
        !pass(Some(View::Cycle), false),
        "scrolled, the heading is out of view"
    );
    // Another view's panel opens at its top.
    assert!(pass(Some(View::Readings), false));
    // Closed and opened again: at its top.
    pass(None, false);
    assert!(pass(Some(View::Cycle), false));
}

#[test]
fn fft_renderer_paints_input_availability_in_both_rate_displays() {
    // Constructed beat/grid fixtures only: this tests presentation, not
    // acquisition, audio detection or a new spectral estimator.
    let mut state = compute(&context_log(), None);
    let c = steadiness::check_series(
        SeriesKind::Rate,
        Grid {
            t0: 0.0,
            step: 10.0,
            y: (0..40).map(|i| (i as f64).sin()).collect(),
        },
        None,
        None,
        None,
        &[],
        &Config::default(),
    );
    assert!(
        c.autocorrelation.is_some(),
        "exercise the actual FFT renderer"
    );
    state.report.series[0] = c;
    let ctx = egui::Context::default();
    theme::install(&ctx);
    for missing in [false, true] {
        let grid = Grid {
            t0: 0.0,
            step: 1.0,
            y: (0..256)
                .map(|i| {
                    if missing && i % 2 != 0 {
                        f64::NAN
                    } else {
                        (std::f64::consts::TAU * i as f64 / 64.0).sin()
                    }
                })
                .collect(),
        };
        state.report.spectra =
            vec![
                timegrapher_core::spectrum::amplitude_spectrum(&grid, "rate", "ms", 1.0, 600)
                    .unwrap(),
            ];
        for fft_timing in [false, true] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 700.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw(
                        ui,
                        Some(&state),
                        "FFT support fixture",
                        View::Fft,
                        &[true, false, false],
                        Opts {
                            fft_timing,
                            ..Default::default()
                        },
                    );
                });
            });
            let text = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                text.contains(if missing {
                    "128 of 256 grid samples available"
                } else {
                    "256 of 256 grid samples available"
                }),
                "{text}"
            );
            assert_eq!(
                text.contains("each size is a sine fitted to the samples present"),
                missing
            );
        }
    }
}

/// Wheel Periods marks the calibre's wheels in their own colour, named
/// beside their lines, or one minute and one hour when no wheel is
/// known; off, it marks neither.
#[test]
fn wheel_periods_marks_the_wheels_in_their_colour_or_the_reference_periods() {
    let mut state = compute(
        &context_log(),
        Some(vec![Wheel {
            name: "fourth wheel".into(),
            period_s: 60.0,
        }]),
    );
    state.report.series[0] = steadiness::check_series(
        SeriesKind::Rate,
        Grid {
            t0: 0.0,
            step: 10.0,
            y: (0..40).map(|i| (i as f64).sin()).collect(),
        },
        None,
        None,
        None,
        &[],
        &Config::default(),
    );
    let grid = Grid {
        t0: 0.0,
        step: 1.0,
        y: (0..256)
            .map(|i| (std::f64::consts::TAU * i as f64 / 64.0).sin())
            .collect(),
    };
    state.report.spectra =
        vec![
            timegrapher_core::spectrum::amplitude_spectrum(&grid, "rate", "ms", 1.0, 600).unwrap(),
        ];
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let wheel = theme::palette(ctx.style().visuals.dark_mode).wheel;
    let drawn = |state: &Steadiness, wheels: bool| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, 700.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                draw(
                    ui,
                    Some(state),
                    "",
                    View::Fft,
                    &[true, false, false],
                    Opts {
                        wheels,
                        ..Default::default()
                    },
                );
            });
        });
        let texts: Vec<(String, Color32)> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.fallback_color)),
                _ => None,
            })
            .collect();
        (texts, format!("{:?}", out.shapes))
    };
    let named =
        |texts: &[(String, Color32)], name: &str| texts.iter().filter(|(t, _)| t == name).count();
    // The wheel's grey is the faint text colour, which the plot uses
    // too: the wheel's line and name add to what is drawn in it.
    let in_wheel = |shapes: &str| shapes.matches(&format!("{wheel:?}")).count();
    let (on, shapes_on) = drawn(&state, true);
    assert_eq!(named(&on, "Fourth Wheel"), 1, "{on:?}");
    let (off, shapes_off) = drawn(&state, false);
    assert_eq!(named(&off, "Fourth Wheel"), 0, "{off:?}");
    assert!(
        in_wheel(&shapes_on) > in_wheel(&shapes_off),
        "the wheel's line and name in its colour"
    );
    // No wheel known: one minute marked, beside the axis's own "1 min".
    state.wheels.clear();
    let (on, _) = drawn(&state, true);
    let (off, _) = drawn(&state, false);
    assert_eq!(named(&on, "1 min"), named(&off, "1 min") + 1, "{on:?}");
}

/// The Readings view dashes a series' two levels only where every
/// two-level test holds: a fit the tests turned down draws nothing.
#[test]
fn the_two_levels_are_dashed_only_when_the_tests_hold() {
    use timegrapher_core::twostate::{find, Params, Sample};
    let samples: Vec<Sample> = (0..200)
        .map(|i| Sample {
            start_s: i as f64 * 10.0,
            end_s: (i + 1) as f64 * 10.0,
            value: if (i / 10) % 2 == 0 { 1.0 } else { 3.0 } + (i % 3) as f64 * 0.01,
            ..Default::default()
        })
        .collect();
    let mut ts = find(&samples, &Params::default());
    ts.two_levels = true;
    assert_eq!(state_levels(Some(&ts)), Some([ts.low, ts.high]));
    ts.two_levels = false;
    assert_eq!(state_levels(Some(&ts)), None);
    assert_eq!(state_levels(None), None);
}

/// A take whose own clock corrects its rate is drawn on it: Readings
/// Over Time's rate dots move by the correction, as the tests' median
/// does, so the plot and the cards give one rate.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "a test's clock fixture, not a take being opened"
)]
fn a_takes_clock_corrects_the_readings_drawn() {
    let mut log = context_log();
    log.duration_s = 900.0;
    log.jumps.clear();
    log.clipped_beats.clear();
    // 2 s/d fast on the card's clock.
    let beat = 0.125 * (1.0 - 2.0 / 86_400.0);
    log.beats = (0..7200)
        .map(|index| timegrapher_core::beats::Beat {
            time: index as f64 * beat,
            index,
            quality: 1.0,
        })
        .collect();
    let median = |s: &Steadiness| {
        let mut y: Vec<f64> = s.grids[0]
            .y
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .collect();
        y.sort_by(f64::total_cmp);
        y[y.len() / 2]
    };
    let heard = compute(&log, None);
    let fit = timegrapher_core::clock::ClockFit::fixed(20.0, log.duration_s);
    let clocked = compute_with(
        &log,
        &reading_breaks(Vec::new(), &log),
        None,
        Some(&fit),
        &std::sync::atomic::AtomicU8::new(0),
    );
    let shift = timegrapher_core::clock::rate_error_s_per_day(20.0);
    let moved = median(&clocked) - median(&heard);
    assert!((moved.abs() - shift.abs()).abs() < 0.05, "{moved} {shift}");
    let tested = clocked.report.series[0].median.expect("a median");
    assert!((tested - median(&clocked)).abs() < 0.05, "{tested}");
}

// Known beat records only: these controls do not run the detector.
fn context_log() -> BeatLog {
    BeatLog {
        sample_rate: 48000,
        duration_s: 120.0,
        bph: 28800,
        lift_deg: 52.0,
        beats: (0..960)
            .map(|index| timegrapher_core::beats::Beat {
                time: index as f64 * 0.125 + if index >= 480 { 0.03 } else { 0.0 },
                index,
                quality: if index % 13 == 0 { 0.2 } else { 1.0 },
            })
            .collect(),
        amplitude_windows: Vec::new(),
        clipped_beats: vec![1.0, 12.0, 41.0, 61.0, 80.0],
        clipped_samples: 17,
        jumps: vec![60.03],
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    }
}

#[test]
fn an_opened_take_keeps_the_sides_it_named_by_count() {
    // Sides named by the beat count alone at the jump: the log the
    // readings come from says so, as session's does, and its whole
    // take carries core's caveat.
    let mut log = context_log();
    log.by_count = vec![60.03];
    let live = LiveAnalyzer::from_log(log, LiveConfig::default());
    let derived = log_of(&live).expect("log");
    assert_eq!(derived.by_count, vec![60.03]);
    let m = timegrapher_core::session::measure(&derived, None, 0.0, 120.0);
    assert!(
        m.caveats.iter().any(|c| matches!(
            c,
            timegrapher_core::measure::Caveat::SidesByCount { count: 1 }
        )),
        "{:?}",
        m.caveats
    );
}

#[test]
fn derived_log_keeps_known_cuts_and_beat_quality() {
    let log = context_log();
    let original: Vec<_> = log
        .beats
        .iter()
        .map(|b| (b.time, b.index, b.quality))
        .collect();
    let mut live = LiveAnalyzer::from_log(log, LiveConfig::default());
    live.set_breaks(vec![40.0, 70.0]);
    let derived = log_of(&live).unwrap();
    assert_eq!(derived.jumps, [40.0, 60.03, 70.0]);
    assert_eq!(
        derived
            .beats
            .iter()
            .map(|b| (b.time, b.index, b.quality))
            .collect::<Vec<_>>(),
        original
    );
    assert_eq!(derived.clipped_beats, live.clipped_beats());
    assert_eq!(derived.duration_s, live.duration_s());
    assert_eq!(derived.clipped_samples, 0, "not counted live, unchanged");
}

/// Both ends of a masked stretch are cuts, so no 10-second reading
/// downstream reaches across one or takes in a ragged edge beside one.
#[test]
fn a_masked_stretch_is_cut_at_both_ends() {
    let mut live = LiveAnalyzer::from_log(context_log(), LiveConfig::default());
    live.set_masked(&[(20.0, 33.5)]);
    let derived = log_of(&live).unwrap();
    assert_eq!(derived.jumps, [20.0, 33.5, 60.03]);
    assert!(derived
        .beats
        .iter()
        .all(|b| !(20.0..33.5).contains(&b.time)));
}

/// A take's break inside a joined span keeps the time its gap held,
/// on the joined times; the joins themselves hold none.
#[test]
fn a_join_keeps_the_gaps_of_the_breaks_inside_its_spans() {
    use timegrapher_core::breaks::Break;
    let mut log = context_log();
    log.jumps = vec![60.03];
    let cut = |audio_s, gap_s| Break { audio_s, gap_s };
    // One stretch after a 10 s settle, and a second after a move.
    let (joined, cuts) = join(
        &log,
        &[(10.0, 70.0), (80.0, 110.0)],
        &[cut(60.03, 30.0)],
        None,
    );
    assert_eq!(cuts.len(), 2, "{cuts:?}");
    assert!(
        (cuts[0].audio_s - 50.03).abs() < 1e-9 && cuts[0].gap_s == 30.0,
        "{cuts:?}"
    );
    assert_eq!(cuts[1].gap_s, 0.0, "{cuts:?}");
    assert!(joined.jumps.contains(&cuts[1].audio_s), "{cuts:?}");
}

/// A break exactly where a span starts adds nothing: the join there
/// already cuts, with no time, and the first span starts unbroken.
#[test]
fn a_break_at_a_spans_start_adds_no_cut() {
    use timegrapher_core::breaks::Break;
    let log = context_log();
    let cut = |audio_s, gap_s| Break { audio_s, gap_s };
    let (_, cuts) = join(
        &log,
        &[(10.0, 70.0), (80.0, 110.0)],
        &[cut(10.0, 30.0), cut(80.0, 30.0)],
        None,
    );
    assert!(
        cuts.iter().all(|c| c.gap_s == 0.0 && c.audio_s > 0.0),
        "{cuts:?}"
    );
}

#[test]
fn derived_join_transports_cuts_and_existing_sample_context() {
    let mut log = context_log();
    log.jumps = vec![5.0, 10.0, 15.0, 20.0, 30.0, 40.0, 60.03, 80.0, 110.0];
    let spans = [(10.0, 20.0), (30.01, 30.02), (40.0, 80.0)];
    let joined = join(&log, &spans, &[], None).0;
    // The empty middle span adds neither a join nor a cut. The second
    // retained span starts at15 (ten seconds plus the existing5s gap).
    assert_eq!(joined.jumps, [0.0, 5.0, 15.0, 35.03]);
    assert_eq!(joined.duration_s, 55.0);
    assert_eq!(joined.clipped_beats, [2.0, 16.0, 36.0]);
    assert_eq!(joined.clipped_samples, 17, "existing whole-log counter");
    let kept: Vec<_> = log
        .beats
        .iter()
        .filter(|b| (10.0..20.0).contains(&b.time) || (40.0..80.0).contains(&b.time))
        .collect();
    assert_eq!(joined.beats.len(), kept.len());
    for (out, input) in joined.beats.iter().zip(kept) {
        assert_eq!(out.quality, input.quality);
        assert_eq!(
            out.time,
            input.time - if input.time < 20.0 { 10.0 } else { 25.0 }
        );
    }
    assert!(joined.beats.windows(2).all(|b| b[0].index < b[1].index));
}

#[test]
fn derived_join_keeps_amplitude_sides_on_the_renumbered_beats() {
    use timegrapher_core::amplitude::AmplitudeWindow;
    for (start, second_start, expected_swaps) in
        [(0.0, 40.0, [false, false]), (0.125, 40.125, [true, true])]
    {
        let end = start + 10.0;
        let mut log = context_log();
        let window = |a, b| AmplitudeWindow {
            start_s: a,
            end_s: b,
            even_deg: Some(260.0),
            odd_deg: Some(280.0),
            beat_error_ms: Some(0.4),
            beat_error_unlock_ms: Some(-0.2),
            even_unlock_ms: Some(-3.0),
            odd_unlock_ms: Some(-4.0),
            even_drop_ms: Some(0.1),
            odd_drop_ms: Some(0.5),
        };
        log.amplitude_windows = vec![
            window(start - 1.0, start + 1.0),
            window(start + 2.0, start + 4.0),
            window(end - 1.0, end + 1.0),
            window(42.0, 44.0),
        ];
        let joined = join(
            &log,
            &[(start, end), (second_start, second_start + 10.0)],
            &[],
            None,
        )
        .0;
        assert_eq!(
            joined.amplitude_windows.len(),
            2,
            "only fully contained windows"
        );
        for (((a, b), w), expected_swap) in [(start, end), (second_start, second_start + 10.0)]
            .into_iter()
            .zip(&joined.amplitude_windows)
            .zip(expected_swaps)
        {
            let input_first = log.beats.iter().find(|x| x.time >= a).unwrap();
            let offset = if a == start {
                start
            } else {
                second_start - 15.0
            };
            let output_first = joined
                .beats
                .iter()
                .find(|x| x.time == input_first.time - offset)
                .unwrap();
            let swapped = (input_first.index - output_first.index) % 2 != 0;
            assert_eq!(
                swapped, expected_swap,
                "explicit first/later parity coverage"
            );
            assert_eq!(w.even_deg, Some(if expected_swap { 280.0 } else { 260.0 }));
            assert_eq!(
                w.beat_error_ms,
                Some(if expected_swap { -0.4 } else { 0.4 })
            );
            let mut expected = log
                .amplitude_windows
                .iter()
                .find(|w| w.start_s >= a && w.end_s <= b)
                .unwrap()
                .clone();
            expected.start_s -= offset;
            expected.end_s -= offset;
            if swapped {
                expected.swap_sides();
            }
            assert_eq!(
                serde_json::to_value(w).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert_eq!(w.mean(), Some(270.0));
        }
    }
}

/// A rate reading over a dropout with no listed break is left out of
/// the views, as the tests leave it out.
#[test]
fn views_leave_out_a_rate_reading_over_a_dropout() {
    let period = 0.125 / (1.0 + 10.0 / 86400.0);
    let beats: Vec<Beat> = (0..1600)
        .map(|index| Beat {
            index,
            time: index as f64 * period,
            quality: 1.0,
        })
        // Nothing heard over the 2 s stretch from 34 s, inside the 10 s
        // reading from 30 s.
        .filter(|b| !(33.9..36.1).contains(&b.time))
        .collect();
    let log = BeatLog {
        sample_rate: 48000,
        duration_s: 200.0,
        bph: 28800,
        lift_deg: 52.0,
        beats,
        amplitude_windows: vec![],
        clipped_beats: vec![],
        clipped_samples: 0,
        jumps: vec![],
        sided: vec![],
        by_count: vec![],
        snippets: Vec::new(),
    };
    let at = |g: &Grid, t: f64| g.y[((t - g.t0) / g.step).floor() as usize];
    let raw = steady_views::grids_with_breaks(&log, 10.0, &[]);
    assert!(at(&raw[0], 34.5).is_nan(), "the core grids leave it out");
    let s = compute(&log, None);
    assert!(at(&s.grids[0], 34.5).is_nan());
    assert!(at(&s.grids[0], 54.5).is_finite());
}

#[test]
fn derived_compute_uses_existing_piecewise_analysis() {
    let mut log = context_log();
    // Unequal rates ensure whole-history weighting cannot be replaced by
    // a latest-piece fit. Keep the existing phase step and beat qualities.
    for b in &mut log.beats {
        let rate = if b.index < 480 { 10.0 } else { -20.0 };
        let local = if b.index < 480 {
            b.index
        } else {
            b.index - 480
        };
        b.time =
            if b.index < 480 { 0.0 } else { 60.03 } + local as f64 * 0.125 / (1.0 + rate / 86400.0);
    }
    let lc = LongConfig {
        wheels: default_wheels(log.bph),
        ..Default::default()
    };
    let breaks = [timegrapher_core::breaks::Break {
        audio_s: 60.03,
        gap_s: 0.0,
    }];
    let long = longrun::analyse_with_breaks(&log, None, &breaks, &lc);
    let expected = steadiness::check(&log, None, &long, &lc, &Config::default());
    let actual = compute(&log, None);
    assert_eq!(
        serde_json::to_value(&actual.report).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    let cuts = timegrapher_core::clean::Cuts::new(None, &breaks, &[]).piece_cuts();
    let whole = timegrapher_core::timing::fit_pieces(&log.beats, log.bph, &cuts).unwrap();
    assert!((whole.rate_s_per_day + 5.0).abs() < 0.1);
    assert_eq!(
        long.overall.as_ref().unwrap().rate_s_per_day,
        whole.rate_s_per_day
    );
    assert!(
        (whole.rate_s_per_day + 20.0).abs() > 10.0,
        "not latest piece"
    );
    assert_eq!(
        actual.grids[0].y.len(),
        steady_views::grids_with_breaks(&log, 10.0, &breaks)[0]
            .y
            .len()
    );
}

#[test]
fn the_beat_error_view_gives_sizes_where_its_sign_turns() {
    use timegrapher_core::amplitude::AmplitudeWindow;
    // `after` ms of beat error after the jump at 60.03 s, 0.6 ms before.
    let log_with = |after: f64| {
        let mut log = context_log();
        for b in &mut log.beats {
            let be = if b.index < 480 { 0.6 } else { after };
            let half = be / 2000.0;
            b.time += if b.index % 2 == 1 { half } else { -half };
        }
        log.amplitude_windows = (0..60)
            .map(|i| {
                let a = i as f64 * 2.0;
                AmplitudeWindow {
                    start_s: a,
                    end_s: a + 2.0,
                    even_deg: Some(270.0),
                    odd_deg: Some(270.0),
                    beat_error_ms: Some(if a < 60.0 { 0.6 } else { after }),
                    beat_error_unlock_ms: Some(if a < 60.0 { 0.6 } else { after }),
                    even_unlock_ms: None,
                    odd_unlock_ms: None,
                    even_drop_ms: None,
                    odd_drop_ms: None,
                }
            })
            .collect();
        log
    };
    let finite = |s: &Steadiness| -> Vec<f64> {
        s.grids[2]
            .y
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .collect()
    };
    let turned = compute(&log_with(-0.4), None);
    let be = finite(&turned);
    assert!(be.iter().all(|&v| v > 0.0), "{be:?}");
    assert!(be.iter().any(|&v| (v - 0.4).abs() < 1e-9), "{be:?}");
    // Where the beats' sign agrees either side, the readings keep theirs.
    let mut agree = log_with(0.4);
    for w in &mut agree.amplitude_windows {
        w.beat_error_unlock_ms = w.beat_error_unlock_ms.map(|v| -v);
    }
    let both_slow = compute(&agree, None);
    assert!(finite(&both_slow).iter().any(|&v| v < 0.0));
}

/// A watch whose rate swings once a minute (a fourth wheel fault) is
/// found periodic, and every view draws.
#[test]
fn a_once_a_minute_swing_reads_periodic_and_every_view_draws() {
    let cfg = SynthConfig {
        duration_s: 600.0,
        rate_s_per_day: 5.0,
        ..Default::default()
    };
    let audio = generate(
        &cfg,
        |_| 270.0,
        |t| 20.0 * (std::f64::consts::TAU * t / 60.0).sin(),
    );
    let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
    for b in audio.samples.chunks(4800) {
        live.push(b);
    }
    let log = log_of(&live).expect("beat rate");
    let s = compute(&log, None);
    let rate = &s.report.series[0];
    assert!(rate.cycle.is_some(), "{}", rate.headline);
    assert!(s.folds[0].is_some());
    assert_eq!(s.fold_from[0], FoldFrom::Own);
    // The steady amplitude still has a Cycle chart, folded at the
    // rate's cycle (or a wheel's turn if the rate's didn't fit).
    assert!(s.folds[1].is_some());
    assert_ne!(s.fold_from[1], FoldFrom::Own);

    // A picked calibre's train names the cycle instead.
    let named = compute(
        &log,
        Some(vec![Wheel {
            name: "seconds wheel".into(),
            period_s: 60.0,
        }]),
    );
    let wheel = named.report.series[0]
        .cycle
        .as_ref()
        .and_then(|c| c.wheel.clone())
        .or_else(|| {
            named.report.series[0]
                .period
                .as_ref()
                .and_then(|p| p.wheel.clone())
        });
    assert_eq!(wheel.as_deref(), Some("seconds wheel"));

    let ctx = egui::Context::default();
    theme::install(&ctx);
    for v in View::ALL {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, 700.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut shown = [true, false, true];
                series_switches(ui, &mut shown);
                draw(ui, Some(&s), "test", v, &shown, Opts::default());
            });
        });
        assert!(!out.shapes.is_empty());
    }

    // The map panes draw a map the core gives, with its key, and
    // say what they need when it gives none.
    let drawn = |s: &Steadiness, v: View| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, 700.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                draw(
                    ui,
                    Some(s),
                    "maps",
                    v,
                    &[true, false, false],
                    Opts::default(),
                );
            });
        });
        fn texts(shape: &egui::Shape, into: &mut String) {
            match shape {
                egui::Shape::Text(t) => into.push_str(&format!("{}\n", t.galley.text())),
                egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, into)),
                _ => {}
            }
        }
        let mut all = String::new();
        out.shapes.iter().for_each(|c| texts(&c.shape, &mut all));
        all
    };
    let mut s = s;
    s.maps.clear();
    assert!(drawn(&s, View::Stft).contains("The STFT Map needs"));
    let nan = f64::NAN;
    for method in ["stft", "sst"] {
        s.maps.push(timegrapher_core::timefreq::TimeFrequency {
            series: "rate",
            method,
            unit: "ms",
            step_s: 10.0,
            window_s: None,
            hop_s: Some(300.0),
            period_s: vec![600.0, 120.0, 30.0],
            t_s: vec![600.0, 900.0],
            values: vec![vec![0.1, 0.4, nan], vec![0.2, 0.3, 0.1]],
        });
    }
    for v in [View::Stft, View::Wavelet] {
        let t = drawn(&s, v);
        assert!(t.contains("not read") && !t.contains("needs"), "{v:?}: {t}");
    }
}

/// The lag a once-a-minute swing repeats at is noted beside its dot
/// on a plot of room, and is the dot's hover only on a short one, as
/// every point note is.
#[test]
fn a_repeats_note_is_hover_only_on_a_short_plot() {
    // Twenty minutes of beats whose timing swings once a minute.
    let beats = (0..9600)
        .map(|index| {
            let t = index as f64 * 0.125;
            timegrapher_core::beats::Beat {
                time: t + 0.002 * (std::f64::consts::TAU * t / 60.0).sin(),
                index,
                quality: 1.0,
            }
        })
        .collect();
    let log = BeatLog {
        sample_rate: 48000,
        duration_s: 1200.0,
        bph: 28800,
        lift_deg: 52.0,
        beats,
        amplitude_windows: vec![],
        clipped_beats: vec![],
        clipped_samples: 0,
        jumps: vec![],
        sided: vec![],
        by_count: vec![],
        snippets: Vec::new(),
    };
    let s = compute(&log, None);
    let a = s.report.series[0].autocorrelation.as_ref().expect("acf");
    assert!(a.repeat_lag_s.is_some(), "no repeat found");
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let drawn = |h: f32| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, h),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let opts = Opts::default();
                draw(
                    ui,
                    Some(&s),
                    "repeats",
                    View::Autocorrelation,
                    &[true, false, false],
                    opts,
                );
            });
        });
        fn texts(shape: &egui::Shape, into: &mut String) {
            match shape {
                egui::Shape::Text(t) => into.push_str(&format!("{}\n", t.galley.text())),
                egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, into)),
                _ => {}
            }
        }
        let mut all = String::new();
        out.shapes.iter().for_each(|c| texts(&c.shape, &mut all));
        all
    };
    assert!(drawn(500.0).contains("repeats at"));
    assert!(!drawn(150.0).contains("repeats at"));

    // Short is the plot's own height inside its x axis: given 110 pt,
    // the plot is under 100 pt and its note is hover only; given 130,
    // it isn't.
    let short_in = |h: f32| {
        let mut short = None;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, 900.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.allocate_ui(Vec2::new(400.0, h), |ui| {
                    ui.set_max_height(h);
                    short = Some(note_short(ui, &None, ("short", h as u32)));
                });
            });
        });
        short.unwrap()
    };
    assert!(short_in(110.0));
    assert!(!short_in(130.0));
}

/// Two wild readings among a hundred steady ones don't set the scale.
#[test]
fn a_few_wild_readings_leave_the_scale_alone() {
    let mut pts: Vec<[f64; 2]> = (0..116)
        .map(|i| [i as f64, 10.0 + (i % 7) as f64 * 0.5])
        .collect();
    pts[60][1] = 1200.0;
    pts[62][1] = 1140.0;
    let (lo, hi) = spread(&pts).unwrap();
    assert!(lo > 8.0 && hi < 16.0, "{lo} to {hi}");
    // Two states, a third of the readings each way, keep both.
    let two: Vec<[f64; 2]> = (0..90)
        .map(|i| [i as f64, if i % 3 == 0 { 30.0 } else { 10.0 }])
        .collect();
    let (lo, hi) = spread(&two).unwrap();
    assert!(lo < 10.0 && hi > 30.0, "{lo} to {hi}");
}
