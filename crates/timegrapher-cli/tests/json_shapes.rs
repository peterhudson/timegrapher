//! The shape of each command's `--json` document: every field's path, as
//! an agent reading it relies on. A field renamed or removed fails here;
//! a field added does too, until its path is added to the command's file
//! in `tests/json_shapes/` (run with `UPDATE_JSON_SHAPES=1` to write them).
//! docs/reference/agent-interface.md promises additive JSON, so a path leaving a
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

/// The watch of [`watch_wav`] with its rate swinging 40 s/d peak to
/// peak every 12 s, a cycle the session names, so that its cycles and
/// findings keep their fields.
fn cycling_watch_wav(path: &Path, seconds: f64) {
    let audio = generate(
        &SynthConfig {
            duration_s: seconds,
            rate_s_per_day: 6.0,
            beat_error_ms: 0.4,
            ..Default::default()
        },
        |_| 275.0,
        |t| 20.0 * (std::f64::consts::TAU * t / 12.0).sin(),
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

/// A session file `analyze` cannot read beside the file, or one with an
/// entry `session` would refuse, leaves the file's own reading standing:
/// `take` gives the session file and the error, and the text says so on
/// stderr.
#[test]
fn analyze_reads_its_file_beside_a_session_file_it_cannot_read() {
    let dir = scratch("broken-take");
    let wav = dir.join("du.wav");
    watch_wav(&wav, 20.0);
    let w = wav.to_str().unwrap();
    let config = dir.join("config");
    for text in [
        "this is [ not a session file",
        "[[recording]]\nfile = \"du.wav\"\nposition = \"sideways\"\n",
    ] {
        std::fs::write(dir.join("session.toml"), text).unwrap();
        let j = json(&config, &["analyze", w, "--json"]);
        assert!(j["beats_found"].as_u64().unwrap() > 0, "{j}");
        let take = &j["take"];
        assert!(
            take["session_file"]
                .as_str()
                .unwrap()
                .ends_with("session.toml"),
            "{j}"
        );
        assert!(!take["error"].as_str().unwrap().is_empty(), "{j}");
        assert!(take.get("readings").is_none(), "{j}");
        let out = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
            .args(["analyze", w])
            .env("TIMEGRAPHER_CONFIG_DIR", &config)
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{err}");
        assert!(
            err.contains("could not be read for this file's take"),
            "{err}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_keeps_its_fields() {
    let dir = scratch("session");
    cycling_watch_wav(&dir.join("du.wav"), 60.0);
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
    // Its text leads with that reading, with its figures and their ±, as
    // the MCP lead does (#826); the whole file read alone follows.
    let out = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args(["analyze", cu.to_str().unwrap()])
        .env("TIMEGRAPHER_CONFIG_DIR", dir.join("config"))
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let at = |s: &str| {
        text.find(s)
            .unwrap_or_else(|| panic!("no {s:?} in\n{text}"))
    };
    let reading = at("Crown Up (CU, 3H) from ");
    assert!(
        at("Take ") < reading && reading < at("Whole file ") && at("Whole file ") < at("Duration ")
    );
    let line = text[reading..].lines().next().unwrap();
    assert!(
        line.contains("rate ") && line.contains(" s/d, amplitude ") && line.contains(" ms"),
        "{line}"
    );
    // Each figure carries its ± from the reading's standard error. A
    // minute's file holds 6 of the 30 rate values the rate's ± takes its
    // scatter from, so the rate has none (#1470).
    assert_eq!(line.matches(" ± ").count(), 2, "{line}");
    let m = &take["readings"][0]["measurement"];
    assert!(m["confidence"]["rate"]["se"].is_null(), "{m}");
    let pm = |k: &str, d: usize| {
        timegrapher_core::library::pm_text(m["confidence"][k]["se"].as_f64().unwrap(), d).unwrap()
    };
    for (t, unit) in [(pm("amplitude", 0), "°"), (pm("beat_error", 2), " ms")] {
        assert!(line.contains(&format!(" ± {t}{unit}")), "{t}{unit}: {line}");
    }
    // The whole file's rate, amplitude and beat error carry as many
    // decimals as their ± (both at the figure's place, #1470).
    let decimals = |t: &str| t.split_once('.').map_or(0, |(_, d)| d.len());
    for head in ["Rate         ", "Amplitude    ", "Beat error   "] {
        let line = text[at("Whole file ")..]
            .lines()
            .find(|l| l.starts_with(head))
            .unwrap_or_else(|| panic!("no {head:?} in\n{text}"));
        let words: Vec<&str> = line[head.len()..].split_whitespace().collect();
        // The rate's ± waits for 30 values, as above.
        if head.starts_with("Rate") {
            assert_ne!(words[1], "±", "{line}");
            continue;
        }
        assert_eq!(words[1], "±", "{line}");
        assert_eq!(decimals(words[0]), decimals(words[2]), "{line}");
        // Tick and Tock, and the drop's beat error, take the same decimals
        // as the figure beside them.
        if let Some((_, rest)) = line.split_once(" (Tick ") {
            let (tick, rest) = rest.split_once(", Tock ").unwrap();
            let tock = rest.split_once(';').unwrap().0;
            assert_eq!(decimals(tick), decimals(words[0]), "{line}");
            assert_eq!(decimals(tock), decimals(words[0]), "{line}");
        }
        if let Some((_, rest)) = line.split_once("(from the unlock; ") {
            let drop = rest.split_once(" ms from the drop").unwrap().0;
            assert_eq!(decimals(drop), decimals(words[0]), "{line}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A take listed as its own folder ("."), as the app writes one, names
/// its only sound file: `analyze` on that file says what `session` reads.
#[test]
fn analyze_reads_a_one_file_take_listed_as_its_folder() {
    let dir = scratch("dot-take");
    let wav = dir.join("audio-001.wav");
    watch_wav(&wav, 60.0);
    std::fs::write(
        dir.join("session.toml"),
        "bph = 28800\nlift = 52\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
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
    let analyze = json(
        &dir.join("config"),
        &["analyze", wav.to_str().unwrap(), "--json"],
    );
    let readings = analyze["take"]["readings"]
        .as_array()
        .expect("a take block");
    assert_eq!(readings.len(), 1);
    assert_eq!(
        readings[0]["measurement"],
        session["readings"][0]["measurement"]
    );
    // The beat shape says where it was measured: from the settled start.
    assert_eq!(readings[0]["shape"], session["readings"][0]["shape"]);
    assert_eq!(
        session["readings"][0]["shape"]["from_s"],
        session["readings"][0]["measurement"]["start_s"]
    );
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
fn train_keeps_its_fields() {
    let config = scratch("train");
    // Every field: a period from the beat rate and one carried in from a
    // given period, a blank that names its missing count, and meshes.
    check(
        "train",
        &json(
            &config,
            &[
                "train",
                "--bph",
                "21600",
                "--escape",
                "15/10",
                "--fourth",
                "120",
                "--third",
                "60/10",
                "--centre",
                "80",
                "--period",
                "centre=3600",
                "--json",
            ],
        ),
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
    // Let go, as the app does when it closes, so a change can be made.
    drop(lib);
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
    check(
        "watches_change",
        &json(
            &config,
            &["watches", "new", "Daytona", "--library", r, "--json"],
        ),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn excerpt_keeps_its_fields_and_cuts_across_segments() {
    let dir = scratch("excerpt");
    let take = dir.join("take");
    std::fs::create_dir_all(&take).unwrap();
    watch_wav(&take.join("audio-001.wav"), 20.0);
    watch_wav(&take.join("audio-002.wav"), 20.0);
    let out = dir.join("cut.flac");
    let doc = json(
        &dir,
        &[
            "excerpt",
            take.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--from",
            "15",
            "--to",
            "30",
            "--json",
        ],
    );
    check("excerpt", &doc);
    assert_eq!(doc["schema"], "timegrapher.excerpt/1");
    assert_eq!(doc["input"]["files"].as_array().unwrap().len(), 2);
    assert!((doc["duration_s"].as_f64().unwrap() - 15.0).abs() < 1e-3);
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(doc["bytes"].as_u64().unwrap(), bytes.len() as u64);
    assert_eq!(
        doc["sha256"],
        timegrapher_core::archive::sha256_hex(&bytes).as_str()
    );
    // The samples are the recording's own, across the join of the segments.
    let a = timegrapher_core::excerpt::read(&take.join("audio-001.wav")).unwrap();
    let b = timegrapher_core::excerpt::read(&take.join("audio-002.wav")).unwrap();
    let cut = timegrapher_core::excerpt::read(&out).unwrap();
    let rate = a.sample_rate as usize;
    let mut joined = a.samples.clone();
    joined.extend_from_slice(&b.samples);
    assert_eq!(cut.samples, joined[15 * rate..30 * rate]);
    // A second run never writes over the file.
    let again = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args([
            "excerpt",
            take.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .env("TIMEGRAPHER_CONFIG_DIR", &dir)
        .output()
        .unwrap();
    assert!(!again.status.success());
    assert_eq!(std::fs::read(&out).unwrap(), bytes);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A sound file its folder's session file lists is read with the take's
/// lift angle and beat rate by `analyze`, `profile` and `shape`, as
/// `series` and the app read it; a file beside it the session file
/// doesn't list is read at the default 52° (#1573).
#[test]
fn analyze_profile_and_shape_read_a_file_in_a_take_with_its_settings() {
    let dir = scratch("file-in-take");
    let config = dir.join("config");
    let take = dir.join("du.wav");
    let other = dir.join("other.wav");
    watch_wav(&take, 20.0);
    watch_wav(&other, 20.0);
    std::fs::write(
        dir.join("session.toml"),
        "bph = 28800\nlift = 47\n\n[[recording]]\nfile = \"du.wav\"\nposition = \"DU\"\n",
    )
    .unwrap();
    for command in ["analyze", "profile", "shape"] {
        let settings = |file: &Path| {
            let j = json(&config, &[command, file.to_str().unwrap(), "--json"]);
            let s = &j["input"]["settings"];
            (s["lift_deg"].as_f64(), s["bph"].as_u64())
        };
        assert_eq!(settings(&take), (Some(47.0), Some(28800)), "{command}");
        assert_eq!(settings(&other).0, Some(52.0), "{command}");
    }
}
