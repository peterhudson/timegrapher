//! A self-contained HTML report for a long run: inline SVG charts, no
//! scripts or external files, light and dark themes.

use crate::long::{duration, false_alarm, wheel_note};
use std::fmt::Write;
use timegrapher_core::library::fixed_text;
use timegrapher_core::longrun::LongReport;
use timegrapher_core::longterm::{false_alarm_text, Component, Search};
use timegrapher_core::periodicity::Wheel;
use timegrapher_core::{chart_scale, period_ticks};

const W: f64 = 760.0;
const H: f64 = 220.0;
const ML: f64 = 56.0;
const MR: f64 = 16.0;
const MT: f64 = 14.0;
const MB: f64 = 40.0;

/// Text put into the page: core's one escaper (#1649).
use timegrapher_core::xml::escape as esc;

/// About five round tick values covering `lo..hi`, labelled: core's
/// grid step (`chart_scale::step_for`), written as every axis writes a
/// value of that step (`chart_scale::axis_number`).
fn ticks(lo: f64, hi: f64) -> Vec<(f64, String)> {
    let step = chart_scale::step_for(hi - lo);
    let mut v = Vec::new();
    let mut k = (lo / step - 1e-9).ceil();
    while k * step <= hi + 1e-9 * step {
        let t = k * step;
        v.push((t, chart_scale::axis_number(t, step)));
        k += 1.0;
    }
    v
}

struct Axis {
    lo: f64,
    hi: f64,
    log: bool,
}

impl Axis {
    /// A log axis over the values above zero.
    fn log_of(values: impl Iterator<Item = f64>) -> Axis {
        let (mut lo, mut hi) = values
            .filter(|v| v.is_finite() && *v > 0.0)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                (a.min(v), b.max(v))
            });
        if !lo.is_finite() {
            (lo, hi) = (0.0, 1.0);
        }
        if hi - lo < 1e-12 {
            lo -= 0.5;
            hi += 0.5;
        }
        Axis { lo, hi, log: true }
    }
    /// A value axis on core's chart scale (`chart_scale::data_range`): every
    /// point of the lines, padded as the app's charts over time are.
    fn of_values(values: &[f64]) -> Axis {
        let (lo, hi) = chart_scale::data_range(values).unwrap_or((0.0, 1.0));
        Axis { lo, hi, log: false }
    }
    fn frac(&self, v: f64) -> f64 {
        if self.log {
            (v.ln() - self.lo.ln()) / (self.hi.ln() - self.lo.ln())
        } else {
            (v - self.lo) / (self.hi - self.lo)
        }
    }
    fn x(&self, v: f64) -> f64 {
        ML + self.frac(v) * (W - ML - MR)
    }
    fn y(&self, v: f64) -> f64 {
        H - MB - self.frac(v) * (H - MT - MB)
    }
    /// The ticks and their labels. A log axis is one of periods, marked
    /// at the times core's `period_ticks` names, as many as the plot has
    /// room for, as the app's and the MCP view's period axes are.
    fn ticks(&self) -> Vec<(f64, String)> {
        if !self.log {
            return ticks(self.lo, self.hi);
        }
        let room = ((W - ML - MR) / period_ticks::MARK_ROOM_PX).max(2.0) as usize;
        period_ticks::period_ticks(self.lo, self.hi, room)
            .into_iter()
            .map(|t| (t.period_s, t.label.to_string()))
            .collect()
    }
}

struct Line {
    pts: Vec<(f64, f64)>,
    class: &'static str,
    /// Dots with hover text instead of a line.
    dots: bool,
    tip: Box<dyn Fn(f64, f64) -> String>,
}

struct VLine {
    x: f64,
    label: String,
}

fn chart(
    xa: &Axis,
    ya: &Axis,
    xlabel: &str,
    ylabel: &str,
    lines: &[Line],
    vlines: &[VLine],
    hline: Option<(f64, &str)>,
) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg viewBox="0 0 {W} {H}" role="img" aria-label="{}">"#,
        esc(ylabel)
    );
    axes(&mut s, xa, ya, xlabel, ylabel);
    marks(&mut s, xa, ya, vlines, hline);
    series(&mut s, xa, ya, lines);
    s.push_str("</svg>");
    s
}

/// The chart's grid, ticks, axes and their labels.
fn axes(s: &mut String, xa: &Axis, ya: &Axis, xlabel: &str, ylabel: &str) {
    for (t, label) in ya.ticks() {
        let y = ya.y(t);
        let _ = write!(
            s,
            r#"<line class="grid" x1="{ML}" x2="{}" y1="{y:.1}" y2="{y:.1}"/><text class="tick" x="{}" y="{:.1}" text-anchor="end">{}</text>"#,
            W - MR,
            ML - 6.0,
            y + 4.0,
            label
        );
    }
    for (t, label) in xa.ticks() {
        let x = xa.x(t);
        let _ = write!(
            s,
            r#"<line class="axis" x1="{x:.1}" x2="{x:.1}" y1="{}" y2="{}"/><text class="tick" x="{x:.1}" y="{}" text-anchor="middle">{}</text>"#,
            H - MB,
            H - MB + 4.0,
            H - MB + 17.0,
            label
        );
    }
    let _ = write!(
        s,
        r#"<line class="axis" x1="{ML}" x2="{}" y1="{}" y2="{}"/>"#,
        W - MR,
        H - MB,
        H - MB
    );
    let _ = write!(
        s,
        r#"<text class="label" x="{}" y="{}" text-anchor="middle">{}</text>"#,
        (ML + W - MR) / 2.0,
        H - 6.0,
        esc(xlabel)
    );
    let _ = write!(
        s,
        r#"<text class="label" transform="translate(13 {}) rotate(-90)" text-anchor="middle">{}</text>"#,
        (MT + H - MB) / 2.0,
        esc(ylabel)
    );
}

/// The wheels' lines and the threshold, where they fall on the chart.
fn marks(s: &mut String, xa: &Axis, ya: &Axis, vlines: &[VLine], hline: Option<(f64, &str)>) {
    for v in vlines {
        if v.x < xa.lo || v.x > xa.hi {
            continue;
        }
        let x = xa.x(v.x);
        let _ = write!(
            s,
            r#"<line class="wheel" x1="{x:.1}" x2="{x:.1}" y1="{MT}" y2="{}"/><text class="wheel-label" x="{:.1}" y="{}">{}</text>"#,
            H - MB,
            x + 3.0,
            MT + 10.0,
            esc(&v.label)
        );
    }
    if let Some((y, label)) = hline {
        if y >= ya.lo && y <= ya.hi {
            let yy = ya.y(y);
            let _ = write!(
                s,
                r#"<line class="threshold" x1="{ML}" x2="{}" y1="{yy:.1}" y2="{yy:.1}"/><text class="wheel-label" x="{}" y="{:.1}" text-anchor="end">{}</text>"#,
                W - MR,
                W - MR - 2.0,
                yy - 4.0,
                esc(label)
            );
        }
    }
}

/// Each line, as dots or as a path broken where a value is missing.
fn series(s: &mut String, xa: &Axis, ya: &Axis, lines: &[Line]) {
    for l in lines {
        if l.dots {
            for &(x, y) in &l.pts {
                if x.is_finite() && y.is_finite() {
                    let _ = write!(
                        s,
                        r#"<circle class="{}" cx="{:.1}" cy="{:.1}" r="2.5"><title>{}</title></circle>"#,
                        l.class,
                        xa.x(x),
                        ya.y(y.clamp(ya.lo, ya.hi)),
                        esc(&(l.tip)(x, y))
                    );
                }
            }
            continue;
        }
        let mut d = String::new();
        let mut pen = false;
        for &(x, y) in &l.pts {
            if !(x.is_finite() && y.is_finite()) || (xa.log && x <= 0.0) {
                pen = false;
                continue;
            }
            let _ = write!(
                d,
                "{}{:.1},{:.1}",
                if pen { " L" } else { " M" },
                xa.x(x),
                ya.y(y.clamp(ya.lo, ya.hi))
            );
            pen = true;
        }
        let _ = write!(s, r#"<path class="{}" d="{}"/>"#, l.class, d.trim());
    }
}

/// Time axis in minutes for runs under three hours, hours beyond.
fn time_unit(span: f64) -> (f64, &'static str) {
    if span < 3.0 * 3600.0 {
        (60.0, "minutes from start")
    } else {
        (3600.0, "hours from start")
    }
}

fn over_time(r: &LongReport) -> String {
    let (u, label) = time_unit(r.duration_s);
    let mut out = String::new();
    let mid = |s: &timegrapher_core::longrun::Slice| (s.start_s + s.end_s) / 2.0 / u;
    let rate: Vec<(f64, f64)> = r
        .slices
        .iter()
        .map(|s| (mid(s), s.rate_s_per_day.unwrap_or(f64::NAN)))
        .collect();
    let amp: Vec<(f64, f64)> = r
        .slices
        .iter()
        .map(|s| (mid(s), s.amplitude_deg.unwrap_or(f64::NAN)))
        .collect();
    let xa = Axis {
        lo: 0.0,
        hi: r.duration_s / u,
        log: false,
    };
    let slice = slice_name(r.slice_s);
    for (title, pts, unit) in [("Rate", rate, "s/d"), ("Amplitude", amp, "deg")] {
        let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();
        let ya = Axis::of_values(&ys);
        let _ = write!(
            out,
            "<h3>{title} in {slice} slices</h3>{}",
            chart(
                &xa,
                &ya,
                label,
                unit,
                &[Line {
                    pts,
                    class: "series",
                    dots: false,
                    tip: Box::new(|_, _| String::new()),
                }],
                &[],
                None,
            )
        );
    }
    out
}

/// The search, reduced to at most 900 points (the max in each log-period bin).
fn search_points(s: &Search) -> Vec<(f64, f64)> {
    let n = s.period_s.len();
    if n <= 900 {
        return s
            .period_s
            .iter()
            .copied()
            .zip(s.score.iter().copied())
            .collect();
    }
    let (lo, hi) = (
        s.period_s
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min)
            .ln(),
        s.period_s.iter().copied().fold(0.0, f64::max).ln(),
    );
    let mut best = vec![(f64::NAN, f64::NEG_INFINITY); 900];
    for (&p, &v) in s.period_s.iter().zip(&s.score) {
        let b = (((p.ln() - lo) / (hi - lo)) * 899.0).round() as usize;
        if v > best[b].1 {
            best[b] = (p, v);
        }
    }
    let mut pts: Vec<(f64, f64)> = best.into_iter().filter(|b| b.0.is_finite()).collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    pts
}

fn search_chart(s: &Search, wheels: &[Wheel]) -> String {
    if s.period_s.is_empty() {
        return "<p class=\"note\">The run is too short to search.</p>".into();
    }
    let pts = search_points(s);
    let xa = Axis::log_of(pts.iter().map(|p| p.0));
    let ya = Axis {
        lo: 0.0,
        hi: pts.iter().map(|p| p.1).fold(s.threshold * 1.3, f64::max) * 1.05,
        log: false,
    };
    let vlines: Vec<VLine> = wheels
        .iter()
        .map(|w| VLine {
            x: w.period_s,
            label: w.name.clone(),
        })
        .collect();
    chart(
        &xa,
        &ya,
        "period (log scale)",
        "score (-log10 p)",
        &[Line {
            pts,
            class: "series",
            dots: false,
            tip: Box::new(|_, _| String::new()),
        }],
        &vlines,
        Some((s.threshold, "1% false-alarm level")),
    )
}

/// Diverging blue-grey-red for a raster cell, `v` scaled to -1..1.
fn diverging(v: f64) -> String {
    if !v.is_finite() {
        return "var(--empty)".into();
    }
    let v = v.clamp(-1.0, 1.0);
    let k = (v.abs() * 8.0).round() as usize;
    format!("var(--{}{k})", if v < 0.0 { "neg" } else { "pos" })
}

/// How a raster row is coloured: the folded values as they are (times
/// `scale`), or as rate from their slope (for timing offsets in seconds).
#[derive(Clone, Copy)]
enum RasterKind {
    Value(f64),
    Rate,
}

/// The rows of `c`'s raster as drawn: at most 240 rows of at most 60
/// columns, thinned by core's one rule (`longterm::thin`), each cell the
/// mean of the cycles and bins it covers.
fn raster_rows(c: &Component, kind: RasterKind) -> timegrapher_core::longterm::Thinned {
    // A rate is read on the cycles themselves, before rows are merged: an
    // averaged row's last bin is not one bin before the next one's first.
    let mut t = match kind {
        RasterKind::Value(_) => c.fold.thinned(240, 60),
        RasterKind::Rate => timegrapher_core::longterm::thin(
            &timegrapher_core::longterm::rate_rows(&c.fold.raster, c.period_s),
            240,
            60,
        ),
    };
    if let RasterKind::Value(scale) = kind {
        for v in t.cells.iter_mut().flatten() {
            *v *= scale;
        }
    }
    t
}

/// The cycles a raster row averages, `[first, end)` from 0, counted
/// from 1: "cycle 4", or "cycles 4–8 averaged".
fn cycles_text([first, end]: [usize; 2]) -> String {
    if end - first <= 1 {
        format!("cycle {}", first + 1)
    } else {
        format!("cycles {}–{end} averaged", first + 1)
    }
}

fn raster(c: &Component, kind: RasterKind, unit: &str) -> String {
    if c.fold.raster.is_empty() {
        return String::new();
    }
    let thinned = raster_rows(c, kind);
    let merged = &thinned.cells;
    let cols = thinned.columns.len();
    let each = if merged.len() < c.fold.raster.len() {
        "each row the mean of a run of cycles"
    } else {
        "one row each"
    };
    // Full colour at core's limit for a colour map.
    let cells: Vec<f64> = merged.iter().flatten().copied().collect();
    let lim = timegrapher_core::chart_scale::colour_limit(&cells);
    let rh = (360.0 / merged.len() as f64).clamp(1.5, 12.0);
    let h = rh * merged.len() as f64;
    let cw = (W - ML - MR) / cols as f64;
    let mut s = format!("<h4>Every cycle, {each}, top to bottom</h4>");
    let _ = write!(
        s,
        r#"<svg viewBox="0 0 {W} {}" role="img" aria-label="raster">"#,
        h + 34.0
    );
    for (i, row) in merged.iter().enumerate() {
        for (j, &v) in row.iter().enumerate() {
            let val = v;
            let _ = write!(
                s,
                r#"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="{}"><title>{}, {} s into it: {}</title></rect>"#,
                ML + j as f64 * cw,
                MT + i as f64 * rh,
                cw + 0.3,
                rh + 0.3,
                diverging(val / lim),
                cycles_text(thinned.rows[i]),
                fixed_text((j as f64 + 0.5) / cols as f64 * c.period_s, 1),
                if val.is_finite() {
                    format!("{} {unit}", timegrapher_core::library::signed_text(val, 2))
                } else {
                    "no data".into()
                }
            );
        }
    }
    let _ = write!(
        s,
        r#"<text class="tick" x="{}" y="{:.1}" text-anchor="end">1</text><text class="tick" x="{}" y="{:.1}" text-anchor="end">{}</text>"#,
        ML - 6.0,
        MT + 9.0,
        ML - 6.0,
        MT + h,
        c.fold.raster.len()
    );
    let _ = write!(
        s,
        r#"<text class="label" x="{}" y="{:.1}" text-anchor="middle">seconds into each {} s cycle; blue below, red above; full colour at ±{} {unit}</text></svg>"#,
        (ML + W - MR) / 2.0,
        MT + h + 18.0,
        fixed_text(c.period_s, 1),
        fixed_text(lim, 2)
    );
    s
}

fn component(
    c: &Component,
    kind: &str,
    swing: String,
    shape: &[f64],
    points: Option<&[f64]>,
    unit: &str,
    raster_kind: RasterKind,
) -> String {
    let n = shape.len();
    let x = |i: usize| (i as f64 + 0.5) / n as f64 * c.period_s;
    let xa = Axis {
        lo: 0.0,
        hi: c.period_s,
        log: false,
    };
    let mut lines = Vec::new();
    let mut vals: Vec<f64> = shape.to_vec();
    if let Some(p) = points {
        vals.extend(p.iter().copied());
        let u = unit.to_string();
        lines.push(Line {
            pts: p.iter().enumerate().map(|(i, &v)| (x(i), v)).collect(),
            class: "dot",
            dots: true,
            tip: Box::new(move |x, y| {
                format!(
                    "{} s: {} {u} (median over cycles)",
                    fixed_text(x, 1),
                    timegrapher_core::library::signed_text(y, 2)
                )
            }),
        });
    }
    lines.push(Line {
        pts: shape.iter().enumerate().map(|(i, &v)| (x(i), v)).collect(),
        class: "series",
        dots: false,
        tip: Box::new(|_, _| String::new()),
    });
    let ya = Axis::of_values(&vals);
    let rows = c.fold.raster.len();
    format!(
        r#"<div class="component"><h3>{kind}: {} s cycle</h3>
<p>{swing}; explains {}% of the detrended variation; false-alarm chance {}; period known to ±{} s. {}.</p>
<h4>One cycle, averaged over {rows} cycles</h4>{}
{}</div>"#,
        fixed_text(c.period_s, 2),
        fixed_text(c.explained * 100.0, 0),
        false_alarm_text(c.significance, c.beyond_simulated, false_alarm),
        fixed_text(c.resolution_s, 2),
        esc(&capitalise(&wheel_note(c))),
        chart(&xa, &ya, "seconds into the cycle", unit, &lines, &[], None),
        raster(c, raster_kind, unit)
    )
}

pub fn slice_name(s: f64) -> String {
    if s < 60.0 {
        format!("{} s", fixed_text(s, 0))
    } else {
        format!("{} min", fixed_text(s / 60.0, 0))
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The take's rate as the report gives it, no sign at zero.
fn rate_text(rate_s_per_day: f64) -> String {
    format!(
        "{} s/d",
        timegrapher_core::library::signed_text(rate_s_per_day, 2)
    )
}

pub fn html(title: &str, r: &LongReport, wheels: &[Wheel], clock_note: Option<&str>) -> String {
    let mut body = String::new();
    facts(&mut body, r, clock_note);
    body.push_str("<h2>Over the run</h2>");
    body.push_str(&over_time(r));

    rate_cycles(&mut body, r, wheels);
    amplitude_cycles(&mut body, r, wheels);
    page(title, &body)
}

/// The table of the run's facts: the recording, clock, rate, beat error,
/// amplitude and their caveats.
fn facts(body: &mut String, r: &LongReport, clock_note: Option<&str>) {
    let rate = r.overall.map_or("not enough clean beats".into(), |f| {
        rate_text(f.rate_s_per_day)
    });
    let clock = match (&r.clock, clock_note) {
        (Some(c), Some(note)) => format!(
            "corrected: sound card {} ppm {} than true time, a steady error {note}",
            fixed_text(c.ppm.abs(), 2),
            if c.ppm >= 0.0 { "slower" } else { "faster" },
        ),
        (Some(c), None) if !c.corrects() => format!(
            "not calibrated: {}",
            c.withheld.map(|w| w.note()).unwrap_or_default()
        ),
        (Some(c), None) => format!(
            "calibrated: sound card {} ppm {} than NTP time ({} entries, {} ms rms)",
            fixed_text(c.ppm.abs(), 2),
            if c.ppm >= 0.0 { "slower" } else { "faster" },
            c.points,
            fixed_text(c.residual_ms, 1)
        ),
        (None, _) => "not calibrated: rates are on the sound card's clock".into(),
    };
    let o = |v: Option<f64>, d: usize| v.map_or("-".into(), |x| fixed_text(x, d));
    let _ = write!(
        body,
        r#"<table class="facts">
<tr><th>Recording</th><td>{} at {} Hz, {} bph, {} beats ({}% clean)</td></tr>
<tr><th>Clock</th><td>{}</td></tr>
<tr><th>Rate</th><td>{rate} over the run; {} slices from {} to {} s/d (5th to 95th percentile)</td></tr>
<tr><th>Beat error</th><td>{}</td></tr>
<tr><th>Amplitude</th><td>{} deg median; slices from {} to {} deg (lift angle {} deg)</td></tr>
{}</table>"#,
        duration(r.duration_s),
        r.sample_rate,
        r.bph,
        r.beats_found,
        fixed_text(r.clean_fraction * 100.0, 1),
        esc(&clock),
        slice_name(r.slice_s),
        crate::session_cmd::signed(r.rate_p05, 1),
        crate::session_cmd::signed(r.rate_p95, 1),
        r.overall
            .map_or("-".into(), |f| esc(&crate::beat_error_text(
                r.beat_error_unlock_ms,
                f.beat_error_ms,
                timegrapher_core::measure::DropWhy::of(&r.caveats)
            ))),
        o(r.amplitude_deg, 0),
        o(r.amplitude_p05, 0),
        o(r.amplitude_p95, 0),
        r.lift_deg,
        // What the figures above carry, as the text summary says it.
        crate::long::caveat_lines(&r.caveats)
            .iter()
            .map(|(label, line)| format!("<tr><th>{label}</th><td>{}</td></tr>\n", esc(line)))
            .collect::<String>()
    );
}

/// The search for periodic changes in rate, and each one named.
fn rate_cycles(body: &mut String, r: &LongReport, wheels: &[Wheel]) {
    body.push_str("<h2>Periodic changes in rate</h2>");
    body.push_str(r#"<p class="note">Each trial period is scored together with its harmonics against the noise around it. A peak above the dashed line has less than a 1% chance of being noise anywhere in the search. Grey lines mark the wheels' turn periods.</p>"#);
    body.push_str(&search_chart(&r.timing.search, wheels));
    if r.rate_components.is_empty() {
        body.push_str("<p>No periodic change in rate named (1% false-alarm level, held across the take's tests).</p>");
    }
    for c in &r.rate_components {
        // A cycle too short for a rate reading is drawn as timing, in ms.
        if c.size().1 == "ms" {
            let ms = |v: &[f64]| v.iter().map(|x| x * 1000.0).collect::<Vec<_>>();
            body.push_str(&component(
                &c.component,
                "Timing",
                format!(
                    "Beats run early and late by {} ms peak to peak over each turn",
                    fixed_text(c.timing_swing_ms, 3)
                ),
                &ms(&c.component.shape),
                Some(&ms(&c.component.fold.profile)),
                "ms",
                RasterKind::Value(1000.0),
            ));
            continue;
        }
        body.push_str(&component(
            &c.component,
            "Rate",
            format!(
                "The rate itself swings {} s/d peak to peak (10 s rate readings show less)",
                fixed_text(c.rate_swing_s_per_day, 1)
            ),
            &c.rate_shape,
            None,
            "s/d",
            RasterKind::Rate,
        ));
    }
}

/// The search for periodic changes in amplitude, each one named, and how
/// to read the rasters.
fn amplitude_cycles(body: &mut String, r: &LongReport, wheels: &[Wheel]) {
    body.push_str("<h2>Periodic changes in amplitude</h2>");
    body.push_str(&search_chart(&r.amplitude.search, wheels));
    if r.amplitude.components.is_empty() {
        body.push_str("<p>No periodic change in amplitude named (1% false-alarm level, held across the take's tests).</p>");
    }
    for c in &r.amplitude.components {
        body.push_str(&component(
            c,
            "Amplitude",
            format!(
                "Amplitude swings {} deg peak to peak",
                fixed_text(c.peak_to_peak, 1)
            ),
            &c.shape,
            Some(&c.fold.profile),
            "deg",
            RasterKind::Value(1.0),
        ));
    }
    body.push_str(r#"<p class="note">In the rasters each row is one turn of the cycle. A change that repeats once per turn lines up at the same place in every row. Rows are merged when there are more than 240 cycles.</p>"#);
}

/// The whole page around `body`, with its light and dark styles.
fn page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Long run report</title>
<style>
:root {{
  color-scheme: light;
  --surface: #fcfcfb; --text: #0b0b0b; --text2: #52514e; --muted: #8a8984; --rule: #e4e3df;
  --series: #2a78d6; --empty: #f7f7f5;
  {light_ramp}
}}
@media (prefers-color-scheme: dark) {{
  :root:not([data-theme="light"]) {{
    color-scheme: dark;
    --surface: #1a1a19; --text: #ffffff; --text2: #c3c2b7; --muted: #8f8e86; --rule: #34342f;
    --series: #3987e5; --empty: #232321;
    {dark_ramp}
  }}
}}
:root[data-theme="dark"] {{
  color-scheme: dark;
  --surface: #1a1a19; --text: #ffffff; --text2: #c3c2b7; --muted: #8f8e86; --rule: #34342f;
  --series: #3987e5; --empty: #232321;
  {dark_ramp}
}}
body {{ background: var(--surface); color: var(--text); font: 15px/1.45 system-ui, sans-serif; margin: 0 auto; max-width: 800px; padding: 16px; }}
h1 {{ font-size: 22px; margin: 8px 0 4px; }} h2 {{ font-size: 18px; margin-top: 28px; }} h3 {{ font-size: 15px; margin: 18px 0 4px; }} h4 {{ font-size: 13px; color: var(--text2); margin: 10px 0 2px; font-weight: 600; }}
p {{ margin: 6px 0; }} .note {{ color: var(--text2); font-size: 13px; }}
svg {{ width: 100%; height: auto; display: block; }}
.facts {{ border-collapse: collapse; width: 100%; }} .facts th {{ text-align: left; color: var(--text2); font-weight: 600; padding: 3px 12px 3px 0; vertical-align: top; white-space: nowrap; }} .facts td {{ padding: 3px 0; }}
.grid {{ stroke: var(--rule); stroke-width: 1; }} .axis {{ stroke: var(--muted); stroke-width: 1; }}
.tick {{ fill: var(--text2); font-size: 11px; }} .label {{ fill: var(--text2); font-size: 12px; }}
.series {{ fill: none; stroke: var(--series); stroke-width: 2; stroke-linejoin: round; }}
.dot {{ fill: var(--series); fill-opacity: 0.45; }}
.wheel {{ stroke: var(--muted); stroke-dasharray: 3 3; }} .wheel-label {{ fill: var(--text2); font-size: 11px; }}
.threshold {{ stroke: var(--text2); stroke-dasharray: 6 4; }}
.component {{ border-top: 1px solid var(--rule); margin-top: 16px; }}
</style></head><body>
<h1>{}</h1>
<p class="note">Long-run analysis by timegrapher {}</p>
{body}
</body></html>
"#,
        esc(title),
        env!("CARGO_PKG_VERSION"),
        light_ramp = ramp("#f0efec", "#1c5cab", "#c0302f"),
        dark_ramp = ramp("#383835", "#5598e7", "#e66767"),
    )
}

/// CSS variables `--neg0..8` and `--pos0..8` from the neutral mid colour to each pole.
fn ramp(mid: &str, neg: &str, pos: &str) -> String {
    let rgb = |h: &str| {
        let v = u32::from_str_radix(&h[1..], 16).unwrap_or(0);
        [(v >> 16) as f64, ((v >> 8) & 255) as f64, (v & 255) as f64]
    };
    let (m, n, p) = (rgb(mid), rgb(neg), rgb(pos));
    let mut s = String::new();
    for k in 0..=8 {
        let t = k as f64 / 8.0;
        for (name, pole) in [("neg", n), ("pos", p)] {
            let c: Vec<u8> = (0..3)
                .map(|i| (m[i] + (pole[i] - m[i]) * t).round() as u8)
                .collect();
            let _ = write!(s, "--{name}{k}: #{:02x}{:02x}{:02x}; ", c[0], c[1], c[2]);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use timegrapher_core::longterm::Fold;

    /// The report's facts carry the caveats the text summary prints with
    /// them: here, sides named by the beat count across a jump.
    #[test]
    fn the_report_gives_its_figures_caveats() {
        use timegrapher_core::beats::Beat;
        use timegrapher_core::stream::BeatLog;
        let log = BeatLog {
            sample_rate: 48_000,
            duration_s: 600.0,
            bph: 18_000,
            lift_deg: 52.0,
            beats: (0..3000)
                .map(|i| Beat {
                    index: i,
                    time: i as f64 * 0.2,
                    quality: 1.0,
                })
                .collect(),
            amplitude_windows: Vec::new(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: vec![300.0],
            sided: Vec::new(),
            by_count: vec![300.0],
            snippets: Vec::new(),
        };
        let lc = timegrapher_core::longterm::LongConfig::default();
        let r = timegrapher_core::longrun::analyse(&log, None, &lc);
        let (label, line) = crate::long::caveat_lines(&r.caveats)[0].clone();
        let page = html("take", &r, &[], None);
        assert!(
            page.contains(&format!("<tr><th>{label}</th><td>{}</td></tr>", esc(&line))),
            "{label}: {line}"
        );
    }

    /// A take that gains a steady 8.64 s/d draws 8.64 in every bin of the
    /// rate raster, its ends too, when each drawn row merges several
    /// cycles: the rate is read on the cycles before they are merged.
    #[test]
    fn a_merged_rate_raster_reads_each_cycles_rate() {
        let (period, bins, cycles) = (100.0, 20, 600);
        let dt = period / bins as f64;
        let raster: Vec<Vec<f64>> = (0..cycles)
            .map(|k| {
                (0..bins)
                    .map(|j| -1e-4 * (k * bins + j) as f64 * dt)
                    .collect()
            })
            .collect();
        let c = Component {
            period_s: period,
            resolution_s: 0.0,
            significance: 0.0,
            beyond_simulated: false,
            harmonics: 1,
            explained: 0.0,
            peak_to_peak: 0.0,
            wheel: None,
            nearest_wheel: None,
            shape: vec![0.0; bins],
            fold: Fold {
                period_s: period,
                profile: vec![0.0; bins],
                counts: vec![cycles; bins],
                raster,
                t0: 0.0,
            },
            harmonic_of: None,
        };
        let t = raster_rows(&c, RasterKind::Rate);
        let (rows, group, cols) = (&t.cells, t.rows[0][1] - t.rows[0][0], t.columns.len());
        assert!(group > 1 && cols == bins);
        assert!(
            rows.iter().flatten().all(|v| (v - 8.64).abs() < 1e-6),
            "{:?}",
            rows[0]
        );
    }

    /// A cycle's chart is scaled as the app's charts are, on core's
    /// `chart_scale::data_range`: a flat cycle at 2 is drawn from 1.95 to
    /// 2.05, not from 1.5 to 2.5.
    #[test]
    fn a_cycles_chart_takes_cores_scale() {
        let bins = 10;
        let c = Component {
            period_s: 60.0,
            resolution_s: 0.0,
            significance: 0.0,
            beyond_simulated: false,
            harmonics: 1,
            explained: 0.0,
            peak_to_peak: 0.0,
            wheel: None,
            nearest_wheel: None,
            shape: vec![2.0; bins],
            fold: Fold {
                period_s: 60.0,
                profile: vec![2.0; bins],
                counts: vec![3; bins],
                raster: Vec::new(),
                t0: 0.0,
            },
            harmonic_of: None,
        };
        let page = component(
            &c,
            "Rate",
            String::new(),
            &c.shape,
            None,
            "s/d",
            RasterKind::Rate,
        );
        let ticks: Vec<&str> = page
            .split(r#"text-anchor="end">"#)
            .skip(1)
            .filter_map(|t| t.split('<').next())
            .collect();
        assert!(
            ticks.contains(&"2.04") && !ticks.contains(&"2.4"),
            "{ticks:?}"
        );
    }

    /// A fold-map cell at zero reads without a sign in its hover label
    /// (Walter's walkthrough of #746).
    #[test]
    fn a_fold_map_cell_at_zero_reads_without_a_sign() {
        let bins = 10;
        let cells: Vec<Vec<f64>> = (0..4)
            .map(|k| {
                (0..bins)
                    .map(|j| if (k + j) % 2 == 0 { -1e-4 } else { 0.5 })
                    .collect()
            })
            .collect();
        let c = Component {
            period_s: 10.0,
            resolution_s: 0.0,
            significance: 0.0,
            beyond_simulated: false,
            harmonics: 1,
            explained: 0.0,
            peak_to_peak: 0.0,
            wheel: None,
            nearest_wheel: None,
            shape: vec![0.0; bins],
            fold: Fold {
                period_s: 10.0,
                profile: vec![0.0; bins],
                counts: vec![4; bins],
                raster: cells,
                t0: 0.0,
            },
            harmonic_of: None,
        };
        let svg = raster(&c, RasterKind::Value(1.0), "deg");
        assert!(!svg.contains("-0.00"), "{svg}");
        assert!(svg.contains(": 0.00 deg</title>") && svg.contains(": +0.50 deg</title>"));
    }

    /// The one-cycle chart's hover and the report's rate read without a
    /// sign at zero (Trevor's review of #746).
    #[test]
    fn the_cycle_hover_and_the_rate_at_zero_read_without_a_sign() {
        let bins = 10;
        let c = Component {
            period_s: 10.0,
            resolution_s: 0.0,
            significance: 0.0,
            beyond_simulated: false,
            harmonics: 1,
            explained: 0.0,
            peak_to_peak: 0.0,
            wheel: None,
            nearest_wheel: None,
            shape: vec![0.0; bins],
            fold: Fold {
                period_s: 10.0,
                profile: vec![0.0; bins],
                counts: vec![4; bins],
                raster: vec![vec![0.0; bins]; 4],
                t0: 0.0,
            },
            harmonic_of: None,
        };
        let points: Vec<f64> = (0..bins)
            .map(|j| if j % 2 == 0 { -1e-4 } else { 0.5 })
            .collect();
        let svg = component(
            &c,
            "amplitude",
            String::new(),
            &c.shape,
            Some(&points),
            "deg",
            RasterKind::Value(1.0),
        );
        assert!(!svg.contains("-0.00"), "{svg}");
        assert!(svg.contains(": 0.00 deg (median over cycles)"), "{svg}");
        assert!(svg.contains(": +0.50 deg (median over cycles)"), "{svg}");
        assert_eq!(rate_text(-0.001), "0.00 s/d");
        assert_eq!(rate_text(1.234), "+1.23 s/d");
    }
}
