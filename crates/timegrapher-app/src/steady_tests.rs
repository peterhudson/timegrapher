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

/// A size core withheld for thin cover (NaN, #1638) leaves the FFT's line
/// open there: the runs either side are drawn apart, never bridged, and
/// no peak is read across the gap (Norm: masks cull, never fill).
#[test]
fn a_withheld_size_leaves_a_gap_in_the_spectrum() {
    let periods = [1000.0, 320.0, 100.0, 10.0, 5.0];
    let sizes = [1.0, f64::NAN, 2.0, 0.5, 1.0];
    let runs = super::log_runs(&periods, &sizes);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0], vec![[3.0, 0.0]]);
    assert_eq!(runs[1].len(), 3);
    assert!(runs.concat().iter().all(|q| q[0] != 320f64.log10()));
    // Unread sizes (0, infinite) are left out without opening a gap.
    assert_eq!(
        super::log_runs(&[10.0, 5.0, 4.0], &[1.0, 0.0, 2.0]).len(),
        1
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
use crate::theme::INFO_W;

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

/// Ten minutes of a watch whose rate swings once a minute (a fourth
/// wheel fault), heard as the app hears it, and the checks drawn from
/// it: made once and shared, as each check of its own made and heard it
/// again, about 20 s apiece on Windows (#907).
#[test]
fn a_once_a_minute_swing_in_every_pane() {
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
    the_info_panel_holds_the_findings_and_the_plots_say_what_a_dot_is(&s);
    a_short_panes_info_scrolls_and_the_cycle_note_stays_in_its_column(&s);
    the_info_panel_opens_at_its_top(&s);
    the_readout_stays_clear_of_info_over_the_rows(&s);
    a_once_a_minute_swing_reads_periodic_and_every_view_draws(&log, s);
}

/// The plots carry only their titles and a key saying what a dot is;
/// the Info panel holds the view's description, each series' finding
/// whole and its statistics, which say what they counted, and what was
/// tested: beside the plots in a wide pane, over them in a narrow one.
fn the_info_panel_holds_the_findings_and_the_plots_say_what_a_dot_is(s: &Steadiness) {
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
                        take_span: None,
                        joined_in: None,
                        ..Default::default()
                    };
                    draw(
                        ui,
                        Some(s),
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

/// In a pane too narrow for Info beside the rows, with Info open and the
/// pointer on a Readings Over Time row, no readout text ends past Info's
/// left edge, and the value is still shown; with Info shut the same
/// readout reaches past that edge, so the check bites (Trevor on #1582).
/// The pane is 432 wide, so Info starts about 120 from its left: past
/// the hovered row's value alone (about 100) and short of its time and
/// value together (about 137). The pointer is just left of that edge.
fn the_readout_stays_clear_of_info_over_the_rows(s: &Steadiness) {
    // Each text drawn, with where it starts and ends, and Info's left
    // edge as the pane works it out: in a fresh context each time, as a
    // pointer one call left behind would still hover in the next.
    let drawn = |at: Option<egui::Pos2>, info: bool| {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut all = Vec::new();
        let mut left = None;
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(432.0, 700.0),
                )),
                events: at.map(egui::Event::PointerMoved).into_iter().collect(),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    left = theme::info_over_left(ui.available_rect_before_wrap(), true);
                    let opts = Opts {
                        info,
                        ..Default::default()
                    };
                    draw(ui, Some(s), "", View::Readings, &[true, true, false], opts);
                });
            });
            fn texts(shape: &egui::Shape, into: &mut Vec<(String, f32, f32)>) {
                match shape {
                    egui::Shape::Text(t) => {
                        let r = t.visual_bounding_rect();
                        into.push((t.galley.text().to_string(), r.left(), r.right()));
                    }
                    egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, into)),
                    _ => {}
                }
            }
            all.clear();
            out.shapes.iter().for_each(|c| texts(&c.shape, &mut all));
        }
        (
            all,
            left.expect("432 is too narrow for Info beside the rows"),
        )
    };
    // The readout: what the pointer on a row adds to the pane.
    let readout = |at: egui::Pos2, info: bool| {
        let (still, _) = drawn(None, info);
        let (all, left) = drawn(Some(at), info);
        let still: Vec<String> = still.into_iter().map(|t| t.0).collect();
        let added: Vec<_> = all.into_iter().filter(|t| !still.contains(&t.0)).collect();
        (added, left)
    };
    // Just left of where Info would start, on a row whose readout
    // writes the time: the pointer's own row.
    let x = drawn(None, false).1 - 8.0;
    let at = (2..28)
        .map(|i| egui::pos2(x, i as f32 * 25.0))
        .find(|&at| readout(at, false).0.iter().any(|t| t.0.contains(':')))
        .unwrap_or_else(|| {
            let (all, left) = drawn(Some(egui::pos2(x, 200.0)), false);
            panic!("no point on a Readings row (Info from {left}): {all:?}")
        });
    let (shut, left) = readout(at, false);
    assert!(shut.iter().any(|t| t.2 > left), "{left}: {shut:?}");
    let (open, left) = readout(at, true);
    assert!(!open.is_empty(), "the value stays");
    for (text, _, end) in &open {
        assert!(*end <= left, "{text} ends at {end}, past Info at {left}");
    }
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
                    take_span: None,
                    joined_in: None,
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
fn a_short_panes_info_scrolls_and_the_cycle_note_stays_in_its_column(s: &Steadiness) {
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
                    take_span: None,
                    joined_in: None,
                    ..Default::default()
                };
                draw(
                    ui,
                    Some(s),
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
fn the_info_panel_opens_at_its_top(s: &Steadiness) {
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
                        take_span: None,
                        joined_in: None,
                        ..Default::default()
                    };
                    draw(ui, Some(s), "", view, &[true, true, true], opts);
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
        for (fft_timing, info) in [(false, false), (true, false), (false, true), (true, true)] {
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
                            info,
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
            // Off the plot, in the Info panel only (#1528).
            assert_eq!(
                text.contains(if missing {
                    "128 of 256 grid samples available"
                } else {
                    "256 of 256 grid samples available"
                }),
                info,
                "{text}"
            );
            assert_eq!(
                text.contains(
                    "each size is a sine and a constant fitted together to the samples present"
                ),
                missing && info
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
    let wheel = theme::palette(ctx.style().visuals.dark_mode).callout;
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
    // The wheel's grey is the call-out grey, which the plot's other
    // call-outs use too: the wheel's line and name add to what is drawn
    // in it.
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
    let beat = timegrapher_core::synth::period(0.125, 2.0);
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
    // A join never renames a side (#1146): stretches that start on a Tick
    // or on a Tock keep every window's sides and signs as they were.
    for (start, second_start, expected_swaps) in
        [(0.0, 40.0, [false, false]), (0.125, 40.125, [false, false])]
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
            culled_ms: None,
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
    let period = timegrapher_core::timing::period_s(0.125, 10.0);
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
#[expect(
    clippy::disallowed_methods,
    reason = "a test checking the derived view against core's piecewise fit"
)]
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
        b.time = if b.index < 480 { 0.0 } else { 60.03 }
            + local as f64 * timegrapher_core::timing::period_s(0.125, rate);
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
                    culled_ms: None,
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
fn a_once_a_minute_swing_reads_periodic_and_every_view_draws(log: &BeatLog, s: Steadiness) {
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
        log,
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
    // The scale the Readings view draws them on, one reading a point.
    let spread = |pts: &[[f64; 2]]| {
        steady_views::readings_range(&Grid {
            t0: 0.0,
            step: 1.0,
            y: pts.iter().map(|p| p[1]).collect(),
        })
    };
    let (lo, hi) = spread(&pts).unwrap();
    assert!(lo > 8.0 && hi < 16.0, "{lo} to {hi}");
    // Two states, a third of the readings each way, keep both.
    let two: Vec<[f64; 2]> = (0..90)
        .map(|i| [i as f64, if i % 3 == 0 { 30.0 } else { 10.0 }])
        .collect();
    let (lo, hi) = spread(&two).unwrap();
    assert!(lo < 10.0 && hi > 30.0, "{lo} to {hi}");
}

/// A wheel's line is a full call-out; a ×2 or a reference period is a
/// fainter one, in the same grey.
#[test]
fn a_wheels_twice_and_a_reference_are_fainter_call_outs() {
    let line = |turn| WheelLine {
        x: 1.0,
        turn,
        name: String::new(),
        short: String::new(),
        hover: String::new(),
    };
    assert_eq!(line(Turn::Wheel).width(), crate::strength::CALL_OUT_W);
    assert_eq!(line(Turn::Twice).width(), 1.0);
    assert_eq!(line(Turn::Reference).width(), 1.0);
    let pal = &theme::DARK;
    assert_eq!(line(Turn::Twice).ink(pal), line(Turn::Wheel).ink(pal));
}

/// A calibre's meshes are marked after its wheels, faint and named by
/// their pinion (#1472); one at a wheel's or a ×2's period is left to that
/// line, and with the marks off there are none.
#[test]
fn meshes_are_marked_faintly_unless_a_wheel_marks_their_period() {
    let w = |name: &str, period_s: f64| Wheel {
        name: name.into(),
        period_s,
    };
    let wheels = [w("fourth wheel", 60.0)];
    let mesh = |name: &str, period_s: f64| timegrapher_core::train::MeshLine {
        name: name.into(),
        wheel: String::new(),
        period_s,
        about: format!("about the {name}"),
    };
    let meshes = [
        mesh("fourth pinion", 60.0 / 9.0),
        mesh("third pinion", 600.0 / 13.0),
        mesh("centre pinion", 30.2),
    ];
    let marks = Marks {
        wheels: &wheels,
        meshes: &meshes,
    };
    let lines = wheel_lines(Some(marks), (0.0, 5.0));
    let got: Vec<(&str, &str, bool)> = lines
        .iter()
        .map(|l| (l.name.as_str(), l.short.as_str(), l.turn == Turn::Mesh))
        .collect();
    assert_eq!(
        got,
        [
            ("Fourth Wheel", "4th Wheel", false),
            ("Fourth Wheel ×2", "4th Wheel ×2", false),
            ("Fourth Pinion Mesh", "4th Mesh", true),
            ("Third Pinion Mesh", "3rd Mesh", true),
        ]
    );
    let line = &lines[2];
    assert_eq!(line.rank(), Rank::Faint);
    assert_eq!(line.width(), 1.0);
    // Core says what it is (`train::Mesh::line`).
    assert_eq!(line.hover, "Fourth Pinion Mesh: about the fourth pinion.");
    assert_eq!(crate::marks::mesh_short("centre pinion"), "Centre Mesh");
    assert!(wheel_lines(None, (0.0, 5.0)).is_empty());
}

/// A trend or a split of levels the headline leaves out says why, in
/// core's words, as `series` and the MCP view do (#954, #971); one it
/// names says nothing more.
#[test]
fn the_stats_line_says_why_a_trend_or_split_is_left_out() {
    use timegrapher_core::steadiness::{Changes, Segment, Trend};
    let mut c = steadiness::check_series(
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
    let dropped = "not significant after Holm's correction (p 0.031 alone)";
    c.trend = Some(Trend {
        per_hour: 0.42,
        explained: 0.1,
        curve_explained: 0.1,
        start: 0.0,
        end: 0.0,
        turn: None,
        p_value: 0.031,
        significant: false,
        named: false,
        note: Some(dropped.into()),
    });
    let small = "under the 20% a level change needs";
    let seg = |a: f64| Segment {
        start_s: a,
        end_s: a + 100.0,
        mean: a,
    };
    c.changes = Some(Changes {
        block_s: 60.0,
        block_sigma: 1.0,
        long_run_sigma: 1.0,
        segments: vec![seg(0.0), seg(100.0), seg(200.0)],
        explained: 0.1,
        p_value: 0.5,
        named: false,
        note: Some(small.into()),
    });
    let line = stats_line(&c);
    assert!(
        line.contains(&format!("trend +0.42 s/d/h, {dropped}")),
        "{line}"
    );
    assert!(
        line.contains(&format!("3 stretches of one level, {small}")),
        "{line}"
    );
    // Named, with one stretch: no note, no stretches.
    if let Some(t) = c.trend.as_mut() {
        t.named = true;
        t.note = None;
    }
    if let Some(ch) = c.changes.as_mut() {
        ch.segments.truncate(1);
    }
    let line = stats_line(&c);
    assert!(
        line.contains("trend +0.42 s/d/h ·") || line.ends_with("trend +0.42 s/d/h"),
        "{line}"
    );
    assert!(!line.contains("stretches"), "{line}");
}

/// The amplitude's statistics say degrees as its sentence does, "°", not
/// core's unit name "deg".
#[test]
fn the_amplitude_stats_line_says_degrees() {
    let c = steadiness::check_series(
        SeriesKind::Amplitude,
        Grid {
            t0: 0.0,
            step: 10.0,
            y: (0..40).map(|i| 270.0 + (i as f64).sin()).collect(),
        },
        None,
        None,
        None,
        &[],
        &Config::default(),
    );
    let line = stats_line(&c);
    assert!(line.contains("median 270°"), "{line}");
    // Its ± from one reading to the next is at 1°, rounded up, as core's
    // headline writes it (Walter's ±5.8° beside ±6°, #1470).
    let st = c.short_term_sd.expect("a short-term SD");
    let pm = format!("±{}° reading to reading", st.ceil().max(1.0));
    assert!(line.contains(&pm), "{line}");
    assert!(line.contains("° ·"), "{line}");
    assert!(!line.contains("deg"), "{line}");
    assert!(!line.contains(" °"), "{line}");
}

/// The readings plot draws level lines only where the words name the
/// levels; stretches the level test turns down draw none (#1112).
#[test]
fn level_lines_are_drawn_only_where_the_words_name_them() {
    use timegrapher_core::steadiness::{Changes, Segment};
    let mut c = steadiness::check_series(
        SeriesKind::BeatError,
        Grid {
            t0: 0.0,
            step: 10.0,
            y: (0..40).map(|i| 0.37 + 0.01 * (i as f64).sin()).collect(),
        },
        None,
        None,
        None,
        &[],
        &Config::default(),
    );
    let seg = |a: f64, mean: f64| Segment {
        start_s: a,
        end_s: a + 100.0,
        mean,
    };
    c.changes = Some(Changes {
        block_s: 60.0,
        block_sigma: 1.0,
        long_run_sigma: 1.0,
        segments: vec![seg(0.0, 0.35), seg(100.0, 0.39), seg(200.0, 0.37)],
        explained: 0.1,
        p_value: 0.5,
        named: false,
        note: Some("under the 20% a level change needs".into()),
    });
    assert!(level_lines(&c).is_empty());
    assert!(stats_line(&c).contains("3 stretches of one level"));
    c.changes.as_mut().unwrap().named = true;
    assert_eq!(
        level_lines(&c),
        [
            [[0.0, 0.35], [100.0, 0.35]],
            [[100.0, 0.39], [200.0, 0.39]],
            [[200.0, 0.37], [300.0, 0.37]],
        ]
    );
    c.changes.as_mut().unwrap().segments.truncate(1);
    assert!(level_lines(&c).is_empty(), "one stretch is no level change");
}

/// Axis numbers on screen carry a true minus, as the design guide asks
/// where numbers line up (#1402); zero never reads "−0".
#[test]
fn axis_numbers_on_screen_carry_a_true_minus() {
    assert_eq!(value_text(SeriesKind::Rate, -2.0, 1.0), "\u{2212}2 s/d");
    assert_eq!(
        value_text(SeriesKind::BeatError, -0.25, 0.05),
        "\u{2212}0.25 ms"
    );
    assert_eq!(value_text(SeriesKind::Amplitude, 4.0, 1.0), "4°");
    assert_eq!(plain(-0.5, 0.5), "\u{2212}0.5");
    assert_eq!(plain(-0.0, 0.5), "0.0");
}

/// The Allan plot names its dashed line only where it is a line: with one
/// point there is none, and the caption would cover the least scatter's
/// label (#1560).
#[test]
fn a_one_point_independence_line_has_no_caption() {
    assert_eq!(independent_caption_at(&[]), None);
    assert_eq!(independent_caption_at(&[[1.0, 0.5]]), None);
    assert_eq!(
        independent_caption_at(&[[1.0, 0.5], [2.0, 0.2]]),
        Some([2.0, 0.2])
    );
}

/// Readings Over Time's readout writes the time on its axis, and whose
/// time it is while a position's stretches are joined end to end, as
/// the axis title does (#1277).
#[test]
fn the_readings_readout_says_whose_time_it_is() {
    assert_eq!(readout_time(252.0, None), "4:12");
    assert_eq!(readout_time(252.0, Some("DD")), "4:12 in DD");
    assert_eq!(readout_time(3725.0, None), "1:02:05");
}

/// While Info is drawn over the rows, a row's readout stops at Info's
/// left edge; with Info shut or beside the rows the row's own edge is
/// the limit (#1580).
#[test]
fn the_readout_stops_at_infos_left_edge_while_info_is_over_the_rows() {
    let narrow = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(540.0, 400.0));
    let wide = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 400.0));
    let left = theme::info_over_left(narrow, true).expect("over the rows");
    assert_eq!(left, 540.0 - INFO_W - 4.0);
    assert_eq!(theme::info_over_left(narrow, false), None);
    assert_eq!(theme::info_over_left(wide, true), None);
    assert_eq!(readout_right(532.0, Some(left)), left);
    assert_eq!(readout_right(532.0, None), 532.0);
}

/// A short row drops the readout's range first, then "mean of 8", then
/// the time, and the value only when it doesn't fit alone; nothing it
/// draws runs past its right edge (Hugh on #1580).
#[test]
fn a_short_readout_keeps_the_value_longest() {
    let s = |t: &str| t.to_string();
    let band = vec![s("157°"), s("mean of 8"), s("150 to 162")];
    let time = Some(s("2:07:06"));
    assert_eq!(
        readout_choices(time.clone(), &band),
        vec![
            (time.clone(), Some(s("157°, mean of 8, 150 to 162"))),
            (time.clone(), Some(s("157°, mean of 8"))),
            (time.clone(), Some(s("157°"))),
            (None, Some(s("157°"))),
        ]
    );
    assert_eq!(
        readout_choices(None, &[s("157°")]),
        vec![(None, Some(s("157°")))]
    );
    assert_eq!(
        readout_choices(time.clone(), &[]),
        vec![(time.clone(), None)]
    );

    let ctx = egui::Context::default();
    theme::install(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 400.0));
    let title = egui::Rect::from_min_size(egui::pos2(8.0, 8.0), Vec2::new(100.0, 14.0));
    // The readout's parts as drawn with the row's right edge at `right`.
    let drawn = |right: f32| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let parts = (time.clone(), band.clone());
                paint_readout(ui, (title, right), parts, Color32::RED);
            });
        });
        let mut parts = Vec::new();
        for c in &out.shapes {
            if let egui::Shape::Text(t) = &c.shape {
                let r = t.visual_bounding_rect().right();
                assert!(r <= right, "{right}: {} ends at {r}", t.galley.text());
                parts.push(t.galley.text().to_string());
            }
        }
        parts
    };
    let mut seen: Vec<Vec<String>> = Vec::new();
    for right in (0..200).map(|i| title.right() + i as f32 * 3.0) {
        let parts = drawn(right);
        if seen.last() != Some(&parts) {
            seen.push(parts);
        }
    }
    let want: Vec<Vec<String>> = vec![
        vec![],
        vec![s("157°")],
        vec![s("2:07:06"), s("157°")],
        vec![s("2:07:06"), s("157°, mean of 8")],
        vec![s("2:07:06"), s("157°, mean of 8, 150 to 162")],
    ];
    assert_eq!(seen, want);
}
