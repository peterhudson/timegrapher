//! What limits the readings, in words. `analyze` and each `session`
//! reading carry `caveats[]` (docs/agent-interface.md); this turns them into
//! plain sentences for the model's notes and for the views, so both say the
//! same thing. A position's caveats are grouped across the positions that
//! share them, so a caveat every take has is said once.

use serde_json::{json, Value};

/// The `_meta` key the views read the sentences from.
pub const META_KEY: &str = "timegrapher/limits";

/// One caveat as a sentence, or `None` for a code this server does not
/// know yet (the model still reads it in the JSON).
pub fn sentence(c: &Value, lift_deg: Option<f64>) -> Option<String> {
    let pct = |k: &str| c[k].as_f64().map(|f| f * 100.0);
    Some(match c["code"].as_str()? {
        "no_watch" => "No watch was heard, so there are no readings.".into(),
        "set_rate_disagrees" => match c["heard"].as_u64() {
            Some(h) => format!(
                "The beat rate set for this watch is not the one heard ({h} beats per \
                 hour), so every reading is poor. Check the calibre or beat rate."
            ),
            None => "The beat rate set for this watch is not the one heard, so every \
                     reading is poor. Check the calibre or beat rate."
                .into(),
        },
        "short_span" => format!(
            "Only {} s of beats were heard, fewer than the 10 s a sure reading \
             needs. Record longer.",
            c["span_s"]
                .as_f64()
                .map_or("a few".into(), |s| format!("{s:.0}"))
        ),
        "clipped" => format!(
            "{} of the beats were clipped, so the amplitude and beat error are less \
             sure. Turn the microphone down.",
            pct("fraction").map_or("Some".into(), |p| format!("{p:.0}%"))
        ),
        "beats_missing" => format!(
            "Only {} of the beats expected were found, so every reading is less sure. \
             Check that the watch sits firmly on the microphone and the room is quiet.",
            pct("fraction").map_or("some".into(), |p| format!("{p:.0}%"))
        ),
        "lift_unknown" => match lift_deg {
            Some(l) => format!(
                "No calibre or lift angle was given, so the amplitude assumes a lift \
                 angle of {l}°. Another lift angle changes the amplitude, so ask for the \
                 calibre."
            ),
            None => "The lift angle is a default, not the calibre's, so the amplitude \
                     is less sure. Ask for the calibre."
                .into(),
        },
        "clock_unknown" => "The sound card's clock was not checked against a \
                            reference, so the rate is a little less sure."
            .into(),
        "breaks" => match c["count"].as_u64() {
            Some(1) => "The recording was paused once; the readings leave the pause out.".into(),
            Some(n) => {
                format!("The recording was paused {n} times; the readings leave the pauses out.")
            }
            None => "The recording was paused; the readings leave the pauses out.".into(),
        },
        _ => return None,
    })
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
    if schema.starts_with("timegrapher.analyze/") {
        return doc["caveats"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| sentence(c, lift))
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
            let Some(text) = sentence(c, lift) else {
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

/// The same, as notes for the model.
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
            "schema": "timegrapher.analyze/1",
            "input": { "settings": { "lift_deg": 52.0 } },
            "caveats": [
                { "code": "short_span", "span_s": 5.7 },
                { "code": "lift_unknown" },
                { "code": "something_new" },
            ],
        });
        let said = notes(&doc);
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(said[0].starts_with("Only 6 s of beats"), "{said:?}");
        assert!(said[1].contains("lift angle of 52°"), "{said:?}");
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
            said[0].starts_with("Dial Down: No watch was heard") && said[0].contains("Only 3 s"),
            "{said:?}"
        );
    }

    #[test]
    fn other_documents_have_none() {
        assert!(limits(&json!({ "schema": "timegrapher.series/1" })).is_empty());
    }
}
