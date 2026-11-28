//! Machine-readable output: every `--json` document has the same
//! envelope, so an agent can tell what it is reading and where it came from.
//!
//! The `schema` field names the kind of document and its version
//! (`timegrapher.analyze/3`). Within a version, fields are only ever added;
//! a change that would break a reader bumps the version. See
//! `docs/reference/agent-interface.md`.

use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Version of the document kinds that have not changed since the first.
pub const SCHEMA_VERSION: u32 = 1;

/// Current version of a document kind. `record/2` keeps every take and
/// drops the verdict; `series/2` dropped the one-word verdict, the
/// findings' severity and advice, and `moves_with`, and `series/3` the
/// isochronism's quadratic (`curve_centre_deg`, `curve`, `curve_explained`,
/// `turn_deg`), and `series/4` left the wind not given unless set
/// (`wind_h` null, never full wind's); `session/2` dropped
/// the tolerances, limits, verdicts and the findings' severity and advice,
/// and `session/3` made a state of wind not given its own (`wind_h` null)
/// instead of full wind's;
/// `analyze/2`, `doctor/2` and `regress-holdout/2` dropped their grades,
/// verdicts, named faults and advice, and `analyze/3` is core's
/// ([`timegrapher_core::analyze_doc::VERSION`]).
pub fn schema_version(kind: &str) -> u32 {
    match kind {
        "record" => 2,
        "series" => 4,
        "session" => 3,
        "analyze" => timegrapher_core::analyze_doc::VERSION,
        "doctor" => 2,
        "regress-holdout" => 2,
        _ => SCHEMA_VERSION,
    }
}

#[derive(Serialize)]
pub struct Envelope<'a, T: Serialize> {
    pub schema: String,
    pub software: Value,
    pub input: Value,
    #[serde(flatten)]
    pub body: &'a T,
}

/// Name, version, build and platform. `git_hash` is the short commit hash
/// ("unknown" in a build without git), `git_dirty` whether the work tree had
/// uncommitted changes (null when unknown), `build_date` the UTC date of the
/// build. These tell builds apart; they are not readings, and anything that
/// compares documents should leave this block out.
pub fn software() -> Value {
    let dirty = match env!("TIMEGRAPHER_GIT_DIRTY") {
        "true" => json!(true),
        "false" => json!(false),
        _ => Value::Null,
    };
    json!({
        "name": "timegrapher",
        "version": env!("CARGO_PKG_VERSION"),
        "git_hash": env!("TIMEGRAPHER_GIT_HASH"),
        "git_dirty": dirty,
        "build_date": env!("TIMEGRAPHER_BUILD_DATE"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
}

/// The JSON text of a document of `kind`.
pub fn to_json<T: Serialize>(kind: &str, input: Value, body: &T) -> Result<String, String> {
    serde_json::to_string_pretty(&Envelope {
        schema: format!("timegrapher.{kind}/{}", schema_version(kind)),
        software: software(),
        input,
        body,
    })
    .map_err(|e| e.to_string())
}

pub fn print<T: Serialize>(kind: &str, input: Value, body: &T) -> Result<(), String> {
    println!("{}", to_json(kind, input, body)?);
    Ok(())
}

/// Where the provenance of a recording is kept: `take.wav` -> `take.wav.json`.
pub fn sidecar_path(file: &Path) -> PathBuf {
    timegrapher_core::clockstore::sidecar_path(file)
}

/// Describe the input files and the settings used. A recording made by
/// `timegrapher doctor --save` (or anything else that writes the same
/// sidecar) brings its device and mixer settings along.
pub fn input(files: &[PathBuf], settings: Value) -> Value {
    let files: Vec<Value> = files
        .iter()
        .map(|f| {
            let mut v = json!({ "path": f.display().to_string() });
            if let Some(info) = std::fs::read_to_string(sidecar_path(f))
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            {
                v["recording"] = info.get("recording").cloned().unwrap_or(info);
            }
            v
        })
        .collect();
    json!({ "files": files, "settings": settings })
}

/// The file a path names once written: the canonical path of an existing
/// file, or of its folder joined to its name for one not yet written.
fn resolved(p: &Path) -> PathBuf {
    if let Ok(c) = p.canonicalize() {
        return c;
    }
    let parent = match p.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    match (parent.canonicalize(), p.file_name()) {
        (Ok(d), Some(name)) => d.join(name),
        _ => p.to_path_buf(),
    }
}

/// Whether `a` and `b` name one file: the same resolved path, or one
/// folder and names that differ only in case, as on a disk that ignores
/// case (macOS, Windows), where they are one file.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || (a.parent() == b.parent()
            && match (a.file_name(), b.file_name()) {
                (Some(x), Some(y)) => x.eq_ignore_ascii_case(y),
                _ => false,
            })
}

/// The files a take keeps beside its sound, named by core: an output may
/// name none of them, there or not yet (#2372).
fn take_files() -> [&'static str; 10] {
    [
        timegrapher_core::take::MANIFEST,
        timegrapher_core::take::RECORD,
        timegrapher_core::take::CLOCK_LOG_NAMES[0],
        timegrapher_core::take::CLOCK_LOG_NAMES[1],
        timegrapher_core::take::CLOCK_LOG_NAMES[2],
        timegrapher_core::library::POSITIONS_TEST_FILE,
        timegrapher_core::library::POISE_TEST_FILE,
        timegrapher_core::test_marks::FILE,
        timegrapher_core::power_reserve::FILE,
        timegrapher_core::recorder::STREAMED_TO,
    ]
}

/// The folders a take keeps its analysis and reports in, named by core:
/// an output may name no file inside them.
fn take_folders() -> [&'static str; 3] {
    [
        timegrapher_core::analysis_take::DIR,
        timegrapher_core::checkpoint::DIR,
        timegrapher_core::take::REPORT_DIR,
    ]
}

/// What of the take read from `read` (resolved, in the folder `dir`, for
/// the recording named `input`) the resolved output `r` would write over,
/// if anything.
fn take_file(r: &Path, input: &Path, read: &Path, dir: &Path) -> Option<String> {
    if same_file(r, read) {
        return Some("the recording being read".into());
    }
    if [input, read]
        .iter()
        .any(|f| same_file(r, &resolved(&sidecar_path(f))))
    {
        return Some("the recording's sidecar".into());
    }
    if let Some(name) = take_files()
        .into_iter()
        .find(|n| same_file(r, &dir.join(n)))
    {
        return Some(format!("the take's {name}"));
    }
    if let Some(folder) = take_folders().into_iter().find(|f| {
        let f = resolved(&dir.join(f));
        r.starts_with(&f) || r.parent().is_some_and(|p| same_file(p, &f))
    }) {
        return Some(format!("in the take's {folder} folder"));
    }
    let sound = |f: &Path| {
        let f = resolved(f);
        same_file(r, &f) || same_file(r, &resolved(&sidecar_path(&f)))
    };
    let beside = r.parent() == Some(dir) && timegrapher_core::take::is_sound(r);
    let listed = timegrapher_core::take_path::recordings_in(dir)
        .unwrap_or_default()
        .iter()
        .any(|f| sound(f));
    (beside || listed).then(|| "a recording of the take being read, or its sidecar".into())
}

/// Refuse output files that are the input being read, a file of the take
/// it is read from, or each other, before anything is read or written:
/// `analyze x.wav --beats x.wav` would put a CSV where the recording was
/// (#1876), and `--beats TAKE/session.toml` where the take's calibre, lift
/// and positions were (#2372). The recording read is the FLAC when its WAV
/// was kept as one (core's `audio::kept_path`). The take's files are every
/// sound file in the folder and each recording's sidecar, [`take_files`]
/// and any file in [`take_folders`], refused whether or not they are there
/// yet: a CSV named as one would be read as the take's.
pub fn files_apart(input: &Path, outputs: &[(&str, &Path)]) -> Result<(), String> {
    let named = resolved(input);
    let read = resolved(&timegrapher_core::audio::kept_path(input).unwrap_or(input.to_path_buf()));
    let dir = read.parent().map(Path::to_path_buf);
    let mut seen: Vec<(&str, PathBuf)> = Vec::new();
    for &(flag, p) in outputs {
        let r = resolved(p);
        if let Some(what) = dir.as_deref().and_then(|d| take_file(&r, &named, &read, d)) {
            return Err(format!(
                "{flag} {} is {what}; name another file",
                p.display()
            ));
        }
        if let Some((other, _)) = seen.iter().find(|(_, s)| same_file(s, &r)) {
            return Err(format!(
                "{flag} and {other} both name {}; give each its own file",
                p.display()
            ));
        }
        seen.push((flag, r));
    }
    Ok(())
}

#[cfg(test)]
mod files_apart_tests {
    use super::files_apart;
    use std::path::Path;

    #[test]
    fn an_output_may_not_be_the_input_or_another_output() {
        let dir = std::env::temp_dir().join(format!("tg-apart-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("x.wav");
        std::fs::write(&wav, b"RIFF").unwrap();
        let beats = dir.join("beats.csv");
        let same = dir.join(".").join("x.wav");
        assert!(files_apart(&wav, &[("--beats", &beats)]).is_ok());
        let e = files_apart(&wav, &[("--beats", &same)]).unwrap_err();
        assert!(
            e.contains("--beats") && e.contains("recording being read"),
            "{e}"
        );
        let e = files_apart(&wav, &[("--beats", &beats), ("--windows", &beats)]).unwrap_err();
        assert!(e.contains("--windows and --beats"), "{e}");
        assert!(files_apart(Path::new("x.wav"), &[("--trend", Path::new("t.csv"))]).is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A take's own files are refused as outputs, there or not yet, in any
    /// case, and so is the FLAC read for a WAV kept as one (#2372).
    #[test]
    fn an_output_may_not_be_a_file_of_the_take() {
        let dir = std::env::temp_dir().join(format!("tg-apart-take-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Canonical, so a folder not yet made sits under the same path as
        // the take's (macOS's temporary folder is reached through a link).
        let dir = dir.canonicalize().unwrap();
        let wav = dir.join("take_part01-001.wav");
        std::fs::write(&wav, b"RIFF").unwrap();
        std::fs::write(dir.join("take_part01-002.wav"), b"RIFF").unwrap();
        std::fs::write(dir.join("session.toml"), b"").unwrap();
        for sub in [
            "analysis",
            "analysis/sub",
            "analysis.partial",
            "session_report",
            "elsewhere",
        ] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        for (name, says) in [
            ("session.toml", "take's session.toml"),
            ("Session.TOML", "take's session.toml"),
            ("session.json", "take's session.json"),
            ("clock.csv", "take's clock.csv"),
            ("clocklog.txt", "take's clocklog.txt"),
            ("clock.log", "take's clock.log"),
            ("take_part01-002.wav", "recording of the take"),
            ("TAKE_PART01-001.WAV", "recording being read"),
            ("positions-test.json", "take's positions-test.json"),
            ("poise-test.json", "take's poise-test.json"),
            ("test-marks.json", "take's test-marks.json"),
            ("power-reserve.json", "take's power-reserve.json"),
            ("streamed-to.txt", "take's streamed-to.txt"),
            ("take_part01-001.wav.json", "recording's sidecar"),
            ("take_part01-002.wav.json", "recording of the take"),
            // A WAV left beside its FLAC, which the take's list leaves out.
            ("take_part03-001.wav", "recording of the take"),
            ("analysis/analysis.json", "in the take's analysis folder"),
            // Two folders down, and in the folder named in another case.
            ("analysis/sub/x.csv", "in the take's analysis folder"),
            ("ANALYSIS/x.csv", "in the take's analysis folder"),
            (
                "analysis.partial/partial.json",
                "in the take's analysis.partial folder",
            ),
            (
                "session_report/report.html",
                "in the take's session_report folder",
            ),
        ] {
            let e = files_apart(&wav, &[("--windows", &dir.join(name))]).unwrap_err();
            assert!(e.contains("--windows") && e.contains(says), "{name}: {e}");
        }
        assert!(files_apart(&wav, &[("--beats", &dir.join("beats.csv"))]).is_ok());
        // A take file's name in a folder that is not the take's is written.
        for name in ["session.toml", "take_part01-001.wav.json", "x.wav"] {
            let other = dir.join("elsewhere").join(name);
            assert!(files_apart(&wav, &[("--beats", &other)]).is_ok(), "{name}");
        }
        // Two outputs whose names differ only in case are one file.
        let e = files_apart(
            &wav,
            &[
                ("--beats", &dir.join("a.csv")),
                ("--windows", &dir.join("A.csv")),
            ],
        )
        .unwrap_err();
        assert!(e.contains("--windows and --beats both name"), "{e}");
        // The WAV kept as FLAC: the FLAC is what is read.
        let gone = dir.join("take_part02-001.wav");
        let flac = dir.join("take_part02-001.flac");
        std::fs::write(&flac, b"fLaC").unwrap();
        let e = files_apart(&gone, &[("--beats", &flac)]).unwrap_err();
        assert!(e.contains("recording being read"), "{e}");
        let e =
            files_apart(&gone, &[("--beats", &dir.join("take_part02-001.wav.json"))]).unwrap_err();
        assert!(e.contains("recording's sidecar"), "{e}");
        // The sidecar of the FLAC actually read, named by the WAV's name.
        let e = files_apart(
            &gone,
            &[("--beats", &dir.join("take_part02-001.flac.json"))],
        )
        .unwrap_err();
        assert!(e.contains("recording's sidecar"), "{e}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
