//! Machine-readable output: every `--json` document has the same
//! envelope, so an agent can tell what it is reading and where it came from.
//!
//! The `schema` field names the kind of document and its version
//! (`timegrapher.analyze/2`). Within a version, fields are only ever added;
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
/// verdicts, named faults and advice.
pub fn schema_version(kind: &str) -> u32 {
    match kind {
        "record" => 2,
        "series" => 4,
        "session" => 3,
        "analyze" => 2,
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

/// Refuse output files that are the input being read or each other,
/// before anything is read or written: `analyze x.wav --beats x.wav`
/// would put a CSV where the recording was (#1876).
pub fn files_apart(input: &Path, outputs: &[(&str, &Path)]) -> Result<(), String> {
    let read = resolved(input);
    let mut seen: Vec<(&str, PathBuf)> = Vec::new();
    for &(flag, p) in outputs {
        let r = resolved(p);
        if r == read {
            return Err(format!(
                "{flag} {} is the recording being read; name another file",
                p.display()
            ));
        }
        if let Some((other, _)) = seen.iter().find(|(_, s)| *s == r) {
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
}
