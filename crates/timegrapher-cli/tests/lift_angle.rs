//! `long` and `series` take the lift angle from `--lift`, else the
//! calibre, else 52°, and say so on stderr when they fall back.
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Output};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tg-lift-angle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("calibres.json"),
            json!({
                "schema": "timegrapher.user-calibres/1",
                "custom": [
                    {"name": "Bench 44", "bph": 28800, "lift_angle_deg": 44.0},
                    {"name": "Bench Unknown", "bph": 28800},
                ],
            })
            .to_string(),
        )
        .unwrap();
        let fixture = Self(dir);
        let out = fixture.run(&["synth", fixture.wav().to_str().unwrap(), "--duration", "40"]);
        assert!(out.status.success(), "{}", stderr(&out));
        fixture
    }
    fn wav(&self) -> PathBuf {
        self.0.join("watch.wav")
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_timegrapher"))
            .env("TIMEGRAPHER_CONFIG_DIR", &self.0)
            .args(args)
            .output()
            .unwrap()
    }
    /// The lift angle `command` used, from its JSON settings, and its stderr.
    fn lift(&self, command: &str, extra: &[&str]) -> (f64, String) {
        let wav = self.wav();
        let out_dir = self.0.join(format!("out-{command}-{}", extra.join("-")));
        let mut args = vec![command, wav.to_str().unwrap(), "--json"];
        let out_dir = out_dir.to_str().unwrap().to_string();
        if command == "long" {
            args.extend(["--out", &out_dir]);
        }
        args.extend(extra);
        let out = self.run(&args);
        assert!(
            out.status.success(),
            "{command} {extra:?}: {}",
            stderr(&out)
        );
        let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
        let lift = doc["input"]["settings"]["lift_deg"].as_f64().unwrap();
        (lift, stderr(&out))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn has_note(err: &str) -> bool {
    err.lines()
        .any(|l| l.starts_with("note: lift 52°, the default"))
}

#[test]
fn a_missing_recording_gets_no_lift_note() {
    let f = Fixture::new();
    let missing = f.0.join("missing.wav");
    for command in ["long", "series"] {
        let out = f.run(&[command, missing.to_str().unwrap()]);
        assert!(!out.status.success(), "{command}");
        assert!(!has_note(&stderr(&out)), "{command}: {}", stderr(&out));
    }
}

#[test]
fn long_and_series_take_the_lift_from_the_flag_then_the_calibre_then_52() {
    let f = Fixture::new();
    for command in ["long", "series"] {
        let (lift, err) = f.lift(command, &["--calibre", "Bench 44"]);
        assert_eq!(lift, 44.0, "{command}");
        assert!(!has_note(&err), "{command}: {err}");

        let (lift, err) = f.lift(command, &["--calibre", "Bench 44", "--lift", "50"]);
        assert_eq!(lift, 50.0, "{command}");
        assert!(!has_note(&err), "{command}: {err}");

        let (lift, err) = f.lift(command, &["--calibre", "Bench Unknown"]);
        assert_eq!(lift, 52.0, "{command}");
        assert!(
            err.contains(
                "note: lift 52°, the default; the calibre's lift angle is not in the table"
            ),
            "{command}: {err}"
        );

        let (lift, err) = f.lift(command, &["--calibre", "Bench Unknown", "--lift", "48"]);
        assert_eq!(lift, 48.0, "{command}");
        assert!(!has_note(&err), "{command}: {err}");

        let (lift, err) = f.lift(command, &[]);
        assert_eq!(lift, 52.0, "{command}");
        assert!(
            err.contains("note: lift 52°, the default; calibre not set"),
            "{command}: {err}"
        );
    }
}
