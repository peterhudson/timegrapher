//! `--wheel NAME=SECONDS` holds the period to core's domain for a length
//! of time (finite and above 0), as a usage error before any recording is
//! read (#2423): `series --wheel custom=NaN` once ran and wrote
//! `period_s: null`.
use std::process::Command;

fn run(command: &str, missing: &str, wheel: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args([command, missing, "--wheel", wheel, "--json"])
        .output()
        .unwrap()
}

#[test]
fn a_wheel_period_that_is_not_a_finite_positive_number_is_a_usage_error() {
    let missing = std::env::temp_dir().join(format!("tg-wheel-{}.wav", std::process::id()));
    let missing = missing.to_string_lossy().into_owned();
    for command in ["series", "long"] {
        for wheel in [
            "custom=NaN",
            "custom=inf",
            "custom=-inf",
            "custom=0",
            "custom=-5",
            "custom=soon",
            "custom",
        ] {
            let out = run(command, &missing, wheel);
            let error = String::from_utf8_lossy(&out.stderr);
            assert_eq!(
                out.status.code(),
                Some(2),
                "{command} --wheel {wheel}: {error}"
            );
            assert!(
                error.contains("--wheel"),
                "{command} --wheel {wheel}: {error}"
            );
            assert!(
                !error.contains(&missing),
                "{command} --wheel {wheel}: {error}"
            );
        }
        for wheel in ["custom=NaN", "custom=inf"] {
            let error = String::from_utf8_lossy(&run(command, &missing, wheel).stderr).into_owned();
            assert!(
                error.contains("period must be a finite number above 0"),
                "{command} --wheel {wheel}: {error}"
            );
        }
    }
}

#[test]
fn a_finite_positive_wheel_period_passes_to_the_recording() {
    let missing = std::env::temp_dir().join(format!("tg-wheel-ok-{}.wav", std::process::id()));
    let missing = missing.to_string_lossy().into_owned();
    for command in ["series", "long"] {
        let out = run(command, &missing, "third wheel=450");
        let error = String::from_utf8_lossy(&out.stderr);
        assert_ne!(out.status.code(), Some(2), "{command}: {error}");
        assert!(!error.contains("--wheel"), "{command}: {error}");
    }
}
