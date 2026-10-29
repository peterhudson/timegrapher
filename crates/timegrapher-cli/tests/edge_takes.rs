//! Each synthetic edge take read by `timegrapher session --json` reads what
//! core reads for it ([`edge_takes::read`]), to the last bit, with the same
//! caveats: the command line adds no logic of its own to an awkward take.

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
        if readings.len() != core.len() {
            wrong.push(format!(
                "{}: {} readings from session, {} entries in core",
                t.id,
                readings.len(),
                core.len()
            ));
            continue;
        }
        for (n, (reading, read)) in readings.iter().zip(&core).enumerate() {
            let c = &read.measurement;
            let id = format!("{} #{}", t.id, n + 1);
            let m = &reading["measurement"];
            let mut same = |what: &str, core: Option<f64>, key: &str| {
                let cli = m[key].as_f64();
                if core != cli {
                    wrong.push(format!(
                        "{id}: {what} {core:?} in core, {cli:?} from session"
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
                    "{id}: caveats {core_codes:?} in core, {codes:?} from session"
                ));
            }
            eprintln!("{id}: {codes:?}");
        }
    }
    std::fs::remove_dir_all(&root).ok();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
