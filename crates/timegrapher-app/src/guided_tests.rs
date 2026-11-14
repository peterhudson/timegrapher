//! Tests of the Positions Test and the Dynamic Poise test's guided runs.

use super::*;
use timegrapher_core::live::{LiveAnalyzer, LiveConfig};
use timegrapher_core::measure::Caveat;
use timegrapher_core::synth::{generate, SynthConfig};

/// The table's figures are core's figure text (#510).
#[test]
fn the_tables_figures_are_cores() {
    assert_eq!(signed(Some(-0.04), 1), "0.0");
    assert_eq!(signed(Some(-1.26), 1), "\u{2212}1.3");
    assert_eq!(signed(Some(2.0), 1), "+2.0");
    assert_eq!(plain(Some(-0.004), 2), "0.00");
    assert_eq!(plain(Some(-0.3), 2), "\u{2212}0.30");
    assert_eq!(plain(None, 0), "—");
}

/// The 24 Hours After half offers the full-wind tests kept in the
/// watch's takes that began 12 to 36 hours ago, newest first, and no
/// other.
#[test]
fn the_full_wind_tests_offered_are_those_from_12_to_36_hours_ago() {
    // The words the hovers give, from the same window.
    assert_eq!(full_wind_ago(), "12 to 36 hours ago");
    let dir = std::env::temp_dir().join(format!("tg-full-wind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let now = 1_800_000_000.0;
    let keep = |name: &str, hours: f64, wind_h: Option<f64>| {
        let take = dir.join(name);
        std::fs::create_dir_all(&take).unwrap();
        let f = FullWind {
            wind_h,
            against: None,
            format: RESULTS_FORMAT.into(),
            began_utc: String::new(),
            began_unix: now - hours * 3600.0,
            watch: "W".into(),
            calibre: String::new(),
            bph: None,
            lift_angle_deg: None,
            settle_s: None,
            measure_s: None,
            positions: vec![KeptRow {
                position: "DU".into(),
                rate_s_per_day: Some(1.0),
                amplitude_deg: None,
                beat_error_ms: None,
                beat_error_source: None,
            }],
        };
        std::fs::write(take.join(RESULTS_FILE), f.to_json()).unwrap();
    };
    assert_eq!(full_winds_in(&dir, now), [], "no takes folder");
    keep("a", 40.0, None);
    keep("b", 6.0, None);
    assert_eq!(full_winds_in(&dir, now), [], "too old and too new");
    keep("c", 30.0, None);
    keep("d", 20.0, None);
    // A 24 hour half's results, though in the window, are never offered.
    keep("e", 25.0, Some(24.0));
    let found: Vec<f64> = full_winds_in(&dir, now)
        .iter()
        .map(|f| f.hours_ago(now))
        .collect();
    assert_eq!(found, [20.0, 30.0], "newest first");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Each full-wind test offered says when it began and what it
/// measured, so one stopped early can be told from a whole one.
#[test]
fn a_full_wind_choice_says_when_and_what_it_measured() {
    let row = |p: &str| KeptRow {
        position: p.into(),
        rate_s_per_day: Some(1.0),
        amplitude_deg: None,
        beat_error_ms: None,
        beat_error_source: None,
    };
    let mut f = FullWind {
        wind_h: None,
        against: None,
        format: RESULTS_FORMAT.into(),
        began_utc: "2026-10-27T18:58:04.120Z".into(),
        began_unix: 0.0,
        watch: "W".into(),
        calibre: String::new(),
        bph: None,
        lift_angle_deg: None,
        settle_s: None,
        measure_s: None,
        positions: vec![row("DU")],
    };
    assert_eq!(choice_text(&f), "2026-10-27 18:58 UTC · Dial Up only");
    f.positions = ORDER.iter().map(|p| row(p.code())).collect();
    assert_eq!(choice_text(&f), "2026-10-27 18:58 UTC · 6 positions");
}

/// A figure that rounds to zero reads 0.0, with no sign.
#[test]
fn a_zero_reads_without_a_sign() {
    assert_eq!(signed(Some(-0.01), 1), "0.0");
    assert_eq!(signed(Some(0.04), 1), "0.0");
    assert_eq!(signed(Some(-0.06), 1), "−0.1");
    assert_eq!(signed(Some(2.0), 1), "+2.0");
    assert_eq!(signed(None, 1), "—");
}

/// The pane drawn for `test` in `phase` with `live` figures: each
/// piece of text and where it is.
fn drawn(test: &Test, phase: Phase, live: Option<&FreeCell>) -> Vec<(String, egui::Rect)> {
    drawn_with(Some(test), Some(phase), None, live)
}

/// [`drawn`] for any test, or none with an opened take's `saved` results.
fn drawn_with(
    test: Option<&Test>,
    phase: Option<Phase>,
    saved: Option<&FullWind>,
    live: Option<&FreeCell>,
) -> Vec<(String, egui::Rect)> {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut found = Vec::new();
    for _ in 0..2 {
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                pane(
                    ui,
                    test,
                    phase,
                    None,
                    saved,
                    None,
                    live,
                    20.0,
                    false,
                    None,
                    ("Current Position", false),
                );
            });
        });
        found = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => {
                    Some((t.galley.text().to_string(), t.visual_bounding_rect()))
                }
                _ => None,
            })
            .collect();
    }
    found
}

/// The banner's buttons keep their places as the test moves on: Stop
/// never moves, and Restart stays where it was, so a click meant for
/// Stop as a position's measuring ends never lands on Restart. They
/// sit on one line at one height, each at the left of its slot.
#[test]
fn the_test_buttons_keep_their_places_from_phase_to_phase() {
    let mut test = Test::new().timed(20.0, 60.0);
    let rect = |found: &[(String, egui::Rect)], text: &str| {
        found
            .iter()
            .find(|(t, _)| t == text)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("{text}: {found:?}"))
    };
    let at = |found: &[(String, egui::Rect)], text: &str| rect(found, text).center().x;
    assert!(matches!(test.tick(0.0), Phase::Settling(_)));
    let ph = test.tick(5.0);
    let settling = drawn(&test, ph, None);
    let measuring_phase = test.tick(30.0);
    assert!(matches!(measuring_phase, Phase::Measuring(_)));
    let measuring = drawn(&test, measuring_phase, None);
    assert_eq!(test.tick(85.0), Phase::Turn);
    let turn = drawn(&test, Phase::Turn, None);
    // Stop Test is on the Listen card; the banner keeps the rest in
    // their places.
    for found in [&settling, &measuring, &turn] {
        assert!(!found.iter().any(|(t, _)| t == "Stop"), "{found:?}");
    }
    let restart = at(&measuring, "Restart DU Measurement");
    assert_eq!(at(&settling, "Restart DU Measurement"), restart);
    assert_eq!(at(&turn, "Restart DU Measurement"), restart);
    test.advance(86.0);
    test.pause(90.0);
    let ph = test.tick(91.0);
    let paused = drawn(&test, ph, None);
    at(&paused, "Resume");
    // One line: every button's words start at the same height (their
    // ink ends lower where a letter has a tail).
    let y = rect(&measuring, "Restart DU Measurement").top();
    for (found, text) in [
        (&measuring, "Pause"),
        (&turn, "Start DD Measurement"),
        (&turn, "Restart DU Measurement"),
        (&paused, "Resume"),
    ] {
        let top = rect(found, text).top();
        assert!((top - y).abs() < 0.5, "{text} at {top}, Restart at {y}");
    }
    // On the title's line, so the step's button is in reach in the
    // shortest pane: never under the progress bar.
    let title = rect(&turn, "Turn the watch to Dial Down");
    let start = rect(&turn, "Start DD Measurement");
    assert!(
        (start.center().y - title.center().y).abs() < 6.0,
        "{title:?} {start:?}"
    );
    // Pause is its own width at the left of the first slot, where
    // Start begins, not stretched across it: its words sit well short
    // of the slot's middle, where a stretched button would centre them.
    let pause = rect(&measuring, "Pause");
    let start = rect(&turn, "Start DD Measurement");
    assert!(
        pause.center().x < start.center().x - 20.0,
        "{pause:?} {start:?}"
    );
    assert!(pause.left() > start.left(), "{pause:?} {start:?}");
}

/// The guided table's headings never run into each other: each ends
/// before the next begins.
#[test]
fn the_guided_tables_headings_stand_apart() {
    let mut test = Test::new().timed(20.0, 60.0);
    test.tick(0.0);
    let ph = test.tick(30.0);
    let found = drawn(&test, ph, None);
    let heads = [
        "Position",
        "Rate",
        "From Average",
        "Amplitude",
        "Beat Error",
    ];
    let rects: Vec<egui::Rect> = heads
        .iter()
        .map(|h| {
            found
                .iter()
                .find(|(t, _)| t == h)
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("{h}: {found:?}"))
        })
        .collect();
    for (i, w) in rects.windows(2).enumerate() {
        assert!(
            w[0].right() + 8.0 <= w[1].left(),
            "{} ends at {}, {} begins at {}",
            heads[i],
            w[0].right(),
            heads[i + 1],
            w[1].left()
        );
    }
    // Each unit under its name, never the comma form (Hugh, #1086).
    for (i, unit) in [(1, "s/d"), (2, "s/d"), (3, "°"), (4, "ms")] {
        let n = rects[i];
        let under = |r: &egui::Rect| r.top() >= n.bottom() - 1.0;
        let at_right = |r: &egui::Rect| (r.right() - n.right()).abs() < 2.0;
        let ok = found
            .iter()
            .any(|(t, r)| t == unit && under(r) && at_right(r));
        assert!(ok, "{} over {unit}: {found:?}", heads[i]);
    }
    assert!(!found
        .iter()
        .any(|(t, _)| t.contains(", s/d") || t.contains(", ms")));
}

/// The position being measured shows its figures so far, once it has
/// them; before then it says it is measuring.
#[test]
fn the_position_being_measured_shows_its_figures_so_far() {
    let mut test = Test::new().timed(20.0, 60.0);
    test.tick(0.0);
    let phase = test.tick(30.0);
    let words =
        |found: Vec<(String, egui::Rect)>| found.into_iter().map(|(t, _)| t).collect::<Vec<_>>();
    let before = words(drawn(&test, phase, None));
    assert!(before.iter().any(|t| t == "Measuring…"), "{before:?}");
    let live = FreeCell::Read(Some(-4.25), Some(281.4), Some(0.31));
    let after = words(drawn(&test, phase, Some(&live)));
    for want in ["−4.2", "281", "0.31"] {
        assert!(after.iter().any(|t| t == want), "{want}: {after:?}");
    }
    assert!(!after.iter().any(|t| t == "Measuring…"), "{after:?}");
    // Settling, there is nothing to show yet, whatever is passed.
    let mut test = Test::new().timed(20.0, 60.0);
    test.tick(0.0);
    let ph = test.tick(5.0);
    let settling = words(drawn(&test, ph, Some(&live)));
    assert!(settling.iter().any(|t| t == "Settling…"), "{settling:?}");
    assert!(!settling.iter().any(|t| t == "281"), "{settling:?}");
}

/// A bump of the stand inside a position, masked: the position is read
/// without it, its rate comes back, the masked caveat says how much was
/// left out, and the masked seconds aren't counted as beats missing.
/// Masking after the test has finished reads it again.
#[test]
fn a_masked_bump_inside_a_position_is_left_out() {
    let each = SETTLE_S + MEASURE_S;
    // The first position, dial up, measures from 20 s to 80 s; the
    // bump knocks the beats about for 6 s from 45 s.
    let bump = (45.0, 51.0);
    let cfg = SynthConfig {
        duration_s: each * 6.0 + 1.0,
        rate_s_per_day: 0.0,
        ..Default::default()
    };
    let audio = generate(
        &cfg,
        |_| 280.0,
        |t| {
            if t >= bump.0 && t < bump.1 {
                4000.0
            } else {
                3.0
            }
        },
    );
    let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
    for b in audio.samples.chunks(4800) {
        live.push(b);
    }
    let mut t = Test::new();
    t.tick(0.0);
    for i in 1..=6 {
        t.tick(each * i as f64);
        t.advance(each * i as f64);
    }
    let log = crate::steady::log_of(&live).expect("beat rate");
    t.finish(&log, None, &[]);
    let first = |t: &Test| t.readings.as_ref().unwrap()[0].measurement.clone();
    let bumped = first(&t).rate_s_per_day.unwrap();
    assert!((bumped - 3.0).abs() > 5.0, "the bump didn't show: {bumped}");

    let masked = [(bump.0 - 1.0, bump.1 + 1.0)];
    live.set_masked(&masked);
    let log = crate::steady::log_of(&live).expect("beat rate");
    t.reread(&log, None, &masked);
    let m = first(&t);
    let rate = m.rate_s_per_day.unwrap();
    assert!((rate - 3.0).abs() < 1.0, "rate {rate}");
    assert!((m.masked_s - 8.0).abs() < 1e-9, "masked {}", m.masked_s);
    assert!(
        m.caveats
            .iter()
            .any(|c| matches!(c, Caveat::Masked { seconds } if (seconds - 8.0).abs() < 1e-9)),
        "{:?}",
        m.caveats
    );
    assert!(
        !m.caveats
            .iter()
            .any(|c| matches!(c, Caveat::BeatsMissing { .. })),
        "{:?}",
        m.caveats
    );
    // The other positions are as they were.
    let r = t.readings.as_ref().unwrap();
    assert_eq!(r.len(), 6);
    assert_eq!(r[1].measurement.masked_s, 0.0);
}

/// A jump in the beats inside a position (the sound lost its place by
/// 40 ms at 50 s): the position's rate is fitted on each side of it
/// and combined, as the whole take's is, not across it.
#[test]
fn a_jump_inside_a_position_is_cut() {
    let each = SETTLE_S + MEASURE_S;
    let duration = each * 6.0 + 1.0;
    let period = timegrapher_core::timing::period_s(0.125, 3.0);
    let log = BeatLog {
        sample_rate: 48000,
        duration_s: duration,
        bph: 28800,
        lift_deg: 52.0,
        beats: (0..)
            .map(|index| {
                let t = index as f64 * period;
                timegrapher_core::beats::Beat {
                    index,
                    time: t + if t >= 50.0 { 0.04 } else { 0.0 },
                    quality: 1.0,
                }
            })
            .take_while(|b| b.time < duration)
            .collect(),
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: vec![50.0],
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    let mut t = Test::new();
    t.tick(0.0);
    for i in 1..=6 {
        t.tick(each * i as f64);
        t.advance(each * i as f64);
    }
    t.finish(&log, None, &[]);
    let m = &t.readings.as_ref().unwrap()[0].measurement;
    let rate = m.rate_s_per_day.unwrap();
    assert!((rate - 3.0).abs() < 0.5, "rate {rate}");
}

/// Six positions with their own rates, each held for one settle and
/// one measurement: the test reads each position's rate and the
/// largest difference between them.
#[test]
#[expect(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn six_positions_read_their_own_rates() {
    let each = SETTLE_S + MEASURE_S;
    let rates = [2.0, 4.0, -3.0, -1.0, 0.0, 6.0];
    let cfg = SynthConfig {
        duration_s: each * 6.0 + 1.0,
        rate_s_per_day: 0.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |t| rates[((t / each) as usize).min(5)]);
    let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
    for b in audio.samples.chunks(4800) {
        live.push(b);
    }
    let log = crate::steady::log_of(&live).expect("beat rate");
    let mut t = Test::new();
    t.tick(0.0);
    for i in 1..=6 {
        assert_eq!(t.tick(each * i as f64), Phase::Turn);
        // Each position is read as soon as it is measured.
        assert!(t.needs_reading());
        t.read_new(&log, None, &[]);
        assert_eq!(t.readings_so_far().len(), i);
        t.advance(each * i as f64);
    }
    let so_far = t.readings_so_far().to_vec();
    t.finish(&log, None, &[]);
    let rates_of = |v: &[Reading]| -> Vec<Option<f64>> {
        v.iter().map(|r| r.measurement.rate_s_per_day).collect()
    };
    assert_eq!(rates_of(t.readings.as_deref().unwrap()), rates_of(&so_far));
    let readings = t.readings.as_ref().unwrap();
    assert_eq!(readings.len(), 6);
    for (r, want) in readings.iter().zip(rates) {
        let got = r.measurement.rate_s_per_day.unwrap();
        assert!((got - want).abs() < 1.0, "{}: {got} for {want}", r.label);
    }
    // The table's figures are session's: its D and mean X, its names.
    let tbl = table(readings, Kind::Positions);
    let report = session::evaluate(readings);
    let d = tbl.largest_difference.unwrap();
    assert!((d - report.states[0].d_rate.unwrap()).abs() < 1e-9);
    assert!((tbl.mean_rate.unwrap() - report.states[0].x.unwrap()).abs() < 1e-9);
    let dvh = tbl.dvh_rate.unwrap();
    assert!((dvh - report.states[0].dvh_rate.unwrap()).abs() < 1e-9);
    let dvh_a = tbl.dvh_amplitude.unwrap();
    assert!((dvh_a - report.states[0].dvh_amplitude.unwrap()).abs() < 1e-9);
    assert!((d - 9.0).abs() < 1.5, "D {d}");
    assert_eq!(tbl.rows[4].0, "Crown Up (CU, 3H)");
    let spread: f64 = tbl.rows.iter().filter_map(|r| r.2).sum();
    assert!(spread.abs() < 1e-9, "differences from the mean sum to zero");

    // Kept as the full-wind test, then the same watch 24 hours on,
    // 2 s/d faster in every position but crown right, 8 s/d faster
    // there, and 20° lower.
    let about = About {
        watch: "Test Watch".into(),
        calibre: "ETA 2824-2".into(),
        bph: Some(28800),
        lift_angle_deg: 52.0,
    };
    let full = t.kept(&about).unwrap();
    assert_eq!(full.positions.len(), 6);
    assert_eq!(full.positions[5].position, "CR");
    // Saved and read back as it was, with how it was read.
    let full = FullWind::from_json(&full.to_json()).unwrap();
    assert_eq!(full.calibre, "ETA 2824-2");
    assert_eq!(
        (full.bph, full.lift_angle_deg, full.settle_s, full.measure_s),
        (Some(28800), Some(52.0), Some(SETTLE_S), Some(MEASURE_S))
    );
    assert!(FullWind::from_json("{}").is_err());
    // A file saved before those were kept still reads.
    let mut old: serde_json::Value = serde_json::from_str(&full.to_json()).unwrap();
    for k in ["bph", "lift_angle_deg", "settle_s", "measure_s"] {
        old.as_object_mut().unwrap().remove(k);
    }
    let old = FullWind::from_json(&old.to_string()).unwrap();
    assert_eq!((old.bph, old.settle_s), (None, None));
    // The text saved and copied names the watch and its calibre first.
    let csv = export_text(&t, &about, ',');
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "Watch,Test Watch");
    assert_eq!(lines[1], "Calibre,ETA 2824-2");
    assert_eq!(lines[2], "\"Beat Rate, bph\",28800");
    assert_eq!(lines[3], "\"Lift Angle, °\",52");
    assert!(lines[4].starts_with("\"Test Began, UTC\",20"));
    assert!(csv.contains("\nPosition,Rate (s/d),"));
    let later: Vec<Reading> = readings
        .iter()
        .map(|r| {
            let mut r = r.clone();
            let m = &mut r.measurement;
            let up = if r.position == session::Position::H12 {
                8.0
            } else {
                2.0
            };
            m.rate_s_per_day = m.rate_s_per_day.map(|x| x + up);
            m.amplitude_deg = m.amplitude_deg.map(|x| x - 20.0);
            r
        })
        .collect();
    let c = comparison(&full, 24.0, &later).unwrap();
    assert_eq!(c.rows.len(), 6);
    for (p, v) in &c.rows {
        let label = position_label(*p);
        let up = if label.starts_with("Crown Right") {
            8.0
        } else {
            2.0
        };
        assert!((v[2].unwrap() - up).abs() < 1e-9, "{label}: {v:?}");
        assert!((v[5].unwrap() + 20.0).abs() < 1e-9, "{label}: {v:?}");
    }
    assert!((c.im.unwrap() - 2.0).abs() < 1e-9, "crown right left out");
    assert!(
        (c.ie.unwrap() - 3.0).abs() < 1e-9,
        "the mean of the changes"
    );
    let csv = comparison_text(&c, ',');
    assert!(csv.starts_with("Position,\"Rate at Full Wind, s/d\",\"Rate at 24.0 h, s/d\""));
    // A 24 hour half keeps its own rows, the hours since full wind and
    // the results it was set against, and reads back to the same
    // comparison from the file alone.
    let mut half = Test::after(full.clone());
    let h = half.wind_h().unwrap();
    assert!(h > 0.0);
    half.readings = Some(later.clone());
    let kept = half.kept(&about).unwrap();
    assert_eq!(
        (kept.wind_h, kept.against.as_deref()),
        (Some(h), Some(&full))
    );
    let saved = FullWind::from_json(&kept.to_json()).unwrap();
    assert_eq!(saved, kept);
    let run = comparison(&full, h, &later).unwrap();
    let again = saved_comparison(&saved).unwrap();
    assert_eq!(
        (again.hours, again.im, again.ie),
        (run.hours, run.im, run.ie)
    );
    assert_eq!(again.rows.len(), 6);
    for ((a, x), (b, y)) in run.rows.iter().zip(&again.rows) {
        assert_eq!(a, b);
        for (x, y) in x.iter().zip(y) {
            assert!((x.unwrap() - y.unwrap()).abs() < 1e-9, "{a}: {x:?} {y:?}");
        }
    }
    // A full-wind test's results have nothing to compare, and its file
    // carries neither key.
    assert!(saved_comparison(&full).is_none());
    assert!(!full.to_json().contains("wind_h"));
    // Its text carries the comparison and the hours since full wind.
    let csv = export_text(&half, &about, ',');
    assert!(csv.contains("Hours After Full Wind,"));
    assert!(csv.contains("\"Rate at Full Wind, s/d\""));
}

#[test]
fn the_test_settles_measures_and_waits_for_each_turn() {
    let mut t = Test::new();
    assert_eq!(t.tick(100.0), Phase::Settling(SETTLE_S));
    assert_eq!(t.tick(100.0 + SETTLE_S + 10.0), Phase::Measuring(10.0));
    let end = 100.0 + SETTLE_S + MEASURE_S;
    assert_eq!(t.tick(end), Phase::Turn);
    assert_eq!(t.measured_count(), 1);
    assert_eq!(t.next_position(), Some(Position::DialDown));
    // Measured again: the first stretch is dropped and made afresh.
    t.redo(end);
    assert_eq!(t.measured_count(), 0);
    assert_eq!(t.tick(end + SETTLE_S + MEASURE_S), Phase::Turn);
    assert_eq!(t.measured_count(), 1);
    for (i, &p) in ORDER.iter().enumerate().skip(1) {
        let now = 1000.0 * i as f64;
        assert_eq!(t.advance(now), Some(p));
        assert_eq!(t.tick(now + SETTLE_S + MEASURE_S), Phase::Turn);
    }
    assert_eq!(t.measured_count(), 6);
    assert_eq!(t.next_position(), None);
}

/// A test with its own times settles and measures for them.
#[test]
fn the_times_in_each_position_can_be_set() {
    let mut t = Test::new().timed(45.0, 120.0);
    assert_eq!(t.tick(0.0), Phase::Settling(45.0));
    assert_eq!(t.tick(50.0), Phase::Measuring(5.0));
    assert_eq!(t.tick(165.0), Phase::Turn);
    assert_eq!(t.stretches(), [(Position::DialUp, 45.0, 165.0)]);
    assert_eq!(
        t.unused(170.0),
        [(0.0, 45.0, SETTLING), (165.0, 170.0, NOT_USED)]
    );
}

/// Paused part way through measuring: what was measured is left out,
/// the pause is Not Used, and Resume settles and measures afresh.
#[test]
fn a_pause_starts_the_position_again_on_resume() {
    let (sm, m) = (SETTLE_S, MEASURE_S);
    let mut t = Test::new();
    t.tick(0.0);
    t.pause(sm + 10.0);
    assert_eq!(t.tick(500.0), Phase::Paused);
    assert!(t.measured(500.0).is_empty());
    assert_eq!(
        t.unused(100.0),
        [
            (0.0, sm, SETTLING),
            (sm, sm + 10.0, NOT_USED),
            (sm + 10.0, 100.0, NOT_USED)
        ]
    );
    t.resume(100.0);
    assert_eq!(t.tick(100.0), Phase::Settling(sm));
    assert_eq!(t.tick(100.0 + sm + m), Phase::Turn);
    assert_eq!(
        t.stretches(),
        [(Position::DialUp, 100.0 + sm, 100.0 + sm + m)]
    );
    assert_eq!(t.unused(100.0 + sm + m)[2], (sm + 10.0, 100.0, NOT_USED));
}

/// The time the test leaves out: each settling, the wait for the next
/// turn, a measurement started again, and a test stopped part way.
#[test]
fn the_unused_time_is_named() {
    let (sm, m) = (SETTLE_S, MEASURE_S);
    let mut t = Test::new();
    t.tick(0.0);
    assert_eq!(t.unused(5.0), [(0.0, 5.0, SETTLING)]);
    assert_eq!(t.unused(sm + 1.0), [(0.0, sm, SETTLING)]);
    // Measured, then 30 s turning the watch before the next start.
    let end = sm + m;
    t.tick(end);
    assert_eq!(
        t.unused(end + 30.0),
        [(0.0, sm, SETTLING), (end, end + 30.0, NOT_USED)]
    );
    t.advance(end + 30.0);
    let s2 = end + 30.0;
    // Knocked 10 s into measuring: that try is left out whole.
    t.redo(s2 + sm + 10.0);
    let s3 = s2 + sm + 10.0;
    let u = t.unused(s3 + 1.0);
    assert_eq!(
        &u[2..],
        [
            (s2, s2 + sm, SETTLING),
            (s2 + sm, s3, NOT_USED),
            (s3, s3 + 1.0, SETTLING)
        ]
    );
    // Stopped while measuring: none of it was used.
    t.stop(s3 + sm + 5.0);
    let u = t.unused(s3 + 100.0);
    assert_eq!(u.last(), Some(&(s3 + sm, s3 + sm + 5.0, NOT_USED)));
    assert_eq!(t.stretches().len(), 1);
}

/// Stopped while waiting to turn the watch, the position just measured
/// keeps its measured stretch: only the wait since is unused.
#[test]
fn a_stop_while_turning_keeps_the_measured_position() {
    let (sm, m) = (SETTLE_S, MEASURE_S);
    let mut t = Test::new();
    t.tick(0.0);
    let end = sm + m;
    assert_eq!(t.tick(end + 1.0), Phase::Turn);
    t.stop(end + 30.0);
    let u = t.unused(end + 40.0);
    assert_eq!(u, [(0.0, sm, SETTLING), (end, end + 30.0, NOT_USED)]);
    assert_eq!(t.stretches(), [(Position::DialUp, sm, end)]);
}

/// A test begun ten minutes into the sound: the time before it is
/// named, and the first position is measured from its own settle, the
/// stretch under way included as far as it has got.
#[test]
fn the_time_before_the_test_and_the_measuring_are_marked() {
    let (sm, m) = (SETTLE_S, MEASURE_S);
    let mut t = Test::new();
    t.tick(600.0);
    assert_eq!(t.unused(601.0)[0], (0.0, 600.0, BEFORE_TEST));
    assert!(t.measured(610.0).is_empty());
    assert_eq!(
        t.measured(600.0 + sm + 5.0),
        [(600.0 + sm, 600.0 + sm + 5.0)]
    );
    // Started again: Before Test stays where the test began.
    t.redo(600.0 + sm + 10.0);
    assert_eq!(t.unused(700.0)[0], (0.0, 600.0, BEFORE_TEST));
    let s = 600.0 + sm + 10.0;
    t.tick(s + sm + m);
    assert_eq!(t.measured(s + sm + m + 30.0), [(s + sm, s + sm + m)]);
}

#[test]
fn unknown_saved_sources_stay_visible_but_do_not_enter_the_table_mean() {
    let t = table_of(
        &[
            (
                session::Position::CH,
                Some(2.0),
                Some(280.0),
                Some(0.4),
                None,
            ),
            (
                session::Position::CB,
                Some(4.0),
                Some(260.0),
                Some(0.8),
                None,
            ),
        ],
        Vec::new(),
        Kind::Positions,
    );
    assert_eq!(t.rows[0].4, Some(0.4));
    assert_eq!(t.rows[1].4, Some(0.8));
    assert_eq!(t.mean_beat_error, None);
    assert_eq!(t.largest_beat_error, None);
    assert_eq!(t.mean_rate, Some(3.0));
    assert_eq!(t.mean_amplitude, Some(270.0));
    assert!(t.notes.iter().any(|n| n.contains("source not recorded")));
    assert!(t.notes.iter().any(|n| n.contains("leaves out 2 readings")));
}

#[test]
fn saved_beat_error_rows_keep_values_without_guessing_old_sources() {
    let old = r#"{"position":"DU","rate_s_per_day":1.0,"amplitude_deg":270.0,"beat_error_ms":0.8}"#;
    let mut row: KeptRow = serde_json::from_str(old).unwrap();
    assert_eq!(row.beat_error_ms, Some(0.8));
    assert_eq!(row.beat_error_source, None);
    assert_eq!(
        serde_json::to_value(&row).unwrap()["beat_error_source"],
        serde_json::Value::Null
    );
    row.beat_error_source = Some(session::BeatErrorSource::Drop);
    let saved = serde_json::to_string(&row).unwrap();
    assert_eq!(serde_json::from_str::<KeptRow>(&saved).unwrap(), row);
}

/// The table's means and differences are `session`'s: a position read
/// twice counts once, its readings averaged, as core
/// `session::sourced_indices` takes it.
#[test]
fn the_table_summary_is_the_session_values() {
    use session::Position as P;
    let figures: Vec<SourcedFigures> = vec![
        (
            P::CH,
            Some(10.0),
            Some(280.0),
            Some(0.2),
            Some(session::BeatErrorSource::Unlock),
        ),
        (
            P::CH,
            Some(20.0),
            Some(290.0),
            Some(0.4),
            Some(session::BeatErrorSource::Unlock),
        ),
        (
            P::H3,
            Some(0.0),
            Some(250.0),
            Some(0.1),
            Some(session::BeatErrorSource::Unlock),
        ),
    ];
    let t = table_of(&figures, Vec::new(), Kind::Positions);
    let ix = session::sourced_indices(0.0, &figures);
    // Dial Up averages to 15 s/d before the mean and the difference.
    assert_eq!(t.mean_rate, Some(7.5));
    assert_eq!(t.largest_difference, Some(15.0));
    // Dial Up's 285° and 0.3 ms against Crown Up's 250° and 0.1 ms.
    assert_eq!(t.largest_amplitude, Some(35.0));
    assert!((t.largest_beat_error.unwrap() - 0.2).abs() < 1e-9);
    assert_eq!(t.largest_amplitude, ix.d_amplitude);
    assert_eq!(t.largest_beat_error, ix.d_beat_error);
    assert_eq!(t.mean_rate, ix.x);
    assert_eq!(t.mean_amplitude, ix.amplitude_mean);
    assert_eq!(t.mean_beat_error, ix.beat_error_mean);
    assert_eq!(t.dvh_rate, ix.dvh_rate);
    assert_eq!(t.dvh_amplitude, ix.dvh_amplitude);
    assert_eq!(t.rows.len(), 3);
    assert_eq!(t.rows[0].2, Some(2.5));
}

/// The table saved and copied: a label holding the comma is quoted in
/// the CSV and not with tabs, a missing figure is an empty field, and
/// the minus sign is a plain one.
#[test]
fn the_table_is_written_for_a_spreadsheet() {
    let t = Table {
        beat_error_hovers: Vec::new(),
        beat_error_dimmed: Vec::new(),
        beat_error_summary: String::new(),
        rows: vec![
            (
                "Dial Up (DU)".into(),
                Some(-3.25),
                Some(-1.0),
                Some(281.0),
                Some(0.4),
            ),
            ("Crown Up (CU, 3H)".into(), None, None, Some(250.0), None),
        ],
        mean_rate: Some(-2.25),
        mean_amplitude: Some(265.5),
        mean_beat_error: Some(0.4),
        largest_difference: None,
        largest_amplitude: Some(31.0),
        largest_beat_error: None,
        dvh_rate: None,
        dvh_amplitude: Some(-31.0),
        notes: vec!["Crown Up (CU, 3H): no regular beat found".into()],
    };
    let csv = table_text(&t, ',', Kind::Positions);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "Position,Rate (s/d),From Average (s/d),Amplitude (°),Beat Error (ms)"
    );
    assert_eq!(lines[1], "Dial Up (DU),-3.2,-1.0,281,0.40");
    assert_eq!(lines[2], "\"Crown Up (CU, 3H)\",,,250,");
    assert_eq!(lines[3], "Average,-2.2,,266,0.40");
    assert_eq!(lines[4], "Largest Difference,,,31,");
    assert_eq!(lines[5], "");
    assert_eq!(lines[7], "Vertical less horizontal amplitude (°),-31");
    assert_eq!(lines[8], "\"Crown Up (CU, 3H): no regular beat found\"");
    let tsv = table_text(&t, '\t', Kind::Positions);
    assert!(tsv
        .lines()
        .nth(2)
        .unwrap()
        .starts_with("Crown Up (CU, 3H)\t\t"));
    assert!(!csv.contains('−') && !tsv.contains('"'));
}
#[test]
fn position_sources_live_in_cell_hovers_with_one_summary() {
    use session::{BeatErrorSource as S, Position as P};
    let t = table_of(
        &[
            (P::CH, Some(1.0), Some(270.0), Some(0.2), Some(S::Unlock)),
            (P::CB, Some(2.0), Some(280.0), Some(0.8), Some(S::Drop)),
            (P::H3, Some(3.0), Some(290.0), Some(0.4), Some(S::Unlock)),
        ],
        Vec::new(),
        Kind::Positions,
    );
    assert_eq!(t.notes.len(), 1);
    assert_eq!(t.beat_error_hovers[0], "Beat Error from the Unlock");
    assert_eq!(t.beat_error_hovers[1], "Beat Error from the Drop");
    // Dimmed as the Power Reserve table dims it (#1076).
    assert_eq!(t.beat_error_dimmed, vec![false, true, false]);
    assert!(t.beat_error_summary.contains("mean of 2 of 3 positions"));
    assert_eq!(t.notes[0], t.beat_error_summary);
}

/// The table's beat-error cells and key, as drawn: a beat error from
/// the drop in secondary ink, one from the unlock in text ink, and the
/// key above the table only when a row is dimmed (#1076).
#[test]
fn a_position_from_the_drop_is_drawn_grey_under_its_key() {
    use session::{BeatErrorSource as S, Position as P};
    let draw = |drop_source: S| {
        let t = table_of(
            &[
                (P::CH, Some(1.0), Some(270.0), Some(0.2), Some(S::Unlock)),
                (P::CB, Some(2.0), Some(280.0), Some(0.8), Some(drop_source)),
            ],
            Vec::new(),
            Kind::Positions,
        );
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut ink = (egui::Color32::TRANSPARENT, egui::Color32::TRANSPARENT);
        let mut cells = Vec::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ink = (theme::pal(ui).text, theme::pal(ui).text_secondary);
                    table_grid(ui, ("positions", Kind::Positions), &t, None, None);
                });
            });
            cells = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(s) => Some((
                        s.galley.text().to_string(),
                        s.galley.job.sections.first().map(|f| f.format.color),
                        s.pos.y,
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
        }
        (cells, ink)
    };
    let find = |cells: &[(String, Option<egui::Color32>, f32)], text: &str| {
        cells.iter().find(|c| c.0 == text).map(|c| (c.1, c.2))
    };
    let (cells, ink) = draw(S::Drop);
    assert_eq!(
        find(&cells, "0.80").map(|c| c.0),
        Some(Some(ink.1)),
        "{cells:?}"
    );
    assert_eq!(
        find(&cells, "0.20").map(|c| c.0),
        Some(Some(ink.0)),
        "{cells:?}"
    );
    let key = find(&cells, crate::reserve::DIM_KEY_TEXT).expect("key drawn");
    assert_eq!(key.0, Some(ink.1));
    let head = find(&cells, "Beat Error").expect("title drawn");
    assert!(key.1 < head.1, "{cells:?}");
    // Every row from the unlock: no grey cell, and no key.
    let (cells, ink) = draw(S::Unlock);
    assert_eq!(
        find(&cells, "0.80").map(|c| c.0),
        Some(Some(ink.0)),
        "{cells:?}"
    );
    assert!(
        find(&cells, crate::reserve::DIM_KEY_TEXT).is_none(),
        "{cells:?}"
    );
}

/// The Dynamic Poise test's banner names each stop by the hour at the
/// top, says to turn the watch 45°, and its unused time is its own.
#[test]
fn the_poise_banner_turns_the_watch_45_degrees_by_the_hour() {
    let mut t = Test::poise().timed(1.0, 2.0);
    assert_eq!(t.position(), Position::CrownDown);
    t.tick(0.0);
    assert_eq!(t.tick(3.5), Phase::Turn);
    let (title, detail, _) = phase_words(&t, Phase::Turn);
    assert_eq!(title, "Done: Crown Down (1 of 8)");
    assert!(
        detail.starts_with(
            "Turn the watch 45°, to 7:30 Up (7H30) at the top, then press Start 7H30 Measurement."
        ),
        "{detail}"
    );
    assert!(t.unused(4.0).iter().any(|u| u.2 == NOT_USED_POISE));
    assert_eq!(t.advance(4.0), Some(Position::Up7h30));
    let phase = t.tick(4.5);
    let (title, _, _) = phase_words(&t, phase);
    assert_eq!(title, "7:30 Up (2 of 8)");
    assert_eq!(Kind::Poise.code(Position::CrownLeft), "6H");
    assert_eq!(Kind::Positions.code(Position::CrownLeft), "CL");
}

/// The pane with nothing yet, its Info open or shut: each piece of text.
fn drawn_empty(info: bool) -> Vec<String> {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut found = Vec::new();
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 600.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let card = ("Current Position", info);
                pane(
                    ui, None, None, None, None, None, None, 20.0, false, None, card,
                );
            });
        });
        found = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
    }
    found
}

/// The table's method notes are the Info panel's, drawn only with it
/// open; under the empty table, one line where the test starts (Hugh,
/// #1528).
#[test]
fn the_method_notes_are_in_info_and_the_empty_table_keeps_one_line() {
    let shut = drawn_empty(false);
    let open = drawn_empty(true);
    let lines = info_lines(20.0);
    assert!(
        lines[0].starts_with("Duration leaves out the first 20 s"),
        "{lines:?}"
    );
    for l in &lines {
        assert!(!shut.contains(l), "{l}: {shut:?}");
        assert!(open.contains(l), "{l}: {open:?}");
    }
    let start = "New Test leads you through the six positions.";
    assert!(shut.iter().any(|t| t == start), "{shut:?}");
    assert!(
        !shut.iter().any(|t| t.contains("It lets the watch settle")),
        "{shut:?}"
    );
}

/// An opened 24 hour half's take shows the run again from its saved
/// results alone: its own table under the hours since full wind, and
/// the comparison with the full-wind results it was set against.
#[test]
fn an_opened_24_hour_half_shows_its_table_and_comparison() {
    let row = |position: &str, rate: f64, amp: f64| KeptRow {
        position: position.into(),
        rate_s_per_day: Some(rate),
        amplitude_deg: Some(amp),
        beat_error_ms: None,
        beat_error_source: None,
    };
    let full = FullWind {
        format: RESULTS_FORMAT.into(),
        began_utc: "2026-11-12T08:00:00.000Z".into(),
        began_unix: 1_794_470_400.0,
        watch: "W".into(),
        calibre: String::new(),
        bph: Some(28800),
        lift_angle_deg: Some(52.0),
        settle_s: None,
        measure_s: None,
        positions: vec![row("DU", 2.0, 300.0), row("CD", 6.0, 270.0)],
        wind_h: None,
        against: None,
    };
    let later = FullWind {
        began_unix: full.began_unix + 24.5 * 3600.0,
        positions: vec![row("DU", 5.0, 260.0), row("CD", 12.0, 240.0)],
        wind_h: Some(24.5),
        against: Some(Box::new(full.clone())),
        ..full.clone()
    };
    let c = saved_comparison(&later).unwrap();
    assert_eq!(c.rows.len(), 2);
    assert_eq!(c.rows[0].1[2], Some(3.0));
    assert_eq!(c.rows[1].1[5], Some(-30.0));
    let texts: Vec<String> = drawn_with(None, None, Some(&later), None)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    for want in [
        "This Test, 24.5 h After Full Wind",
        "Full Wind Against 24.5 Hours",
        "Rate at 24.5 h",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want} in {texts:?}");
    }
    assert!(!texts.iter().any(|t| t == "This Take"), "not read again");
}

/// Over an opened 24 hour half, whose tables have rows, the pane shows
/// no line for an empty table (Hugh, #1552).
#[test]
fn an_opened_24_hour_half_shows_no_empty_table_line() {
    let row = |position: &str| KeptRow {
        position: position.into(),
        rate_s_per_day: Some(2.0),
        amplitude_deg: Some(280.0),
        beat_error_ms: None,
        beat_error_source: None,
    };
    let full = FullWind {
        format: RESULTS_FORMAT.into(),
        began_utc: "2026-11-12T08:00:00.000Z".into(),
        began_unix: 1_794_470_400.0,
        watch: "W".into(),
        calibre: String::new(),
        bph: Some(28800),
        lift_angle_deg: Some(52.0),
        settle_s: None,
        measure_s: None,
        positions: vec![row("DU")],
        wind_h: None,
        against: None,
    };
    let later = FullWind {
        wind_h: Some(24.5),
        against: Some(Box::new(full.clone())),
        ..full
    };
    let texts: Vec<String> = drawn_with(None, None, Some(&later), None)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    let start = "New Test leads you through the six positions.";
    assert!(
        texts.iter().any(|t| t.starts_with("This Test")),
        "{texts:?}"
    );
    assert!(!texts.iter().any(|t| t == start), "{texts:?}");
}

/// Six positions, 10 s each, read from one synthetic watch.
fn six_readings() -> Vec<Reading> {
    let cfg = SynthConfig {
        duration_s: 61.0,
        rate_s_per_day: 3.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
    for b in audio.samples.chunks(4800) {
        live.push(b);
    }
    let log = crate::steady::log_of(&live).expect("beat rate");
    ORDER
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let from = 10.0 * i as f64;
            Reading {
                label: p.code().into(),
                position: p.core(),
                wind_h: None,
                date: None,
                notes: None,
                duration_s: 10.0,
                bph: 28800,
                bph_set: Some(28800),
                measurement: session::measure(&log, None, from, from + 10.0),
                cycles: Vec::new(),
                shape: None,
                reference: None,
            }
        })
        .collect()
}

/// Save as Text… writes core's report of the test (#1508): the watch
/// and how it was read, its Date from the take's name, the table as
/// core writes it, and in the 24 hour half the full-wind test beside
/// it. The lift angle is left out when it is the default.
#[test]
fn the_text_report_is_cores() {
    let about = About {
        watch: "Test Watch".into(),
        calibre: "ETA 2824-2".into(),
        bph: Some(28800),
        lift_angle_deg: 52.0,
    };
    let readings = six_readings();
    let mut t = Test::new();
    t.readings = Some(readings.clone());
    let text = report_text(&t, &about, true, Some("2026-11-13 19.17")).unwrap();
    let header = text_report::Header {
        watch: Some("Test Watch".into()),
        date: Some("2026-11-13 19:17".into()),
        calibre: Some("ETA 2824-2".into()),
        bph: Some(28800),
        lift_angle_deg: Some(52.0),
        settle_s: Some(t.settle_s),
        measure_s: Some(t.measure_s),
    };
    assert_eq!(text, text_report::positions(&header, &readings, None));
    assert!(text.contains("Date        2026-11-13 19:17\n"), "{text}");
    let unset = report_text(&t, &about, false, None).unwrap();
    assert!(
        !unset.contains("Lift Angle") && !unset.contains("Date"),
        "{unset}"
    );
    // The 24 hour half carries the full-wind test beside it.
    let mut half = Test::after(t.kept(&about).unwrap());
    half.readings = Some(readings);
    let text = report_text(&half, &about, true, None).unwrap();
    assert!(text.contains("Against the Full-Wind Test"), "{text}");
}
