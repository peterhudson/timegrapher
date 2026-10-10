//! The public test corpus (github.com/peterhudson/timegrapher-corpus) as
//! `regress` reads it: a checkout holds `corpus.json` and each take's
//! metadata under `takes/<take>/`; the audio is attached to a release per
//! take. `--fetch` downloads what the run needs and checks it by SHA-256;
//! `--smoke` runs the takes that have no tg reference through the engine,
//! which passes unless the engine panics, or hears a watch in a take
//! recorded without one. See docs/regress.md.

use crate::archive::sha256_hex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use timegrapher_core::analysis::{self, AnalysisConfig};
use timegrapher_core::audio;
use timegrapher_core::stream::{self, StreamConfig};

/// The repository the release assets live in.
pub const REPO: &str = "peterhudson/timegrapher-corpus";
pub const INDEX: &str = "corpus.json";

/// Smoke takes recorded with no watch on the stand: `analyze` must say no
/// watch is heard in each of their files. Their readings are not compared.
pub const NO_WATCH: &[&str] = &["silence_empty_stand"];

#[derive(Debug, Deserialize)]
pub struct Corpus {
    pub takes: Vec<Take>,
}

#[derive(Debug, Deserialize)]
pub struct Take {
    pub id: String,
    /// "smoke", "referenced" or "paired".
    pub tier: String,
    pub release: String,
    pub files: Vec<File>,
}

#[derive(Debug, Deserialize)]
pub struct File {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}

/// The corpus index in `root`, if `root` is a corpus checkout.
pub fn load(root: &Path) -> Result<Option<Corpus>, String> {
    let p = root.join(INDEX);
    match std::fs::read_to_string(&p) {
        Ok(t) => serde_json::from_str(&t)
            .map(Some)
            .map_err(|e| format!("{}: {e}", p.display())),
        Err(_) => Ok(None),
    }
}

/// The folder of a corpus checkout that holds the takes.
pub fn takes_dir(root: &Path) -> PathBuf {
    root.join("takes")
}

fn picked(id: &str, filter: &[String]) -> bool {
    filter.is_empty() || filter.iter().any(|t| id.contains(t.as_str()))
}

/// Whether a take has a session file for `regress` to read.
fn has_session(dir: &Path) -> bool {
    fn walk(d: &Path, depth: usize) -> bool {
        if d.join(crate::session_cmd::MANIFEST).is_file() {
            return true;
        }
        depth > 0
            && std::fs::read_dir(d).is_ok_and(|rd| {
                rd.flatten()
                    .any(|e| e.path().is_dir() && walk(&e.path(), depth - 1))
            })
    }
    walk(dir, 2)
}

/// Download the audio of every take this run reads (those with a session
/// file, and the smoke takes with `smoke`) that is missing or changed.
pub fn fetch(
    root: &Path,
    c: &Corpus,
    filter: &[String],
    smoke: bool,
    quiet: bool,
) -> Result<(), String> {
    let mut got = 0u64;
    for t in &c.takes {
        let dir = takes_dir(root).join(&t.id);
        let wanted = picked(&t.id, filter) && (has_session(&dir) || (smoke && t.tier == "smoke"));
        if !wanted {
            continue;
        }
        for f in &t.files {
            if f.name.contains(['/', '\\']) || f.name.starts_with('.') {
                return Err(format!("{INDEX}: {}: unsafe file name {}", t.id, f.name));
            }
            let path = dir.join(&f.name);
            if std::fs::read(&path).is_ok_and(|d| sha256_hex(&d) == f.sha256) {
                continue;
            }
            if !quiet {
                eprintln!(
                    "fetching {}/{} ({:.0} MB)",
                    t.id,
                    f.name,
                    f.bytes as f64 / 1e6
                );
            }
            let url = format!(
                "https://github.com/{REPO}/releases/download/{}/{}",
                t.release, f.name
            );
            let part = dir.join(format!("{}.part", f.name));
            let status = Command::new("curl")
                .args(["-fsSL", "--retry", "3", "-o"])
                .arg(&part)
                .arg(&url)
                .status()
                .map_err(|e| format!("curl: {e} (curl downloads the corpus audio)"))?;
            if !status.success() {
                let _ = std::fs::remove_file(&part);
                return Err(format!("{}/{}: download failed ({status})", t.id, f.name));
            }
            let data = std::fs::read(&part).map_err(|e| e.to_string())?;
            if sha256_hex(&data) != f.sha256 {
                let _ = std::fs::remove_file(&part);
                return Err(format!(
                    "{}/{}: SHA-256 does not match {INDEX}",
                    t.id, f.name
                ));
            }
            std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
            got += data.len() as u64;
        }
    }
    if got > 0 {
        eprintln!("fetched {:.0} MB of corpus audio", got as f64 / 1e6);
    }
    Ok(())
}

/// What the engine did with one smoke recording.
#[derive(Debug, Clone, Serialize)]
pub struct Smoke {
    pub take: String,
    /// File name; cleared with --quiet.
    pub file: String,
    /// "ran", "stopped" (an error message, which is fine) or "panicked".
    pub outcome: &'static str,
    pub beats: Option<usize>,
    pub detail: Option<String>,
    /// `analyze`'s `watch_heard` for the file; none if it could not run.
    pub watch_heard: Option<bool>,
    /// Whether the take was recorded with no watch (see [`NO_WATCH`]).
    pub no_watch: bool,
}

impl Smoke {
    pub fn fails(&self) -> bool {
        self.outcome == "panicked" || (self.no_watch && self.watch_heard != Some(false))
    }
}

/// Run every smoke take's files through the stream analysis.
pub fn smoke(root: &Path, c: &Corpus, filter: &[String]) -> Vec<Smoke> {
    let mut out = Vec::new();
    for t in c
        .takes
        .iter()
        .filter(|t| t.tier == "smoke" && picked(&t.id, filter))
    {
        for f in &t.files {
            let path = takes_dir(root).join(&t.id).join(&f.name);
            let cfg = StreamConfig::default();
            let r =
                std::panic::catch_unwind(|| stream::analyze_files(&[path.as_path()], &cfg, |_| {}));
            let (outcome, beats, detail) = match r {
                Ok(Ok(log)) => ("ran", Some(log.beats.len()), None),
                Ok(Err(e)) => ("stopped", None, Some(e.to_string())),
                Err(p) => {
                    let msg = p
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
                    ("panicked", None, msg)
                }
            };
            let watch_heard = std::panic::catch_unwind(|| {
                audio::load(&path).ok().map(|a| {
                    analysis::analyze(&a, &AnalysisConfig::default())
                        .summary
                        .watch_heard
                })
            })
            .ok()
            .flatten();
            out.push(Smoke {
                take: t.id.clone(),
                file: f.name.clone(),
                outcome,
                beats,
                detail,
                watch_heard,
                no_watch: NO_WATCH.contains(&t.id.as_str()),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smoke(outcome: &'static str, watch_heard: Option<bool>, no_watch: bool) -> Smoke {
        Smoke {
            take: "t".into(),
            file: "f.flac".into(),
            outcome,
            beats: None,
            detail: None,
            watch_heard,
            no_watch,
        }
    }

    #[test]
    fn a_watch_heard_on_an_empty_stand_fails() {
        assert!(!smoke("ran", Some(false), true).fails());
        assert!(smoke("ran", Some(true), true).fails());
        assert!(smoke("ran", None, true).fails());
        assert!(!smoke("ran", Some(true), false).fails());
        assert!(!smoke("stopped", None, false).fails());
        assert!(smoke("panicked", None, false).fails());
    }
}
