//! A report bundle of a take folder carries the take's clock log under
//! its own name: the recorder's `clock.csv`, or an older take's
//! `clocklog.txt` (#1886).
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
fn a_take_folders_clock_log_goes_into_the_bundle() {
    for name in ["clock.csv", "clocklog.txt"] {
        bundles_its_clock_log(name);
    }
}

fn bundles_its_clock_log(name: &str) {
    let dir =
        Dir(std::env::temp_dir().join(format!("tg-bundle-clock-{}-{name}", std::process::id())));
    let take = dir.0.join("take");
    std::fs::create_dir_all(&take).unwrap();
    let made = tg(&[
        "synth".as_ref(),
        take.join("take.wav").as_os_str(),
        "--duration=20".as_ref(),
    ]);
    assert!(made.status.success(), "{made:?}");
    std::fs::write(
        take.join(name),
        "audio_s,unix_s\n0,1763400000\n10,1763400010\n",
    )
    .unwrap();
    let out = dir.0.join("bundle");
    let run = tg(&[
        "report".as_ref(),
        take.as_os_str(),
        "--no-audio".as_ref(),
        "--out".as_ref(),
        out.as_os_str(),
    ]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let zips: Vec<PathBuf> = walk(&dir.0)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "zip"))
        .collect();
    assert_eq!(zips.len(), 1, "{zips:?}");
    let bytes = std::fs::read(&zips[0]).unwrap();
    let has = |name: &str| bytes.windows(name.len()).any(|w| w == name.as_bytes());
    assert!(
        has(&format!("long/{name}")),
        "the take's {name} is not in the bundle"
    );
}

fn walk(d: &std::path::Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            v.extend(walk(&p));
        } else {
            v.push(p);
        }
    }
    v
}
