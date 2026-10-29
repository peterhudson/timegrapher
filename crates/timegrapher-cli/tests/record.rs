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
    assert_eq!(j["schema"], "timegrapher.record/2");
    assert!(j.get("verdict").is_none());
    assert_eq!(j["position"], "DU");
    assert_eq!(j["check"]["watch_heard"], true);
    assert_eq!(j["check"]["bph"], 28800);
    let rate = j["check"]["rate_s_per_day"].as_f64().unwrap();
    assert!((rate - 5.0).abs() < 0.5, "rate {rate}");
    // No time since winding was given, so none is claimed.
    assert!(j["wind_h"].is_null());
    // The check reads the stretch `session` reads: after the settling
    // time (the 20 s default, held to half of this 35 s take: the session
    // file gives none).
    assert_eq!(j["check"]["from_s"], 17.5);
    let du = j["check"].clone();

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
    // No settling time is written, so `session`'s default applies.
    assert!(!text.contains("settle_s"));
    assert_eq!(text.matches("[[recording]]").count(), 2);
    assert!(text.contains("position = \"CL\"\nwind_h = 24.0"));
    assert!(text.contains("position = \"DU\"\ndate = "));
    assert_eq!(text.matches("wind_h").count(), 1);

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
    // The quick check and the report agree on the same take.
    let r0 = &readings[0]["measurement"];
    for (check, report) in [
        ("rate_s_per_day", "rate_s_per_day"),
        ("amplitude_deg", "amplitude_deg"),
        ("beat_error_ms", "beat_error_unlock_ms"),
    ] {
        let a = du[check].as_f64().unwrap();
        let b = r0[report]
            .as_f64()
            .unwrap_or_else(|| panic!("{report} in {r0}"));
        assert!((a - b).abs() < 1e-9, "{check}: record {a}, session {b}");
    }

    // An entry given more settling than its take holds reads nothing: it
    // is noted and left out, and the rest are still read.
    let file = text
        .lines()
        .find(|l| l.starts_with("file = "))
        .expect("an entry's file");
    fs::write(
        sess.join("session.toml"),
        format!("{text}\n[[recording]]\n{file}\nposition = \"DD\"\nsettle_s = 1000.0\n"),
    )
    .unwrap();
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
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(err.contains("(DD) at 0 s is left out"), "{err}");
    let j = doc(&out);
    assert_eq!(j["readings"].as_array().unwrap().len(), 2);
    let left = j["left_out"].as_array().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0]["position"], "DD");
    assert!(
        left[0]["why"]
            .as_str()
            .unwrap()
            .contains("settle_s of 1000"),
        "{j}"
    );
    // The summary and the report say so too.
    let out = bin()
        .args([
            "session",
            sess.to_str().unwrap(),
            "--no-shape",
            "--no-cycles",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("Left out     ") && text.contains("(DD) at 0 s"),
        "{text}"
    );
    let html = fs::read_to_string(sess.join("session_report").join("report.html")).unwrap();
    assert!(html.contains("Left out: "), "{html}");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn an_empty_stand_is_kept_and_stated() {
    let d = scratch("empty");
    let wav = hiss(&d);
    let sess = d.join("sess");
    let out = record(&sess, &wav, &["--position", "DU", "--bph", "28800"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let j = doc(&out);
    assert_eq!(j["check"]["watch_heard"], false);
    assert!(j["check"]["rate_s_per_day"].is_null());
    assert!(j["check"]["timing_scatter_fraction"].as_f64().unwrap() >= 0.02);
    let toml = fs::read_to_string(sess.join("session.toml")).unwrap();
    assert!(
        toml.contains("# No regular beat found (timing scatter"),
        "{toml}"
    );
    assert!(toml.contains("[[recording]]"), "{toml}");
    assert_eq!(
        j["session_file"],
        sess.join("session.toml").display().to_string()
    );
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
