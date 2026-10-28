//! `watches` lists the app's library without changing it.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use timegrapher_core::library::{self, Library, Watch, CACHE_FILE, TAKES, UNSORTED};

/// The settings folder the commands keep the takes' readings in, apart
/// from the user's own.
fn config_dir() -> PathBuf {
    std::env::temp_dir().join(format!("tg-cli-watches-config-{}", std::process::id()))
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args(args)
        .env("TIMEGRAPHER_CONFIG_DIR", config_dir())
        .output()
        .unwrap()
}

/// A take as the app saves one: its session file, 40 s of a watch 10 s/d
/// fast and the readings at the end in session.json.
fn take(dir: &Path, position: &str, started: &str) {
    use timegrapher_core::synth::{generate, SynthConfig};
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("session.toml"),
        format!("bph = 28800\n\n[[recording]]\nfile = \".\"\nposition = \"{position}\"\n"),
    )
    .unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 40.0,
            rate_s_per_day: 10.0,
            beat_error_ms: 0.6,
            ..Default::default()
        },
        |_| 270.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&dir.join("take_part01-001.wav"), &audio).unwrap();
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
    // Its own reading, as `session` reads it: not the figures the app
    // showed when it saved the take, which stay as `at_end`.
    let r = &t["reading"];
    assert_eq!(r["position"], "DU", "{t}");
    assert_eq!(r["watch_heard"], true, "{t}");
    assert_eq!(r["calibrated"], false, "{t}");
    assert!(
        (r["rate_s_per_day"].as_f64().unwrap() - 10.0).abs() < 1.0,
        "{t}"
    );
    assert!(
        (r["beat_error_ms"].as_f64().unwrap().abs() - 0.6).abs() < 0.2,
        "{t}"
    );
    // The watch was made to swing 270°.
    assert!(
        (r["amplitude_deg"].as_f64().unwrap() - 270.0).abs() < 5.0,
        "{t}"
    );
    assert_eq!(t["at_end"]["rate_s_per_day"], 4.2);
    assert_eq!(t["at_end"]["beat_error_ms"], -0.6);
    // Kept in the settings folder, not in the library, and read from there
    // the next time.
    assert!(config_dir().join(library::READINGS_FILE).is_file());
    let again = run(&["watches", "show", "pepsi", "--library", &lib, "--json"]);
    assert!(!String::from_utf8_lossy(&again.stderr).contains("reading "));
    let doc2: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(doc2["takes"][0]["reading"], *r);
    // The same figures `session` gives for the take (which writes its
    // summary.json into the folder, so it comes last).
    let out = run(&["session", &path.to_string_lossy(), "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let session: Value = serde_json::from_slice(&out.stdout).unwrap();
    let m = &session["readings"][0]["measurement"];
    assert_eq!(r["rate_s_per_day"], m["rate_s_per_day"], "{m}");
    assert_eq!(r["amplitude_deg"], m["amplitude_deg"], "{m}");
    assert_eq!(r["beat_error_ms"], m["beat_error_unlock_ms"], "{m}");
    assert_eq!(r["calibrated"], m["calibrated"], "{m}");
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
    // The line shows the take's own reading, as the JSON gives it.
    let doc: Value = serde_json::from_slice(
        &run(&["watches", "show", "Pepsi", "--library", &lib, "--json"]).stdout,
    )
    .unwrap();
    let r = &doc["takes"][0]["reading"];
    let line = format!(
        "2026-10-20 14.32 Dial Up: Dial Up · 10:00 · {:+.1} s/d · {:.0}° · {:.1} ms · ",
        r["rate_s_per_day"].as_f64().unwrap(),
        r["amplitude_deg"].as_f64().unwrap(),
        r["beat_error_ms"].as_f64().unwrap().abs()
    );
    let out = run(&["watches", "show", "Pepsi", "--library", &lib]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(&line), "{line}: {text}");
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
