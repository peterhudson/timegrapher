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

/// The `_meta` key the views read the no-regular-beat sentence from.
pub const NO_BEAT_KEY: &str = "timegrapher/no_beat";

/// The sentence ([`no_regular_beat`]) for a result whose recording has no
/// regular beat, so the views say core's words rather than their own:
/// `analyze`'s, or `record`'s quick check. `None` when the beat was found.
pub fn no_beat(doc: &Value) -> Option<String> {
    let schema = doc["schema"].as_str().unwrap_or("");
    if schema.starts_with("timegrapher.analyze/") && doc["watch_heard"] == false {
        return Some(no_regular_beat(doc, doc["bph"].as_f64()));
    }
    let c = &doc["check"];
    if schema.starts_with("timegrapher.record/") && c["watch_heard"] == false {
        // The check gives its timing scatter in ms.
        let m = json!({
            "jitter_us": c["timing_scatter_ms"].as_f64().map(|ms| ms * 1000.0),
            "caveats": c["caveats"],
        });
        return Some(no_regular_beat(&m, c["bph"].as_f64()));
    }
    None
}

/// A session reading's name: its position as core names it
/// ([`crate::summary::position_name`]), and its state of wind when the
/// session has more than one.
fn reading_name(r: &Value) -> String {
    let name = crate::summary::position_name(r["position"].as_str().unwrap_or(""));
    match r["wind_h"].as_f64() {
        Some(h) if h > 0.0 => format!("{name} after {h} h"),
        _ => name,
    }
}

/// What limits the readings in an `analyze` or `session` document, as
/// `[{where, text}]`: `where` names the session positions a sentence is
/// about, or is null for a single recording (or every position). Empty for
/// any other document. A recording its take's session file lists is read as
/// that take reads it, so its limits are the take readings' own.
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
        if let Some(readings) = doc["take"]["readings"].as_array() {
            return by_reading(readings, say);
        }
    }
    // `long` and `series` carry theirs at the top, for the whole take.
    if [
        "timegrapher.analyze/",
        "timegrapher.long/",
        "timegrapher.series/",
    ]
    .iter()
    .any(|k| schema.starts_with(k))
    {
        return doc["caveats"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| say(c, doc))
            .map(|text| json!({ "where": null, "text": text }))
            .collect();
    }
    // A live reading carries no caveats; its clipping is said when core
    // counts it (`Clipping::noted_fraction`), in core's words (#808).
    if schema.starts_with("timegrapher.live/") {
        return doc["reading"]["clipped_fraction"]
            .as_f64()
            .filter(|&f| timegrapher_core::analysis::Clipping::noted_fraction(f))
            .and_then(|fraction| timegrapher_core::measure::Caveat::Clipped { fraction }.long(None))
            .map(|text| json!({ "where": null, "text": text }))
            .into_iter()
            .collect();
    }
    if !schema.starts_with("timegrapher.session/") {
        return Vec::new();
    }
    let readings = doc["readings"].as_array().cloned().unwrap_or_default();
    by_reading(&readings, say)
}

/// Each reading's caveats as sentences, with the readings each is about.
fn by_reading(readings: &[Value], say: impl Fn(&Value, &Value) -> Option<String>) -> Vec<Value> {
    // Each sentence once, with the readings it applies to, in the order
    // first seen.
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for r in readings {
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
            said[0], "The beats span 6 s, under the 10 s needed to judge whether they are regular.",
            "{said:?}"
        );
        assert!(said[1].contains("the default 52°"), "{said:?}");
        // The two clocks that disagree are said with both figures, in
        // core's words (#498).
        let c = json!({ "code": "clock_sources_disagree", "usb_ppm": 120.0, "log_ppm": 20.0, "log_ppm_sd": 2.0 });
        assert_eq!(
            fact(&c, None).unwrap(),
            "The USB lines measure the sound card at +120.0 ppm and the clock log at +20 ± 2 \
             ppm; the rates are corrected by the USB lines."
        );
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
        assert_eq!(
            l[1]["where"],
            "Dial Down (DD) and Crown Up (CU, 3H) after 24 h"
        );
        assert!(l[1]["text"]
            .as_str()
            .unwrap()
            .starts_with("4% of the beats"));
    }

    /// A reading named by any code core reads ("DU" or Witschi's "CH")
    /// is named in the limits as the lead names it (#691).
    #[test]
    fn a_readings_position_is_named_as_core_names_it() {
        for code in ["DU", "CH"] {
            let r = json!({ "position": code });
            assert_eq!(reading_name(&r), "Dial Up (DU)");
            assert_eq!(reading_name(&r), crate::summary::position_name(code));
        }
        let r = json!({ "position": "PU", "wind_h": 24.0 });
        assert_eq!(reading_name(&r), "Crown Up (CU, 3H) after 24 h");
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
            said[0].starts_with("Dial Down (DD): No regular beat found")
                && said[0].contains("span 3 s"),
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
                // "measured" holds "sure" and is a plain fact: it alone is
                // allowed, and every other form of each word is still caught.
                let s = fact(c, lift)
                    .unwrap()
                    .to_lowercase()
                    .replace("measured", "");
                for word in [
                    "sure",
                    "poor",
                    "fair",
                    "good",
                    "trust",
                    "check the",
                    "turn ",
                    "ask for",
                    "record longer",
                ] {
                    assert!(!s.contains(word), "{word}: {s}");
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
            "No regular beat found (too few beats to time). There are no rate, amplitude or \
             beat error readings."
        );
    }

    #[test]
    fn the_views_get_the_no_regular_beat_sentence() {
        let said = "No regular beat found (timing scatter 8.6 ms, 6.9% of a beat). There are \
                    no rate, amplitude or beat error readings.";
        let analyze = json!({ "schema": "timegrapher.analyze/2", "bph": 28800,
            "jitter_us": 8600.0, "watch_heard": false });
        assert_eq!(no_beat(&analyze).as_deref(), Some(said));
        let record = json!({ "schema": "timegrapher.record/2",
            "check": { "bph": 28800, "timing_scatter_ms": 8.6, "watch_heard": false } });
        assert_eq!(no_beat(&record).as_deref(), Some(said));
        assert_eq!(
            no_beat(&json!({ "schema": "timegrapher.analyze/2", "watch_heard": true })),
            None
        );
        assert_eq!(
            no_beat(&json!({ "schema": "timegrapher.record/2", "check": { "watch_heard": true } })),
            None
        );
    }

    #[test]
    fn a_live_reading_says_its_clipping_when_core_counts_it() {
        let live = |f: f64| {
            limits(&json!({ "schema": "timegrapher.live/1", "reading": { "clipped_fraction": f } }))
        };
        assert!(live(0.01).is_empty());
        let said = live(0.3);
        assert_eq!(said.len(), 1);
        assert!(
            said[0]["text"]
                .as_str()
                .unwrap()
                .starts_with("30% of the beats have a clipped sample"),
            "{said:?}"
        );
    }

    #[test]
    fn other_documents_have_none() {
        assert!(limits(&json!({ "schema": "timegrapher.series/2" })).is_empty());
        assert!(limits(&json!({ "schema": "timegrapher.watch/1", "caveats": [{ "code": "breaks", "count": 2 }] })).is_empty());
    }

    #[test]
    fn a_long_run_and_a_series_say_their_caveats_for_the_whole_take() {
        let caveats = json!([
            { "code": "sides_by_count", "count": 1 },
            { "code": "beat_error_as_size" },
            { "code": "clock_sources_disagree", "usb_ppm": 120.0, "log_ppm": 20.0, "log_ppm_sd": 2.0 }]);
        // In core's words, each said once for the whole take.
        let said: Vec<Value> = caveats
            .as_array()
            .unwrap()
            .iter()
            .map(|c| json!({ "where": null, "text": fact(c, None).unwrap() }))
            .collect();
        assert_eq!(said.len(), 3);
        for schema in ["timegrapher.long/1", "timegrapher.series/2"] {
            let doc = json!({ "schema": schema, "caveats": caveats });
            assert_eq!(limits(&doc), said, "{schema}");
        }
    }
}
