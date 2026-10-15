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
        HTML.replace("/*PALETTE*/", &css)
    })
}

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
    (vec![format!("--windows={}", f.0.display())], f)
}

/// The windows file written by `analyze --windows` (see [`analyze_extras`])
/// as the three series of the strip.
fn strip(csv: &str, duration_s: f64) -> Value {
    let mut lines = csv.lines();
    let header: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
    let col = |name: &str| header.iter().position(|h| *h == name);
    let (Some(kind), Some(start), Some(end)) = (col("kind"), col("start_s"), col("end_s")) else {
        return Value::Null;
    };
    let rate = col("rate_s_per_day");
    let be = col("beat_error_ms");
    let be_unlock = col("beat_error_unlock_ms");
    let amp = [col("amplitude_even_deg"), col("amplitude_odd_deg")];
    let mut rates = Vec::new();
    let (mut amps, mut ticks, mut tocks) = (Vec::new(), Vec::new(), Vec::new());
    let (mut unlocks, mut drops) = (Vec::new(), Vec::new());
    for line in lines {
        let f: Vec<&str> = line.split(',').collect();
        let get = |i: Option<usize>| {
            i.and_then(|i| f.get(i))
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite())
        };
        let (Some(a), Some(b)) = (get(Some(start)), get(Some(end))) else {
            continue;
        };
        let t = (a + b) / 2.0;
        match f.get(kind).copied() {
            Some("rate") => {
                if let Some(v) = get(rate) {
                    rates.push([t, v]);
                }
            }
            Some("amplitude") => {
                // As the app's Amplitude chart: the average of the two sides,
                // and each side's own (the Tick is the even beats).
                let (tick, tock) = (get(amp[0]), get(amp[1]));
                let sides: Vec<f64> = [tick, tock].into_iter().flatten().collect();
                if !sides.is_empty() {
                    amps.push([t, sides.iter().sum::<f64>() / sides.len() as f64]);
                }
                if let Some(v) = tick {
                    ticks.push([t, v]);
                }
                if let Some(v) = tock {
                    tocks.push([t, v]);
                }
                // Signed, as the app draws them: the unlock's is the headline.
                if let Some(v) = get(be_unlock) {
                    unlocks.push([t, v]);
                }
                if let Some(v) = get(be) {
                    drops.push([t, v]);
                }
            }
            _ => {}
        }
    }
    json!({
        "duration_s": duration_s,
        "rate": thin(rates),
        "amplitude": thin(amps),
        "amplitude_tick": thin(ticks),
        "amplitude_tock": thin(tocks),
        "beat_error_unlock": thin(unlocks),
        "beat_error_drop": thin(drops),
    })
}

/// Every k-th point, so at most [`MAX_POINTS`] remain.
fn thin(points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let k = points.len().div_ceil(MAX_POINTS).max(1);
    points.into_iter().step_by(k).collect()
}

/// Takes out of a `series --views` document what only the Steadiness and
/// Distribution charts draw (each series' `views`, the shared
/// `period_span`, the `spectra` the FFT view draws and each distribution's
/// probability-plot `points`, a few
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
    json!({ "views": views, "period_span": period_span, "spectra": spectra, "points": points })
}

/// What the Recording view needs beyond `analyze`'s document: the strip
/// from the windows file, and the Tick and Tock profiles over ten seconds
/// in the middle of the recording.
pub async fn analyze_charts(doc: &Value, extras: &[String], recording: &str) -> Value {
    let mut out = Map::new();
    let duration = doc["duration_s"].as_f64().unwrap_or(0.0);
    if let Some(path) = extras.first().and_then(|e| e.strip_prefix("--windows=")) {
        if let Ok(csv) = std::fs::read_to_string(path) {
            out.insert("strip".into(), strip(&csv, duration));
        }
    }
    if doc["watch_heard"] != json!(false) && duration > 0.0 {
        let span = duration.min(10.0);
        let at = (duration / 2.0 - span / 2.0).max(0.0);
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
        if let Ok(cli::Outcome {
            json: Some(profile),
            ..
        }) = cli::run(&line).await
        {
            out.insert("profile".into(), profile);
        }
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn strip_from_windows() {
        let csv = "kind,start_s,end_s,rate_s_per_day,beat_error_ms,beat_error_unlock_ms,amplitude_even_deg,amplitude_odd_deg\n\
                   rate,0,10,4.5,-0.4,,,\n\
                   amplitude,0,2,,-0.39,-0.38,270,268\n\
                   amplitude,2,4,,-0.41,,272,\n";
        let s = strip(csv, 30.0);
        assert_eq!(s["rate"], json!([[5.0, 4.5]]));
        assert_eq!(s["amplitude"], json!([[1.0, 269.0], [3.0, 272.0]]));
        assert_eq!(s["amplitude_tick"], json!([[1.0, 270.0], [3.0, 272.0]]));
        assert_eq!(s["amplitude_tock"], json!([[1.0, 268.0]]));
        assert_eq!(s["beat_error_unlock"], json!([[1.0, -0.38]]));
        assert_eq!(s["beat_error_drop"], json!([[1.0, -0.39], [3.0, -0.41]]));
    }

    #[test]
    fn thins_long_runs() {
        let pts: Vec<[f64; 2]> = (0..10_000).map(|i| [i as f64, 0.0]).collect();
        assert!(thin(pts).len() <= MAX_POINTS);
    }

    #[test]
    fn the_view_takes_the_apps_colours() {
        let page = page();
        assert!(!page.contains("/*PALETTE*/"));
        for c in [LIGHT.trace_rate, DARK.trace_rate, LIGHT.tick, DARK.tock] {
            assert!(page.contains(&c.css()), "{}", c.css());
        }
    }

    #[test]
    fn the_view_needs_no_network() {
        for banned in ["http://", "https://", "<script src", "@import", "fetch("] {
            assert!(!page().contains(banned), "view.html contains {banned}");
        }
    }
}
