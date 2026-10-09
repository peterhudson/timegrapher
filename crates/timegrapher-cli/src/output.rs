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

/// Current version of each document kind.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
pub struct Envelope<'a, T: Serialize> {
    pub schema: String,
    pub software: Value,
    pub input: Value,
    #[serde(flatten)]
    pub body: &'a T,
}

pub fn software() -> Value {
    json!({
        "name": "timegrapher",
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
}

/// The JSON text of a document of `kind`.
pub fn to_json<T: Serialize>(kind: &str, input: Value, body: &T) -> Result<String, String> {
    serde_json::to_string_pretty(&Envelope {
        schema: format!("timegrapher.{kind}/{SCHEMA_VERSION}"),
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
