//! The plain-text reports a test can be saved as: the Positions Test's
//! six positions (#1508) and the Dynamic Poise test's eight upright stops
//! (#1509). Text only: no PDF, no logos, no formatting options. Each is
//! written once here, from core's own figures, and the app, the CLI and
//! the MCP server all emit this text.
//!
//! A report is a few heading lines about the watch and the test, the
//! table in the pane's order with its summary rows, then the facts under
//! it. Figures keep the ASCII minus, as text for a file does
//! ([`crate::figure_text::true_minus`]); the rest is UTF-8 (±, °, α). A
//! missing figure is a dash, and a missing fact's line is left out.
//! Sentences wrap at [`WIDTH`] columns, never inside a figure.

use crate::figure_text::{base_digits, fixed_text, grouped, signed_text};
use crate::measure::Caveat;
use crate::poise;
use crate::reading_text::wind_words_to;
use crate::session::{self, Measurement, Position, Reading, SourcedFigures, StateIndices};

/// The column the sentences wrap at (Hugh, 2026-11-14): a text file reads
/// in any editor and in an email at this width.
pub const WIDTH: usize = 72;

/// Decimals of the hours after full wind, a tenth, as the app's pane
/// and CSV write them.
const HOURS_DIGITS: usize = 1;

/// The file names `timegrapher session` writes the reports to, beside
/// `report.html`.
pub const POSITIONS_FILE: &str = "positions-report.txt";
pub const POISE_FILE: &str = "poise-report.txt";

/// A figure missing from a table.
const MISSING: &str = "-";

/// Two spaces between columns.
const GAP: &str = "  ";

/// What a report says of the watch and the test above its table. Every
/// field is left out when not known; nothing is filled in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Header {
    pub watch: Option<String>,
    /// When the take began, in its recording computer's time
    /// ([`take_date`]).
    pub date: Option<String>,
    pub calibre: Option<String>,
    pub bph: Option<u32>,
    pub lift_angle_deg: Option<f64>,
    pub settle_s: Option<f64>,
    pub measure_s: Option<f64>,
}

/// An earlier test the report's readings are set against: the full-wind
/// test's figures, and how many hours after it this one was read.
#[derive(Debug, Clone, Copy)]
pub struct Earlier<'a> {
    pub figures: &'a [SourcedFigures],
    pub hours: f64,
}

/// When a take began, as its folder's name records it (the app and
/// `timegrapher record` name a take by its start,
/// [`crate::library::TAKE_NAME_TIME`]): "2026-11-13 19:17". None for a
/// folder not so named.
pub fn take_date(folder_name: &str) -> Option<String> {
    crate::library::named_start(folder_name).map(|s| s.replace('T', " "))
}

/// The software line: "Timegrapher 1.0.1, engine 3fa2b9c1", the build
/// and the core source that read the figures ([`crate::library::engine`]).
pub fn software() -> String {
    let (version, source) = crate::library::engine();
    format!("Timegrapher {version}, engine {source}")
}

/// Each reading's figures, as core reads them: what the tables average.
pub fn figures(readings: &[Reading]) -> Vec<SourcedFigures> {
    readings
        .iter()
        .map(|r| {
            let m = &r.measurement;
            (
                r.position,
                m.rate_s_per_day,
                m.amplitude_deg,
                m.beat_error(),
                m.beat_error_source,
            )
        })
        .collect()
}

/// The Positions Test report: the heading, the six positions in the
/// test's order ([`Position::ALL`]) with their rate, rate less the
/// average, amplitude and beat error, then Average, Largest Difference,
/// vertical less horizontal, the comparison with the full-wind test when
/// `earlier` gives one, and the conditions found in the sound.
pub fn positions(header: &Header, readings: &[Reading], earlier: Option<Earlier>) -> String {
    positions_in(header, readings, earlier, &Position::ALL)
}

/// [`positions`] with the six rows in `order`, the order of the table the
/// report is of (an opened take's, in the card's order, #1705). `order`
/// holds the six; the figures under the rows don't depend on it.
pub fn positions_in(
    header: &Header,
    readings: &[Reading],
    earlier: Option<Earlier>,
    order: &[Position],
) -> String {
    positions_of(header, &figures(readings), earlier, order, readings)
}

/// [`positions_in`] from each position's figures, as a saved test keeps
/// them (an opened 24 hour half, #1706). `readings` give the notes the
/// conditions found in the sound; a saved test has none, so its notes say
/// only how the beat error was given.
pub fn positions_of(
    header: &Header,
    figures: &[SourcedFigures],
    earlier: Option<Earlier>,
    order: &[Position],
    readings: &[Reading],
) -> String {
    let mut out = heading("Positions Test", header);
    let ix = session::sourced_indices_over(0.0, figures, order);
    let mut rows = table_rows(&ix, "From Average");
    rows.push(mean_row("Average", &ix));
    rows.push(vec![
        "Largest Difference".into(),
        plain(ix.d_rate, base_digits::RATE),
        String::new(),
        plain(ix.d_amplitude, base_digits::AMPLITUDE),
        plain(ix.d_beat_error, base_digits::BEAT_ERROR),
    ]);
    out += &table(&rows, 2, 2);
    out.push('\n');
    out += &facts(&[
        (
            "Vertical less horizontal rate",
            unit(signed(ix.dvh_rate, base_digits::RATE), " s/d"),
        ),
        (
            "Vertical less horizontal amplitude",
            unit(signed(ix.dvh_amplitude, base_digits::AMPLITUDE), "°"),
        ),
    ]);
    if let Some(e) = earlier {
        out += &comparison(e, figures);
    }
    out += &notes(readings, &ix);
    out
}

/// The Dynamic Poise report: the heading, the eight upright stops in the
/// pane's order ([`Position::UPRIGHT`]) with the same columns as the
/// Positions Test's table and a Mean row, then core's full sentence on
/// the once-round wave and the heavy spot ([`poise::PoiseFit::sentence`])
/// with its caveats, and the conditions found in the sound. No picture.
pub fn poise(header: &Header, readings: &[Reading]) -> String {
    let mut out = heading("Dynamic Poise Test", header);
    let figures = figures(readings);
    let ix = session::sourced_indices_over(0.0, &figures, &Position::UPRIGHT);
    let mut rows = table_rows(&ix, "From Mean");
    rows.push(mean_row("Mean", &ix));
    out += &table(&rows, 2, 1);
    out.push('\n');
    let fit = poise::shown_fit(readings.iter().map(|r| (r.position, &r.measurement)));
    let words = fit.map_or_else(|| format!("{}.", poise::pending_text()), |f| f.sentence());
    out += &wrap(&words, "");
    out += &notes(readings, &ix);
    out
}

/// The heading: the test's name, then the watch and how it was read, a
/// line each, with the names lined up.
fn heading(title: &str, h: &Header) -> String {
    let mut lines: Vec<(&str, String)> = Vec::new();
    let mut add = |k, v: Option<String>| {
        if let Some(v) = v.filter(|v| !v.trim().is_empty()) {
            lines.push((k, v.trim().to_string()));
        }
    };
    add("Watch", h.watch.clone());
    add("Date", h.date.clone());
    add("Calibre", h.calibre.clone());
    add(
        "Beat Rate",
        h.bph.map(|b| format!("{} bph", grouped(u64::from(b)))),
    );
    add(
        "Lift Angle",
        h.lift_angle_deg
            .map(|l| format!("{}°", fixed_text(l, base_digits::AMPLITUDE))),
    );
    add("Settling", h.settle_s.map(each));
    add("Measured", h.measure_s.map(each));
    add("Software", Some(software()));
    let mut out = format!("{title}\n\n");
    out += &facts(&lines);
    out.push('\n');
    out
}

/// Seconds in each position, "20 s in each position".
fn each(s: f64) -> String {
    format!("{} s in each position", fixed_text(s, 0))
}

/// Name and value lines with the values lined up.
fn facts<S: AsRef<str>>(lines: &[(&str, S)]) -> String {
    let w = lines
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    lines
        .iter()
        .map(|(k, v)| {
            let pad = w - k.chars().count();
            format!("{k}{}{GAP}{}\n", " ".repeat(pad), v.as_ref())
        })
        .collect()
}

/// The header rows and a row for each position measured.
fn table_rows(ix: &StateIndices, from: &str) -> Vec<Vec<String>> {
    let mut rows = vec![
        vec![
            "Position".into(),
            "Rate".into(),
            from.into(),
            "Amplitude".into(),
            "Beat Error".into(),
        ],
        vec![
            String::new(),
            "s/d".into(),
            "s/d".into(),
            "°".into(),
            "ms".into(),
        ],
    ];
    for v in &ix.positions {
        rows.push(vec![
            v.position.display_label(),
            signed(v.rate_s_per_day, base_digits::RATE),
            signed(
                v.rate_s_per_day.zip(ix.x).map(|(r, m)| r - m),
                base_digits::RATE,
            ),
            plain(v.amplitude_deg, base_digits::AMPLITUDE),
            plain(v.beat_error_ms, base_digits::BEAT_ERROR),
        ]);
    }
    rows
}

/// The Average (or Mean) row.
fn mean_row(name: &str, ix: &StateIndices) -> Vec<String> {
    vec![
        name.into(),
        signed(ix.x, base_digits::RATE),
        String::new(),
        plain(ix.amplitude_mean, base_digits::AMPLITUDE),
        plain(ix.beat_error_mean, base_digits::BEAT_ERROR),
    ]
}

/// The rows as a table: the first column to the left, the figures to the
/// right, a rule under the `head` header rows and above the last
/// `summary` rows.
fn table(rows: &[Vec<String>], head: usize, summary: usize) -> String {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..cols)
        .map(|c| {
            rows.iter()
                .filter_map(|r| r.get(c))
                .map(|s| s.chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    let total = widths.iter().sum::<usize>() + GAP.len() * cols.saturating_sub(1);
    let rule = format!("{}\n", "-".repeat(total));
    let summary_at = rows.len().saturating_sub(summary).max(head);
    let mut out = String::new();
    for (i, r) in rows.iter().enumerate() {
        if i == head || (i == summary_at && summary > 0) {
            out += &rule;
        }
        let mut l = String::new();
        for (c, w) in widths.iter().enumerate() {
            let cell = r.get(c).map_or("", String::as_str);
            let pad = " ".repeat(w - cell.chars().count());
            if c == 0 {
                l += &format!("{cell}{pad}");
            } else {
                l += &format!("{GAP}{pad}{cell}");
            }
        }
        out += l.trim_end();
        out.push('\n');
    }
    out
}

/// The 24 hour half set against the full-wind test
/// ([`session::comparison`], as the app's pane reads it): each position's
/// rate and amplitude at full wind and now, and the change, then Im and
/// Ie. Positions read in only one of the two are left out.
fn comparison(e: Earlier, later: &[SourcedFigures]) -> String {
    let Some(c) = session::comparison(e.figures, e.hours, later) else {
        return String::new();
    };
    // The hours to a tenth, as the pane and the CSV give them (Hugh on
    // #1713).
    let now = wind_words_to(e.hours, HOURS_DIGITS);
    // Short heads, so the table keeps within the width: the section's
    // heading names both states in full.
    let later_head = format!("{} h", fixed_text(e.hours, HOURS_DIGITS));
    let mut rows = vec![
        vec![
            "Position".into(),
            "Rate".into(),
            String::new(),
            String::new(),
            "Amp.".into(),
            String::new(),
            String::new(),
        ],
        vec![
            String::new(),
            "Full".into(),
            later_head.clone(),
            "Change".into(),
            "Full".into(),
            later_head,
            "Change".into(),
        ],
        vec![
            String::new(),
            "s/d".into(),
            "s/d".into(),
            "s/d".into(),
            "°".into(),
            "°".into(),
            "°".into(),
        ],
    ];
    let (r, d) = (base_digits::RATE, base_digits::AMPLITUDE);
    for (p, v) in &c.rows {
        rows.push(vec![
            p.display_label(),
            signed(v[0], r),
            signed(v[1], r),
            signed(v[2], r),
            plain(v[3], d),
            plain(v[4], d),
            signed(v[5], d),
        ]);
    }
    let mut out = format!("\nAgainst the Full-Wind Test, {now}\n\n");
    out += &table(&rows, 3, 0);
    out.push('\n');
    out += &facts(&[
        (session::IM_NAME, unit(signed(c.im, r), " s/d")),
        (session::IE_NAME, unit(plain(c.ie, r), " s/d")),
    ]);
    out
}

/// The notes under a table: the beat error's source, then each
/// condition found in a position's sound, a bullet each.
fn notes(readings: &[Reading], ix: &StateIndices) -> String {
    let mut lines = vec![ix.beat_error_summary()];
    for r in readings {
        lines.extend(conditions(
            &r.position.display_label(),
            r.bph_set,
            &r.measurement,
        ));
    }
    let mut out = String::from("\nNotes\n");
    for l in lines {
        out += &wrap(&l, "- ");
    }
    out
}

/// The conditions found in one position's measurement, as plain facts:
/// no regular beat, clipping, a short stretch of beats, and how its beat
/// error and sides were read.
pub fn conditions(label: &str, bph_set: Option<u32>, m: &Measurement) -> Vec<String> {
    let mut out = Vec::new();
    if !m.watch_heard {
        out.push(format!(
            "{label}: {}",
            session::no_regular_beat_in(m, bph_set)
        ));
    }
    if m.clipped_beats > 0 {
        let pct = m.clipped_fraction * 100.0;
        out.push(if pct < 0.5 {
            format!("{label}: clipping on under 1% of beats")
        } else {
            format!("{label}: clipping on {pct:.0}% of beats")
        });
    }
    out.extend(m.caveats.iter().filter_map(|c| caveat_condition(label, c)));
    out
}

/// The line for a caveat a position's reading carries, where a table says
/// it.
fn caveat_condition(label: &str, c: &Caveat) -> Option<String> {
    match c {
        Caveat::ShortSpan { span_s } => Some(format!("{label}: {span_s:.0} s of beats")),
        Caveat::BeatErrorAsSize => Some(format!(
            "{label}: beat error given as a size, its sign differs either side of a cut"
        )),
        Caveat::SidesByCount { count } => Some(format!(
            "{label}: Tick and Tock named by the beat count across {count} break{}, \
             not told apart by their sound",
            crate::measure::plural(*count)
        )),
        _ => None,
    }
}

/// `text` wrapped at [`WIDTH`] columns, the first line opening with
/// `lead` and the others indented to match. A break never falls inside a
/// figure: the space either side of a "±" holds.
pub fn wrap(text: &str, lead: &str) -> String {
    let indent = " ".repeat(lead.chars().count());
    // Words, with "6.4 ± 0.6" kept as one.
    let mut words: Vec<String> = Vec::new();
    let mut join_next = false;
    for w in text.split_whitespace() {
        match words.last_mut() {
            Some(last) if join_next || w == "±" => {
                last.push(' ');
                last.push_str(w);
            }
            _ => words.push(w.to_string()),
        }
        join_next = w == "±";
    }
    let mut out = String::new();
    let mut line = lead.to_string();
    let mut empty = true;
    for w in words {
        let len = line.chars().count() + usize::from(!empty) + w.chars().count();
        if !empty && len > WIDTH {
            out += &line;
            out.push('\n');
            line = indent.clone();
            empty = true;
        }
        if !empty {
            line.push(' ');
        }
        line += &w;
        empty = false;
    }
    out += &line;
    out.push('\n');
    out
}

fn signed(v: Option<f64>, digits: usize) -> String {
    v.filter(|x| x.is_finite())
        .map_or_else(|| MISSING.into(), |x| signed_text(x, digits))
}

fn plain(v: Option<f64>, digits: usize) -> String {
    v.filter(|x| x.is_finite())
        .map_or_else(|| MISSING.into(), |x| fixed_text(x, digits))
}

/// A figure with its unit ("-35.0 s/d", "-16°"), or the dash alone.
fn unit(v: String, u: &str) -> String {
    if v == MISSING {
        v
    } else {
        format!("{v}{u}")
    }
}

#[cfg(test)]
#[path = "text_report_tests.rs"]
mod tests;
