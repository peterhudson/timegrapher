//! The Scope pane: the sound of the latest beats as it came in, at the
//! three spans a Witschi scope offers and a few more. 20 ms shows one
//! sound in detail, from the unlock to the drop; 5 and 10 ms the drop
//! closer still, 50 ms the sound and what rings on after it. One Beat
//! runs from just before one beat to just before the next, at the
//! watch's beat rate. 200 ms shows the whole beat and the quiet after it.
//! 400 ms shows the tick, the tock and the next tick in a row.
//! The latest beats are drawn over each other, the newest strongest, so a
//! sound that changes from beat to beat shows as a blur.

use crate::marks::{self, Rank};
use crate::theme;
use eframe::egui::{self, Color32, RichText, Vec2};
use timegrapher_core::beats::Beat;
use timegrapher_core::filter::highpass4;

/// How much of each beat is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Zoom {
    Ms5,
    Ms10,
    Ms20,
    Ms50,
    /// One beat period at the watch's beat rate.
    Beat,
    Ms200,
    Ms400,
}

impl Zoom {
    /// The window around a beat's reference point (the drop), seconds:
    /// before it and after it. `beat_s` is the time between beats, which
    /// only One Beat uses.
    pub fn window(self, beat_s: f64) -> (f64, f64) {
        match self {
            Zoom::Ms5 => (0.0035, 0.0015),
            Zoom::Ms10 => (0.008, 0.002),
            // The unlock comes up to about 12 ms before the drop.
            Zoom::Ms20 => (0.014, 0.006),
            Zoom::Ms50 => (0.015, 0.035),
            Zoom::Beat => (0.015, beat_s.max(0.05) - 0.015),
            Zoom::Ms200 => (0.020, 0.180),
            Zoom::Ms400 => (0.020, 0.380),
        }
    }

    pub fn span_ms(self, beat_s: f64) -> f64 {
        let (a, b) = self.window(beat_s);
        (a + b) * 1000.0
    }

    /// Grid step, milliseconds: about ten lines across.
    fn step_ms(self, beat_s: f64) -> f64 {
        match self.span_ms(beat_s) {
            s if s <= 6.0 => 0.5,
            s if s <= 12.0 => 1.0,
            s if s <= 25.0 => 2.0,
            s if s <= 60.0 => 5.0,
            s if s <= 250.0 => 20.0,
            _ => 50.0,
        }
    }

    /// Short enough for the profile's Unlock and Drop marks to read.
    pub fn marks(self) -> bool {
        matches!(self, Zoom::Ms5 | Zoom::Ms10 | Zoom::Ms20 | Zoom::Ms50)
    }

    /// Ticks and tocks each in a row of their own, or one row that runs
    /// from a tick through the beats after it.
    pub fn split(self) -> bool {
        self != Zoom::Ms400
    }
}

/// How the pane is drawn: set in the pane itself.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct View {
    pub zoom: Zoom,
    /// The latest beats drawn over each other, per row.
    pub ring: usize,
}

impl Default for View {
    fn default() -> Self {
        View {
            zoom: Zoom::Ms20,
            ring: 8,
        }
    }
}

/// The beats a row draws, oldest first: the latest `ring` whose whole
/// window ends before `end` (on one side when `even` is given).
pub fn pick(
    beats: &[Beat],
    zoom: Zoom,
    beat_s: f64,
    ring: usize,
    end: f64,
    even: Option<bool>,
) -> Vec<Beat> {
    let (_, after) = zoom.window(beat_s);
    let mut out: Vec<Beat> = beats
        .iter()
        .rev()
        .filter(|b| b.time + after <= end)
        .filter(|b| even.is_none_or(|e| (b.index.rem_euclid(2) == 0) == e))
        .take(ring.max(1))
        .copied()
        .collect();
    out.reverse();
    out
}

/// One beat's sound, high-passed so hum and rumble don't hide it, from the
/// start of its window: `audio` starts at `audio_t0` seconds.
pub fn trace(
    audio: &[f32],
    fs: f64,
    audio_t0: f64,
    beat: &Beat,
    zoom: Zoom,
    beat_s: f64,
) -> Option<Vec<f32>> {
    let (before, after) = zoom.window(beat_s);
    // A little more first, for the filter to settle on.
    let lead = 0.010;
    let i0 = ((beat.time - before - lead - audio_t0) * fs).round();
    let i1 = ((beat.time + after - audio_t0) * fs).round();
    if i0 < 0.0 || i1 as usize > audio.len() || i1 <= i0 {
        return None;
    }
    let mut x = audio[i0 as usize..i1 as usize].to_vec();
    highpass4(&mut x, fs, 1000.0);
    Some(x.split_off((lead * fs).round() as usize))
}

/// What a row draws: each trace with its colour, oldest first.
pub struct Row {
    pub label: &'static str,
    pub traces: Vec<Vec<f32>>,
    pub color: Color32,
    /// Where the profile found the unlock and the drop, for the spans up
    /// to 50 ms.
    pub marks: Vec<Mark>,
}

/// An edge the profile found, marked on a row: ms from the reference
/// point, its line's colour and its name.
pub struct Mark {
    pub ms: f64,
    pub color: Color32,
    pub name: &'static str,
}

impl Mark {
    /// What its hover says: "Drop: −0.27 ms", as the Profile words it.
    fn hover(&self) -> String {
        format!("{}: {}", self.name, crate::profiles::ms_text(self.ms))
    }
}

/// Draw the rows one above the other, sharing a time axis.
pub fn draw(ui: &mut egui::Ui, rows: &[Row], zoom: Zoom, beat_s: f64, fs: f64) {
    let pal = theme::pal(ui);
    let avail = ui.available_size();
    let axis_h = 18.0;
    let (rect, _) = ui.allocate_exact_size(avail, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let plot = egui::Rect::from_min_max(
        rect.min + Vec2::new(44.0, 0.0),
        rect.max - Vec2::new(4.0, axis_h),
    );
    let (before, _) = zoom.window(beat_s);
    let span = zoom.span_ms(beat_s);
    let x_of = |ms: f64| plot.left() + ((ms + before * 1000.0) / span) as f32 * plot.width();
    // Grid and the time axis, in ms from the drop.
    let step = zoom.step_ms(beat_s);
    let mut ms = -(before * 1000.0 / step).floor() * step + 0.0;
    while ms <= span - before * 1000.0 + 1e-9 {
        let x = x_of(ms);
        painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            egui::Stroke::new(1.0_f32, pal.separator),
        );
        // Room left at the right for the unit; no "-0".
        if x < plot.right() - 22.0 {
            painter.text(
                egui::pos2(x, plot.bottom() + 3.0),
                egui::Align2::CENTER_TOP,
                if step < 1.0 {
                    format!("{:.1}", ms + 0.0)
                } else {
                    format!("{:.0}", ms + 0.0)
                },
                egui::FontId::proportional(11.0),
                pal.text_tertiary,
            );
        }
        ms += step;
    }
    painter.text(
        egui::pos2(plot.right(), plot.bottom() + 3.0),
        egui::Align2::RIGHT_TOP,
        "ms",
        egui::FontId::proportional(11.0),
        pal.text_tertiary,
    );
    let n = rows.len().max(1);
    let gap = 6.0;
    // With the marks on, each row keeps a lane of one row of names above
    // it, so the rows don't jump; a row too short for that takes its names
    // inside, along its top, and the lane gives way.
    let names_id = ui.id().with("scope names");
    let lane_h = marks::ROW_H + marks::LANE_GAP;
    let with_lane = (plot.height() - gap * (n as f32 - 1.0) - lane_h * n as f32) / n as f32;
    let inside = zoom.marks() && marks::short(ui, names_id.with("short"), with_lane);
    let lane = if zoom.marks() && !inside { lane_h } else { 0.0 };
    let h = (plot.height() - gap * (n as f32 - 1.0) - lane * n as f32) / n as f32;
    for (k, row) in rows.iter().enumerate() {
        let r = egui::Rect::from_min_size(
            egui::pos2(plot.left(), plot.top() + k as f32 * (h + gap + lane) + lane),
            Vec2::new(plot.width(), h),
        );
        painter.text(
            egui::pos2(rect.left(), r.center().y),
            egui::Align2::LEFT_CENTER,
            row.label,
            theme::semibold(12.5),
            row.color,
        );
        let mid = r.center().y;
        painter.line_segment(
            [egui::pos2(r.left(), mid), egui::pos2(r.right(), mid)],
            egui::Stroke::new(1.0_f32, pal.separator),
        );
        // The edges, solid and under the trace, as on the Profile.
        let edges: Vec<(f32, &Mark)> = row
            .marks
            .iter()
            .map(|m| (x_of(m.ms), m))
            .filter(|(x, _)| *x >= r.left() && *x <= r.right())
            .collect();
        for (x, m) in &edges {
            painter.line_segment(
                [egui::pos2(*x, r.top()), egui::pos2(*x, r.bottom())],
                egui::Stroke::new(1.0_f32, m.color),
            );
        }
        let top = row
            .traces
            .iter()
            .flat_map(|t| t.iter())
            .fold(0.0f32, |m, &v| m.max(v.abs()))
            .max(1e-6);
        let half = r.height() * 0.48;
        let cols = r.width().max(1.0) as usize;
        let count = row.traces.len();
        // Each column's highest point over every trace, for the names
        // inside a short row to keep clear of.
        let mut tops: Vec<(f32, f32)> = Vec::new();
        if inside {
            for col in 0..cols {
                let x = r.left() + col as f32 + 0.5;
                let hi = row
                    .traces
                    .iter()
                    .filter_map(|t| {
                        let per = t.len() as f32 / cols as f32;
                        let j0 = (col as f32 * per) as usize;
                        let j1 = (((col + 1) as f32 * per).ceil() as usize).min(t.len());
                        t.get(j0..j1)?.iter().copied().reduce(f32::max)
                    })
                    .fold(f32::MIN, f32::max);
                if hi > f32::MIN {
                    let y = mid - (hi / top).clamp(-1.0, 1.0) * half;
                    tops.push((x, y));
                }
            }
        }
        if inside {
            // Over the lines, under the trace.
            inside_names(ui, &painter, r, &edges, &tops, names_id.with(k));
        }
        for (i, t) in row.traces.iter().enumerate() {
            // The newest strongest; the older ones fade.
            let a = if i + 1 == count {
                1.0
            } else {
                0.15 + 0.45 * (i as f32 + 1.0) / count as f32
            };
            let c = row.color.gamma_multiply(a);
            let per = (t.len() as f32 / cols as f32).max(1.0);
            let y = |v: f32| mid - (v / top).clamp(-1.0, 1.0) * half;
            if per <= 1.5 {
                let pts: Vec<egui::Pos2> = t
                    .iter()
                    .enumerate()
                    .map(|(j, &v)| {
                        let ms = j as f64 / fs * 1000.0 - before * 1000.0;
                        egui::pos2(x_of(ms), y(v))
                    })
                    .collect();
                painter.add(egui::Shape::line(pts, egui::Stroke::new(1.2_f32, c)));
            } else {
                // Too many samples for the width: each column from its
                // lowest to its highest.
                for col in 0..cols {
                    let j0 = (col as f32 * per) as usize;
                    let j1 = (((col + 1) as f32 * per) as usize).min(t.len());
                    if j0 >= j1 {
                        continue;
                    }
                    let (lo, hi) = t[j0..j1]
                        .iter()
                        .fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
                    let x = r.left() + col as f32 + 0.5;
                    painter.line_segment(
                        [egui::pos2(x, y(hi)), egui::pos2(x, y(lo) + 0.5)],
                        egui::Stroke::new(1.0_f32, c),
                    );
                }
            }
        }
        if row.traces.is_empty() {
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                "No beats yet",
                egui::FontId::proportional(12.0),
                pal.text_tertiary,
            );
        }
        if !inside {
            let named: Vec<marks::Named> = edges
                .iter()
                .map(|(x, m)| marks::Named {
                    x: *x,
                    name: m.name.to_string(),
                    rank: Rank::Strong,
                    ink: m.color,
                    hover: m.hover(),
                })
                .collect();
            marks::lane_above_one(ui, r, &named, names_id.with(k));
        }
    }
}

/// The edges' names inside a short row, along its top on boxes, each
/// where [`marks::inside`] puts it clear of the newest trace (`tops`, each
/// column's screen x and highest point), or left to its line's hover.
/// Drawn before the trace, so it runs over the boxes.
fn inside_names(
    ui: &egui::Ui,
    painter: &egui::Painter,
    r: egui::Rect,
    edges: &[(f32, &Mark)],
    tops: &[(f32, f32)],
    id: egui::Id,
) {
    let galleys: Vec<_> = edges
        .iter()
        .map(|(_, m)| marks::galley(ui, m.name, Rank::Strong, false))
        .collect();
    let spans: Vec<(f32, f32, Rank)> = edges
        .iter()
        .zip(&galleys)
        .map(|((x, _), g)| (*x, g.size().x, Rank::Strong))
        .collect();
    let row_foot = r.top() + marks::INSIDE_TOP + marks::ROW_H;
    let clear = |a: f32, b: f32| {
        tops.iter()
            .filter(|(x, _)| (a..=b).contains(x))
            .all(|(_, y)| *y > row_foot + 1.0)
    };
    let at = marks::inside(
        &spans,
        r.left(),
        r.right(),
        (f32::NEG_INFINITY, f32::NEG_INFINITY),
        &clear,
    );
    let fill = ui.visuals().extreme_bg_color.gamma_multiply(0.8);
    for (((x, m), g), a) in edges.iter().zip(galleys).zip(at) {
        let line = egui::Rect::from_x_y_ranges(x - 3.0..=x + 3.0, r.top()..=r.bottom());
        let hit = match a {
            Some(a) => {
                let rect = marks::inside_rect(r, a, g.size().x);
                painter.extend(marks::inside_name(rect, a, g, fill));
                line.union(rect)
            }
            None => line,
        };
        marks::hover(ui, id.with(("edge", m.name)), hit, m.hover());
    }
}

/// The span and ring controls, in a row above the plot.
pub fn settings(ui: &mut egui::Ui, view: &mut View) {
    theme::tool_field(ui, "Span", "How much time each trace covers", |ui| {
        theme::segmented(
            ui,
            &mut view.zoom,
            &[
                (
                    Zoom::Ms5,
                    "5",
                    "The drop up close, 3.5 ms before it to 1.5 ms after, ticks above tocks",
                ),
                (
                    Zoom::Ms10,
                    "10",
                    "The drop and the sound just before it, ticks above tocks",
                ),
                (
                    Zoom::Ms20,
                    "20",
                    "One sound in detail, from the unlock to the drop, ticks above tocks",
                ),
                (
                    Zoom::Ms50,
                    "50",
                    "One sound and what rings on after it, ticks above tocks",
                ),
                (
                    Zoom::Ms200,
                    "200",
                    "The whole beat and the quiet after it, ticks above tocks",
                ),
                (
                    Zoom::Ms400,
                    "400",
                    "A tick, the tock after it and the next tick, in one row",
                ),
            ],
        );
        ui.label(RichText::new("ms").size(12.5));
        let beat = "From just before one beat to just before the next, at the watch's beat \
                    rate, ticks above tocks";
        if theme::in_toolbar(ui) {
            let on = view.zoom == Zoom::Beat;
            if theme::icon_toggle(ui, theme::Icon::Drum, on, theme::pal(ui).accent)
                .on_hover_text(format!("One Beat: {beat}"))
                .clicked()
            {
                view.zoom = Zoom::Beat;
            }
        } else {
            theme::segmented(ui, &mut view.zoom, &[(Zoom::Beat, "One Beat", beat)]);
        }
    });
    theme::note_icon(ui, theme::Icon::Drum, "Span: One Beat");
    theme::tool_field(
        ui,
        "Beats",
        "How many of the latest beats are drawn over each other",
        |ui| {
            theme::segmented(
                ui,
                &mut view.ring,
                &[
                    (1, "1", "Only the latest beat"),
                    (4, "4", "The latest four, older ones fainter"),
                    (8, "8", "The latest eight, older ones fainter"),
                    (16, "16", "The latest sixteen, older ones fainter"),
                ],
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat(index: i64, time: f64) -> Beat {
        Beat {
            index,
            time,
            quality: 1.0,
        }
    }

    #[test]
    fn picks_the_latest_whole_beats_on_one_side() {
        let beats: Vec<Beat> = (0..20).map(|i| beat(i, 0.125 * i as f64)).collect();
        // A beat at 2.375 s needs until 2.381 s for its 20 ms window.
        let p = pick(&beats, Zoom::Ms20, 0.125, 3, 2.38, Some(true));
        assert_eq!(p.iter().map(|b| b.index).collect::<Vec<_>>(), [14, 16, 18]);
        let p = pick(&beats, Zoom::Ms400, 0.125, 2, 2.38, None);
        assert_eq!(p.iter().map(|b| b.index).collect::<Vec<_>>(), [15, 16]);
    }

    #[test]
    fn a_trace_starts_at_its_window() {
        let fs = 48000.0;
        let audio: Vec<f32> = (0..48000).map(|i| ((i as f32) * 0.3).sin()).collect();
        let t = trace(&audio, fs, 0.0, &beat(0, 0.5), Zoom::Ms20, 0.125).unwrap();
        assert_eq!(t.len(), (0.020 * fs) as usize);
        let t = trace(&audio, fs, 0.0, &beat(0, 0.5), Zoom::Ms5, 0.125).unwrap();
        assert_eq!(t.len(), (0.005 * fs) as usize);
        assert!(trace(&audio, fs, 0.0, &beat(0, 0.005), Zoom::Ms20, 0.125).is_none());
        assert!(trace(&audio, fs, 0.0, &beat(0, 0.99), Zoom::Ms200, 0.125).is_none());
    }

    #[test]
    fn one_beat_spans_the_beat_period() {
        // 28,800 bph beats every 125 ms, 18,000 bph every 200 ms.
        assert!((Zoom::Beat.span_ms(0.125) - 125.0).abs() < 1e-9);
        assert!((Zoom::Beat.span_ms(0.2) - 200.0).abs() < 1e-9);
        for z in [
            Zoom::Ms5,
            Zoom::Ms10,
            Zoom::Ms20,
            Zoom::Ms50,
            Zoom::Ms200,
            Zoom::Ms400,
        ] {
            assert_eq!(z.span_ms(0.125), z.span_ms(0.2));
        }
        assert!((Zoom::Ms5.span_ms(0.125) - 5.0).abs() < 1e-9);
        assert_eq!(Zoom::Ms5.step_ms(0.125), 0.5);
    }
}
