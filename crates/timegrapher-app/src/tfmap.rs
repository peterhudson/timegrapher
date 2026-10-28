//! The STFT Map and Wavelet Map panes' maps: a series' size at each period
//! (up, on a log scale, longest at the top) and time of the take
//! (across), from core `timefreq`, as a heatmap in viridis. A cell nothing
//! was read for (a gap, a window with a missing reading, the wavelet's
//! cone at a stretch's ends) is left empty: the card shows through.

use crate::theme;
use eframe::egui::{self, Color32, RichText, Vec2};
use egui_plot::{LineStyle, Plot, PlotImage, PlotPoint, Text};
use timegrapher_core::periodicity::Wheel;
use timegrapher_core::timefreq::TimeFrequency;

pub use timegrapher_core::timefreq::Scale;

/// Viridis at `t`, 0 to 1 (clamped), from the core's table.
pub fn viridis(t: f32) -> Color32 {
    let c = timegrapher_core::palette::viridis(t);
    Color32::from_rgb(c.r, c.g, c.b)
}

/// The map's cells, one column per slot of time `dt` from the first
/// column's: `(t0, dt, columns)`. Columns that aren't there (a window
/// left out) leave their slot empty.
fn slots(m: &TimeFrequency) -> Option<(f64, f64, usize)> {
    let t = &m.t_s;
    let t0 = *t.first()?;
    let dt = m.hop_s.unwrap_or_else(|| {
        let mut d: Vec<f64> = t
            .windows(2)
            .map(|w| w[1] - w[0])
            .filter(|d| *d > 0.0)
            .collect();
        d.sort_by(f64::total_cmp);
        d.first().copied().unwrap_or(1.0)
    });
    if dt.is_nan() || dt <= 0.0 {
        return None;
    }
    let last = *t.last()?;
    Some((t0, dt, ((last - t0) / dt).round() as usize + 1))
}

/// The map's picture, a pixel a cell, the longest period on the top row;
/// an empty cell is clear.
pub fn image(m: &TimeFrequency, scale: Scale) -> Option<egui::ColorImage> {
    let (t0, dt, w) = slots(m)?;
    let h = m.period_s.len();
    if h == 0 || w == 0 {
        return None;
    }
    let mut px = vec![Color32::TRANSPARENT; w * h];
    for (t, col) in m.t_s.iter().zip(&m.values) {
        let x = ((t - t0) / dt).round() as usize;
        if x >= w {
            continue;
        }
        for (y, v) in col.iter().enumerate().take(h) {
            if v.is_finite() {
                px[y * w + x] = viridis(scale.at(*v));
            }
        }
    }
    Some(egui::ColorImage::new([w, h], px))
}

/// The value under the plot point `(t, log10 period)`: its column's time,
/// its row's period and its size (`None` where nothing was read).
fn cell(m: &TimeFrequency, t: f64, lp: f64) -> Option<(f64, f64, Option<f64>)> {
    let (t0, dt, _) = slots(m)?;
    let (j, &tc) = m
        .t_s
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - t).abs().total_cmp(&(b.1 - t).abs()))?;
    if (tc - t).abs() > dt / 2.0 + 1e-9 || t < t0 - dt / 2.0 {
        return None;
    }
    let half = row_half(m)?;
    let (i, &p) = m.period_s.iter().enumerate().min_by(|a, b| {
        (a.1.log10() - lp)
            .abs()
            .total_cmp(&(b.1.log10() - lp).abs())
    })?;
    if (p.log10() - lp).abs() > half + 1e-9 {
        return None;
    }
    let v = m.values.get(j)?.get(i).copied().filter(|v| v.is_finite());
    Some((tc, p, v))
}

/// Half a row's height, in log10 seconds.
fn row_half(m: &TimeFrequency) -> Option<f64> {
    let p = &m.period_s;
    let (a, b) = (p.first()?, p.last()?);
    if p.len() < 2 {
        return Some(0.05);
    }
    Some((a.log10() - b.log10()).abs() / (p.len() - 1) as f64 / 2.0)
}

/// A size in the map's unit: "0.12 ms", "3.1°".
pub fn size_text(m: &TimeFrequency, v: f64) -> String {
    let n = number_text(v);
    match m.unit {
        "deg" => format!("{n}°"),
        u => format!("{n} {u}"),
    }
}

/// A size's number alone, its unit given elsewhere: "0.12", "3.1".
fn number_text(v: f64) -> String {
    let d = if v >= 10.0 {
        0
    } else if v >= 1.0 {
        1
    } else if v >= 0.1 {
        2
    } else {
        3
    };
    format!("{v:.d$}")
}

/// What the map's colour measures: the swing of the series at each
/// period. The rate's map holds the timing's swing, in ms.
fn measure_name(m: &TimeFrequency) -> &'static str {
    match m.series {
        "rate" => "Timing Swing",
        "amplitude" => "Amplitude Swing",
        "beat_error" => "Beat Error Swing",
        _ => "Swing",
    }
}

/// The key's title, with the unit its numbers are in: "Timing Swing (ms)".
pub fn key_title(m: &TimeFrequency) -> String {
    let unit = match m.unit {
        "deg" => "°",
        u => u,
    };
    format!("{} ({unit})", measure_name(m))
}

/// Draw map `m` with `base` (the pane's plot, its axes still to set),
/// over the session's `0..upto_s`, with the wheels' turns marked across
/// it and a key under it.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut egui::Ui,
    base: Plot<'_>,
    id: egui::Id,
    m: &TimeFrequency,
    upto_s: f64,
    log: bool,
    wheels: Option<&[Wheel]>,
    time_label: impl Fn(f64) -> String + 'static,
    period_label: impl Fn(f64) -> String + Clone + 'static,
) {
    let pal = theme::pal(ui);
    let Some(scale) = Scale::of(&m.values, log) else {
        return note(ui, "Nothing in this stretch could be read for the map.");
    };
    let Some((t0, dt, w)) = slots(m) else {
        return note(ui, "Nothing in this stretch could be read for the map.");
    };
    let Some(half) = row_half(m) else {
        return;
    };
    // The picture, made again only when the map or its scale changes.
    let key = egui::Id::new((id, "tf texture"));
    let stamp = (
        m.t_s.len(),
        m.t_s.last().map(|t| t.to_bits()),
        scale.hi.to_bits(),
        scale.lo.to_bits(),
        log,
    );
    let cached = ui.data(|d| d.get_temp::<(String, egui::TextureHandle)>(key));
    let tex = match cached {
        Some((s, t)) if s == format!("{stamp:?}") => t,
        _ => {
            let Some(img) = image(m, scale) else {
                return;
            };
            let t = ui
                .ctx()
                .load_texture(format!("{key:?}"), img, egui::TextureOptions::NEAREST);
            ui.data_mut(|d| d.insert_temp(key, (format!("{stamp:?}"), t.clone())));
            t
        }
    };
    let top = m.period_s.first().map_or(1.0, |p| p.log10()) + half;
    let bottom = m.period_s.last().map_or(0.0, |p| p.log10()) - half;
    let left = t0 - dt / 2.0;
    let width = dt * w as f64;
    let key_h = KEY_H;
    let plot_h = (ui.available_height() - key_h).max(40.0);
    let plot_w = ui.available_width() as f64;
    let x_hi = upto_s.max(left + width);
    let resp = base
        .height(plot_h)
        .x_grid_spacer(|g| theme::even_grid(g, 80.0, &theme::TIME_STEPS))
        .x_axis_formatter(move |g, r| {
            // A mark too near the right edge for its label to show whole
            // goes unlabelled, rather than cut to "10:0".
            if (r.end() - g.value) / (r.end() - r.start()) * plot_w < 18.0 {
                return String::new();
            }
            time_label(g.value)
        })
        .y_grid_spacer(crate::steady::period_grid(plot_h))
        .y_axis_formatter(|g, _| crate::steady::period_label(g.value))
        .default_x_bounds(0.0f64.min(left), x_hi)
        .default_y_bounds(bottom, top)
        .show(ui, |p| {
            p.image(
                PlotImage::new(
                    "map",
                    tex.id(),
                    PlotPoint::new(left + width / 2.0, (top + bottom) / 2.0),
                    Vec2::new(width as f32, (top - bottom) as f32),
                )
                .highlight(false),
            );
            wheel_rows(p, wheels, (bottom, top), x_hi, pal);
            p.pointer_coordinate().and_then(|at| cell(m, at.x, at.y))
        });
    if let Some((t, per, v)) = resp.inner {
        let what = match v {
            Some(v) => size_text(m, v),
            None => "not read".into(),
        };
        resp.response.on_hover_text_at_pointer(format!(
            "{} {what} · period {} · at {}",
            measure_name(m),
            period_label(per),
            crate::strip::fmt_time(t)
        ));
    }
    key_row(ui, m, scale, pal);
}

/// The wheels' turns across the map, dashed in the wheels' colour and
/// named at the right, where they fall inside its periods; with no wheels,
/// the reference periods in grey; with `None`, nothing.
fn wheel_rows(
    p: &mut egui_plot::PlotUi<'_>,
    wheels: Option<&[Wheel]>,
    (lo, hi): (f64, f64),
    right: f64,
    pal: &theme::Palette,
) {
    let Some(wheels) = wheels else {
        return;
    };
    let rows: Vec<(String, f64, Color32)> = if wheels.is_empty() {
        timegrapher_core::periodicity::reference_lines()
            .into_iter()
            .map(|r| (r.label.to_string(), r.period_s, pal.text_secondary))
            .collect()
    } else {
        wheels
            .iter()
            .map(|w| (title_case(&w.name), w.period_s, pal.wheel))
            .collect()
    };
    for (name, period_s, colour) in rows {
        let y = period_s.log10();
        if y <= lo || y >= hi {
            continue;
        }
        p.hline(
            egui_plot::HLine::new(name.clone(), y)
                .color(colour)
                .style(LineStyle::dotted_dense())
                .width(1.25_f32),
        );
        p.text(
            Text::new("wheel", PlotPoint::new(right, y), format!("{name} "))
                .color(colour)
                .anchor(egui::Align2::RIGHT_BOTTOM),
        );
    }
}

/// The height of the key under the map.
pub const KEY_H: f32 = 22.0;

/// The words at the key's two ends. In dB the darkest is its level below
/// the brightest, 0 dB: a swing is a size, so twenty times its log.
fn key_ends(s: Scale) -> (String, String) {
    if s.log {
        let db = 20.0 * (s.lo / s.hi).log10();
        (
            crate::steady::db_label(db),
            format!(
                "{}, {} and above",
                crate::steady::db_label(0.0),
                number_text(s.hi)
            ),
        )
    } else {
        (
            number_text(s.lo),
            format!("{} and above", number_text(s.hi)),
        )
    }
}

/// Under the map: the colours from the smallest size to the largest, and
/// an empty swatch for cells not read.
fn key_row(ui: &mut egui::Ui, m: &TimeFrequency, s: Scale, pal: &theme::Palette) {
    ui.horizontal(|ui| {
        let small = |t: String| RichText::new(t).small().color(pal.text_secondary);
        ui.label(RichText::new(key_title(m)).small().color(pal.text));
        ui.add_space(6.0);
        let (lo, hi) = key_ends(s);
        ui.label(small(lo));
        let (r, _) = ui.allocate_exact_size(Vec2::new(120.0, 10.0), egui::Sense::hover());
        let n = 60;
        for i in 0..n {
            let x0 = r.left() + r.width() * i as f32 / n as f32;
            let x1 = r.left() + r.width() * (i + 1) as f32 / n as f32;
            ui.painter().rect_filled(
                egui::Rect::from_x_y_ranges(x0..=x1 + 0.5, r.y_range()),
                0.0,
                viridis(i as f32 / (n - 1) as f32),
            );
        }
        ui.label(small(hi));
        ui.add_space(12.0);
        let (e, _) = ui.allocate_exact_size(Vec2::new(14.0, 10.0), egui::Sense::hover());
        ui.painter().rect_stroke(
            e,
            2.0,
            egui::Stroke::new(1.0_f32, pal.text_tertiary),
            egui::StrokeKind::Inside,
        );
        ui.label(small("not read".into()));
    })
    .response
    .on_hover_text(
        "The colour gives the size at each period and time; the brightest also stands for \
         every size above it. Empty cells weren't read: a gap, a window with a reading \
         missing, or the edges of a stretch, which the wavelet map can't reach.",
    );
}

/// "escape wheel" as "Escape Wheel".
fn title_case(name: &str) -> String {
    name.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn note(ui: &mut egui::Ui, text: &str) {
    let pal = theme::pal(ui);
    ui.label(RichText::new(text).color(pal.text_secondary));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(values: Vec<Vec<f64>>, t: Vec<f64>, hop: Option<f64>) -> TimeFrequency {
        TimeFrequency {
            series: "amplitude",
            method: if hop.is_some() { "stft" } else { "sst" },
            unit: "deg",
            step_s: 2.0,
            window_s: hop.map(|_| 1200.0),
            hop_s: hop,
            period_s: vec![100.0, 10.0, 1.0],
            t_s: t,
            values,
        }
    }

    #[test]
    fn viridis_runs_from_purple_to_yellow() {
        assert_eq!(viridis(0.0), Color32::from_rgb(68, 1, 84));
        assert_eq!(viridis(1.0), Color32::from_rgb(253, 231, 37));
        assert_eq!(viridis(-1.0), viridis(0.0));
        let mid = viridis(0.5);
        assert!(mid.g() > mid.r() && mid.b() > 100, "{mid:?}");
    }

    #[test]
    fn empty_cells_stay_clear_and_a_missing_window_leaves_its_slot() {
        let nan = f64::NAN;
        // Windows at 600, 900 and 1500 s: the one at 1200 s was left out.
        let m = map(
            vec![
                vec![1.0, 2.0, nan],
                vec![0.0, 4.0, 4.0],
                vec![nan, nan, 2.0],
            ],
            vec![600.0, 900.0, 1500.0],
            Some(300.0),
        );
        let s = Scale::of(&m.values, false).unwrap();
        assert_eq!((s.lo, s.hi), (0.0, 4.0));
        let img = image(&m, s).unwrap();
        assert_eq!(img.size, [4, 3]);
        let at = |x: usize, y: usize| img.pixels[y * 4 + x];
        assert_eq!(at(0, 2), Color32::TRANSPARENT, "not read");
        assert_eq!(at(2, 0), Color32::TRANSPARENT, "the window left out");
        assert_eq!(at(1, 0), viridis(0.0));
        assert_eq!(at(1, 1), viridis(1.0));
        assert_eq!(at(3, 2), viridis(0.5));
        // Under the pointer: the cell's time, period and size.
        assert_eq!(cell(&m, 910.0, 1.0), Some((900.0, 10.0, Some(4.0))));
        assert_eq!(cell(&m, 600.0, 0.0), Some((600.0, 1.0, None)));
        assert_eq!(cell(&m, 1200.0, 1.0), None, "between windows");
        assert_eq!(cell(&m, 600.0, 2.9), None, "above the longest period");
    }

    #[test]
    fn a_log_scale_spans_at_most_300_to_1() {
        let m = map(
            vec![vec![0.001, 1.0, 3.0], vec![0.0, 0.5, 3.0]],
            vec![10.0, 20.0],
            None,
        );
        let s = Scale::of(&m.values, true).unwrap();
        assert_eq!(s.hi, 3.0);
        assert!((s.lo - 0.01).abs() < 1e-12, "{s:?}");
        assert_eq!(s.at(0.0), 0.0);
        assert_eq!(s.at(3.0), 1.0);
        assert!((s.at((0.01f64 * 3.0).sqrt()) - 0.5).abs() < 1e-6);
        assert!(Scale::of(&[vec![f64::NAN]], false).is_none());
        assert_eq!(size_text(&m, 3.04), "3.0°");
    }

    /// In dB the key reads from the floor below the brightest to 0 dB: a
    /// swing is a size, so a 300 to 1 range is about −50 dB.
    #[test]
    fn the_keys_db_scale_reads_down_from_0_db() {
        let s = Scale {
            lo: 0.01,
            hi: 3.0,
            log: true,
        };
        assert_eq!(
            key_ends(s),
            ("−50 dB".to_string(), "0 dB, 3.0 and above".to_string())
        );
        let s = Scale { log: false, ..s };
        assert_eq!(key_ends(s).1, "3.0 and above");
    }

    /// The key says what its colour measures, with the unit: the rate's
    /// map holds the timing's swing in ms, not a rate in s/d.
    #[test]
    fn the_key_names_what_its_colour_measures() {
        let mut m = map(vec![vec![1.0]], vec![10.0], None);
        assert_eq!(key_title(&m), "Amplitude Swing (°)");
        m.series = "rate";
        m.unit = "ms";
        assert_eq!(key_title(&m), "Timing Swing (ms)");
        assert_eq!(size_text(&m, 1.83), "1.8 ms");
        assert_eq!(number_text(0.123), "0.12");
        m.series = "beat_error";
        assert_eq!(key_title(&m), "Beat Error Swing (ms)");
    }
}
