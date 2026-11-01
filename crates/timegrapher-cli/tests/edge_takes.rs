//! Each synthetic edge take read by `timegrapher session --json` reads what
//! core reads for it ([`edge_takes::read`]), to the last bit, with the same
//! caveats: the command line adds no logic of its own to an awkward take.

use serde_json::Value;
use std::process::Command;
use timegrapher_core::edge_takes;

/// `f` on each of `items`, on as many threads as the machine has cores,
/// the results in the items' order. Each edge take is read on its own, so
/// they need not wait for one another (#804).
fn on_each<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(items.len().max(1));
    let mut out: Vec<(usize, R)> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        done.push((i, f(item)));
                    }
                    done
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    });
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, r)| r).collect()
}

#[test]
fn session_reads_each_edge_take_as_core_does() {
    let root = std::env::temp_dir().join(format!("tg-cli-edge-takes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let takes = edge_takes::write_all(&root).unwrap();
    let wrong: Vec<String> = on_each(&takes, |t| {
        let mut wrong = Vec::new();
        let core = edge_takes::read(&root, t).unwrap_or_else(|e| panic!("{e}"));
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
            return wrong;
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
        wrong
    })
    .into_iter()
    .flatten()
    .collect();
    std::fs::remove_dir_all(&root).ok();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// `doctor --file` finishes on every sound file of every edge take, and
/// reads the watch either side of the digital silence (#782): it used to
/// run for ever there.
#[test]
fn doctor_finishes_on_each_edge_take() {
    let root = std::env::temp_dir().join(format!("tg-cli-edge-doctor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let takes = edge_takes::write_all(&root).unwrap();
    let mut files = Vec::new();
    for t in &takes {
        let dir = root.join("takes").join(t.id);
        let mut wavs: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "wav"))
            .collect();
        wavs.sort();
        files.extend(wavs.into_iter().map(|w| (t, w)));
    }
    on_each(&files, |(t, wav)| {
        {
            let mut child = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
                .arg("doctor")
                .arg("--file")
                .arg(wav)
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
    });
    std::fs::remove_dir_all(&root).ok();
}
