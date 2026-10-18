//! `timegrapher record --live FILE`: the latest readings kept in a file
//! while a file stands in for the microphone, played back in real time.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_timegrapher"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tg-record-live-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_live_take_keeps_its_readings_in_the_file() {
    let dir = scratch("feed");
    let wav = dir.join("watch.wav");
    let out = bin()
        .args([
            "synth",
            wav.to_str().unwrap(),
            "--duration",
            "20",
            "--rate=6",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let feed = dir.join("live.json");
    // Short, to keep the test quick, though the take is too short for its
    // check to hear a watch. The live readings start after about 4 s and
    // the file is written once a second, so 10 s leaves six mid-take
    // writes to see: 6 s left two, and a run on Windows saw neither.
    let seconds = 10.0;
    let mut child = bin()
        .args([
            "record",
            dir.join("test").to_str().unwrap(),
            "--position",
            "CD",
        ])
        .arg(format!("--seconds={seconds}"))
        .arg(format!("--file={}", wav.display()))
        .arg(format!("--live={}", feed.display()))
        .arg("--json")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();

    // Read the file while the take runs, as the MCP server does.
    let mut seen_mid_take = None;
    while child.try_wait().unwrap().is_none() {
        std::thread::sleep(Duration::from_millis(100));
        if let Some(d) = fs::read_to_string(&feed)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        {
            // The first reading seen, well before the end.
            if seen_mid_take.is_none()
                && d["done"] == false
                && d["reading"]["beats_used"].as_u64().unwrap_or(0) > 0
            {
                seen_mid_take = Some(d);
            }
        }
    }
    let took = start.elapsed().as_secs_f64();
    let out = child.wait_with_output().unwrap();
    assert!(matches!(out.status.code(), Some(0 | 3)), "{:?}", out.status);

    // Played back in real time, so a live view sees the take as it goes.
    assert!(took >= seconds - 0.5, "took {took:.1} s");
    let mid = seen_mid_take.expect("a reading while recording");
    assert_eq!(mid["schema"], "timegrapher.live/1");
    assert_eq!(mid["position"], "CD");
    assert!(mid["recorded_s"].as_f64().unwrap() < seconds - 0.5, "{mid}");

    let last: Value = serde_json::from_str(&fs::read_to_string(&feed).unwrap()).unwrap();
    assert_eq!(last["done"], true);
    assert_eq!(last["recorded_s"], seconds);
    let rate = last["reading"]["rate_s_per_day"].as_f64().unwrap();
    assert!((rate - 6.0).abs() < 3.0, "rate {rate}");
    let beats = last["strip"]["beats"].as_array().unwrap();
    assert!(beats.len() > 10, "{} beats on the strip", beats.len());
    let t = &last["trend"];
    let n = t["t_s"].as_array().unwrap().len();
    assert!((1..=300).contains(&n));
    for k in ["rate", "amplitude", "beat_error"] {
        assert_eq!(t[k].as_array().unwrap().len(), n, "{k}");
    }
    assert!(!feed.with_extension("tmp").exists());

    // Standard output still carries the one document `record --json` prints.
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["schema"], "timegrapher.record/2");
    let _ = fs::remove_dir_all(&dir);
}

/// Without --live a file is read at once, as before.
#[test]
fn without_live_a_file_is_read_at_once() {
    let dir = scratch("plain");
    let wav = dir.join("watch.wav");
    assert!(bin()
        .args(["synth", wav.to_str().unwrap(), "--duration", "40"])
        .status()
        .unwrap()
        .success());
    let start = Instant::now();
    let out = bin()
        .args([
            "record",
            dir.join("test").to_str().unwrap(),
            "--position",
            "DU",
        ])
        .args(["--seconds", "30"])
        .arg(format!("--file={}", wav.display()))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "{:?}",
        start.elapsed()
    );
    let _ = fs::remove_dir_all(&dir);
}
