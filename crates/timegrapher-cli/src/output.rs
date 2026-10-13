//! Machine-readable output: every `--json` document has the same
//! envelope, so an agent can tell what it is reading and where it came from.
//!
//! The `schema` field names the kind of document and its version
//! (`timegrapher.analyze/1`). Within a version, fields are only ever added;
//! a change that would break a reader bumps the version. See
//! `docs/agent-interface.md`.

use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Version of the document kinds that have not changed since the first.
pub const SCHEMA_VERSION: u32 = 1;

/// Current version of a document kind. `record/2` keeps every take and
/// drops the verdict; `series/2` dropped the one-word verdict, the
/// findings' severity and advice, and `moves_with`; `analyze/2`,
/// `doctor/2` and `regress-holdout/2` dropped their grades, verdicts,
/// named faults and advice.
pub fn schema_version(kind: &str) -> u32 {
    match kind {
        "record" => 2,
        "series" => 2,
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
    let mut s = file.as_os_str().to_owned();
    s.push(".json");
    PathBuf::from(s)
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
