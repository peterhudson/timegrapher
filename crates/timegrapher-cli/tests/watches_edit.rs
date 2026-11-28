//! `watches new|edit|add-take|move-take|keep-analysis-only|delete-take|
//! add-photos|delete-photo` make the app's Watches changes, and only
//! `add-take` works while the app has the library open.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use timegrapher_core::library::{Library, TAKES};

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tg-cli-watches-edit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(base: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args(args)
        .args(["--library", base.join("lib").to_str().unwrap()])
        .env("TIMEGRAPHER_CONFIG_DIR", base.join("config"))
        .output()
        .unwrap()
}

/// The `--json` document of a change that works.
fn ok(base: &Path, args: &[&str]) -> Value {
    let mut a = args.to_vec();
    a.push("--json");
    let out = run(base, &a);
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// Why a change is refused.
fn refused(base: &Path, args: &[&str]) -> String {
    let out = run(base, args);
    assert!(!out.status.success(), "{args:?} was not refused");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn show(base: &Path, name: &str) -> Value {
    ok(base, &["watches", "show", name])
}

/// A take as the app saves one: its session file and 40 s of a watch.
fn take(dir: &Path) {
    use timegrapher_core::synth::{generate, SynthConfig};
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("session.toml"),
        "bph = 28800\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let audio = generate(&SynthConfig::default(), |_| 270.0, |_| 0.0);
    timegrapher_core::audio::write_wav(&dir.join("take_part01-001.wav"), &audio).unwrap();
    std::fs::write(
        dir.join("session.json"),
        r#"{"started_utc":"2026-11-12T17:00:00Z","duration_s":60.0}"#,
    )
    .unwrap();
}

#[test]
fn a_watch_is_added_and_edited_as_the_app_does() {
    let base = scratch("details");
    // The first watch makes the library.
    let doc = ok(
        &base,
        &[
            "watches",
            "new",
            "Pepsi",
            "--calibre",
            "ETA 2824-2",
            "--field",
            "Serial Number=A123",
            "--notes",
            "From the shop",
        ],
    );
    assert_eq!(doc["schema"], "timegrapher.watches-change/1");
    assert_eq!(doc["action"], "new");
    let id = doc["watch"]["id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 16);
    let w = show(&base, "pepsi");
    assert_eq!(w["id"], id.as_str());
    // The calibre gives its beat rate and lift angle.
    assert_eq!(w["calibre"], "ETA 2824-2");
    assert_eq!(w["bph"], 28800);
    assert!(w["lift_deg"].is_number());
    assert_eq!(w["fields"][0]["label"], "Serial Number");
    assert_eq!(w["fields"][0]["value"], "A123");
    assert_eq!(w["notes"], "From the shop");
    // What the calibre gives isn't set over it; a name not in the table,
    // a second watch of the name and a figure out of range are refused.
    for (args, why) in [
        (
            &["watches", "edit", "Pepsi", "--bph", "21600"][..],
            "calibre gives",
        ),
        (
            &["watches", "new", "X", "--calibre", "No Such 1"][..],
            "calibres --search",
        ),
        (&["watches", "new", "PEPSI"][..], "already a watch"),
        (&["watches", "new", "Y", "--lift", "120"][..], "lift angle"),
        (&["watches", "new", "Unsorted"][..], "Quick Listen"),
    ] {
        let e = refused(&base, args);
        assert!(e.contains(why), "{args:?}: {e}");
    }
    let doc = ok(
        &base,
        &[
            "watches",
            "edit",
            "Pepsi",
            "--rename",
            "Pepsi GMT",
            "--calibre",
            "",
            "--bph",
            "21600",
            "--remove-field",
            "serial number",
            "--field",
            "Job Number=7",
            "--notes",
            "",
        ],
    );
    assert_eq!(doc["watch"]["id"], id.as_str());
    let w = show(&base, "Pepsi GMT");
    assert!(w["calibre"].is_null());
    assert_eq!(w["bph"], 21600);
    assert_eq!(w["fields"].as_array().unwrap().len(), 1);
    assert_eq!(w["fields"][0]["label"], "Job Number");
    assert!(w["notes"].is_null());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn takes_are_added_moved_kept_as_analysis_and_deleted() {
    let base = scratch("takes");
    ok(&base, &["watches", "new", "Pepsi"]);
    ok(&base, &["watches", "new", "Daytona"]);
    let outside = base.join("2026-11-12 10.00 Dial Up");
    take(&outside);
    // Copied in: the original stays.
    let doc = ok(
        &base,
        &["watches", "add-take", "Pepsi", outside.to_str().unwrap()],
    );
    assert_eq!(doc["action"], "add-take");
    let added = PathBuf::from(doc["take"].as_str().unwrap());
    assert!(added.join("take_part01-001.wav").is_file() && outside.is_dir());
    assert_eq!(show(&base, "Pepsi")["takes"].as_array().unwrap().len(), 1);
    let e = refused(
        &base,
        &["watches", "add-take", "Pepsi", added.to_str().unwrap()],
    );
    assert!(e.contains("already in your watches"), "{e}");
    // A session file that doesn't read is no take to add.
    let broken = base.join("2026-11-12 09.00 Dial Up");
    take(&broken);
    std::fs::write(broken.join("session.toml"), "bph = \"fast\"\n").unwrap();
    let e = refused(
        &base,
        &["watches", "add-take", "Pepsi", broken.to_str().unwrap()],
    );
    assert!(e.contains("session file doesn't read"), "{e}");
    assert_eq!(show(&base, "Pepsi")["takes"].as_array().unwrap().len(), 1);
    // Moved to another watch.
    let doc = ok(
        &base,
        &[
            "watches",
            "move-take",
            added.to_str().unwrap(),
            "--to",
            "daytona",
        ],
    );
    let moved = PathBuf::from(doc["take"].as_str().unwrap());
    assert_eq!(doc["watch"]["name"], "Daytona");
    assert!(!added.exists() && moved.is_dir());
    // Kept as its analysis: the sound goes, everything else stays.
    let doc = ok(
        &base,
        &["watches", "keep-analysis-only", moved.to_str().unwrap()],
    );
    assert_eq!(doc["action"], "keep-analysis-only");
    assert!(!moved.join("take_part01-001.wav").exists());
    assert!(moved.join("analysis").join("analysis.json").is_file());
    assert!(moved.join("session.json").is_file());
    let toml = std::fs::read_to_string(moved.join("session.toml")).unwrap();
    assert!(toml.contains("audio = false"), "{toml}");
    let t = &show(&base, "Daytona")["takes"][0];
    assert_eq!(t["audio"], false);
    let e = refused(
        &base,
        &["watches", "keep-analysis-only", moved.to_str().unwrap()],
    );
    assert!(e.contains("already been removed"), "{e}");
    // Deleted, only with --yes; a folder outside the library never.
    let e = refused(&base, &["watches", "delete-take", moved.to_str().unwrap()]);
    assert!(e.contains("--yes"), "{e}");
    let e = refused(
        &base,
        &["watches", "delete-take", outside.to_str().unwrap(), "--yes"],
    );
    assert!(e.contains("not a take of these watches"), "{e}");
    assert!(outside.is_dir());
    ok(
        &base,
        &["watches", "delete-take", moved.to_str().unwrap(), "--yes"],
    );
    assert!(!moved.exists());
    // Moved in with --move: the original goes.
    let doc = ok(
        &base,
        &[
            "watches",
            "add-take",
            "Pepsi",
            outside.to_str().unwrap(),
            "--move",
        ],
    );
    assert!(!outside.exists() && PathBuf::from(doc["take"].as_str().unwrap()).is_dir());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn photos_are_added_and_deleted() {
    let base = scratch("photos");
    ok(&base, &["watches", "new", "Pepsi"]);
    let photo = base.join("dial.jpg");
    std::fs::write(&photo, b"not checked here").unwrap();
    let doc = ok(
        &base,
        &["watches", "add-photos", "Pepsi", photo.to_str().unwrap()],
    );
    let kept = PathBuf::from(doc["photos"][0].as_str().unwrap());
    assert!(kept.is_file());
    assert_eq!(show(&base, "Pepsi")["photos"], 1);
    let e = refused(
        &base,
        &[
            "watches",
            "add-photos",
            "Pepsi",
            base.join("notes.txt").to_str().unwrap(),
        ],
    );
    assert!(e.contains("not a photo"), "{e}");
    assert!(refused(&base, &["watches", "delete-photo", kept.to_str().unwrap()]).contains("--yes"));
    ok(
        &base,
        &["watches", "delete-photo", kept.to_str().unwrap(), "--yes"],
    );
    assert!(!kept.exists());
    let _ = std::fs::remove_dir_all(&base);
}

/// While the app has the library open (here: this test holds its lock),
/// only a take can be added; anything that rewrites a watch is refused,
/// saying so.
#[test]
fn only_a_take_is_added_while_the_app_has_the_library_open() {
    let base = scratch("app-open");
    ok(&base, &["watches", "new", "Pepsi"]);
    let app = Library::open(&base.join("lib")).unwrap();
    assert!(app.read_only.is_none(), "the lock is this test's");
    let e = refused(&base, &["watches", "edit", "Pepsi", "--notes", "x"]);
    assert!(e.contains("Close the app") && e.contains("add-take"), "{e}");
    let outside = base.join("2026-11-12 11.00 Dial Up");
    take(&outside);
    let doc = ok(
        &base,
        &["watches", "add-take", "Pepsi", outside.to_str().unwrap()],
    );
    let added = PathBuf::from(doc["take"].as_str().unwrap());
    assert_eq!(added.parent().unwrap().file_name().unwrap(), TAKES);
    assert!(added.join("session.toml").is_file());
    // Only copied: moved, the original would go with a watch the app
    // deletes meanwhile.
    let e = refused(
        &base,
        &[
            "watches",
            "add-take",
            "Pepsi",
            outside.to_str().unwrap(),
            "--move",
        ],
    );
    assert!(e.contains("leave out --move"), "{e}");
    assert!(outside.join("session.toml").is_file());
    drop(app);
    ok(&base, &["watches", "edit", "Pepsi", "--notes", "x"]);
    let _ = std::fs::remove_dir_all(&base);
}

/// A take path that isn't plain is no take: `<watch>/takes/..` is the
/// watch's own folder, and deleting or moving it is refused, changing
/// nothing (Felix on #1487).
#[test]
fn a_path_out_of_a_watchs_takes_is_never_a_take() {
    let base = scratch("dotdot");
    let w = ok(&base, &["watches", "new", "Pepsi"]);
    ok(&base, &["watches", "new", "Daytona"]);
    let folder = PathBuf::from(w["watch"]["folder"].as_str().unwrap());
    let outside = base.join("2026-11-12 12.00 Dial Up");
    take(&outside);
    ok(
        &base,
        &["watches", "add-take", "Pepsi", outside.to_str().unwrap()],
    );
    let up = folder.join(TAKES).join("..");
    let dot = folder.join(TAKES).join(".");
    for args in [
        vec!["watches", "delete-take", up.to_str().unwrap(), "--yes"],
        vec!["watches", "delete-take", dot.to_str().unwrap(), "--yes"],
        vec![
            "watches",
            "move-take",
            up.to_str().unwrap(),
            "--to",
            "Daytona",
        ],
        vec!["watches", "keep-analysis-only", up.to_str().unwrap()],
    ] {
        let e = refused(&base, &args);
        assert!(e.contains("not a take of these watches"), "{args:?}: {e}");
    }
    assert!(folder.join("watch.toml").is_file());
    assert_eq!(show(&base, "Pepsi")["takes"].as_array().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&base);
}

/// The take `doc` says a change moved, and what its `session.toml` says.
fn moved(doc: &Value) -> (PathBuf, timegrapher_core::session::Manifest) {
    let at = PathBuf::from(doc["take"].as_str().unwrap());
    let text = std::fs::read_to_string(at.join("session.toml")).unwrap();
    (at, toml::from_str(&text).unwrap())
}

/// `move-take` gives the take its new watch's settings, as the app's
/// move does (#2453, the app's `a_moved_take_takes_its_new_watchs_settings`):
/// the calibre's lift angle and beat rate where it gives them, the old
/// calibre's wheels going with it and the file's comments kept; then a
/// watch's own lift and rate, the calibre staying. The JSON says whether
/// the take is read with anything new.
#[test]
fn a_moved_take_takes_its_new_watchs_settings_as_the_app_does() {
    use timegrapher_core::calibre_store::{Catalog, UserCalibres};
    let base = scratch("settings");
    ok(&base, &["watches", "new", "Pepsi"]);
    ok(
        &base,
        &["watches", "new", "Explorer", "--calibre", "ETA 2824-2"],
    );
    ok(
        &base,
        &["watches", "new", "Own", "--bph", "21600", "--lift", "49"],
    );
    let outside = base.join("2026-11-12 10.00 Dial Up");
    take(&outside);
    std::fs::write(
        outside.join("session.toml"),
        "# kept as written\nwatch = \"Pepsi\"\ncalibre = \"Old 1\"\nbph = 18000\nlift = 50\n\
         lift_known = false\n\n[wheels]\n\"Escape wheel\" = 6.0\n\n\
         [[recording]]\nfile = \".\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let doc = ok(
        &base,
        &["watches", "add-take", "Pepsi", outside.to_str().unwrap()],
    );
    let taken = doc["take"].as_str().unwrap().to_string();
    let doc = ok(&base, &["watches", "move-take", &taken, "--to", "Explorer"]);
    assert_eq!(doc["settings_changed"], true);
    assert!(doc["left"].is_null());
    let (at, m) = moved(&doc);
    let user = UserCalibres::default();
    let eta = Catalog::new(&user).find("ETA 2824-2").unwrap();
    assert_eq!(m.watch.as_deref(), Some("Explorer"));
    assert_eq!(m.calibre.as_deref(), Some("ETA 2824-2"));
    assert_eq!(
        (m.lift, m.lift_known, m.bph),
        (eta.lift_angle_deg(), Some(true), eta.bph())
    );
    assert!(m.wheels.is_empty(), "the old calibre's wheels go with it");
    let text = std::fs::read_to_string(at.join("session.toml")).unwrap();
    assert!(text.contains("# kept as written"), "{text}");
    // Into the watch it is in: nothing new to read it with.
    let doc = ok(
        &base,
        &[
            "watches",
            "move-take",
            at.to_str().unwrap(),
            "--to",
            "Explorer",
        ],
    );
    assert_eq!(doc["settings_changed"], false);
    // Into a watch of its own lift and beat rate and no calibre: those
    // two, and the calibre stays.
    let doc = ok(
        &base,
        &["watches", "move-take", at.to_str().unwrap(), "--to", "Own"],
    );
    assert_eq!(doc["settings_changed"], true);
    let (_, m) = moved(&doc);
    assert_eq!(m.watch.as_deref(), Some("Own"));
    assert_eq!(m.calibre.as_deref(), Some("ETA 2824-2"));
    assert_eq!((m.lift, m.bph), (Some(49.0), Some(21600)));
    // Other changes say nothing of settings.
    let doc = ok(&base, &["watches", "new", "Daytona"]);
    assert!(doc["settings_changed"].is_null());
    let _ = std::fs::remove_dir_all(&base);
}

/// A take recorded on Auto, moved by `move-take` into a watch that sets
/// a beat rate, is read at that rate from then on, as the app's move does
/// (#2453, the app's `a_take_recorded_on_auto_moved_into_a_watch_takes_its_rate`),
/// even where `session.toml` already gave it. A take recorded at a set
/// rate, or whose `session.json` is corrupt, keeps that file byte for
/// byte, and the move says nothing was left (#2450).
#[test]
fn a_take_recorded_on_auto_moved_by_the_command_line_takes_its_rate() {
    use timegrapher_core::set_rate::recorded_on_auto;
    let base = scratch("auto");
    ok(&base, &["watches", "new", "Pepsi"]);
    ok(&base, &["watches", "new", "Own", "--bph", "17280"]);
    for (name, json, auto) in [
        (
            "auto",
            r#"{"watch": "Pepsi", "lift_deg": 52.0, "bph": null}"#,
            true,
        ),
        (
            "set",
            r#"{"watch": "Pepsi", "lift_deg": 52.0, "bph": 17280}"#,
            false,
        ),
        ("corrupt", r#"{"watch": "Pepsi", "bph": nu"#, false),
    ] {
        let outside = base.join(format!("2026-11-26 10.00 {name}"));
        take(&outside);
        std::fs::write(
            outside.join("session.toml"),
            "watch = \"Pepsi\"\nbph = 17280\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
        )
        .unwrap();
        std::fs::write(outside.join("session.json"), json).unwrap();
        let doc = ok(
            &base,
            &["watches", "add-take", "Pepsi", outside.to_str().unwrap()],
        );
        let taken = PathBuf::from(doc["take"].as_str().unwrap());
        assert_eq!(recorded_on_auto(&taken), auto, "{name}");
        let doc = ok(
            &base,
            &[
                "watches",
                "move-take",
                taken.to_str().unwrap(),
                "--to",
                "Own",
            ],
        );
        assert!(doc["left"].is_null(), "{name}: {doc}");
        let (at, m) = moved(&doc);
        assert_eq!(m.bph, Some(17280), "{name}");
        assert!(!recorded_on_auto(&at), "{name}");
        // Only the take recorded on Auto is read with anything new.
        assert_eq!(doc["settings_changed"], auto, "{name}");
        let after = std::fs::read_to_string(at.join("session.json")).unwrap();
        assert_eq!(after == json, !auto, "{name}: {after}");
    }
    let _ = std::fs::remove_dir_all(&base);
}
