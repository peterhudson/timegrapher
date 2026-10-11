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
    format!("{v:+.digits$}")
}

/// One reading: rate, amplitude and beat error (as a size), each with its
/// standard error where the document gives one.
fn readings(m: &Value, rate: Option<f64>) -> Option<String> {
    let conf = &m["confidence"];
    let mut parts = Vec::new();
    if let Some(r) = rate {
        parts.push(format!(
            "rate {}{} s/day",
            signed(r, 1),
            pm(&conf["rate"], 1)
        ));
    }
    if let Some(a) = m["amplitude_deg"].as_f64() {
        parts.push(format!("amplitude {a:.0}{}°", pm(&conf["amplitude"], 0)));
    }
    let be = m["beat_error_unlock_ms"]
        .as_f64()
        .or(m["beat_error_ms"].as_f64())
        .or(m["overall"]["beat_error_ms"].as_f64());
    if let Some(b) = be {
        parts.push(format!(
            "beat error {:.2}{} ms",
            b.abs(),
            pm(&conf["beat_error"], 2)
        ));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn position_name(p: &str) -> &str {
    match p {
        "DU" => "Dial Up (DU)",
        "DD" => "Dial Down (DD)",
        "CU" => "Crown Up (CU)",
        "CD" => "Crown Down (CD)",
        "CL" => "Crown Left (CL)",
        "CR" => "Crown Right (CR)",
        "CH" => "Dial Up (DU)",
        "CB" => "Dial Down (DD)",
        "3H" => "Crown Up (CU)",
        "6H" => "Crown Left (CL)",
        "9H" => "Crown Down (CD)",
        "12H" => "Crown Right (CR)",
        other => other,
    }
}

/// The lead for `analyze`, `session` and `record` documents; `None` for
/// others. `args` are the tool call's, to tell a given lift angle from the
/// default.
pub fn lead(doc: &Value, args: &Map<String, Value>) -> Option<String> {
    let schema = doc["schema"].as_str().unwrap_or("");
    let mut lines = Vec::new();
    if schema.starts_with("timegrapher.record/") {
        return Some(take(doc, args));
    }
    if schema.starts_with("timegrapher.analyze/") {
        if doc["watch_heard"] == false {
            lines.push("No watch was heard, so there are no readings.".to_string());
        } else {
            let rate = doc["rate_corrected_s_per_day"]
                .as_f64()
                .or(doc["overall"]["rate_s_per_day"].as_f64());
            if let Some(r) = readings(doc, rate) {
                lines.push(format!("Readings: {r}."));
            }
        }
    } else if schema.starts_with("timegrapher.session/") {
        lines.push("Readings by position:".to_string());
        for r in doc["readings"].as_array().into_iter().flatten() {
            let m = &r["measurement"];
            let mut name = position_name(r["position"].as_str().unwrap_or("")).to_string();
            if let Some(h) = r["wind_h"].as_f64().filter(|h| *h > 0.0) {
                name += &format!(" after {h} h");
            }
            let said = if m["watch_heard"] == false {
                "no watch heard".to_string()
            } else {
                readings(m, m["rate_s_per_day"].as_f64()).unwrap_or_default()
            };
            lines.push(format!("- {name}: {said}"));
        }
        for s in doc["report"]["states"].as_array().into_iter().flatten() {
            lines.push(characteristic_values(s));
        }
    } else {
        return None;
    }
    // A sentence the lead already says (no watch heard) is not repeated.
    let limits: Vec<String> = crate::caveats::notes(doc)
        .into_iter()
        .filter(|l| !lines.contains(l))
        .collect();
    if !limits.is_empty() {
        lines.push("What limits these readings:".to_string());
        lines.extend(limits.into_iter().map(|l| format!("- {l}")));
    }
    Some(lines.join("\n"))
}

/// A recorded take: what was recorded, what its quick check measured, the
/// lift angle and where it came from, the beats found and clipped, and
/// whether the take joined the test. Facts only: no grades.
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
        lines.push(format!("The check{window} heard no watch."));
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
    lines.push(
        match doc["verdict"].as_str() {
            Some("ok") => "The take was added to the test.",
            Some("no_watch_heard") => "The take was not added to the test, as no watch was heard.",
            Some("clipping") => {
                "The take was not added to the test, as more than 1% of its beats were clipped."
            }
            _ => "",
        }
        .to_string(),
    );
    lines.retain(|l| !l.is_empty());
    lines.join(" ")
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
    fn a_recording_leads_with_its_readings_and_no_grades() {
        let doc = json!({
            "schema": "timegrapher.analyze/1",
            "watch_heard": true,
            "overall": { "rate_s_per_day": 6.01, "beat_error_ms": -0.39 },
            "amplitude_deg": 269.6,
            "beat_error_unlock_ms": -0.4,
            "input": { "settings": { "lift_deg": 52.0 } },
            "confidence": { "rate": { "grade": "poor", "se": null },
                            "amplitude": { "grade": "fair", "se": 0.042 },
                            "beat_error": { "grade": "good", "se": 0.004 } },
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
            s.contains("What limits these readings:\n- No calibre or lift angle"),
            "{s}"
        );
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
    fn a_take_states_its_facts_and_its_lift_angle() {
        let doc = json!({
            "schema": "timegrapher.record/1",
            "input": { "settings": { "calibre": null, "lift_deg": 52.0 } },
            "position": "DU", "seconds": 30.0,
            "check": { "from_s": 15.0, "to_s": 30.0, "watch_heard": true,
                "beats_found": 118, "beats_expected": 120, "rate_s_per_day": 3.0,
                "amplitude_deg": 269.4, "beat_error_ms": -0.4,
                "clipping": { "beats": 0, "beat_fraction": 0.0 } },
            "verdict": "ok",
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
    fn no_watch_heard_has_no_readings() {
        let doc = json!({ "schema": "timegrapher.analyze/1", "watch_heard": false,
            "caveats": [{ "code": "no_watch" }] });
        assert!(lead(&doc, &Map::new())
            .unwrap()
            .starts_with("No watch was heard"));
        assert!(lead(&json!({ "schema": "timegrapher.doctor/1" }), &Map::new()).is_none());
    }
}
