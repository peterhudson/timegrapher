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
use crate::synth::{self, generate, Rng, SynthConfig};
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
    /// The caveat codes each reading of the take carries, in the session
    /// file's order: these and no others.
    pub caveats: &'static [&'static [&'static str]],
    /// What a reading must find, when the take holds enough of the watch
    /// to judge it by.
    pub truth: Option<Truth>,
    /// Why the take does not yet read as it was made, when it does not:
    /// the test then requires it still not to, so the list only shrinks
    /// as the reading is fixed (guard G3).
    pub known: Option<&'static str>,
}

/// The figures a reading of an edge take must find.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Truth {
    /// s/d on the true clock.
    pub rate: f64,
    /// Degrees, or none where the take gives nothing to measure it by.
    pub amplitude: Option<f64>,
    /// ms, by size, or none where the take gives nothing to measure it by.
    pub beat_error: Option<f64>,
}

/// The watch as every edge take makes it.
const MADE: Truth = Truth {
    rate: RATE,
    amplitude: Some(AMPLITUDE),
    beat_error: Some(BEAT_ERROR),
};

/// The takes, in the order they are written.
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
pub fn all() -> Vec<EdgeTake> {
    vec![
        EdgeTake {
            id: "edge_short_8s",
            summary: "8 s of the watch: too short for its regularity to be assessed",
            caveats: &[&["short_span", "clock_unknown"]],
            truth: None,
            known: None,
        },
        EdgeTake {
            id: "edge_short_25s_settle_20",
            summary: "25 s with a 20 s settling time and no end given: 5 s is measured",
            caveats: &[&["short_span", "clock_unknown"]],
            truth: None,
            known: None,
        },
        EdgeTake {
            id: "edge_listed_pause",
            summary: "two stretches of 90 s with a 90 s pause between, listed in session.json",
            caveats: &[&["breaks"]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_sleep_jump",
            summary: "the computer slept 1,283 s at 60 s, shown by the clock log alone, \
                      and a 0.4 s step at 30 s under the break rule",
            caveats: &[&["clock_unknown", "breaks", "sides_by_count"]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_silence_mid",
            summary: "60 s of silence between two stretches of 45 s of the watch, which runs \
                      on unheard",
            caveats: &[&["beats_missing"]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_all_silence",
            summary: "30 s of the stand's noise and no watch",
            caveats: &[&["no_watch", "beats_missing", "clock_unknown"]],
            truth: None,
            known: None,
        },
        EdgeTake {
            id: "edge_unreadable_session_json",
            summary: "the listed pause take with its session.json cut short: the clock log \
                      stands in, and says so",
            caveats: &[&["breaks", "breaks_unlisted"]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_card_20ppm",
            summary: "120 s on a sound card 20 ppm slow, measured by its clock log",
            caveats: &[&[]],
            // Made at 5 s/d on the card's seconds; a true second is 20 ppm
            // longer, so on true time the watch runs 1.728 s/d slower.
            truth: Some(Truth {
                rate: RATE - 20e-6 * 86_400.0,
                ..MADE
            }),
            known: None,
        },
        EdgeTake {
            id: "edge_silence_long",
            summary: "300 s of silence, over several of the stream's chunks, between two \
                      stretches of 60 s of the watch, which runs on unheard",
            caveats: &[&["beats_missing"]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_quiet_step_12db",
            summary: "240 s of the watch, heard 12 dB more quietly from 100 s on over the \
                      same stand noise",
            caveats: &[&[]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_quiet_step",
            summary: "240 s of the watch, heard 16 dB more quietly from 100 s on over the \
                      same stand noise, as when it is turned or the mic is touched",
            caveats: &[&[]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_digital_silence",
            summary: "300 s of digital silence (every sample zero), with no break listed, \
                      between two stretches of 60 s of the watch, which runs on unheard",
            caveats: &[&["beats_missing"]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_quiet_step_20db",
            summary: "240 s of the watch, heard 20 dB more quietly from 100 s on over the \
                      same stand noise: beats lost in the noise are left out, and the rate \
                      is the watch's",
            caveats: &[&["beats_missing"]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_sleep_two_entries",
            summary: "the computer slept 1,283 s at 60 s, shown by the clock log alone, in \
                      the first of two readings of one file (0 to 90 s and 90 to 180 s)",
            caveats: &[&["breaks", "sides_by_count"], &[]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_clock_back",
            summary: "120 s with the computer's clock set back 0.6 s at 60 s, as a time \
                      server does: true time does not step, so nothing is lost",
            caveats: &[&["clock_unknown"]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_mask_across_settle",
            summary: "120 s with 15 to 30 s masked by hand, across the end of the 20 s \
                      settling time",
            caveats: &[&["masked"]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_sides_turn_at_jump",
            summary: "the watch knocked at 60 s, its beats then 1.4 beats on: Tick and \
                      Tock swap on the count, and are told apart by their sound",
            caveats: &[&["breaks"]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_reads_nothing",
            summary: "two readings of one file, the second starting where it ends (moved \
                      again before it settled): it is left out, and the first is read",
            caveats: &[&[]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_watch_stops",
            summary: "120 s of the watch, then 120 s of the stand's noise: the watch ran \
                      down, as at the end of a power reserve run",
            caveats: &[&["beats_missing"]],
            truth: Some(MADE),
            known: None,
        },
        EdgeTake {
            id: "edge_usb_lines_and_log",
            summary: "120 s on a sound card 20 ppm slow whose recording carries the USB \
                      bus's lines, with a clock log: the lines set the rate",
            caveats: &[&[]],
            truth: Some(Truth {
                rate: RATE - 20e-6 * 86_400.0,
                ..MADE
            }),
            known: None,
        },
        EdgeTake {
            id: "edge_usb_lines_and_correction",
            summary: "120 s on a sound card 20 ppm slow whose recording carries the USB \
                      bus's lines, with a correction of 20 ppm in the session file and no \
                      clock log: the correction sets the rate",
            caveats: &[&[]],
            truth: Some(Truth {
                rate: RATE - 20e-6 * 86_400.0,
                ..MADE
            }),
            known: None,
        },
        EdgeTake {
            id: "edge_usb_lines_and_other_correction",
            summary: "the same sound with a correction of 15 ppm in the session file, which \
                      the lines (20 ppm) contradict, and no clock log: the lines set the \
                      rate, and the caveat names both",
            caveats: &[&["clock_sources_disagree"]],
            // Read on the lines' 20 ppm, the card's true error, over the
            // correction's 15, and said: 5 ppm or more apart (#635).
            truth: Some(Truth {
                rate: RATE - 20e-6 * 86_400.0,
                ..MADE
            }),
            known: None,
        },
        EdgeTake {
            id: "edge_lift_48",
            summary: "120 s of the watch made at a 52 degree lift, read at the 48 the \
                      session file gives: amplitude scales with the lift",
            caveats: &[&[]],
            truth: Some(Truth {
                amplitude: Some(AMPLITUDE * 48.0 / LIFT),
                ..MADE
            }),
            known: None,
        },
        EdgeTake {
            id: "edge_kept_lift_48",
            summary: "the same sound kept as its analysis alone, measured at a 52 degree \
                      lift, opened with the 48 the session file gives: it reads as the \
                      sound does at 48",
            caveats: &[&[]],
            truth: Some(Truth {
                amplitude: Some(AMPLITUDE * 48.0 / LIFT),
                ..MADE
            }),
            known: None,
        },
        EdgeTake {
            id: "edge_no_unlock",
            summary: "120 s of a watch whose unlock makes no sound: rate and the drop are \
                      heard, amplitude and the unlock's beat error are not",
            caveats: &[&[]],
            truth: Some(Truth {
                amplitude: None,
                beat_error: None,
                ..MADE
            }),
            known: None,
        },
        EdgeTake {
            id: "edge_45s",
            summary: "45 s of the watch with the default settling time: 25 s is measured, \
                      under the 30 s the app's whole take once needed at a 10 s Average Over",
            caveats: &[&["clock_unknown"]],
            truth: Some(MADE),
            known: None,
        },
    ]
}

/// The watch as every edge take makes it, `duration_s` long, from `seed`.
fn made(duration_s: f64, seed: u64) -> SynthConfig {
    SynthConfig {
        sample_rate: FS,
        duration_s,
        lift_deg: LIFT,
        rate_s_per_day: RATE,
        beat_error_ms: BEAT_ERROR,
        seed,
        ..Default::default()
    }
}

/// The watch's sound, `duration_s` long, from a fresh `seed`.
fn watch(duration_s: f64, seed: u64) -> Vec<f32> {
    generate(&made(duration_s, seed), |_| AMPLITUDE, |_| 0.0).samples
}

/// The watch, `duration_s` long, heard `drop_db` more quietly from
/// `from_s` on, over the stand's noise at the level `generate` adds it
/// throughout.
fn watch_stepping_down(duration_s: f64, from_s: f64, drop_db: f64, seed: u64) -> Vec<f32> {
    let clean = generate(
        &SynthConfig {
            snr_db: 300.0,
            ..made(duration_s, seed)
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

/// Keep the take in `dir` as its analysis alone, measured as `manifest`
/// (its session file) says, and remove its sound.
fn keep_analysis(dir: &Path, manifest: &str, frames: usize) -> Result<(), String> {
    let m: Manifest = toml::from_str(manifest).map_err(|e| e.to_string())?;
    let cfg = take::stream_settings(&m, None, take::Given::default());
    let wav = dir.join("audio-001.wav");
    let shapes = crate::analysis_take::shape_stretches(&m, frames as f64 / FS as f64);
    let software = serde_json::json!({"name": "timegrapher", "version": "edge take"});
    let kept = crate::analysis_take::measure(&[wav.as_path()], &cfg, &shapes, software, |_| {})
        .map_err(|e| e.to_string())?;
    crate::analysis_take::write(dir, &kept).map_err(|e| e.to_string())?;
    std::fs::remove_file(&wav).map_err(|e| e.to_string())
}

/// `samples` with the USB bus's lines added, 3 to 10 kHz on true time,
/// as a card `ppm` slow hears them ([`crate::usbclock`]).
fn with_usb_lines(mut samples: Vec<f32>, ppm: f64) -> Vec<f32> {
    for k in 3..=10 {
        let f = 1000.0 * k as f64 * (1.0 + ppm * 1e-6);
        for (i, v) in samples.iter_mut().enumerate() {
            *v += (0.001 * (2.0 * std::f64::consts::PI * f * i as f64 / FS as f64).sin()) as f32;
        }
    }
    samples
}

/// The stand's noise and no watch, at the level `generate` adds it.
fn silence(duration_s: f64, seed: u64) -> Vec<f32> {
    let rms = synth::noise_rms(SynthConfig::default().snr_db) * synth::SCALE;
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

/// How one take is made: its sound, the breaks its `session.json` lists
/// and the steps its clock log shows, both as `(frame, gap_s)`, how slow
/// its card runs, and the session file's settings and readings.
struct Recipe {
    samples: Vec<f32>,
    breaks: Vec<(usize, f64)>,
    steps: Vec<(usize, f64)>,
    ppm: f64,
    extra: &'static str,
    entries: &'static str,
    /// The lift angle the session file gives.
    lift: f64,
    /// Kept as its analysis alone, made at this lift, with no sound.
    kept_at: Option<f64>,
}

/// One reading of the whole take, Dial Up, on its clock log.
const WHOLE_TAKE: &str = "[[recording]]\nfile = \".\"\nposition = \"DU\"\nclock = \"clock.csv\"\n";

impl Recipe {
    /// `samples` read as one whole take, with nothing listed or stepped.
    fn of(samples: Vec<f32>) -> Self {
        Recipe {
            samples,
            breaks: Vec::new(),
            steps: Vec::new(),
            ppm: 0.0,
            extra: "",
            entries: WHOLE_TAKE,
            lift: LIFT,
            kept_at: None,
        }
    }
}

fn manifest(take: &EdgeTake, lift: f64, extra: &str, entries: &str) -> String {
    format!(
        "watch = \"synthetic: {}\"\nbph = 28800\nlift = {lift}\nnotes = \"{}\"\n{extra}\n{entries}",
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
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
pub fn write_all(root: &Path) -> Result<Vec<EdgeTake>, String> {
    let takes = all();
    for (n, take) in takes.iter().enumerate() {
        let dir = root.join("takes").join(take.id);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let seed = 100 + n as u64;
        let r = match take.id {
            "edge_short_8s" => Recipe::of(watch(8.0, seed)),
            "edge_45s" => Recipe::of(watch(45.0, seed)),
            "edge_short_25s_settle_20" => Recipe {
                extra: "settle_s = 20\n",
                ..Recipe::of(watch(25.0, seed))
            },
            "edge_listed_pause" | "edge_unreadable_session_json" => {
                let mut s = watch(90.0, seed);
                let join = s.len();
                s.extend(watch(90.0, seed + 50));
                Recipe {
                    breaks: vec![(join, 90.0)],
                    steps: vec![(join, 90.0)],
                    ..Recipe::of(s)
                }
            }
            "edge_sleep_jump" => {
                let mut s = watch(60.0, seed);
                let join = s.len();
                s.extend(watch(60.0, seed + 50));
                let small = 30 * FS as usize;
                Recipe {
                    steps: vec![(small, 0.4), (join, 1283.0)],
                    ..Recipe::of(s)
                }
            }
            "edge_silence_mid" => {
                // The watch runs on unheard: its beats come back in step.
                let mut s = watch(150.0, seed);
                let quiet = silence(60.0, seed + 50);
                let from = 45 * FS as usize;
                s[from..from + quiet.len()].copy_from_slice(&quiet);
                Recipe::of(s)
            }
            "edge_silence_long" => {
                let mut s = watch(420.0, seed);
                let quiet = silence(300.0, seed + 50);
                let from = 60 * FS as usize;
                s[from..from + quiet.len()].copy_from_slice(&quiet);
                Recipe::of(s)
            }
            "edge_digital_silence" => {
                let mut s = watch(420.0, seed);
                s[60 * FS as usize..360 * FS as usize].fill(0.0);
                Recipe::of(s)
            }
            "edge_all_silence" => Recipe::of(silence(30.0, seed)),
            "edge_quiet_step_12db" => Recipe::of(watch_stepping_down(240.0, 100.0, 12.0, seed)),
            "edge_quiet_step" => Recipe::of(watch_stepping_down(240.0, 100.0, 16.0, seed)),
            "edge_quiet_step_20db" => Recipe::of(watch_stepping_down(240.0, 100.0, 20.0, seed)),
            "edge_card_20ppm" => Recipe {
                ppm: 20.0,
                ..Recipe::of(watch(120.0, seed))
            },
            "edge_sleep_two_entries" => {
                let mut s = watch(60.0, seed);
                let join = s.len();
                s.extend(watch(120.0, seed + 50));
                Recipe {
                    steps: vec![(join, 1283.0)],
                    entries: "[[recording]]\nfile = \".\"\nposition = \"DU\"\n\
                              clock = \"clock.csv\"\nstart_s = 0\nend_s = 90\n\n\
                              [[recording]]\nfile = \".\"\nposition = \"DD\"\n\
                              clock = \"clock.csv\"\nstart_s = 90\nend_s = 180\n",
                    ..Recipe::of(s)
                }
            }
            "edge_clock_back" => Recipe {
                steps: vec![(60 * FS as usize, -0.6)],
                ..Recipe::of(watch(120.0, seed))
            },
            "edge_mask_across_settle" => Recipe {
                entries: "[[recording]]\nfile = \".\"\nposition = \"DU\"\n\
                          clock = \"clock.csv\"\nmasked = [[15.0, 30.0]]\n",
                ..Recipe::of(watch(120.0, seed))
            },
            "edge_sides_turn_at_jump" => {
                // Tock sounds unlike Tick, so the sides can be told by
                // ear; the second stretch opens a beat in, on the other side.
                let two_sides = |d: f64, seed: u64| {
                    let cfg = SynthConfig {
                        odd_gains: Some([0.2, 0.25, 0.7]),
                        ..made(d, seed)
                    };
                    generate(&cfg, |_| AMPLITUDE, |_| 0.0).samples
                };
                let mut s = two_sides(60.0, seed);
                let beat = (FS as f64 * 3600.0 / 28_800.0) as usize;
                s.extend_from_slice(&two_sides(61.0, seed + 50)[beat + beat * 2 / 5..]);
                Recipe::of(s)
            }
            "edge_reads_nothing" => Recipe {
                entries: "[[recording]]\nfile = \".\"\nposition = \"DU\"\n\
                          clock = \"clock.csv\"\nend_s = 90\n\n\
                          [[recording]]\nfile = \".\"\nposition = \"DD\"\n\
                          clock = \"clock.csv\"\nstart_s = 90\nend_s = 90\n",
                ..Recipe::of(watch(120.0, seed))
            },
            "edge_watch_stops" => {
                let mut s = watch(120.0, seed);
                s.extend(silence(120.0, seed + 50));
                Recipe::of(s)
            }
            "edge_usb_lines_and_log" => Recipe {
                ppm: 20.0,
                ..Recipe::of(with_usb_lines(watch(120.0, seed), 20.0))
            },
            "edge_usb_lines_and_correction" => Recipe {
                ppm: 20.0,
                extra: "card_ppm = 20\n",
                entries: "[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
                ..Recipe::of(with_usb_lines(watch(120.0, seed), 20.0))
            },
            "edge_usb_lines_and_other_correction" => Recipe {
                ppm: 20.0,
                extra: "card_ppm = 15\n",
                entries: "[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
                ..Recipe::of(with_usb_lines(watch(120.0, seed), 20.0))
            },
            "edge_lift_48" => Recipe {
                lift: 48.0,
                ..Recipe::of(watch(120.0, seed))
            },
            "edge_kept_lift_48" => Recipe {
                lift: 48.0,
                kept_at: Some(LIFT),
                ..Recipe::of(watch(120.0, seed - 1))
            },
            "edge_no_unlock" => {
                let mut cfg = made(120.0, seed);
                cfg.sounds[0].gain = 0.0;
                Recipe::of(generate(&cfg, |_| AMPLITUDE, |_| 0.0).samples)
            }
            other => return Err(format!("no recipe for {other}")),
        };
        let Recipe {
            samples,
            breaks,
            steps,
            ppm,
            extra,
            entries,
            lift,
            kept_at,
        } = r;
        let frames = samples.len();
        let wav = dir.join("audio-001.wav");
        write_wav(
            &wav,
            &Audio {
                samples,
                sample_rate: FS,
            },
        )
        .map_err(|e| format!("{}: {e}", dir.display()))?;
        match kept_at {
            Some(at) => {
                keep_analysis(&dir, &manifest(take, at, extra, entries), frames)?;
                let kept = format!("audio = false\n{extra}");
                write(&dir, "session.toml", &manifest(take, lift, &kept, entries))?;
            }
            None => write(&dir, "session.toml", &manifest(take, lift, extra, entries))?,
        }
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
                "truth": t.truth.map(|t| serde_json::json!({
                    "rate": t.rate, "amplitude": t.amplitude, "beat_error": t.beat_error
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

/// The take written under `root`, each of its entries read as `timegrapher
/// session` reads it with no settings given ([`take::measure_entry`]),
/// leaving out, as session does, one with nothing to read.
pub fn read(root: &Path, take: &EdgeTake) -> Result<Vec<take::EntryRead>, String> {
    let dir = root.join("takes").join(take.id);
    let text = std::fs::read_to_string(dir.join("session.toml"))
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let m: Manifest = toml::from_str(&text).map_err(|e| format!("{}: {e}", take.id))?;
    let stream = take::stream_settings(&m, None, take::Given::default());
    let s = take::EntrySettings {
        stream,
        card_clock: false,
        settle_s: None,
    };
    m.recordings
        .iter()
        .filter(|e| !take::reads_nothing(e))
        .map(|e| {
            take::measure_entry(&dir, &m, e, &s, take::usb, |first| {
                take::correction_for(&m, first, None, None)
            })
            .map_err(|err| format!("{}: {err}", take.id))
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The stand's noise alone, as edge_all_silence makes it, reads
    /// `no_watch` on these seeded draws, in session and, up to 90 s, on
    /// the live card: before #675, 16 in 100 draws of 30 s, 6 in 100 of
    /// 90 s and 5 in 100 of 120 s were read as a watch with a rate, and
    /// with the sound of one side deciding, 3 in 200 more of 30 s. Not
    /// every draw passes: seed 5032 of 30 s sounds like a watch on both
    /// sides at 1.94% scatter and is read as one (about 1 in 60 draws of
    /// 30 s), and under 10 s of sound, where one side's sound decides, 4
    /// in 150 draws of 10 s are.
    #[test]
    fn stand_noise_reads_no_watch_on_the_seeded_draws() {
        let take = all()
            .into_iter()
            .find(|t| t.id == "edge_all_silence")
            .expect("edge_all_silence");
        let root = std::env::temp_dir().join(format!("tg-stand-noise-{}", std::process::id()));
        let dir = root.join("takes").join(take.id);
        std::fs::create_dir_all(&dir).unwrap();
        write(&dir, "session.toml", &manifest(&take, LIFT, "", WHOLE_TAKE)).unwrap();
        write(&dir, "session.json", &record(&[])).unwrap();
        let mut heard = Vec::new();
        let mut split = Vec::new();
        // Draws from two ranges of seeds, so the test is not tuned to one.
        for (duration_s, draws) in [(30.0, 20), (90.0, 10), (120.0, 10)] {
            let frames = (duration_s * FS as f64) as usize;
            write(&dir, "clock.csv", &clock_log(frames, 0.0, &[])).unwrap();
            for seed in (0..draws).chain(1000..1000 + draws) {
                let audio = Audio {
                    samples: silence(duration_s, seed),
                    sample_rate: FS,
                };
                write_wav(&dir.join("audio-001.wav"), &audio).unwrap();
                let e = &read(&root, &take).unwrap()[0];
                let m = &e.measurement;
                if !caveat_codes(m).iter().any(|c| c == "no_watch") {
                    heard.push((duration_s, seed, m.rate_s_per_day));
                }
                // The live card, opened on the same take, reads the same
                // stretch alike. Past 90 s it can read a short last run
                // of beats between noise's jumps instead (#675).
                if duration_s > 90.0 {
                    continue;
                }
                let live = crate::live::LiveAnalyzer::from_log(e.log.clone(), Default::default())
                    .reading_at(m.end_s, m.end_s - m.start_s);
                if live.watch_heard != m.watch_heard {
                    split.push((duration_s, seed, live.rate_s_per_day));
                }
            }
        }
        let _ = std::fs::remove_dir_all(&root);
        assert!(heard.is_empty(), "noise read as a watch: {heard:?}");
        assert!(
            split.is_empty(),
            "the live card and session differ: {split:?}"
        );
    }
}
