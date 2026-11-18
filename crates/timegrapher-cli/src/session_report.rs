//! A self-contained HTML report for a test session: the positions side
//! by side, Witschi's characteristic values, and the conditions found in
//! the sound with their figures. Inline SVG, no scripts, light and dark
//! themes.

use crate::session_cmd::{opt, signed, wind, Session};
use std::fmt::Write;
use timegrapher_core::chart_scale;
use timegrapher_core::library::{fixed_text, signed_text};
use timegrapher_core::position_line::{Lane, PositionLine, StateLines};
use timegrapher_core::session::{Position, PositionGroup, Reading, ReferenceClock, StateIndices};
use timegrapher_core::steadiness::SeriesKind;
use timegrapher_core::twostate;

/// Text put into the page: core's one escaper (#1649).
use timegrapher_core::xml::escape as esc;

fn cell(text: String) -> String {
    format!(r#"<td class="num">{text}</td>"#)
}

const W: f64 = 760.0;
/// Room left of the plot for the lanes' names.
const ML: f64 = 150.0;
/// Room right of the plot for the last tick and the unit after it.
const MR: f64 = 64.0;
/// Each lane's height, and a group heading's.
const LANE_H: f64 = 22.0;
/// A row of average names above the plot, and the gap between the plot and
/// the nearer row: core's, as the app's named lines.
use chart_scale::{NAME_LANE_GAP as LANE_GAP, NAME_ROW_H as NAME_ROW};
/// The bottom axis' ticks.
const MB: f64 = 24.0;
/// About how wide a character of the 11 px labels is.
const CHAR_W: f64 = 6.2;

/// The position line core gives for one measure (#1729): each position a
/// dot on its own lane, Horizontal then Vertical; this test's averages as
/// grey dashed lines named above the plot; on a later state the full-wind
/// figure hollow, with an arrow to this test's; `refs` (a reference
/// timegrapher's figures, by position) as diamonds. Positions are never
/// joined.
fn position_line_svg(line: &PositionLine, refs: &[(Position, f64)], id: &str) -> String {
    let Some([lo0, hi0, step]) = line.axis else {
        return String::new();
    };
    // The axis also holds the reference figures.
    let (lo, hi) = refs.iter().fold((lo0, hi0), |(lo, hi), &(_, v)| {
        (
            lo.min((v / step).floor() * step),
            hi.max((v / step).ceil() * step),
        )
    });
    let pw = W - ML - MR;
    let x = move |v: f64| ML + pw * (v - lo) / (hi - lo);
    // Room above for the averages' names: a lane of two rows.
    let mt = 6.0
        + if line.averages.is_empty() {
            0.0
        } else {
            LANE_GAP + NAME_ROW
        };
    let groups = [PositionGroup::Horizontal, PositionGroup::Vertical];
    let rows: usize = groups
        .iter()
        .filter(|g| line.lanes.iter().any(|l| l.group == **g))
        .count()
        + line.lanes.len();
    let ph = LANE_H * rows as f64;
    let h = mt + ph + MB;
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg viewBox="0 0 {W} {h:.0}" role="img" aria-label="{} by position">"#,
        esc(line.title)
    );
    let _ = write!(
        s,
        r#"<defs><marker id="arrow-{id}" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 L8 4 L0 8 z" class="arrowhead"/></marker></defs>"#
    );
    value_axis(&mut s, [lo, hi, step], &x, mt, mt + ph, line);
    // The averages, under the dots.
    for a in &line.averages {
        let _ = write!(
            s,
            r#"<line class="average" x1="{0:.1}" x2="{0:.1}" y1="{mt:.1}" y2="{1:.1}"><title>{2}</title></line>"#,
            x(a.value),
            mt + ph,
            esc(&a.hover)
        );
    }
    average_names(&mut s, line, &x, mt);
    // The lanes, each group under its heading.
    let mut y = mt;
    for g in groups {
        let lanes: Vec<_> = line.lanes.iter().filter(|l| l.group == g).collect();
        if lanes.is_empty() {
            continue;
        }
        let _ = write!(
            s,
            r#"<text class="group" x="6" y="{:.1}">{}</text>"#,
            y + LANE_H / 2.0 + 4.0,
            g.name()
        );
        y += LANE_H;
        for l in lanes {
            lane_svg(&mut s, l, &x, y + LANE_H / 2.0, refs, (line.unit, id));
            y += LANE_H;
        }
    }
    s.push_str("</svg>");
    s.push_str(&position_key(line, refs));
    s
}

/// The averages' names in a lane of one row above the plot from `top`,
/// placed by core's named-lines rule ([`chart_scale::lane_place_row`], the
/// app's), each joined to its line by one straight leader, angled where
/// its name had to slide, so lines close together are told apart (Walter on #1742).
fn average_names(s: &mut String, line: &PositionLine, x: &dyn Fn(f64) -> f64, top: f64) {
    let names: Vec<(f64, f64, bool)> = line
        .averages
        .iter()
        .map(|a| (x(a.value), a.name.len() as f64 * CHAR_W, false))
        .collect();
    // One row: a plot that names only three lines slides a name along
    // rather than out a row, so no leader passes behind another name
    // (Walter on #1742).
    let placed: Vec<(f64, u8)> = chart_scale::lane_place_row(&names, ML + 2.0, W - MR - 2.0)
        .into_iter()
        .map(|a| (a, 0))
        .collect();
    // The leaders first, then each name on a box of the page's colour, as
    // the app draws its named lines.
    let mut boxes = String::new();
    for (a, (&(lx, w, _), &(left, row))) in line.averages.iter().zip(names.iter().zip(&placed)) {
        let off = LANE_GAP + f64::from(row) * NAME_ROW;
        let centre = left + w / 2.0;
        // One straight line, angled where the name slid (Hugh on #1747).
        let pts = [(centre, top - off - 1.0), (lx, top)];
        let pts: Vec<String> = pts
            .iter()
            .map(|(px, py)| format!("{px:.1},{py:.1}"))
            .collect();
        let _ = write!(
            s,
            r#"<polyline class="leader" points="{}"/>"#,
            pts.join(" ")
        );
        let _ = write!(
            boxes,
            r#"<g><title>{}</title><rect class="namebox" x="{:.1}" y="{:.1}" width="{:.1}" height="{NAME_ROW}"/><text class="label" x="{left:.1}" y="{:.1}">{}</text></g>"#,
            esc(&a.hover),
            left - 2.0,
            top - off - NAME_ROW,
            w + 4.0,
            top - off - 3.5,
            esc(a.name)
        );
    }
    s.push_str(&boxes);
}

/// The value grid down the plot from `top` to `bottom`, its ticks, and the
/// unit after the last tick's figure.
fn value_axis(
    s: &mut String,
    [lo, hi, step]: [f64; 3],
    x: &dyn Fn(f64) -> f64,
    top: f64,
    bottom: f64,
    line: &PositionLine,
) {
    // A rate's ticks carry their sign, a minus a true one: core's one rule
    // for a tick (Hugh on #1742; #1779).
    let signed = line.series == SeriesKind::Rate;
    let tick = |t: f64| chart_scale::axis_tick(t, step, signed);
    let ticks = ((hi - lo) / step).round() as usize;
    for i in 0..=ticks {
        let t = lo + i as f64 * step;
        let _ = write!(
            s,
            r#"<line class="grid" x1="{0:.1}" x2="{0:.1}" y1="{top:.1}" y2="{1:.1}"/><text class="tick" x="{0:.1}" y="{2:.1}" text-anchor="middle">{3}</text>"#,
            x(t),
            bottom,
            bottom + 15.0,
            tick(t)
        );
    }
    // The unit follows the last tick's figure.
    let last = tick(hi);
    let _ = write!(
        s,
        r#"<text class="label" x="{:.1}" y="{:.1}" text-anchor="start">{}</text>"#,
        // Degrees touch their figure; other units follow a space.
        W - MR
            + last.chars().count() as f64 * CHAR_W / 2.0
            + if line.unit == "°" { 1.0 } else { 4.0 },
        bottom + 15.0,
        esc(line.unit)
    );
}

/// One lane at height `cy`: its rule and name, the arrow from the full-wind
/// figure, the hollow full-wind dot, the reference diamond, this test's dot.
fn lane_svg(
    s: &mut String,
    l: &Lane,
    x: &dyn Fn(f64) -> f64,
    cy: f64,
    refs: &[(Position, f64)],
    (unit, id): (&str, &str),
) {
    let _ = write!(
        s,
        r#"<line class="grid" x1="{ML}" x2="{:.1}" y1="{cy:.1}" y2="{cy:.1}"/><text class="lane" x="{:.1}" y="{:.1}" text-anchor="end">{}</text>"#,
        W - MR,
        ML - 8.0,
        cy + 4.0,
        esc(&l.name)
    );
    if let (Some(e), Some(v)) = (l.full_wind, l.value) {
        // The arrow stops short of this test's dot.
        let (x0, x1) = (x(e), x(v));
        if (x1 - x0).abs() > 12.0 {
            let x1 = x1 - 7.0 * (x1 - x0).signum();
            let _ = write!(
                s,
                r#"<line class="move" x1="{x0:.1}" x2="{x1:.1}" y1="{cy:.1}" y2="{cy:.1}" marker-end="url(#arrow-{id})"/>"#
            );
        }
    }
    if let Some(e) = l.full_wind {
        let _ = write!(
            s,
            r#"<circle class="hollow" cx="{:.1}" cy="{cy:.1}" r="5"><title>{}</title></circle>"#,
            x(e),
            esc(&l.hover)
        );
    }
    if let Some(&(_, v)) = refs.iter().find(|(p, _)| p.common() == l.position) {
        let _ = write!(
            s,
            r#"<path class="ref" d="M{:.1} {:.1} l6 6 l-6 6 l-6 -6 z"><title>reference timegrapher: {} {}</title></path>"#,
            x(v),
            cy - 6.0,
            fixed_text(v, 1),
            esc(unit)
        );
    }
    if let Some(v) = l.value {
        let _ = write!(
            s,
            r#"<circle class="dot" cx="{:.1}" cy="{cy:.1}" r="5"><title>{}</title></circle>"#,
            x(v),
            esc(&l.hover)
        );
    }
}

/// The key under a position line: the solid and the hollow dot on a
/// comparison, and the reference diamond where there are references.
fn position_key(line: &PositionLine, refs: &[(Position, f64)]) -> String {
    let mut key = Vec::new();
    if let Some([this, full]) = line.key {
        key.push(format!(
            r#"<span><svg width="12" height="12" viewBox="0 0 12 12"><circle class="dot" cx="6" cy="6" r="5"/></svg> {this}</span><span><svg width="12" height="12" viewBox="0 0 12 12"><circle class="hollow" cx="6" cy="6" r="4.5"/></svg> {full}</span>"#
        ));
    }
    if !refs.is_empty() {
        if key.is_empty() {
            key.push(format!(
                r#"<span><svg width="12" height="12" viewBox="0 0 12 12"><circle class="dot" cx="6" cy="6" r="5"/></svg> {}</span>"#,
                timegrapher_core::position_line::THIS_TEST
            ));
        }
        key.push(r#"<span><svg width="12" height="12" viewBox="0 0 12 12"><path class="ref" d="M6 0 l6 6 l-6 6 l-6 -6 z"/></svg> reference timegrapher</span>"#.to_string());
    }
    if key.is_empty() {
        return String::new();
    }
    format!(r#"<p class="legend">{}</p>"#, key.join(""))
}

/// One state of wind's position lines, rate, amplitude and beat error,
/// with a reference timegrapher's rates and amplitudes where the readings
/// carry them.
fn charts(lines: &StateLines, readings: &[&Reading], n: usize) -> String {
    let refs = |f: &dyn Fn(&Reading) -> Option<f64>| -> Vec<(Position, f64)> {
        readings
            .iter()
            .filter_map(|r| f(r).map(|v| (r.position, v)))
            .collect()
    };
    // The position aggregates mix every reading at a position, so the
    // reference rates are overlaid only when every reading is on one clock
    // and every reference rate is on that clock too (`Reference::clock`);
    // all supplied values remain in the reference table.
    let calibrated = readings.iter().any(|r| r.measurement.calibrated);
    let one_clock = readings
        .iter()
        .all(|r| r.measurement.calibrated == calibrated);
    let rref = refs(&|r| r.reference.as_ref()?.rate_s_per_day);
    let matched = one_clock
        && readings.iter().all(|r| {
            r.reference
                .as_ref()
                .is_none_or(|f| f.rate_s_per_day.is_none() || f.same_clock_as(calibrated))
        });
    let omitted = !matched && !rref.is_empty();
    let aref = refs(&|r| r.reference.as_ref()?.amplitude_deg);
    format!(
        r#"<h3>Rate by position</h3>{}{}<h3>Amplitude by position</h3>{}<h3>Beat Error by position</h3>{}"#,
        position_line_svg(
            &lines.rate,
            if matched { &rref } else { &[] },
            &format!("s{n}-rate")
        ),
        if omitted {
            r#"<p class="note">Reference rates are omitted from this rate chart because its readings and the reference rates are not all on one clock (a clock-corrected reading beside a reference whose clock basis is not recorded or is the sound card's, or the other way round). Reference values remain in the table.</p>"#
        } else {
            ""
        },
        position_line_svg(&lines.amplitude, &aref, &format!("s{n}-amplitude")),
        position_line_svg(&lines.beat_error, &[], &format!("s{n}-beat-error")),
    )
}

fn indices_table(states: &[StateIndices]) -> String {
    let mut s = String::from(r#"<table class="data"><thead><tr><th>Value</th>"#);
    for st in states {
        let _ = write!(s, r#"<th class="num">{}</th>"#, wind_words(st.wind_h));
    }
    s.push_str("<th>Meaning</th></tr></thead><tbody>");
    type Row<'a> = (&'a str, &'a dyn Fn(&StateIndices) -> String, &'a str);
    let rows: [Row; 10] = [
        (
            "X",
            &|st| signed(st.x, 1),
            "mean rate over the positions, seconds per day",
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
            "largest rate difference between positions, seconds per day",
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
            "vertical mean minus horizontal mean, seconds per day",
        ),
        (
            "Di",
            &|st| signed(st.di, 1),
            "6H minus CH, seconds per day (short-term analogue of COSC's D)",
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
        r#"<table class="data"><thead><tr><th>Position</th><th title="Hours since the watch was fully wound (wind_h in the session file). Where it is not given, the reading is its own state, never taken as fully wound.">Wind</th><th class="num" title="Fitted over every beat: the average the hands keep.">Rate<br>s/d</th><th class="num" title="Median of the 10 s rates. Where the rate wanders with dips, as tg's median reading does, it sits above the fitted rate.">10 s median<br>s/d</th><th class="num">Amplitude<br>deg</th><th class="num">Tick / Tock<br>deg</th><th class="num" title="The size of the difference between Tick and Tock, from the unlock where it was found. Which beat is longer is not shown: it depends on the beat the analysis locked on first, so it can change from one run to the next.">Beat error<br>ms</th><th class="num" title="The same size, measured from the drop.">from the drop<br>ms</th><th class="num" title="How much each beat strays from a steady rhythm: the median beat-to-beat timing scatter over 10 s, in millionths of a second. A noisy recording raises it too.">Jitter<br>µs</th><th class="num">10 s rates<br>s/d</th><th class="num" title="Share of the 2 s windows that found the unlock on both sides. Amplitude and the beat error from the unlock rest on those windows.">Unlock<br>found</th><th class="num" title="Share of the beats with a clipped sample. Clipping flattens the loudest part of the tick.">Clipped</th><th class="num">Measured</th><th>Recording</th></tr></thead><tbody>"#,
    );
    for r in &sn.readings {
        let m = &r.measurement;
        let clock = if m.calibrated {
            ""
        } else {
            r#" <span class="sub">card clock</span>"#
        };
        if !m.watch_heard {
            let _ = write!(
                s,
                r#"<tr><th>{}</th><td>{}</td><td colspan="10">{}</td><td class="num">{}</td><td class="file">{}{clock}</td></tr>"#,
                r.position.display_label(),
                wind(r.wind_h),
                esc(&timegrapher_core::session::no_regular_beat_in(m, r.bph_set)),
                duration(m.measured_s()),
                esc(&r.label),
            );
            continue;
        }
        let _ = write!(
            s,
            r#"<tr><th>{}</th><td>{}</td>{}<td class="num">{}</td>{}<td class="num">{} / {}</td>{}<td class="num">{}</td><td class="num">{}</td><td class="num">{} to {}</td><td class="num">{}</td><td class="num">{}%</td><td class="num">{}</td><td class="file">{}{clock}</td></tr>"#,
            r.position.display_label(),
            wind(r.wind_h),
            cell(signed(m.rate_s_per_day, 1)),
            signed(m.rate_window_median_s_per_day, 1),
            cell(opt(m.amplitude_deg, 0)),
            opt(m.amplitude_even_deg, 0),
            opt(m.amplitude_odd_deg, 0),
            cell(opt(m.beat_error(), 2)),
            opt(m.beat_error_ms.map(f64::abs), 2),
            opt(m.jitter_us, 0),
            signed(m.rate_p05, 0),
            signed(m.rate_p95, 0),
            m.unlock_coverage
                .map_or("-".into(), |c| format!("{}%", fixed_text(100.0 * c, 0))),
            fixed_text(100.0 * m.clipped_fraction, 1),
            duration(m.measured_s()),
            esc(&r.label),
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
        r#"<h2>Against the reference timegrapher</h2><table class="data"><thead><tr><th>Position</th><th class="num">Rate s/d<br>here / ref</th><th class="num">Difference</th><th class="num">Amplitude deg<br>here / ref</th><th class="num">Difference</th><th class="num">Beat error ms<br>here / ref</th><th class="num">Here from<br>the drop</th><th>Reference</th><th>Recording</th></tr></thead><tbody>"#,
    );
    for r in with {
        let f = r.reference.as_ref().unwrap();
        let m = &r.measurement;
        let be = m.beat_error();
        let basis = if m.calibrated {
            "clock corrected"
        } else {
            "sound-card clock"
        };
        let rate_difference = if f.same_clock_as(m.calibrated) {
            signed(
                m.rate_s_per_day.zip(f.rate_s_per_day).map(|(a, b)| a - b),
                1,
            )
        } else {
            "—".to_string()
        };
        let ref_basis = match f.clock {
            Some(ReferenceClock::Own) => "reference, its own clock",
            Some(ReferenceClock::SoundCard) => "reference, sound-card clock",
            None => "reference as supplied",
        };
        let _ = write!(
            s,
            r#"<tr><th>{}</th><td class="num">{} <span class="sub">{}</span><br>/ {} <span class="sub">{}</span></td><td class="num">{}</td><td class="num">{} / {}</td><td class="num">{}</td><td class="num">{} / {}</td><td class="num">{}</td><td>{}</td><td class="file">{}</td></tr>"#,
            r.position.display_label(),
            signed(m.rate_s_per_day, 1),
            basis,
            signed(f.rate_s_per_day, 1),
            ref_basis,
            rate_difference,
            opt(m.amplitude_deg, 0),
            opt(f.amplitude_deg, 0),
            signed(m.amplitude_deg.zip(f.amplitude_deg).map(|(a, b)| a - b), 0),
            opt(be, 2),
            opt(f.beat_error_ms, 2),
            opt(m.beat_error_ms.map(f64::abs), 2),
            esc(f.source.as_deref().unwrap_or("")),
            esc(&r.label)
        );
    }
    s.push_str(r#"</tbody></table><p class="note">Rate difference is shown where the reference's clock is the reading's: a reference on its own clock beside a clock-corrected reading, one on the sound card's clock beside an uncorrected reading. It is omitted otherwise, as for a clock-corrected reading whose reference clock basis is not recorded. Where the basis is not recorded, an uncorrected reading's difference uses the supplied rate; check its clock basis against the source. Reference values are shown as supplied, and a reference's own clock error is not corrected here.</p><p class="note">The reading here is the whole recording after settling; a bench timegrapher shows its average over a few seconds, so a watch whose rate wanders can differ by a few seconds per day from either.</p>"#);
    s
}

fn cycles_table(sn: &Session) -> String {
    let rows: Vec<(&Reading, &timegrapher_core::session::Cycle)> = sn
        .readings
        .iter()
        .flat_map(|r| r.cycles.iter().map(move |c| (r, c)))
        .collect();
    let mut s = String::from("<h2>Periodic changes</h2>");
    // Not searched is not "none found" (#1880).
    if sn.input["settings"]["cycles"] == false {
        s.push_str("<p>Not searched for: the search for periodic changes was turned off for this session.</p>");
        return s;
    }
    if rows.is_empty() {
        s.push_str("<p>No periodic change in rate or amplitude named in any recording (1% false-alarm level, held across each recording's tests).</p>");
        return s;
    }
    s.push_str(r#"<table class="data"><thead><tr><th>Position</th><th>In</th><th class="num">Period s</th><th class="num">Size p-p</th><th class="num">Share</th><th class="num">False alarm</th><th>Wheel</th><th>Recording</th></tr></thead><tbody>"#);
    for (r, c) in rows {
        let wheel = match (&c.harmonic_of, &c.wheel, &c.nearest_wheel) {
            (Some(h), _, _) => format!(
                "harmonic of the {} s cycle (1/{} of its period)",
                fixed_text(h.period_s, 2),
                h.n
            ),
            (None, Some(w), _) => w.clone(),
            (None, None, Some((w, off))) => {
                format!("none; nearest {w} {}%", signed_text(*off, 1))
            }
            _ => String::new(),
        };
        let _ = write!(
            s,
            r#"<tr><th>{}</th><td>{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}%</td><td class="num">{}</td><td>{}</td><td class="file">{}</td></tr>"#,
            r.position.display_label(),
            c.series,
            fixed_text(c.period_s, 2),
            c.size_text(),
            fixed_text(c.explained * 100.0, 0),
            timegrapher_core::longterm::false_alarm_text(
                c.significance,
                c.beyond_simulated,
                |s| format!("{:.0e}", 10f64.powf(-s.min(300.0)))
            ),
            esc(&wheel),
            esc(&r.label)
        );
    }
    s.push_str(r#"</tbody></table><p class="note">For the full picture of a cycle (its shape and a raster of every turn), run <code>timegrapher long</code> on that recording.</p>"#);
    s
}

fn states_lines(sn: &Session) -> String {
    let mut s = String::from(
        r#"<h2>Two states</h2><p class="note">Whether amplitude (from 2 s windows) and rate (from 10 s readings) sit at two levels the watch switches between, rather than one level with noise and slow drift. Tested on 10 s medians with the slow drift taken off. ΔBIC is the evidence for two levels over one (10 or more is needed); separation is the gap between the levels in fitted widths (2 or more). Each line also says when the low level comes back on a fixed period, as a cycle does (see above), and when only Tick or only Tock carries the split.</p>"#,
    );
    for r in sn.readings.iter().filter(|r| r.measurement.watch_heard) {
        let m = &r.measurement;
        let mut line = twostate::describe(&m.amplitude_states, "amplitude");
        if let Some(c) = line.get(..1) {
            line = c.to_uppercase() + &line[1..];
        }
        let _ = write!(
            s,
            r#"<p class="states"><b>{}</b> {}; {}. <span class="file">{}</span></p>"#,
            r.position.display_label(),
            esc(&line),
            esc(&twostate::describe(&m.rate_states, "rate")),
            esc(&r.label)
        );
    }
    s
}

fn shape_table(sn: &Session) -> String {
    let with: Vec<&Reading> = sn.readings.iter().filter(|r| r.shape.is_some()).collect();
    if with.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        r#"<h2>Beat shape</h2><p class="note">From the first minute after settling. Tick and Tock are the two kinds of beat, one from each pallet stone (which is which can't be told from the sound). Intervals are between the halfway rises of the unlock (1), impulse (2) and drop (3); levels are against the drop.</p><table class="data"><thead><tr><th>Position</th><th>Side</th><th class="num">1 to 2<br>ms</th><th class="num">1 to 3<br>ms</th><th class="num">Level<br>1:3</th><th class="num">Level<br>2:3</th><th class="num">Noise between<br>beats</th><th class="num">Unlock<br>found</th><th class="num">Extra sounds<br>before / after</th></tr></thead><tbody>"#,
    );
    for r in with {
        let sh = r.shape.as_ref().unwrap();
        for (k, (side, v)) in [("Tick", &sh.even), ("Tock", &sh.odd)]
            .into_iter()
            .enumerate()
        {
            let _ = write!(
                s,
                r#"<tr><th>{}</th><td>{side}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}%</td><td class="num">{} / {}</td></tr>"#,
                if k == 0 {
                    r.position.display_label()
                } else {
                    String::new()
                },
                opt(v.i12_ms, 2),
                opt(v.i13_ms, 2),
                opt(v.ratio13, 2),
                opt(v.ratio23, 2),
                opt(v.noise_ratio, 2),
                fixed_text(v.sound1_found * 100.0, 0),
                v.extra_pre.len(),
                v.extra_post.len()
            );
        }
    }
    s.push_str("</tbody></table>");
    s
}

/// The facts row for a take kept as its analysis alone: no sound, and
/// what measured it.
fn kept_row(v: &serde_json::Value) -> String {
    if v.is_null() {
        return String::new();
    }
    let by = &v["measured_by"];
    let who = match (by["version"].as_str(), by["git_hash"].as_str()) {
        (Some(ver), Some(h)) if !h.is_empty() => format!(" by timegrapher {ver} ({h})"),
        (Some(ver), _) => format!(" by timegrapher {ver}"),
        _ => String::new(),
    };
    format!(
        "<tr><th>Sound</th><td>None kept: the take was kept as its analysis alone. Its beats, \
         amplitude and beat shape are as measured when it was saved{}.</td></tr>\n",
        esc(&who)
    )
}

pub fn html(sn: &Session) -> String {
    let title = match (sn.watch, sn.calibre) {
        (Some(w), Some(c)) => format!("{w}, calibre {c}"),
        (Some(w), None) => w.to_string(),
        (None, Some(c)) => format!("Calibre {c}"),
        (None, None) => "Test session".to_string(),
    };
    let mut body = facts_table(sn);
    body.push_str(&findings_list(sn));
    body.push_str(&readings_section(sn));
    body.push_str("<h2>Characteristic values</h2>");
    body.push_str(&indices_table(&sn.report.states));
    body.push_str(&isochronism_section(sn));
    body.push_str(&poise_section(sn));
    body.push_str(&state_charts(sn));
    body.push_str(&reference_table(sn));
    body.push_str(&cycles_table(sn));
    body.push_str(&states_lines(sn));
    body.push_str(&shape_table(sn));
    page(&title, sn.owner, &body)
}

/// The facts at the top: movement, recordings, clock, sound kept, notes.
fn facts_table(sn: &Session) -> String {
    let mut body = String::new();
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
<tr><th>Movement</th><td>{} bph, {}</td></tr>
<tr><th>Recordings</th><td>{} in {} position(s): {}</td></tr>
<tr><th>Clock</th><td>{}</td></tr>
{}{}</table>"#,
        sn.bph,
        sn.lift_text(),
        sn.readings.len(),
        positions.len(),
        positions.join(", "),
        match calibrated {
            0 => "rates are on the sound card's clock, not calibrated".to_string(),
            n if n == sn.readings.len() => "every rate is corrected for the sound card's clock".to_string(),
            n => format!("{n} of {} rates corrected for the sound card's clock; the others are on the card's clock", sn.readings.len()),
        },
        kept_row(&sn.input["settings"]["analysis_only"]),
        sn.notes
            .map(|n| format!("<tr><th>Notes</th><td>{}</td></tr>", esc(n)))
            .unwrap_or_default()
    );
    body
}

/// The findings, each with its evidence; nothing when there are none.
fn findings_list(sn: &Session) -> String {
    let r = &sn.report;
    let mut body = String::new();
    if !r.findings.is_empty() {
        body.push_str(r#"<h2>Findings</h2><ul class="findings">"#);
        for f in &r.findings {
            let _ = write!(
                body,
                r#"<li><strong>{}</strong><br><span class="evidence">{}</span></li>"#,
                esc(&f.title),
                esc(&f.evidence),
            );
        }
        body.push_str("</ul>");
    }
    body
}

/// The readings table, what was left out, and how a reading is made.
fn readings_section(sn: &Session) -> String {
    let mut body = String::new();
    body.push_str("<h2>Readings</h2>");
    body.push_str(&readings_table(sn));
    for l in &sn.left_out {
        body.push_str(&format!(
            r#"<p class="note">Left out: {}</p>"#,
            esc(&l.text())
        ));
    }
    body.push_str(&format!(r#"<p class="note">Each reading starts after the settling time and runs to the end of the recording (or the measuring time set in the session file). 10 s rates: the 5th to 95th percentile of rate over 10 s windows, a measure of how steady the rate is. Beat error is a size: which beat is longer is not shown, because it depends on the beat the analysis locked on first. The settling time is left out, as the quick check <code>record</code> prints leaves it out. That check uses the session file's <code>settle_s</code> (20 s when the file has none, as this report does) and none of <code>--settle</code>, <code>--bph</code>, <code>--lift</code>, <code>--calibre</code>, a reading's own <code>settle_s</code>, <code>measure_s</code>, <code>start_s</code>, <code>end_s</code>, a clock log, <code>card_ppm_fast</code> or a restart in listening, so a reading that uses one of these can differ from it.{}</p>"#, if sn.readings.iter().any(|r| r.measurement.calibrated) { " The sound card's clock error is corrected where a clock log or <code>card_ppm_fast</code> is given." } else { "" }));
    body
}

/// The isochronism table and Im, Im* and Ie; nothing without isochronism.
fn isochronism_section(sn: &Session) -> String {
    let r = &sn.report;
    let mut body = String::new();
    if !r.isochronism.is_empty() {
        body.push_str(r#"<h3>Isochronism</h3><table class="data"><thead><tr><th>Position</th><th class="num">From</th><th class="num">To</th><th class="num">Rate change<br>s/d</th><th class="num">Amplitude change<br>deg</th><th class="num">Beat error change<br>ms</th></tr></thead><tbody>"#);
        for i in &r.isochronism {
            let _ = write!(
                body,
                r#"<tr><th>{}</th><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td></tr>"#,
                i.position.display_label(),
                wind_words(Some(i.from_wind_h)),
                wind_words(Some(i.to_wind_h)),
                signed(Some(i.rate_change), 1),
                signed(i.amplitude_change, 0),
                signed(i.beat_error_change, 2)
            );
        }
        let _ = write!(
            body,
            r#"</tbody></table><p>Im {} seconds per day (largest, excluding 12H), Im* {} (all positions), Ie {} (change in mean rate).</p>"#,
            signed(r.im, 1),
            signed(r.im_all, 1),
            opt(r.ie, 1)
        );
    }
    body
}

/// The Dynamic Poise fit of each state of wind that gives one, in core's
/// words ([`timegrapher_core::poise::PoiseFit::text`]).
fn poise_section(sn: &Session) -> String {
    let mut body = String::new();
    for p in &sn.poise {
        let _ = write!(
            body,
            "<h3>Dynamic Poise, {}</h3><p>{}</p>",
            esc(&wind_words(p.wind_h)),
            esc(&p.fit.text)
        );
    }
    body
}

/// The rate and amplitude charts of each state of wind.
fn state_charts(sn: &Session) -> String {
    let r = &sn.report;
    let mut body = String::new();
    for (n, (st, lines)) in r.states.iter().zip(&r.position_lines).enumerate() {
        if r.states.len() > 1 {
            let _ = write!(body, "<h3>{}</h3>", esc(&lines.name));
        }
        let rs: Vec<&Reading> = sn
            .readings
            .iter()
            .filter(|rd| timegrapher_core::session::in_wind_state(rd.wind_h, st.wind_h))
            .collect();
        body.push_str(&charts(lines, &rs, n));
    }
    body
}

/// The whole page around `body`, titled `title`.
fn page(title: &str, owner: Option<&str>, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Test session report</title>
<style>
:root {{
  color-scheme: light;
  --surface: #fcfcfb; --text: #0b0b0b; --text2: #52514e; --muted: #8a8984; --rule: #e4e3df;
  --series: #2a78d6; --ref: #d97a1f;
}}
@media (prefers-color-scheme: dark) {{
  :root:not([data-theme="light"]) {{
    color-scheme: dark;
    --surface: #1a1a19; --text: #ffffff; --text2: #c3c2b7; --muted: #8f8e86; --rule: #34342f;
    --series: #3987e5; --ref: #f0a050;
  }}
}}
:root[data-theme="dark"] {{
  color-scheme: dark;
  --surface: #1a1a19; --text: #ffffff; --text2: #c3c2b7; --muted: #8f8e86; --rule: #34342f;
  --series: #3987e5; --ref: #f0a050;
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
.data .file {{ color: var(--text2); font-size: 12px; word-break: break-all; }}
.states {{ margin: 4px 0; }}
.states .file {{ color: var(--text2); font-size: 12px; margin-left: 6px; }}
.data .what {{ color: var(--text2); font-size: 12px; }}
.sub {{ color: var(--muted); font-weight: 400; font-size: 12px; }}
.findings {{ list-style: none; padding: 0; }} .findings li {{ padding: 8px 0 8px 12px; border-left: 3px solid var(--muted); margin: 8px 0; }}
.evidence {{ font-variant-numeric: tabular-nums; }}
.grid {{ stroke: var(--rule); stroke-width: 1; }} .axis {{ stroke: var(--muted); stroke-width: 1; }}
.tick {{ fill: var(--text2); font-size: 11px; }} .label {{ fill: var(--text2); font-size: 11px; }}
.dot {{ fill: var(--series); }} .ref {{ fill: none; stroke: var(--ref); stroke-width: 2; }}
.hollow {{ fill: var(--surface); stroke: var(--series); stroke-width: 1.5; }} .move {{ stroke: var(--text2); stroke-width: 1.2; }} .arrowhead {{ fill: var(--text2); }}
.average {{ stroke: var(--muted); stroke-width: 1.2; stroke-dasharray: 4 3; }} .leader {{ fill: none; stroke: var(--muted); stroke-width: 1; }} .namebox {{ fill: var(--surface); }}
.lane {{ fill: var(--text); font-size: 11px; }} .group {{ fill: var(--text2); font-size: 11px; font-weight: 600; }}
code {{ font-size: 13px; }}
</style></head><body>
<h1>{}</h1>
<p class="note">Multi-position test by timegrapher {}{}</p>
{body}
</body></html>
"#,
        esc(title),
        env!("CARGO_PKG_VERSION"),
        owner.map(|o| format!(" · {}", esc(o))).unwrap_or_default()
    )
}

/// A state of wind in a table cell: "full wind", "24 h after winding" or
/// "wind not given".
fn wind_words(w: Option<f64>) -> String {
    timegrapher_core::reading_text::wind_state_words(w)
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "tests build the clock they expect by hand"
)]
mod tests {
    use super::*;
    use timegrapher_core::{
        beats::Beat,
        session::{self, Reference},
        stream::BeatLog,
    };

    fn reading(ppm: Option<f64>, position: Position) -> Reading {
        let log = BeatLog {
            lock_template_kept: false,
            sample_rate: 48000,
            duration_s: 60.0,
            bph: 28800,
            lift_deg: 52.0,
            beats: (0..480)
                .map(|index| Beat {
                    index,
                    time: index as f64 / 8.0,
                    quality: 1.0,
                })
                .collect(),
            amplitude_windows: vec![],
            clipped_beats: vec![],
            clipped_samples: 0,
            jumps: vec![],
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        };
        let clock = ppm.and_then(|ppm| session::fixed_clock(ppm, log.duration_s));
        let mut measurement = session::measure(&log, clock.as_ref(), 0.0, log.duration_s);
        assert!(measurement.watch_heard);
        assert!(measurement.rate_s_per_day.unwrap().is_finite());
        // Independent table controls; these numbers do not test amplitude estimation.
        measurement.amplitude_deg = Some(260.0);
        Reading {
            bph_set: Some(28800),
            label: format!("{}.wav", position.code()),
            position,
            wind_h: None,
            date: None,
            notes: None,
            duration_s: log.duration_s,
            bph: log.bph,
            measurement,
            cycles: vec![],
            shape: None,
            reference: Some(Reference {
                rate_s_per_day: Some(0.0),
                amplitude_deg: Some(250.0),
                beat_error_ms: None,
                source: Some("tg on the same file".into()),
                clock: None,
            }),
        }
    }

    fn render(readings: Vec<Reading>) -> String {
        html(&session_of(readings))
    }

    fn session_of(readings: Vec<Reading>) -> Session<'static> {
        let report = session::evaluate(&readings);
        Session {
            schema: "timegrapher.session/3".into(),
            software: serde_json::json!({}),
            input: serde_json::json!({}),
            watch: None,
            calibre: None,
            owner: None,
            notes: None,
            bph: 28800,
            lift_deg: 52.0,
            lift_known: true,
            left_out: Vec::new(),
            poise: timegrapher_core::poise::by_state(&readings),
            text_reports: Vec::new(),
            report,
            power_reserve: None,
            readings,
        }
    }

    /// A lift angle that is only the default is said to be the default,
    /// in core's caveat wording, in both the text and the HTML report
    /// (issue #603); one set for the watch is shown plainly.
    /// Eight upright readings give the Dynamic Poise fit, in core's words,
    /// as the app's pane does (#1474); six and two flat give none.
    #[test]
    fn upright_readings_give_the_poise_fit() {
        let upright: Vec<Reading> = Position::TEN
            .iter()
            .filter_map(|&p| {
                let deg = p.top_deg()?;
                let mut r = reading(None, p);
                r.wind_h = Some(0.0);
                r.measurement.rate_s_per_day = Some(4.0 + 6.0 * (deg - 90.0).to_radians().cos());
                r.measurement.amplitude_deg = Some(180.0);
                Some(r)
            })
            .collect();
        assert_eq!(upright.len(), 8);
        let s = session_of(upright);
        assert_eq!(s.poise.len(), 1);
        let p = &s.poise[0];
        assert_eq!(p.wind_h, Some(0.0));
        assert!((p.fit.wave.peak_deg.value - 90.0).abs() < 1e-6, "{p:?}");
        let page = html(&s);
        assert!(page.contains("<h3>Dynamic Poise, full wind</h3>"), "{page}");
        assert!(page.contains(&esc(&p.fit.text)));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["poise"][0]["fit"]["positions"], 8);
        // 24.0 h and 24.04 h are one state, as the characteristic values
        // name them (wind to a tenth): one fit, named 24 h, beside them.
        let mut later = s.readings.clone();
        for (i, r) in later.iter_mut().enumerate() {
            r.wind_h = Some(if i % 2 == 0 { 24.0 } else { 24.04 });
        }
        let later = session_of(later);
        assert_eq!(later.report.states.len(), 1);
        assert_eq!(later.poise.len(), 1);
        assert_eq!(later.poise[0].wind_h, later.report.states[0].wind_h);
        assert_eq!(later.poise[0].wind_h, Some(24.0));
        assert_eq!(later.poise[0].fit.positions, 8);
        let six = session_of(Position::ALL.iter().map(|&p| reading(None, p)).collect());
        assert!(six.poise.is_empty() && !html(&six).contains("Dynamic Poise"));
        assert!(serde_json::to_value(&six).unwrap().get("poise").is_none());
    }

    /// A search turned off is said to be off, never "none found" (#1880).
    #[test]
    fn cycles_not_searched_are_not_none_found() {
        let mut s = session_of(vec![reading(Some(0.0), Position::CH)]);
        let searched = html(&s);
        assert!(searched.contains("No periodic change"), "{searched}");
        s.input = serde_json::json!({ "settings": { "cycles": false } });
        let off = html(&s);
        assert!(
            off.contains("Not searched for") && !off.contains("No periodic change"),
            "{off}"
        );
    }

    #[test]
    fn default_lift_says_so() {
        let known = session_of(vec![reading(Some(0.0), Position::CH)]);
        assert_eq!(known.lift_text(), "lift angle 52°");
        assert!(html(&known).contains("<td>28800 bph, lift angle 52°</td>"));
        let unknown = Session {
            lift_known: false,
            ..known
        };
        let words = timegrapher_core::measure::Caveat::LiftUnknown
            .short(52.0, 0)
            .unwrap();
        assert_eq!(words, "lift angle 52°, the default");
        assert_eq!(unknown.lift_text(), words);
        assert!(html(&unknown).contains(&format!("<td>28800 bph, {words}</td>")));
    }

    fn reference_cells(html: &str) -> Vec<Vec<&str>> {
        html.split_once("<h2>Against the reference timegrapher</h2>")
            .unwrap()
            .1
            .split_once("<tbody>")
            .unwrap()
            .1
            .split_once("</tbody>")
            .unwrap()
            .0
            .split("<tr>")
            .skip(1)
            .map(|row| {
                row.split("<td")
                    .skip(1)
                    .map(|cell| {
                        cell.split_once('>')
                            .unwrap()
                            .1
                            .split_once("</td>")
                            .unwrap()
                            .0
                    })
                    .collect()
            })
            .collect()
    }

    fn rate_chart(html: &str) -> &str {
        html.split_once("<h3>Rate by position</h3>")
            .unwrap()
            .1
            .split_once("<h3>Amplitude by position</h3>")
            .unwrap()
            .0
    }

    /// A cycle core marks as a harmonic says so in the table, beside the
    /// cycle it is a harmonic of (#762).
    #[test]
    fn the_cycles_table_names_a_harmonic_by_its_cycle() {
        let cycle = |period_s: f64, wheel: Option<&str>| session::Cycle {
            series: "rate",
            period_s,
            size: 2.0,
            size_unit: "s/d",
            significance: 8.0,
            beyond_simulated: false,
            explained: 0.3,
            wheel: wheel.map(str::to_string),
            nearest_wheel: None,
            fraction_of: None,
            harmonic_of: None,
        };
        let mut r = reading(None, Position::CH);
        r.cycles = vec![
            cycle(60.19, Some("fourth wheel")),
            session::Cycle {
                harmonic_of: Some(timegrapher_core::longterm::Harmonic {
                    period_s: 60.19,
                    n: 2,
                }),
                ..cycle(30.5, None)
            },
        ];
        let html = render(vec![r]);
        assert!(html.contains("<td>fourth wheel</td>"), "{html}");
        assert!(
            html.contains("<td>harmonic of the 60.19 s cycle (1/2 of its period)</td>"),
            "{html}"
        );
    }

    #[test]
    fn nonzero_clock_keeps_corrected_rate_but_withholds_unsupported_reference_difference() {
        let r = reading(Some(-100.0), Position::CH);
        let measured = r.measurement.rate_s_per_day.unwrap();
        assert!(
            measured < -8.0 && measured > -9.0,
            "actual clock mapping must affect this fixture"
        );
        let shown = signed(Some(measured), 1);
        let html = render(vec![r]);
        println!("REPORT_HTML_START\n{html}\nREPORT_HTML_END");
        let cells = reference_cells(&html);
        assert!(cells[0][0].contains(&shown));
        // The reference reads zero, which takes no sign.
        assert!(cells[0][0].contains("/ 0.0 "));
        assert!(
            html.contains(&format!("<td class=\"num\">{shown}</td>")),
            "headline remains corrected"
        );
        assert_eq!(cells[0][1], "—");
        assert!(cells[0][0].contains("clock corrected"));
        assert!(cells[0][0].contains("reference as supplied"));
        assert!(html.contains("reference clock basis is not recorded"));
        assert!(!rate_chart(&html).contains("<path class=\"ref\""));
        assert!(rate_chart(&html).contains("Reference rates are omitted"));
        assert_eq!(cells[0][3], "+10");
        let amp = html
            .split_once("<h3>Amplitude by position</h3>")
            .unwrap()
            .1
            .split_once("</svg>")
            .unwrap()
            .0;
        assert!(
            amp.contains("<path class=\"ref\""),
            "amplitude overlay unchanged"
        );
    }

    #[test]
    fn zero_ppm_is_still_calibrated_without_declaring_a_reference_basis() {
        let r = reading(Some(0.0), Position::CH);
        assert!(r.measurement.calibrated);
        assert!(r.measurement.rate_s_per_day.unwrap().abs() < 1e-6);
        let html = render(vec![r]);
        assert_eq!(reference_cells(&html)[0][1], "—");
        assert!(!rate_chart(&html).contains("<path class=\"ref\""));
    }

    #[test]
    fn uncalibrated_difference_and_amplitude_overlay_are_preserved() {
        let mut r = reading(None, Position::CH);
        assert!(!r.measurement.calibrated);
        r.reference.as_mut().unwrap().rate_s_per_day = Some(2.0);
        let expected = signed(r.measurement.rate_s_per_day.map(|v| v - 2.0), 1);
        let html = render(vec![r]);
        let cells = reference_cells(&html);
        assert_eq!(cells[0][1], expected);
        assert_eq!(cells[0][3], "+10");
        assert!(rate_chart(&html).contains("<path class=\"ref\""));
        assert!(!rate_chart(&html).contains("Reference rates are omitted"));
    }

    #[test]
    fn a_mixed_chart_omits_all_rate_references_but_table_keeps_raw_comparison() {
        let corrected = reading(Some(-100.0), Position::CH);
        let raw = reading(None, Position::CB);
        let expected_raw = signed(raw.measurement.rate_s_per_day, 1);
        let html = render(vec![corrected, raw]);
        assert!(!rate_chart(&html).contains("<path class=\"ref\""));
        let cells = reference_cells(&html);
        assert_eq!(cells[0][1], "—");
        assert_eq!(cells[1][1], expected_raw);
        assert!(rate_chart(&html).contains("Reference rates are omitted"));
    }

    #[test]
    fn missing_values_remain_missing_without_inventing_a_difference() {
        let mut raw = reading(None, Position::CH);
        raw.measurement.rate_s_per_day = None;
        let html = render(vec![raw]);
        assert_eq!(reference_cells(&html)[0][1], "-");
        let mut corrected = reading(Some(-100.0), Position::CH);
        corrected.reference.as_mut().unwrap().rate_s_per_day = None;
        let html = render(vec![corrected]);
        assert_eq!(reference_cells(&html)[0][1], "—");
    }

    #[test]
    fn a_reference_on_its_own_clock_sits_beside_a_clock_corrected_reading() {
        let mut r = reading(Some(-100.0), Position::CH);
        let f = r.reference.as_mut().unwrap();
        f.rate_s_per_day = Some(2.0);
        f.clock = Some(ReferenceClock::Own);
        let expected = signed(r.measurement.rate_s_per_day.map(|v| v - 2.0), 1);
        let html = render(vec![r]);
        let cells = reference_cells(&html);
        assert_eq!(cells[0][1], expected);
        assert!(
            cells[0][0].contains("reference, its own clock"),
            "{}",
            cells[0][0]
        );
        assert!(rate_chart(&html).contains("<path class=\"ref\""));
        assert!(!rate_chart(&html).contains("Reference rates are omitted"));
        // Beside an uncorrected reading, the same reference is on another
        // clock: no difference, no overlay.
        let mut r = reading(None, Position::CH);
        r.reference.as_mut().unwrap().clock = Some(ReferenceClock::Own);
        let html = render(vec![r]);
        assert_eq!(reference_cells(&html)[0][1], "—");
        assert!(!rate_chart(&html).contains("<path class=\"ref\""));
        assert!(rate_chart(&html).contains("Reference rates are omitted"));
    }

    #[test]
    fn a_reference_on_the_sound_cards_clock_sits_beside_an_uncorrected_reading() {
        let mut r = reading(None, Position::CH);
        let f = r.reference.as_mut().unwrap();
        f.rate_s_per_day = Some(2.0);
        f.clock = Some(ReferenceClock::SoundCard);
        let expected = signed(r.measurement.rate_s_per_day.map(|v| v - 2.0), 1);
        let html = render(vec![r]);
        let cells = reference_cells(&html);
        assert_eq!(cells[0][1], expected);
        assert!(cells[0][0].contains("reference, sound-card clock"));
        assert!(rate_chart(&html).contains("<path class=\"ref\""));
        let mut r = reading(Some(-100.0), Position::CH);
        r.reference.as_mut().unwrap().clock = Some(ReferenceClock::SoundCard);
        let html = render(vec![r]);
        assert_eq!(reference_cells(&html)[0][1], "—");
        assert!(!rate_chart(&html).contains("<path class=\"ref\""));
    }

    #[test]
    fn a_reference_clock_is_read_from_the_session_file_and_given_back_as_set() {
        use timegrapher_core::session::Reference;
        let f: Reference = toml::from_str("rate = 1.5\nclock = \"own\"").unwrap();
        assert_eq!(f.clock, Some(ReferenceClock::Own));
        assert_eq!(serde_json::to_value(&f).unwrap()["clock"], "own");
        let f: Reference = toml::from_str("rate = 1.5\nclock = \"sound_card\"").unwrap();
        assert_eq!(f.clock, Some(ReferenceClock::SoundCard));
        let f: Reference = toml::from_str("rate = 1.5").unwrap();
        assert_eq!(f.clock, None);
        assert!(serde_json::to_value(&f).unwrap().get("clock").is_none());
        assert!(toml::from_str::<Reference>("rate = 1.5\nclock = \"quartz\"").is_err());
    }

    #[test]
    fn generic_reference_source_is_preserved_and_escaped() {
        let mut r = reading(None, Position::CH);
        r.reference.as_mut().unwrap().source = Some("No.1900 <bench> & \"source\"".into());
        let html = render(vec![r]);
        let cells = reference_cells(&html);
        assert_eq!(
            cells[0][6],
            "No.1900 &lt;bench&gt; &amp; &quot;source&quot;"
        );
        assert!(!html.contains("No.1900 <bench>"));
    }
}
