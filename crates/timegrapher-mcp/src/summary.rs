//! The plain-words lead of a result: the readings with their standard
//! errors, what limits them, and, for a multi-position test, the
//! characteristic values with what each one compares. It comes before the
//! JSON, so a model reads it first and does not have to find the caveats
//! in the document, or guess which amplitude difference is which. Facts
//! only: no grades or verdicts.

use serde_json::{Map, Value};

/// " ± se" from a confidence entry, to at least `digits` decimals and
/// enough to show a figure that is not zero.
fn pm(c: &Value, digits: usize) -> String {
    let Some(se) = c["se"].as_f64().filter(|v| v.is_finite() && *v > 0.0) else {
        return String::new();
    };
    let mut d = digits;
    while d < 3 && (se * 10f64.powi(d as i32)).round() == 0.0 {
        d += 1;
    }
    format!(" ± {se:.d$}")
}

fn signed(v: f64, digits: usize) -> String {
    timegrapher_core::library::signed_text(v, digits)
}

/// The rate the CLI chose: corrected for the sound card's clock where the
/// document says which correction it applied (`clock`: one stored or
/// given, else the recording's USB lines), else as read on the card's
/// clock. The choice is the CLI's; this only reads it.
fn analyze_rate(doc: &Value) -> Option<(f64, String)> {
    if let Some(rate) = doc["clock"]["rate_s_per_day"].as_f64() {
        let source = doc["clock"]["from"]
            .as_str()
            .unwrap_or("correction supplied by the CLI");
        Some((
            rate,
            format!("corrected for the sound card's clock ({source})"),
        ))
    } else {
        doc["overall"]["rate_s_per_day"]
            .as_f64()
            .map(|rate| (rate, "on the sound card's clock".into()))
    }
}

/// The whole recording's beat error from the drop: its overall fit's.
fn analyze_drop(doc: &Value) -> Option<f64> {
    doc["overall"]["beat_error_ms"].as_f64()
}

/// One reading's rate (s/day, as given), amplitude (°) and beat error (ms):
/// the beat error is core's choice of the reading's signed unlock and drop
/// figures (`drop_ms`), as a size.
fn figures(m: &Value, rate: Option<f64>, drop_ms: Option<f64>) -> [Option<f64>; 3] {
    [
        rate,
        m["amplitude_deg"].as_f64(),
        timegrapher_core::session::beat_error_size(m["beat_error_unlock_ms"].as_f64(), drop_ms),
    ]
}

/// One reading's figures ([`figures`]): rate, amplitude and beat error,
/// each with its standard error where `m` gives one.
fn readings(m: &Value, [rate, amplitude, beat_error]: [Option<f64>; 3]) -> Option<String> {
    let conf = &m["confidence"];
    let mut parts = Vec::new();
    if let Some(r) = rate {
        parts.push(format!(
            "rate {}{} s/day",
            signed(r, 1),
            pm(&conf["rate"], 1)
        ));
    }
    if let Some(a) = amplitude {
        parts.push(format!("amplitude {a:.0}{}°", pm(&conf["amplitude"], 0)));
    }
    if let Some(b) = beat_error {
        parts.push(format!(
            "beat error {:.2}{} ms",
            b,
            pm(&conf["beat_error"], 2)
        ));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// A position as the app and the text report name it, from core
/// ([`Position::display_label`]): "Dial Up (DU)", "Crown Up (CU, 3H)".
/// A code core doesn't know stays as given.
///
/// [`Position::display_label`]: timegrapher_core::session::Position::display_label
pub(crate) fn position_name(p: &str) -> String {
    timegrapher_core::session::Position::parse(p)
        .map_or_else(|| p.to_string(), |p| p.display_label())
}

fn count(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The folders the library scan couldn't read, as a sentence; empty when
/// there are none.
fn unreadable(doc: &Value) -> String {
    match doc["unreadable"].as_array().map_or(0, Vec::len) {
        0 => String::new(),
        n => format!(
            " {} in the library couldn't be read (`unreadable`).",
            count(n as u64, "folder", "folders")
        ),
    }
}

/// `watches list`: how many watches and takes, and what waits in Unsorted.
fn watches(doc: &Value) -> String {
    let list = doc["watches"].as_array().cloned().unwrap_or_default();
    let takes: u64 = list.iter().filter_map(|w| w["takes"].as_u64()).sum();
    let mut s = format!(
        "{}, {}.",
        count(list.len() as u64, "watch", "watches"),
        count(takes, "take", "takes")
    );
    match doc["unsorted"].as_u64().unwrap_or(0) {
        0 => {}
        n => {
            s += &format!(
                " {} in Unsorted, made with Quick Listen.",
                count(n, "take waits", "takes wait")
            )
        }
    }
    s + &unreadable(doc)
}

/// `watches show`: the watch and how many takes it has, how many kept
/// only their analysis, and the newest.
fn watch(doc: &Value) -> String {
    let takes = doc["takes"].as_array().cloned().unwrap_or_default();
    let name = if doc["unsorted"] == true {
        "Unsorted (Quick Listen takes)".to_string()
    } else {
        doc["name"].as_str().unwrap_or("The watch").to_string()
    };
    let mut s = format!("{name}: {}", count(takes.len() as u64, "take", "takes"));
    let only = takes.iter().filter(|t| t["audio"] == false).count() as u64;
    if only > 0 {
        s += &format!(", {only} kept as their analysis only");
    }
    if let Some(t) = takes.first().and_then(|t| t["started_utc"].as_str()) {
        s += &format!("; the newest started {t}");
    }
    s += ".";
    s + &unreadable(doc)
}

/// The lead for `analyze`, `session`, `record` and `series` documents;
/// `None` for others. `args` are the tool call's, to tell a given lift angle from the
/// default.
pub fn lead(doc: &Value, args: &Map<String, Value>) -> Option<String> {
    let schema = doc["schema"].as_str().unwrap_or("");
    let mut lines = Vec::new();
    if schema.starts_with("timegrapher.record/") {
        return Some(take(doc, args));
    }
    if schema.starts_with("timegrapher.series/") {
        return with_limits(series(doc).into_iter().collect(), doc);
    }
    // A long run's figures are the JSON's; the lead says only what limits them.
    if schema.starts_with("timegrapher.long/") {
        return with_limits(Vec::new(), doc);
    }
    if schema.starts_with("timegrapher.watches/") {
        return Some(watches(doc));
    }
    if schema.starts_with("timegrapher.watch/") {
        return Some(watch(doc));
    }
    if schema.starts_with("timegrapher.analyze/") {
        lines.extend(take_lines(&doc["take"]));
        if doc["take"]["readings"].is_array() {
            lines.push(
                "The whole file read alone (no settling, none of the take's settings):".to_string(),
            );
        }
        if doc["watch_heard"] == false {
            lines.push(crate::caveats::no_regular_beat(doc, doc["bph"].as_f64()));
        } else {
            let selected = analyze_rate(doc);
            let found = figures(doc, selected.as_ref().map(|r| r.0), analyze_drop(doc));
            if let Some(r) = readings(doc, found) {
                lines.push(format!("Readings: {r}."));
            }
            if let Some((_, basis)) = selected {
                lines.push(format!("Rate basis: {basis}."));
            }
        }
    } else if schema.starts_with("timegrapher.session/") {
        if let Some(l) = power_reserve(&doc["power_reserve"]) {
            lines.push(l);
        }
        lines.push("Readings by position:".to_string());
        lines.extend(position_lines(&doc["readings"]));
        for s in doc["report"]["states"].as_array().into_iter().flatten() {
            lines.push(characteristic_values(s));
        }
    } else {
        return None;
    }
    with_limits(lines, doc)
}

/// One line for each of `session`'s readings: its position, and its
/// figures or that no regular beat was found.
fn position_lines(list: &Value) -> Vec<String> {
    list.as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let m = &r["measurement"];
            let mut name = position_name(r["position"].as_str().unwrap_or(""));
            if let Some(h) = r["wind_h"].as_f64().filter(|h| *h > 0.0) {
                name += &format!(" after {h} h");
            }
            let said = if m["watch_heard"] == false {
                "no regular beat found".to_string()
            } else {
                readings(m, session_figures(m)).unwrap_or_default()
            };
            format!("- {name}: {said}")
        })
        .collect()
}

/// A `session` reading's figures as the lead gives them ([`figures`]).
fn session_figures(m: &Value) -> [Option<f64>; 3] {
    figures(m, m["rate_s_per_day"].as_f64(), m["beat_error_ms"].as_f64())
}

/// What `analyze`'s `take` block says `session` reads of the file: a line
/// for each entry naming it, and for each left out. Empty with no block.
fn take_lines(take: &Value) -> Vec<String> {
    if take.is_null() {
        return Vec::new();
    }
    if let Some(error) = take["error"].as_str() {
        return vec![format!(
            "The session file in this file's folder couldn't be read for it ({error}); \
             the readings below are the whole file's."
        )];
    }
    let mut lines =
        vec!["The take's session file lists this file; `session` reads it:".to_string()];
    lines.extend(position_lines(&take["readings"]));
    for l in take["left_out"].as_array().into_iter().flatten() {
        lines.push(format!(
            "- {}: left out, {}",
            position_name(l["position"].as_str().unwrap_or("")),
            l["why"].as_str().unwrap_or("")
        ));
    }
    lines
}

/// The figures the lead gives for each reading of an `analyze`
/// document's `take` block ([`session_figures`]).
#[cfg(test)]
pub fn take_figures(doc: &Value) -> Vec<[Option<f64>; 3]> {
    doc["take"]["readings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| session_figures(&r["measurement"]))
        .collect()
}

/// The lead `lines` with what limits the readings after them, or `None`
/// when both are empty.
fn with_limits(mut lines: Vec<String>, doc: &Value) -> Option<String> {
    // A sentence the lead already says (no regular beat found) is not repeated.
    let limits: Vec<String> = crate::caveats::notes(doc)
        .into_iter()
        .filter(|l| !lines.contains(l))
        .collect();
    if !limits.is_empty() {
        lines.push(if doc["take"]["readings"].is_array() {
            "What limits the take's readings:".to_string()
        } else {
            "What limits these readings:".to_string()
        });
        lines.extend(limits.into_iter().map(|l| format!("- {l}")));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// A Power Reserve take's run, as the CLI's text gives it, with the stops
/// on the way and the hours read.
fn power_reserve(p: &Value) -> Option<String> {
    let run_s = p["run_s"].as_f64()?;
    let ending: timegrapher_core::power_reserve::Ending =
        serde_json::from_value(p["ending"].clone()).ok()?;
    let position = position_name(p["position"].as_str().unwrap_or(""));
    let mut s = format!(
        "Power reserve: {} in {position}. {}.",
        timegrapher_core::power_reserve::run_text(run_s),
        ending.text()
    );
    let pauses = p["pauses"].as_array().map_or(0, |a| a.len()) as u64;
    if pauses > 0 {
        s += &format!(
            " The beats stopped for 30 s or more and came back {} on the way.",
            if pauses == 1 {
                "once".to_string()
            } else {
                format!("{pauses} times")
            }
        );
    }
    let hours = p["hours"].as_array().map_or(0, |a| a.len()) as u64;
    if hours > 0 {
        s += &format!(
            " Measured hour by hour: {} in `hours`.",
            count(hours, "hour", "hours")
        );
    }
    Some(s)
}

/// A recorded take: what was recorded, what its quick check measured, the
/// lift angle and where it came from, the beats found and clipped, and the
/// test it joined. Facts only: no grades.
fn take(doc: &Value, args: &Map<String, Value>) -> String {
    let c = &doc["check"];
    let pos = position_name(doc["position"].as_str().unwrap_or(""));
    let mut lines = vec![match doc["seconds"].as_f64() {
        Some(s) => format!("Recorded {pos} for {s:.0} s."),
        None => format!("Recorded {pos}."),
    }];
    let window = match (c["from_s"].as_f64(), c["to_s"].as_f64()) {
        (Some(a), Some(b)) => format!(" from {a:.0} to {b:.0} s"),
        _ => String::new(),
    };
    if c["watch_heard"] == false {
        let bph = c["bph"].as_u64().map(|b| b as u32);
        let said = timegrapher_core::session::no_regular_beat(
            c["timing_scatter_ms"]
                .as_f64()
                .filter(|_| bph.is_some())
                .map(|ms| ms * 1000.0),
            bph.unwrap_or(0),
        );
        lines.push(format!("{said} over the whole take."));
    } else {
        let mut parts = Vec::new();
        if let Some(r) = c["rate_s_per_day"].as_f64() {
            parts.push(format!("rate {} s/day", signed(r, 1)));
        }
        if let Some(a) = c["amplitude_deg"].as_f64() {
            parts.push(format!("amplitude {a:.0}°"));
        }
        if let Some(b) = c["beat_error_ms"].as_f64() {
            parts.push(format!("beat error {:.2} ms", b.abs()));
        }
        if !parts.is_empty() {
            lines.push(format!("The check{window} measured {}.", parts.join(", ")));
        }
    }
    let settings = &doc["input"]["settings"];
    if let Some(l) = settings["lift_deg"].as_f64() {
        let given = |k: &str| args.get(k).is_some_and(|v| !v.is_null());
        let from = if given("lift_deg") {
            "as given".to_string()
        } else if let Some(cal) = settings["calibre"].as_str() {
            format!("the {cal}'s")
        } else {
            "the default, since no calibre or lift angle was given; the amplitude \
             depends on it"
                .to_string()
        };
        lines.push(format!("Lift angle {l}°: {from}."));
    }
    if let (Some(f), Some(e)) = (c["beats_found"].as_u64(), c["beats_expected"].as_u64()) {
        lines.push(format!("{f} of {e} expected beats were found."));
    }
    if let Some(n) = c["clipping"]["beats"].as_u64() {
        let pct = c["clipping"]["beat_fraction"].as_f64().unwrap_or(0.0) * 100.0;
        lines.push(if n == 0 {
            "No beats were clipped.".to_string()
        } else {
            let beats = if n == 1 { "beat" } else { "beats" };
            let were = if n == 1 { "was" } else { "were" };
            format!("{n} {beats} ({pct:.1}%) {were} clipped.")
        });
    }
    // Every take joins the test, whatever its sound: the clipping and
    // no-regular-beat facts above go with it.
    if let Some(file) = doc["session_file"].as_str() {
        lines.push("The take was added to the test.".to_string());
        // Without a folder the test's folder is new: the next position
        // goes into it.
        if let (Some(w), Some(dir)) = (
            doc["library_watch"].as_str(),
            std::path::Path::new(file).parent(),
        ) {
            lines.push(format!(
                "The test's folder, made among {w}'s takes, is {}.",
                dir.display()
            ));
        }
    }
    lines.retain(|l| !l.is_empty());
    lines.join(" ")
}

/// A long take's series: each one's headline as `series` words it, with
/// the readings behind it, then the findings' titles. Facts only: no word
/// or colour stands beside a headline.
fn series(doc: &Value) -> Option<String> {
    let mut lines = Vec::new();
    for s in doc["series"].as_array().into_iter().flatten() {
        let Some(h) = s["headline"].as_str() else {
            continue;
        };
        let unit = match s["unit"].as_str().unwrap_or("") {
            "deg" => "°",
            u => u,
        };
        let mut facts = Vec::new();
        if let (Some(n), Some(step)) = (s["readings"].as_u64(), s["step_s"].as_f64()) {
            facts.push(format!("{n} readings of {step:.0} s"));
        }
        let mut say = |key: &str, what: &str| {
            if let Some(v) = s[key].as_f64() {
                let gap = if unit == "°" { "" } else { " " };
                facts.push(format!("{what} {v:.2}{gap}{unit}"));
            }
        };
        say("median", "median");
        say("sd", "SD");
        say("short_term_sd", "reading to reading");
        let h = h.trim_end_matches('.');
        // The steady headline already gives the readings, their median and
        // their scatter ("Rate: 360 10 s readings, median ..."), and the
        // too-short one says there are too few. Other headlines may name
        // readings too ("10 s readings are too coarse to show it") without
        // giving these figures, so the word alone is not the key.
        let stated = h.contains(" readings, median ") || h.contains("too few");
        // The first sentence is the series' own; any after it ("The rate
        // also repeats ...", "The beat timing repeats every 5.0 s ...")
        // says something more, so the figures go after the first. A later
        // sentence starts with a capital ("The", "Tick", "Tock"); a ". "
        // inside one, as in a user's wheel named "sec. wheel", does not.
        let end = h
            .match_indices(". ")
            .map(|(i, _)| i)
            .find(|&i| h[i + 2..].starts_with(|c: char| c.is_uppercase()));
        let (first, rest) = end.map_or((h, ""), |i| (&h[..i], &h[i + 2..]));
        let rest = if rest.is_empty() {
            String::new()
        } else {
            format!(" {rest}.")
        };
        lines.push(if facts.is_empty() || stated {
            format!("- {h}.")
        } else {
            format!("- {first} ({}).{rest}", facts.join("; "))
        });
    }
    if lines.is_empty() {
        return None;
    }
    lines.insert(0, "Series:".to_string());
    if let Some(clock) = doc.get("clock") {
        if clock.is_null() {
            lines.insert(
                1,
                "Readings are on the sound card's clock; no correction applied.".into(),
            );
        } else if let Some(ppm) = clock["ppm"].as_f64() {
            let from = clock["from"].as_str().unwrap_or("source unspecified");
            let drift = if clock["tracks_drift"].as_bool() == Some(true) {
                "; mapping follows drift"
            } else {
                ""
            };
            lines.insert(
                1,
                format!("Readings use clock correction {ppm:+.3} ppm ({from}{drift})."),
            );
        }
    }
    if let Some(supports) = doc["spectrum_support"].as_array() {
        for s in supports {
            let series = s["series"].as_str().unwrap_or("series");
            // Core's own words for what the FFT read.
            lines.push(match s["support"]["description"].as_str() {
                Some(d) => format!("{series}: {d}"),
                None => format!("FFT {series} input support was not recorded."),
            });
        }
    }
    let found: Vec<&str> = doc["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f["title"].as_str())
        .collect();
    if !found.is_empty() {
        lines.push("Detected:".to_string());
        lines.extend(found.iter().map(|t| format!("- {t}")));
    }
    Some(lines.join("\n"))
}

/// Witschi's characteristic values for one state of wind, each named by
/// what it compares, with its field in the JSON.
fn characteristic_values(s: &Value) -> String {
    let at = match s["wind_h"].as_f64() {
        Some(h) if h > 0.0 => format!(" after {h} h"),
        _ => String::new(),
    };
    let mut parts = Vec::new();
    let mut say = |key: &str, what: &str, unit: &str, digits: usize, sign: bool| {
        if let Some(v) = s[key].as_f64() {
            let v = if sign {
                signed(v, digits)
            } else {
                format!("{v:.digits$}")
            };
            parts.push(format!("- {what}: {v}{unit} ({key})"));
        }
    };
    say("x", "mean rate over the positions", " s/day", 1, true);
    say(
        "d_rate",
        "largest rate difference between any two positions",
        " s/day",
        1,
        false,
    );
    say(
        "dvh_rate",
        "vertical minus horizontal rate (mean of the crown positions minus mean of dial up and dial down)",
        " s/day",
        1,
        true,
    );
    say(
        "amplitude_mean",
        "mean amplitude over the positions",
        "°",
        0,
        false,
    );
    say(
        "d_amplitude",
        "largest amplitude difference between any two positions (not vertical against horizontal)",
        "°",
        1,
        false,
    );
    say(
        "dvh_amplitude",
        "vertical minus horizontal amplitude (mean of the crown positions minus mean of dial up and dial down)",
        "°",
        1,
        true,
    );
    format!("Characteristic values{at}:\n{}", parts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn analyze_lead_honors_the_clis_selected_clock_basis() {
        let cases: Value =
            serde_json::from_str(include_str!("../tests/fixtures/analysis_rates.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let doc = case["doc"].clone();
            let before = doc.clone();
            let lead = lead(&doc, &Map::new()).unwrap();
            assert!(
                lead.starts_with(case["expected_lead"].as_str().unwrap()),
                "{}: {lead}",
                case["name"]
            );
            assert!(
                lead.contains(case["expected_basis"].as_str().unwrap()),
                "{}: {lead}",
                case["name"]
            );
            assert_eq!(doc, before, "summary must not replace raw or USB readings");
        }
    }

    #[test]
    fn a_recording_leads_with_its_readings() {
        let doc = json!({
            "schema": "timegrapher.analyze/2",
            "watch_heard": true,
            "overall": { "rate_s_per_day": 6.01, "beat_error_ms": -0.39 },
            "amplitude_deg": 269.6,
            "beat_error_unlock_ms": -0.4,
            "input": { "settings": { "lift_deg": 52.0 } },
            "confidence": { "rate": { "se": null },
                            "amplitude": { "se": 0.042 },
                            "beat_error": { "se": 0.004 } },
            "caveats": [{ "code": "lift_unknown" }],
        });
        let s = lead(&doc, &Map::new()).unwrap();
        assert!(
            s.starts_with(
                "Readings: rate +6.0 s/day, amplitude 270 ± 0.04°, beat error 0.40 ± 0.004 ms."
            ),
            "{s}"
        );
        for word in ["confidence", "poor", "fair", "good", "sure"] {
            assert!(!s.contains(word), "{word}: {s}");
        }
        assert!(
            s.contains("What limits these readings:\n- The lift angle is the default 52°"),
            "{s}"
        );
    }

    /// Positions are named as core names them, whichever of its codes
    /// or aliases the document gives (#691).
    #[test]
    fn positions_are_named_as_core_names_them() {
        assert_eq!(position_name("DU"), "Dial Up (DU)");
        assert_eq!(position_name("CB"), "Dial Down (DD)");
        assert_eq!(position_name("3H"), "Crown Up (CU, 3H)");
        assert_eq!(position_name("PU"), "Crown Up (CU, 3H)");
        assert_eq!(position_name("12H"), "Crown Right (CR, 12H)");
        assert_eq!(position_name("XX"), "XX");
    }

    /// A file its take lists leads with what `session` reads of it, entry
    /// by entry, before the whole file read alone.
    #[test]
    fn a_takes_file_leads_with_what_session_reads_of_it() {
        let doc = json!({
            "schema": "timegrapher.analyze/2",
            "watch_heard": true,
            "overall": { "rate_s_per_day": 6.01, "beat_error_ms": -0.39 },
            "amplitude_deg": 269.6,
            "take": {
                "session_file": "take/session.toml",
                "readings": [{ "position": "DU", "measurement": {
                    "rate_s_per_day": 5.2, "amplitude_deg": 281.0,
                    "beat_error_unlock_ms": null, "beat_error_ms": -0.3 } }],
                "left_out": [{ "position": "CU", "why": "nothing after settling" }],
            },
        });
        let s = lead(&doc, &Map::new()).unwrap();
        assert!(
            s.starts_with(
                "The take's session file lists this file; `session` reads it:\n\
                 - Dial Up (DU): rate +5.2 s/day, amplitude 281°, beat error 0.30 ms\n\
                 - Crown Up (CU, 3H): left out, nothing after settling\n\
                 The whole file read alone (no settling, none of the take's settings):\n\
                 Readings: rate +6.0 s/day, amplitude 270°, beat error 0.39 ms."
            ),
            "{s}"
        );
        assert_eq!(take_figures(&doc), [[Some(5.2), Some(281.0), Some(0.3)]]);
        let unread = json!({
            "schema": "timegrapher.analyze/2",
            "watch_heard": true,
            "overall": { "rate_s_per_day": 6.01 },
            "take": { "session_file": "take/session.toml", "error": "bad toml" },
        });
        let s = lead(&unread, &Map::new()).unwrap();
        assert!(
            s.starts_with(
                "The session file in this file's folder couldn't be read for it (bad toml); \
                 the readings below are the whole file's.\nReadings: rate +6.0 s/day"
            ),
            "{s}"
        );
    }

    /// The limits of a file its take lists are the take readings' own, not
    /// the whole file's: a take read on a measured clock has no clock caveat.
    #[test]
    fn a_takes_file_is_limited_by_the_takes_readings() {
        let doc = json!({
            "schema": "timegrapher.analyze/2",
            "watch_heard": true,
            "overall": { "rate_s_per_day": 5.0 },
            "caveats": [{ "code": "clock_unknown" }],
            "take": {
                "session_file": "take/session.toml",
                "readings": [{ "position": "DU", "measurement": {
                    "rate_s_per_day": 3.3, "caveats": [{ "code": "lift_unknown" }] } }],
            },
        });
        let limits = crate::caveats::limits(&doc);
        assert_eq!(limits.len(), 1, "{limits:?}");
        assert_eq!(limits[0]["where"], "Dial Up (DU)");
        assert!(
            limits[0]["text"].as_str().unwrap().contains("lift angle"),
            "{limits:?}"
        );
        let s = lead(&doc, &Map::new()).unwrap();
        assert!(
            s.contains("What limits the take's readings:\n- Dial Up (DU): The lift angle"),
            "{s}"
        );
        assert!(!s.contains("clock hasn't been measured"), "{s}");
    }

    #[test]
    fn the_two_amplitude_differences_are_told_apart() {
        let doc = json!({
            "schema": "timegrapher.session/1",
            "readings": [{ "position": "CH", "measurement": { "rate_s_per_day": 6.0,
                "amplitude_deg": 270.0, "beat_error_unlock_ms": 0.4 } }],
            "report": { "states": [{ "wind_h": 0.0, "d_amplitude": 28.4,
                "dvh_amplitude": -15.6, "x": 2.7 }] },
        });
        let s = lead(&doc, &Map::new()).unwrap();
        assert!(s.contains("- Dial Up (DU): rate +6.0 s/day"), "{s}");
        assert!(s.contains("largest amplitude difference between any two positions (not vertical against horizontal): 28.4° (d_amplitude)"), "{s}");
        assert!(s.contains("vertical minus horizontal amplitude (mean of the crown positions minus mean of dial up and dial down): -15.6° (dvh_amplitude)"), "{s}");
    }

    #[test]
    fn a_power_reserve_take_leads_with_its_run() {
        let doc = json!({
            "schema": "timegrapher.session/1",
            "power_reserve": { "schema": "power-reserve/1", "position": "DU",
                "ending": "stopped", "run_s": 151_380.0,
                "pauses": [{ "from_s": 100.0, "to_s": 140.0 }],
                "hours": [{ "hour": 0 }, { "hour": 1 }, { "hour": 2 }] },
            "readings": [],
        });
        let s = lead(&doc, &Map::new()).unwrap();
        assert!(
            s.starts_with(
                "Power reserve: 42 h 03 min in Dial Up (DU). The watch stopped. The \
                 beats stopped for 30 s or more and came back once on the way. Measured \
                 hour by hour: 3 hours in `hours`.\nReadings by position:"
            ),
            "{s}"
        );
        let mut by_hand = doc.clone();
        by_hand["power_reserve"]["ending"] = json!("by_hand");
        by_hand["power_reserve"]["pauses"] = json!([]);
        by_hand["power_reserve"]["hours"] = json!([]);
        assert!(lead(&by_hand, &Map::new()).unwrap().starts_with(
            "Power reserve: 42 h 03 min in Dial Up (DU). Stopped by hand while the watch \
             still ran.\nReadings"
        ));
        let mut plain = doc.clone();
        plain.as_object_mut().unwrap().remove("power_reserve");
        assert!(lead(&plain, &Map::new())
            .unwrap()
            .starts_with("Readings by position:"));
    }

    #[test]
    fn a_take_states_its_facts_and_its_lift_angle() {
        let doc = json!({
            "schema": "timegrapher.record/2",
            "input": { "settings": { "calibre": null, "lift_deg": 52.0 } },
            "position": "DU", "seconds": 30.0,
            "check": { "from_s": 15.0, "to_s": 30.0, "watch_heard": true,
                "beats_found": 118, "beats_expected": 120, "rate_s_per_day": 3.0,
                "amplitude_deg": 269.4, "beat_error_ms": -0.4,
                "clipping": { "beats": 0, "beat_fraction": 0.0 } },
            "session_file": "/tests/ym42/session.toml",
        });
        let s = lead(&doc, &Map::new()).unwrap();
        assert_eq!(
            s,
            "Recorded Dial Up (DU) for 30 s. The check from 15 to 30 s measured rate +3.0 \
             s/day, amplitude 269°, beat error 0.40 ms. Lift angle 52°: the default, since \
             no calibre or lift angle was given; the amplitude depends on it. 118 of 120 \
             expected beats were found. No beats were clipped. The take was added to the test."
        );
        assert!(!s.contains("confidence"));
        let given = json!({ "lift_deg": 50.0 }).as_object().unwrap().clone();
        assert!(lead(&doc, &given)
            .unwrap()
            .contains("Lift angle 52°: as given."));
    }

    #[test]
    fn a_take_with_no_regular_beat_is_still_added() {
        let doc = json!({
            "schema": "timegrapher.record/2",
            "position": "CU", "seconds": 30.0,
            "session_file": "/tests/ym42/session.toml",
            "check": { "from_s": 15.0, "to_s": 30.0, "watch_heard": false,
                "timing_scatter_ms": 8.6, "timing_scatter_fraction": 0.0688, "bph": 28800,
                "beats_found": 40, "beats_expected": 120,
                "clipping": { "beats": 3, "beat_fraction": 0.025 } },
        });
        let s = lead(&doc, &Map::new()).unwrap();
        assert_eq!(
            s,
            "Recorded Crown Up (CU, 3H) for 30 s. No regular beat found (timing scatter 8.6 ms, \
             6.9% of a beat) over the whole take. 40 of 120 expected beats were found. 3 \
             beats (2.5%) were clipped. The take was added to the test."
        );
        assert!(!s.contains("not added"));
    }

    #[test]
    fn a_take_in_a_library_watch_names_its_new_folder() {
        let doc = json!({
            "schema": "timegrapher.record/2",
            "position": "DU", "seconds": 30.0,
            "library_watch": "Pepsi",
            "session_file": "/lib/Pepsi/takes/2026-10-22 09.15/session.toml",
            "check": { "from_s": 15.0, "to_s": 30.0, "watch_heard": false,
                "beats_found": 0, "beats_expected": 120 },
        });
        let s = lead(&doc, &Map::new()).unwrap();
        assert!(
            s.ends_with(
                "The take was added to the test. The test's folder, made among Pepsi's \
                 takes, is /lib/Pepsi/takes/2026-10-22 09.15."
            ),
            "{s}"
        );
    }

    #[test]
    fn a_series_leads_with_each_headline_and_its_readings() {
        let doc = json!({
            "schema": "timegrapher.series/2",
            "series": [
                { "series": "rate", "unit": "s/d", "readings": 360, "step_s": 10.0,
                  "median": 3.1, "sd": 1.24, "short_term_sd": 0.9,
                  "headline": "Rate repeats every 60 min (third wheel), 2.4 s/d peak to peak." },
                { "series": "amplitude", "unit": "deg", "readings": 3, "step_s": 10.0,
                  "headline": "Amplitude: too few readings for the tests" },
            ],
            "findings": [{ "code": "periodic", "series": "rate",
                "title": "Rate repeats every 60 min", "evidence": "p < 0.001" }],
        });
        let s = lead(&doc, &Map::new()).unwrap();
        assert_eq!(
            s,
            "Series:\n\
             - Rate repeats every 60 min (third wheel), 2.4 s/d peak to peak (360 readings of \
             10 s; median 3.10 s/d; SD 1.24 s/d; reading to reading 0.90 s/d).\n\
             - Amplitude: too few readings for the tests.\n\
             Detected:\n\
             - Rate repeats every 60 min"
        );
    }

    #[test]
    fn a_headline_naming_readings_still_gets_the_figures_unless_it_gives_them() {
        let one = |step: f64, headline: &str| {
            let doc = json!({
                "schema": "timegrapher.series/2",
                "series": [{ "series": "rate", "unit": "s/d", "readings": 360, "step_s": step,
                    "median": 3.1, "sd": 1.24, "short_term_sd": 0.9, "headline": headline }],
            });
            lead(&doc, &Map::new()).unwrap()
        };
        let figures = "(360 readings of 10 s; median 3.10 s/d; SD 1.24 s/d; reading to \
                       reading 0.90 s/d)";
        // A level change with the period search's fast line, and correlated
        // readings, as the engine words them: the word "readings" is there,
        // the figures are not.
        for h in [
            "Rate shifts level 1 time: +2.0 s/d, then +4.0 s/d. The beat timing repeats \
             every 5.0 s, the escape wheel's period, 0.24 ms peak to peak; 10 s readings \
             are too coarse to show it.",
            "Successive 10 s rate readings are correlated (lag-1 correlation 0.40, \
             Ljung–Box p 0.002); no cycle, level change or trend detected.",
        ] {
            assert!(one(10.0, h).contains(figures), "{}", one(10.0, h));
        }
        // The figures are the series' own, so they follow its first
        // sentence, not a later one about the beat timing or a cycle.
        let text = one(
            10.0,
            "Rate shifts level 1 time: +2.0 s/d, then +4.0 s/d. The beat timing repeats \
             every 5.0 s, the escape wheel's period, 0.24 ms peak to peak; 10 s readings \
             are too coarse to show it.",
        );
        assert!(
            text.contains(&format!(
                "- Rate shifts level 1 time: +2.0 s/d, then +4.0 s/d {figures}. The beat \
                 timing repeats every 5.0 s, the escape wheel's period, 0.24 ms peak to peak; \
                 10 s readings are too coarse to show it."
            )),
            "{text}"
        );
        let text = one(
            10.0,
            "Rate slides from +1.0 s/d to +5.0 s/d (+0.5 s/d per hour). The rate also \
             repeats every 300 s (the third wheel), 1.2 s/d peak to peak.",
        );
        assert!(
            text.contains(&format!(
                "(+0.5 s/d per hour) {figures}. The rate also repeats every 300 s"
            )),
            "{text}"
        );
        // A user's wheel name may hold ". ": the figures still follow the
        // whole first sentence.
        let text = one(
            10.0,
            "Rate repeats every 60 s (the sec. wheel), 1.2 s/d peak to peak. The rate's \
             level also moves over the take, between +1.0 s/d and +3.0 s/d.",
        );
        assert!(
            text.contains(&format!(
                "(the sec. wheel), 1.2 s/d peak to peak {figures}. The rate's level"
            )),
            "{text}"
        );
        // After correlated readings the period search's other lines are a
        // sentence of their own; the figures stay with the first.
        let text = one(
            10.0,
            "Successive 10 s rate readings are correlated (lag-1 correlation 0.40, \
             Ljung–Box p 0.002); no level change or trend detected. The rate's period \
             search also finds 60 s (1.20 s/d peak to peak, the fourth wheel's period).",
        );
        assert!(
            text.contains(&format!(
                "no level change or trend detected {figures}. The rate's period search"
            )),
            "{text}"
        );
        // The steady and too-short headlines give them, or say there are too
        // few, themselves, whatever the reading length.
        for (step, h) in [
            (
                10.0,
                "Rate: 360 10 s readings, median +3.1 s/d, ±0.9 s/d from one reading to the \
                 next; no cycle, level change, trend or correlation between readings detected.",
            ),
            (
                10.0,
                "Rate: 360 10 s readings, median +3.1 s/d, ±0.9 s/d from one reading to the \
                 next; no level change, trend or correlation between readings detected. The \
                 rate's period search also finds 60 s (1.20 s/d peak to peak, the fourth \
                 wheel's period).",
            ),
            (
                120.0,
                "Rate: 360 2 min readings, median +3.1 s/d, ±0.9 s/d from one reading to the \
                 next; no cycle, level change, trend or correlation between readings detected.",
            ),
            (
                10.0,
                "12 10 s rate readings over 120 s: too few to search for structure (needs 30 \
                 readings over 120 s).",
            ),
        ] {
            let text = one(step, h);
            assert!(!text.contains("readings of"), "{text}");
        }
    }

    #[test]
    fn a_long_run_and_a_series_end_with_what_limits_their_readings() {
        let caveats = json!([{ "code": "beat_error_as_size" }]);
        let said = "What limits these readings:\n- The beat error's sign differs either side of \
                    a cut in the stretch, so it is given as a size.";
        let long = json!({ "schema": "timegrapher.long/1", "caveats": caveats });
        assert_eq!(lead(&long, &Map::new()).unwrap(), said);
        assert!(lead(
            &json!({ "schema": "timegrapher.long/1", "caveats": [] }),
            &Map::new()
        )
        .is_none());
        let series = json!({ "schema": "timegrapher.series/2", "caveats": caveats,
            "series": [{ "headline": "Rate: too few readings for the tests" }] });
        let text = lead(&series, &Map::new()).unwrap();
        assert!(
            text.starts_with("Series:") && text.ends_with(said),
            "{text}"
        );
    }

    #[test]
    fn fft_support_qualifies_text_without_changing_legacy_series_leads() {
        let mut doc = json!({"schema":"timegrapher.series/2", "series":[{"headline":"Rate: too few readings for the tests"}]});
        let legacy = lead(&doc, &Map::new()).unwrap();
        assert!(!legacy.contains("FFT"));
        for (support, expected) in [
            (
                json!({"finite_bins":256,"total_bins":256,"description":"FFT input: 256 of 256 grid samples available."}),
                "rate: FFT input: 256 of 256 grid samples available.",
            ),
            (
                json!({"finite_bins":128,"total_bins":256,"description":"Core's words."}),
                "rate: Core's words.",
            ),
            (Value::Null, "FFT rate input support was not recorded."),
            (
                json!({"finite_bins":true,"total_bins":256}),
                "FFT rate input support was not recorded.",
            ),
        ] {
            doc["spectrum_support"] = json!([{"series":"rate","support":support}]);
            let text = lead(&doc, &Map::new()).unwrap();
            assert!(text.contains(expected), "{text}");
            assert!(text.starts_with(&legacy));
        }
    }

    #[test]
    fn series_lead_reports_applied_clock_without_changing_legacy_documents() {
        let mut doc = json!({
            "schema": "timegrapher.series/2",
            "series": [{ "headline": "Rate: too few readings for the tests" }],
        });
        let legacy = lead(&doc, &Map::new()).unwrap();
        assert_eq!(legacy, "Series:\n- Rate: too few readings for the tests.");
        doc["clock"] = Value::Null;
        assert_eq!(lead(&doc, &Map::new()).unwrap(), "Series:\nReadings are on the sound card's clock; no correction applied.\n- Rate: too few readings for the tests.");
        for (ppm, from, drift) in [
            (12.5, "stored for 'input', 2026-10-15", false),
            (
                -12.5,
                "from 3 USB lines in the recording, 0.2 ppm apart",
                false,
            ),
            (0.0, "stored for 'zero input', 2026-10-15", false),
            (12.5, "from clock log /take/clock.csv", true),
        ] {
            doc["clock"] = json!({ "ppm": ppm, "rate_error_s_per_day": ppm * 0.0864, "from": from, "tracks_drift": drift });
            let suffix = if drift { "; mapping follows drift" } else { "" };
            assert_eq!(lead(&doc, &Map::new()).unwrap(), format!("Series:\nReadings use clock correction {ppm:+.3} ppm ({from}{suffix}).\n- Rate: too few readings for the tests."));
        }
    }

    #[test]
    fn no_watch_heard_has_no_readings() {
        let doc = json!({ "schema": "timegrapher.analyze/2", "watch_heard": false,
            "caveats": [{ "code": "no_watch" }] });
        assert!(lead(&doc, &Map::new())
            .unwrap()
            .starts_with("No regular beat found"));
        assert!(lead(&json!({ "schema": "timegrapher.doctor/2" }), &Map::new()).is_none());
    }

    #[test]
    fn the_watches_leads_count_takes_and_say_what_is_missing() {
        let list = json!({"schema": "timegrapher.watches/1",
            "watches": [{"takes": 3}, {"takes": 1}], "unsorted": 1, "unreadable": ["x"]});
        assert_eq!(
            lead(&list, &Map::new()).unwrap(),
            "2 watches, 4 takes. 1 take waits in Unsorted, made with Quick Listen. \
             1 folder in the library couldn't be read (`unreadable`)."
        );
        let empty = json!({"schema": "timegrapher.watches/1", "watches": [], "unsorted": 0});
        assert_eq!(lead(&empty, &Map::new()).unwrap(), "0 watches, 0 takes.");
        let one = json!({"schema": "timegrapher.watch/1", "name": "Pepsi", "unsorted": false,
            "archived": true, "takes": [
                {"audio": true, "started_utc": "2026-10-20T21:32:00Z"},
                {"audio": false, "started_utc": "2026-10-19T08:00:00Z"}]});
        assert_eq!(
            lead(&one, &Map::new()).unwrap(),
            "Pepsi: 2 takes, 1 kept as their analysis only; the newest started \
             2026-10-20T21:32:00Z.",
            "an older app's archived mark is not stated"
        );
        let unsorted = json!({"schema": "timegrapher.watch/1", "name": "Unsorted",
            "unsorted": true, "takes": []});
        assert_eq!(
            lead(&unsorted, &Map::new()).unwrap(),
            "Unsorted (Quick Listen takes): 0 takes."
        );
    }
}
