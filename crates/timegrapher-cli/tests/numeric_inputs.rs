//! Explicit invalid inputs must fail during parsing, before audio or persistent writes.
use std::{path::PathBuf, process::Command};
struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("tg-numeric-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_timegrapher"));
        c.env("TIMEGRAPHER_CONFIG_DIR", &self.0);
        c
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn numeric_inputs_reject_before_audio_and_output_creation() {
    let f = Fixture::new("audio");
    let missing = f.0.join("must-not-read.wav");
    let output = f.0.join("must-not-write.csv");
    for flag in [
        "--bph=0",
        "--lift=0",
        "--lift=NaN",
        "--lift=95",
        "--escape-teeth=0",
        "--card-ppm=-1000000",
        "--trend-average=5",
        "--trend-average=NaN",
    ] {
        let out = f
            .command()
            .arg("analyze")
            .arg(&missing)
            .arg(flag)
            .arg("--beats")
            .arg(&output)
            .output()
            .unwrap();
        assert!(!output.exists());
        assert_eq!(out.status.code(), Some(2), "{flag}: {out:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("invalid value"), "{err}");
        assert!(
            !err.contains("No such file"),
            "must reject argument before reading audio: {err}"
        );
    }
    // Valid explicit zero correction/lift52 reaches the ordinary missing-file error.
    let out = f
        .command()
        .arg("analyze")
        .arg(&missing)
        .args(["--bph=28800", "--lift=52", "--card-ppm=0"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("must-not-read.wav"));
}

#[test]
fn numeric_inputs_invalid_clock_set_preserves_existing_store_bytes() {
    let f = Fixture::new("clock");
    let path = f.0.join("clocks.json");
    let original = b"{\n \"schema\":\"timegrapher.clocks/1\",\"devices\":[]\n}\n";
    for value in ["NaN", "inf", "-inf", "-1000000", "-1000001"] {
        std::fs::write(&path, original).unwrap();
        let out = f
            .command()
            .args(["clock", "set", "never-store-this", value])
            .output()
            .unwrap();
        let after = std::fs::read(&path).unwrap();
        assert_eq!(
            after, original,
            "invalid {value} must preserve the actual existing store"
        );
        assert_eq!(out.status.code(), Some(2), "{value}: {out:?}");
    }
}

#[test]
fn numeric_inputs_report_settings_reject_before_bundle_analysis_work() {
    let f = Fixture::new("report");
    let missing = f.0.join("missing-report-recording.wav");
    let dest = f.0.join("report-output");
    for flag in ["--bph=0", "--lift=0", "--lift=NaN", "--lift=95"] {
        let out = f
            .command()
            .arg("report")
            .arg(&missing)
            .arg(flag)
            .args(["--no-audio", "--out"])
            .arg(&dest)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{flag}: {out:?}");
        assert!(!dest.exists(), "reject before creating report output");
        assert!(String::from_utf8_lossy(&out.stderr).contains("invalid value"));
    }
}
