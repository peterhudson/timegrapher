//! A session file's own `cycles = false` turns the cycle search off, and
//! the report says it was not searched for, never that none were found
//! (#1880).
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
fn a_session_files_cycles_false_reads_as_not_searched_for() {
    let root = Dir(std::env::temp_dir().join(format!("tg-cycles-off-{}", std::process::id())));
    let take = root.0.join("take");
    std::fs::create_dir_all(&take).unwrap();
    let made = tg(&take, &["synth", "DU.wav", "--duration=40", "--rate=4"]);
    assert!(made.status.success(), "{made:?}");
    let report = |cycles: &str, out: &str| {
        std::fs::write(
            take.join("session.toml"),
            format!("{cycles}[[recording]]\nfile = \"DU.wav\"\nposition = \"DU\"\n"),
        )
        .unwrap();
        let run = tg(&root.0, &["session", "take", "--out", out]);
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        std::fs::read_to_string(root.0.join(out).join("report.html")).unwrap()
    };
    let off = report("cycles = false\n\n", "off");
    assert!(off.contains("Not searched for"), "{off}");
    assert!(!off.contains("No periodic change"), "{off}");
    let on = report("", "on");
    assert!(!on.contains("Not searched for"), "{on}");
}
