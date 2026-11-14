//! Number flags refuse NaN, the infinities and values outside their
//! domain with a plain error naming the flag, before any recording is
//! read (#1650): `--highpass=NaN` once ran and reported no watch heard.
use std::process::Command;

/// Each command, a flag and its values that must be refused; the recording
/// named does not exist, so an error about the flag shows it was checked
/// before the file was opened.
const CASES: &[(&str, &str, &[&str])] = &[
    ("analyze", "--highpass", &["NaN", "inf", "-inf", "0", "-5"]),
    ("analyze", "--notch", &["NaN", "inf", "0", "50,inf"]),
    ("shape", "--highpass", &["NaN", "inf"]),
    ("shape", "--notch", &["inf"]),
    ("profile", "--at", &["NaN", "inf", "-1"]),
    ("profile", "--span", &["NaN", "inf", "0"]),
    ("profile", "--highpass", &["NaN"]),
    ("long", "--highpass", &["NaN"]),
    ("long", "--notch", &["inf"]),
    ("series", "--highpass", &["NaN"]),
    ("series", "--reading", &["NaN", "inf", "0"]),
    ("series", "--wind-h", &["NaN", "inf", "-1"]),
    ("series", "--stretch-min", &["NaN", "inf", "-1"]),
    ("report", "--at", &["NaN", "-1"]),
    ("report", "--span", &["inf", "0"]),
];

#[test]
fn non_finite_and_out_of_range_numbers_are_refused_before_reading() {
    let missing = std::env::temp_dir().join(format!("tg-finite-{}.wav", std::process::id()));
    let missing = missing.to_string_lossy().into_owned();
    for (command, flag, values) in CASES {
        for value in *values {
            let out = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
                .args([
                    *command,
                    missing.as_str(),
                    format!("{flag}={value}").as_str(),
                    "--json",
                ])
                .output()
                .unwrap();
            let error = String::from_utf8_lossy(&out.stderr);
            assert!(!out.status.success(), "{command} {flag}={value} ran");
            assert!(error.contains(flag), "{command} {flag}={value}: {error}");
            assert!(
                !error.contains(&missing),
                "{command} {flag}={value}: {error}"
            );
        }
    }
}

#[test]
fn synth_and_regress_numbers_are_refused_too() {
    let out_file = std::env::temp_dir().join(format!("tg-finite-synth-{}.wav", std::process::id()));
    let synth: &[(&str, &str)] = &[
        ("--duration", "NaN"),
        ("--duration", "0"),
        ("--amplitude", "inf"),
        ("--rate", "NaN"),
        ("--beat-error", "inf"),
        ("--snr", "NaN"),
        ("--fault-period", "0"),
        ("--fault-length", "inf"),
        ("--fault-rate", "NaN"),
        ("--fault-amplitude", "inf"),
    ];
    for (flag, value) in synth {
        let out = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
            .args([
                "synth",
                &out_file.to_string_lossy(),
                &format!("{flag}={value}"),
            ])
            .output()
            .unwrap();
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "synth {flag}={value} ran");
        assert!(error.contains(flag), "synth {flag}={value}: {error}");
        assert!(!out_file.exists(), "synth {flag}={value} wrote a file");
    }
    let root = std::env::temp_dir().join(format!("tg-finite-regress-{}", std::process::id()));
    for flag in ["--rate-margin", "--amplitude-margin", "--beat-error-margin"] {
        for value in ["NaN", "inf", "-0.1"] {
            let out = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
                .args([
                    "regress",
                    &root.to_string_lossy(),
                    &format!("{flag}={value}"),
                ])
                .output()
                .unwrap();
            let error = String::from_utf8_lossy(&out.stderr);
            assert!(!out.status.success(), "regress {flag}={value} ran");
            assert!(error.contains(flag), "regress {flag}={value}: {error}");
        }
    }
    // Ordinary values still parse: a signed rate and a fault's negative
    // amplitude step.
    let ok = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args([
            "synth",
            &out_file.to_string_lossy(),
            "--duration=2",
            "--rate=-3",
            "--fault-amplitude=-15",
        ])
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let _ = std::fs::remove_file(&out_file);
}
