//! A recording with USB lines and no clock log is read on its lines, over a
//! correction given for its input, and says when the two disagree (#635).
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use timegrapher_core::synth::{generate, SynthConfig};

/// A minute of a watch on a card 20 ppm slow, with three of the
/// computer's USB lines (5, 6 and 7 kHz true) heard 20 ppm high on its
/// clock.
fn recording() -> PathBuf {
    let cfg = SynthConfig {
        duration_s: 60.0,
        ..Default::default()
    };
    let mut audio = generate(&cfg, |_| 250.0, |_| 0.0);
    let fs = audio.sample_rate as f64;
    for (f, a) in [(5000.0, 0.002), (6000.0, 0.003), (7000.0, 0.002)] {
        let f = f * (1.0 + 20e-6);
        for (i, v) in audio.samples.iter_mut().enumerate() {
            *v += (a * (2.0 * std::f64::consts::PI * f * i as f64 / fs).sin()) as f32;
        }
    }
    let dir = std::env::temp_dir().join(format!("tg-usb-over-correction-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("usb.wav");
    timegrapher_core::audio::write_wav(&path, &audio).unwrap();
    path
}

fn analyze(file: &std::path::Path, card_ppm: &str) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .env("TIMEGRAPHER_CONFIG_DIR", file.parent().unwrap())
        .args(["analyze", "--json", "--card-ppm", card_ppm])
        .arg(file)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn analyze_reads_the_usb_lines_over_a_given_correction_and_says_when_they_disagree() {
    let file = recording();
    let doc = analyze(&file, "5");
    let clock = &doc["clock"];
    assert!(
        (clock["ppm"].as_f64().unwrap() - 20.0).abs() < 0.5,
        "{clock}"
    );
    assert!(clock["from"].as_str().unwrap().contains("USB"), "{clock}");
    let caveat = doc["caveats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["code"] == "clock_sources_disagree")
        .unwrap_or_else(|| panic!("{}", doc["caveats"]));
    assert_eq!(caveat["against"], "correction");
    assert_eq!(caveat["log_ppm"], 5.0);
    // Within 5 ppm of the lines, the correction is not said.
    let doc = analyze(&file, "18");
    assert!((doc["clock"]["ppm"].as_f64().unwrap() - 20.0).abs() < 0.5);
    assert!(
        !doc["caveats"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["code"] == "clock_sources_disagree"),
        "{}",
        doc["caveats"]
    );
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}
