//! Consumed manifest validity without decoding sound or opening a device.
use std::path::PathBuf;
use std::process::{Command, Output};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "tg-persisted-manifest-inputs-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("session")).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        Self(root)
    }
    fn write(&self, fields: &str) -> Vec<u8> {
        let text=format!("# original text must survive refusal\n{fields}\n[[recording]]\nfile=\"missing.wav\"\nposition=\"DU\"\n");
        std::fs::write(self.0.join("session/session.toml"), &text).unwrap();
        text.into_bytes()
    }
    fn run(&self, command: &str, overrides: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_timegrapher"));
        cmd.env("TIMEGRAPHER_CONFIG_DIR", self.0.join("config"))
            .arg(command)
            .arg(self.0.join("session"));
        if command == "record" {
            cmd.arg("--file").arg(self.0.join("missing.wav")).args([
                "--position",
                "DU",
                "--seconds",
                "1",
            ]);
        }
        cmd.args(overrides).output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn consumed_invalid_manifest_inputs_fail_before_audio_or_append_even_with_flags() {
    let f = Fixture::new();
    for (fields, field) in [
        ("bph=0", "bph"),
        ("lift=nan", "lift"),
        ("lift=inf", "lift"),
        ("lift_angle=0", "lift"),
        ("escape_teeth=0", "escape_teeth"),
        ("card_ppm=-1000000", "card_ppm"),
        ("card_ppm=nan", "card_ppm"),
    ] {
        let original = f.write(fields);
        for command in ["session", "record"] {
            for overrides in [&[][..], &["--bph=28800", "--lift=52"][..]] {
                let out = f.run(command, overrides);
                let error = String::from_utf8_lossy(&out.stderr);
                assert!(!out.status.success(), "{command}: {fields}");
                assert!(
                    error.contains("session.toml") && error.contains(&format!("{field}:")),
                    "{command}: {error}"
                );
                assert!(
                    !error.contains("missing.wav"),
                    "audio lookup happened first: {error}"
                );
                assert_eq!(
                    std::fs::read(f.0.join("session/session.toml")).unwrap(),
                    original
                );
                assert_eq!(
                    std::fs::read_dir(f.0.join("session")).unwrap().count(),
                    1,
                    "no take/report should be created"
                );
            }
        }
    }
}

#[test]
fn valid_legacy_manifest_context_and_zero_correction_reach_audio_validation() {
    let f = Fixture::new();
    for fields in [
        "",
        "calibre=\"Unknown label\"\nlift_angle=52\nlift_known=false\ncard_ppm=0",
        "bph=21600\nlift=44\nescape_teeth=20\ncard_ppm=-12.5",
    ] {
        let original = f.write(fields);
        for command in ["session", "record"] {
            let out = f.run(command, &[]);
            let error = String::from_utf8_lossy(&out.stderr);
            assert!(!out.status.success());
            assert!(error.contains("missing.wav"), "{command}: {error}");
            assert_eq!(
                std::fs::read(f.0.join("session/session.toml")).unwrap(),
                original
            );
            assert_eq!(std::fs::read_dir(f.0.join("session")).unwrap().count(), 1);
        }
    }
}
