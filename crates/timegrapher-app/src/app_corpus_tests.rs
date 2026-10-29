//! Each corpus take opened as the app opens it, headless, reads what the
//! command line reads for it (#514): each recording the take's session.toml
//! lists (the whole take, ".", or one file of it), opened in the app,
//! reads on the cards what `timegrapher session` reads for that entry, to
//! the last bit. Both read through core, so any difference is logic
//! written twice.
//!
//! The same holds for core's synthetic edge takes, written afresh.
//!
//! Needs the corpus (`TIMEGRAPHER_CORPUS`, its audio fetched) and a built
//! `timegrapher` (`TIMEGRAPHER_CLI`); the edge takes need only the latter.
//! With neither set it skips, saying so; with `TIMEGRAPHER_REQUIRE_CORPUS`
//! set, as in CI, a missing one fails.

use super::*;
use std::time::Instant;

/// The longest the app may take to open one take: the overnight take is
/// eight hours of audio.
const OPEN_LIMIT_S: u64 = 1200;

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
            eprintln!("SKIP: set TIMEGRAPHER_CORPUS and TIMEGRAPHER_CLI to compare the app with the command line");
            None
        }
    }
}

/// The takes the corpus lists that have a session.toml, by id.
fn takes(root: &Path) -> Vec<String> {
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("corpus.json")).unwrap()).unwrap();
    json["takes"]
        .as_array()
        .expect("corpus.json lists its takes")
        .iter()
        .filter_map(|t| t["id"].as_str().map(str::to_string))
        .filter(|id| root.join("takes").join(id).join("session.toml").is_file())
        .collect()
}

/// The take opened as File > Open Recording opens it, waited for.
fn opened(dir: &Path) -> TimegrapherApp {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.start_batch(dir);
    let t = Instant::now();
    while app.batch.is_some() && t.elapsed().as_secs() < OPEN_LIMIT_S {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        app.batch.is_none(),
        "{dir:?} still opening after {OPEN_LIMIT_S} s"
    );
    app
}

/// `timegrapher session --json` for the take: its readings, by label.
fn session_readings(cli: &Path, dir: &Path) -> Vec<serde_json::Value> {
    let out = std::env::temp_dir().join(format!(
        "tg-app-corpus-{}-{}",
        dir.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&out);
    let run = std::process::Command::new(cli)
        .arg("session")
        .arg(dir)
        .arg("--json")
        .arg("--out")
        .arg(&out)
        .output()
        .expect("the command line runs");
    let _ = std::fs::remove_dir_all(&out);
    assert!(
        run.status.success(),
        "session {dir:?}: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&run.stdout).expect("session JSON");
    json["readings"].as_array().cloned().unwrap_or_default()
}

/// Where the app's whole-take reading and the command line's differ, in
/// words; none where they are the same.
fn differences(id: &str, app: &Option<WholeTake>, cli: &serde_json::Value) -> Vec<String> {
    let m = &cli["measurement"];
    let Some(w) = app else {
        return vec![format!(
            "{id}: the app has no whole-take reading; session has {m}"
        )];
    };
    let mut out = Vec::new();
    let mut same = |what: &str, a: Option<f64>, key: &str| {
        let b = m[key].as_f64();
        if a != b {
            out.push(format!("{id}: {what} {a:?} in the app, {b:?} from session"));
        }
    };
    same("rate", w.m.rate_s_per_day, "rate_s_per_day");
    same("amplitude", w.m.amplitude_deg, "amplitude_deg");
    same("beat error", w.m.beat_error_ms, "beat_error_ms");
    same("start", Some(w.m.start_s), "start_s");
    same("end", Some(w.m.end_s), "end_s");
    same("beats", Some(w.m.beats as f64), "beats");
    out
}

/// Corpus entries the app still reads differently from session, and why. Each
/// must still differ, so the list only shrinks as the app is fixed.
const KNOWN: &[(&str, &str)] = &[
    (
        "reverso21600_DU_level_overnight/take_part01-096.flac",
        "26 s file: session reads its 13 s after settling; the app wants three average windows",
    ),
    (
        "ym42_CR_long/take_part01-003.flac",
        "two entries for one file with their own measure_s and settle_s: the app keeps neither \
         the first's end nor session's halfway cap on the second's settling",
    ),
];

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

/// Each recording the takes under `root` list, read on the app's cards and
/// by `timegrapher session`, compared: what differs and is not on `known`,
/// or is on it and no longer differs. Takes, and then recordings, are read
/// side by side, one to a core, so the whole corpus fits CI's time.
fn compare(root: &Path, cli: &Path, known: &[(&str, &str)]) -> Vec<String> {
    let ids = takes(root);
    let readings = on_every_core(&ids, |id| {
        session_readings(cli, &root.join("takes").join(id))
    });
    let entries: Vec<(String, serde_json::Value)> = ids
        .iter()
        .zip(readings)
        .flat_map(|(id, rs)| rs.into_iter().map(move |r| (id.clone(), r)))
        .collect();
    let found = on_every_core(&entries, |(id, reading)| {
        let label = reading["label"].as_str().expect("a reading's label");
        let name = format!("{id}/{label}");
        let app = opened(&root.join("takes").join(id).join(label));
        let found = differences(&name, &app.whole_take(), reading);
        match known.iter().find(|(k, _)| *k == name) {
            Some((_, why)) if !found.is_empty() => {
                eprintln!("{name}: differs, as known ({why})");
                Vec::new()
            }
            Some(_) => vec![format!("{name}: now the same; take it off its known list")],
            None => {
                eprintln!(
                    "{name}: {}",
                    if found.is_empty() { "same" } else { "DIFFERS" }
                );
                found
            }
        }
    });
    eprintln!("{} recordings compared", entries.len());
    assert!(!entries.is_empty(), "no recording to compare in {root:?}");
    found.into_iter().flatten().collect()
}

/// Edge takes the app still reads differently from session, and why. Each
/// must still differ, so the list only shrinks as the app is fixed.
const EDGE_KNOWN: &[(&str, &str)] = &[
    (
        "edge_short_8s/.",
        "4 s measured: session reads it, with short_span; the app wants three average windows",
    ),
    (
        "edge_short_25s_settle_20/.",
        "12.5 s measured after settling: session reads it; the app wants three average windows",
    ),
    (
        "edge_all_silence/.",
        "15 s measured: session says no watch; the app wants three average windows",
    ),
    (
        "edge_sleep_two_entries/. (0–90 s)",
        "one of two readings of one file: the app opens a file whole, not a stretch of it",
    ),
    (
        "edge_sleep_two_entries/. (90–180 s)",
        "one of two readings of one file: the app opens a file whole, not a stretch of it",
    ),
];

/// Every recording each corpus take lists reads the same on the app's
/// cards as from `timegrapher session`.
#[test]
fn the_app_reads_each_corpus_take_as_the_command_line_does() {
    let Some((root, cli)) = corpus_and_cli() else {
        return;
    };
    let wrong = compare(&root, &cli, KNOWN);
    assert!(
        wrong.is_empty(),
        "the app and session differ:\n{}",
        wrong.join("\n")
    );
}

/// Each synthetic edge take (core's `edge_takes`: short, paused, slept,
/// silent, unlisted breaks, a drifting card) reads the same on the app's
/// cards as from `timegrapher session`. Needs only `TIMEGRAPHER_CLI`.
#[test]
fn the_app_reads_each_edge_take_as_the_command_line_does() {
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    let Some(cli) = std::env::var_os("TIMEGRAPHER_CLI").map(PathBuf::from) else {
        assert!(!required, "TIMEGRAPHER_CLI is needed");
        eprintln!("SKIP: set TIMEGRAPHER_CLI to compare the app with the command line");
        return;
    };
    let root = std::env::temp_dir().join(format!("tg-app-edge-takes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    timegrapher_core::edge_takes::write_all(&root).unwrap();
    let wrong = compare(&root, &cli, EDGE_KNOWN);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        wrong.is_empty(),
        "the app and session differ:\n{}",
        wrong.join("\n")
    );
}
