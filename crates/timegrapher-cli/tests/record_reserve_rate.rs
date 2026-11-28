//! `timegrapher record --power-reserve` on Auto reads its stop at the rate
//! the watch is heard at: a first lock on noise gives way to the watch, so
//! the test isn't ended as stopped while the watch runs (#2496; Norm 37's
//! file A on #2527).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_timegrapher"))
}

fn run(args: &[&str]) -> std::process::Output {
    let out = bin().args(args).output().unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn synth(dir: &Path, name: &str, seconds: &str, bph: &str) -> PathBuf {
    let f = dir.join(name);
    let path = f.to_str().unwrap();
    run(&["synth", path, "--duration", seconds, "--bph", bph]);
    f
}

/// 32 s of 72,000 clicks (Auto's first lock), then 620 s of a 28,800
/// watch: the test runs the whole file and ends by hand. Held at the
/// first lock, it ended as stopped 600 s after the clicks.
#[test]
fn a_first_lock_on_noise_does_not_end_the_test_as_stopped() {
    let d = std::env::temp_dir().join(format!("tg-reserve-rate-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    let clicks = synth(&d, "clicks.wav", "32", "72000");
    let watch = synth(&d, "watch.wav", "620", "28800");
    let joined = d.join("joined.flac");
    let (c, w, j) = (
        clicks.to_str().unwrap(),
        watch.to_str().unwrap(),
        joined.to_str().unwrap(),
    );
    run(&["excerpt", c, w, "--out", j]);
    let take = d.join("take");
    let take = take.to_str().unwrap();
    let out = run(&[
        "record",
        take,
        "--position",
        "DU",
        "--file",
        j,
        "--power-reserve",
        "--json",
    ]);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["power_reserve"]["ending"], "by_hand", "{doc}");
    assert_eq!(doc["check"]["to_s"], 652.0, "{doc}");
    let _ = fs::remove_dir_all(&d);
}
