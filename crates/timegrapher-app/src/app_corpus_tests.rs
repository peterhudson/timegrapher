//! Each corpus take opened as the app opens it, headless, reads what the
//! command line reads for it (#514): each recording the take's session.toml
//! lists (the whole take, ".", or one file of it), opened in the app,
//! reads on the cards what `timegrapher session` reads for that entry, to
//! the last bit. Both read through core, so any difference is logic
//! written twice.
//!
//! Needs the corpus (`TIMEGRAPHER_CORPUS`, its audio fetched) and a built
//! `timegrapher` (`TIMEGRAPHER_CLI`). With neither set it skips, saying
//! so; with `TIMEGRAPHER_REQUIRE_CORPUS` set, as in CI, a missing one
//! fails.

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

/// Every recording each corpus take lists reads the same on the app's
/// cards as from `timegrapher session`.
#[test]
fn the_app_reads_each_corpus_take_as_the_command_line_does() {
    let Some((root, cli)) = corpus_and_cli() else {
        return;
    };
    let mut compared = 0;
    let mut wrong = Vec::new();
    for id in takes(&root) {
        let dir = root.join("takes").join(&id);
        for reading in session_readings(&cli, &dir) {
            let label = reading["label"].as_str().expect("a reading's label");
            let name = format!("{id}/{label}");
            let app = opened(&dir.join(label));
            let found = differences(&name, &app.whole_take(), &reading);
            eprintln!(
                "{name}: {}",
                if found.is_empty() { "same" } else { "DIFFERS" }
            );
            wrong.extend(found);
            compared += 1;
        }
    }
    eprintln!("{compared} recordings compared");
    assert!(compared > 0, "no recording to compare in {root:?}");
    assert!(
        wrong.is_empty(),
        "the app and session differ:\n{}",
        wrong.join("\n")
    );
}
