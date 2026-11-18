//! `analyze` never writes over the recording it reads (#1876).
use std::{path::PathBuf, process::Command};

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

#[test]
fn an_output_naming_the_recording_is_refused_and_the_recording_kept() {
    let dir = Dir(std::env::temp_dir().join(format!("tg-outputs-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).unwrap();
    let wav = dir.0.join("x.wav");
    let made = tg(&["synth".as_ref(), wav.as_os_str(), "--duration=10".as_ref()]);
    assert!(made.status.success(), "{made:?}");
    let before = std::fs::read(&wav).unwrap();
    for flag in ["--beats", "--windows", "--trend"] {
        let arg = format!("{flag}={}", wav.display());
        let out = tg(&[
            "analyze".as_ref(),
            wav.as_os_str(),
            "--bph=21600".as_ref(),
            arg.as_ref(),
        ]);
        assert!(!out.status.success(), "{flag}: {out:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("recording being read"), "{flag}: {err}");
        assert_eq!(
            std::fs::read(&wav).unwrap(),
            before,
            "{flag} changed the recording"
        );
    }
    let csv = dir.0.join("both.csv");
    let (b, w) = (
        format!("--beats={}", csv.display()),
        format!("--windows={}", csv.display()),
    );
    let out = tg(&["analyze".as_ref(), wav.as_os_str(), b.as_ref(), w.as_ref()]);
    assert!(!out.status.success(), "{out:?}");
    assert!(
        !csv.exists(),
        "nothing is written when two outputs share a file"
    );
}
