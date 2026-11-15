//! `series` leaves the wind not given unless `--wind-h` sets it, and
//! never takes a take with no wind given as begun at full wind (#1665).
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

fn run(dir: &PathBuf, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .env("TIMEGRAPHER_CONFIG_DIR", dir)
        .args(args)
        .output()
        .unwrap()
}

/// The `series` JSON of a 40 s take, with `extra` flags.
fn series(dir: &PathBuf, extra: &[&str]) -> Value {
    let wav = dir.join("watch.wav");
    let mut args = vec!["series", wav.to_str().unwrap(), "--json"];
    args.extend_from_slice(extra);
    let out = run(dir, &args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn series_leaves_the_wind_not_given_unless_set() {
    let dir = std::env::temp_dir().join(format!("tg-series-wind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("watch.wav");
    let out = run(&dir, &["synth", wav.to_str().unwrap(), "--duration", "40"]);
    assert!(out.status.success());
    let unset = series(&dir, &[]);
    assert_eq!(unset["schema"], "timegrapher.series/4");
    assert!(
        unset["input"]["settings"]["wind_h"].is_null(),
        "{}",
        unset["input"]
    );
    let set = series(&dir, &["--wind-h", "24"]);
    assert_eq!(set["input"]["settings"]["wind_h"], 24.0);
    let _ = std::fs::remove_dir_all(&dir);
}
