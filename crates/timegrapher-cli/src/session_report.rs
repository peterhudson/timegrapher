//! A self-contained HTML report for a test session: the positions side
//! by side, Witschi's characteristic values, and the findings with their
//! evidence. Inline SVG, no scripts, light and dark themes.

use crate::session_cmd::{opt, severity, signed, wind, Session};
use std::fmt::Write;
use timegrapher_core::session::{Mark, Position, Reading, Severity, StateIndices, Tolerance};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn cell(text: String, m: Mark) -> String {
    match m {
        Mark::Outside => format!(r#"<td class="num out" title="outside tolerance">{text}</td>"#),
        _ => format!(r#"<td class="num">{text}</td>"#),
    }
}

const W: f64 = 760.0;
const H: f64 = 230.0;
const ML: f64 = 56.0;
const MR: f64 = 16.0;
const MT: f64 = 14.0;
const MB: f64 = 44.0;

fn nice_step(span: f64) -> f64 {
    let raw = span.max(1e-9) / 5.0;
    let mag = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|&s| s >= raw)
        .unwrap_or(10.0 * mag)
}

struct Series<'a> {
    class: &'a str,
    label: &'a str,
    /// (position, value)
    points: Vec<(Position, f64)>,
}

/// Values per position as dots, with tolerance bands behind them.
fn position_chart(
    unit: &str,
    series: &[Series],
    bands: &[(Option<bool>, f64, f64)],
    zero: bool,
) -> String {
    let vals: Vec<f64> = series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.1))
        .chain(bands.iter().flat_map(|b| [b.1, b.2]))
        .chain(zero.then_some(0.0))
        .collect();
    if series.iter().all(|s| s.points.is_empty()) {
        return String::new();
    }
    let lo = vals.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let pad = ((hi - lo) * 0.08).max(5.0);
    let (lo, hi) = (lo - pad, hi + pad);
    let step = nice_step(hi - lo);
    let (lo, hi) = ((lo / step).floor() * step, (hi / step).ceil() * step);
    let pw = W - ML - MR;
    let ph = H - MT - MB;
    let y = |v: f64| MT + ph * (1.0 - (v - lo) / (hi - lo));
    let col = pw / Position::ALL.len() as f64;
    let x = |p: Position| {
        let i = Position::ALL.iter().position(|q| *q == p).unwrap_or(0);
        ML + col * (i as f64 + 0.5)
    };
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg viewBox="0 0 {W} {H}" role="img" aria-label="{unit} by position">"#
    );
    // Bands: Some(false) horizontal positions only, Some(true) vertical only.
    for &(vert, a, b) in bands {
        for p in Position::ALL {
            if vert.is_some_and(|v| v != p.is_vertical()) {
                continue;
            }
            let _ = write!(
                s,
                r#"<rect class="band" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}"/>"#,
                x(p) - col / 2.0 + 2.0,
                y(b),
                col - 4.0,
                (y(a) - y(b)).max(0.5)
            );
        }
    }
    let mut t = lo;
    while t <= hi + 1e-9 {
        let _ = write!(
            s,
            r#"<line class="grid" x1="{ML}" x2="{:.1}" y1="{:.1}" y2="{:.1}"/><text class="tick" x="{:.1}" y="{:.1}" text-anchor="end">{}</text>"#,
            W - MR,
            y(t),
            y(t),
            ML - 6.0,
            y(t) + 4.0,
            if step < 1.0 {
                format!("{t:.1}")
            } else {
                format!("{t:.0}")
            }
        );
        t += step;
    }
    if zero && lo < 0.0 && hi > 0.0 {
        let _ = write!(
            s,
            r#"<line class="axis" x1="{ML}" x2="{:.1}" y1="{:.1}" y2="{:.1}"/>"#,
            W - MR,
            y(0.0),
            y(0.0)
        );
    }
    for p in Position::ALL {
        let _ = write!(
            s,
            r#"<text class="tick" x="{:.1}" y="{:.1}" text-anchor="middle">{}</text><text class="label" x="{:.1}" y="{:.1}" text-anchor="middle">{}</text>"#,
            x(p),
            H - MB + 16.0,
            p.code(),
            x(p),
            H - MB + 31.0,
            p.description()
        );
    }
    let _ = write!(
        s,
        r#"<text class="label" transform="translate(14,{:.1}) rotate(-90)" text-anchor="middle">{}</text>"#,
        MT + ph / 2.0,
        esc(unit)
    );
    let n = series.len().max(1) as f64;
    for (k, ser) in series.iter().enumerate() {
        let dx = (k as f64 - (n - 1.0) / 2.0) * 14.0;
        for &(p, v) in &ser.points {
            let (cx, cy) = (x(p) + dx, y(v));
            if ser.class == "ref" {
                let _ = write!(
                    s,
                    r#"<path class="ref" d="M{:.1} {:.1} l6 6 l-6 6 l-6 -6 z"><title>{}: {v:.1} {}</title></path>"#,
                    cx,
                    cy - 6.0,
                    esc(ser.label),
                    esc(unit)
                );
            } else {
                let _ = write!(
                    s,
                    r#"<circle class="{}" cx="{cx:.1}" cy="{cy:.1}" r="5"><title>{} {p}: {v:.1} {}</title></circle>"#,
                    ser.class,
                    esc(ser.label),
                    esc(unit)
                );
            }
        }
    }
    s.push_str("</svg>");
    if series.len() > 1 {
        s.push_str(r#"<p class="legend">"#);
        for ser in series {
            let sw = if ser.class == "ref" {
                r#"<svg width="12" height="12" viewBox="0 0 12 12"><path class="ref" d="M6 0 l6 6 l-6 6 l-6 -6 z"/></svg>"#.to_string()
            } else {
                format!(
                    r#"<svg width="12" height="12" viewBox="0 0 12 12"><circle class="{}" cx="6" cy="6" r="5"/></svg>"#,
                    ser.class
                )
            };
            let _ = write!(s, r#"<span>{sw} {}</span>"#, esc(ser.label));
        }
        s.push_str("</p>");
    }
    s
}

fn charts(st: &StateIndices, readings: &[&Reading], tol: &Tolerance) -> String {
    let refs = |f: &dyn Fn(&Reading) -> Option<f64>| -> Vec<(Position, f64)> {
        readings
            .iter()
            .filter_map(|r| f(r).map(|v| (r.position, v)))
            .collect()
    };
    let rate_pts: Vec<(Position, f64)> = st
        .positions
        .iter()
        .filter_map(|v| v.rate_s_per_day.map(|r| (v.position, r)))
        .collect();
    let amp_pts: Vec<(Position, f64)> = st
        .positions
        .iter()
        .filter_map(|v| v.amplitude_deg.map(|r| (v.position, r)))
        .collect();
    let rref = refs(&|r| r.reference.as_ref()?.rate_s_per_day);
    let aref = refs(&|r| r.reference.as_ref()?.amplitude_deg);
    let mut rs = vec![Series {
        class: "dot",
        label: "this recording",
        points: rate_pts,
    }];
    if !rref.is_empty() {
        rs.push(Series {
            class: "ref",
            label: "reference timegrapher",
            points: rref,
        });
    }
    let mut as_ = vec![Series {
        class: "dot",
        label: "this recording",
        points: amp_pts,
    }];
    if !aref.is_empty() {
        as_.push(Series {
            class: "ref",
            label: "reference timegrapher",
            points: aref,
        });
    }
    let full = st.wind_h <= 2.0;
    let rate_band = [(None, tol.rate_min, tol.rate_max)];
    let amp_band = [
        (Some(false), tol.amplitude_h.0, tol.amplitude_h.1),
        (Some(true), tol.amplitude_v.0, tol.amplitude_v.1),
    ];
    format!(
        r#"<h3>Rate by position</h3>{}<h3>Amplitude by position</h3>{}<p class="note">Shaded: {} tolerance{}.</p>"#,
        position_chart("s/d", &rs, if full { &rate_band } else { &[] }, true),
        position_chart("deg", &as_, if full { &amp_band } else { &[] }, false),
        esc(&tol.name),
        if full {
            ", fully wound"
        } else {
            " applies fully wound only, so not shown"
        }
    )
}

fn indices_table(states: &[StateIndices]) -> String {
    let mut s = String::from(r#"<table class="data"><thead><tr><th>Value</th>"#);
    for st in states {
        let _ = write!(
            s,
            r#"<th class="num">{}</th>"#,
            if st.wind_h <= 0.0 {
                "full wind".into()
            } else {
                format!("{:.0} h after winding", st.wind_h)
            }
        );
    }
    s.push_str("<th>Meaning</th></tr></thead><tbody>");
    type Row<'a> = (&'a str, &'a dyn Fn(&StateIndices) -> String, &'a str);
    let rows: [Row; 10] = [
        (
            "X",
            &|st| signed(st.x, 1),
            "mean rate over the positions, s/d",
        ),
        (
            "XH",
            &|st| signed(st.xh, 1),
            "mean rate, horizontal positions",
        ),
        (
            "XV",
            &|st| signed(st.xv, 1),
            "mean rate, vertical positions",
        ),
        (
            "D",
            &|st| opt(st.d_rate, 1),
            "largest rate difference between positions, s/d",
        ),
        (
            "DV",
            &|st| opt(st.dv_rate, 1),
            "the same over the vertical positions",
        ),
        (
            "DH",
            &|st| opt(st.dh_rate, 1),
            "the same over the horizontal positions",
        ),
        (
            "DVH",
            &|st| signed(st.dvh_rate, 1),
            "vertical mean minus horizontal mean, s/d",
        ),
        (
            "Di",
            &|st| signed(st.di, 1),
            "6H minus CH, s/d (short-term analogue of COSC's D)",
        ),
        (
            "Amplitude",
            &|st| opt(st.amplitude_mean, 0),
            "mean over the positions, deg",
        ),
        (
            "Amplitude D / DVH",
            &|st| {
                format!(
                    "{} / {}",
                    opt(st.d_amplitude, 0),
                    signed(st.dvh_amplitude, 0)
                )
            },
            "largest difference, and vertical minus horizontal, deg",
        ),
    ];
    for (name, f, what) in rows {
        let _ = write!(s, "<tr><th>{name}</th>");
        for st in states {
            let _ = write!(s, r#"<td class="num">{}</td>"#, f(st));
        }
        let _ = write!(s, r#"<td class="what">{what}</td></tr>"#);
    }
    s.push_str("</tbody></table>");
    s
}

fn readings_table(sn: &Session) -> String {
    let mut s = String::from(
        r#"<table class="data"><thead><tr><th>Position</th><th>Wind</th><th class="num">Rate<br>s/d</th><th class="num">Amplitude<br>deg</th><th class="num">Beat error<br>ms</th><th class="num">Jitter<br>µs</th><th class="num">10 s rates<br>s/d</th><th class="num">Measured</th><th>Recording</th></tr></thead><tbody>"#,
    );
    for (r, v) in sn.readings.iter().zip(&sn.report.verdicts) {
        let m = &r.measurement;
        let _ = write!(
            s,
            r#"<tr><th>{} <span class="sub">{}</span></th><td>{}</td>{}{}{}<td class="num">{}</td><td class="num">{} to {}</td><td class="num">{}</td><td class="file">{}{}</td></tr>"#,
            r.position.code(),
            r.position.description(),
            wind(r.wind_h),
            cell(signed(m.rate_s_per_day, 1), v.rate),
            cell(opt(m.amplitude_deg, 0), v.amplitude),
            cell(opt(m.beat_error_ms.map(f64::abs), 2), v.beat_error),
            opt(m.jitter_us, 0),
            signed(m.rate_p05, 0),
            signed(m.rate_p95, 0),
            duration(m.end_s - m.start_s),
            esc(&r.label),
            if m.calibrated {
                ""
            } else {
                r#" <span class="sub">card clock</span>"#
            }
        );
    }
    s.push_str("</tbody></table>");
    s
}

fn duration(s: f64) -> String {
    let s = s.round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn reference_table(sn: &Session) -> String {
    let with: Vec<&Reading> = sn
        .readings
        .iter()
        .filter(|r| r.reference.is_some())
        .collect();
    if with.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        r#"<h2>Against the reference timegrapher</h2><table class="data"><thead><tr><th>Position</th><th class="num">Rate s/d<br>here / ref</th><th class="num">Difference</th><th class="num">Amplitude deg<br>here / ref</th><th class="num">Difference</th><th class="num">Beat error ms<br>here / ref</th><th>Reference</th><th>Recording</th></tr></thead><tbody>"#,
    );
    for r in with {
        let f = r.reference.as_ref().unwrap();
        let m = &r.measurement;
        let be = m.beat_error_ms.map(f64::abs);
        let _ = write!(
            s,
            r#"<tr><th>{}</th><td class="num">{} / {}</td><td class="num">{}</td><td class="num">{} / {}</td><td class="num">{}</td><td class="num">{} / {}</td><td>{}</td><td class="file">{}</td></tr>"#,
            r.position.code(),
            signed(m.rate_s_per_day, 1),
            signed(f.rate_s_per_day, 1),
            signed(
                m.rate_s_per_day.zip(f.rate_s_per_day).map(|(a, b)| a - b),
                1
            ),
            opt(m.amplitude_deg, 0),
            opt(f.amplitude_deg, 0),
            signed(m.amplitude_deg.zip(f.amplitude_deg).map(|(a, b)| a - b), 0),
            opt(be, 2),
            opt(f.beat_error_ms, 2),
            esc(f.source.as_deref().unwrap_or("")),
            esc(&r.label)
        );
    }
    s.push_str(r#"</tbody></table><p class="note">The reading here is the whole recording after settling; a bench timegrapher shows its average over a few seconds, so a watch whose rate wanders can differ by a few s/d from either.</p>"#);
    s
}

fn cycles_table(sn: &Session) -> String {
    let rows: Vec<(&Reading, &timegrapher_core::session::Cycle)> = sn
        .readings
        .iter()
        .flat_map(|r| r.cycles.iter().map(move |c| (r, c)))
        .collect();
    let mut s = String::from("<h2>Periodic changes</h2>");
    if rows.is_empty() {
        s.push_str("<p>No periodic change in rate or amplitude above the 1% false-alarm level in any recording.</p>");
        return s;
    }
    s.push_str(r#"<table class="data"><thead><tr><th>Position</th><th>In</th><th class="num">Period s</th><th class="num">Size p-p</th><th class="num">Share</th><th class="num">False alarm</th><th>Wheel</th><th>Recording</th></tr></thead><tbody>"#);
    for (r, c) in rows {
        let wheel = match (&c.wheel, &c.nearest_wheel) {
            (Some(w), _) => w.clone(),
            (None, Some((w, off))) => format!("none; nearest {w} {off:+.1}%"),
            _ => String::new(),
        };
        let _ = write!(
            s,
            r#"<tr><th>{}</th><td>{}</td><td class="num">{:.2}</td><td class="num">{:.1} {}</td><td class="num">{:.0}%</td><td class="num">{:.0e}</td><td>{}</td><td class="file">{}</td></tr>"#,
            r.position.code(),
            c.series,
            c.period_s,
            c.size,
            c.unit(),
            c.explained * 100.0,
            10f64.powf(-c.significance.min(300.0)),
            esc(&wheel),
            esc(&r.label)
        );
    }
    s.push_str(r#"</tbody></table><p class="note">For the full picture of a cycle (its shape and a raster of every turn), run <code>timegrapher long</code> on that recording.</p>"#);
    s
}

fn shape_table(sn: &Session) -> String {
    let with: Vec<&Reading> = sn.readings.iter().filter(|r| r.shape.is_some()).collect();
    if with.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        r#"<h2>Beat shape</h2><p class="note">From the first minute after settling. Even and odd beats are the two pallet stones. Intervals are between the halfway rises of the unlock (1), impulse (2) and drop (3); levels are against the drop.</p><table class="data"><thead><tr><th>Position</th><th>Side</th><th class="num">1 to 2<br>ms</th><th class="num">1 to 3<br>ms</th><th class="num">Level<br>1:3</th><th class="num">Level<br>2:3</th><th class="num">Noise between<br>beats</th><th class="num">Unlock<br>found</th><th class="num">Extra sounds<br>before / after</th></tr></thead><tbody>"#,
    );
    for r in with {
        let sh = r.shape.as_ref().unwrap();
        for (k, (side, v)) in [("even", &sh.even), ("odd", &sh.odd)]
            .into_iter()
            .enumerate()
        {
            let _ = write!(
                s,
                r#"<tr><th>{}</th><td>{side}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{:.0}%</td><td class="num">{} / {}</td></tr>"#,
                if k == 0 { r.position.code() } else { "" },
                opt(v.i12_ms, 2),
                opt(v.i13_ms, 2),
                opt(v.ratio13, 2),
                opt(v.ratio23, 2),
                opt(v.noise_ratio, 2),
                v.sound1_found * 100.0,
                v.extra_pre.len(),
                v.extra_post.len()
            );
        }
    }
    s.push_str("</tbody></table>");
    s
}

pub fn html(sn: &Session) -> String {
    let r = &sn.report;
    let title = match (sn.watch, sn.calibre) {
        (Some(w), Some(c)) => format!("{w}, calibre {c}"),
        (Some(w), None) => w.to_string(),
        (None, Some(c)) => format!("Calibre {c}"),
        (None, None) => "Test session".to_string(),
    };
    let mut body = String::new();
    let t = &r.tolerance;
    let positions: Vec<&str> = {
        let mut p: Vec<Position> = sn.readings.iter().map(|r| r.position).collect();
        p.sort();
        p.dedup();
        p.into_iter().map(|p| p.code()).collect()
    };
    let calibrated = sn
        .readings
        .iter()
        .filter(|r| r.measurement.calibrated)
        .count();
    let _ = write!(
        body,
        r#"<table class="facts">
<tr><th>Movement</th><td>{} bph, lift angle {}°</td></tr>
<tr><th>Recordings</th><td>{} in {} position(s): {}</td></tr>
<tr><th>Clock</th><td>{}</td></tr>
<tr><th>Tolerance</th><td>{}: {:+.0} to {:+.0} s/d; amplitude {:.0}–{:.0}° horizontal, {:.0}–{:.0}° vertical; beat error under {} ms (fully wound)</td></tr>
{}</table>"#,
        sn.bph,
        sn.lift_deg,
        sn.readings.len(),
        positions.len(),
        positions.join(", "),
        match calibrated {
            0 => "rates are on the sound card's clock (uncalibrated), good to a few s/d".to_string(),
            n if n == sn.readings.len() => "every rate is corrected for the sound card's clock".to_string(),
            n => format!("{n} of {} rates corrected for the sound card's clock; the others are on the card's clock", sn.readings.len()),
        },
        esc(&t.name),
        t.rate_min,
        t.rate_max,
        t.amplitude_h.0,
        t.amplitude_h.1,
        t.amplitude_v.0,
        t.amplitude_v.1,
        t.beat_error_ms,
        sn.notes
            .map(|n| format!("<tr><th>Notes</th><td>{}</td></tr>", esc(n)))
            .unwrap_or_default()
    );

    body.push_str("<h2>Findings</h2>");
    if r.findings.is_empty() {
        body.push_str("<p>Nothing outside tolerance or the project's limits.</p>");
    } else {
        body.push_str(r#"<ul class="findings">"#);
        for f in &r.findings {
            let class = match f.severity {
                Severity::Fault => "fault",
                Severity::Warning => "warn",
                Severity::Note => "note",
            };
            let _ = write!(
                body,
                r#"<li class="{class}"><span class="tag">{}</span> <strong>{}</strong><br><span class="evidence">{}</span><br><span class="advice">{}</span></li>"#,
                severity(f.severity),
                esc(&f.title),
                esc(&f.evidence),
                esc(&f.advice)
            );
        }
        body.push_str("</ul>");
    }

    body.push_str("<h2>Readings</h2>");
    body.push_str(&readings_table(sn));
    body.push_str(r#"<p class="note">Red: outside tolerance. Each reading starts after the settling time and runs to the end of the recording (or the measuring time set in the session file). 10 s rates: the 5th to 95th percentile of rate over 10 s windows, a measure of how steady the rate is.</p>"#);

    body.push_str("<h2>Characteristic values</h2>");
    body.push_str(&indices_table(&r.states));
    if !r.isochronism.is_empty() {
        body.push_str(r#"<h3>Isochronism</h3><table class="data"><thead><tr><th>Position</th><th class="num">From</th><th class="num">To</th><th class="num">Rate change s/d</th><th class="num">Amplitude change deg</th></tr></thead><tbody>"#);
        for i in &r.isochronism {
            let _ = write!(
                body,
                r#"<tr><th>{}</th><td class="num">{:.0} h</td><td class="num">{:.0} h</td><td class="num">{}</td><td class="num">{}</td></tr>"#,
                i.position,
                i.from_wind_h,
                i.to_wind_h,
                signed(Some(i.rate_change), 1),
                signed(i.amplitude_change, 0)
            );
        }
        let _ = write!(
            body,
            r#"</tbody></table><p>Im {} s/d (largest, excluding 12H), Im* {} s/d (all positions), Ie {} s/d (change in mean rate), N {} (with Witschi's default thermal term 0.6).</p>"#,
            signed(r.im, 1),
            signed(r.im_all, 1),
            opt(r.ie, 1),
            opt(r.n, 1)
        );
    }
    for st in &r.states {
        if r.states.len() > 1 {
            let _ = write!(
                body,
                "<h3>{}</h3>",
                if st.wind_h <= 0.0 {
                    "Fully wound".to_string()
                } else {
                    format!("{:.0} h after winding", st.wind_h)
                }
            );
        }
        let rs: Vec<&Reading> = sn
            .readings
            .iter()
            .filter(|rd| {
                ((rd.wind_h.unwrap_or(0.0) * 10.0).round() / 10.0 - st.wind_h).abs() < 0.05
            })
            .collect();
        body.push_str(&charts(st, &rs, t));
    }

    body.push_str(&reference_table(sn));
    body.push_str(&cycles_table(sn));
    body.push_str(&shape_table(sn));

    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Test session report</title>
<style>
:root {{
  color-scheme: light;
  --surface: #fcfcfb; --text: #0b0b0b; --text2: #52514e; --muted: #8a8984; --rule: #e4e3df;
  --series: #2a78d6; --ref: #d97a1f; --band: #e3efe0; --bad: #c0302f; --warn: #b26b00; --badbg: #fbe9e7;
}}
@media (prefers-color-scheme: dark) {{
  :root:not([data-theme="light"]) {{
    color-scheme: dark;
    --surface: #1a1a19; --text: #ffffff; --text2: #c3c2b7; --muted: #8f8e86; --rule: #34342f;
    --series: #3987e5; --ref: #f0a050; --band: #22332a; --bad: #ff7b72; --warn: #f0b34a; --badbg: #3a2220;
  }}
}}
:root[data-theme="dark"] {{
  color-scheme: dark;
  --surface: #1a1a19; --text: #ffffff; --text2: #c3c2b7; --muted: #8f8e86; --rule: #34342f;
  --series: #3987e5; --ref: #f0a050; --band: #22332a; --bad: #ff7b72; --warn: #f0b34a; --badbg: #3a2220;
}}
body {{ background: var(--surface); color: var(--text); font: 15px/1.45 system-ui, sans-serif; margin: 0 auto; max-width: 820px; padding: 16px; }}
h1 {{ font-size: 22px; margin: 8px 0 4px; }} h2 {{ font-size: 18px; margin-top: 28px; }} h3 {{ font-size: 15px; margin: 18px 0 4px; }}
p {{ margin: 6px 0; }} .note {{ color: var(--text2); font-size: 13px; }}
svg {{ width: 100%; height: auto; display: block; }}
.legend {{ display: flex; gap: 16px; font-size: 13px; color: var(--text2); }} .legend svg {{ display: inline; width: 12px; height: 12px; vertical-align: -1px; }}
.facts {{ border-collapse: collapse; width: 100%; }} .facts th {{ text-align: left; color: var(--text2); font-weight: 600; padding: 3px 12px 3px 0; vertical-align: top; white-space: nowrap; }} .facts td {{ padding: 3px 0; }}
.scroll {{ overflow-x: auto; }}
.data {{ border-collapse: collapse; width: 100%; font-size: 14px; display: block; overflow-x: auto; }}
.data th, .data td {{ padding: 4px 8px; border-bottom: 1px solid var(--rule); text-align: left; vertical-align: top; }}
.data thead th {{ color: var(--text2); font-weight: 600; font-size: 12px; }}
.data .num {{ text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }}
.data .out {{ color: var(--bad); font-weight: 600; background: var(--badbg); }}
.data .file {{ color: var(--text2); font-size: 12px; word-break: break-all; }}
.data .what {{ color: var(--text2); font-size: 12px; }}
.sub {{ color: var(--muted); font-weight: 400; font-size: 12px; }}
.findings {{ list-style: none; padding: 0; }} .findings li {{ padding: 8px 0 8px 12px; border-left: 3px solid var(--muted); margin: 8px 0; }}
.findings .fault {{ border-color: var(--bad); }} .findings .warn {{ border-color: var(--warn); }}
.tag {{ font-size: 11px; font-weight: 700; letter-spacing: 0.04em; color: var(--text2); }}
.fault .tag {{ color: var(--bad); }} .warn .tag {{ color: var(--warn); }}
.evidence {{ font-variant-numeric: tabular-nums; }} .advice {{ color: var(--text2); font-size: 13px; }}
.grid {{ stroke: var(--rule); stroke-width: 1; }} .axis {{ stroke: var(--muted); stroke-width: 1; }}
.tick {{ fill: var(--text2); font-size: 11px; }} .label {{ fill: var(--text2); font-size: 11px; }}
.band {{ fill: var(--band); }}
.dot {{ fill: var(--series); }} .ref {{ fill: none; stroke: var(--ref); stroke-width: 2; }}
code {{ font-size: 13px; }}
</style></head><body>
<h1>{}</h1>
<p class="note">Multi-position test by timegrapher {}{}</p>
{body}
</body></html>
"#,
        esc(&title),
        env!("CARGO_PKG_VERSION"),
        sn.owner
            .map(|o| format!(" · {}", esc(o)))
            .unwrap_or_default()
    )
}
