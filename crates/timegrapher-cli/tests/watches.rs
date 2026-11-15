//! `watches` lists the app's library without changing it.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use timegrapher_core::library::{self, Library, Watch, CACHE_FILE, TAKES, UNSORTED};

/// The settings folder the commands keep the takes' readings in, apart
/// from the user's own and this test's own: the tests run side by side in
/// one process, and two sharing one `readings.json` could each drop the
/// other's kept reading (#781). The test harness names each test's thread
/// after the test.
fn config_dir() -> PathBuf {
    let test = std::thread::current()
        .name()
        .unwrap_or("main")
        .replace("::", "-");
    std::env::temp_dir().join(format!(
        "tg-cli-watches-config-{test}-{}",
        std::process::id()
    ))
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
#[expect(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
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
    // Its id, as its watch.toml keeps it: stays when the watch is renamed.
    let id = w["id"].as_str().unwrap();
    assert!(
        std::fs::read_to_string(root.join("Pepsi").join(library::WATCH_FILE))
            .unwrap()
            .contains(&format!("id = \"{id}\""))
    );
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
    let _ = std::fs::remove_dir_all(config_dir());
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
        "2026-10-20 14.32 Dial Up: Dial Up · 10:00 · {:+.1} s/d · {:.0}° · {:.2} ms · ",
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
    let _ = std::fs::remove_dir_all(config_dir());
}

/// A take's kept reading is worked out again when the clock correction
/// stored for the input it was recorded on changes, though nothing in the
/// take's folder does, so the library and `session` give the same rate
/// (audit F1). A correction left as it was keeps the reading.
#[test]
fn a_kept_reading_follows_a_changed_clock_correction() {
    let root = library("clock");
    let lib = root.to_string_lossy().into_owned();
    let dir = root.join(UNSORTED).join(TAKES).join("2026-10-22 09.15");
    // Recorded on the input "Audit Mic", as the app writes it.
    std::fs::write(dir.join("session.json"), r#"{"device":"Audit Mic"}"#).unwrap();
    let reading = || {
        let out = run(&["watches", "show", "Unsorted", "--library", &lib, "--json"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
        let read_again = String::from_utf8_lossy(&out.stderr).contains("reading ");
        (doc["takes"][0]["reading"].clone(), read_again)
    };
    let (before, _) = reading();
    assert_eq!(before["calibrated"], false, "{before}");
    let (kept, read_again) = reading();
    assert_eq!(kept, before);
    assert!(!read_again, "nothing changed, so the kept reading is used");

    let out = run(&["clock", "set", "Audit Mic", "50"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (after, read_again) = reading();
    assert!(
        read_again,
        "the correction changed, so the take is read again"
    );
    let out = run(&["session", &dir.to_string_lossy(), "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let session: Value = serde_json::from_slice(&out.stdout).unwrap();
    let m = &session["readings"][0]["measurement"];
    assert_eq!(after["calibrated"], true, "{after}");
    assert_eq!(after["calibrated"], m["calibrated"], "{m}");
    assert_eq!(after["rate_s_per_day"], m["rate_s_per_day"], "{m}");
    assert_ne!(after["rate_s_per_day"], before["rate_s_per_day"]);
    let (kept, read_again) = reading();
    assert_eq!(kept, after);
    assert!(!read_again, "the correction is as it was kept");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(config_dir());
}

/// A take's kept reading is worked out again when the user's definition
/// of the calibre its session file names changes (here its lift angle),
/// though nothing in the take's folder does. A definition left as it was
/// keeps the reading.
#[test]
fn a_kept_reading_follows_a_changed_user_calibre() {
    let root = library("calibre");
    let lib = root.to_string_lossy().into_owned();
    let dir = root.join(UNSORTED).join(TAKES).join("2026-10-22 09.15");
    std::fs::write(
        dir.join("session.toml"),
        "bph = 28800\ncalibre = \"Bench 44\"\n\n[[recording]]\nfile = \".\"\nposition = \"DD\"\n",
    )
    .unwrap();
    let calibres = |lift: f64| {
        std::fs::create_dir_all(config_dir()).unwrap();
        std::fs::write(
            config_dir().join("calibres.json"),
            serde_json::json!({
                "schema": "timegrapher.user-calibres/1",
                "custom": [{"name": "Bench 44", "bph": 28800, "lift_angle_deg": lift}],
            })
            .to_string(),
        )
        .unwrap();
    };
    let reading = || {
        let out = run(&["watches", "show", "Unsorted", "--library", &lib, "--json"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
        let read_again = String::from_utf8_lossy(&out.stderr).contains("reading ");
        (doc["takes"][0]["reading"].clone(), read_again)
    };
    calibres(44.0);
    let (before, _) = reading();
    let (kept, read_again) = reading();
    assert_eq!(kept, before);
    assert!(!read_again, "nothing changed, so the kept reading is used");

    calibres(52.0);
    let (after, read_again) = reading();
    assert!(read_again, "the calibre changed, so the take is read again");
    // A larger lift angle reads the same beats as a larger amplitude.
    assert!(
        after["amplitude_deg"].as_f64().unwrap() > before["amplitude_deg"].as_f64().unwrap(),
        "{before} {after}"
    );
    let (kept, read_again) = reading();
    assert_eq!(kept, after);
    assert!(!read_again, "the calibre is as it was kept");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(config_dir());
}

/// A watch's full wind and each take's hours since it, as the app saves
/// them (#1745): the time as given, 24.0 for a take its session file gives
/// 24 h, and null for a take whose wind was not given, never 0 (#1665).
#[test]
fn watches_show_gives_the_full_wind_and_each_takes_wind() {
    let root = std::env::temp_dir().join(format!("tg-cli-watches-wind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut lib = Library::create(&root).unwrap();
    let mut w = Watch::new("Submariner");
    w.full_wind = Some("2026-11-13T19:00:00.000Z".into());
    let dir = lib.add(&w).unwrap();
    let wound = dir.join(TAKES).join("2026-11-14 19.00 Dial Up");
    take(&wound, "DU", "2026-11-14T19:00:00Z");
    std::fs::write(
        wound.join("session.toml"),
        "bph = 28800\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\nwind_h = 24.0\n",
    )
    .unwrap();
    take(
        &dir.join(TAKES).join("2026-11-12 09.00 Dial Up"),
        "DU",
        "2026-11-12T09:00:00Z",
    );
    drop(lib);
    let lib = root.to_string_lossy().into_owned();
    let out = run(&["watches", "show", "Submariner", "--library", &lib, "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["full_wind"], "2026-11-13T19:00:00.000Z");
    let wind = |name: &str| {
        doc["takes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("no take {name}: {doc}"))["wind_h"]
            .clone()
    };
    assert_eq!(wind("2026-11-14 19.00 Dial Up"), 24.0);
    assert_eq!(wind("2026-11-12 09.00 Dial Up"), Value::Null);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(config_dir());
}
