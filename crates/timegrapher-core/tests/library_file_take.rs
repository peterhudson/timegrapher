//! `library::file_take_under`, the one rule for what a take moved into a
//! watch is read with (#1283), which the app's move and `timegrapher
//! watches move-take` both call (#2453). Only a take recorded on Auto has
//! its `session.json` marked with the watch's rate (#2450).
use std::path::{Path, PathBuf};
use timegrapher_core::calibre_store::UserCalibres;
use timegrapher_core::library::{file_take_under, Watch};
use timegrapher_core::set_rate::{recorded_on_auto, WATCH_RATE_KEY};
use timegrapher_core::take::{MANIFEST, RECORD};

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tg-file-take-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A take folder with `session.toml` and `session.json` as given.
fn take(dir: &Path, toml: &str, json: &str) {
    std::fs::write(dir.join(MANIFEST), toml).unwrap();
    std::fs::write(dir.join(RECORD), json).unwrap();
}

fn own(bph: u32) -> Watch {
    let mut w = Watch::new("Own");
    w.bph = Some(bph);
    w
}

/// A take recorded at a set rate, or one whose `session.json` can't be
/// read, moved into a watch that sets a rate: its `session.toml` takes
/// the watch's rate, its `session.json` keeps every byte, and the move
/// gives no error (#2450).
#[test]
fn a_take_not_recorded_on_auto_keeps_its_session_json() {
    let dir = scratch("set");
    let user = UserCalibres::default();
    for json in [
        // Recorded at a set rate, written as the recorder writes it.
        "{\n  \"watch\": \"Pepsi\",\n  \"lift_deg\": 52.0,\n  \"bph\": 21600\n}\n",
        // Corrupt, and a JSON file that is no object.
        "{\"watch\": \"Pepsi\", \"bph\": nu",
        "[1, 2]",
    ] {
        take(&dir, "watch = \"Pepsi\"\nbph = 21600\n", json);
        assert!(!recorded_on_auto(&dir), "{json}");
        assert_eq!(
            file_take_under(&dir, &own(28_800), &user),
            Ok(true),
            "{json}"
        );
        assert_eq!(std::fs::read_to_string(dir.join(RECORD)).unwrap(), json);
        let toml = std::fs::read_to_string(dir.join(MANIFEST)).unwrap();
        assert!(
            toml.contains("bph = 28800") && toml.contains("watch = \"Own\""),
            "{toml}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A take recorded on Auto, moved into a watch that sets a rate: marked,
/// so the watch's rate stands, and read again even where `session.toml`
/// already gave that rate (Peter on #2357). Moved again, nothing changes.
#[test]
fn a_take_recorded_on_auto_is_marked_with_its_watchs_rate() {
    let dir = scratch("auto");
    let user = UserCalibres::default();
    take(
        &dir,
        "watch = \"Own\"\nbph = 17280\n",
        r#"{"watch": "Pepsi", "lift_deg": 52.0, "bph": null}"#,
    );
    assert!(recorded_on_auto(&dir));
    assert_eq!(file_take_under(&dir, &own(17_280), &user), Ok(true));
    assert!(!recorded_on_auto(&dir));
    let json = std::fs::read_to_string(dir.join(RECORD)).unwrap();
    assert!(json.contains(WATCH_RATE_KEY), "{json}");
    assert_eq!(file_take_under(&dir, &own(17_280), &user), Ok(false));
    // A watch that sets no rate leaves it on Auto.
    take(
        &dir,
        "bph = 17280\n",
        r#"{"watch": "Pepsi", "lift_deg": 52.0, "bph": null}"#,
    );
    assert_eq!(file_take_under(&dir, &Watch::new("Own"), &user), Ok(false));
    assert!(recorded_on_auto(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A take kept as its analysis alone, recorded on Auto, moved into a
/// watch that sets a rate: it keeps what it was measured with, its
/// `session.json` byte for byte, and is not read again (Vernon 101).
#[test]
fn a_kept_take_recorded_on_auto_keeps_its_session_json() {
    use timegrapher_core::analysis_take::{DIR, HEADER};
    let dir = scratch("kept");
    let json = r#"{"watch": "Pepsi", "lift_deg": 52.0, "bph": null}"#;
    take(&dir, "watch = \"Pepsi\"\naudio = false\n", json);
    std::fs::create_dir_all(dir.join(DIR)).unwrap();
    std::fs::write(dir.join(DIR).join(HEADER), "{}").unwrap();
    assert!(recorded_on_auto(&dir));
    let user = UserCalibres::default();
    assert_eq!(file_take_under(&dir, &own(28_800), &user), Ok(false));
    assert_eq!(std::fs::read_to_string(dir.join(RECORD)).unwrap(), json);
    let toml = std::fs::read_to_string(dir.join(MANIFEST)).unwrap();
    assert!(
        toml.contains("watch = \"Own\"") && !toml.contains("bph"),
        "{toml}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A take moved into a watch whose calibre is one of the user's own,
/// with wheels: its `session.toml` gets that calibre's `[wheels]`, in
/// place of the old calibre's (Vernon 101).
#[test]
fn a_user_calibres_wheels_go_with_the_take() {
    let dir = scratch("wheels");
    let mut user = UserCalibres::default();
    user.put_custom(
        serde_json::from_value(serde_json::json!({
            "name": "Own 1",
            "bph": 21600,
            "wheels": [
                {"name": "Escape wheel", "period_s": 7.5},
                {"name": "Third wheel", "period_s": 450.0}
            ]
        }))
        .unwrap(),
    );
    take(
        &dir,
        "calibre = \"Old 1\"\n\n[wheels]\n\"Fourth wheel\" = 60.0\n",
        "{}",
    );
    let mut w = Watch::new("Own");
    w.calibre = Some("Own 1".into());
    assert_eq!(file_take_under(&dir, &w, &user), Ok(true));
    let m: timegrapher_core::session::Manifest =
        toml::from_str(&std::fs::read_to_string(dir.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(m.calibre.as_deref(), Some("Own 1"));
    assert_eq!(m.bph, Some(21600));
    let wheels: Vec<_> = m.wheels.into_iter().collect();
    assert_eq!(
        wheels,
        [
            ("Escape wheel".to_string(), 7.5),
            ("Third wheel".to_string(), 450.0)
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}
