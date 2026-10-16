//! Stored-clock boundary checks without decoding sound or opening devices.
use serde_json::json;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "tg-clock-inputs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, field: &str, value: serde_json::Value) -> Vec<u8> {
        let mut doc = json!({"schema":"timegrapher.clocks/1","devices":[{"device":"USB","ppm":0,"ppm_sd":null,"span_s":0,"measured_utc":"legacy","source":"set"}]});
        if field == "schema" {
            doc[field] = value;
        } else {
            doc["devices"][0][field] = value;
        }
        let bytes = serde_json::to_vec(&doc).unwrap();
        std::fs::write(self.0.join("clocks.json"), &bytes).unwrap();
        bytes
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_timegrapher"))
            .env("TIMEGRAPHER_CONFIG_DIR", &self.0)
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn invalid_store_blocks_readers_and_writers_before_audio_or_device_work() {
    let f = Fixture::new();
    for (field, value) in [
        ("ppm", json!(-1000000)),
        ("ppm_sd", json!(-1)),
        ("span_s", json!(-1)),
        ("schema", json!("future")),
    ] {
        let original = f.write(field, value);
        for args in [
            vec!["clock", "list"],
            vec!["clock", "set", "USB", "0"],
            vec!["clock", "forget", "USB"],
            vec!["analyze", "__missing-clock-input.wav"],
            vec![
                "clock",
                "from-log",
                "__missing-clock-log.csv",
                "__missing-clock-input.wav",
                "--device",
                "USB",
                "--save",
            ],
            vec![
                "clock",
                "measure",
                "--device",
                "__nonexistent_clock_input__",
                "--save",
            ],
        ] {
            let out = f.run(&args);
            let error = String::from_utf8_lossy(&out.stderr);
            assert!(!out.status.success(), "{args:?}");
            assert!(
                error.contains("clocks.json") && error.contains(field),
                "{args:?}: {error}"
            );
            assert!(
                !error.contains("__missing") && !error.contains("__nonexistent"),
                "{error}"
            );
            assert_eq!(std::fs::read(f.0.join("clocks.json")).unwrap(), original);
            assert_eq!(std::fs::read_dir(&f.0).unwrap().count(), 1);
        }
    }
}

#[test]
fn explicit_zero_and_unsaved_log_fit_bypass_store_but_valid_store_preserves_defaults() {
    let f = Fixture::new();
    f.write("ppm", json!(-1000000));
    for args in [
        vec!["analyze", "__missing-clock-input.wav", "--card-ppm", "0"],
        vec![
            "clock",
            "from-log",
            "__missing-clock-log.csv",
            "__missing-clock-input.wav",
            "--device",
            "USB",
        ],
    ] {
        let out = f.run(&args);
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success());
        assert!(
            error.contains("__missing-clock-input.wav") && !error.contains("clocks.json"),
            "{error}"
        );
    }
    f.write("ppm", json!(0));
    let out = f.run(&["clock", "list", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = f.run(&["analyze", "__missing-clock-input.wav", "--device", "USB"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("__missing-clock-input.wav"));
    let out = f.run(&["clock", "set", "USB", "0"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.0.join("clocks.json")).unwrap()).unwrap();
    assert_eq!(doc["devices"][0]["ppm"], 0.0);
    assert_eq!(doc["devices"][0]["span_s"], 0.0);
    assert!(doc["devices"][0]["ppm_sd"].is_null());
}
