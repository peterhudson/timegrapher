//! `analyze` never writes its CSVs over a file of the take it reads: the
//! session file, record, clock log or test marks beside the recording, the
//! recording's sidecar, the kept analysis, another of the take's
//! recordings, or the FLAC read for a WAV kept as one (#2372).
use std::{
    path::{Path, PathBuf},
    process::Command,
};

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tg(args: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args(args)
        .output()
        .unwrap()
}

fn synth(wav: &Path) {
    let made = tg(&[
        "synth".as_ref(),
        wav.as_os_str(),
        "--duration=10".as_ref(),
        "--bph=21600".as_ref(),
    ]);
    assert!(made.status.success(), "{made:?}");
}

/// `analyze <recording> <flag>=<output>` is refused, saying `says`, and
/// leaves `output` as it was (or not there).
fn refused(recording: &Path, output: &Path, says: &str) {
    let before = std::fs::read(output).ok();
    for flag in ["--beats", "--windows", "--trend"] {
        let arg = format!("{flag}={}", output.display());
        let out = tg(&[
            "analyze".as_ref(),
            recording.as_os_str(),
            "--bph=21600".as_ref(),
            arg.as_ref(),
        ]);
        assert!(
            !out.status.success(),
            "{flag} {}: {out:?}",
            output.display()
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(says), "{flag} {}: {err}", output.display());
        assert_eq!(
            std::fs::read(output).ok(),
            before,
            "{flag} changed {}",
            output.display()
        );
    }
}

#[test]
fn an_output_naming_a_file_of_the_take_is_refused_and_the_file_kept() {
    let dir = Dir(std::env::temp_dir().join(format!("tg-take-files-{}", std::process::id())));
    let take = dir.0.join("take");
    std::fs::create_dir_all(&take).unwrap();
    let wav = take.join("take_part01-001.wav");
    synth(&wav);
    synth(&take.join("take_part01-002.wav"));
    std::fs::write(
        take.join("session.toml"),
        "bph = 21600\nlift = 52\n\n[[recording]]\nfile = \"take_part01-001.wav\"\nposition = \"DU\"\n\
         \n[[recording]]\nfile = \"take_part01-002.wav\"\nposition = \"CH\"\n",
    )
    .unwrap();
    std::fs::write(take.join("session.json"), "{}\n").unwrap();
    std::fs::write(take.join("clock.csv"), "audio_s,ntp_s\n").unwrap();
    // The user's test marks, the recording's sidecar and the kept analysis:
    // none can be told from a CSV once written over.
    for name in [
        "positions-test.json",
        "poise-test.json",
        "test-marks.json",
        "take_part01-001.wav.json",
    ] {
        std::fs::write(take.join(name), "{}\n").unwrap();
    }
    std::fs::create_dir_all(take.join("analysis")).unwrap();
    std::fs::write(take.join("analysis").join("analysis.json"), "{}\n").unwrap();
    for (name, says) in [
        ("session.toml", "take's session.toml"),
        ("session.json", "take's session.json"),
        ("clock.csv", "take's clock.csv"),
        // Not there yet, but read as the take's clock log once written.
        ("clock.log", "take's clock.log"),
        ("take_part01-002.wav", "recording of the take"),
        ("positions-test.json", "take's positions-test.json"),
        ("poise-test.json", "take's poise-test.json"),
        ("test-marks.json", "take's test-marks.json"),
        ("take_part01-001.wav.json", "recording's sidecar"),
        ("analysis/analysis.json", "in the take's analysis folder"),
    ] {
        refused(&wav, &take.join(name), says);
    }
    assert!(
        !take.join("clock.log").exists(),
        "no clock log is written for a refused output"
    );
    // An output of its own beside them is written.
    let beats = take.join("beats.csv");
    let arg = format!("--beats={}", beats.display());
    let out = tg(&["analyze".as_ref(), wav.as_os_str(), arg.as_ref()]);
    assert!(out.status.success(), "{out:?}");
    assert!(beats.is_file());
}

/// A WAV kept as FLAC is read from the FLAC, so the FLAC is the recording
/// being read and is refused as an output by the WAV's name.
#[test]
fn the_flac_read_for_a_kept_wav_is_refused() {
    let dir = Dir(std::env::temp_dir().join(format!("tg-take-flac-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).unwrap();
    let wav = dir.0.join("take_part01-001.wav");
    let flac = dir.0.join("take_part01-001.flac");
    synth(&wav);
    timegrapher_core::flac::encode_wav(&wav, &flac).unwrap();
    std::fs::remove_file(&wav).unwrap();
    refused(&wav, &flac, "recording being read");
}
