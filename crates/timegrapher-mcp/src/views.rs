//! Charts in the chat: an MCP App view (the `ui://` resource below) that the
//! chat app shows beside a tool's result, drawing what the desktop app
//! draws. Chat apps without MCP Apps ignore it and keep the JSON.
//!
//! The tool's structured content stays the CLI's document unchanged; what
//! only the charts need (the readings over time, the Tick and Tock profiles)
//! rides in the result's `_meta` under [`META_KEY`].

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use serde_json::{json, Map, Value};
use timegrapher_core::figure_text::true_minus;
use timegrapher_core::histogram::normal_quantile;
use timegrapher_core::palette::{Palette, Rgb, DARK, LIGHT};
use timegrapher_core::walk::{default_merge, merged, IsochronismWalk, WalkPoint};

use crate::cli;

pub const URI: &str = "ui://timegrapher/charts";
pub const MIME: &str = "text/html;profile=mcp-app";
pub const META_KEY: &str = "timegrapher/charts";
const HTML: &str = include_str!("view.html");

/// The page with the app's palette in it, so the charts and the app cannot
/// drift apart.
pub fn page() -> &'static str {
    static PAGE: OnceLock<String> = OnceLock::new();
    PAGE.get_or_init(|| {
        let css = format!(
            ":root {{\n  color-scheme: light;\n{}}}\n:root[data-theme=\"dark\"] {{\n  color-scheme: dark;\n{}}}",
            variables(&LIGHT),
            variables(&DARK)
        );
        let viridis = serde_json::to_string(&timegrapher_core::palette::VIRIDIS)
            .expect("the viridis table as JSON");
        let period_ticks = serde_json::to_string(&timegrapher_core::period_ticks::TICKS)
            .expect("the period marks as JSON");
        let marks: Vec<(&str, f64)> = PROBABILITY_MARKS
            .iter()
            .map(|&(pc, label)| (label, normal_quantile(pc / 100.0)))
            .collect();
        let marks = serde_json::to_string(&marks).expect("the probability marks as JSON");
        let positions = serde_json::to_string(&positions()).expect("the positions as JSON");
        let no_beat = serde_json::to_string(timegrapher_core::measure::NO_WATCH_TITLE)
            .expect("the no-beat heading as JSON");
        let minus = serde_json::to_string(&true_minus("-")).expect("the minus sign as JSON");
        let wind_not_given =
            serde_json::to_string(timegrapher_core::reading_text::WIND_NOT_GIVEN)
                .expect("the wind-not-given words as JSON");
        HTML.replace("/*PALETTE*/", &css)
            .replace("/*BASE_DIGITS*/null", &base_digits())
            .replace("/*VIRIDIS*/null", &viridis)
            .replace("/*TIME_MAP*/null", &time_map())
            .replace("/*PERIOD_TICKS*/null", &period_ticks)
            .replace("/*REFERENCE_HOVER*/null", &reference_hover())
            .replace("/*SCORE_WORDS*/null", &score_words())
            .replace("/*WALK_WORDS*/null", &walk_words())
            .replace(
                "/*MARK_ROOM_PX*/null",
                &timegrapher_core::period_ticks::MARK_ROOM_PX.to_string(),
            )
            .replace("/*PROBABILITY_MARKS*/null", &marks)
            .replace("/*POSITIONS*/null", &positions)
            .replace("/*NO_BEAT_TITLE*/null", &no_beat)
            .replace("/*WIND_NOT_GIVEN*/null", &wind_not_given)
            .replace("/*MINUS*/null", &minus)
    })
}

/// What each core reference line says on hover, by its label, for the
/// page's `REFERENCE_HOVER` (core `steady_views::reference_hover`).
fn reference_hover() -> String {
    let said: serde_json::Map<String, Value> = timegrapher_core::periodicity::reference_lines()
        .iter()
        .map(|r| {
            let text = timegrapher_core::steady_views::reference_hover(r.label);
            (r.label.to_string(), Value::from(text))
        })
        .collect();
    Value::Object(said).to_string()
}

/// The Periods plot's words from core, for the page's `SCORE_WORDS`.
fn score_words() -> String {
    use timegrapher_core::steady_views as v;
    json!({
        "axis_name": v::SCORE_AXIS_NAME,
        "axis_hover": v::SCORE_AXIS_HOVER,
        "threshold_name": v::THRESHOLD_NAME,
        "threshold_hover": v::threshold_hover(timegrapher_core::longterm::LongConfig::default().false_alarm),
    })
    .to_string()
}

/// Core `palette::time_colour` (Hugh's M3, viridis lifted into each
/// theme) as core's `time_table` reads it, for the page's `TIME_MAP`, so
/// the page draws the one map and holds no copy of it.
fn time_map() -> String {
    let side = |dark: bool| -> Vec<[u8; 3]> {
        timegrapher_core::palette::time_table(dark)
            .into_iter()
            .map(|c| [c.r, c.g, c.b])
            .collect()
    };
    json!({ "dark": side(true), "light": side(false) }).to_string()
}

/// The Isochronism card's words from core, for the page's `WALK_WORDS`.
fn walk_words() -> String {
    use timegrapher_core::steady_views as v;
    json!({
        "title": v::WALK_TITLE, "hint": v::WALK_HINT, "since": v::WALK_SINCE, "key_start": v::WALK_KEY_START, "start": v::WALK_START,
        "rate_name": v::WALK_RATE_NAME, "rate_unit": v::WALK_RATE_UNIT,
        "amp_name": v::WALK_AMP_NAME, "amp_unit": v::WALK_AMP_UNIT,
    })
    .to_string()
}

/// Each figure's decimals with no ± to follow, as core's
/// `library::base_digits` gives them, for the page's `BASE_DIGITS`.
fn base_digits() -> String {
    use timegrapher_core::library::base_digits as b;
    json!({
        "rate": b::RATE,
        "rate_range": b::RATE_RANGE,
        "amplitude": b::AMPLITUDE,
        "beat_error": b::BEAT_ERROR,
        "jitter": b::JITTER,
        "clock": b::CLOCK,
    })
    .to_string()
}

/// Each position's common code and name, as core gives them, under both its
/// Witschi code and its common one, so the page names a reading whichever
/// code it carries and writes no name of its own.
fn positions() -> Map<String, Value> {
    let mut map = Map::new();
    for p in timegrapher_core::session::Position::ALL {
        let entry = json!([p.common(), p.name()]);
        map.insert(p.code().to_string(), entry.clone());
        map.insert(p.common().to_string(), entry);
    }
    map
}

/// Shares labelled up the probability plot, percent, as on probability
/// paper: the app's marks. The page gets each one's normal score from
/// [`normal_quantile`], so it never works one out itself.
const PROBABILITY_MARKS: [(f64, &str); 11] = [
    (0.1, "0.1%"),
    (1.0, "1%"),
    (5.0, "5%"),
    (10.0, "10%"),
    (25.0, "25%"),
    (50.0, "50%"),
    (75.0, "75%"),
    (90.0, "90%"),
    (95.0, "95%"),
    (99.0, "99%"),
    (99.9, "99.9%"),
];

/// A colour at an opacity, as CSS.
fn rgba(c: Rgb, a: f32) -> String {
    format!("rgba({}, {}, {}, {a})", c.r, c.g, c.b)
}

/// One theme as the page's CSS variables.
fn variables(p: &Palette) -> String {
    [
        ("window", p.window.css()),
        ("card", p.card.css()),
        ("control", p.control.css()),
        ("separator", p.separator.css()),
        ("text", p.text.css()),
        ("text2", p.text_secondary.css()),
        ("text3", p.text_tertiary.css()),
        ("tick", p.tick.css()),
        ("tock", p.tock.css()),
        ("rate", p.trace_rate.css()),
        ("amplitude", p.trace_amplitude.css()),
        ("beat-error", p.trace_beat_error.css()),
        // Line Strength on Plots, as in the app: the grid and the axes are
        // the text colour at their own opacities, call-outs one grey.
        ("grid", rgba(p.text, p.grid_opacity)),
        ("axis", rgba(p.text, p.axis_opacity)),
        ("callout", p.callout.css()),
    ]
    .iter()
    .map(|(name, value)| format!("  --{name}: {value};\n"))
    .collect()
}

/// Most points drawn per series: plenty for a chat-sized chart, and it keeps
/// a day-long run's result small.
const MAX_POINTS: usize = 1500;

/// The `_meta` key the Tick and Tock view reads a `profile` result's facts
/// from (#510); `analyze`'s ride with its profile as `profile_facts`.
pub const PROFILE_FACTS_KEY: &str = "timegrapher/profile_facts";

/// Each side's facts as core words them (`profile::facts_of`), with their
/// names, so the view writes the app's lines: `{names, a, b}`, or `None`
/// for a document without both sides.
pub fn profile_facts(profile: &Value) -> Option<Value> {
    let side = |s: &Value| {
        let s = s.as_object()?;
        let f = |k: &str| s.get(k).and_then(Value::as_f64);
        Some(json!(timegrapher_core::profile::facts_of(
            s.get("beats").and_then(Value::as_u64).unwrap_or(0) as usize,
            f("unlock_ms"),
            f("drop_ms"),
        )))
    };
    Some(json!({
        "names": timegrapher_core::profile::FACT_NAMES,
        "a": side(&profile["a"])?,
        "b": side(&profile["b"])?,
    }))
}

/// The `_meta` key the views read their ± texts from (#806).
pub const PM_KEY: &str = "timegrapher/pm";

fn pm_of(se: &Value, digits: usize) -> Value {
    se.as_f64()
        .and_then(|se| timegrapher_core::library::pm_text(se, digits))
        .map_or(Value::Null, Value::from)
}

/// The ± beside each tile's figure, as core's `library::pm_text` writes it,
/// so the view shows the app's and the CLI's text instead of rounding its
/// own, and under `digits` the decimals each figure is written to
/// (`library::figure_digits`: its fixed place, #1470): for
/// `analyze`, the whole file's rate, amplitude and beat error and each take
/// reading's, in the take's order; for a live reading, its amplitude's. A null ±
/// where there is no standard error; `None` for other results.
pub fn pm_texts(doc: &Value) -> Option<Value> {
    use timegrapher_core::library::{base_digits as base, figure_digits};
    let schema = doc["schema"].as_str().unwrap_or("");
    let tiles = |rate: &Value, amplitude: &Value, beat_error: &Value| {
        json!({
            "rate": pm_of(rate, base::RATE),
            "amplitude": pm_of(amplitude, base::AMPLITUDE),
            "beat_error": pm_of(beat_error, base::BEAT_ERROR),
            "digits": {
                "rate": figure_digits(rate.as_f64(), base::RATE),
                "amplitude": figure_digits(amplitude.as_f64(), base::AMPLITUDE),
                "beat_error": figure_digits(beat_error.as_f64(), base::BEAT_ERROR),
            },
        })
    };
    if schema.starts_with("timegrapher.analyze/") {
        let readings: Vec<Value> = doc["take"]["readings"]
            .as_array()
            .map(|rs| {
                rs.iter()
                    .map(|r| {
                        let c = &r["measurement"]["confidence"];
                        tiles(
                            &c["rate"]["se"],
                            &c["amplitude"]["se"],
                            &c["beat_error"]["se"],
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let c = &doc["confidence"];
        let mut pm = tiles(
            &c["rate"]["se"],
            &doc["amplitude_se_deg"],
            &c["beat_error"]["se"],
        );
        pm["readings"] = json!(readings);
        return Some(pm);
    }
    if schema.starts_with("timegrapher.live/") {
        // A live rate and beat error have no ±; its amplitude has.
        let mut pm = tiles(
            &Value::Null,
            &doc["reading"]["amplitude_error_deg"],
            &Value::Null,
        );
        if let Some(o) = pm.as_object_mut() {
            o.remove("rate");
            o.remove("beat_error");
        }
        return Some(pm);
    }
    None
}

/// The `_meta` that links a tool to the view.
pub fn tool_meta() -> Value {
    json!({ "ui": { "resourceUri": URI } })
}

/// The view as `resources/list` shows it.
pub fn resource() -> Value {
    json!({
        "uri": URI,
        "name": "charts",
        "title": "Timegrapher Charts",
        "description": "The readings drawn as the timegrapher app draws them.",
        "mimeType": MIME,
    })
}

/// The view's contents for `resources/read`: one self-contained page that
/// needs no network (the hosts' default content policy).
pub fn contents() -> Value {
    json!({
        "uri": URI,
        "mimeType": MIME,
        "text": page(),
        "_meta": { "ui": { "prefersBorder": false } },
    })
}

/// A file in the temporary folder that is removed when dropped.
pub struct TempFile(PathBuf);

impl TempFile {
    fn new(ext: &str) -> TempFile {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        TempFile(
            std::env::temp_dir().join(format!("timegrapher-mcp-{}-{n}.{ext}", std::process::id())),
        )
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Extra arguments for `analyze` so the same run also writes its readings
/// over time, and the file they go to.
pub fn analyze_extras() -> (Vec<String>, TempFile) {
    let f = TempFile::new("csv");
    (vec![format!("--trend={}", f.0.display())], f)
}

/// The readings over time written by `analyze --trend` (see
/// [`analyze_extras`]), each averaged as the app's charts average them, as
/// the three series of the strip, with the averaging time, the ppm the
/// rates are corrected by (none on the sound card's clock) and each chart's
/// scale ([`timegrapher_core::chart_scale::data_range`]).
fn strip(csv: &str, duration_s: f64) -> Value {
    let mut lines = csv.lines();
    let header: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
    let col = |name: &str| header.iter().position(|h| *h == name);
    let Some(t_col) = col("t_s") else {
        return Value::Null;
    };
    let cols = [
        "rate_s_per_day",
        "amplitude_deg",
        "amplitude_even_deg",
        "amplitude_odd_deg",
        "beat_error_unlock_ms",
        "beat_error_ms",
    ]
    .map(col);
    let mut series: [Vec<[f64; 2]>; 6] = Default::default();
    let mut average_s = None;
    let mut clock_ppm = None;
    for line in lines {
        let f: Vec<&str> = line.split(',').collect();
        let get = |i: Option<usize>| {
            i.and_then(|i| f.get(i))
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite())
        };
        let Some(t) = get(Some(t_col)) else {
            continue;
        };
        average_s = average_s.or(get(col("average_s")));
        clock_ppm = clock_ppm.or(get(col("clock_ppm")));
        let mut row = cols.map(get);
        // Beat error from the unlock where core read it there, from the
        // drop only where it fell back, as the app's strip draws it.
        let chosen = timegrapher_core::session::chosen_beat_error(row[4], row[5]);
        let from = |s| chosen.filter(|c| c.1 == s).map(|c| c.0);
        row[4] = from(timegrapher_core::session::BeatErrorSource::Unlock);
        row[5] = from(timegrapher_core::session::BeatErrorSource::Drop);
        for (k, v) in row.into_iter().enumerate() {
            match v {
                Some(v) => series[k].push([t, v]),
                // No figure here (a cut, a mask, too few beats): the line
                // lifts, as the app's does, with one gap for the run.
                None if series[k].last().is_some_and(|p| p[1].is_finite()) => {
                    series[k].push([t, f64::NAN]);
                }
                None => {}
            }
        }
    }
    let [rates, amps, ticks, tocks, unlocks, drops] = series;
    // Each chart's scale, as the app's charts over time take it: every
    // point of its lines.
    let range = |lines: &[&Vec<[f64; 2]>]| {
        let ys: Vec<f64> = lines
            .iter()
            .flat_map(|l| l.iter().map(|p| p[1]).filter(|y| y.is_finite()))
            .collect();
        timegrapher_core::chart_scale::data_range(&ys).map(|(lo, hi)| [lo, hi])
    };
    let ranges = json!({
        "rate": range(&[&rates]),
        "amplitude": range(&[&amps, &ticks, &tocks]),
        "beat_error": range(&[&unlocks, &drops]),
    });
    json!({
        "duration_s": duration_s,
        "average_s": average_s,
        "clock_ppm": clock_ppm,
        "y_range": ranges,
        "rate": thin(rates),
        "amplitude": thin(amps),
        "amplitude_tick": thin(ticks),
        "amplitude_tock": thin(tocks),
        "beat_error_unlock": thin(unlocks),
        "beat_error_drop": thin(drops),
    })
}

/// Every k-th point, so at most [`MAX_POINTS`] remain, and every gap (a
/// point with no value, null in the view's data), so a line still lifts
/// where it did.
fn thin(points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let k = points.len().div_ceil(MAX_POINTS).max(1);
    points
        .into_iter()
        .enumerate()
        .filter(|(i, p)| i % k == 0 || !p[1].is_finite())
        .map(|(_, p)| p)
        .collect()
}

/// Takes out of a `series --views` document what only the Steadiness and
/// Distribution charts draw (each series' `views`, the shared
/// `period_span`, the `spectra` the FFT view draws, the time-frequency maps
/// and each distribution's probability-plot `points`, a few
/// hundred kB on a long take), so the model reads the headlines and not
/// thousands of numbers; the view puts them back.
pub fn take_series_charts(doc: &mut Value) -> Value {
    let mut views = Vec::new();
    if let Some(series) = doc.get_mut("series").and_then(Value::as_array_mut) {
        for s in series {
            add_unit_text(s);
            views.push(
                s.as_object_mut()
                    .and_then(|o| o.remove("views"))
                    .unwrap_or(Value::Null),
            );
        }
    }
    let period_span = doc
        .as_object_mut()
        .and_then(|o| o.remove("period_span"))
        .unwrap_or(Value::Null);
    let spectra = doc
        .as_object_mut()
        .and_then(|o| o.remove("spectra"))
        .unwrap_or(Value::Null);
    let mut spectra = spectra;
    for s in spectra.as_array_mut().into_iter().flatten() {
        add_unit_text(s);
    }
    if let Some(spectra) = spectra.as_array() {
        // Keep availability context model-side while large chart arrays move
        // to metadata. Missing legacy support remains explicitly unknown.
        doc["spectrum_support"] = spectra
            .iter()
            .map(|s| {
                json!({
                    "series": s["series"], "unit": s["unit"], "support": s["support"],
                })
            })
            .collect();
    }
    // The time-frequency maps (about 1 MB on a 2 h take) go to the view;
    // the model reads each map's outline.
    let maps = doc
        .as_object_mut()
        .and_then(|o| o.remove("time_frequency"))
        .unwrap_or(Value::Null);
    let mut maps = maps;
    for m in maps.as_array_mut().into_iter().flatten() {
        add_unit_text(m);
    }
    if let Some(maps) = maps.as_array() {
        doc["time_frequency_maps"] = maps.iter().map(map_outline).collect();
    }
    // No view draws the coherence grids, which are larger still: the model
    // keeps everything else about each pair.
    for c in doc
        .get_mut("coherence")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(o) = c.as_object_mut() {
            o.remove("coherence");
            o.remove("phase_deg");
        }
    }
    let mut points = Vec::new();
    if let Some(d) = doc.get_mut("distributions") {
        if let Some(all) = d.get_mut("overall").and_then(Value::as_array_mut) {
            for e in all {
                points.push(
                    e.as_object_mut()
                        .and_then(|o| o.remove("points"))
                        .unwrap_or(Value::Null),
                );
            }
        }
        if let Some(stretches) = d.get_mut("stretches").and_then(Value::as_array_mut) {
            for st in stretches {
                for e in st
                    .get_mut("distributions")
                    .and_then(Value::as_array_mut)
                    .into_iter()
                    .flatten()
                {
                    if let Some(o) = e.as_object_mut() {
                        o.remove("points");
                    }
                }
            }
        }
    }
    // The isochronism walk, every 5 min point, goes to the view merged to
    // the card's width by core; the model reads its outline.
    let walk = doc
        .as_object_mut()
        .and_then(|o| o.remove("walk"))
        .and_then(|w| walk_of(&w));
    if let Some(w) = &walk {
        doc["walk"] = walk_outline(w);
    }
    let walk_view = walk.as_ref().and_then(|w| walk_chart(w, doc));
    json!({
        "views": views, "period_span": period_span, "spectra": spectra, "points": points,
        "maps": maps, "walk": walk_view,
    })
}

/// Adds `unit_text`, the unit as it follows a number with its gap
/// ("°", " ms", " s/d"), by core's [`unit_after`], so the view writes
/// units by the same rule as core's sentences and the app (#1428).
///
/// [`unit_after`]: timegrapher_core::steadiness::unit_after
fn add_unit_text(item: &mut Value) {
    let Some(unit) = item.get("unit").and_then(Value::as_str) else {
        return;
    };
    let text = match timegrapher_core::steadiness::unit_after(unit) {
        (_, "") => String::new(),
        (gap, unit) => format!("{gap}{unit}"),
    };
    item["unit_text"] = Value::String(text);
}

/// The walk as the card draws it: core's view of it, merged to the card's
/// width, with core's axes (the amplitude widened to the curve's, as the
/// app's is) and the curve's headline one fact a line (Hugh's F4).
fn walk_chart(w: &IsochronismWalk, doc: &Value) -> Option<Value> {
    let v = timegrapher_core::steady_views::walk_view(w)?;
    let iso = &doc["isochronism"];
    let amps: Vec<f64> = iso["bins"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| b["amplitude_deg"].as_f64())
        .collect();
    let curve = iso["bin_deg"]
        .as_f64()
        .and_then(|bin| timegrapher_core::steady_views::curve_amp(&amps, bin));
    let axes = timegrapher_core::steady_views::walk_axes(&v.dots, curve);
    let mut v = serde_json::to_value(v).unwrap_or(Value::Null);
    v["axes"] = json!(axes);
    // Screen text, so with a true minus, as the app's (#1431).
    for d in v["dots"].as_array_mut().into_iter().flatten() {
        if let Some(h) = d["hover"].as_str() {
            d["hover"] = json!(true_minus(h));
        }
    }
    v["summary"] = json!(iso["headline"]
        .as_str()
        .map(|h| timegrapher_core::steady_views::headline_lines(&true_minus(h)))
        .unwrap_or_default());
    Some(v)
}

/// The `walk` a `series` document carries, as core's walk.
fn walk_of(v: &Value) -> Option<IsochronismWalk> {
    let points = v["points"]
        .as_array()?
        .iter()
        .map(|p| {
            Some(WalkPoint {
                t_s: p["t_s"].as_f64()?,
                wind_h: p["wind_h"].as_f64()?,
                rate_s_per_day: p["rate_s_per_day"].as_f64()?,
                amplitude_deg: p["amplitude_deg"].as_f64()?,
                readings: p["readings"].as_u64()? as usize,
                amp_windows: p["amp_windows"].as_u64()? as usize,
                // Absent from a document written before the walk's floors (#741).
                rate_slots: p["rate_slots"].as_u64().unwrap_or(0) as usize,
                amp_slots: p["amp_slots"].as_u64().unwrap_or(0) as usize,
                thin: p["thin"].as_bool().unwrap_or(false),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(IsochronismWalk {
        bin_s: v["bin_s"].as_f64()?,
        points,
        rate_only_to_s: v["rate_only_to_s"].as_f64(),
    })
}

/// What the model reads of the walk: how many points, at what width, and
/// the first and last at the card's width.
fn walk_outline(w: &IsochronismWalk) -> Value {
    let m = merged(w, default_merge(w));
    json!({
        "bin_s": w.bin_s, "points": w.points.len(),
        "merged_bin_s": m.bin_s, "merged_points": m.points.len(),
        "first": m.points.first(), "last": m.points.last(),
    })
}

/// What a time-frequency map covers, without its cells: its periods and
/// times, how many cells were read, and the colour scale it is drawn in.
fn map_outline(m: &Value) -> Value {
    let span = |k: &str| {
        let v: Vec<f64> = m[k]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_f64)
            .collect();
        let lo = v.iter().copied().reduce(f64::min);
        let hi = v.iter().copied().reduce(f64::max);
        (v.len(), lo, hi)
    };
    let (rows, shortest, longest) = span("period_s");
    let (columns, first, last) = span("t_s");
    let cells = m["values"].as_array().into_iter().flatten();
    let read = cells
        .flat_map(|c| c.as_array().into_iter().flatten())
        .filter(|v| v.is_number())
        .count();
    json!({
        "series": m["series"], "method": m["method"], "unit": m["unit"],
        "step_s": m["step_s"], "window_s": m["window_s"], "hop_s": m["hop_s"],
        "rows": rows, "shortest_period_s": shortest, "longest_period_s": longest,
        "columns": columns, "first_t_s": first, "last_t_s": last,
        "cells_read": read, "cells": rows * columns, "scale": m["scale"],
    })
}

/// Where the Recording view's Tick and Tock profiles are read, as start and
/// length in seconds. For a file its take's session file lists, where
/// `session` measured the beat shape of the first entry naming it (its
/// `shape`'s `from_s` and `measured_s`, in `analyze`'s `take` block),
/// none where it measured none. For a file alone, core's beat-shape
/// stretch for a take of one entry naming it
/// ([`analysis_take::shape_stretches`]): the first
/// [`analysis_take::SHAPE_S`] once the watch has settled, with nothing
/// masked. None for an empty recording or one with no regular beat.
///
/// [`analysis_take::shape_stretches`]: timegrapher_core::analysis_take::shape_stretches
/// [`analysis_take::SHAPE_S`]: timegrapher_core::analysis_take::SHAPE_S
pub fn profile_stretch(doc: &Value) -> Option<(f64, f64)> {
    if let Some(readings) = doc["take"]["readings"].as_array() {
        return readings.iter().find_map(|r| {
            let shape = &r["shape"];
            Some((shape["from_s"].as_f64()?, shape["measured_s"].as_f64()?))
        });
    }
    if doc["watch_heard"] == json!(false) {
        return None;
    }
    // A file alone reads as a take of one entry naming it, with no
    // session file's settings: core's beat-shape stretch for that entry.
    use timegrapher_core::session::{Manifest, RecordingEntry};
    let duration_s = doc["duration_s"].as_f64().unwrap_or(0.0);
    let alone = Manifest {
        recordings: vec![RecordingEntry {
            file: ".".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let shapes = timegrapher_core::analysis_take::shape_stretches(&alone, duration_s);
    let (from, to) = shapes.into_iter().next().flatten()?;
    Some((from, to - from))
}

/// What the Recording view needs beyond `analyze`'s document: the strip
/// from the trend file, and the Tick and Tock profiles over the stretch
/// core measures the beat shape on ([`profile_stretch`]).
pub async fn analyze_charts(
    doc: &Value,
    extras: &[String],
    recording: &str,
    scope: &cli::Scope<'_>,
    profile_budget: std::time::Duration,
) -> Result<(Value, Option<String>), cli::RunError> {
    let mut warning = None;
    let mut out = Map::new();
    let duration = doc["duration_s"].as_f64().unwrap_or(0.0);
    if let Some(path) = extras.first().and_then(|e| e.strip_prefix("--trend=")) {
        if let Ok(csv) = std::fs::read_to_string(path) {
            out.insert("strip".into(), strip(&csv, duration));
        }
    }
    let stretch = profile_stretch(doc);
    if let Some((at, span)) = stretch {
        let mut line = vec![
            "profile".to_string(),
            format!("--at={at}"),
            format!("--span={span}"),
            "--json".into(),
        ];
        // The profile's beat rate and lift angle are the reading's.
        if let Some(bph) = doc["bph"].as_u64() {
            line.push(format!("--bph={bph}"));
        }
        if let Some(lift) = doc["input"]["settings"]["lift_deg"]
            .as_f64()
            .or(doc["lift_deg"].as_f64())
        {
            line.push(format!("--lift={lift}"));
        }
        line.push("--".into());
        line.push(recording.into());
        match scope.run(&line, Some(profile_budget)).await {
            Ok(cli::Outcome { json: Some(profile), exit_code: Some(0), .. }) => {
                if let Some(facts) = profile_facts(&profile) {
                    out.insert("profile_facts".into(), facts);
                }
                out.insert("profile".into(), profile);
            }
            Ok(outcome) => {
                warning = Some(format!("Auxiliary Tick/Tock profile unavailable (exit code {:?}): {}. The completed analysis is preserved.", outcome.exit_code, outcome.stderr));
            }
            Err(error) if error.stopped() => return Err(error),
            Err(error) => warning = Some(format!("Auxiliary Tick/Tock profile unavailable: {error}. The completed analysis is preserved.")),
        }
    }
    Ok((Value::Object(out), warning))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profiles_facts_are_cores() {
        let p = json!({
            "a": { "beats": 120, "unlock_ms": -0.27, "drop_ms": 1.2 },
            "b": { "beats": 119, "unlock_ms": null, "drop_ms": 1.0 },
        });
        assert_eq!(
            profile_facts(&p),
            Some(json!({
                "names": ["Beats", "Unlock", "Drop", "Unlock to Drop"],
                "a": ["120", "−0.27 ms", "+1.20 ms", "1.47 ms"],
                "b": ["119", "–", "+1.00 ms", "–"],
            }))
        );
        assert_eq!(profile_facts(&json!({ "a": {} })), None);
    }

    #[test]
    fn the_page_carries_cores_base_decimals() {
        use timegrapher_core::library::base_digits as b;
        let page = page();
        assert!(!page.contains("/*BASE_DIGITS*/"));
        assert!(page.contains(&format!(
            "const BASE_DIGITS = {{\"amplitude\":{},\"beat_error\":{},\"clock\":{},\"jitter\":{},\"rate\":{},\"rate_range\":{}}};",
            b::AMPLITUDE,
            b::BEAT_ERROR,
            b::CLOCK,
            b::JITTER,
            b::RATE,
            b::RATE_RANGE
        )));
    }

    #[test]
    fn the_page_carries_cores_wind_not_given() {
        assert!(!page().contains("/*WIND_NOT_GIVEN*/"));
        assert!(page().contains(&format!(
            "const WIND_NOT_GIVEN = \"{}\";",
            timegrapher_core::reading_text::WIND_NOT_GIVEN
        )));
    }

    #[test]
    fn the_page_carries_cores_no_beat_heading() {
        assert!(!page().contains("/*NO_BEAT_TITLE*/"));
        assert!(page().contains(&format!(
            "const NO_BEAT_TITLE = \"{}\";",
            timegrapher_core::measure::NO_WATCH_TITLE
        )));
    }

    #[test]
    fn the_view_gets_its_plus_or_minus_in_cores_words() {
        // The view takes core's ±, rounded up at the figure's place, so
        // 0.4° reads "± 1°" (#806, #1470).
        let doc = json!({
            "schema": "timegrapher.analyze/1", "amplitude_se_deg": 0.4,
            "confidence": { "rate": { "se": 0.06 }, "beat_error": { "se": 0.004 } },
            "take": { "readings": [
                { "measurement": { "confidence": { "rate": { "se": 1.24 }, "amplitude": { "se": 2.6 } } } },
                { "measurement": { "watch_heard": false } },
            ] },
        });
        // Each figure at its place, its ± rounded up there (#1470).
        let digits = |rate: usize, amplitude: usize, beat_error: usize| json!({ "rate": rate, "amplitude": amplitude, "beat_error": beat_error });
        assert_eq!(
            pm_texts(&doc),
            Some(
                json!({ "rate": "0.1", "amplitude": "1", "beat_error": "0.01",
                "digits": digits(1, 0, 2), "readings": [
                { "rate": "1.3", "amplitude": "3", "beat_error": null, "digits": digits(1, 0, 2) },
                { "rate": null, "amplitude": null, "beat_error": null, "digits": digits(1, 0, 2) },
            ] })
            )
        );
        let live =
            json!({ "schema": "timegrapher.live/1", "reading": { "amplitude_error_deg": 0.4 } });
        assert_eq!(
            pm_texts(&live),
            Some(json!({ "amplitude": "1", "digits": digits(1, 0, 2) }))
        );
        assert_eq!(
            pm_texts(&json!({ "schema": "timegrapher.session/1" })),
            None
        );
    }

    #[test]
    fn the_profiles_are_read_where_core_reads_the_beat_shape() {
        // Settled for 20 s, then a minute; a short recording settles for
        // half its length and reads the rest.
        let file = |d: f64| profile_stretch(&json!({ "duration_s": d }));
        assert_eq!(file(300.0), Some((20.0, 60.0)));
        assert_eq!(file(30.0), Some((15.0, 15.0)));
        assert_eq!(file(0.0), None);
        assert_eq!(
            profile_stretch(&json!({ "duration_s": 300.0, "watch_heard": false })),
            None
        );
    }

    /// A file its take lists is read where `session` measured the beat
    /// shape, wherever the take's settling or masks put it.
    #[test]
    fn a_takes_file_has_its_profiles_read_where_session_read_its_shape() {
        let take =
            |readings: Value| json!({ "duration_s": 300.0, "take": { "readings": readings } });
        assert_eq!(
            profile_stretch(&take(json!([
                { "measurement": { "watch_heard": false } },
                { "shape": { "from_s": 95.0, "measured_s": 60.0 } },
            ]))),
            Some((95.0, 60.0))
        );
        assert_eq!(profile_stretch(&take(json!([{ "shape": null }]))), None);
    }

    #[test]
    fn the_chart_data_stays_out_of_what_the_model_reads() {
        let mut doc = json!({
            "series": [{ "series": "rate", "headline": "Rate: no cycle or level change detected", "views": { "readings": {} } }],
            "period_span": [0.5, 2.0],
            "spectra": [{ "series": "rate", "unit": "ms", "step_s": 10.0,
                "period_s": [600.0, 60.0], "amplitude": [0.2, 1.5] }],
            "distributions": {
                "overall": [{ "series": "rate", "points": [[1.0, 0.0]] }],
                "stretches": [{ "distributions": [{ "points": [[1.0, 0.0]] }] }],
            },
        });
        let charts = take_series_charts(&mut doc);
        let text = doc.to_string();
        assert!(
            !text.contains("views")
                && !text.contains("points")
                && !text.contains("period_span")
                && !text.contains("spectra")
        );
        assert_eq!(
            doc["series"][0]["headline"],
            "Rate: no cycle or level change detected"
        );
        assert_eq!(charts["views"][0]["readings"], json!({}));
        assert_eq!(charts["period_span"], json!([0.5, 2.0]));
        assert_eq!(charts["spectra"][0]["period_s"], json!([600.0, 60.0]));
        assert_eq!(charts["points"][0], json!([[1.0, 0.0]]));
    }

    #[test]
    fn the_walk_goes_to_the_view_as_core_merges_it_and_the_model_reads_its_outline() {
        let points: Vec<Value> = (0..600)
            .map(|i| {
                let t = 150.0 + 300.0 * i as f64;
                json!({ "t_s": t, "wind_h": t / 3600.0, "rate_s_per_day": i as f64 / 100.0,
                    "amplitude_deg": 300.0 - i as f64 / 10.0, "readings": 30, "amp_windows": 150 })
            })
            .collect();
        let mut doc = json!({ "walk": { "bin_s": 300.0, "points": points },
            "isochronism": { "headline": "The rate rises as the amplitude falls; flattest near 280°.",
                "bin_deg": 10.0, "bins": [{ "amplitude_deg": 200.0 }, { "amplitude_deg": 250.0 }] } });
        let charts = take_series_charts(&mut doc);
        let dots = charts["walk"]["dots"].as_array().unwrap();
        assert_eq!(dots.len(), 200);
        // Core's axes: the amplitude widened to the curve's bins (190° to
        // 260° padded) and the walk's own (237° to 303°), on round ends.
        assert_eq!(charts["walk"]["axes"][0], json!([180.0, 320.0, 20.0]));
        assert_eq!(
            charts["walk"]["summary"],
            json!([
                "The rate rises as the amplitude falls",
                "Flattest near 280°"
            ])
        );
        assert!(
            doc["isochronism"]["headline"].is_string(),
            "the model keeps it"
        );
        assert_eq!(dots[0]["hover"], "0.1 h after start · 300° · 0.0 s/d");
        assert_eq!(doc["walk"]["points"], 600);
        assert_eq!(doc["walk"]["merged_points"], 200);
        assert!(doc.to_string().len() < 1000, "{doc}");
        // No walk, or too short a one: no card.
        let mut none = json!({ "walk": null });
        assert!(take_series_charts(&mut none)["walk"].is_null());
    }

    /// The walk's words are screen text: a true minus, as the app's
    /// (#1431), while the model's JSON keeps the ASCII minus.
    #[test]
    fn the_walks_words_carry_a_true_minus() {
        let points: Vec<Value> = (0..30)
            .map(|i| {
                let t = 150.0 + 300.0 * i as f64;
                json!({ "t_s": t, "wind_h": t / 3600.0, "rate_s_per_day": -1.25 - i as f64 / 100.0,
                    "amplitude_deg": 300.0 - i as f64, "readings": 30, "amp_windows": 150 })
            })
            .collect();
        let headline = "The rate falls -0.03 s/d per degree; flattest near 280°.";
        let mut doc = json!({ "walk": { "bin_s": 300.0, "points": points },
            "isochronism": { "headline": headline, "bin_deg": 10.0, "bins": [] } });
        let charts = take_series_charts(&mut doc);
        assert_eq!(
            charts["walk"]["summary"][0],
            "The rate falls \u{2212}0.03 s/d per degree"
        );
        let hover = charts["walk"]["dots"][0]["hover"].as_str().unwrap();
        assert!(hover.ends_with("\u{2212}1.2 s/d"), "{hover}");
        assert_eq!(doc["isochronism"]["headline"], headline, "the model's own");
    }

    #[test]
    fn the_page_writes_cores_minus() {
        let line = format!("const MINUS = {};", json!(true_minus("-")));
        assert!(page().contains(&line), "{line}");
    }

    #[test]
    fn each_series_carries_its_unit_as_core_writes_it() {
        let mut doc = json!({ "series": [
            { "series": "rate", "unit": "s/d" },
            { "series": "amplitude", "unit": "deg" },
            { "series": "beat_error", "unit": "ms" },
            { "series": "rate", "unit": "" },
            { "series": "rate" },
        ] });
        take_series_charts(&mut doc);
        let text: Vec<_> = doc["series"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.get("unit_text").cloned())
            .collect();
        assert_eq!(
            text,
            [
                Some(json!(" s/d")),
                Some(json!("°")),
                Some(json!(" ms")),
                Some(json!("")),
                None
            ]
        );
    }

    #[test]
    fn fft_support_remains_model_visible_when_arrays_move_to_charts() {
        let description = "FFT input: 128 of 256 grid samples available. With samples missing, each size is a sine fitted to the samples present (Lomb–Scargle); nothing is filled in.";
        for support in [
            Value::Null,
            json!({"finite_bins": 128, "total_bins": 256, "description": description}),
        ] {
            let mut spectrum = json!({"series":"rate", "unit":"ms", "period_s":[64.0,32.0], "amplitude":[5.0,1.0]});
            if !support.is_null() {
                spectrum["support"] = support.clone();
            }
            let mut doc = json!({"schema":"timegrapher.series/2", "spectra":[spectrum.clone()], "series":[{"series":"rate","headline":"Rate"}]});
            let charts = take_series_charts(&mut doc);
            // The view writes the unit as core does (#1428).
            spectrum["unit_text"] = json!(" ms");
            assert_eq!(
                charts["spectra"][0], spectrum,
                "all chart arrays and support unchanged"
            );
            assert!(doc.get("spectra").is_none());
            assert_eq!(
                doc["spectrum_support"],
                json!([{"series":"rate", "unit":"ms", "support":support}])
            );
            let lead = crate::summary::lead(&doc).unwrap();
            assert!(lead.contains(if support.is_null() {
                "FFT rate input support was not recorded."
            } else {
                "128 of 256 grid samples available"
            }));
            assert_eq!(lead.contains(description), !support.is_null());
        }
    }

    #[test]
    fn series_chart_extraction_preserves_applied_clock_context() {
        for clock in [
            Value::Null,
            json!({ "ppm": 0.0, "rate_error_s_per_day": 0.0, "from": "stored for 'input', 2026-10-15", "tracks_drift": false }),
        ] {
            let mut doc = json!({
                "clock": clock,
                "series": [{ "headline": "Rate: too few readings", "views": { "readings": {} } }],
                "period_span": [0.5, 2.0],
            });
            let charts = take_series_charts(&mut doc);
            assert_eq!(doc["clock"], clock);
            assert!(doc["series"][0].get("views").is_none());
            assert_eq!(charts["views"][0], json!({ "readings": {} }));
        }
    }

    #[test]
    fn strip_from_trend() {
        let csv = "t_s,average_s,rate_s_per_day,amplitude_deg,amplitude_even_deg,amplitude_odd_deg,beat_error_unlock_ms,beat_error_ms,heard,clock_ppm\n\
                   1,10,,,,,,,false,20.000\n\
                   2,10,4.5,269,270,268,-0.38,-0.39,true,20.000\n\
                   3,10,4.7,272,272,,,-0.41,true,20.000\n\
                   4,10,,,,,,,false,20.000\n\
                   5,10,,,,,,,false,20.000\n\
                   6,10,4.9,271,271,271,-0.36,-0.40,true,20.000\n";
        let s = strip(csv, 30.0);
        assert_eq!(s["average_s"], json!(10.0));
        assert_eq!(s["clock_ppm"], json!(20.0));
        // Where a reading has no figure (here a cut from 4 to 5 s) the
        // line lifts: one null for the run, none before the first figure.
        assert_eq!(
            s["rate"],
            json!([[2.0, 4.5], [3.0, 4.7], [4.0, null], [6.0, 4.9]])
        );
        assert_eq!(
            s["amplitude"],
            json!([[2.0, 269.0], [3.0, 272.0], [4.0, null], [6.0, 271.0]])
        );
        assert_eq!(
            s["amplitude_tick"],
            json!([[2.0, 270.0], [3.0, 272.0], [4.0, null], [6.0, 271.0]])
        );
        assert_eq!(
            s["amplitude_tock"],
            json!([[2.0, 268.0], [3.0, null], [6.0, 271.0]])
        );
        assert_eq!(
            s["beat_error_unlock"],
            json!([[2.0, -0.38], [3.0, null], [6.0, -0.36]])
        );
        assert_eq!(
            s["beat_error_drop"],
            json!([[3.0, -0.41], [4.0, null]]),
            "the drop's only where core fell back to it"
        );
        // Each chart's scale is core's, over every point of its lines.
        let range = |ys: &[f64]| {
            let (lo, hi) = timegrapher_core::chart_scale::data_range(ys).unwrap();
            json!([lo, hi])
        };
        assert_eq!(s["y_range"]["rate"], range(&[4.5, 4.7, 4.9]));
        assert_eq!(s["y_range"]["amplitude"], range(&[268.0, 272.0]));
        assert_eq!(s["y_range"]["beat_error"], range(&[-0.36, -0.38, -0.41]));
    }

    #[test]
    fn thins_long_runs() {
        let pts: Vec<[f64; 2]> = (0..10_000).map(|i| [i as f64, 0.0]).collect();
        assert!(thin(pts).len() <= MAX_POINTS);
        // A gap is kept wherever it falls, so the line still lifts there.
        let mut pts: Vec<[f64; 2]> = (0..10_000).map(|i| [i as f64, 0.0]).collect();
        pts[5001][1] = f64::NAN;
        assert!(thin(pts).iter().any(|p| p[0] == 5001.0 && p[1].is_nan()));
    }

    #[test]
    fn the_view_takes_the_apps_colours() {
        let page = page();
        assert!(!page.contains("/*PALETTE*/"));
        for c in [LIGHT.trace_rate, DARK.trace_rate, LIGHT.tick, DARK.tock] {
            assert!(page.contains(&c.css()), "{}", c.css());
        }
        // The call-out grey, in both themes.
        for c in [LIGHT.callout, DARK.callout] {
            let line = format!("--callout: {};", c.css());
            assert!(page.contains(&line), "{line}");
        }
        // The grid and the axes at the palette's own opacities.
        for p in [&LIGHT, &DARK] {
            for (name, a) in [("grid", p.grid_opacity), ("axis", p.axis_opacity)] {
                let line = format!("--{name}: {};", rgba(p.text, a));
                assert!(page.contains(&line), "{line}");
            }
        }
    }

    #[test]
    fn the_maps_go_to_the_view_and_the_model_reads_their_outline() {
        let map = json!({ "series": "amplitude", "method": "stft", "unit": "deg",
            "step_s": 2.0, "window_s": 1200.0, "hop_s": 300.0,
            "period_s": [400.0, 10.0], "t_s": [600.0, 900.0],
            "values": [[1.0, null], [2.0, 0.5]],
            "scale": { "linear": { "lo": 0.0, "hi": 2.0 }, "log": { "lo": 0.5, "hi": 2.0 } } });
        let mut doc = json!({
            "series": [{ "series": "amplitude", "headline": "Amplitude" }],
            "time_frequency": [map.clone()],
            "coherence": [{ "x": "rate", "y": "amplitude", "period_s": [60.0],
                "t_s": [5.0], "coherence": [[0.9]], "phase_deg": [[10.0]], "noise_95": [0.4] }],
        });
        let charts = take_series_charts(&mut doc);
        // The view writes the unit as core does, degrees with no gap (#1428).
        let mut map = map;
        map["unit_text"] = json!("°");
        assert_eq!(charts["maps"], json!([map]));
        assert!(doc.get("time_frequency").is_none());
        assert_eq!(
            doc["time_frequency_maps"],
            json!([{ "series": "amplitude", "method": "stft", "unit": "deg",
                "step_s": 2.0, "window_s": 1200.0, "hop_s": 300.0,
                "rows": 2, "shortest_period_s": 10.0, "longest_period_s": 400.0,
                "columns": 2, "first_t_s": 600.0, "last_t_s": 900.0,
                "cells_read": 3, "cells": 4, "scale": map["scale"] }])
        );
        // The coherence keeps everything but its grids.
        assert_eq!(
            doc["coherence"],
            json!([{ "x": "rate", "y": "amplitude", "period_s": [60.0], "t_s": [5.0],
                "noise_95": [0.4] }])
        );
    }

    #[test]
    fn the_view_takes_the_cores_viridis() {
        let page = page();
        assert!(!page.contains("/*VIRIDIS*/"));
        let table = serde_json::to_string(&timegrapher_core::palette::VIRIDIS).unwrap();
        assert!(page.contains(&format!("const VIRIDIS = {table};")));
    }

    #[test]
    fn the_view_marks_periods_from_the_cores_list() {
        let page = page();
        assert!(!page.contains("/*PERIOD_TICKS*/"));
        let ticks = serde_json::to_string(&timegrapher_core::period_ticks::TICKS).unwrap();
        assert!(page.contains(&format!("const PERIOD_TICKS = {ticks};")));
        assert!(!page.contains("/*REFERENCE_HOVER*/"));
        let hover = timegrapher_core::steady_views::reference_hover("1 min");
        assert!(page.contains(&format!("\"1 min\":\"{hover}\"")));
        assert!(!page.contains("/*SCORE_WORDS*/"));
        assert!(!page.contains("/*WALK_WORDS*/"));
        assert!(!page.contains("/*TIME_MAP*/"));
        assert!(page.contains(&format!("const TIME_MAP = {};", time_map())));
        let false_alarm = timegrapher_core::longterm::LongConfig::default().false_alarm;
        let hover = timegrapher_core::steady_views::threshold_hover(false_alarm);
        assert!(page.contains(&hover));
        assert!(!page.contains("/*MARK_ROOM_PX*/"));
        let room = timegrapher_core::period_ticks::MARK_ROOM_PX;
        assert!(page.contains(&format!("const MARK_ROOM_PX = {room};")));
    }

    #[test]
    fn the_view_names_positions_as_core_names_them() {
        let page = page();
        assert!(!page.contains("/*POSITIONS*/"));
        assert!(
            !HTML.contains("\"Dial Up\""),
            "the page writes no name of its own"
        );
        let at = page.find("const POSITIONS = ").unwrap() + 18;
        let end = at + page[at..].find(';').unwrap();
        let names: Map<String, Value> = serde_json::from_str(&page[at..end]).unwrap();
        assert_eq!(names.len(), 12);
        assert_eq!(names["CH"], json!(["DU", "Dial Up"]));
        assert_eq!(names["DU"], names["CH"]);
        assert_eq!(names["3H"], json!(["CU", "Crown Up"]));
        assert_eq!(names["CR"], names["12H"]);
    }

    #[test]
    fn the_view_places_probability_marks_by_the_cores_normal_scores() {
        let page = page();
        assert!(!page.contains("/*PROBABILITY_MARKS*/"));
        assert!(
            !page.contains("normalQuantile"),
            "the page works out no normal scores"
        );
        let at = page.find("const PROBABILITY_MARKS = ").unwrap() + 26;
        let end = at + page[at..].find(';').unwrap();
        let marks: Vec<(String, f64)> = serde_json::from_str(&page[at..end]).unwrap();
        assert_eq!(marks.len(), 11);
        assert_eq!(marks[5], ("50%".to_string(), 0.0));
        assert_eq!(marks[9].0, "99%");
        assert_eq!(marks[9].1, normal_quantile(0.99));
        assert!((marks[0].1 + marks[10].1).abs() < 1e-12);
    }

    #[test]
    fn the_view_needs_no_network() {
        for banned in ["http://", "https://", "<script src", "@import", "fetch("] {
            assert!(!page().contains(banned), "view.html contains {banned}");
        }
    }

    /// The view's tests draw on the map the page gets: core's time colour
    /// map as `time_map` reads it, kept in a fixture they read
    /// (TIMEGRAPHER_BLESS=1 writes it afresh).
    #[test]
    fn the_time_map_fixture_is_core_s() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/time_map.json");
        let now = time_map();
        if std::env::var_os("TIMEGRAPHER_BLESS").is_some() {
            std::fs::write(&path, format!("{now}\n")).unwrap();
        }
        let kept = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            kept.trim(),
            now,
            "run with TIMEGRAPHER_BLESS=1 to write the fixture"
        );
    }
}
