//! Each corpus take opened as the app opens it, headless, reads what the
//! command line reads for it (#514): each recording the take's session.toml
//! lists (the whole take, ".", or one file of it), opened in the app,
//! reads on the cards what `timegrapher session` reads for that entry, to
//! the last bit. Both read through core, so any difference is logic
//! written twice.
//!
//! Each take opened whole reads in its panes what the command line
//! reads for it too: the Distributions pane's distributions are `series`'s,
//! and the Power Reserve pane's run from a long take is `session`'s.
//! Gaps still open are listed, each with its issue.
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

/// The takes the corpus lists that have a session.toml, by id, but for its
/// holdout takes: their audio is fetched only for `regress --holdout`, and
/// their readings stay sealed (docs/developer/regress.md, "Holdout takes").
fn takes(root: &Path) -> Vec<String> {
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("corpus.json")).unwrap()).unwrap();
    json["takes"]
        .as_array()
        .expect("corpus.json lists its takes")
        .iter()
        .filter(|t| t["tier"].as_str() != Some("holdout"))
        .filter_map(|t| t["id"].as_str().map(str::to_string))
        .filter(|id| root.join("takes").join(id).join("session.toml").is_file())
        .collect()
}

/// The take opened as File > Open Recording opens it, waited for.
fn opened(dir: &Path) -> TimegrapherApp {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    // At 10 s, so edge_45s (25 s measured) guards against the whole take
    // needing a multiple of Average Over again.
    app.set_average(10.0);
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
    session_json(cli, dir)["readings"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// `timegrapher session --json` for the take.
fn session_json(cli: &Path, dir: &Path) -> serde_json::Value {
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
    serde_json::from_slice(&run.stdout).expect("session JSON")
}

/// Where the app's whole-take reading and the command line's differ, in
/// words, figures and caveats alike; none where they are the same.
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
    // The caveats each names, in any order.
    let mut app_codes = timegrapher_core::edge_takes::caveat_codes(&w.m);
    let mut cli_codes: Vec<String> = m["caveats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["code"].as_str().map(str::to_string))
        .collect();
    app_codes.sort();
    cli_codes.sort();
    if app_codes != cli_codes {
        out.push(format!(
            "{id}: caveats {app_codes:?} in the app, {cli_codes:?} from session"
        ));
    }
    out
}

/// Corpus entries the app still reads differently from session, and why.
/// Each must still differ, so the list only shrinks as the app is fixed. A
/// file the session file lists more than once is named with its entry's
/// place among them: `file entry 1`.
const KNOWN: &[(&str, &str)] = &[
    (
        "ym42_CR_long/take_part01-003.flac entry 1",
        "by design (#1197): one position split in two entries of one file; the app reads \
         a position's stretches together, as a Positions Test does",
    ),
    (
        "ym42_CR_long/take_part01-003.flac entry 2",
        "by design (#1197): one position split in two entries of one file; the app reads \
         a position's stretches together, as a Positions Test does",
    ),
];

/// Each recording the takes under `root` list, read on the app's cards and
/// by `timegrapher session`, compared: what differs and is not on `known`,
/// or is on it and no longer differs. Takes, and then recordings, are read
/// side by side, one to a core, so the whole corpus fits CI's time.
fn compare(root: &Path, cli: &Path, known: &[(&str, &str)]) -> Vec<String> {
    let ids = takes(root);
    let readings = timegrapher_core::cores::on_every_core(&ids, |id| {
        session_readings(cli, &root.join("takes").join(id))
    });
    let entries: Vec<(String, serde_json::Value)> = ids
        .iter()
        .zip(readings)
        .flat_map(|(id, rs)| rs.into_iter().map(move |r| (id.clone(), r)))
        .collect();
    let label = |r: &serde_json::Value| r["label"].as_str().expect("a reading's label").to_string();
    // Each reading's name, with its place among the entries for its file
    // when there are several.
    let names: Vec<String> = entries
        .iter()
        .enumerate()
        .map(|(i, (id, r))| {
            let file =
                |(other, o): &(String, serde_json::Value)| other == id && label(o) == label(r);
            let name = format!("{id}/{}", label(r));
            if entries.iter().filter(|e| file(e)).count() > 1 {
                let place = entries[..i].iter().filter(|e| file(e)).count() + 1;
                format!("{name} entry {place}")
            } else {
                name
            }
        })
        .collect();
    let named: Vec<_> = entries.iter().zip(names).collect();
    let found = timegrapher_core::cores::on_every_core(&named, |((id, reading), name)| {
        let label = label(reading);
        let name = name.clone();
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
        "edge_sleep_two_entries/. (0–90 s)",
        "one of two readings of one file: the app opens a file whole, not a stretch of it",
    ),
    (
        "edge_sleep_two_entries/. (90–180 s)",
        "one of two readings of one file: the app opens a file whole, not a stretch of it",
    ),
    (
        "edge_reads_nothing/. (0–90 s)",
        "a stretch of a file: the app opens a file whole, not a stretch of it",
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

/// The empty stand opened: no watch heard, so the beat rate shows
/// "Auto" with no number, not the rate the engine last listened at
/// (#1972, "Auto 21600"). Needs only `TIMEGRAPHER_CORPUS`.
#[test]
fn the_empty_stand_finds_no_beat_rate() {
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(!required, "TIMEGRAPHER_CORPUS is needed");
        eprintln!("SKIP: set TIMEGRAPHER_CORPUS to open the empty stand");
        return;
    };
    let app = opened(&root.join("takes").join("silence_empty_stand"));
    assert_eq!(app.bph_heard(), None);
    assert_eq!(app.watch_facts().1, "Auto");
}

/// The empty stand opened and looked back over at every Average Over:
/// wherever the reading cards withhold, the Tick Tock Profile draws no
/// shape and no facts either; it showed "Beats 25" there
/// after a change of the strip's Length (#1965, #2123). Needs only
/// `TIMEGRAPHER_CORPUS`.
#[test]
fn the_empty_stands_profile_withholds_with_the_cards() {
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(!required, "TIMEGRAPHER_CORPUS is needed");
        eprintln!("SKIP: set TIMEGRAPHER_CORPUS to open the empty stand");
        return;
    };
    let dir = root.join("takes").join("ym42_DU_65h_excerpt_emptystand");
    let mut app = opened(&dir);
    // Opened as File > Open Recording opens it, so the shape at a moment
    // the session kept none for is worked out from the recording.
    app.input = Input::File;
    app.file_path = dir.display().to_string();
    let latest = app.latest_s();
    let mut withheld = 0;
    for average in [2.0, 4.0, 10.0, 30.0] {
        app.set_average(average);
        let mut t = 20.0;
        while t < latest {
            app.strip_end = Some(t);
            let end = app.strip_end_s();
            // The shape worked out from the recording, waited for to the
            // end: a wait cut short would show no shape and pass on main.
            let working = Some("Working it out from the recording…");
            let started = Instant::now();
            let mut shown = app.profiles_at(end);
            while shown.line.as_deref() == working && started.elapsed().as_secs() < OPEN_LIMIT_S {
                std::thread::sleep(std::time::Duration::from_millis(5));
                shown = app.profiles_at(end);
            }
            let at = format!("{average} s at {end:.0} s");
            assert_ne!(shown.line.as_deref(), working, "still working, {at}");
            if app.cards_unheard().is_some() {
                withheld += 1;
                assert!(shown.profiles.iter().all(Option::is_none), "{at}");
            }
            t += 15.0;
        }
    }
    assert!(withheld > 0, "the cards withhold on the empty stand");
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

/// Split entries retain their clock even without readable recorder metadata.
fn split_entries_clock_probe(metadata: &str, folder: bool) {
    use timegrapher_core::{audio, synth};
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    let Some(cli) = std::env::var_os("TIMEGRAPHER_CLI").map(PathBuf::from) else {
        assert!(!required, "TIMEGRAPHER_CLI is needed");
        eprintln!("SKIP: set TIMEGRAPHER_CLI for the duplicate-entry probe");
        return;
    };
    let root = std::env::temp_dir().join(format!(
        "tg-app-duplicate-clock-{metadata}-{folder}-{}",
        std::process::id()
    ));
    let dir = root.join(format!("takes/duplicate_clock_{metadata}_{folder}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&dir).unwrap();
    let sound = synth::generate(
        &synth::SynthConfig {
            duration_s: 300.0,
            rate_s_per_day: -7.0,
            ..Default::default()
        },
        |_| 270.0,
        |_| 0.0,
    );
    let fs = sound.sample_rate;
    audio::write_wav(&dir.join("take.wav"), &sound).unwrap();
    let mut clock = String::from("unix_s,frames\n");
    let mut frames: Vec<u32> = (0..=30).map(|i| i * 10 * fs).collect();
    frames.push(100 * fs - fs / 10);
    frames.sort_unstable();
    for frame in frames {
        let wall = 1_700_000_000.0
            + frame as f64 / fs as f64 * 1.00002
            + if frame >= 100 * fs { 60.0 } else { 0.0 };
        clock.push_str(&format!("{wall:.6},{frame}\n"));
    }
    std::fs::write(dir.join("clock.csv"), clock).unwrap();
    let record = serde_json::json!({"segments": ["take.wav"], "breaks": [
        {"frame": 100 * fs, "audio_s": 100.0, "gap_s": 60.0}
    ]})
    .to_string();
    match metadata {
        "valid" => std::fs::write(dir.join("session.json"), &record).unwrap(),
        "truncated" => {
            std::fs::write(dir.join("session.json"), &record[..record.len() / 2]).unwrap()
        }
        "missing" => {}
        _ => unreachable!(),
    }
    std::fs::write(
        dir.join("session.toml"),
        "bph = 28800\nlift = 52\n\
         [[recording]]\nfile = \"take.wav\"\nposition = \"DU\"\nend_s = 160\nclock = \"clock.csv\"\n\
         [[recording]]\nfile = \"take.wav\"\nposition = \"DD\"\nstart_s = 160\nclock = \"clock.csv\"\n",
    )
    .unwrap();
    let readings = session_readings(&cli, &dir);
    assert_eq!(readings.len(), 2);
    for reading in &readings {
        let rate = reading["measurement"]["rate_s_per_day"].as_f64().unwrap();
        assert!((rate + 8.728).abs() < 0.2, "corrected rate: {rate}");
    }
    let path = if folder {
        dir.clone()
    } else {
        dir.join("take.wav")
    };
    let mut app = opened(&path);
    let clock = app.take_clock.as_ref().expect("the app retains the clock");
    assert!((clock.ppm + 20.0).abs() < 0.1);
    assert_eq!(clock.breaks.len(), 1);
    assert!((clock.breaks[0].gap_s - 60.0).abs() < 0.01);
    for (position, reading) in [Position::DialUp, Position::DialDown]
        .into_iter()
        .zip(&readings)
    {
        app.position = position;
        let wrong = differences(metadata, &app.whole_take(), reading);
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn split_entries_clock_valid_metadata() {
    split_entries_clock_probe("valid", false);
}
#[test]
fn split_entries_clock_truncated_metadata() {
    split_entries_clock_probe("truncated", false);
}
#[test]
fn split_entries_clock_missing_metadata() {
    split_entries_clock_probe("missing", false);
}

#[test]
fn split_entries_clock_folder_valid_metadata() {
    split_entries_clock_probe("valid", true);
}

#[test]
fn split_entries_clock_folder_truncated_metadata() {
    split_entries_clock_probe("truncated", true);
}

#[test]
fn split_entries_clock_folder_missing_metadata() {
    split_entries_clock_probe("missing", true);
}

/// `timegrapher series --json` for the take.
fn series_json(cli: &Path, dir: &Path) -> serde_json::Value {
    let run = std::process::Command::new(cli)
        .arg("series")
        .arg(dir)
        .arg("--json")
        .output()
        .expect("the command line runs");
    assert!(
        run.status.success(),
        "series {dir:?}: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    serde_json::from_slice(&run.stdout).expect("series JSON")
}

/// The figures of a distribution that the app draws and `series` writes:
/// its count, quantiles, bins and peaks (not its step, which `series`
/// gives on the corrected clock).
fn dist_figures(d: &serde_json::Value) -> serde_json::Value {
    let keys = [
        "n", "median", "p10", "p90", "edges", "counts", "below", "above", "peaks",
    ];
    keys.iter()
        .map(|k| ((*k).to_string(), d[k].clone()))
        .collect()
}

/// The figures of two distributions that differ, the app's then
/// `series`'s, in words.
fn figures_apart(ours: Option<&serde_json::Value>, theirs: Option<&serde_json::Value>) -> String {
    let (Some(a), Some(b)) = (ours, theirs) else {
        return format!("{ours:?} in the app, {theirs:?} from series");
    };
    let short = |v: &serde_json::Value| v.to_string().chars().take(80).collect::<String>();
    a.as_object()
        .into_iter()
        .flatten()
        .filter(|(k, v)| b.get(k.as_str()) != Some(v))
        .map(|(k, v)| {
            format!(
                "{k} {} in the app, {} from series",
                short(v),
                short(&b[k.as_str()])
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Where the app's panes for the whole take, opened as one recording,
/// differ from the command line's: each distribution the Distributions
/// pane draws, against `series`, and the run the Power Reserve pane reads
/// from a long take, against `session`. Each as `(what, how)`; `what` is
/// `id`, then the series and its values ("readings" or "2 s"), or
/// "power reserve". With them, whether either surface read a run, and
/// how many seconds of the take the app read.
fn pane_differences(id: &str, root: &Path, cli: &Path) -> (Vec<(String, String)>, bool, f64) {
    use timegrapher_core::distribution as dist;
    let dir = root.join("takes").join(id);
    let mut app = opened(&dir);
    app.update_dist_readings(None);
    let series = series_json(cli, &dir);
    let mut out = Vec::new();
    let theirs = series["distributions"]["overall"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for (k, short) in [
        (SeriesKind::Rate, false),
        (SeriesKind::Amplitude, false),
        (SeriesKind::Amplitude, true),
        (SeriesKind::BeatError, false),
        (SeriesKind::BeatError, true),
    ] {
        let (values, kind, _, label, _) = app.hist_values(k, short, &theme::DARK);
        let step = if short {
            dist::STRETCH_S
        } else {
            app.average_s
        };
        let ours = dist::distribution(&values, kind, label, step)
            .map(|d| dist_figures(&serde_json::to_value(d).unwrap()));
        let other = theirs
            .iter()
            .find(|d| d["series"] == kind.series && d["values"] == label)
            .map(dist_figures);
        if ours != other {
            out.push((
                format!("{id} {} {label}", kind.series),
                figures_apart(ours.as_ref(), other.as_ref()),
            ));
        }
    }
    app.reserve_from_long_take();
    let ours = app
        .reserve
        .as_ref()
        .and_then(|t| t.results.as_ref())
        .map(|r| serde_json::to_value(r).unwrap())
        .unwrap_or(serde_json::Value::Null);
    let theirs = session_json(cli, &dir)["power_reserve"].clone();
    let run = !ours.is_null() || !theirs.is_null();
    if ours != theirs {
        out.push((
            format!("{id} power reserve"),
            format!("{ours} in the app, {theirs} from session"),
        ));
    }
    let length = app.live.as_ref().map_or(0.0, |l| l.duration_s());
    (out, run, length)
}

/// Pane figures the app still reads differently from the command line,
/// with the issue that tracks each. A name starting `* ` stands for every
/// take; each entry must still differ on at least one take, so the list
/// only shrinks as the app is fixed. A take's "power reserve" entry is
/// held to that only where a surface reads a run: a pull request's corpus
/// cut is too short for one. Likewise an entry is held only where the take
/// read reaches the seconds it gives, where its difference is: a pull
/// request's cut of three files may stop short of it.
const PANES_KNOWN: &[(&str, &str, f64)] = &[
    ("ym42_CR_long amplitude 2 s", BREAK_EDGE, 5400.0),
    ("ym42_CR_long beat_error 2 s", BREAK_EDGE, 5400.0),
];

/// #636, in [`PANES_KNOWN`]: at ym42_CR_long's 5,400 s break (3.883 s
/// gap) the app keeps the 2 s window ending at the break and leaves out
/// the next, cut by a jump its engine finds 0.05 s after the break;
/// `series` reads the break on true time with its gap and keeps the
/// other. One window of about 3,700.
const BREAK_EDGE: &str = "#636: the app and series pick a different 2 s window at a break \
                          with a gap";

/// Whether the difference `what` is the known gap `known`.
fn is_known(known: &str, what: &str) -> bool {
    known
        .strip_prefix("* ")
        .map_or(known == what, |rest| what.ends_with(&format!(" {rest}")))
}

/// For each corpus take, opened whole in the app as File > Open Recording
/// opens it, the Distributions pane's distributions are `series`'s, and
/// the Power Reserve pane's run from a long take is `session`'s (#514).
/// Both read through core, so any difference is logic written twice.
#[test]
fn the_apps_panes_read_each_corpus_take_as_the_command_line_does() {
    let Some((root, cli)) = corpus_and_cli() else {
        return;
    };
    let ids = takes(&root);
    assert!(!ids.is_empty(), "no take to compare in {root:?}");
    let read = timegrapher_core::cores::on_every_core(&ids, |id| pane_differences(id, &root, &cli));
    let with_run: Vec<&str> = ids
        .iter()
        .zip(&read)
        .filter(|(_, (_, run, _))| *run)
        .map(|(id, _)| id.as_str())
        .collect();
    // Each take's id with the seconds of it the app read.
    let lengths: Vec<(&str, f64)> = ids
        .iter()
        .zip(&read)
        .map(|(id, (_, _, length))| (id.as_str(), *length))
        .collect();
    let found: Vec<(String, String)> = read.into_iter().flat_map(|(d, _, _)| d).collect();
    let mut wrong = Vec::new();
    for (what, how) in &found {
        match PANES_KNOWN.iter().find(|(k, _, _)| is_known(k, what)) {
            Some((_, why, _)) => eprintln!("{what}: differs, as known ({why}): {how}"),
            None => wrong.push(format!("{what}: {how}")),
        }
    }
    for &(known, _, reaches_s) in PANES_KNOWN {
        let held = known
            .strip_suffix(" power reserve")
            .is_none_or(|id| with_run.contains(&id))
            && lengths.iter().any(|&(id, length)| {
                let names = known.starts_with("* ") || known.starts_with(&format!("{id} "));
                names && length >= reaches_s
            });
        if held && !found.iter().any(|(what, _)| is_known(known, what)) {
            wrong.push(format!("{known}: now the same; take it off PANES_KNOWN"));
        }
    }
    eprintln!("{} takes compared", ids.len());
    assert!(
        wrong.is_empty(),
        "the app's panes and the command line differ:\n{}",
        wrong.join("\n")
    );
}

/// `secs` of the corpus recording `flac` from `from_s` in, listened to as
/// from a microphone whose blocks are stamped `ppm` fast against the
/// computer's clock, stopped and saved: the saved take's folder, under
/// `base`. See [`listened`].
fn listened_and_saved(
    base: &Path,
    flac: &Path,
    stretch: (f64, f64),
    ppm: f64,
    pause: Option<(f64, f64)>,
    silent_s: f64,
) -> PathBuf {
    saved(
        &mut listened(base, flac, stretch, ppm, pause, silent_s),
        base,
    )
}

/// The stopped `app`'s take saved under `base`: its folder.
fn saved(app: &mut TimegrapherApp, base: &Path) -> PathBuf {
    app.save_into(&base.join("saved"));
    app.wait_for_save();
    app.session
        .saved_to()
        .expect("the take is saved")
        .to_path_buf()
}

/// The app just stopped after `secs` of the corpus recording `flac` from
/// `from_s` in, listened to as from a microphone whose blocks are stamped
/// `ppm` fast against the computer's clock, recorded under `base`. The
/// input stalls for `pause` (audio seconds in, and how long by the
/// computer's clock) where one is given, so the take holds a break there,
/// and `silent_s` of a quiet stand follows the sound, as when the watch is
/// taken off the stand before Stop.
fn listened(
    base: &Path,
    flac: &Path,
    stretch: (f64, f64),
    ppm: f64,
    pause: Option<(f64, f64)>,
    silent_s: f64,
) -> TimegrapherApp {
    let fs = timegrapher_core::audio::load(flac).unwrap().sample_rate as f64;
    // The tail is a quiet stand's hiss, not digital zero: a microphone
    // never hears nothing.
    let mut seed = 0x2073_u32;
    let hiss = (0..(silent_s * fs) as usize)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 2e-3 - 1e-3
        })
        .collect();
    listened_then(base, flac, stretch, ppm, pause, hiss)
}

/// [`listened`], with the sound `tail` after the watch's in place of a
/// quiet stand's hiss: a stand's own noise, say.
fn listened_then(
    base: &Path,
    flac: &Path,
    stretch: (f64, f64),
    ppm: f64,
    pause: Option<(f64, f64)>,
    tail: Vec<f32>,
) -> TimegrapherApp {
    listened_moved(base, flac, stretch, ppm, pause, tail, None)
}

/// [`listened_then`], with the watch moved to another position `moved.1`
/// once `moved.0` seconds of sound are heard.
fn listened_moved(
    base: &Path,
    flac: &Path,
    (from_s, secs): (f64, f64),
    ppm: f64,
    pause: Option<(f64, f64)>,
    tail: Vec<f32>,
    mut moved: Option<(f64, Position)>,
) -> TimegrapherApp {
    std::fs::create_dir_all(base).unwrap();
    let audio = timegrapher_core::audio::load(flac).unwrap();
    let fs = audio.sample_rate as usize;
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(audio.sample_rate, "test".into());
    app.session.listen();
    let info = app.info("test", audio.sample_rate, 16);
    app.session.recorder = Some(Recorder::start(&base.join("live"), info).unwrap());
    let tx = super::tests::queued_capture(&mut app, base);
    let from = (from_s * fs as f64) as usize;
    let block = fs / 50;
    let mut sound = audio.samples[from..from + (secs * fs as f64) as usize].to_vec();
    sound.extend(tail);
    for (k, samples) in sound.chunks(block).enumerate() {
        let heard = (k * block) as f64 / fs as f64;
        if let Some((_, to)) = moved.take_if(|(at, _)| heard >= *at) {
            while app.poll_for(std::time::Duration::from_secs(60)) > 0 {}
            app.change_position(to);
        }
        let stalled = pause.filter(|&(at, _)| heard >= at).map_or(0.0, |(_, s)| s);
        let us = 3e6 + (heard / (1.0 + ppm * 1e-6) + stalled) * 1e6;
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_micros(us as u64);
        let samples = samples.to_vec();
        tx.send(timegrapher_core::capture::Event::Audio(
            timegrapher_core::capture::Block { samples, at },
        ))
        .unwrap();
    }
    while app.poll_for(std::time::Duration::from_secs(60)) > 0 {}
    app.stop();
    app
}

/// The measurement behind the Watches row of the take saved in `dir`:
/// its last position read as `library::reading` reads it, through core's
/// `take::measure_entry`, with its ±s and caveats.
fn row_measurement(dir: &Path) -> timegrapher_core::session::Measurement {
    use timegrapher_core::take;
    let text = std::fs::read_to_string(dir.join(take::MANIFEST)).unwrap();
    let m: timegrapher_core::session::Manifest = toml::from_str(&text).unwrap();
    let e = m.recordings.last().expect("a saved take has a position");
    let settings = take::EntrySettings {
        stream: take::stream_settings(&m, None, take::Given::default()),
        card_clock: false,
        settle_s: None,
    };
    let read = take::measure_entry(dir, &m, e, &settings, take::usb, |first| {
        take::correction_for(&m, first, None, None)
    });
    read.unwrap_or_else(|e| panic!("the saved take reads: {e}"))
        .measurement
}

/// Where the Watches row of the take saved in `dir` and its cards once
/// opened, at its end with the Average Over over the whole take, differ,
/// in words: figures, ±s, why one has none, caveats, and the beats and
/// span they rest on. `paused` says the take should hold a break.
fn row_against_cards(name: &str, dir: &Path, paused: bool) -> Vec<String> {
    let mut wrong = Vec::new();
    let row = timegrapher_core::library::reading(dir).unwrap();
    let x = row_measurement(dir);
    let mut app = opened(dir);
    app.set_average(60.0);
    // As the window does: the whole take read for the cards' hover.
    let _ = app.whole_take();
    let card = app.reading().expect("the opened take reads at its end");
    let shown_be =
        timegrapher_core::session::beat_error_signed(card.beat_error_unlock_ms, card.beat_error_ms);
    let c = x.confidence;
    let pairs = [
        ("rate", row.rate_s_per_day, card.rate_s_per_day),
        ("amplitude", row.amplitude_deg, card.amplitude_deg),
        ("beat error", row.beat_error_ms, shown_be),
        (
            "rate ±",
            c.and_then(|c| c.rate.se),
            card.rate_error_s_per_day,
        ),
        (
            "amplitude ±",
            c.and_then(|c| c.amplitude.se),
            card.amplitude_error_deg,
        ),
        (
            "beat error ±",
            c.and_then(|c| c.beat_error.se),
            card.beat_error_unlock_error_ms,
        ),
    ];
    for (what, row, card) in pairs {
        eprintln!("{name}: {what} {row:?} in the row, {card:?} on the cards");
        if row.is_none() && what.ends_with('±') {
            continue;
        }
        if row.is_none() || row != card {
            wrong.push(format!(
                "{name}: {what} {row:?} in the row, {card:?} on the cards"
            ));
        }
    }
    // The beats and seconds the figures rest on: the fit's own (Norm).
    let span = x.end_s - x.start_s - x.masked_s;
    if (x.beats_fitted, span) != (card.beats_used, card.span_s) {
        wrong.push(format!(
            "{name}: {} beats fitted over {span} s in the row, {} over {} on the cards",
            x.beats_fitted, card.beats_used, card.span_s
        ));
    }
    if paused && x.breaks.is_empty() {
        wrong.push(format!("{name}: the pause left no break"));
    }
    // Across the pause, the Rate hover names the whole take's beats, both
    // sides of it (Hugh 5 on #2075).
    let said = app.rate_beats_behind(Some(&card));
    let first = said.split("\n\n").next().unwrap_or_default().to_string();
    let whole = format!(
        "{} beats in {} of 1 min: the beats the readings are fitted to, fewer seconds than \
         Average Over asks for. They are the whole take after settling, fitted apart at each \
         break, masked stretch or gap of more than 2 s with no beats, with any piece under 10 s \
         left out.",
        x.beats_fitted,
        crate::fields::duration(span.round())
    );
    if paused && first != whole {
        wrong.push(format!(
            "{name}: the Rate hover says {first:?}, not {whole:?}"
        ));
    }
    let why = (
        c.and_then(|c| c.rate.no_pm),
        c.and_then(|c| c.amplitude.no_pm),
    );
    if why != (card.rate_no_pm, card.amplitude_no_pm) || x.caveats != card.caveats {
        wrong.push(format!(
            "{name}: no-± reasons {why:?} and caveats {:?} in the row, {:?} and {:?} \
             on the cards",
            x.caveats,
            (card.rate_no_pm, card.amplitude_no_pm),
            card.caveats
        ));
    }
    wrong
}

/// A saved take's Watches row and its cards once opened, at its end with
/// the Average Over over the whole take, give one rate, amplitude and
/// beat error, with their ±s, why one has none and the caveats, to the
/// last bit (#2073, Norm's acceptance (a)): the row is
/// `library::reading`, `session`'s reading of the take, and the cards read
/// the same stretch on the same clock. Each take is listened to as from a
/// microphone, on the card's own clock (0 ppm) and on one 25 ppm fast, so
/// a 50 s take's clock log corrects and a 5 s one's is withheld. Three
/// corpus takes, one of them again with a 5 s pause, and 5 s of the first
/// at four places; the cards' beats and span are the row's too. Needs only
/// `TIMEGRAPHER_CORPUS`.
#[test]
fn a_saved_takes_row_and_its_cards_give_one_reading() {
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(!required, "TIMEGRAPHER_CORPUS is needed");
        eprintln!("SKIP: set TIMEGRAPHER_CORPUS to read saved takes");
        return;
    };
    let file = |id: &str| root.join("takes").join(id).join("take_part01-001.flac");
    let first = file("eta2824_bare_DU_10min_g4");
    let mut cases = vec![
        (first.clone(), (60.0, 50.0), None),
        (
            file("pepsi3285_DU_10min_newstand_firsttest"),
            (60.0, 50.0),
            None,
        ),
        (
            file("peacock1258_DU_15min_lowwind_newstand"),
            (60.0, 50.0),
            None,
        ),
        // A take with a pause: the cards' beats are the whole stretch's,
        // not the latest unbroken run's (Norm on #2075).
        (first.clone(), (60.0, 50.0), Some((25.0, 5.0))),
    ];
    cases.extend([0.0, 30.0, 100.0, 300.0].map(|at| (first.clone(), (at, 5.0), None)));
    let mut wrong = Vec::new();
    for (k, (flac, stretch, pause)) in cases.iter().enumerate() {
        for ppm in [0.0, 25.0] {
            let name = format!(
                "{} {stretch:?} at {ppm} ppm, paused {pause:?}",
                flac.display()
            );
            let base = std::env::temp_dir()
                .join(format!("tg-one-reading-{k}-{ppm}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            let dir = listened_and_saved(&base, flac, *stretch, ppm, *pause, 0.0);
            wrong.extend(row_against_cards(&name, &dir, pause.is_some()));
            let _ = std::fs::remove_dir_all(&base);
        }
    }
    assert!(
        wrong.is_empty(),
        "the row and the cards differ:\n{}",
        wrong.join("\n")
    );
}

/// The app as the window runs it with a take's folder given at launch,
/// as `timegrapher-app <folder>` opens it: a fresh app, its settings at
/// their defaults, drawn frame by frame until the take is read, with the
/// text the frame painted.
fn window_opened(dir: &Path) -> (TimegrapherApp, egui::Context) {
    let mut app = TimegrapherApp::new(Some(dir.to_path_buf()), None);
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let t = Instant::now();
    loop {
        let _ = window_frame(&mut app, &ctx);
        if app.live.is_some() && app.batch.is_none() {
            break;
        }
        assert!(
            t.elapsed().as_secs() < OPEN_LIMIT_S,
            "{dir:?} still opening after {OPEN_LIMIT_S} s"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    (app, ctx)
}

/// One frame of the window, 1920 by 1080, and the text it painted.
fn window_frame(app: &mut TimegrapherApp, ctx: &egui::Context) -> String {
    let mut frame = eframe::Frame::_new_kittest();
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(1920.0, 1080.0),
            )),
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut frame),
    );
    super::tests::painted_text(&out)
}

/// A saved take whose watch was taken off the stand before Stop, opened
/// at launch as the window opens it: its cards at its end read the last
/// stretch with a watch heard, not "No Beats Heard" over the quiet, at
/// the default Average Over and at one the quiet outlasts (Vernon and
/// Walter 15 on #2075). Needs only `TIMEGRAPHER_CORPUS`.
#[test]
fn a_saved_takes_cards_read_past_a_silent_tail() {
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(!required, "TIMEGRAPHER_CORPUS is needed");
        eprintln!("SKIP: set TIMEGRAPHER_CORPUS to read saved takes");
        return;
    };
    // 30 s of a watch then a quiet stand: 9.7 s, as Walter 15's take,
    // just under Average Over, so the watch verdict still hears the watch
    // before it with no beat in the stretch; 10 s; 10 s with the capture
    // stalled 2 s at 31 s, so the quiet after that break still awaits its
    // verdict. And 70 s, longer than every Average Over (#2081). Takes the
    // PR corpus cut holds.
    let pepsi = "pepsi3285_DU_10min_newstand_firsttest";
    let eta = "eta2824_bare_DU_10min_g4";
    let both: &[f64] = &[10.0, 60.0];
    let takes = [
        (pepsi, 60.0, 9.7, None, both),
        (pepsi, 60.0, 10.0, None, both),
        (pepsi, 60.0, 10.0, Some((31.0, 2.0)), &[10.0][..]),
        (eta, 200.0, 10.0, None, both),
        (eta, 200.0, 70.0, None, both),
    ];
    for (k, (take, from_s, silent_s, pause, averages)) in takes.into_iter().enumerate() {
        let flac = root.join(format!("takes/{take}/take_part01-001.flac"));
        let base = std::env::temp_dir().join(format!("tg-silent-tail-{k}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = listened_and_saved(&base, &flac, (from_s, 30.0), 0.0, pause, silent_s);
        let (mut app, ctx) = window_opened(&dir);
        for &average in averages {
            app.set_average(average);
            // As the window has it once its background reads are in.
            let _ = app.whole_take();
            let painted = window_frame(&mut app, &ctx);
            let card = app.card_reading().expect("the opened take reads");
            let what =
                format!("{take}, {silent_s} s quiet, pause {pause:?}, Average Over {average} s");
            eprintln!("{what}: {:?} s/d at {} s", card.rate_s_per_day, card.time_s);
            assert!(
                !painted.contains("No Beats Heard"),
                "{what}: the cards say No Beats Heard"
            );
            assert!(
                card.watch_heard && card.listening.is_none() && card.rate_s_per_day.is_some(),
                "{what}: no figures on the cards"
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}

/// A take ending in a quiet stand's hiss, just stopped and opened again at
/// launch: where Average Over doesn't hold the whole take, the cards read
/// at the watch's last beat (core's `beats::last_watch_beat_s`), bit for
/// bit, and that beat is the watch's, within one beat before the hiss
/// starts, never a stray one in it (Norm's ruling and acceptance on
/// #2081). Needs only `TIMEGRAPHER_CORPUS`.
#[test]
fn a_stopped_takes_cards_read_to_the_watchs_last_beat() {
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(!required, "TIMEGRAPHER_CORPUS is needed");
        eprintln!("SKIP: set TIMEGRAPHER_CORPUS to read saved takes");
        return;
    };
    // 30 s of the watch, then the hiss.
    const SOUND_S: f64 = 30.0;
    let pepsi = "pepsi3285_DU_10min_newstand_firsttest";
    let eta = "eta2824_bare_DU_10min_g4";
    let takes = [
        (pepsi, 60.0, 9.7, &[10.0][..]),
        (pepsi, 60.0, 10.0, &[10.0][..]),
        (eta, 200.0, 70.0, &[10.0, 60.0][..]),
    ];
    let mut wrong = Vec::new();
    for (k, (take, from_s, silent_s, averages)) in takes.into_iter().enumerate() {
        let flac = root.join(format!("takes/{take}/take_part01-001.flac"));
        let base = std::env::temp_dir().join(format!("tg-last-beat-{k}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut stopped = listened(&base, &flac, (from_s, SOUND_S), 0.0, None, silent_s);
        stop_read_in(&mut stopped);
        let (mut opened, ctx) = window_opened(&saved(&mut stopped, &base));
        for &average in averages {
            for (name, app) in [("just stopped", &mut stopped), ("opened", &mut opened)] {
                let what = format!("{take}, {silent_s} s quiet, Average Over {average} s, {name}");
                app.set_average(average);
                let _ = app.whole_take();
                let painted = window_frame(app, &ctx);
                // The engine the cards read: a take just stopped is read as
                // its saved take opens (#2151).
                let live = app
                    .stopped_take()
                    .map_or(app.live.as_ref().unwrap(), |s| &s.live);
                let t = timegrapher_core::beats::last_watch_beat_s(live.beats())
                    .expect("the watch's run is long enough");
                let want = live.reading_at(t, average);
                let got = app.card_reading().expect("the take reads");
                eprintln!("{what}: {:?} s/d at {} s", got.rate_s_per_day, got.time_s);
                wrong.extend(last_beat_differences(&what, &got, &want, &painted));
                let beat_s = 3600.0 / f64::from(got.bph.unwrap_or(0));
                if !(SOUND_S - beat_s <= t && t <= SOUND_S) {
                    wrong.push(format!(
                        "{what}: the last beat at {t} s, not within {beat_s} s before {SOUND_S} s"
                    ));
                }
            }
        }
        let _ = std::fs::remove_dir_all(&base);
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A watch heard live for a while, then the stand's own noise after its
/// watch stopped, then Stop, as a watchmaker lifts the watch and stops a
/// minute later (#2112, #2114). The noise is no watch: the cards keep the
/// beat rate the watch was heard at, with figures read at it, and the
/// Positions Test's This Take table keeps its row. Needs only
/// `TIMEGRAPHER_CORPUS`.
#[test]
fn a_take_stopped_after_stand_noise_keeps_its_watchs_rate_and_table() {
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(!required, "TIMEGRAPHER_CORPUS is needed");
        eprintln!("SKIP: set TIMEGRAPHER_CORPUS to listen to corpus takes");
        return;
    };
    let watch = root.join("takes/pepsi3285_DU_10min_newstand_firsttest/take_part01-001.flac");
    // The stand after the YM42 stopped: its hum and hiss, no watch.
    let stand = root.join("takes/ym42_DU_65h_excerpt_dying/take_part01-005.flac");
    let stand = timegrapher_core::audio::load(&stand).unwrap();
    let fs = timegrapher_core::audio::load(&watch).unwrap().sample_rate;
    assert_eq!(
        stand.sample_rate, fs,
        "both takes are heard at one sample rate"
    );
    // The stand's noise locked on to as a 72,000 bph "watch", as Shane's
    // first take did 1 time in 3: 8 s of the stand, then 5 s of clicks at
    // 72,000, so Auto lets the watch's rate go and locks on to them.
    let hum = timegrapher_core::synth::generate(
        &timegrapher_core::synth::SynthConfig {
            bph: 72_000,
            duration_s: 5.0,
            sample_rate: fs,
            ..Default::default()
        },
        |_| 260.0,
        |_| 0.0,
    );
    let mut locked = stand.samples[..(8.0 * f64::from(fs)) as usize].to_vec();
    locked.extend_from_slice(&hum.samples);
    let tails = [
        (150.0, "the stand", stand.samples.clone()),
        (90.0, "the stand", stand.samples.clone()),
        (150.0, "the stand locked on to", locked),
    ];
    let ctx = super::tests::themed();
    let mut wrong = Vec::new();
    for (k, (secs, tail, sound)) in tails.into_iter().enumerate() {
        let base = std::env::temp_dir().join(format!("tg-stand-tail-{k}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = listened_then(&base, &watch, (0.0, secs), 0.0, None, sound);
        app.set_average(60.0);
        let what = format!("{secs} s of the watch, then {tail}");
        let card = app.card_reading().expect("the take reads");
        eprintln!(
            "{what}: {:?} bph, {:?} s/d at {} s, Auto {:?}",
            card.bph,
            card.rate_s_per_day,
            card.time_s,
            app.bph_heard()
        );
        if app.bph_heard() != Some(28800) {
            wrong.push(format!("{what}: Auto {:?}, not 28800", app.bph_heard()));
        }
        if !card.rate_s_per_day.is_some_and(|r| r.abs() < 60.0) {
            wrong.push(format!("{what}: rate {:?} s/d", card.rate_s_per_day));
        }
        // The watch's rate in the Dial Up row, as on the cards.
        let row = app.free_table(&ctx).and_then(|t| {
            t.rows.into_iter().find_map(|r| match r.2 {
                guided::FreeCell::Read(rate, ..) => Some(rate),
                _ => None,
            })
        });
        eprintln!("{what}: the This Take row reads {row:?} s/d");
        match row {
            None => wrong.push(format!("{what}: no This Take row")),
            Some(rate) if !rate.is_some_and(|r| r.abs() < 60.0) => {
                wrong.push(format!("{what}: the This Take row reads {rate:?} s/d"));
            }
            Some(_) => {}
        }
        let _ = std::fs::remove_dir_all(&base);
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// How the cards' reading `got` differs from the reading at the watch's
/// last beat, `want`, bit for bit: its time, figures and what the cards
/// painted.
fn last_beat_differences(
    what: &str,
    got: &LiveReading,
    want: &LiveReading,
    painted: &str,
) -> Vec<String> {
    let mut wrong = Vec::new();
    if painted.contains("No Beats Heard") || got.rate_s_per_day.is_none() {
        wrong.push(format!("{what}: no figures on the cards"));
    }
    let pairs = [
        ("time", Some(got.time_s), Some(want.time_s)),
        ("rate", got.rate_s_per_day, want.rate_s_per_day),
        ("amplitude", got.amplitude_deg, want.amplitude_deg),
        ("beat error", got.beat_error_ms, want.beat_error_ms),
        (
            "unlock beat error",
            got.beat_error_unlock_ms,
            want.beat_error_unlock_ms,
        ),
    ];
    for (field, g, w) in pairs {
        if g.map(f64::to_bits) != w.map(f64::to_bits) {
            wrong.push(format!(
                "{what}: {field} {g:?} on the cards, {w:?} at the last beat"
            ));
        }
    }
    wrong
}

/// The take just stopped, once its read as the saved take opens is in.
fn stop_read_in(app: &mut TimegrapherApp) {
    let t = Instant::now();
    while app.stop_read_waits().is_some() {
        app.poll_stop_read();
        assert!(
            t.elapsed().as_secs() < OPEN_LIMIT_S,
            "the Stop read is still under way"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    app.poll_stop_read();
    assert!(
        app.stopped_take().is_some(),
        "the take stopped is read whole"
    );
}

/// The figures a card shows from `r`, every one to the bit: Norm's
/// acceptance on #2151.
fn card_bits(r: &LiveReading) -> String {
    format!(
        "t {:?} rate {:?}±{:?} amp {:?}±{:?} be {:?}/{:?}±{:?} beats {} span {:?} whole {} caveats {:?}",
        r.time_s.to_bits(),
        r.rate_s_per_day.map(f64::to_bits),
        r.rate_error_s_per_day.map(f64::to_bits),
        r.amplitude_deg.map(f64::to_bits),
        r.amplitude_error_deg.map(f64::to_bits),
        r.beat_error_ms.map(f64::to_bits),
        r.beat_error_unlock_ms.map(f64::to_bits),
        r.beat_error_unlock_error_ms.map(f64::to_bits),
        r.beats_used,
        r.span_s.to_bits(),
        r.whole_take,
        r.caveats,
    )
}

/// A take stopped reads as the same take saved and opened, to the bit, at
/// Average Over 10 s and 60 s (Norm's acceptance on #2151): before the
/// template is built again (38.36 s), after it (73 s and 5 min), and under
/// 10 s. Until it is read, the cards show the open's wait and no figure.
#[test]
fn a_stopped_takes_cards_are_its_saved_takes_to_the_bit() {
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(!required, "TIMEGRAPHER_CORPUS is needed");
        return eprintln!("skipped: TIMEGRAPHER_CORPUS isn't set");
    };
    let flac = root.join("takes/eta2824_holder_DU_30min/take_part01-001.flac");
    if !flac.exists() {
        assert!(!required, "{flac:?} is needed");
        return eprintln!("skipped: {flac:?} isn't here");
    }
    for secs in [38.36, 73.0, 8.0, 300.0] {
        let base = std::env::temp_dir().join(format!("tg-2151-{secs}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut stopped = listened(&base, &flac, (0.0, secs), 0.0, None, 0.0);
        assert!(
            stopped.stop_read_waits().is_some(),
            "{secs} s: the cards wait at Stop"
        );
        assert!(
            stopped.shown_on_cards().is_none(),
            "{secs} s: no figure on the cards while it is read"
        );
        // The panes keep what they showed: nothing withholds with the
        // cards while they wait (Walter 21).
        assert!(
            stopped.card_reading().is_some() && (secs < 30.0 || stopped.cards_unheard().is_none()),
            "{secs} s: the panes withhold while it is read"
        );
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let text = window_frame(&mut stopped, &ctx);
        assert!(text.contains("Analysing the take"), "{secs} s: {text}");
        assert!(
            !text.contains("s/d"),
            "{secs} s: a figure while it is read: {text}"
        );
        // The strip, unpinned, waits as the cards do (Trevor on #2164).
        let live = stopped.card_reading().expect("live reads while it waits");
        if let Some(rate) = readouts::rate_figure(&live) {
            stopped.readings_pinned = false;
            let strip = window_frame(&mut stopped, &ctx);
            stopped.readings_pinned = true;
            let digits = rate.trim_start_matches(['+', '-', '\u{2212}']);
            assert!(
                strip.contains("Rate") && !strip.contains(digits),
                "{secs} s: the strip shows live's {rate} while it is read: {strip}"
            );
        }
        // The window's frames while it is read, and after (Gordon).
        let waited = Instant::now();
        let mut waiting = Vec::new();
        while stopped.stop_read_waits().is_some() {
            let f = Instant::now();
            let _ = window_frame(&mut stopped, &ctx);
            waiting.push(f.elapsed().as_secs_f64() * 1e3);
            assert!(
                waited.elapsed().as_secs() < OPEN_LIMIT_S,
                "{secs} s: still read"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let read_s = waited.elapsed().as_secs_f64();
        stop_read_in(&mut stopped);
        let after: Vec<f64> = (0..30)
            .map(|_| {
                let f = Instant::now();
                let _ = window_frame(&mut stopped, &ctx);
                f.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        let spread = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            (
                v.get(v.len() / 2).copied().unwrap_or(0.0),
                v.last().copied().unwrap_or(0.0),
            )
        };
        eprintln!(
            "{secs} s stopped: read as saved in {read_s:.2} s; frames while read {:?} ms, after {:?} ms (median, worst)",
            spread(waiting),
            spread(after)
        );
        let (mut opened, ctx) = window_opened(&saved(&mut stopped, &base));
        for average in [10.0, 60.0] {
            let mut bits = Vec::new();
            for app in [&mut stopped, &mut opened] {
                app.set_average(average);
                // As the window does: its frames, then the whole take read
                // for the cards.
                for _ in 0..2 {
                    let _ = window_frame(app, &ctx);
                }
                let _ = app.whole_take();
                bits.push(card_bits(&app.card_reading().expect("a reading")));
            }
            assert_eq!(
                bits[0], bits[1],
                "{secs} s over {average} s: stopped, then opened"
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}

/// A stopped take's whole take and its cards' sparklines are its saved
/// take's too (Trevor on #2164): with a move and then a break in the
/// take, so the saved sound's time differs from the session's where the
/// position's stretch starts, and a mask put on once the take is read,
/// which the stopped take takes as an opened one does. The whole take is
/// read before the save, as the stopped take; sparklines read while it
/// waits are read again once it lands. More sound kept after the read
/// drops it.
#[test]
fn a_stopped_takes_whole_take_and_sparklines_are_its_saved_takes() {
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(!required, "TIMEGRAPHER_CORPUS is needed");
        return eprintln!("skipped: TIMEGRAPHER_CORPUS isn't set");
    };
    let flac = root.join("takes/eta2824_holder_DU_30min/take_part01-001.flac");
    if !flac.exists() {
        assert!(!required, "{flac:?} is needed");
        return eprintln!("skipped: {flac:?} isn't here");
    }
    let base = std::env::temp_dir().join(format!("tg-2164-whole-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    // Moved to Crown Up 10 s in, then a 4 s stall 20 s in, inside its
    // settling: a break, so session and saved sound part, and the
    // position's settled stretch starts at a different second in each.
    let moved = Some((10.0, Position::CrownUp));
    let pause = Some((20.0, 4.0));
    let mut stopped = listened_moved(&base, &flac, (0.0, 90.0), 0.0, pause, Vec::new(), moved);
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    // Sparklines read while it waits are not kept past it.
    stopped.set_average(10.0);
    let waiting = stopped.card_reading().expect("live reads while it waits");
    let waited = format!("{:?}", stopped.spark_trend(&ctx, &waiting));
    stop_read_in(&mut stopped);
    let r = stopped.card_reading().expect("a reading");
    let landed = format!("{:?}", stopped.spark_trend(&ctx, &r));
    assert_ne!(landed, waited, "the sparklines read live's, not the take's");
    stopped.set_average(60.0);
    let _ = stopped.spark_trend(&ctx, &r);
    stopped.set_average(10.0);
    assert_eq!(
        format!("{:?}", stopped.spark_trend(&ctx, &r)),
        landed,
        "the sparklines kept from the wait"
    );
    // A mask once it is read: retuned, as an opened take's engine is.
    stopped.put_masked(vec![(50.0, 56.0)]);
    // What the stopped take shows, read before it is saved: saved, it
    // reads as the take opened.
    let shown = |app: &mut TimegrapherApp, ctx: &egui::Context| {
        for _ in 0..2 {
            let _ = window_frame(app, ctx);
        }
        let r = app.card_reading().expect("a reading");
        let whole = app.whole_take().expect("the whole take");
        (
            card_bits(&r),
            format!("{:?} {:?}", whole.m, whole.span.to_bits()),
            format!("{:?}", app.spark_trend(ctx, &r)),
        )
    };
    let mut seen = vec![shown(&mut stopped, &ctx)];
    assert!(stopped.stopped_take().is_some(), "read as the stopped take");
    let (mut opened, ctx) = window_opened(&saved(&mut stopped, &base));
    // The saved take keeps the mask, in its sound's own time: 4 s sooner,
    // after the break.
    assert_eq!(opened.masked, vec![(46.0, 52.0)], "the saved take's mask");
    opened.set_average(10.0);
    seen.push(shown(&mut opened, &ctx));
    let (s, o) = (&seen[0], &seen[1]);
    assert_eq!(s.0, o.0, "the cards");
    assert_eq!(s.1, o.1, "the whole take");
    assert_eq!(s.2, o.2, "the sparklines");
    // More sound kept after the read: it no longer holds.
    let short = base.join("short");
    let mut more = listened(&short, &flac, (0.0, 8.0), 0.0, None, 0.0);
    stop_read_in(&mut more);
    let block = timegrapher_core::capture::Block {
        samples: vec![0.0; 48_000],
        at: std::time::SystemTime::now(),
    };
    let r = more.session.recorder.as_mut().expect("the recorder");
    r.write(&block).expect("written");
    assert!(more.stopped_take().is_none(), "a read of less sound holds");
    let _ = std::fs::remove_dir_all(&base);
}

/// Walter 17's splice on #2133: 25 s of the ETA 2824 in its holder (28800)
/// straight into the Reverso (21600), heard live. In the seconds after it,
/// while live has lost its sides, Beat Error shows the read's figure, the
/// saved take's, and never a bare dash (Norm and Hugh 5).
#[test]
fn walters_splice_shows_the_reads_beat_error() {
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(!required, "TIMEGRAPHER_CORPUS is needed");
        return eprintln!("skipped: TIMEGRAPHER_CORPUS isn't set");
    };
    let load = |id: &str| {
        let flac = root.join("takes").join(id).join("take_part01-001.flac");
        flac.exists()
            .then(|| timegrapher_core::audio::load(&flac).unwrap())
    };
    let (Some(eta), Some(reverso)) = (
        load("eta2824_holder_DU_30min"),
        load("reverso21600_DU_20min_newstand"),
    ) else {
        assert!(
            !required,
            "the ETA holder and Reverso takes' sound is needed"
        );
        return eprintln!("skipped: the takes' sound isn't here");
    };
    let fs = eta.sample_rate as usize;
    let first = &eta.samples[..25 * fs];
    let splice = &reverso.samples[..8 * fs];
    let (read_sides, worded) = super::readouts_tests::beat_error_after(first, splice);
    eprintln!(
        "{read_sides} frames showed the read's beat error with live's sides lost, {worded} worded"
    );
    assert!(
        read_sides > 0,
        "Walter's moments: the read's beat error, live's sides lost"
    );
}

/// A take made of corpus sound, opened: each part is a take's first
/// file from `from` to `to` seconds, one after another, as a watch put on
/// and taken off the stand is heard. None without `TIMEGRAPHER_CORPUS`.
fn spliced_take(name: &str, parts: &[(&str, usize, usize)]) -> Option<TimegrapherApp> {
    let required = std::env::var_os("TIMEGRAPHER_REQUIRE_CORPUS").is_some();
    let Some(root) = std::env::var_os("TIMEGRAPHER_CORPUS").map(PathBuf::from) else {
        assert!(!required, "TIMEGRAPHER_CORPUS is needed");
        eprintln!("SKIP: set TIMEGRAPHER_CORPUS to open {name}");
        return None;
    };
    let file = |take: &str| root.join("takes").join(take).join("take_part01-001.flac");
    let mut sound: Option<timegrapher_core::audio::Audio> = None;
    for &(take, from, to) in parts {
        let a = timegrapher_core::audio::load(&file(take)).unwrap();
        let fs = a.sample_rate as usize;
        let part = &a.samples[from * fs..to * fs];
        match &mut sound {
            Some(s) => s.samples.extend_from_slice(part),
            None => {
                sound = Some(timegrapher_core::audio::Audio {
                    samples: part.to_vec(),
                    sample_rate: a.sample_rate,
                })
            }
        }
    }
    let dir = std::env::temp_dir().join(format!("tg-app-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    timegrapher_core::audio::write_wav(&dir.join("take.wav"), &sound.unwrap()).unwrap();
    std::fs::write(
        dir.join(timegrapher_core::take::MANIFEST),
        "bph = 28800\nlift = 55\n\n[[recording]]\nfile = \"take.wav\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let app = opened(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    Some(app)
}

/// Shane's take (#2177): 70 s of `silence_empty_stand`, 100 s of
/// `ym42_DU_2h`, 60 s of the stand. `name` keeps each test's folder
/// its own.
fn stand_both_ends(name: &str) -> Option<TimegrapherApp> {
    spliced_take(
        name,
        &[
            ("silence_empty_stand", 0, 70),
            ("ym42_DU_2h", 0, 100),
            ("silence_empty_stand", 100, 160),
        ],
    )
}

/// The stretches the strip draws as possible beats.
fn possible_beats(app: &TimegrapherApp) -> Vec<(f64, f64)> {
    app.strip_faint(None)
        .into_iter()
        .filter(|s| s.2 == "Possible Beats")
        .map(|s| (s.0, s.1))
        .collect()
}

/// The largest lead, ms, of the beats from `from` to `to` s on the strip
/// as it is anchored now.
fn widest_lead(app: &TimegrapherApp, from: f64, to: f64) -> f64 {
    let live = app.live.as_ref().unwrap();
    let period = 3600.0 / live.bph().unwrap() as f64;
    let anchor = app.anchor.expect("anchored");
    let breaks = app.take_breaks.as_deref().unwrap_or_default();
    live.beats()
        .iter()
        .filter(|b| b.time > from && b.time < to)
        .map(|b| {
            anchor
                .lead_on_true(b, period, breaks, app.take_rate_clock())
                .abs()
        })
        .fold(0.0, f64::max)
}

/// The empty stand, the Yacht-Master put on it, then the stand again, as
/// Shane played them (#2177), opened. The stand's beats, where no
/// reading hears a watch (core's `live::unheard_spans`), are drawn as
/// possible beats, not Tick and Tock; Fit spans the watch's beats alone,
/// not the noise's scattered across the strip, which widened it to
/// +/-500 ms; and the whole take's "over" is the stretch it read, the
/// stand's lead left out, not the whole 230 s. Needs only
/// `TIMEGRAPHER_CORPUS`.
#[test]
fn the_stand_at_both_ends_is_possible_beats_and_left_out_of_fit() {
    let Some(app) = stand_both_ends("stand-ends") else {
        return;
    };
    let live = app.live.as_ref().unwrap();
    let faint = possible_beats(&app);
    let possible = |t: f64| timegrapher_core::live::in_spans(&faint, t);
    let noise = live
        .beats()
        .iter()
        .filter(|b| b.time < 65.0 || b.time > 175.0);
    assert!(noise.clone().count() > 50, "the stand has beats to draw");
    assert!(noise.clone().all(|b| possible(b.time)), "{faint:?}");
    let on = live
        .beats()
        .iter()
        .filter(|b| b.time > 85.0 && b.time < 165.0);
    assert!(!on.clone().any(|b| possible(b.time)), "{faint:?}");
    // About +50 s/d drifts some 60 ms over the watch's 100 s.
    assert!(
        app.strip.half_width_ms <= 100.0,
        "{}",
        app.strip.half_width_ms
    );
    let w = app.whole_take().expect("whole take");
    // Read from the watch's start, after the stand's 70 s that the reading
    // leaves out, to the take's end, as `session` reads it (#2073).
    assert!(w.span > 150.0 && w.span < 165.0, "over {} s", w.span);
    let rate = w.m.rate_s_per_day.unwrap();
    assert!((rate - 50.0).abs() < 10.0, "{rate}");
    assert_eq!(w.span, w.m.measured_s());
}

/// The watch, the stand for 30 s in its place, then the watch again, its
/// beat running on through (Trevor on #2192): the stand in the middle is
/// possible beats, and Fit's width holds the watch's beats on both sides
/// of it, not the noise's between them.
#[test]
fn the_stand_between_two_stretches_of_the_watch_is_left_out_of_fit() {
    let Some(app) = spliced_take(
        "stand-middle",
        &[
            ("ym42_DU_2h", 0, 60),
            ("silence_empty_stand", 0, 30),
            ("ym42_DU_2h", 90, 150),
        ],
    ) else {
        return;
    };
    let faint = possible_beats(&app);
    assert!(
        timegrapher_core::live::in_spans(&faint, 75.0),
        "the stand in the middle: {faint:?}"
    );
    assert!(
        app.strip.half_width_ms <= 100.0,
        "{}",
        app.strip.half_width_ms
    );
}

/// With Auto-Centre on, Fit and the centre the strip keeps put the
/// watch's last beats on the centre line: the stand's beats after them,
/// which the beat finder keeps near the watch's beat line, don't move it
/// (Trevor on #2192).
#[test]
fn auto_centre_centres_on_the_watch_not_the_stand_after_it() {
    let Some(mut app) = stand_both_ends("stand-follow") else {
        return;
    };
    app.follow = true;
    app.fit_take();
    // The watch's last seconds lie near the centre line.
    let fitted = widest_lead(&app, 165.0, 170.0);
    assert!(fitted < 20.0, "Fit with Auto-Centre: {fitted} ms");
    app.anchor = None;
    app.centre();
    let centred = widest_lead(&app, 165.0, 170.0);
    assert!(centred < 20.0, "Auto-Centre: {centred} ms");
}

/// A take whose charts' run is read after it opened, in the background
/// as the window reads it: Fit is made again once the run says where the
/// watch is, and a view changed by hand in between is kept (Trevor on
/// #2192).
#[test]
fn fit_is_made_again_when_the_run_comes_after_the_take_opens() {
    let Some(mut app) = stand_both_ends("stand-refit") else {
        return;
    };
    let ctx = egui::Context::default();
    let read_again = |app: &mut TimegrapherApp| {
        // The run not read yet, as for a take opened without one.
        app.trend.clear();
        app.trend_key = (app.rev, 0);
        app.fit_take();
        let t = Instant::now();
        while app.trend.is_empty() && t.elapsed().as_secs() < 120 {
            app.take_reads_shown(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!app.trend.is_empty(), "the run came");
    };
    read_again(&mut app);
    assert!(
        app.strip.half_width_ms <= 100.0,
        "fitted again: {}",
        app.strip.half_width_ms
    );
    // Fitted before the run, with every beat; then set by hand.
    app.trend.clear();
    app.fit_take();
    let wide = app.strip.half_width_ms;
    assert!(wide > 100.0, "every beat: {wide}");
    app.strip.set_half_width(wide / 2.0);
    let by_hand = app.strip.half_width_ms;
    app.trend_key = (app.rev, 0);
    let t = Instant::now();
    while app.trend.is_empty() && t.elapsed().as_secs() < 120 {
        app.take_reads_shown(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(app.strip.half_width_ms, by_hand, "kept as set by hand");
}

/// While listening, the newest beats have no reading yet, so nothing is
/// drawn as possible beats for want of one (#2192).
#[test]
fn nothing_is_possible_beats_for_want_of_a_reading_while_listening() {
    let Some(mut app) = stand_both_ends("stand-listening") else {
        return;
    };
    assert!(!possible_beats(&app).is_empty(), "opened: the stand");
    let base = std::env::temp_dir().join(format!("tg-app-listening-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let _tx = super::tests::queued_capture(&mut app, &base);
    assert!(app.running());
    assert!(app.strip_unheard().is_empty());
    assert!(
        possible_beats(&app).is_empty(),
        "{:?}",
        possible_beats(&app)
    );
    let _ = std::fs::remove_dir_all(&base);
}
