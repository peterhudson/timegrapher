//! Each synthetic edge take read by `timegrapher session --json` reads what
//! core reads for it ([`edge_takes::read`]), to the last bit, with the same
//! caveats; and its sound fed to `record` gets the check `session` reads of
//! the take saved: the command line adds no logic of its own.

use serde_json::Value;
use std::process::Command;
use timegrapher_core::edge_takes;

#[test]
fn session_reads_each_edge_take_as_core_does() {
    let root = std::env::temp_dir().join(format!("tg-cli-edge-takes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let takes = edge_takes::write_all(&root).unwrap();
    let mut wrong = Vec::new();
    for t in &takes {
        let core = edge_takes::read(&root, t).unwrap_or_else(|e| panic!("{e}"));
        let c = &core.measurement;
        let out = root.join("out").join(t.id);
        let run = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
            .arg("session")
            .arg(root.join("takes").join(t.id))
            .args(["--json", "--out"])
            .arg(&out)
            .env("TIMEGRAPHER_CONFIG_DIR", root.join("config"))
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}: {}",
            t.id,
            String::from_utf8_lossy(&run.stderr)
        );
        let doc: Value = serde_json::from_slice(&run.stdout).unwrap();
        let readings = doc["readings"].as_array().cloned().unwrap_or_default();
        let [reading] = readings.as_slice() else {
            wrong.push(format!("{}: {} readings, not one", t.id, readings.len()));
            continue;
        };
        let m = &reading["measurement"];
        let mut same = |what: &str, core: Option<f64>, key: &str| {
            let cli = m[key].as_f64();
            if core != cli {
                wrong.push(format!(
                    "{}: {what} {core:?} in core, {cli:?} from session",
                    t.id
                ));
            }
        };
        same("rate", c.rate_s_per_day, "rate_s_per_day");
        same("amplitude", c.amplitude_deg, "amplitude_deg");
        same("beat error", c.beat_error_unlock_ms, "beat_error_unlock_ms");
        same("start", Some(c.start_s), "start_s");
        same("end", Some(c.end_s), "end_s");
        same("beats", Some(c.beats as f64), "beats");
        let codes: Vec<String> = m["caveats"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["code"].as_str().map(str::to_string))
            .collect();
        let core_codes = edge_takes::caveat_codes(c);
        if codes != core_codes {
            wrong.push(format!(
                "{}: caveats {core_codes:?} in core, {codes:?} from session",
                t.id
            ));
        }
        eprintln!("{}: {codes:?}", t.id);
    }
    std::fs::remove_dir_all(&root).ok();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Edge takes whose sound, recorded by `record`, gets a check that differs
/// from what `session` then reads of the saved take, and why. Each must
/// still differ, so the list only shrinks as the check is fixed.
const RECORD_KNOWN: &[(&str, &str)] = &[
    (
        "edge_listed_pause",
        "record's check reads the log with no breaks; session breaks it where \
         the beats' timing jumps",
    ),
    (
        "edge_unreadable_session_json",
        "record's check reads the log with no breaks; session breaks it where \
         the beats' timing jumps",
    ),
];

/// Each edge take's sound fed to `record` as its input (live-fed): the
/// check `record --json` gives at the end is what `session` reads of the
/// take it saved, to the last bit.
#[test]
fn record_checks_each_edge_take_as_session_reads_it_saved() {
    let root = std::env::temp_dir().join(format!("tg-cli-edge-record-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let takes = edge_takes::write_all(&root).unwrap();
    let tg = |args: &[&std::ffi::OsStr]| -> Value {
        let run = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
            .args(args)
            .env("TIMEGRAPHER_CONFIG_DIR", root.join("config"))
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        serde_json::from_slice(&run.stdout).unwrap()
    };
    let mut wrong = Vec::new();
    for t in &takes {
        let into = root.join("recorded").join(t.id);
        let file = root.join("takes").join(t.id).join("audio-001.wav");
        let rec = tg(&[
            "record".as_ref(),
            into.as_os_str(),
            "--position=DU".as_ref(),
            "--lift=52".as_ref(),
            format!("--file={}", file.display()).as_ref(),
            "--json".as_ref(),
        ]);
        let saved = std::path::PathBuf::from(rec["take"].as_str().expect("the take saved"));
        let out = root.join("out").join(t.id);
        let doc = tg(&[
            "session".as_ref(),
            saved.as_os_str(),
            "--json".as_ref(),
            "--out".as_ref(),
            out.as_os_str(),
        ]);
        let c = &rec["check"];
        let m = &doc["readings"][0]["measurement"];
        let mut here = Vec::new();
        for (what, check, session) in [
            ("from", "from_s", "start_s"),
            ("to", "to_s", "end_s"),
            ("beats", "beats_found", "beats"),
            ("watch heard", "watch_heard", "watch_heard"),
            ("rate", "rate_s_per_day", "rate_s_per_day"),
            ("amplitude", "amplitude_deg", "amplitude_deg"),
            ("beat error", "beat_error_ms", "beat_error_unlock_ms"),
        ] {
            if c[check] != m[session] {
                here.push(format!(
                    "{}: {what} {} from record, {} from session",
                    t.id, c[check], m[session]
                ));
            }
        }
        eprintln!(
            "{}: {}",
            t.id,
            if here.is_empty() { "same" } else { "DIFFERS" }
        );
        match RECORD_KNOWN.iter().find(|(k, _)| *k == t.id) {
            None => wrong.extend(here),
            Some((_, why)) if !here.is_empty() => eprintln!("  as known: {why}"),
            Some(_) => wrong.push(format!("{}: now the same; take it off RECORD_KNOWN", t.id)),
        }
    }
    std::fs::remove_dir_all(&root).ok();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
