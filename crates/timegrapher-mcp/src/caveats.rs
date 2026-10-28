//! What limits the readings, in words. `analyze` and each `session`
//! reading carry `caveats[]` (docs/agent-interface.md); this turns them into
//! plain facts for the result's lead (summary.rs) and for the views, so both say the
//! same thing, with no grades or advice. A position's caveats are grouped across the positions that
//! share them, so a caveat every take has is said once.

use serde_json::{json, Value};

/// The `_meta` key the views read the sentences from.
pub const META_KEY: &str = "timegrapher/limits";

/// One caveat as a plain fact, with no judgement of the readings or advice
/// (that is the skill's), or `None` for a code this server does not know
/// yet (the model still reads it in the JSON).
pub fn fact(c: &Value, lift_deg: Option<f64>) -> Option<String> {
    if c["code"] == "no_watch" {
        return Some(no_regular_beat(&json!({ "caveats": [c] }), None));
    }
    // Core's words, the app's
    // ([`timegrapher_core::measure::Caveat::long`]).
    serde_json::from_value::<timegrapher_core::measure::Caveat>(c.clone())
        .ok()?
        .long(lift_deg)
}

/// The canonical sentence for a measurement with no regular beat: the
/// timing scatter of the beats, in ms and as a share of the beat period,
/// when the measurement gives it (`jitter_us`, with the beat rate).
pub fn no_regular_beat(m: &Value, bph: Option<f64>) -> String {
    let no_sound = m["caveats"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|c| c["code"] == "no_watch" && c["reason"] == "no_beat_sound");
    let said = match bph.filter(|b| *b > 0.0) {
        _ if no_sound => timegrapher_core::measure::no_watch_text(
            timegrapher_core::measure::NoWatchReason::NoBeatSound,
            None,
            None,
        ),
        Some(b) => {
            timegrapher_core::session::no_regular_beat(m["jitter_us"].as_f64(), b.round() as u32)
        }
        None => timegrapher_core::session::no_regular_beat(None, 0),
    };
    format!("{said}. There are no rate, amplitude or beat error readings.")
}

/// A session reading's name: its position, and its state of wind when the
/// session has more than one.
fn reading_name(r: &Value) -> String {
    let p = r["position"].as_str().unwrap_or("");
    let name = match p {
        "CH" => "Dial Up",
        "CB" => "Dial Down",
        "3H" => "Crown Up",
        "6H" => "Crown Left",
        "9H" => "Crown Down",
        "12H" => "Crown Right",
        other => other,
    };
    match r["wind_h"].as_f64() {
        Some(h) if h > 0.0 => format!("{name} after {h} h"),
        _ => name.to_string(),
    }
}

/// What limits the readings in an `analyze` or `session` document, as
/// `[{where, text}]`: `where` names the session positions a sentence is
/// about, or is null for a single recording (or every position). Empty for
/// any other document.
pub fn limits(doc: &Value) -> Vec<Value> {
    let schema = doc["schema"].as_str().unwrap_or("");
    let lift = doc["input"]["settings"]["lift_deg"]
        .as_f64()
        .or(doc["lift_deg"].as_f64());
    let bph = doc["bph"].as_f64();
    // No regular beat is said with the measurement's own timing scatter.
    let say = |c: &Value, m: &Value| match c["code"].as_str() {
        Some("no_watch") if c["reason"] == "no_beat_sound" => fact(c, lift),
        Some("no_watch") => Some(no_regular_beat(m, m["bph"].as_f64().or(bph))),
        _ => fact(c, lift),
    };
    if schema.starts_with("timegrapher.analyze/") {
        return doc["caveats"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| say(c, doc))
            .map(|text| json!({ "where": null, "text": text }))
            .collect();
    }
    if !schema.starts_with("timegrapher.session/") {
        return Vec::new();
    }
    let readings = doc["readings"].as_array().cloned().unwrap_or_default();
    // Each sentence once, with the readings it applies to, in the order
    // first seen.
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for r in &readings {
        for c in r["measurement"]["caveats"].as_array().into_iter().flatten() {
            let Some(text) = say(c, &r["measurement"]) else {
                continue;
            };
            let name = reading_name(r);
            match out.iter_mut().find(|(t, _)| *t == text) {
                Some((_, names)) => {
                    if !names.contains(&name) {
                        names.push(name)
                    }
                }
                None => out.push((text, vec![name])),
            }
        }
    }
    // A sentence every reading shares is said without names, and the
    // sentences about the same positions are said together.
    let mut said: Vec<(Option<String>, String)> = Vec::new();
    for (text, names) in out {
        let every = names.len() == readings.len() && readings.len() > 1;
        let place = (!every).then(|| join(&names));
        match said.iter_mut().find(|(p, _)| *p == place) {
            Some((_, t)) => {
                t.push(' ');
                t.push_str(&text);
            }
            None => said.push((place, text)),
        }
    }
    said.into_iter()
        .map(|(place, text)| json!({ "where": place, "text": text }))
        .collect()
}

fn join(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The same facts as lines for the model's lead.
pub fn notes(doc: &Value) -> Vec<String> {
    limits(doc)
        .into_iter()
        .map(|l| match l["where"].as_str() {
            Some(w) => format!("{w}: {}", l["text"].as_str().unwrap_or("")),
            None => l["text"].as_str().unwrap_or("").to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recordings_caveats_become_sentences() {
        let doc = json!({
            "schema": "timegrapher.analyze/2",
            "input": { "settings": { "lift_deg": 52.0 } },
            "caveats": [
                { "code": "short_span", "span_s": 5.7 },
                { "code": "lift_unknown" },
                { "code": "something_new" },
            ],
        });
        let said = notes(&doc);
        assert_eq!(said.len(), 2, "{said:?}");
        assert_eq!(
            said[0], "The beats span 6 s, under the 10 s an analysis of a recording asks for.",
            "{said:?}"
        );
        assert!(said[1].contains("the default 52°"), "{said:?}");
    }

    #[test]
    fn a_sessions_caveats_name_their_positions_once() {
        let doc = json!({
            "schema": "timegrapher.session/1",
            "lift_deg": 52.0,
            "readings": [
                { "position": "CH", "measurement": { "caveats": [
                    { "code": "clock_unknown" } ] } },
                { "position": "CB", "measurement": { "caveats": [
                    { "code": "clipped", "fraction": 0.04 }, { "code": "clock_unknown" } ] } },
                { "position": "3H", "wind_h": 24.0, "measurement": { "caveats": [
                    { "code": "clipped", "fraction": 0.04 }, { "code": "clock_unknown" } ] } },
            ],
        });
        let l = limits(&doc);
        assert_eq!(l.len(), 2, "{l:?}");
        assert_eq!(l[0]["where"], Value::Null, "every position: said once");
        assert!(l[0]["text"].as_str().unwrap().contains("clock"));
        assert_eq!(l[1]["where"], "Dial Down and Crown Up after 24 h");
        assert!(l[1]["text"]
            .as_str()
            .unwrap()
            .starts_with("4% of the beats"));
    }

    #[test]
    fn a_positions_sentences_are_said_together() {
        let doc = json!({
            "schema": "timegrapher.session/1",
            "readings": [
                { "position": "CH", "measurement": { "caveats": [] } },
                { "position": "CB", "measurement": { "caveats": [
                    { "code": "no_watch" }, { "code": "short_span", "span_s": 3.0 } ] } },
            ],
        });
        let said = notes(&doc);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].starts_with("Dial Down: No regular beat found") && said[0].contains("span 3 s"),
            "{said:?}"
        );
    }

    #[test]
    fn the_facts_carry_no_grades_or_advice() {
        let codes = json!([
            { "code": "no_watch" }, { "code": "set_rate_disagrees", "heard": 21600 },
            { "code": "short_span", "span_s": 4.0 }, { "code": "clipped", "fraction": 0.03 },
            { "code": "beats_missing", "fraction": 0.8 }, { "code": "lift_unknown" },
            { "code": "clock_unknown" }, { "code": "breaks", "count": 2 },
            { "code": "sides_by_count", "count": 1 }, { "code": "sides_by_count", "count": 2 },
            { "code": "beat_error_as_size" },
            { "code": "masked", "seconds": 12.0 },
        ]);
        for c in codes.as_array().unwrap() {
            for lift in [Some(52.0), None] {
                let s = fact(c, lift).unwrap().to_lowercase();
                // Grades as whole words ("measured" holds "sure"), advice
                // as phrases.
                let words: Vec<&str> = s.split(|ch: char| !ch.is_alphanumeric()).collect();
                for word in ["sure", "poor", "fair", "good", "trust"] {
                    assert!(!words.contains(&word), "{word}: {s}");
                }
                for phrase in ["check the", "turn ", "ask for", "record longer"] {
                    assert!(!s.contains(phrase), "{phrase}: {s}");
                }
            }
        }
    }

    #[test]
    fn masked_stretches_are_said_with_their_seconds() {
        assert_eq!(
            fact(&json!({ "code": "masked", "seconds": 12.4 }), None).unwrap(),
            "12 s of this stretch were masked out and left out of the readings."
        );
        assert_eq!(
            fact(&json!({ "code": "masked", "seconds": 0.3 }), None).unwrap(),
            "0.3 s of this stretch were masked out and left out of the readings."
        );
        // A sliver that would read 0.0 s goes unsaid, as in the app and
        // the CLI; so does a caveat whose figure the JSON leaves out.
        for c in [
            json!({ "code": "masked", "seconds": 0.02 }),
            json!({ "code": "masked" }),
        ] {
            assert_eq!(fact(&c, None), None);
        }
    }

    #[test]
    fn no_regular_beat_gives_the_timing_scatter() {
        let doc = json!({ "schema": "timegrapher.analyze/2", "bph": 28800, "jitter_us": 8600.0,
            "watch_heard": false, "caveats": [{ "code": "no_watch" }] });
        assert_eq!(
            notes(&doc),
            [
                "No regular beat found (timing scatter 8.6 ms, 6.9% of a beat). There are no \
              rate, amplitude or beat error readings."
            ]
        );
        assert_eq!(
            no_regular_beat(&Value::Null, Some(28800.0)),
            "No regular beat found. There are no rate, amplitude or beat error readings."
        );
    }

    #[test]
    fn other_documents_have_none() {
        assert!(limits(&json!({ "schema": "timegrapher.series/2" })).is_empty());
    }
}
