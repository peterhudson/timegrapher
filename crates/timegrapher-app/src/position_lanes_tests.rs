//! The Positions Test pane with its charts (#1729): beside the table, the
//! averages named, a 24 hour half's full-wind figures keyed, and the
//! hovered position marked in the table and every chart.

use crate::guided::{pane, FullWind, KeptRow, RESULTS_FORMAT};
use crate::theme;
use eframe::egui;

/// A full-wind test of the six and its 24 hour half set against it.
fn halves() -> FullWind {
    let row = |position: &str, rate: f64, amp: f64, be: f64| KeptRow {
        position: position.into(),
        rate_s_per_day: Some(rate),
        amplitude_deg: Some(amp),
        beat_error_ms: Some(be),
        beat_error_source: Some(timegrapher_core::session::BeatErrorSource::Unlock),
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
        positions: vec![
            row("DU", 12.2, 300.0, 0.03),
            row("DD", 10.0, 304.0, 0.04),
            row("CD", 11.6, 306.0, 0.04),
            row("CL", 10.6, 304.0, 0.03),
            row("CU", 8.6, 302.0, 0.04),
            row("CR", 11.1, 303.0, 0.05),
        ],
        wind_h: None,
        against: None,
    };
    FullWind {
        began_unix: full.began_unix + 24.1 * 3600.0,
        positions: vec![
            row("DU", 11.8, 333.0, 0.03),
            row("DD", 10.8, 326.0, 0.04),
            row("CD", 11.9, 334.0, 0.04),
            row("CL", 11.0, 326.0, 0.03),
            row("CU", 10.2, 328.0, 0.04),
            row("CR", 10.4, 327.0, 0.05),
        ],
        wind_h: Some(24.1),
        against: Some(Box::new(full.clone())),
        ..full
    }
}

/// The pane's texts and their places, at `size`, with the pointer at
/// `pointer`, after a few frames; and the shapes, for a picture.
fn drawn(
    saved: &FullWind,
    size: egui::Vec2,
    pointer: Option<egui::Pos2>,
    dark: bool,
) -> (Vec<(String, egui::Rect)>, egui::FullOutput, egui::Color32) {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    let mut out = None;
    for _ in 0..4 {
        let mut input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        if let Some(p) = pointer {
            input.events.push(egui::Event::PointerMoved(p));
        }
        out = Some(ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                pane(
                    ui,
                    None,
                    None,
                    None,
                    Some(saved),
                    None,
                    None,
                    20.0,
                    true,
                    None,
                    ("Current Position", false, [true; 3]),
                );
            });
        }));
    }
    let out = out.unwrap();
    let texts = out
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
    (texts, out, ctx.style().visuals.panel_fill)
}

fn find<'a>(texts: &'a [(String, egui::Rect)], want: &str) -> Vec<&'a egui::Rect> {
    texts
        .iter()
        .filter(|(t, _)| t == want)
        .map(|(_, r)| r)
        .collect()
}

/// At a wide window the three charts sit right of the table, each with
/// its lanes named in two groups, its averages named over it and its
/// unit at its axis's end; the 24 hour half's charts carry the key.
#[test]
fn the_charts_sit_beside_the_table_with_their_lanes_and_averages() {
    let saved = halves();
    for (dark, w) in [(true, 2000.0), (false, 1600.0), (true, 1280.0)] {
        let size = egui::vec2(w, 820.0);
        let (texts, out, back) = drawn(&saved, size, None, dark);
        let table = find(&texts, "Crown Up (CU, 3H)");
        let table_right = table.first().expect("the table").right();
        for title in ["Rate", "Amplitude", "Beat Error"] {
            let t = find(&texts, title);
            assert!(
                t.iter().any(|r| r.left() > table_right),
                "{w}: {title} chart right of the table: {t:?}"
            );
        }
        // Each chart names its groups and its six lanes.
        for name in ["Horizontal", "Vertical", "DU", "DD", "CD", "CL", "CU", "CR"] {
            let n = find(&texts, name)
                .iter()
                .filter(|r| r.left() > table_right)
                .count();
            if std::env::var("SHOT_DIR").is_err() {
                assert!(n >= 3, "{w}: {name} in every chart, {n}");
            }
        }
        // The three averages over each chart, and the unit at its end.
        assert!(find(&texts, "Average").len() >= 3, "{w}");
        for unit in ["s/d", "°", "ms"] {
            assert!(
                find(&texts, unit).iter().any(|r| r.left() > table_right),
                "{w}: {unit}"
            );
        }
        // A 24 hour half: the key, once.
        assert_eq!(find(&texts, "This test").len(), 1, "{w}");
        assert_eq!(find(&texts, "Full wind test").len(), 1, "{w}");
        if let Ok(d) = std::env::var("SHOT_DIR") {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let svg = crate::export::svg(&out.shapes, screen, back);
            let theme = if dark { "dark" } else { "light" };
            std::fs::write(format!("{d}/positions-{theme}-{w}.svg"), svg).unwrap();
        }
    }
}

/// Hovering a position's row in the table names it in the first chart's
/// title row with its figures, and rings its dot in each chart.
#[test]
fn a_hovered_row_names_its_position_in_the_charts() {
    let saved = halves();
    let size = egui::vec2(2000.0, 820.0);
    let (texts, ..) = drawn(&saved, size, None, true);
    let row = **find(&texts, "Crown Up (CU, 3H)").first().expect("row");
    let (texts, out, back) = drawn(&saved, size, Some(row.center()), true);
    let want = "Crown Up (CU, 3H) +10.2 s/d (Full wind test +8.6 s/d)";
    assert!(
        texts.iter().any(|(t, _)| t == want),
        "{want} in {:?}",
        texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    let amp = "328° (Full wind test 302°)";
    assert_eq!(
        find(&texts, amp).len(),
        1,
        "its amplitude with the full-wind test"
    );
    if let Ok(d) = std::env::var("SHOT_DIR") {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        std::fs::write(
            format!("{d}/positions-hover-dark-2000.svg"),
            crate::export::svg(&out.shapes, screen, back),
        )
        .unwrap();
    }
}

/// Too narrow for both, the charts go over the table.
#[test]
fn a_narrow_pane_puts_the_charts_over_the_table() {
    let saved = halves();
    let (texts, ..) = drawn(&saved, egui::vec2(800.0, 1400.0), None, true);
    let rate = **find(&texts, "Rate").first().expect("rate chart");
    let row = **find(&texts, "Crown Up (CU, 3H)").first().expect("row");
    assert!(rate.bottom() < row.top(), "{rate:?} over {row:?}");
}

/// The averages' names on every chart, at the narrowest pane that still
/// puts the charts beside the table (320 pt of chart, about) and at 1280:
/// no two overlap, where three averages within a name's width of each
/// other once hid one under another (Hugh on #1785).
#[test]
fn the_averages_names_never_overlap() {
    let saved = halves();
    let beside = |w: f32| {
        let (texts, ..) = drawn(&saved, egui::vec2(w, 820.0), None, true);
        let rate = **find(&texts, "Rate").first().expect("rate chart");
        let row = **find(&texts, "Crown Up (CU, 3H)").first().expect("row");
        (rate.left() > row.right(), texts)
    };
    // The narrowest width beside, to within 4 px.
    let (mut lo, mut hi) = (800.0_f32, 1280.0_f32);
    assert!(beside(hi).0 && !beside(lo).0);
    while hi - lo > 4.0 {
        let mid = (lo + hi) / 2.0;
        if beside(mid).0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    for w in [hi, 1280.0] {
        let (_, texts) = beside(w);
        let names: Vec<&egui::Rect> = ["Average", "Horizontal", "Vertical"]
            .iter()
            .flat_map(|n| find(&texts, n))
            .collect();
        assert!(names.len() >= 9, "{w}: {names:?}");
        for (i, a) in names.iter().enumerate() {
            for b in &names[i + 1..] {
                assert!(!a.intersects(**b), "{w}: {a:?} overlaps {b:?}");
            }
        }
    }
}
