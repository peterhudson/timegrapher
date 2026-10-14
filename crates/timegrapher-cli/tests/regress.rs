//! Regression integrity through the executable, without audio or capture.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, takes: Value) -> Self {
        let root = std::env::temp_dir().join(format!("tg-regress-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("baseline.json"),
            serde_json::to_vec(&json!({
                "schema": "timegrapher.regress-baseline/1", "software": {}, "takes": takes,
            }))
            .unwrap(),
        )
        .unwrap();
        Self(root)
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_timegrapher"));
        cmd.arg("regress")
            .arg(&self.0)
            .arg("--baseline")
            .arg(self.0.join("baseline.json"))
            .args(["--json", "--quiet", "--jobs", "1"]);
        cmd
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unreadable_new_take_exits_three_and_reports_the_missing_evidence() {
    let f = Fixture::new("new", json!({}));
    std::fs::create_dir_all(f.0.join("new")).unwrap();
    std::fs::write(
        f.0.join("new/session.toml"),
        "[[recording]]\nfile = \"missing.flac\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let out = f.command().output().unwrap();
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["passed"], false);
    assert_eq!(doc["unreadable"], json!(["new #1"]));
    assert_eq!(doc["counts"]["new"], 5); // New numerical rows cannot mask failure.

    let before = std::fs::read(f.0.join("baseline.json")).unwrap();
    let out = f.command().arg("--write-baseline").output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no baseline was written"));
    assert_eq!(std::fs::read(f.0.join("baseline.json")).unwrap(), before);
}

#[test]
fn missing_required_session_fails_but_an_explicit_exclusion_does_not() {
    let f = Fixture::new("missing", json!({"removed/session.toml": []}));
    std::fs::create_dir_all(f.0.join("takes")).unwrap();
    std::fs::write(f.0.join("corpus.json"), "{\"takes\": []}").unwrap();
    // A smoke run can have no referenced sessions, but required baseline
    // sessions still have to be present.
    let out = f.command().arg("--smoke").output().unwrap();
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["passed"], false);
    assert_eq!(doc["missing_sessions"], json!(["removed"]));
    let before = std::fs::read(f.0.join("baseline.json")).unwrap();
    let out = f
        .command()
        .args(["--smoke", "--write-baseline"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(std::fs::read(f.0.join("baseline.json")).unwrap(), before);

    let out = f
        .command()
        .args(["--smoke", "--take", "other"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["passed"], true);
    assert_eq!(doc["missing_sessions"], json!([]));
    assert_eq!(doc["not_run"], json!(["removed"]));
}

#[test]
fn an_unreadable_existing_baseline_is_not_replaced() {
    let f = Fixture::new("invalid-utf8", json!({}));
    std::fs::create_dir_all(f.0.join("takes")).unwrap();
    std::fs::write(f.0.join("corpus.json"), "{\"takes\": []}").unwrap();
    let baseline = f.0.join("baseline.json");
    let original = b"existing baseline with invalid UTF-8: \xff";
    std::fs::write(&baseline, original).unwrap();
    let out = f
        .command()
        .args(["--smoke", "--write-baseline"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("baseline.json"));
    assert_eq!(std::fs::read(&baseline).unwrap(), original);

    // Holdout still ignores a baseline read error and never rewrites it.
    let out = f.command().arg("--holdout").output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(std::fs::read(&baseline).unwrap(), original);

    // The same run can create a genuinely absent baseline.
    std::fs::remove_file(&baseline).unwrap();
    let out = f
        .command()
        .args(["--smoke", "--write-baseline"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let created: Value = serde_json::from_slice(&std::fs::read(&baseline).unwrap()).unwrap();
    assert_eq!(created["schema"], "timegrapher.regress-baseline/1");
}
