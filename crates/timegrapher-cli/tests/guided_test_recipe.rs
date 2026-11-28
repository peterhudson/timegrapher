//! The watchmaker skill's guided test, step 5: `record` a take, then
//! `analyze` the take folder it names for the Tick and Tock shapes. Runs
//! the recipe as the skill writes it, so the skill cannot point at a file
//! a take does not hold (#2425).

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const GUIDED_TEST: &str = include_str!("../../../skills/watchmaker/references/guided-test.md");
const RECIPE: &str = "timegrapher analyze TAKE --calibre NAME --json";

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_timegrapher"))
}

fn ok(out: &Output) -> serde_json::Value {
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn the_guided_tests_shape_recipe_runs_on_a_recorded_take() {
    // The skill gives the recipe in these words, on a take folder, and
    // names no sound file of its own.
    assert!(
        GUIDED_TEST.lines().any(|l| l.trim() == RECIPE),
        "guided-test.md no longer gives `{RECIPE}`"
    );
    assert!(!GUIDED_TEST.contains("audio-001.wav"));

    let d = std::env::temp_dir().join(format!("tg-guided-recipe-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    let wav = d.join("watch.wav");
    let w = wav.to_str().unwrap();
    assert!(bin()
        .args(["synth", w, "--duration", "40"])
        .status()
        .unwrap()
        .success());

    let sess = d.join("sess");
    let rec = ok(&bin()
        .args(["record", sess.to_str().unwrap(), "--file", w])
        .args(["--position", "DU", "--seconds", "35"])
        .args(["--calibre", "2824-2", "--json"])
        .output()
        .unwrap());
    let take = rec["take"].as_str().unwrap();

    // The recipe, TAKE and NAME filled in from `record`'s output.
    let cmd: Vec<String> = RECIPE
        .split_whitespace()
        .skip(1)
        .map(|a| match a {
            "TAKE" => take.to_owned(),
            "NAME" => "2824-2".to_owned(),
            a => a.to_owned(),
        })
        .collect();
    let j = ok(&bin().args(&cmd).output().unwrap());
    assert_eq!(j["schema"], "timegrapher.analyze/3");
    assert_eq!(j["watch_heard"], true);
    assert_eq!(j["bph"], 28800);
    for side in ["even", "odd"] {
        assert!(
            j["shape"][side]["windows_measured"].as_u64().unwrap() > 0,
            "no {side} shape: {}",
            j["shape"]
        );
    }
    let _ = fs::remove_dir_all(PathBuf::from(&d));
}
