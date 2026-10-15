//! Catalogue discovery/analysis resolution without decoding audio or opening devices.
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Output};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tg-custom-calibres-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("calibres.json"), json!({
            "schema":"timegrapher.user-calibres/1",
            "custom":[{"name":"Bench 44","aliases":["Workshop-A"],"bph":21600,"lift_angle_deg":44.0,"escape_teeth":20,"wheels":[{"name":"third wheel","period_s":123.0}]}],
            "wheels":[{"calibre":"Rolex 3235","name":"third wheel","period_s":241.0}],
        }).to_string()).unwrap();
        Self(dir)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_timegrapher"))
            .env("TIMEGRAPHER_CONFIG_DIR", &self.0)
            .args(args)
            .output()
            .unwrap()
    }
    fn missing(&self) -> String {
        self.0.join("missing.wav").to_string_lossy().into_owned()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn custom_calibre_discovery_and_direct_commands_accept_the_same_alias() {
    let fixture = Fixture::new();
    let discovery = fixture.run(&["calibres", "workshop a", "--json"]);
    assert!(
        discovery.status.success(),
        "{}",
        String::from_utf8_lossy(&discovery.stderr)
    );
    let doc: Value = serde_json::from_slice(&discovery.stdout).unwrap();
    assert_eq!(doc["custom"][0]["name"], "Bench 44");
    assert_eq!(doc["custom"][0]["bph"], 21600);
    assert_eq!(doc["custom"][0]["lift_angle_deg"], 44.0);
    assert_eq!(doc["custom"][0]["escape_teeth"], 20);
    assert!(doc["slots"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["name"] == "third wheel" && s["period_s"] == 123.0));
    let missing = fixture.missing();
    for command in ["analyze", "long", "series"] {
        for name in ["Bench 44", "workshop a"] {
            let out = fixture.run(&[command, &missing, "--calibre", name, "--json"]);
            let error = String::from_utf8_lossy(&out.stderr);
            assert!(!out.status.success());
            assert!(error.contains(&missing), "{command}/{name}: {error}");
            assert!(!error.contains("not in the"), "{command}/{name}: {error}");
        }
    }
}

#[test]
fn catalogue_errors_precede_audio_but_no_calibre_keeps_default_path() {
    let fixture = Fixture::new();
    let missing = fixture.missing();
    for command in ["analyze", "long", "series"] {
        let out = fixture.run(&[command, &missing, "--calibre", "__unknown_calibre__"]);
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success());
        assert!(
            error.contains("--calibre") && error.contains("calibres --search"),
            "{error}"
        );
        assert!(!error.contains(&missing), "{error}");
    }
    std::fs::write(fixture.0.join("calibres.json"), "not JSON").unwrap();
    for command in ["analyze", "long", "series"] {
        let out = fixture.run(&[command, &missing, "--calibre", "ETA 2824-2"]);
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success());
        assert!(error.contains("calibres.json"), "{command}: {error}");
        assert!(!error.contains(&missing), "{error}");
        let out = fixture.run(&[command, &missing]);
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(error.contains(&missing), "{command}: {error}");
        assert!(!error.contains("calibres.json"), "{error}");
    }
    std::fs::remove_file(fixture.0.join("calibres.json")).unwrap();
    for command in ["analyze", "long", "series"] {
        let out = fixture.run(&[command, &missing, "--calibre", "ETA 2824-2"]);
        assert!(String::from_utf8_lossy(&out.stderr).contains(&missing));
    }
}
