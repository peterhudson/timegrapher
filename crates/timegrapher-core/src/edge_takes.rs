//! Synthetic edge takes: short, paused, slept, silent and badly kept
//! takes, written as a corpus is laid out (`corpus.json` and
//! `takes/<id>/` with a `session.toml`), so every surface can read the
//! same awkward takes and the differential tests can compare them
//! (architecture guide, guard G2). Each take is a 28,800 bph watch at a
//! known rate, amplitude and beat error, with what reading it should say.
//!
//! `cargo run -p timegrapher-core --example edge_takes -- DIR` writes them.

use crate::audio::{write_wav, Audio};
use crate::session::Manifest;
use crate::stream::StreamConfig;
use crate::synth::{generate, Rng, SynthConfig};
use crate::take;
use std::path::Path;

const FS: u32 = 48_000;
const RATE: f64 = 5.0;
const AMPLITUDE: f64 = 280.0;
const BEAT_ERROR: f64 = 0.4;
const LIFT: f64 = 52.0;
/// Unix seconds at each take's first frame: 2026-10-29 00:00:00 UTC.
const T0: f64 = 1_793_232_000.0;

/// One edge take: its folder name, what it holds, and what a reading of
/// it must say.
#[derive(Debug, Clone)]
pub struct EdgeTake {
    pub id: &'static str,
    /// One line: what is awkward about the take.
    pub summary: &'static str,
    /// The caveat codes a reading of the take carries: these and no others.
    pub caveats: &'static [&'static str],
    /// The rate, amplitude and beat error a reading must find (s/d on the
    /// true clock, degrees, ms), when the take holds enough of the watch
    /// to judge it by.
    pub truth: Option<(f64, f64, f64)>,
    /// Why the take does not yet read as it was made, when it does not:
    /// the test then requires it still not to, so the list only shrinks
    /// as the reading is fixed (guard G3).
    pub known: Option<&'static str>,
}

/// The takes, in the order they are written.
pub fn all() -> Vec<EdgeTake> {
    vec![
        EdgeTake {
            id: "edge_short_8s",
            summary: "8 s of the watch: too short for its regularity to be assessed",
            caveats: &["short_span", "clock_unknown"],
            truth: None,
            known: None,
        },
        EdgeTake {
            id: "edge_short_25s_settle_20",
            summary: "25 s with a 20 s settling time and no end given",
            caveats: &["clock_unknown"],
            truth: None,
            known: None,
        },
        EdgeTake {
            id: "edge_listed_pause",
            summary: "two stretches of 90 s with a 90 s pause between, listed in session.json",
            caveats: &["breaks"],
            truth: Some((RATE, AMPLITUDE, BEAT_ERROR)),
            known: None,
        },
        EdgeTake {
            id: "edge_sleep_jump",
            summary: "the computer slept 1,283 s at 60 s, shown by the clock log alone, \
                      and a 0.4 s step at 30 s under the break rule",
            caveats: &["clock_unknown", "breaks", "sides_by_count"],
            truth: Some((RATE, AMPLITUDE, BEAT_ERROR)),
            known: None,
        },
        EdgeTake {
            id: "edge_silence_mid",
            summary: "60 s of silence between two stretches of 45 s of the watch, which runs \
                      on unheard",
            caveats: &["beats_missing"],
            truth: Some((RATE, AMPLITUDE, BEAT_ERROR)),
            known: None,
        },
        EdgeTake {
            id: "edge_all_silence",
            summary: "30 s of the stand's noise and no watch",
            caveats: &["no_watch", "beats_missing", "clock_unknown"],
            truth: None,
            known: None,
        },
        EdgeTake {
            id: "edge_unreadable_session_json",
            summary: "the listed pause take with its session.json cut short: the clock log \
                      stands in, and says so",
            caveats: &["breaks", "breaks_unlisted"],
            truth: Some((RATE, AMPLITUDE, BEAT_ERROR)),
            known: None,
        },
        EdgeTake {
            id: "edge_card_20ppm",
            summary: "120 s on a sound card 20 ppm slow, measured by its clock log",
            caveats: &[],
            // Made at 5 s/d on the card's seconds; a true second is 20 ppm
            // longer, so on true time the watch runs 1.728 s/d slower.
            truth: Some((RATE - 20e-6 * 86_400.0, AMPLITUDE, BEAT_ERROR)),
            known: None,
        },
        EdgeTake {
            id: "edge_silence_long",
            summary: "300 s of silence, over several of the stream's chunks, between two \
                      stretches of 60 s of the watch, which runs on unheard",
            caveats: &["beats_missing"],
            truth: Some((RATE, AMPLITUDE, BEAT_ERROR)),
            known: None,
        },
        EdgeTake {
            id: "edge_quiet_step_12db",
            summary: "240 s of the watch, heard 12 dB more quietly from 100 s on over the \
                      same stand noise",
            caveats: &[],
            truth: Some((RATE, AMPLITUDE, BEAT_ERROR)),
            known: None,
        },
        EdgeTake {
            id: "edge_quiet_step",
            summary: "240 s of the watch, heard 16 dB more quietly from 100 s on over the \
                      same stand noise, as when it is turned or the mic is touched",
            caveats: &[],
            truth: Some((RATE, AMPLITUDE, BEAT_ERROR)),
            known: None,
        },
        EdgeTake {
            id: "edge_digital_silence",
            summary: "300 s of digital silence (every sample zero), with no break listed, \
                      between two stretches of 60 s of the watch, which runs on unheard",
            caveats: &["beats_missing"],
            truth: Some((RATE, AMPLITUDE, BEAT_ERROR)),
            known: None,
        },
        EdgeTake {
            id: "edge_quiet_step_20db",
            summary: "240 s of the watch, heard 20 dB more quietly from 100 s on over the \
                      same stand noise: beats lost in the noise are left out, and the rate \
                      is the watch's",
            caveats: &["beats_missing"],
            truth: Some((RATE, AMPLITUDE, BEAT_ERROR)),
            known: None,
        },
    ]
}

/// The watch's sound, `duration_s` long, from a fresh `seed`.
fn watch(duration_s: f64, seed: u64) -> Vec<f32> {
    generate(
        &SynthConfig {
            sample_rate: FS,
            duration_s,
            lift_deg: LIFT,
            rate_s_per_day: RATE,
            beat_error_ms: BEAT_ERROR,
            seed,
            ..Default::default()
        },
        |_| AMPLITUDE,
        |_| 0.0,
    )
    .samples
}

/// The watch, `duration_s` long, heard `drop_db` more quietly from
/// `from_s` on, over the stand's noise at the level `generate` adds it
/// throughout.
fn watch_stepping_down(duration_s: f64, from_s: f64, drop_db: f64, seed: u64) -> Vec<f32> {
    let clean = generate(
        &SynthConfig {
            sample_rate: FS,
            duration_s,
            lift_deg: LIFT,
            rate_s_per_day: RATE,
            beat_error_ms: BEAT_ERROR,
            snr_db: 300.0,
            seed,
            ..Default::default()
        },
        |_| AMPLITUDE,
        |_| 0.0,
    )
    .samples;
    let quieter = 10f32.powf(-drop_db as f32 / 20.0);
    let from = (from_s * FS as f64) as usize;
    let noise = silence(duration_s, seed + 50);
    clean
        .iter()
        .zip(noise)
        .enumerate()
        .map(|(i, (&w, n))| if i < from { w } else { w * quieter } + n)
        .collect()
}

/// The stand's noise and no watch, at the level `generate` adds it.
fn silence(duration_s: f64, seed: u64) -> Vec<f32> {
    let rms = 10f64.powf(-SynthConfig::default().snr_db / 20.0) * 0.5;
    let mut rng = Rng::new(seed);
    (0..(duration_s * FS as f64) as usize)
        .map(|_| (rms * rng.normal()) as f32)
        .collect()
}

/// A clock log for `frames` of sound on a card `ppm` slow: lines as the
/// recorder writes them, a line either side of each step and one at the
/// last frame, true time stepping by `gap_s` at each `(frame, gap_s)` of
/// `steps`.
fn clock_log(frames: usize, ppm: f64, steps: &[(usize, f64)]) -> String {
    let mut lines = String::from("unix_s,frames\n");
    let every = 10 * FS as usize;
    // As the recorder logs: a line a second over the first 3 s, then every
    // 10 s.
    let fs = FS as usize;
    let mut at: Vec<usize> = [fs / 10, fs, 2 * fs, 3 * fs]
        .into_iter()
        .chain((every..frames).step_by(every))
        .filter(|&f| f < frames)
        .collect();
    at.extend(
        steps
            .iter()
            .flat_map(|&(f, _)| [f.saturating_sub(FS as usize / 10), f + FS as usize / 10]),
    );
    at.push(frames);
    at.sort_unstable();
    at.dedup();
    for f in at {
        let gap: f64 = steps.iter().filter(|s| s.0 <= f).map(|s| s.1).sum();
        let t = T0 + f as f64 / FS as f64 * (1.0 + ppm * 1e-6) + gap;
        lines.push_str(&format!("{t:.6},{f}\n"));
    }
    lines
}

fn manifest(take: &EdgeTake, extra: &str) -> String {
    format!(
        "watch = \"synthetic: {}\"\nbph = 28800\nlift = {LIFT}\nnotes = \"{}\"\n{extra}\n\
         [[recording]]\nfile = \".\"\nposition = \"DU\"\nclock = \"clock.csv\"\n",
        take.id, take.summary
    )
}

/// `session.json` for one segment, listing `breaks` as (frame, gap_s).
fn record(breaks: &[(usize, f64)]) -> String {
    let list: Vec<String> = breaks
        .iter()
        .map(|&(f, g)| {
            format!(
                r#"{{"frame": {f}, "audio_s": {}, "gap_s": {g}}}"#,
                f as f64 / FS as f64
            )
        })
        .collect();
    format!(
        r#"{{"segments": ["audio-001.wav"], "breaks": [{}]}}"#,
        list.join(", ")
    )
}

fn write(dir: &Path, name: &str, text: &str) -> Result<(), String> {
    std::fs::write(dir.join(name), text).map_err(|e| format!("{}: {e}", dir.join(name).display()))
}

/// Write every edge take under `root`, laid out as a corpus: `corpus.json`
/// and `takes/<id>/` (`audio-001.wav`, `session.toml`, `session.json`,
/// `clock.csv`). Returns the takes written.
pub fn write_all(root: &Path) -> Result<Vec<EdgeTake>, String> {
    let takes = all();
    for (n, take) in takes.iter().enumerate() {
        let dir = root.join("takes").join(take.id);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let seed = 100 + n as u64;
        let (samples, breaks, steps, ppm, extra) = match take.id {
            "edge_short_8s" => (watch(8.0, seed), vec![], vec![], 0.0, ""),
            "edge_short_25s_settle_20" => {
                (watch(25.0, seed), vec![], vec![], 0.0, "settle_s = 20\n")
            }
            "edge_listed_pause" | "edge_unreadable_session_json" => {
                let mut s = watch(90.0, seed);
                let join = s.len();
                s.extend(watch(90.0, seed + 50));
                (s, vec![(join, 90.0)], vec![(join, 90.0)], 0.0, "")
            }
            "edge_sleep_jump" => {
                let mut s = watch(60.0, seed);
                let join = s.len();
                s.extend(watch(60.0, seed + 50));
                let small = 30 * FS as usize;
                (s, vec![], vec![(small, 0.4), (join, 1283.0)], 0.0, "")
            }
            "edge_silence_mid" => {
                // The watch runs on unheard: its beats come back in step.
                let mut s = watch(150.0, seed);
                let quiet = silence(60.0, seed + 50);
                let from = 45 * FS as usize;
                s[from..from + quiet.len()].copy_from_slice(&quiet);
                (s, vec![], vec![], 0.0, "")
            }
            "edge_silence_long" => {
                let mut s = watch(420.0, seed);
                let quiet = silence(300.0, seed + 50);
                let from = 60 * FS as usize;
                s[from..from + quiet.len()].copy_from_slice(&quiet);
                (s, vec![], vec![], 0.0, "")
            }
            "edge_all_silence" => (silence(30.0, seed), vec![], vec![], 0.0, ""),
            "edge_quiet_step_12db" => (
                watch_stepping_down(240.0, 100.0, 12.0, seed),
                vec![],
                vec![],
                0.0,
                "",
            ),
            "edge_quiet_step" => (
                watch_stepping_down(240.0, 100.0, 16.0, seed),
                vec![],
                vec![],
                0.0,
                "",
            ),
            "edge_card_20ppm" => (watch(120.0, seed), vec![], vec![], 20.0, ""),
            "edge_digital_silence" => {
                let mut s = watch(420.0, seed);
                s[60 * FS as usize..360 * FS as usize].fill(0.0);
                (s, vec![], vec![], 0.0, "")
            }
            "edge_quiet_step_20db" => (
                watch_stepping_down(240.0, 100.0, 20.0, seed),
                vec![],
                vec![],
                0.0,
                "",
            ),
            other => return Err(format!("no recipe for {other}")),
        };
        let frames = samples.len();
        write_wav(
            &dir.join("audio-001.wav"),
            &Audio {
                samples,
                sample_rate: FS,
            },
        )
        .map_err(|e| format!("{}: {e}", dir.display()))?;
        write(&dir, "session.toml", &manifest(take, extra))?;
        write(&dir, "clock.csv", &clock_log(frames, ppm, &steps))?;
        let json = record(&breaks);
        if take.id == "edge_unreadable_session_json" {
            write(&dir, "session.json", &json[..json.len() / 2])?;
        } else {
            write(&dir, "session.json", &json)?;
        }
    }
    let list: Vec<serde_json::Value> = takes
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "tier": "synthetic",
                "summary": t.summary,
                "caveats": t.caveats,
                "truth": t.truth.map(|(r, a, b)| serde_json::json!({
                    "rate": r, "amplitude": a, "beat_error": b
                })),
                "known": t.known,
            })
        })
        .collect();
    let corpus = serde_json::json!({ "schema": 1, "takes": list });
    write(
        root,
        "corpus.json",
        &serde_json::to_string_pretty(&corpus).expect("JSON"),
    )?;
    Ok(takes)
}

/// The take written under `root`, read as `timegrapher session` reads its
/// one entry with no settings given ([`take::measure_entry`]).
pub fn read(root: &Path, take: &EdgeTake) -> Result<take::EntryRead, String> {
    let dir = root.join("takes").join(take.id);
    let text = std::fs::read_to_string(dir.join("session.toml"))
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let m: Manifest = toml::from_str(&text).map_err(|e| format!("{}: {e}", take.id))?;
    let mut stream = StreamConfig::default();
    stream.analysis.bph = m.bph;
    stream.analysis.amplitude.lift_deg = LIFT;
    stream.analysis.lift_known = true;
    let s = take::EntrySettings {
        stream,
        card_clock: false,
        settle_s: None,
    };
    take::measure_entry(&dir, &m, &m.recordings[0], &s, |_| Ok(None), |_| Ok(None))
        .map_err(|e| format!("{}: {e}", take.id))
}

/// The codes of the caveats a reading carries, as its JSON gives them.
pub fn caveat_codes(m: &crate::session::Measurement) -> Vec<String> {
    m.caveats
        .iter()
        .filter_map(|c| {
            serde_json::to_value(c).ok()?["code"]
                .as_str()
                .map(str::to_string)
        })
        .collect()
}
