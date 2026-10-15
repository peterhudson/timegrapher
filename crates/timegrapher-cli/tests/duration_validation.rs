//! Acquisition argument validation without opening a microphone or decoding audio.
use std::path::PathBuf;
use std::process::{Command, Output};

fn run(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn invalid_acquisition_lengths_fail_before_input_or_output_access() {
    let root = std::env::temp_dir().join(format!("tg-duration-{}", std::process::id()));
    let missing = root.join("missing.wav");
    let folder = root.join("must-not-be-created");
    for value in ["NaN", "inf", "-inf", "0", "-1", "1e300", "1e308", "1e-300"] {
        for (args, flag) in [
            (
                vec![
                    "doctor".into(),
                    format!("--file={}", missing.display()),
                    format!("--seconds={value}"),
                ],
                "--seconds",
            ),
            (
                vec![
                    "record".into(),
                    folder.to_string_lossy().into_owned(),
                    "--position=DU".into(),
                    format!("--file={}", missing.display()),
                    format!("--seconds={value}"),
                ],
                "--seconds",
            ),
            (
                vec![
                    "clock".into(),
                    "measure".into(),
                    "--device=__timegrapher_duration_test_missing_input__".into(),
                    format!("--minutes={value}"),
                ],
                "--minutes",
            ),
        ] {
            let out = run(&args);
            let error = String::from_utf8_lossy(&out.stderr);
            assert!(!out.status.success(), "{args:?}");
            assert!(error.contains(flag), "{args:?}: {error}");
            assert!(
                !error.contains(&missing.to_string_lossy().to_string()),
                "{error}"
            );
            assert!(!error.contains("no sound input"), "{error}");
            assert!(
                !folder.exists(),
                "invalid length created the recording folder"
            );
        }
    }
}

#[test]
fn default_and_ordinary_lengths_reach_file_validation() {
    let root = std::env::temp_dir().join(format!("tg-duration-valid-{}", std::process::id()));
    let missing: PathBuf = root.join("missing.wav");
    for seconds in [None, Some("5"), Some("120")] {
        let mut args = vec!["doctor".into(), format!("--file={}", missing.display())];
        if let Some(seconds) = seconds {
            args.push(format!("--seconds={seconds}"));
        }
        let out = run(&args);
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success());
        assert!(
            error.contains(&missing.to_string_lossy().to_string()),
            "{error}"
        );
        assert!(!error.contains("--seconds"), "{error}");
    }
}
