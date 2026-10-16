//! Non-acoustic error-order contracts for record/session catalogue resolution.
use std::path::PathBuf;
use std::process::Command;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tg-session-catalogue-errors-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("session")).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(root.join("config/calibres.json"),serde_json::json!({
            "schema":"timegrapher.user-calibres/1","wheels":[],
            "custom":[{"name":"Bench 44","aliases":["Workshop-A"],"bph":21600,"lift_angle_deg":44.0,"wheels":[]}],
        }).to_string()).unwrap();
        std::fs::write(
            root.join("session/session.toml"),
            "calibre=\"Workshop-A\"\n[[recording]]\nfile=\"missing.wav\"\nposition=\"DU\"\n",
        )
        .unwrap();
        Self(root)
    }
    fn error(&self, command: &str, calibre: Option<&str>) -> String {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_timegrapher"));
        cmd.env("TIMEGRAPHER_CONFIG_DIR", self.0.join("config"))
            .arg(command)
            .arg(self.0.join("session"));
        if command == "record" {
            cmd.arg("--file").arg(self.0.join("missing.wav")).args([
                "--seconds",
                "1",
                "--position",
                "DU",
            ]);
        }
        if let Some(name) = calibre {
            cmd.args(["--calibre", name]);
        }
        let out = cmd.output().unwrap();
        assert!(!out.status.success());
        String::from_utf8(out.stderr).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn record_and_session_use_custom_names_before_audio_and_fail_requested_catalogue_errors() {
    let f = Fixture::new();
    for command in ["record", "session"] {
        for name in [None, Some("Workshop-A"), Some("Bench 44")] {
            let error = f.error(command, name);
            assert!(error.contains("missing.wav"), "{command} {name:?}: {error}");
            assert!(!error.contains("not in the"), "{error}");
        }
        let error = f.error(command, Some("Unknown explicit"));
        assert!(
            error.contains("--calibre") && !error.contains("missing.wav"),
            "{error}"
        );
    }
    std::fs::write(f.0.join("config/calibres.json"), "malformed").unwrap();
    for command in ["record", "session"] {
        let error = f.error(command, None);
        assert!(
            error.contains("calibres.json") && !error.contains("missing.wav"),
            "{command}: {error}"
        );
    }
}
