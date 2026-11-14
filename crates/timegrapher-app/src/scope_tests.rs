//! The Scope as it is drawn and set: each span's window and grid, the
//! sound a trace keeps, the rows, their axis and edges, the newest beat
//! strongest, and the Span and Beats controls.

use super::*;
use eframe::egui::{Event, FullOutput, PointerButton, Pos2, Rect, Shape};

const SPANS: [Zoom; 6] = [
    Zoom::Ms5,
    Zoom::Ms10,
    Zoom::Ms20,
    Zoom::Ms50,
    Zoom::Ms200,
    Zoom::Ms400,
];

fn themed() -> egui::Context {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    ctx
}

/// One frame of `body` in a panel `w` × `h`.
fn frame(
    ctx: &egui::Context,
    (w, h): (f32, f32),
    events: Vec<Event>,
    body: impl FnMut(&mut egui::Ui),
) -> FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h))),
        events,
        ..Default::default()
    };
    let mut body = body;
    ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| body(ui));
    })
}

/// Each piece of text a frame painted, with where.
fn texts(out: &FullOutput) -> Vec<(String, Rect)> {
    out.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            Shape::Text(t) => Some((
                t.galley.text().to_string(),
                t.galley.rect.translate(t.pos.to_vec2()),
            )),
            _ => None,
        })
        .collect()
}

fn has(out: &FullOutput, s: &str) -> bool {
    texts(out).iter().any(|(t, _)| t == s)
}

fn click(at: Pos2) -> Vec<Event> {
    let b = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    vec![Event::PointerMoved(at), b(true), b(false)]
}

fn beat(index: i64, time: f64) -> Beat {
    Beat {
        index,
        time,
        quality: 1.0,
    }
}

fn row(label: &'static str, traces: Vec<Vec<f32>>, marks: Vec<Mark>) -> Row {
    Row {
        label,
        traces,
        color: Color32::from_rgb(200, 60, 40),
        marks,
    }
}

fn mark(name: &'static str, ms: f64) -> Mark {
    Mark {
        ms,
        color: Color32::from_rgb(40, 120, 200),
        name,
        moment: None,
    }
}

/// Each span is the time its button names, set around the drop; the
/// grid draws about ten lines across it; the Profile's edges are marked
/// up to 50 ms; only 400 ms runs the beats in one row.
#[test]
fn each_span_is_its_named_time_with_about_ten_grid_lines() {
    for (z, ms) in SPANS.into_iter().zip([5.0, 10.0, 20.0, 50.0, 200.0, 400.0]) {
        let (before, after) = z.window(0.125);
        assert!(before > 0.0 && after > 0.0, "{z:?} holds the drop");
        assert!((z.span_ms(0.125) - ms).abs() < 1e-9, "{z:?}");
    }
    // The unlock comes up to about 12 ms before the drop: 20 ms shows it.
    assert!(Zoom::Ms20.window(0.125).0 >= 0.012);
    // One Beat at every common beat rate, and never under 50 ms.
    for bph in [18000.0, 19800.0, 21600.0, 25200.0, 28800.0, 36000.0] {
        let beat_s = 3600.0 / bph;
        assert!((Zoom::Beat.span_ms(beat_s) - beat_s * 1000.0).abs() < 1e-9);
    }
    assert!((Zoom::Beat.span_ms(0.01) - 50.0).abs() < 1e-9);
    let steps: Vec<f64> = SPANS.iter().map(|z| z.step_ms(0.125)).collect();
    assert_eq!(steps, [0.5, 1.0, 2.0, 5.0, 20.0, 50.0]);
    for z in SPANS.into_iter().chain([Zoom::Beat]) {
        for beat_s in [0.1, 0.125, 0.2] {
            let lines = z.span_ms(beat_s) / z.step_ms(beat_s);
            assert!(
                (4.0..=10.0 + 1e-9).contains(&lines),
                "{z:?} {beat_s}: {lines}"
            );
        }
        assert_eq!(z.marks(), z.span_ms(0.125) <= 50.0, "{z:?}");
        assert_eq!(z.split(), z != Zoom::Ms400, "{z:?}");
    }
    let v: View = serde_json::from_str("{}").unwrap();
    assert_eq!(v, View::default());
    assert_eq!((v.zoom, v.ring), (Zoom::Ms20, 8));
}

/// Beats asks for one at least, and none whose window isn't whole yet.
#[test]
fn pick_takes_one_beat_at_least_and_none_unfinished() {
    let beats: Vec<Beat> = (0..10).map(|i| beat(i, 0.125 * i as f64)).collect();
    let p = pick(&beats, Zoom::Ms20, 0.125, 0, 2.0, None);
    assert_eq!(p.iter().map(|b| b.index).collect::<Vec<_>>(), [9]);
    // The last beat's 20 ms window ends at 1.131 s.
    let p = pick(&beats, Zoom::Ms20, 0.125, 1, 1.130, None);
    assert_eq!(p[0].index, 8);
    let p = pick(&beats, Zoom::Ms20, 0.125, 16, 1.131, Some(false));
    assert_eq!(
        p.iter().map(|b| b.index).collect::<Vec<_>>(),
        [1, 3, 5, 7, 9]
    );
    assert!(pick(&beats, Zoom::Ms20, 0.125, 8, 0.0, None).is_empty());
    assert!(pick(&[], Zoom::Ms20, 0.125, 8, 9.0, None).is_empty());
}

/// A trace keeps the watch's sound and drops the hum under it.
#[test]
fn a_trace_drops_hum_and_keeps_the_watch() {
    let fs = 48000.0;
    let hum = |i: usize| 0.5 + 0.8 * (std::f32::consts::TAU * 50.0 * i as f32 / fs as f32).sin();
    let tick = |i: usize| 0.1 * (std::f32::consts::TAU * 4000.0 * i as f32 / fs as f32).sin();
    let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
    let only_hum: Vec<f32> = (0..48000).map(hum).collect();
    let both: Vec<f32> = (0..48000).map(|i| hum(i) + tick(i)).collect();
    let b = beat(0, 0.5);
    let h = trace(&only_hum, fs, 0.0, &b, Zoom::Ms20, 0.125).unwrap();
    let w = trace(&both, fs, 0.0, &b, Zoom::Ms20, 0.125).unwrap();
    assert!(rms(&h) < 0.01, "hum left: {}", rms(&h));
    let kept = rms(&w) / (0.1 / 2f32.sqrt());
    assert!((0.9..1.1).contains(&kept), "{kept}");
    // The audio may start later than 0: the window is found in it.
    let late = trace(&both[24000..], fs, 0.5, &beat(0, 0.75), Zoom::Ms20, 0.125).unwrap();
    assert_eq!(late.len(), w.len());
}

/// Two rows on one time axis in ms from the drop, each named, and a
/// row with no beats says so.
#[test]
fn the_rows_share_a_time_axis_from_the_drop() {
    let ctx = themed();
    let rows = [
        row("Tick", vec![vec![0.1; 960]], Vec::new()),
        row("Tock", Vec::new(), Vec::new()),
    ];
    let out = frame(&ctx, (600.0, 300.0), Vec::new(), |ui| {
        draw(ui, &rows, Zoom::Ms20, 0.125, 48000.0)
    });
    let t = texts(&out);
    let at = |s: &str| {
        t.iter()
            .find(|(x, _)| x == s)
            .unwrap_or_else(|| panic!("{s}: {t:?}"))
            .1
    };
    assert!(
        at("Tick").center().y < at("Tock").center().y,
        "ticks above tocks"
    );
    assert!(has(&out, "No beats yet"));
    // 14 ms before the drop to 6 after, every 2 ms; the unit at the end.
    let labels: Vec<&str> = t
        .iter()
        .filter(|(_, r)| r.top() > at("Tock").bottom())
        .map(|(s, _)| s.as_str())
        .collect();
    assert_eq!(labels.first(), Some(&"−14"), "{labels:?}");
    assert!(labels.contains(&"0") && labels.contains(&"4"), "{labels:?}");
    assert_eq!(labels.last(), Some(&"ms"), "{labels:?}");
    // The drop's 0 sits 14/20 of the way across the plot.
    let x0 = at("0").center().x;
    let (left, right) = (at("−14").center().x, at("ms").right());
    let frac = (x0 - left) / (right - left);
    assert!((frac - 0.7).abs() < 0.02, "{frac}");
}

/// The newest beat is drawn strongest and older ones fainter; a trace
/// with more samples than the row is wide is drawn column by column.
#[test]
fn the_newest_beat_is_drawn_strongest() {
    let ctx = themed();
    // 20 ms at 8 kHz: 160 samples, fewer than the row's width.
    let wave = |a: f32| {
        (0..160)
            .map(|i| a * (i as f32 * 0.4).sin())
            .collect::<Vec<f32>>()
    };
    let rows = [row(
        "Tick",
        vec![wave(0.2), wave(0.5), wave(1.0)],
        Vec::new(),
    )];
    let out = frame(&ctx, (600.0, 200.0), Vec::new(), |ui| {
        draw(ui, &rows, Zoom::Ms20, 0.125, 8000.0)
    });
    let alphas: Vec<u8> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            Shape::Path(p) if p.points.len() == 160 => match p.stroke.color {
                egui::epaint::ColorMode::Solid(c) => Some(c.a()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(alphas.len(), 3, "one line per beat");
    assert!(alphas[0] < alphas[1] && alphas[1] < alphas[2], "{alphas:?}");
    assert_eq!(alphas[2], 255, "the newest at full strength");
    // At 192 kHz the same 20 ms is 3840 samples: one filled column per
    // column of the row.
    let dense = [row("Tick", vec![vec![0.3; 3840]], Vec::new())];
    let out = frame(&ctx, (600.0, 200.0), Vec::new(), |ui| {
        draw(ui, &dense, Zoom::Ms20, 0.125, 192_000.0)
    });
    let strokes = out
        .shapes
        .iter()
        .filter(|c| matches!(&c.shape, Shape::Rect(r) if r.fill == dense[0].color))
        .count();
    assert!((500..=560).contains(&strokes), "{strokes}");
}

/// The Profile's edges are marked on each row and named: above it in a
/// pane with room, inside it along its top in a short one. An edge
/// outside the span isn't drawn.
#[test]
fn the_edges_are_named_above_a_row_or_inside_a_short_one() {
    let ctx = themed();
    let rows = |marks: fn() -> Vec<Mark>| {
        [
            row("Tick", vec![vec![0.0; 960]], marks()),
            row("Tock", vec![vec![0.0; 960]], marks()),
        ]
    };
    let edges = || vec![mark("Unlock", -11.0), mark("Drop", -0.3), mark("Far", 30.0)];
    let place = |h: f32| {
        let rs = rows(edges);
        let mut out = frame(&ctx, (700.0, h), Vec::new(), |ui| {
            draw(ui, &rs, Zoom::Ms20, 0.125, 48000.0)
        });
        for _ in 0..2 {
            out = frame(&ctx, (700.0, h), Vec::new(), |ui| {
                draw(ui, &rs, Zoom::Ms20, 0.125, 48000.0)
            });
        }
        texts(&out)
    };
    for h in [400.0, 200.0] {
        let t = place(h);
        let names = |n: &str| t.iter().filter(|(s, _)| s == n).count();
        assert_eq!((names("Unlock"), names("Drop")), (2, 2), "h {h}: {t:?}");
        assert_eq!(names("Far"), 0, "outside the span: {t:?}");
        let tick = t.iter().find(|(s, _)| s == "Tick").unwrap().1;
        let unlock = t.iter().find(|(s, _)| s == "Unlock").unwrap().1;
        assert!(unlock.left() < t.iter().find(|(s, _)| s == "Drop").unwrap().1.left());
        // The rows are about 160 points tall at 400, 80 at 200.
        let up = tick.center().y - unlock.center().y;
        if h > 300.0 {
            assert!(up > 80.0, "in the lane above the row: {up}");
            // The rows make room for their lanes: the top row's names
            // are in the pane, not over its edge.
            assert!(unlock.top() >= 0.0, "{unlock:?}");
        } else {
            assert!(
                (0.0..40.0).contains(&up),
                "inside the row, along its top: {up}"
            );
        }
    }
}

/// The Span and Beats controls set the view; One Beat is its own
/// button.
#[test]
fn span_and_beats_set_the_view() {
    let ctx = themed();
    let mut view = View::default();
    let mut out = frame(&ctx, (900.0, 120.0), Vec::new(), |ui| {
        ui.horizontal(|ui| settings(ui, &mut view));
    });
    for (press, want) in [
        ("400", (Zoom::Ms400, 8)),
        ("One Beat", (Zoom::Beat, 8)),
        ("16", (Zoom::Beat, 16)),
        ("5", (Zoom::Ms5, 16)),
        ("1", (Zoom::Ms5, 1)),
    ] {
        let at = texts(&out)
            .into_iter()
            .find(|(t, _)| t == press)
            .unwrap_or_else(|| panic!("{press}"))
            .1
            .center();
        frame(&ctx, (900.0, 120.0), click(at), |ui| {
            ui.horizontal(|ui| settings(ui, &mut view));
        });
        assert_eq!((view.zoom, view.ring), want, "{press}");
        out = frame(&ctx, (900.0, 120.0), Vec::new(), |ui| {
            ui.horizontal(|ui| settings(ui, &mut view));
        });
    }
    assert!(has(&out, "Span") && has(&out, "Beats") && has(&out, "ms"));
}

/// One frame of `draw` in a 600 × 400 panel, and the rect it was given.
fn drawn(ctx: &egui::Context, rows: &[Row], z: Zoom, fs: f64) -> (FullOutput, Rect) {
    let mut given = Rect::NOTHING;
    let out = frame(ctx, (600.0, 400.0), Vec::new(), |ui| {
        given = Rect::from_min_size(ui.cursor().min, ui.available_size());
        draw(ui, rows, z, 0.125, fs)
    });
    (out, given)
}

/// Each span puts the drop where its window says: the unlock up to about
/// 12 ms before it at 20 ms, most of 50 ms after it for what rings on,
/// and nearly all of 200 and 400 ms after it for the quiet and the next
/// beats. The axis starts at the window's first grid line and its 0 sits
/// that share of the way across the plot.
#[test]
fn each_span_puts_the_drop_where_its_window_says() {
    let ctx = themed();
    let rows = [row("Tick", vec![vec![0.1; 100]], Vec::new())];
    for (z, before_ms, first) in [
        (Zoom::Ms5, 3.5, "−3.5"),
        (Zoom::Ms10, 8.0, "−8"),
        (Zoom::Ms20, 14.0, "−14"),
        (Zoom::Ms50, 15.0, "−15"),
        (Zoom::Ms200, 20.0, "−20"),
        (Zoom::Ms400, 20.0, "0"),
    ] {
        let (out, given) = drawn(&ctx, &rows, z, 48000.0);
        let t = texts(&out);
        let axis: Vec<&(String, Rect)> = t
            .iter()
            .filter(|(_, r)| r.top() > given.bottom() - 18.0)
            .collect();
        assert_eq!(axis.first().map(|a| a.0.as_str()), Some(first), "{z:?}");
        let x0 = axis
            .iter()
            .find(|(s, _)| s == "0" || s == "0.0")
            .unwrap_or_else(|| panic!("{z:?}: {axis:?}"))
            .1
            .center()
            .x;
        let (left, width) = (given.left() + 44.0, given.width() - 48.0);
        let want = before_ms / z.span_ms(0.125);
        let frac = f64::from((x0 - left) / width);
        assert!((frac - want).abs() < 0.005, "{z:?}: {frac} for {want}");
    }
}

/// Past 50 ms no edges are marked, so the rows keep no names lane above
/// them: two rows fill the plot, 6 points apart. A trace at full size
/// fills 96% of its row's height, clear of the row's edges.
#[test]
fn rows_without_edges_fill_the_plot_and_traces_their_rows() {
    let ctx = themed();
    // 200 ms at 1 kHz: 200 samples, one line point each.
    let wave: Vec<f32> = (0..200)
        .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
        .collect();
    let rows = [
        row("Tick", vec![wave.clone()], Vec::new()),
        row("Tock", vec![wave], Vec::new()),
    ];
    for z in [Zoom::Ms200, Zoom::Ms400] {
        let (out, given) = drawn(&ctx, &rows, z, 1000.0);
        let t = texts(&out);
        let at = |s: &str| t.iter().find(|(x, _)| x == s).unwrap().1.center().y;
        let (top, plot_h) = (given.top(), given.height() - 18.0);
        let h = (plot_h - 6.0) / 2.0;
        assert!((at("Tick") - (top + h / 2.0)).abs() < 0.6, "{z:?}");
        assert!((at("Tock") - (top + 1.5 * h + 6.0)).abs() < 0.6, "{z:?}");
        let spans: Vec<f32> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                Shape::Path(p) if p.points.len() == 200 => {
                    let ys = p.points.iter().map(|q| q.y);
                    let lo = ys.clone().fold(f32::MAX, f32::min);
                    Some(ys.fold(f32::MIN, f32::max) - lo)
                }
                _ => None,
            })
            .collect();
        assert_eq!(spans.len(), 2, "{z:?}");
        for s in spans {
            assert!((s / h - 0.96).abs() < 0.005, "{z:?}: {}", s / h);
        }
    }
}

/// An edge before the span's start isn't drawn, as one past its end
/// isn't: neither its line nor its name.
#[test]
fn an_edge_before_the_span_is_not_drawn() {
    let ctx = themed();
    let ink = Color32::from_rgb(40, 120, 200);
    let rows = [row(
        "Tick",
        vec![vec![0.0; 960]],
        vec![mark("Early", -16.0), mark("Drop", -0.3)],
    )];
    let mut out = drawn(&ctx, &rows, Zoom::Ms20, 48000.0).0;
    for _ in 0..2 {
        out = drawn(&ctx, &rows, Zoom::Ms20, 48000.0).0;
    }
    assert!(has(&out, "Drop"));
    assert!(!has(&out, "Early"), "{:?}", texts(&out));
    let lines = out
        .shapes
        .iter()
        .filter(|c| matches!(&c.shape, Shape::LineSegment { stroke, .. } if stroke.color == ink))
        .count();
    assert_eq!(lines, 1, "only the drop's line");
}

/// A trace with more samples than the row has columns draws each column
/// from its lowest to its highest. Where the sound is still, a column's
/// span is under a point, and drawn as a line piece it was rounded to
/// the pixel at both ends into nothing: the quiet stretches between the
/// sounds showed no trace when the Span was long. Each column keeps a
/// point of ink at least.
#[test]
fn a_still_stretch_of_a_long_trace_still_shows() {
    const INK: egui::Color32 = egui::Color32::from_rgb(201, 3, 199);
    let ctx = egui::Context::default();
    let r = Rect::from_min_size(Pos2::new(10.0, 10.0), egui::vec2(400.0, 100.0));
    // Barely moving: under a point either way of the middle.
    let t: Vec<f32> = (0..4000).map(|j| (j % 7) as f32 * 1e-4).collect();
    let out = frame(&ctx, (500.0, 200.0), Vec::new(), |ui| {
        draw_trace(ui.painter(), &t, r, INK, (60.0, 1.0, 40.0), |j| {
            r.left() + j as f32 * r.width() / t.len() as f32
        });
    });
    let mut cols = std::collections::BTreeSet::new();
    for p in ctx.tessellate(out.shapes, out.pixels_per_point) {
        let egui::epaint::Primitive::Mesh(m) = p.primitive else {
            continue;
        };
        for tri in m.indices.chunks(3) {
            let v: Vec<_> = tri.iter().map(|&i| m.vertices[i as usize]).collect();
            let area = ((v[1].pos - v[0].pos).x * (v[2].pos - v[0].pos).y
                - (v[1].pos - v[0].pos).y * (v[2].pos - v[0].pos).x)
                .abs();
            if area > 1e-3 && v.iter().any(|v| v.color == INK) {
                let x = v.iter().map(|v| v.pos.x).sum::<f32>() / 3.0;
                cols.insert(x.floor() as i32);
            }
        }
    }
    let missing: Vec<i32> = (11..409).filter(|x| !cols.contains(x)).collect();
    assert!(missing.is_empty(), "columns with no ink: {missing:?}");
}
