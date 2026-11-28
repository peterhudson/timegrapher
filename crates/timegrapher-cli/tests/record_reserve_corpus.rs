//! The #2496 reporter's R16 recipe through `timegrapher record
//! --power-reserve` on Auto: real shop noise (the corpus take
//! `shopvoices_28800_DU_14s`, clip C) makes Auto let the watch's 28,800
//! lock go and relock at 72,000 late in the run. The stop check keeps the
//! rate the watch is heard at, so the test runs the whole file and ends by
//! hand (#2527). Read at the lock of the moment (main's rule before #2527),
//! it ended stopped at 752.0 s with the last beat at 737.89 s (#2531).
//!
//! Runs where `TIMEGRAPHER_CORPUS` is set (the `regress (engine)` CI job,
//! whose `--smoke` fetch brings this take), and skips elsewhere unless
//! `TIMEGRAPHER_REQUIRE_CORPUS` is set.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use timegrapher_core::archive::sha256_hex;

const TAKE: &str = "shopvoices_28800_DU_14s";
const CLIP: &str = "take_part01-001.flac";
/// The reporter's R16.flac (#2496); a rebuild on main matched it.
const R16_SHA256: &str = "f35e670c9df7d54ba720df188b38951e61381044259f329664d18c0166c28e4e";

fn run(args: &[&str]) -> std::process::Output {
    let out = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn synth(dir: &Path, name: &str, seconds: &str, bph: &str) -> String {
    let f = dir.join(name).to_str().unwrap().to_string();
    run(&["synth", &f, "--duration", seconds, "--bph", bph]);
    f
}

/// 6 s of digital silence, 48 kHz 16-bit mono: the bytes Python's `wave`
/// writes for the recipe's `zeros6.wav`.
fn zeros(dir: &Path) -> String {
    let data = 2 * 6 * 48_000u32;
    let mut w = Vec::with_capacity(44 + data as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&48_000u32.to_le_bytes());
    w.extend_from_slice(&96_000u32.to_le_bytes()); // bytes per second
    w.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data.to_le_bytes());
    w.resize(44 + data as usize, 0);
    let f = dir.join("zeros6.wav");
    fs::write(&f, w).unwrap();
    f.to_str().unwrap().to_string()
}

/// The clip, checked against the corpus manifest's SHA-256.
fn clip(root: &Path) -> PathBuf {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("corpus.json")).unwrap()).unwrap();
    let take = manifest["takes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == TAKE)
        .unwrap_or_else(|| panic!("corpus.json has no {TAKE}"));
    let file = take["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == CLIP)
        .unwrap_or_else(|| panic!("{TAKE} has no {CLIP}"));
    let path = root.join("takes").join(TAKE).join(CLIP);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(
        sha256_hex(&bytes),
        file["sha256"].as_str().unwrap(),
        "{}",
        path.display()
    );
    path
}

/// R16: 60 s of a 28,800 watch, 6 s of silence (Auto lets the lock go),
/// 32 s of 72,000 clicks (logged under a 72,000 lock of their own), 640 s
/// of the watch, clip C, then 20 s of the watch. 772.0 s in all.
#[test]
fn a_relock_on_real_shop_noise_does_not_end_the_test_as_stopped() {
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(
            std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_none(),
            "TIMEGRAPHER_CORPUS is not set"
        );
        eprintln!("SKIP R16: TIMEGRAPHER_CORPUS not set");
        return;
    };
    let clip = clip(&root);
    let d = std::env::temp_dir().join(format!("tg-reserve-r16-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    let parts = [
        synth(&d, "w60.wav", "60", "28800"),
        zeros(&d),
        synth(&d, "f32.wav", "32", "72000"),
        synth(&d, "w640.wav", "640", "28800"),
        clip.to_str().unwrap().to_string(),
        synth(&d, "w20.wav", "20", "28800"),
    ];
    let r16 = d.join("R16.flac");
    let r16 = r16.to_str().unwrap();
    let mut args = vec!["excerpt"];
    args.extend(parts.iter().map(String::as_str));
    args.extend(["--out", r16]);
    run(&args);
    // The very file the reporter ran, so this is their case and no other.
    assert_eq!(sha256_hex(&fs::read(r16).unwrap()), R16_SHA256);
    let take = d.join("R16");
    let out = run(&[
        "record",
        take.to_str().unwrap(),
        "--position",
        "DU",
        "--file",
        r16,
        "--power-reserve",
        "--json",
    ]);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["power_reserve"]["ending"], "by_hand", "{doc}");
    // The last beat is the file's last tick (771.92 s to the hundredth, as
    // #2496 and #2527 give it), not a beat from before the noise.
    let last = doc["power_reserve"]["last_beat_s"].as_f64().unwrap();
    assert!((last - 771.92).abs() < 0.005, "{doc}");
    assert_eq!(doc["check"]["to_s"], 772.0, "{doc}");
    let _ = fs::remove_dir_all(&d);
}
