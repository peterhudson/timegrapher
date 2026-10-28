//! `watches` lists the app's library without changing it.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use timegrapher_core::library::{Library, Watch, CACHE_FILE, TAKES, UNSORTED};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args(args)
        .output()
        .unwrap()
}

/// A take as the app saves one: its session file, one sound file and the
/// readings at the end in session.json.
fn take(dir: &Path, position: &str, started: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("session.toml"),
        format!("bph = 28800\n\n[[recording]]\nfile = \".\"\nposition = \"{position}\"\n"),
    )
    .unwrap();
    std::fs::write(dir.join("take_part01-001.flac"), vec![0u8; 2000]).unwrap();
    std::fs::write(
        dir.join("session.json"),
        format!(
            r#"{{"started_utc":"{started}","duration_s":600.0,
                "result":{{"rate_s_per_day":4.2,"amplitude_deg":285.0,"beat_error_unlock_ms":-0.6}}}}"#
        ),
    )
    .unwrap();
}

fn library(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("tg-cli-watches-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut lib = Library::create(&root).unwrap();
    let mut w = Watch::new("Pepsi");
    w.calibre = Some("Rolex 3135".into());
    w.bph = Some(28800);
    let dir = lib.add(&w).unwrap();
    take(
        &dir.join(TAKES).join("2026-10-20 14.32 Dial Up"),
        "DU",
        "2026-10-20T21:32:00Z",
    );
    let mut old = Watch::new("Sold Seiko");
    old.archived = true;
    lib.add(&old).unwrap();
    take(
        &root.join(UNSORTED).join(TAKES).join("2026-10-22 09.15"),
        "DD",
        "2026-10-22T16:15:00Z",
    );
    drop(lib);
    let _ = std::fs::remove_file(root.join(CACHE_FILE));
    root
}

#[test]
fn watches_lists_and_shows_the_library_as_json() {
    let root = library("json");
    let lib = root.to_string_lossy().into_owned();
    // The app has the library open meanwhile: listing still works.
    let app = Library::open(&root).unwrap();
    assert_eq!(app.read_only, None);
    std::fs::remove_file(root.join(CACHE_FILE)).unwrap();
    let out = run(&["watches", "list", "--library", &lib, "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["schema"], "timegrapher.watches/1");
    assert_eq!(doc["library"], lib.as_str());
    assert_eq!(doc["unsorted"], 1);
    assert_eq!(doc["caveats"], serde_json::json!([]));
    assert!(doc["bytes"].as_u64().unwrap() >= 4000);
    let watches = doc["watches"].as_array().unwrap();
    // A watch an older app marked archived is listed like any other.
    assert_eq!(watches.len(), 2, "{doc}");
    assert_eq!(watches[1]["name"], "Sold Seiko");
    assert_eq!(watches[1]["archived"], true, "the field stays");
    let w = &watches[0];
    assert_eq!(w["name"], "Pepsi");
    assert_eq!(w["calibre"], "Rolex 3135");
    assert_eq!(w["bph"], 28800);
    assert!(w["lift_deg"].is_null());
    assert_eq!(w["takes"], 1);
    assert_eq!(w["newest_started_utc"], "2026-10-20T21:32:00Z");
    assert!(w.get("id").is_none());
    // It took no lock and wrote nothing, not even the cache.
    assert!(!root.join(CACHE_FILE).exists());
    drop(app);
    let reopened = Library::open(&root).unwrap();
    assert_eq!(reopened.read_only, None);
    drop(reopened);

    // --all is still accepted, and changes nothing.
    let out = run(&["watches", "list", "--all", "--library", &lib, "--json"]);
    assert!(out.status.success());
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["watches"].as_array().unwrap().len(), 2);

    // One watch, by its name in any case.
    let out = run(&["watches", "show", "pepsi", "--library", &lib, "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["schema"], "timegrapher.watch/1");
    assert_eq!(doc["name"], "Pepsi");
    assert_eq!(doc["unsorted"], false);
    assert_eq!(doc["photos"], 0);
    let t = &doc["takes"][0];
    let path = root
        .join("Pepsi")
        .join(TAKES)
        .join("2026-10-20 14.32 Dial Up");
    assert_eq!(t["path"], path.to_string_lossy().as_ref());
    assert_eq!(t["name"], "2026-10-20 14.32 Dial Up");
    assert_eq!(t["positions"][0], "DU");
    assert_eq!(t["duration_s"], 600.0);
    assert_eq!(t["audio"], true);
    assert_eq!(t["incomplete"], false);
    assert_eq!(t["at_end"]["rate_s_per_day"], 4.2);
    assert_eq!(t["at_end"]["beat_error_ms"], -0.6);
    assert!(t["bytes"].as_u64().unwrap() >= 2000);
    let out = run(&["watches", "show", "Sold Seiko", "--library", &lib, "--json"]);
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["archived"], true);
    let out = run(&["watches", "show", "Unsorted", "--library", &lib, "--json"]);
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["unsorted"], true);
    // As agent-interface.md describes it.
    assert_eq!(doc["name"], "Unsorted");
    assert_eq!(doc["archived"], false);
    assert_eq!(doc["fields"], serde_json::json!([]));
    assert_eq!(doc["photos"], 0);
    for k in ["calibre", "bph", "lift_deg", "notes"] {
        assert!(doc[k].is_null(), "{k}: {doc}");
    }
    assert_eq!(doc["takes"][0]["name"], "2026-10-22 09.15");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn watches_prints_text_and_says_when_there_is_nothing_to_list() {
    let root = library("text");
    let lib = root.to_string_lossy().into_owned();
    let out = run(&["watches", "list", "--library", &lib]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    for s in [
        "2 watches,",
        "Sold Seiko: ",
        "Unsorted: 1 take, made with Quick Listen",
        "Pepsi: Rolex 3135, 1 take, newest started 2026-10-20T21:32:00Z",
    ] {
        assert!(text.contains(s), "{s}: {text}");
    }
    assert!(!text.contains("Archived"), "{text}");
    let out = run(&["watches", "show", "Pepsi", "--library", &lib]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("2026-10-20 14.32 Dial Up: Dial Up · 10:00 · +4.2 s/d · 285° · 0.6 ms · "),
        "{text}"
    );
    // A Positions Test's take names its kind first, as the app's line does.
    std::fs::write(
        root.join("Pepsi")
            .join(TAKES)
            .join("2026-10-20 14.32 Dial Up")
            .join("positions-test.json"),
        "{}",
    )
    .unwrap();
    let out = run(&["watches", "show", "Pepsi", "--library", &lib]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("2026-10-20 14.32 Dial Up: Positions Test · Dial Up · 10:00 · "),
        "{text}"
    );

    let out = run(&["watches", "show", "Daytona", "--library", &lib]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no watch called \"Daytona\""));
    let empty = root.join("not a library");
    std::fs::create_dir_all(&empty).unwrap();
    let out = run(&["watches", "list", "--library", &empty.to_string_lossy()]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not a Timegrapher library"), "{err}");
    assert!(!empty.join("library.toml").exists());
    let _ = std::fs::remove_dir_all(&root);
}
