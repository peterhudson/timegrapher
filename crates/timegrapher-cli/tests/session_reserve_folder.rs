//! `session` of loose files reads a Power Reserve sidecar from the folder
//! the files share, never from the folder it is run in (#1879). A broken
//! `power-reserve.json` shows which one is read: reading it is an error.
use std::{path::PathBuf, process::Command};

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tg(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn loose_files_read_their_own_folders_reserve_not_the_working_folders() {
    let root = Dir(std::env::temp_dir().join(format!("tg-reserve-cwd-{}", std::process::id())));
    let (a, b) = (root.0.join("watchA"), root.0.join("watchB"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    for name in ["DU.wav", "DD.wav"] {
        let made = tg(&b, &["synth", name, "--duration=40", "--rate=4"]);
        assert!(made.status.success(), "{made:?}");
    }
    std::fs::write(a.join("power-reserve.json"), "not watch B's").unwrap();
    let session = |out: &str| {
        tg(
            &a,
            &[
                "session",
                "../watchB/DU.wav",
                "../watchB/DD.wav",
                "--out",
                out,
            ],
        )
    };
    let run = session("../out1");
    assert!(
        run.status.success(),
        "watch A's reserve was read for watch B: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    std::fs::write(b.join("power-reserve.json"), "watch B's own").unwrap();
    let run = session("../out2");
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(
        !run.status.success() && err.contains("watchB"),
        "watch B's reserve unread: {err}"
    );
}
