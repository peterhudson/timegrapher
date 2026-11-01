//! `timegrapher report`: a zip a person can open, check and attach to an
//! issue, holding what a maintainer needs to reproduce a problem; and
//! `report --replay`, which runs the bundle's commands again with the
//! current build and says what moved. See docs/reports.md.
//!
//! Nothing here opens the sound input: a microphone check goes in only as
//! the recording and sidecar `doctor --save` already wrote.

use crate::archive::{read_folder, read_zip, sha256_hex, write_zip};
use crate::output;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use timegrapher_core::redact::{os_version, recent_files, Redactor};
use timegrapher_core::{clockstore, excerpt, recorder};

#[derive(Clone, Copy, Debug, clap::ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Bug,
    Reading,
    Setup,
    Idea,
}

pub struct Options {
    pub recording: Vec<PathBuf>,
    pub at: f64,
    pub span: f64,
    pub no_audio: bool,
    pub kind: Kind,
    pub note: Option<String>,
    pub bph: Option<u32>,
    pub lift: Option<f64>,
    pub out: Option<PathBuf>,
    pub json: bool,
    pub listened_back: bool,
    pub cc0: bool,
}

/// A command the bundle ran, as it can be run again.
#[derive(Serialize, serde::Deserialize, Clone)]
pub struct Recorded {
    /// Arguments after `timegrapher`.
    pub args: Vec<String>,
    /// "bundle" (run in the unpacked bundle) or "recording" (run beside
    /// the original recording, which the bundle may not hold).
    pub cwd: String,
    /// The file in the bundle holding what it printed.
    pub output: String,
    /// Whether everything it reads is in the bundle.
    pub replayable: bool,
}

#[derive(Serialize)]
struct FileEntry {
    path: String,
    bytes: usize,
    sha256: String,
}

/// Run this program with `args` in `dir` and return the JSON it printed.
fn run_self(args: &[String], dir: &Path) -> Result<Value, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let o = Command::new(exe)
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| e.to_string())?;
    if !o.status.success() {
        let err = String::from_utf8_lossy(&o.stderr);
        return Err(format!(
            "`timegrapher {}` failed: {}",
            args.join(" "),
            err.trim()
        ));
    }
    serde_json::from_slice(&o.stdout)
        .map_err(|e| format!("`timegrapher {}` printed no JSON: {e}", args.join(" ")))
}

fn settings_args(o: &Options) -> Vec<String> {
    let mut a = Vec::new();
    if let Some(b) = o.bph {
        a.extend(["--bph".to_string(), b.to_string()]);
    }
    if let Some(l) = o.lift {
        a.extend(["--lift".to_string(), l.to_string()]);
    }
    a
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
pub fn run(o: &Options) -> Result<(), String> {
    let now = SystemTime::now();
    let created = recorder::utc(now);
    let stamp: String = created
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T')
        .take(15)
        .collect();
    let mut red = Redactor::from_env();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let mut commands: Vec<Recorded> = Vec::new();
    let mut left_out: Vec<String> = vec![
        "the list of sound inputs: report never touches the sound input; \
         `doctor --save` records the one it used beside its recording"
            .into(),
        "environment variables".into(),
    ];
    let work = std::env::temp_dir().join(format!("timegrapher-report-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(work.join("audio")).map_err(|e| e.to_string())?;
    let result = (|| -> Result<Value, String> {
        let mut recording = Value::Null;
        if !o.recording.is_empty() {
            let files = crate::long::expand_dirs(&o.recording)?;
            let first = files.first().ok_or("no recording found")?;
            let folder = o.recording.len() == 1 && o.recording[0].is_dir();
            let source = if folder { &o.recording[0] } else { first };
            let source_name = name_of(source);
            let mut excerpt_info = Value::Null;
            if o.no_audio {
                left_out.push("the recording (--no-audio)".into());
            } else {
                eprintln!("cutting {:.0} s from {:.0} s", o.span, o.at);
                let pcm = excerpt::cut(&files, o.at, o.span).map_err(|e| e.to_string())?;
                if pcm.frames() == 0 {
                    return Err(format!("the recording ends before {} s (--at)", o.at));
                }
                let flac = work.join("audio/excerpt.flac");
                excerpt::write_flac(&flac, &pcm).map_err(|e| e.to_string())?;
                excerpt_info = json!({
                    "from_s": o.at,
                    "span_s": pcm.duration_s(),
                    "sample_rate": pcm.sample_rate,
                    "channels": pcm.channels,
                    "bits_per_sample": pcm.bits_per_sample,
                });
                let side = std::fs::read_to_string(output::sidecar_path(first))
                    .ok()
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok());
                let mut side = side.unwrap_or_else(|| json!({}));
                if let Some(m) = side.as_object_mut() {
                    m.insert(
                        "excerpt".into(),
                        json!({ "of": source_name, "from_s": o.at, "span_s": pcm.duration_s() }),
                    );
                }
                let side_text = red.apply(&serde_json::to_string_pretty(&side).unwrap());
                std::fs::write(work.join("audio/excerpt.flac.json"), &side_text)
                    .map_err(|e| e.to_string())?;
                entries.push((
                    "audio/excerpt.flac".into(),
                    std::fs::read(&flac).map_err(|e| e.to_string())?,
                ));
                entries.push(("audio/excerpt.flac.json".into(), side_text.into_bytes()));
                let mut args = vec![
                    "analyze".to_string(),
                    "audio/excerpt.flac".into(),
                    "--json".into(),
                ];
                args.extend(settings_args(o));
                eprintln!("analysing the excerpt");
                let v = run_self(&args, &work)?;
                entries.push((
                    "results/analyze-excerpt.json".into(),
                    red.apply(&serde_json::to_string_pretty(&v).unwrap())
                        .into_bytes(),
                ));
                commands.push(Recorded {
                    args,
                    cwd: "bundle".into(),
                    output: "results/analyze-excerpt.json".into(),
                    replayable: true,
                });
            }
            // The whole recording, run beside it so its paths are relative.
            let dir = source
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .to_path_buf();
            let mut args = if folder {
                vec!["long".to_string(), source_name.clone(), "--json".into()]
            } else if o.recording.len() == 1 {
                vec!["analyze".to_string(), source_name.clone(), "--json".into()]
            } else {
                Vec::new()
            };
            if args.is_empty() {
                left_out.push(
                    "an analysis of the whole recording (several files given; \
                     pass their folder instead)"
                        .into(),
                );
            } else {
                args.extend(settings_args(o));
                eprintln!("analysing the whole recording");
                let v = run_self(&args, &dir)?;
                let out = if folder {
                    "results/long-whole.json"
                } else {
                    "results/analyze-whole.json"
                };
                entries.push((
                    out.into(),
                    red.apply(&serde_json::to_string_pretty(&v).unwrap())
                        .into_bytes(),
                ));
                commands.push(Recorded {
                    args: args.iter().map(|a| red.apply(a)).collect(),
                    cwd: "recording".into(),
                    output: out.into(),
                    replayable: false,
                });
            }
            if folder {
                let log = o.recording[0].join("clocklog.txt");
                if let Ok(t) = std::fs::read_to_string(&log) {
                    entries.push(("long/clocklog.txt".into(), red.apply(&t).into_bytes()));
                }
            }
            recording = json!({
                "name": red.apply(&source_name),
                "files": files.len(),
                "excerpt": excerpt_info,
            });
        }
        if let Some(p) = clockstore::path() {
            if let Ok(t) = std::fs::read_to_string(&p) {
                entries.push(("env/clock.json".into(), red.apply(&t).into_bytes()));
            }
        }
        if let Some(dir) = clockstore::config_dir() {
            let mut logs = recent_files(&dir.join("logs"), |_| true);
            logs.extend(recent_files(&dir, |n| {
                n.starts_with("crash-") && n.ends_with(".txt")
            }));
            for f in logs {
                if let Ok(t) = std::fs::read_to_string(&f) {
                    entries.push((
                        format!("logs/{}", red.apply(&name_of(&f))),
                        red.apply(&t).into_bytes(),
                    ));
                }
            }
        }
        Ok(recording)
    })();
    let _ = std::fs::remove_dir_all(&work);
    let recording = result?;

    let note = o.note.as_deref().map(|n| red.apply(n));
    let files: Vec<FileEntry> = entries
        .iter()
        .map(|(p, d)| FileEntry {
            path: p.clone(),
            bytes: d.len(),
            sha256: sha256_hex(d),
        })
        .collect();
    let audio = entries.iter().any(|(p, _)| p == "audio/excerpt.flac");
    let body = json!({
        "created_utc": created,
        "kind": o.kind,
        "note": note,
        "os_version": os_version().map(|v| red.apply(&v)),
        "recording": recording,
        "commands": commands,
        "files": files,
        "consent": {
            "audio_included": audio,
            "listened_back": audio && o.listened_back,
            "corpus_cc0": audio && o.cc0,
        },
        "redacted": red.summary(),
        "left_out": left_out,
    });
    let manifest = output::to_json("report", json!({}), &body)?;
    let mut all = vec![("manifest.json".to_string(), manifest.clone().into_bytes())];
    all.extend(entries);
    let dir = o.out.clone().unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let zip = dir.join(format!("timegrapher-report-{stamp}Z.zip"));
    write_zip(&zip, &all)?;
    let bytes = std::fs::metadata(&zip).map(|m| m.len()).unwrap_or(0);
    if o.json {
        println!("{manifest}");
    } else {
        println!("Report     {}", zip.display());
        println!(
            "Size       {:.1} MB{}",
            bytes as f64 / 1e6,
            if bytes > 25_000_000 {
                " (over GitHub's 25 MB limit for attachments: shorten --span)"
            } else {
                ""
            }
        );
        if audio {
            println!("Audio      audio/excerpt.flac: listen to it before you send the report");
        }
        println!("Open it to see everything it holds; nothing is sent anywhere.");
    }
    if bytes > 25_000_000 {
        eprintln!("warning: the report is over 25 MB, GitHub's limit for attachments");
    }
    Ok(())
}

/// A bundle's files, from its zip or its unpacked folder.
fn load_bundle(p: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let kind = std::fs::symlink_metadata(p)
        .map_err(|e| format!("{}: {e}", p.display()))?
        .file_type();
    if kind.is_dir() || kind.is_symlink() {
        read_folder(p)
    } else {
        read_zip(p)
    }
}

/// A value that differs between the stored result and this build's.
#[derive(Serialize)]
pub struct Moved {
    pub path: String,
    pub then: Value,
    pub now: Value,
}

/// Where `now` differs from `then`, leaving out the `software` and `input`
/// blocks, which describe the run rather than the reading.
pub fn diff(then: &Value, now: &Value) -> Vec<Moved> {
    let mut out = Vec::new();
    walk("", then, now, &mut out);
    out
}

fn walk(path: &str, a: &Value, b: &Value, out: &mut Vec<Moved>) {
    if path.is_empty() {
        if let (Value::Object(x), Value::Object(y)) = (a, b) {
            let mut x = x.clone();
            let mut y = y.clone();
            for k in ["software", "input"] {
                x.remove(k);
                y.remove(k);
            }
            return walk_obj(path, &x, &y, out);
        }
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => walk_obj(path, x, y, out),
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                walk(&format!("{path}[{i}]"), p, q, out);
            }
        }
        (Value::Number(x), Value::Number(y)) => {
            let (p, q) = (x.as_f64().unwrap_or(0.0), y.as_f64().unwrap_or(0.0));
            if (p - q).abs() > 1e-9 * p.abs().max(q.abs()).max(1.0) {
                out.push(Moved {
                    path: path.into(),
                    then: a.clone(),
                    now: b.clone(),
                });
            }
        }
        _ if a != b => {
            let short = |v: &Value| match v {
                Value::Array(x) => json!(format!("{} items", x.len())),
                Value::Object(_) => json!("an object"),
                _ => v.clone(),
            };
            out.push(Moved {
                path: path.into(),
                then: short(a),
                now: short(b),
            });
        }
        _ => {}
    }
}

fn walk_obj(
    path: &str,
    x: &serde_json::Map<String, Value>,
    y: &serde_json::Map<String, Value>,
    out: &mut Vec<Moved>,
) {
    let join = |k: &str| {
        if path.is_empty() {
            k.to_string()
        } else {
            format!("{path}.{k}")
        }
    };
    for (k, v) in x {
        walk(&join(k), v, y.get(k).unwrap_or(&Value::Null), out);
    }
    for (k, v) in y {
        if !x.contains_key(k) {
            walk(&join(k), &Value::Null, v, out);
        }
    }
}

#[derive(Serialize)]
struct Replayed {
    args: Vec<String>,
    output: String,
    ran: bool,
    error: Option<String>,
    moved: Vec<Moved>,
}

/// Whether `args` are of the one shape `report` records for the excerpt:
/// `analyze audio/excerpt.flac --json` with `--bph` and `--lift`, and
/// nothing that writes a file.
fn safe_to_replay(args: &[String]) -> bool {
    let fixed = ["analyze", "audio/excerpt.flac", "--json"];
    if args.len() < 3 || args[..3] != fixed {
        return false;
    }
    let rest = &args[3..];
    rest.len().is_multiple_of(2)
        && rest
            .chunks(2)
            .all(|p| matches!(p[0].as_str(), "--bph" | "--lift") && p[1].parse::<f64>().is_ok())
}

#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
pub fn replay(bundle: &Path, json: bool) -> Result<(), String> {
    let files = load_bundle(bundle)?;
    let get = |name: &str| files.iter().find(|(p, _)| p == name).map(|(_, d)| d);
    let manifest: Value = serde_json::from_slice(
        get("manifest.json").ok_or("no manifest.json: not a timegrapher report")?,
    )
    .map_err(|e| format!("manifest.json: {e}"))?;
    let schema = manifest["schema"].as_str().unwrap_or("");
    if !schema.starts_with("timegrapher.report/") {
        return Err(format!(
            "manifest.json is '{schema}', not a timegrapher report"
        ));
    }
    // Every listed file is there and unchanged.
    let mut damaged = Vec::new();
    for f in manifest["files"].as_array().into_iter().flatten() {
        let p = f["path"].as_str().unwrap_or("");
        match get(p) {
            Some(d) if f["sha256"].as_str() == Some(sha256_hex(d).as_str()) => {}
            Some(_) => damaged.push(format!("{p} (changed)")),
            None => damaged.push(format!("{p} (missing)")),
        }
    }
    let work = std::env::temp_dir().join(format!("timegrapher-replay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    for (p, d) in &files {
        let safe = !p.starts_with('/')
            && !p.contains('\\')
            && !p.contains(':')
            && p.split('/').all(|c| !c.is_empty() && c != "..");
        if !safe {
            return Err(format!("the bundle holds a file at an unsafe path: {p}"));
        }
        let to = work.join(p);
        std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&to, d).map_err(|e| e.to_string())?;
    }
    let recorded: Vec<Recorded> =
        serde_json::from_value(manifest["commands"].clone()).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for c in &recorded {
        let mut r = Replayed {
            args: c.args.clone(),
            output: c.output.clone(),
            ran: false,
            error: None,
            moved: Vec::new(),
        };
        if !c.replayable {
            r.error = Some("its input is not in the bundle".into());
        } else if !safe_to_replay(&c.args) {
            r.error = Some("not a command report writes, so not run".into());
        } else {
            let then: Option<Value> = get(&c.output).and_then(|d| serde_json::from_slice(d).ok());
            match (then, run_self(&c.args, &work)) {
                (Some(then), Ok(now)) => {
                    r.ran = true;
                    r.moved = diff(&then, &now);
                }
                (None, _) => r.error = Some(format!("{} is missing", c.output)),
                (_, Err(e)) => r.error = Some(e),
            }
        }
        out.push(r);
    }
    let _ = std::fs::remove_dir_all(&work);
    let body = json!({
        "bundle": {
            "created_utc": manifest["created_utc"],
            "kind": manifest["kind"],
            "note": manifest["note"],
            "software": manifest["software"],
        },
        "damaged": damaged,
        "commands": out,
    });
    if json {
        return output::print(
            "replay",
            json!({ "bundle": bundle.display().to_string() }),
            &body,
        );
    }
    let sw = &manifest["software"];
    println!(
        "Bundle     {} ({}, made {} by timegrapher {} {})",
        bundle.display(),
        manifest["kind"].as_str().unwrap_or("?"),
        manifest["created_utc"].as_str().unwrap_or("?"),
        sw["version"].as_str().unwrap_or("?"),
        sw["git_hash"].as_str().unwrap_or("?"),
    );
    if let Some(n) = manifest["note"].as_str() {
        println!("Note       {n}");
    }
    for d in &damaged {
        println!("Damaged    {d}");
    }
    for r in &out {
        let cmd = format!("timegrapher {}", r.args.join(" "));
        match (&r.error, r.moved.len()) {
            (Some(e), _) => println!("Not run    {cmd}: {e}"),
            (None, 0) => println!("Same       {cmd}"),
            (None, n) => {
                println!("Moved      {cmd}: {n} values");
                for m in r.moved.iter().take(20) {
                    println!("             {}: {} -> {}", m.path, m.then, m.now);
                }
                if n > 20 {
                    println!(
                        "             ... and {} more (--json lists them all)",
                        n - 20
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replays_only_the_excerpt_analysis() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(safe_to_replay(&a(&[
            "analyze",
            "audio/excerpt.flac",
            "--json"
        ])));
        assert!(safe_to_replay(&a(&[
            "analyze",
            "audio/excerpt.flac",
            "--json",
            "--bph",
            "28800",
            "--lift",
            "52"
        ])));
        assert!(!safe_to_replay(&a(&[
            "analyze",
            "audio/excerpt.flac",
            "--json",
            "--beats",
            "/tmp/x.csv"
        ])));
        assert!(!safe_to_replay(&a(&[
            "long", "audio", "--json", "--out", "/"
        ])));
        assert!(!safe_to_replay(&a(&[
            "analyze",
            "audio/excerpt.flac",
            "--json",
            "--lift"
        ])));
    }

    #[test]
    fn diff_skips_the_run_and_finds_moves() {
        let a = json!({"software": {"version": "1"}, "input": {"files": []},
            "rate": 1.0, "beats": [1, 2], "name": "x"});
        let b = json!({"software": {"version": "2"}, "input": {"files": [1]},
            "rate": 1.25, "beats": [1, 2], "name": "x", "new": true});
        let d = diff(&a, &b);
        let paths: Vec<_> = d.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, ["rate", "new"]);
        assert!(diff(&a, &a).is_empty());
    }
}
