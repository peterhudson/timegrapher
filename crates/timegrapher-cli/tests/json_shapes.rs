//! The shape of each command's `--json` document: every field's path, as
//! an agent reading it relies on. A field renamed or removed fails here;
//! a field added does too, until its path is added to the command's file
//! in `tests/json_shapes/` (run with `UPDATE_JSON_SHAPES=1` to write them).
//! docs/agent-interface.md promises additive JSON, so a path leaving a
//! file is a change for that page and the schema version, not a refresh.
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use timegrapher_core::synth::{generate, SynthConfig};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tg-json-shapes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The command's JSON document on stdout, run with a settings folder of
/// its own.
fn json(config: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args(args)
        .env("TIMEGRAPHER_CONFIG_DIR", config)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{args:?}: {e}\n{}", String::from_utf8_lossy(&out.stdout)))
}

/// Every field's path in `v`: `a.b`, with `[]` for an array's elements
/// (the fields of all of them).
fn paths(v: &Value, at: &str, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                let p = if at.is_empty() {
                    k.clone()
                } else {
                    format!("{at}.{k}")
                };
                out.insert(p.clone());
                paths(x, &p, out);
            }
        }
        Value::Array(a) => {
            for x in a {
                paths(x, &format!("{at}[]"), out);
            }
        }
        _ => {}
    }
}

/// `doc`'s paths against the command's file, or written to it with
/// `UPDATE_JSON_SHAPES=1`.
fn check(command: &str, doc: &Value) {
    let mut got = BTreeSet::new();
    paths(doc, "", &mut got);
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/json_shapes")
        .join(format!("{command}.txt"));
    if std::env::var_os("UPDATE_JSON_SHAPES").is_some() {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let text: String = got.iter().map(|p| format!("{p}\n")).collect();
        std::fs::write(&file, text).unwrap();
        return;
    }
    let text = std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("{}: {e} (run with UPDATE_JSON_SHAPES=1)", file.display()));
    let want: BTreeSet<String> = text.lines().map(str::to_string).collect();
    let gone: Vec<&String> = want.difference(&got).collect();
    let new: Vec<&String> = got.difference(&want).collect();
    assert!(
        gone.is_empty() && new.is_empty(),
        "`{command} --json` changed shape.\nGone (renamed or removed; a breaking change): {gone:#?}\nNew (add with UPDATE_JSON_SHAPES=1): {new:#?}"
    );
}

/// A minute of a watch at 28,800 bph, 6 s/d fast, with a beat error.
fn watch_wav(path: &Path, seconds: f64) {
    let audio = generate(
        &SynthConfig {
            duration_s: seconds,
            rate_s_per_day: 6.0,
            beat_error_ms: 0.4,
            ..Default::default()
        },
        |_| 275.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(path, &audio).unwrap();
}

#[test]
fn analyze_shape_and_profile_keep_their_fields() {
    let dir = scratch("analyze");
    let wav = dir.join("watch.wav");
    watch_wav(&wav, 60.0);
    let w = wav.to_str().unwrap();
    let config = dir.join("config");
    check("analyze", &json(&config, &["analyze", w, "--json"]));
    check("shape", &json(&config, &["shape", w, "--json"]));
    check(
        "profile",
        &json(
            &config,
            &["profile", w, "--at", "30", "--span", "20", "--json"],
        ),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_keeps_its_fields() {
    let dir = scratch("session");
    watch_wav(&dir.join("du.wav"), 60.0);
    watch_wav(&dir.join("cu.wav"), 60.0);
    std::fs::write(
        dir.join("session.toml"),
        "bph = 28800\nlift = 52\n\n[[recording]]\nfile = \"du.wav\"\nposition = \"DU\"\n\n\
         [[recording]]\nfile = \"cu.wav\"\nposition = \"CU\"\n\n\
         [[recording]]\nfile = \"cu.wav\"\nposition = \"CD\"\nsettle_s = 100.0\n",
    )
    .unwrap();
    let out = dir.join("out");
    let session = json(
        &dir.join("config"),
        &[
            "session",
            dir.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--json",
        ],
    );
    check("session", &session);
    // `analyze` on a file the session file lists says what `session` reads
    // of it: here its CU entry, the same to the bit, and its CD entry left
    // out.
    let cu = dir.join("cu.wav");
    let analyze = json(
        &dir.join("config"),
        &["analyze", cu.to_str().unwrap(), "--json"],
    );
    check("analyze_take", &analyze);
    let take = &analyze["take"];
    assert_eq!(take["readings"].as_array().unwrap().len(), 1);
    assert_eq!(
        take["readings"][0]["measurement"],
        session["readings"][1]["measurement"]
    );
    assert_eq!(take["left_out"], session["left_out"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn calibres_keep_their_fields() {
    let config = scratch("calibres");
    check("calibres", &json(&config, &["calibres", "--json"]));
    check(
        "calibres_named",
        &json(&config, &["calibres", "ETA 2824-2", "--json"]),
    );
    let _ = std::fs::remove_dir_all(&config);
}

#[test]
fn long_and_series_keep_their_fields() {
    let dir = scratch("long");
    let wav = dir.join("watch.wav");
    watch_wav(&wav, 180.0);
    let w = wav.to_str().unwrap();
    let config = dir.join("config");
    let out = dir.join("long");
    check(
        "long",
        &json(
            &config,
            &["long", w, "--out", out.to_str().unwrap(), "--json"],
        ),
    );
    check("series", &json(&config, &["series", w, "--json"]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn watches_keep_their_fields() {
    use timegrapher_core::library::{Library, Watch};
    let dir = scratch("watches");
    let root = dir.join("library");
    let mut lib = Library::create(&root).unwrap();
    let mut w = Watch::new("Pepsi");
    w.calibre = Some("Rolex 3135".into());
    w.bph = Some(28800);
    let folder = lib.add(&w).unwrap();
    // A take as the app saves one: its session file, its sound and the
    // readings at its end.
    let take = folder
        .join(timegrapher_core::library::TAKES)
        .join("2026-10-01 09.00 Dial Up");
    std::fs::create_dir_all(&take).unwrap();
    std::fs::write(
        take.join("session.toml"),
        "bph = 28800\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
    )
    .unwrap();
    watch_wav(&take.join("take_part01-001.wav"), 40.0);
    std::fs::write(
        take.join("session.json"),
        r#"{"started_utc":"2026-10-01T09:00:00Z","duration_s":40.0,
            "result":{"rate_s_per_day":6.0,"amplitude_deg":275.0,"beat_error_unlock_ms":0.4}}"#,
    )
    .unwrap();
    let config = dir.join("config");
    let r = root.to_str().unwrap();
    check(
        "watches_list",
        &json(&config, &["watches", "list", "--library", r, "--json"]),
    );
    check(
        "watches_show",
        &json(
            &config,
            &["watches", "show", "Pepsi", "--library", r, "--json"],
        ),
    );
    let _ = std::fs::remove_dir_all(&dir);
}
