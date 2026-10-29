//! Each corpus recording read through the MCP server's `analyze_recording`
//! reads what `timegrapher session` reads for it: for each sound file a
//! corpus take's session.toml lists, the figures the server's lead gives
//! (rate, amplitude, beat error) and the stretch its Recording view reads
//! the Tick and Tock profiles over match session's reading of that entry,
//! to the last bit. Both read through core, so any difference is logic
//! written twice, or a reading made in another measurement context (#294).
//!
//! Needs the corpus (`TIMEGRAPHER_CORPUS`, its audio fetched) and a built
//! `timegrapher` (`TIMEGRAPHER_CLI`). With neither set it skips, saying
//! so; with `TIMEGRAPHER_REQUIRE_CORPUS` set, as in CI, a missing one
//! fails.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::{summary, tools, views};

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

/// The figures to compare for one entry, by name: the server's for its
/// `analyze` document, and session's for its reading.
fn figures(doc: &Value, reading: &Value) -> Vec<(&'static str, String, String)> {
    let m = &reading["measurement"];
    let [rate, amplitude, beat_error] = summary::analyze_figures(doc);
    let heard = doc["watch_heard"] != json!(false);
    let profile = views::profile_stretch(doc["duration_s"].as_f64().unwrap_or(0.0))
        .filter(|_| heard)
        .map(|(at, span)| format!("{at:?} s for {span:?} s"));
    // session's beat shape is read from its settled start ([`take::measured`])
    // for `measured_s` when nothing is masked; a mask can move it later
    // ([`analysis_take::shape_stretch`]), which the reading doesn't say.
    assert!(
        m["masked_s"].as_f64().unwrap_or(0.0) == 0.0,
        "{}: masked, so its beat shape's start isn't the settled start; \
         compare it through core's shape_stretch with the entry's masks",
        reading["label"]
    );
    let shape = reading["shape"]["measured_s"]
        .as_f64()
        .map(|len| format!("{:?} s for {len:?} s", m["start_s"].as_f64().unwrap_or(0.0)));
    vec![
        ("rate", bits(rate), bits(m["rate_s_per_day"].as_f64())),
        (
            "amplitude",
            bits(amplitude),
            bits(m["amplitude_deg"].as_f64()),
        ),
        (
            "beat error",
            bits(beat_error),
            bits(timegrapher_core::session::beat_error_size(
                m["beat_error_unlock_ms"].as_f64(),
                m["beat_error_ms"].as_f64(),
            )),
        ),
        (
            "profile stretch",
            profile.unwrap_or("none".into()),
            shape.unwrap_or("none".into()),
        ),
    ]
}

/// Why `analyze` reads a take's file differently from session: one
/// measurement context per take (#294) is not yet true of it.
macro_rules! context {
    () => {
        "#294: analyze reads the whole file, with no settling and none of the \
         take's settings or masks; session reads the entry after settling"
    };
}

/// Figures the server still reads differently from session on every
/// entry, and why. Each must still differ on some entry, so the list only
/// shrinks as the readings are made one.
const KNOWN_FIGURES: &[(&str, &str)] = &[
    (
        "rate",
        concat!(
            context!(),
            "; and the server gives the rate the USB lines correct, session the sound card's"
        ),
    ),
    ("amplitude", context!()),
    ("beat error", context!()),
];

/// Entries where a figure still differs, as "take/file: figure", and why.
/// Each must still differ (on one of the take's entries for that file,
/// where it lists the file twice), so the list only shrinks.
const KNOWN: &[(&str, &str)] = &[(
    "ym42_CR_long/take_part01-003.flac: profile stretch",
    "#294: the take's second entry for this file settles for its own 377 s \
     (300 s, half the file), which analyze, reading the file alone, does not know",
)];

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

/// Every sound file each corpus take lists reads the same through the MCP
/// server's `analyze_recording` as from `timegrapher session`. Takes, and
/// then files, are read side by side, one to a core.
#[test]
fn the_mcp_server_reads_each_corpus_recording_as_session_does() {
    let Some((root, cli)) = corpus_and_cli() else {
        return;
    };
    let ids = takes(&root);
    let sessions = on_every_core(&ids, |id| session(&cli, &root.join("takes").join(id)));
    let entries: Vec<(String, Value, Value, Value)> = ids
        .iter()
        .zip(sessions)
        .flat_map(|(id, doc)| {
            let (bph, lift) = (doc["bph"].clone(), doc["lift_deg"].clone());
            let readings = doc["readings"].as_array().cloned().unwrap_or_default();
            readings
                .into_iter()
                .filter(|r| r["label"].as_str().is_some_and(|l| l != "."))
                .map(move |r| (id.clone(), r, bph.clone(), lift.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    let found = on_every_core(&entries, |(id, reading, bph, lift)| {
        let label = reading["label"].as_str().expect("a reading's label");
        let file = root.join("takes").join(id).join(label);
        let doc = analyze(&cli, &file, bph, lift);
        let found = figures(&doc, reading);
        // Said as each file is read, so a run cut short still shows them.
        let differ: Vec<&str> = found
            .iter()
            .filter(|(_, mcp, session)| mcp != session)
            .map(|(what, _, _)| *what)
            .collect();
        eprintln!(
            "{id}/{label}: {}",
            if differ.is_empty() {
                "same".to_string()
            } else {
                format!("differs in {}", differ.join(", "))
            }
        );
        found
            .into_iter()
            .map(|(what, mcp, session)| (format!("{id}/{label}: {what}"), what, mcp, session))
            .collect::<Vec<_>>()
    });
    let compared = entries.len();
    let mut wrong = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut still = std::collections::BTreeSet::new();
    for (name, what, mcp, session) in found.into_iter().flatten() {
        let differs = mcp != session;
        if differs {
            seen.insert(what);
        }
        let known = KNOWN.iter().find(|(k, _)| *k == name);
        let known_figure = KNOWN_FIGURES.iter().find(|(k, _)| *k == what);
        match (known, known_figure) {
            (Some(_), _) if differs => {
                still.insert(name);
            }
            (None, None) if differs => {
                wrong.push(format!(
                    "{name}: {mcp} from the server, {session} from session"
                ));
            }
            _ => {}
        }
    }
    for (name, why) in KNOWN {
        if still.contains(*name) {
            eprintln!("{name}: differs, as known ({why})");
        } else {
            wrong.push(format!("{name}: now the same; take it off KNOWN"));
        }
    }
    for (what, why) in KNOWN_FIGURES {
        if seen.contains(what) {
            eprintln!("{what}: differs, as known ({why})");
        } else {
            wrong.push(format!(
                "{what}: now the same everywhere; take it off KNOWN_FIGURES"
            ));
        }
    }
    eprintln!("{compared} recordings compared");
    assert!(compared > 0, "no recording to compare in {root:?}");
    assert!(
        wrong.is_empty(),
        "the MCP server and session differ:\n{}",
        wrong.join("\n")
    );
}
