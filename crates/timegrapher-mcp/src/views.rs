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
use timegrapher_core::histogram::normal_quantile;
use timegrapher_core::palette::{Palette, Rgb, DARK, LIGHT};

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
        HTML.replace("/*PALETTE*/", &css)
            .replace("/*VIRIDIS*/null", &viridis)
            .replace("/*PERIOD_TICKS*/null", &period_ticks)
            .replace("/*PROBABILITY_MARKS*/null", &marks)
            .replace("/*POSITIONS*/null", &positions)
    })
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

/// One theme as the page's CSS variables.
fn variables(p: &Palette) -> String {
    let rgba = |c: Rgb, a: f64| format!("rgba({}, {}, {}, {a})", c.r, c.g, c.b);
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
        ("unlock", p.unlock.css()),
        ("drop", p.drop.css()),
        ("peak", p.peak.css()),
        ("sound", p.sound.css()),
        ("wheel", p.wheel.css()),
        // The grid is the text colour, faint, as in the app.
        ("grid", rgba(p.text, 0.12)),
    ]
    .iter()
    .map(|(name, value)| format!("  --{name}: {value};\n"))
    .collect()
}

/// Most points drawn per series: plenty for a chat-sized chart, and it keeps
/// a day-long run's result small.
const MAX_POINTS: usize = 1500;

/// The `_meta` key the views read their ± texts from (#806).
pub const PM_KEY: &str = "timegrapher/pm";

fn pm_of(se: &Value, digits: usize) -> Value {
    se.as_f64()
        .and_then(|se| timegrapher_core::library::pm_text(se, digits))
        .map_or(Value::Null, Value::from)
}

/// The ± beside each tile's figure, as core's `library::pm_text` writes it,
/// so the view shows the app's and the CLI's text instead of rounding its
/// own: for `analyze`, the whole file's rate and amplitude and each take
/// reading's, in the take's order; for a live reading, its amplitude. A
/// null where there is no standard error; `None` for other results.
pub fn pm_texts(doc: &Value) -> Option<Value> {
    let schema = doc["schema"].as_str().unwrap_or("");
    let tiles = |rate: &Value, amplitude: &Value| json!({ "rate": pm_of(rate, 1), "amplitude": pm_of(amplitude, 0) });
    if schema.starts_with("timegrapher.analyze/") {
        let readings: Vec<Value> = doc["take"]["readings"]
            .as_array()
            .map(|rs| {
                rs.iter()
                    .map(|r| {
                        let c = &r["measurement"]["confidence"];
                        tiles(&c["rate"]["se"], &c["amplitude"]["se"])
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut pm = tiles(&doc["confidence"]["rate"]["se"], &doc["amplitude_se_deg"]);
        pm["readings"] = json!(readings);
        return Some(pm);
    }
    if schema.starts_with("timegrapher.live/") {
        return Some(json!({ "amplitude": pm_of(&doc["reading"]["amplitude_error_deg"], 0) }));
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
        for (k, c) in cols.iter().enumerate() {
            match get(*c) {
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
    json!({
        "views": views, "period_span": period_span, "spectra": spectra, "points": points,
        "maps": maps,
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
/// stretch ([`analysis_take::shape_stretch`]): the first
/// [`analysis_take::SHAPE_S`] once the watch has settled
/// ([`take::measured`]), with nothing masked. None for an empty recording
/// or one with no regular beat.
///
/// [`analysis_take::shape_stretch`]: timegrapher_core::analysis_take::shape_stretch
/// [`analysis_take::SHAPE_S`]: timegrapher_core::analysis_take::SHAPE_S
/// [`take::measured`]: timegrapher_core::take::measured
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
    let duration_s = doc["duration_s"].as_f64().unwrap_or(0.0);
    let (from, to) = timegrapher_core::take::measured(0.0, duration_s, None, None);
    timegrapher_core::analysis_take::shape_stretch(&[], from, to)
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
    fn the_view_gets_its_plus_or_minus_in_cores_words() {
        // The view rounded 0.4° up to "± 1°" (#806); core keeps a figure.
        let doc = json!({
            "schema": "timegrapher.analyze/1", "amplitude_se_deg": 0.4,
            "confidence": { "rate": { "se": 0.06 } },
            "take": { "readings": [
                { "measurement": { "confidence": { "rate": { "se": 1.24 }, "amplitude": { "se": 2.6 } } } },
                { "measurement": { "watch_heard": false } },
            ] },
        });
        assert_eq!(
            pm_texts(&doc),
            Some(json!({ "rate": "0.06", "amplitude": "0.4", "readings": [
                { "rate": "1.2", "amplitude": "3" },
                { "rate": null, "amplitude": null },
            ] }))
        );
        let live =
            json!({ "schema": "timegrapher.live/1", "reading": { "amplitude_error_deg": 0.4 } });
        assert_eq!(pm_texts(&live), Some(json!({ "amplitude": "0.4" })));
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
            assert_eq!(
                charts["spectra"][0], spectrum,
                "all chart arrays and support unchanged"
            );
            assert!(doc.get("spectra").is_none());
            assert_eq!(
                doc["spectrum_support"],
                json!([{"series":"rate", "unit":"ms", "support":support}])
            );
            let lead = crate::summary::lead(&doc, &serde_json::Map::new()).unwrap();
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
            json!([[2.0, -0.39], [3.0, -0.41], [4.0, null], [6.0, -0.40]])
        );
        // Each chart's scale is core's, over every point of its lines.
        let range = |ys: &[f64]| {
            let (lo, hi) = timegrapher_core::chart_scale::data_range(ys).unwrap();
            json!([lo, hi])
        };
        assert_eq!(s["y_range"]["rate"], range(&[4.5, 4.7, 4.9]));
        assert_eq!(s["y_range"]["amplitude"], range(&[268.0, 272.0]));
        assert_eq!(
            s["y_range"]["beat_error"],
            range(&[-0.36, -0.38, -0.39, -0.40, -0.41])
        );
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
        // The wheels' own colour, in both themes.
        for c in [LIGHT.wheel, DARK.wheel] {
            assert!(
                page.contains(&format!("--wheel: {};", c.css())),
                "{}",
                c.css()
            );
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
}
