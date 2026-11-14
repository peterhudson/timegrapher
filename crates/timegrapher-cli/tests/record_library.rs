//! `timegrapher record --watch NAME` without a FOLDER: the session folder
//! is made among the takes of a watch in the app's library, with the
//! watch's settings, and the library is not touched while the app has it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use timegrapher_core::library::{Library, Watch, TAKES};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_timegrapher"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tg-record-lib-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn synth(dir: &Path) -> PathBuf {
    let f = dir.join("watch.wav");
    let out = bin()
        .args(["synth", f.to_str().unwrap(), "--duration", "40"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    f
}

fn record(args: &[&str], file: &Path) -> Output {
    bin()
        .arg("record")
        .args(args)
        .args([
            "--file",
            file.to_str().unwrap(),
            "--seconds",
            "35",
            "--json",
        ])
        .output()
        .unwrap()
}

fn doc(out: &Output) -> serde_json::Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn record_files_a_test_under_a_library_watch() {
    let d = scratch("watch");
    let wav = synth(&d);
    let root = d.join("Timegrapher Data");
    let mut lib = Library::create(&root).unwrap();
    let mut w = Watch::new("Pepsi");
    w.bph = Some(28800);
    w.lift = Some(53.0);
    let dir = lib.add(&w).unwrap();
    let lib_arg = root.to_str().unwrap();

    // While the app has the library open, nothing is recorded into it.
    let out = record(
        &["--watch", "Pepsi", "--library", lib_arg, "--position", "DU"],
        &wav,
    );
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("quit the app"), "{err}");
    assert_eq!(fs::read_dir(dir.join(TAKES)).unwrap().count(), 0);
    drop(lib);

    // The watch named in any case; its beat rate and lift angle are used.
    let j = doc(&record(
        &["--watch", "pepsi", "--library", lib_arg, "--position", "DU"],
        &wav,
    ));
    assert_eq!(j["library_watch"], "Pepsi");
    assert_eq!(j["check"]["bph"], 28800);
    assert_eq!(j["input"]["settings"]["lift_deg"], 53.0);
    let session = PathBuf::from(j["session_file"].as_str().unwrap());
    let folder = session.parent().unwrap().to_path_buf();
    assert_eq!(folder.parent().unwrap(), dir.join(TAKES));
    let text = fs::read_to_string(&session).unwrap();
    assert!(text.contains("watch = \"Pepsi\""), "{text}");
    assert!(text.contains("lift = 53"), "{text}");

    // The next position goes into the same folder, given as FOLDER.
    let j = doc(&record(
        &[folder.to_str().unwrap(), "--position", "DD"],
        &wav,
    ));
    assert!(j["library_watch"].is_null());
    let lib = Library::open(&root).unwrap();
    let takes = &lib.find("Pepsi").unwrap().takes;
    assert_eq!(takes.len(), 1);
    assert_eq!(takes[0].positions, ["DU", "DD"]);
    assert!(!takes[0].incomplete);
    // And while the app has it open, not even that.
    let out = record(&[folder.to_str().unwrap(), "--position", "CU"], &wav);
    assert_eq!(out.status.code(), Some(1));
    drop(lib);

    // No such watch, no library, or neither FOLDER nor --watch.
    let out = record(
        &[
            "--watch",
            "Daytona",
            "--library",
            lib_arg,
            "--position",
            "DU",
        ],
        &wav,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no watch called \"Daytona\""));
    let elsewhere = d.join("elsewhere");
    let out = record(
        &[
            "--watch",
            "Pepsi",
            "--library",
            elsewhere.to_str().unwrap(),
            "--position",
            "DU",
        ],
        &wav,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(!elsewhere.exists());
    let out = record(&["--position", "DU"], &wav);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--watch NAME"));
    let _ = fs::remove_dir_all(&d);
}

/// While the app has the library open, a take is not recorded into
/// through a link either: to the take, to its watch, or to the library
/// (#1641). The take's session file is left as it was.
#[cfg(unix)]
#[test]
fn record_through_a_link_into_the_library_is_refused_while_the_app_has_it() {
    let d = scratch("link");
    let wav = synth(&d);
    let root = d.join("Timegrapher Data");
    let mut lib = Library::create(&root).unwrap();
    let dir = lib.add(&Watch::new("Pepsi")).unwrap();
    drop(lib);
    let take = dir.join(TAKES).join("first");
    let j = doc(&record(&[take.to_str().unwrap(), "--position", "DU"], &wav));
    let session = PathBuf::from(j["session_file"].as_str().unwrap());
    let before = fs::read(&session).unwrap();
    let links = d.join("links");
    fs::create_dir_all(&links).unwrap();
    let link = |name: &str, to: &Path| {
        let l = links.join(name);
        std::os::unix::fs::symlink(to, &l).unwrap();
        l
    };
    let aliases = [
        link("take", &take),
        link("watch", &dir).join(TAKES).join("first"),
        link("watch-new", &dir).join(TAKES).join("second"),
        link("library", &root)
            .join(dir.strip_prefix(&root).unwrap())
            .join(TAKES)
            .join("first"),
    ];
    let held = Library::open(&root).unwrap();
    assert!(held.read_only.is_none(), "this test holds the lock");
    for alias in &aliases {
        let out = record(&[alias.to_str().unwrap(), "--position", "DD"], &wav);
        assert_eq!(out.status.code(), Some(1), "{}", alias.display());
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("quit the app"), "{}: {err}", alias.display());
    }
    assert_eq!(fs::read(&session).unwrap(), before);
    assert!(!dir.join(TAKES).join("second").exists());
    drop(held);
    let _ = fs::remove_dir_all(&d);
}
