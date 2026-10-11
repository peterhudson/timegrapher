//! Running the `timegrapher` command-line program, which does the work.

use std::path::PathBuf;
use std::process::Stdio;

use serde_json::Value;

/// Where the `timegrapher` program is: `TIMEGRAPHER_CLI` if set, else the
/// one installed beside this server (the install bundle ships both), else
/// whatever `timegrapher` is on PATH.
pub fn program() -> PathBuf {
    if let Some(p) = std::env::var_os("TIMEGRAPHER_CLI") {
        return PathBuf::from(p);
    }
    let name = format!("timegrapher{}", std::env::consts::EXE_SUFFIX);
    if let Some(beside) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(&name)))
        .filter(|p| p.is_file())
    {
        return beside;
    }
    PathBuf::from(name)
}

pub struct Outcome {
    /// The command's JSON document, when it printed one.
    pub json: Option<Value>,
    /// What it said on stderr (progress, warnings, the error).
    pub stderr: String,
    pub exit_code: Option<i32>,
}

pub async fn run(args: &[String]) -> Result<Outcome, String> {
    let program = program();
    let out = tokio::process::Command::new(&program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| {
            format!(
                "could not run {}: {e}. The timegrapher program must be installed beside \
                 timegrapher-mcp or on PATH (or named by TIMEGRAPHER_CLI).",
                program.display()
            )
        })?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(Outcome {
        json: serde_json::from_str(stdout.trim()).ok(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        exit_code: out.status.code(),
    })
}
