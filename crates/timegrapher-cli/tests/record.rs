//! `timegrapher record` with a file standing in for the microphone: the
//! take folder, the session file it builds up, and the takes it refuses.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_timegrapher"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tg-record-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn synth(dir: &Path) -> PathBuf {
    let f = dir.join("watch.wav");
    let out = bin()
        .args(["synth", f.to_str().unwrap(), "--duration", "40"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    f
}

/// 40 s of hiss at 48 kHz, 16-bit mono: an empty stand.
fn hiss(dir: &Path) -> PathBuf {
    let f = dir.join("hiss.wav");
    let n: u32 = 48_000 * 40;
    let mut b = Vec::with_capacity(44 + 2 * n as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + 2 * n).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&48_000u32.to_le_bytes());
    b.extend_from_slice(&96_000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(2 * n).to_le_bytes());
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    for _ in 0..n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let s = ((x >> 48) as i16) / 16;
        b.extend_from_slice(&s.to_le_bytes());
    }
    fs::write(&f, b).unwrap();
    f
}

fn record(folder: &Path, file: &Path, extra: &[&str]) -> Output {
    bin()
        .args([
            "record",
            folder.to_str().unwrap(),
            "--file",
            file.to_str().unwrap(),
        ])
        .args(["--seconds", "35", "--json"])
        .args(extra)
        .output()
        .unwrap()
}

fn doc(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

#[test]
fn takes_build_up_a_session() {
    let d = scratch("session");
    let wav = synth(&d);
    let sess = d.join("sess");

    let out = record(
        &sess,
        &wav,
        &["--position", "DU", "--calibre", "2824-2", "--watch", "Test"],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let j = doc(&out);
    assert_eq!(j["schema"], "timegrapher.record/1");
    assert_eq!(j["verdict"], "ok");
    assert_eq!(j["position"], "DU");
    assert_eq!(j["check"]["watch_heard"], true);
    assert_eq!(j["check"]["bph"], 28800);
    let rate = j["check"]["rate_s_per_day"].as_f64().unwrap();
    assert!((rate - 5.0).abs() < 0.5, "rate {rate}");

    // The take is a folder as the app saves it, with the app's session file.
    let take = PathBuf::from(j["take"].as_str().unwrap());
    for f in ["audio-001.wav", "clock.csv", "session.json", "session.toml"] {
        assert!(take.join(f).is_file(), "{f} missing from the take");
    }
    let inner = fs::read_to_string(take.join("session.toml")).unwrap();
    assert!(inner.contains("file = \".\"") && inner.contains("position = \"DU\""));

    // The second take goes in the same session file, 24 h after winding,
    // with the calibre's settings kept from the first.
    let out = record(&sess, &wav, &["--position", "crown left", "--wind-h", "24"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let j = doc(&out);
    assert_eq!(j["position"], "CL");
    assert_eq!(j["input"]["settings"]["calibre"], "ETA 2824-2");

    let text = fs::read_to_string(sess.join("session.toml")).unwrap();
    assert!(text.starts_with("# Written by `timegrapher record`"));
    assert!(text.contains("calibre = \"ETA 2824-2\""));
    assert!(text.contains("settle_s = 30.0"));
    assert_eq!(text.matches("[[recording]]").count(), 2);
    assert!(text.contains("position = \"CL\"\nwind_h = 24.0"));

    // `session` reads the folder as it stands.
    let out = bin()
        .args([
            "session",
            sess.to_str().unwrap(),
            "--json",
            "--no-shape",
            "--no-cycles",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = doc(&out);
    let readings = s["readings"].as_array().unwrap();
    assert_eq!(readings.len(), 2);
    assert_eq!(readings[1]["wind_h"], 24.0);
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn an_empty_stand_is_not_added() {
    let d = scratch("empty");
    let wav = hiss(&d);
    let sess = d.join("sess");
    let out = record(&sess, &wav, &["--position", "DU", "--bph", "28800"]);
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let j = doc(&out);
    assert_eq!(j["verdict"], "no_watch_heard");
    assert_eq!(j["check"]["watch_heard"], false);
    assert!(j["check"]["rate_s_per_day"].is_null());
    assert!(j["session_file"].is_null());
    assert!(!sess.join("session.toml").exists());
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn settings_must_agree_with_the_session() {
    let d = scratch("agree");
    let wav = synth(&d);
    let sess = d.join("sess");
    let out = record(&sess, &wav, &["--position", "DU", "--lift", "52"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let before = fs::read_to_string(sess.join("session.toml")).unwrap();

    let out = record(&sess, &wav, &["--position", "DD", "--lift", "55"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("make them agree"));
    assert_eq!(
        fs::read_to_string(sess.join("session.toml")).unwrap(),
        before
    );

    let out = record(&sess, &wav, &["--position", "sideways"]);
    assert_eq!(out.status.code(), Some(1));
    let _ = fs::remove_dir_all(&d);
}
