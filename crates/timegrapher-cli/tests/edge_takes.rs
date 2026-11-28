//! Each synthetic edge take read by `timegrapher session --json` reads what
//! core reads for it ([`edge_takes::read`]), to the last bit, with the same
//! caveats: the command line adds no logic of its own to an awkward take.
//! And `doctor --file` finishes on every sound file of every edge take,
//! and `record` fed an edge take's sound checks it as `session` then reads
//! the take it saved.
//!
//! One test makes the takes once and runs the checks on them: making them
//! is about a third of the work, and each test of its own made them again
//! (#907). The takes are checked side by side, one to a core (#2442).

use serde_json::Value;
use std::path::Path;
use std::process::Command;
use timegrapher_core::edge_takes::{self, EdgeTake};

#[test]
fn session_and_doctor_on_each_edge_take() {
    let root = std::env::temp_dir().join(format!("tg-cli-edge-takes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let takes = edge_takes::write_all(&root).unwrap();
    let (wrong, panicked) = check_side_by_side(&root, &takes);
    std::fs::remove_dir_all(&root).ok();
    assert!(panicked.is_empty(), "panicked: {}", panicked.join(", "));
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The three checks on every take in `takes`, one take to a thread and
/// one thread to a core: each take's commands are their own processes,
/// on their own folders, so the takes check side by side as they did
/// one after another (#2442). Returns what differs, in the order of
/// `takes`, and the takes whose check panicked: each thread is named
/// after its take, so the panic's own message names it too.
fn check_side_by_side(root: &Path, takes: &[EdgeTake]) -> (Vec<String>, Vec<String>) {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let next = &std::sync::atomic::AtomicUsize::new(0);
    let mut found: Vec<(usize, Result<Vec<String>, ()>)> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads.min(takes.len()))
            .map(|_| {
                s.spawn(move || {
                    let mut done = Vec::new();
                    loop {
                        let n = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(t) = takes.get(n) else {
                            return done;
                        };
                        let one = std::thread::Builder::new()
                            .name(t.id.to_string())
                            .spawn_scoped(s, move || {
                                let mut wrong = Vec::new();
                                session_reads_as_core_does(root, t, &mut wrong);
                                doctor_finishes(root, t);
                                record_checks_as_session_reads(root, t, &mut wrong);
                                wrong
                            })
                            .unwrap();
                        done.push((n, one.join().map_err(|_| ())));
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect()
    });
    found.sort_by_key(|(n, _)| *n);
    let mut wrong = Vec::new();
    let mut panicked = Vec::new();
    for (n, f) in found {
        match f {
            Ok(w) => wrong.extend(w),
            Err(()) => panicked.push(takes[n].id.to_string()),
        }
    }
    (wrong, panicked)
}

/// `timegrapher session --json` reads the take `t` as core does: what
/// differs goes in `wrong`.
fn session_reads_as_core_does(root: &Path, t: &EdgeTake, wrong: &mut Vec<String>) {
    let core = edge_takes::read(root, t).unwrap_or_else(|e| panic!("{e}"));
    let out = root.join("out").join(t.id);
    let run = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .arg("session")
        .arg(root.join("takes").join(t.id))
        .args(["--json", "--out"])
        .arg(&out)
        .env("TIMEGRAPHER_CONFIG_DIR", root.join("config"))
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}: {}",
        t.id,
        String::from_utf8_lossy(&run.stderr)
    );
    let doc: Value = serde_json::from_slice(&run.stdout).unwrap();
    let readings = doc["readings"].as_array().cloned().unwrap_or_default();
    if readings.len() != core.len() {
        wrong.push(format!(
            "{}: {} readings from session, {} entries in core",
            t.id,
            readings.len(),
            core.len()
        ));
        return;
    }
    for (n, (reading, read)) in readings.iter().zip(&core).enumerate() {
        let c = &read.measurement;
        let id = format!("{} #{}", t.id, n + 1);
        let m = &reading["measurement"];
        let mut same = |what: &str, core: Option<f64>, key: &str| {
            let cli = m[key].as_f64();
            if core != cli {
                wrong.push(format!(
                    "{id}: {what} {core:?} in core, {cli:?} from session"
                ));
            }
        };
        same("rate", c.rate_s_per_day, "rate_s_per_day");
        same("amplitude", c.amplitude_deg, "amplitude_deg");
        same("beat error", c.beat_error_unlock_ms, "beat_error_unlock_ms");
        same("start", Some(c.start_s), "start_s");
        same("end", Some(c.end_s), "end_s");
        same("beats", Some(c.beats as f64), "beats");
        let codes: Vec<String> = m["caveats"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["code"].as_str().map(str::to_string))
            .collect();
        let core_codes = edge_takes::caveat_codes(c);
        if codes != core_codes {
            wrong.push(format!(
                "{id}: caveats {core_codes:?} in core, {codes:?} from session"
            ));
        }
        eprintln!("{id}: {codes:?}");
    }
}

/// The edge take `t`'s first sound, when it has one, fed to `record` as its input: the check
/// `record --json` gives at the end is what `session` reads of the take it
/// saved, to the last bit. What differs goes in `wrong`.
fn record_checks_as_session_reads(root: &Path, t: &EdgeTake, wrong: &mut Vec<String>) {
    let tg = |args: &[&std::ffi::OsStr]| -> Value {
        let run = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
            .args(args)
            .env("TIMEGRAPHER_CONFIG_DIR", root.join("config"))
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        serde_json::from_slice(&run.stdout).unwrap()
    };
    let file = root.join("takes").join(t.id).join("audio-001.wav");
    if !file.is_file() {
        // A take kept as its analysis alone has no sound to record.
        return;
    }
    let into = root.join("recorded").join(t.id);
    let rec = tg(&[
        "record".as_ref(),
        into.as_os_str(),
        "--position=DU".as_ref(),
        "--lift=52".as_ref(),
        format!("--file={}", file.display()).as_ref(),
        "--json".as_ref(),
    ]);
    let saved = std::path::PathBuf::from(rec["take"].as_str().expect("the take saved"));
    let out = root.join("out-recorded").join(t.id);
    let doc = tg(&[
        "session".as_ref(),
        saved.as_os_str(),
        "--json".as_ref(),
        "--out".as_ref(),
        out.as_os_str(),
    ]);
    let c = &rec["check"];
    let m = &doc["readings"][0]["measurement"];
    for (what, check, session) in [
        ("from", "from_s", "start_s"),
        ("to", "to_s", "end_s"),
        ("beats", "beats_found", "beats"),
        ("watch heard", "watch_heard", "watch_heard"),
        ("rate", "rate_s_per_day", "rate_s_per_day"),
        ("amplitude", "amplitude_deg", "amplitude_deg"),
    ] {
        if c[check] != m[session] {
            wrong.push(format!(
                "{}: {what} {} from record, {} from session",
                t.id, c[check], m[session]
            ));
        }
    }
    // The timing scatter, ms in the check, microseconds in the reading.
    let scatter_us = c["timing_scatter_ms"].as_f64().map(|ms| ms * 1000.0);
    if let Some(us) = m["jitter_us"].as_f64() {
        if scatter_us.is_none_or(|s| (s - us).abs() > 1e-9 * us.abs().max(1.0)) {
            wrong.push(format!(
                "{}: timing scatter {scatter_us:?} us from record, {us} from session",
                t.id
            ));
        }
    }
    // The beat error a reading shows: the unlock's, else the drop's.
    let shown = timegrapher_core::session::beat_error_signed(
        m["beat_error_unlock_ms"].as_f64(),
        m["beat_error_ms"].as_f64(),
    );
    if c["beat_error_ms"].as_f64() != shown {
        wrong.push(format!(
            "{}: beat error {} from record, {shown:?} from session",
            t.id, c["beat_error_ms"]
        ));
    }
}

/// `doctor --file` finishes on every sound file of the take `t`, and reads
/// the watch either side of the digital silence (#782): it used to run for
/// ever there.
fn doctor_finishes(root: &Path, t: &EdgeTake) {
    let dir = root.join("takes").join(t.id);
    let mut wavs: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "wav"))
        .collect();
    wavs.sort();
    for wav in wavs {
        let mut child = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
            .arg("doctor")
            .arg("--file")
            .arg(&wav)
            .arg("--json")
            .env("TIMEGRAPHER_CONFIG_DIR", root.join("config"))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let start = std::time::Instant::now();
        while child.try_wait().unwrap().is_none() {
            if start.elapsed().as_secs() > 120 {
                child.kill().ok();
                panic!("{}: doctor still running after 120 s", wav.display());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let out = child.wait_with_output().unwrap();
        let doc: Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{}: {e}", wav.display()));
        if t.id == "edge_digital_silence" {
            let rate = doc["check"]["rate_s_per_day"].as_f64();
            // The edge take's watch is made at +5 s/d.
            assert!(
                rate.is_some_and(|r| (r - 5.0).abs() < 0.5),
                "{}: rate {rate:?}",
                t.id
            );
        }
    }
}
