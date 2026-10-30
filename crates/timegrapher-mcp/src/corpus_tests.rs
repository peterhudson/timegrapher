//! Each corpus recording read through the MCP server's `analyze_recording`
//! reads what `timegrapher session` reads for it: for each sound file a
//! corpus take's session.toml lists (a take listed as its own folder, ".",
//! by its one sound file), the figures the server's lead gives (rate,
//! amplitude, beat error) for each entry naming the file, and the stretch
//! its Recording view reads the Tick and Tock profiles over, match
//! session's readings of those entries to the last bit. Both read through
//! core, so any difference is logic written twice (#294).
//!
//! The same holds for core's synthetic edge takes, written for the test.
//!
//! Needs a built `timegrapher` (`TIMEGRAPHER_CLI`), and for the corpus the
//! corpus (`TIMEGRAPHER_CORPUS`, its audio fetched). With one not set its
//! test skips, saying so; with `TIMEGRAPHER_REQUIRE_CORPUS` set, as in CI,
//! a missing one fails.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::{summary, tools, views};

/// The command line, or none where the test skips.
fn cli() -> Option<PathBuf> {
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    match std::env::var_os("TIMEGRAPHER_CLI").map(PathBuf::from) {
        Some(cli) => {
            assert!(cli.is_file(), "no command line at {cli:?}");
            Some(cli)
        }
        None if required => panic!("TIMEGRAPHER_CLI is needed"),
        None => {
            eprintln!("SKIP: set TIMEGRAPHER_CLI to compare the MCP server with the command line");
            None
        }
    }
}

/// The corpus root and the command line, or none where the test skips.
fn corpus_and_cli() -> Option<(PathBuf, PathBuf)> {
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    let root = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from);
    let cli = std::env::var_os("TIMEGRAPHER_CLI").map(PathBuf::from);
    match (root, cli) {
        (Some(root), Some(cli)) => {
            assert!(
                root.join("corpus.json").is_file(),
                "no corpus.json in {root:?}"
            );
            assert!(cli.is_file(), "no command line at {cli:?}");
            Some((root, cli))
        }
        _ if required => panic!("TIMEGRAPHER_CORPUS and TIMEGRAPHER_CLI are both needed"),
        _ => {
            eprintln!("SKIP: set TIMEGRAPHER_CORPUS and TIMEGRAPHER_CLI to compare the MCP server with the command line");
            None
        }
    }
}

/// The takes the corpus lists that have a session.toml, by id.
fn takes(root: &Path) -> Vec<String> {
    let json: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("corpus.json")).unwrap()).unwrap();
    json["takes"]
        .as_array()
        .expect("corpus.json lists its takes")
        .iter()
        .filter_map(|t| t["id"].as_str().map(str::to_string))
        .filter(|id| root.join("takes").join(id).join("session.toml").is_file())
        .collect()
}

/// `timegrapher` run with `args`, its JSON.
fn run(cli: &Path, args: &[String]) -> Value {
    let run = std::process::Command::new(cli)
        .args(args)
        .output()
        .expect("the command line runs");
    assert!(
        run.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    serde_json::from_slice(&run.stdout).expect("JSON")
}

/// `timegrapher session --json` for the take.
fn session(cli: &Path, dir: &Path) -> Value {
    let out = std::env::temp_dir().join(format!(
        "tg-mcp-corpus-{}-{}",
        dir.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&out);
    let doc = run(
        cli,
        &[
            "session".into(),
            dir.to_string_lossy().into(),
            "--json".into(),
            "--out".into(),
            out.to_string_lossy().into(),
        ],
    );
    let _ = std::fs::remove_dir_all(&out);
    doc
}

/// `analyze_recording` called on `file` with the take's beat rate and lift
/// angle: the command line the server runs for that call, run.
fn analyze(cli: &Path, file: &Path, bph: &Value, lift: &Value) -> Value {
    let tool = tools::find("analyze_recording").expect("the analyze tool");
    let args = json!({ "recording": file, "bph": bph, "lift_deg": lift });
    let line = tools::command_line(tool, args.as_object().unwrap()).expect("a command line");
    run(cli, &line)
}

/// One figure, written to compare: to the bit, or none.
fn bits(v: Option<f64>) -> String {
    v.map_or("none".into(), |x| format!("{x:?}"))
}

/// A stretch, start and length, written to compare.
fn stretch(s: Option<(f64, f64)>) -> String {
    s.map_or("none".into(), |(at, span)| {
        format!("{at:?} s for {span:?} s")
    })
}

/// The figures to compare for one file, by name: the server's for its
/// `analyze` document, and session's for its `readings` naming the file,
/// in order.
fn figures(doc: &Value, readings: &[Value]) -> Vec<(String, String, String)> {
    let mcp = summary::take_figures(doc);
    let mut found = vec![(
        "entries".to_string(),
        mcp.len().to_string(),
        readings.len().to_string(),
    )];
    for (i, r) in readings.iter().enumerate() {
        let m = &r["measurement"];
        let [rate, amplitude, beat_error] = mcp.get(i).copied().unwrap_or_default();
        let at = |what: &str| format!("{what} (entry {})", i + 1);
        found.push((at("rate"), bits(rate), bits(m["rate_s_per_day"].as_f64())));
        found.push((
            at("amplitude"),
            bits(amplitude),
            bits(m["amplitude_deg"].as_f64()),
        ));
        found.push((
            at("beat error"),
            bits(beat_error),
            bits(timegrapher_core::session::beat_error_size(
                m["beat_error_unlock_ms"].as_f64(),
                m["beat_error_ms"].as_f64(),
            )),
        ));
    }
    // The view shows one profile: the first entry's beat-shape stretch.
    let shape = readings.iter().find_map(|r| {
        let s = &r["shape"];
        Some((s["from_s"].as_f64()?, s["measured_s"].as_f64()?))
    });
    found.push((
        "profile stretch".to_string(),
        stretch(views::profile_stretch(doc)),
        stretch(shape),
    ));
    found
}

/// Runs `work` on each item, on as many threads as the machine has, and
/// returns what it gives in the items' order.
fn on_every_core<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut done: Vec<(usize, R)> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads.min(items.len()))
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        mine.push((i, work(item)));
                    }
                    mine
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a worker finishes"))
            .collect()
    });
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, r)| r).collect()
}

/// Every sound file of each take of the corpus at `root`, read through
/// the MCP server's `analyze_recording` and from `timegrapher session`
/// (`cli`): how many files were compared, and each figure that differs.
/// Each of session's readings is met through the file its entry names,
/// by its label, in order. Takes, and then files, are read side by side,
/// one to a core.
fn compare(root: &Path, cli: &Path) -> (usize, Vec<String>) {
    let ids = takes(root);
    let dirs: Vec<PathBuf> = ids.iter().map(|id| root.join("takes").join(id)).collect();
    let sessions = on_every_core(&dirs, |dir| session(cli, dir));
    // Each take's sound files, as core lists them.
    let sounds: Vec<Vec<PathBuf>> = dirs
        .iter()
        .map(|d| timegrapher_core::take::sound_files(std::slice::from_ref(d)).unwrap_or_default())
        .collect();
    let files: Vec<(usize, PathBuf)> = sounds
        .iter()
        .enumerate()
        .flat_map(|(t, fs)| fs.iter().map(move |f| (t, f.clone())))
        .collect();
    let docs = on_every_core(&files, |(t, file)| {
        let doc = &sessions[*t];
        analyze(cli, file, &doc["bph"], &doc["lift_deg"])
    });
    let mut wrong = Vec::new();
    let mut used: Vec<Vec<bool>> = sessions
        .iter()
        .map(|d| vec![false; d["readings"].as_array().map_or(0, Vec::len)])
        .collect();
    for ((t, file), doc) in files.iter().zip(&docs) {
        let name = format!(
            "{}/{}",
            ids[*t],
            file.file_name().unwrap_or_default().to_string_lossy()
        );
        if let Some(error) = doc["take"]["error"].as_str() {
            wrong.push(format!("{name}: analyze couldn't read its take: {error}"));
            continue;
        }
        if doc["take"].is_null() {
            eprintln!("{name}: no entry names it");
            continue;
        }
        let readings = sessions[*t]["readings"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        // Session's readings for the entries `analyze` says name the file.
        let mut mine = Vec::new();
        for r in doc["take"]["readings"].as_array().into_iter().flatten() {
            let at =
                (0..readings.len()).find(|&i| !used[*t][i] && readings[i]["label"] == r["label"]);
            match at {
                Some(i) => {
                    used[*t][i] = true;
                    mine.push(readings[i].clone());
                }
                None => wrong.push(format!(
                    "{}/{}: analyze reads an entry session has no reading for",
                    ids[*t], r["label"]
                )),
            }
        }
        let found = figures(doc, &mine);
        let differ: Vec<&str> = found
            .iter()
            .filter(|(_, mcp, session)| mcp != session)
            .map(|(what, _, _)| what.as_str())
            .collect();
        // Said as each file is compared.
        eprintln!(
            "{name}: {}",
            if differ.is_empty() {
                format!("same over {} entries", mine.len())
            } else {
                format!("differs in {}", differ.join(", "))
            }
        );
        wrong.extend(
            found
                .into_iter()
                .filter(|(_, mcp, session)| mcp != session)
                .map(|(what, mcp, session)| {
                    format!("{name}: {what}: {mcp} from the server, {session} from session")
                }),
        );
    }
    // Every reading is met through a file, but an entry for a take's whole
    // folder of several files, or a take kept with no sound.
    for (t, doc) in sessions.iter().enumerate() {
        for (i, r) in doc["readings"].as_array().into_iter().flatten().enumerate() {
            let label = r["label"].as_str().unwrap_or("");
            let whole_folder = label == "." || label.starts_with(". (");
            // No one sound file to read it through.
            let unreachable = sounds[t].is_empty() || (whole_folder && sounds[t].len() > 1);
            if used[t][i] {
                continue;
            }
            if unreachable {
                eprintln!(
                    "{}/{label}: names no one sound file, so not read through analyze",
                    ids[t]
                );
            } else {
                wrong.push(format!(
                    "{}/{label}: no file read through analyze names it",
                    ids[t]
                ));
            }
        }
    }
    (files.len(), wrong)
}

/// `compare`'s findings, as a test's verdict.
fn assert_same(root: &Path, (compared, wrong): (usize, Vec<String>)) {
    eprintln!("{compared} recordings compared");
    assert!(compared > 0, "no recording to compare in {root:?}");
    assert!(
        wrong.is_empty(),
        "the MCP server and session differ:\n{}",
        wrong.join("\n")
    );
}

/// Every sound file each corpus take lists reads the same through the MCP
/// server as from `timegrapher session`, through `analyze`'s `take` block.
#[test]
fn the_mcp_server_reads_each_corpus_recording_as_session_does() {
    let Some((root, cli)) = corpus_and_cli() else {
        return;
    };
    assert_same(&root, compare(&root, &cli));
}

/// Each synthetic edge take (core's [`edge_takes`]: short, paused, slept,
/// silent and badly kept takes) reads the same through the MCP server as
/// from `timegrapher session`. Needs only a built `timegrapher`.
///
/// [`edge_takes`]: timegrapher_core::edge_takes
#[test]
fn the_mcp_server_reads_each_edge_take_as_session_does() {
    let Some(cli) = cli() else {
        return;
    };
    let root = std::env::temp_dir().join(format!("tg-mcp-edge-takes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    timegrapher_core::edge_takes::write_all(&root).expect("the edge takes are written");
    let found = compare(&root, &cli);
    let _ = std::fs::remove_dir_all(&root);
    assert_same(&root, found);
}
