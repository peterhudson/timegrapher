//! Synthetic takes with known hidden variations, for testing the
//! steadiness analysis and for learning what each of its views shows.
//!
//! Each scenario is a 28,800 bph watch laid out as an ETA 2824-2 (a
//! 20-tooth escape wheel turning in 5 s, the fourth wheel in 60 s), with
//! steady readings plus one variation whose period, size and timing are
//! known exactly: what the analysis should find is stated with it.
//! `control` has none, so every view should find nothing there.

use crate::synth::{self, Rng, SynthConfig};

/// A rate, amplitude or beat error over time, seconds from the start.
pub type Track = Box<dyn Fn(f64) -> f64 + Send + Sync>;

pub struct Scenario {
    pub name: &'static str,
    /// One line: what is hidden in the take.
    pub summary: &'static str,
    /// What the analysis should find, and in which views.
    pub expect: &'static str,
    /// How the headline of each series that shows the variation begins:
    /// (rate, amplitude or beat error, the start of its headline).
    pub finds: &'static [(&'static str, &'static str)],
    pub duration_s: f64,
    /// Rate, s/d.
    pub rate: Track,
    /// Amplitude, degrees.
    pub amplitude: Track,
    /// Beat error, ms.
    pub beat_error: Track,
}

/// The watch every scenario is: an ETA 2824-2 layout.
pub const CALIBRE: &str = "ETA 2824-2";
pub const BPH: u32 = 28800;
pub const LIFT_DEG: f64 = 50.0;
const RATE: f64 = 5.0;
const AMPLITUDE: f64 = 270.0;
const BEAT_ERROR: f64 = 0.4;
const HOURS_2: f64 = 7200.0;
/// Beat-to-beat spread of each beat's timing, ms, and of its amplitude,
/// degrees: a synthetic choice (each 10 s rate reading then scatters by
/// about 0.9 s/d).
pub const TIMING_JITTER_MS: f64 = 0.25;
pub const AMPLITUDE_JITTER_DEG: f64 = 3.0;

fn steady(v: f64) -> Track {
    Box::new(move |_| v)
}

/// `v` plus a sine of peak to peak `ptp` and period `period` s.
fn sine(v: f64, ptp: f64, period: f64) -> Track {
    let w = std::f64::consts::TAU / period;
    Box::new(move |t| v + 0.5 * ptp * (w * t).sin())
}

/// The scenarios, in the order they are listed.
pub fn all() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "control",
            summary: "Nothing hidden: a steady rate, amplitude and beat error with only reading noise.",
            expect: "Every series steady: \"no cycle, level change, trend or correlation between readings detected\".",
            finds: &[("rate", "Rate: "), ("amplitude", "Amplitude: "), ("beat_error", "Beat error: ")],
            duration_s: HOURS_2,
            rate: steady(RATE),
            amplitude: steady(AMPLITUDE),
            beat_error: steady(BEAT_ERROR),
        },
        Scenario {
            name: "fourth-wheel",
            summary: "The rate swings 2 s/d peak to peak every 60 s, one turn of the fourth wheel.",
            expect: "Rate repeats every 60 s (the fourth wheel), about 2 s/d peak to peak: the Period search, FFT and Cycle views and the autocorrelation's repeat at 60 s.",
            finds: &[("rate", "Rate repeats every 60 s (the fourth wheel)")],
            duration_s: HOURS_2,
            rate: sine(RATE, 2.0, 60.0),
            amplitude: steady(AMPLITUDE),
            beat_error: steady(BEAT_ERROR),
        },
        Scenario {
            name: "escape-wheel",
            summary: "The rate swings 20 s/d peak to peak every 5 s, one turn of the escape wheel: about 0.18 ms of beat timing.",
            expect: "The beat timing repeats every 5.0 s (the escape wheel), about 0.18 ms peak to peak; 10 s readings are too coarse to show it, so the readings stay steady. FFT with Timing shows the line; without, it is off the axis.",
            finds: &[("rate", "Rate: ")],
            duration_s: HOURS_2,
            rate: sine(RATE, 20.0, 5.0),
            amplitude: steady(AMPLITUDE),
            beat_error: steady(BEAT_ERROR),
        },
        Scenario {
            name: "half-period",
            summary: "The rate swings 2 s/d peak to peak every 45 s, four and a half 10 s readings.",
            expect: "Rate repeats every 45 s, about 2 s/d peak to peak, though a fold at 90 s (nine readings) groups the readings the same way.",
            finds: &[("rate", "Rate repeats every 45 s")],
            duration_s: HOURS_2,
            rate: sine(RATE, 2.0, 45.0),
            amplitude: steady(AMPLITUDE),
            beat_error: steady(BEAT_ERROR),
        },
        Scenario {
            name: "slow-cycle",
            summary: "The amplitude swings 6° peak to peak every 8 min, an arbitrary slow cycle (no wheel of the 2824-2 layout turns in 8 min).",
            expect: "Amplitude repeats every 8 min, about 6° peak to peak: Period search, FFT and Cycle at 480 s, and a bump in the Allan deviation near 0.37 of the period (about 180 s).",
            finds: &[("amplitude", "Amplitude repeats every 8 min")],
            duration_s: HOURS_2,
            rate: steady(RATE),
            amplitude: sine(AMPLITUDE, 6.0, 480.0),
            beat_error: steady(BEAT_ERROR),
        },
        Scenario {
            name: "beat-error-cycle",
            summary: "The beat error swings 0.2 ms peak to peak every 120 s.",
            expect: "Beat error repeats every 120 s, about 0.2 ms peak to peak: Period search, FFT and Cycle for the beat error.",
            finds: &[("beat_error", "Beat error repeats every 120 s")],
            duration_s: HOURS_2,
            rate: steady(RATE),
            amplitude: steady(AMPLITUDE),
            beat_error: sine(BEAT_ERROR, 0.2, 120.0),
        },
        Scenario {
            name: "letting-down",
            summary: "The amplitude falls steadily from 280° to 260° over the take, as a mainspring lets down.",
            expect: "Amplitude slides from about 280° to 260°, -10° per hour: the Trend view, and an Allan deviation that rises at long averaging times.",
            finds: &[("amplitude", "Amplitude slides from")],
            duration_s: HOURS_2,
            rate: steady(RATE),
            amplitude: Box::new(|t| 280.0 - 20.0 * t / HOURS_2),
            beat_error: steady(BEAT_ERROR),
        },
        Scenario {
            name: "knock",
            summary: "The rate steps from +5 s/d to +8 s/d at 70 min, as after a knock.",
            expect: "Rate shifts level once, near 70 min, from +5 to +8 s/d: CUSUM bends with its corner at the step, and Change points splits the take there.",
            finds: &[("rate", "Rate shifts level 1 time")],
            duration_s: HOURS_2,
            rate: Box::new(|t| if t < 4200.0 { RATE } else { RATE + 3.0 }),
            amplitude: steady(AMPLITUDE),
            beat_error: steady(BEAT_ERROR),
        },
        Scenario {
            name: "wander",
            summary: "The rate wanders slowly as a random walk, about 1 s/d over 10 min, with no cycle.",
            expect: "Rate wanders with no cycle: the change finder splits the walk into several level shifts, the autocorrelation falls slowly and the Allan deviation rises above the white line at long averaging times.",
            finds: &[("rate", "Rate shifts level")],
            duration_s: HOURS_2,
            rate: walk(RATE, 1.0, 600.0, HOURS_2, 7),
            amplitude: steady(AMPLITUDE),
            beat_error: steady(BEAT_ERROR),
        },
        Scenario {
            name: "rub",
            summary: "For 8 s every 180 s the rate drops 30 s/d and the amplitude 15°, as an intermittent rub.",
            expect: "Rate and amplitude repeat every 3 min, the rate about 27 s/d peak to peak (an 8 s drop is most of one 10 s reading) and the amplitude about 15°: Period search, FFT and Cycle at 180 s, and the low level in the Two states view.",
            finds: &[("rate", "Rate repeats every 3 min"), ("amplitude", "Amplitude repeats every 3 min")],
            duration_s: HOURS_2,
            rate: Box::new(|t| if t % 180.0 < 8.0 { RATE - 30.0 } else { RATE }),
            amplitude: Box::new(|t| {
                if t % 180.0 < 8.0 {
                    AMPLITUDE - 15.0
                } else {
                    AMPLITUDE
                }
            }),
            beat_error: steady(BEAT_ERROR),
        },
    ]
}

/// A random walk from `v`: steps each second whose spread grows to `sd`
/// over `over` seconds, through `duration` seconds, from `seed`.
fn walk(v: f64, sd: f64, over: f64, duration: f64, seed: u64) -> Track {
    let mut rng = Rng::new(seed);
    let step = sd / over.sqrt();
    let mut y = v;
    let path: Vec<f64> = (0..=duration.ceil() as usize + 1)
        .map(|_| {
            let here = y;
            y += step * rng.normal();
            here
        })
        .collect();
    Box::new(move |t| {
        let i = (t.max(0.0) as usize).min(path.len() - 2);
        let f = t.max(0.0) - i as f64;
        path[i] + f * (path[i + 1] - path[i])
    })
}

pub fn find(name: &str) -> Option<Scenario> {
    all().into_iter().find(|s| s.name == name)
}

/// The synthesis settings every scenario uses.
pub fn config(s: &Scenario, seed: u64) -> SynthConfig {
    SynthConfig {
        duration_s: s.duration_s,
        bph: BPH,
        lift_deg: LIFT_DEG,
        rate_s_per_day: 0.0,
        beat_error_ms: BEAT_ERROR,
        seed,
        ..Default::default()
    }
}

/// The scenario's sound in blocks of `block_s` seconds, in order, each
/// passed to `f`: a long take need not be held whole. The blocks join
/// without a seam.
pub fn render(
    s: &Scenario,
    seed: u64,
    block_s: f64,
    mut f: impl FnMut(&[f32]) -> Result<(), String>,
) -> Result<(), String> {
    let cfg = config(s, seed);
    let fs = cfg.sample_rate as f64;
    let total = (cfg.duration_s * fs) as usize;
    let block = ((block_s * fs) as usize).max(1);
    let nominal = 3600.0 / cfg.bph as f64;
    let noise_rms = 10f64.powf(-cfg.snr_db / 20.0);
    let mut beats = Rng::new(seed);
    let mut noise = Rng::new(seed ^ 0x5eed);
    let mut jitter = Rng::new(seed ^ 0x717e);
    // Samples from `base` on, beats added as they come.
    let mut acc: Vec<f64> = Vec::new();
    let mut base = 0usize;
    let (mut t, mut k) = (0.05, 0u64);
    while base < total {
        let end = (base + block).min(total);
        // Every beat with a sound before `end`: a beat's sounds start at
        // most a lift's time (well under 50 ms) before its drop.
        while t < cfg.duration_s - 0.05 && t < end as f64 / fs + 0.05 {
            let at = t + TIMING_JITTER_MS / 1000.0 * jitter.normal();
            let amp = (s.amplitude)(t) + AMPLITUDE_JITTER_DEG * jitter.normal();
            synth::render_beat(&cfg, k, at, amp, (s.beat_error)(t), &mut beats, |i, v| {
                if let Some(j) = usize::try_from(i).ok().and_then(|i| i.checked_sub(base)) {
                    if i as usize >= total {
                        return;
                    }
                    if acc.len() <= j {
                        acc.resize(j + 1, 0.0);
                    }
                    acc[j] += v;
                }
            });
            t += nominal * (1.0 - (s.rate)(t) / 86400.0);
            k += 1;
        }
        let n = end - base;
        if acc.len() < n {
            acc.resize(n, 0.0);
        }
        let out: Vec<f32> = acc
            .drain(..n)
            .map(|v| ((v + noise_rms * noise.normal()) * 0.5) as f32)
            .collect();
        f(&out)?;
        base = end;
    }
    Ok(())
}

/// The beats and 2 s windows the scenario's sound would give, made
/// straight from its tracks with the same beat-to-beat spread: the
/// analysis after the sound, quick enough for tests.
pub fn beat_log(s: &Scenario, seed: u64) -> crate::stream::BeatLog {
    let nominal = 3600.0 / BPH as f64;
    let mut rng = Rng::new(seed ^ 0x717e);
    let mut beats = Vec::new();
    let (mut t, mut k) = (0.05, 0i64);
    while t < s.duration_s - 0.05 {
        let side = if k % 2 == 0 { 0.5 } else { -0.5 };
        beats.push(crate::beats::Beat {
            index: k,
            time: t + side * (s.beat_error)(t) / 1000.0 + TIMING_JITTER_MS / 1000.0 * rng.normal(),
            quality: 1.0,
        });
        t += nominal * (1.0 - (s.rate)(t) / 86400.0);
        k += 1;
    }
    // A 2 s window averages 16 beats, 8 a side.
    let per = 2.0 / nominal;
    let amplitude_windows = (0..(s.duration_s / 2.0) as usize)
        .map(|i| {
            let (a, b) = (2.0 * i as f64, 2.0 * (i + 1) as f64);
            let mid = 0.5 * (a + b);
            let amp = |r: &mut Rng| {
                (s.amplitude)(mid) + AMPLITUDE_JITTER_DEG * (2.0 / per).sqrt() * r.normal()
            };
            let be = (s.beat_error)(mid) + TIMING_JITTER_MS * 2.0 / per.sqrt() * rng.normal();
            crate::amplitude::AmplitudeWindow {
                start_s: a,
                end_s: b,
                even_deg: Some(amp(&mut rng)),
                odd_deg: Some(amp(&mut rng)),
                beat_error_ms: Some(be),
                beat_error_unlock_ms: Some(be),
                even_unlock_ms: None,
                odd_unlock_ms: None,
                even_drop_ms: None,
                odd_drop_ms: None,
            }
        })
        .collect();
    crate::stream::BeatLog {
        sample_rate: 48000,
        duration_s: s.duration_s,
        bph: BPH,
        lift_deg: LIFT_DEG,
        beats,
        amplitude_windows,
        clipped_beats: vec![],
        clipped_samples: 0,
        jumps: vec![],
        sided: vec![],
        by_count: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_tracks_are_finite() {
        let all = all();
        for (i, s) in all.iter().enumerate() {
            assert!(all[..i].iter().all(|o| o.name != s.name), "{}", s.name);
            for t in [0.0, 1.5, 4199.0, 4201.0, s.duration_s] {
                for v in [(s.rate)(t), (s.amplitude)(t), (s.beat_error)(t)] {
                    assert!(v.is_finite(), "{} at {t}", s.name);
                }
            }
        }
    }

    #[test]
    fn blocks_join_without_a_seam() {
        // The same take rendered in one block and in many is the same
        // sound, sample for sample.
        let mut s = find("fourth-wheel").unwrap();
        s.duration_s = 20.0;
        let mut whole = Vec::new();
        render(&s, 3, 20.0, |b| {
            whole.extend_from_slice(b);
            Ok(())
        })
        .unwrap();
        let mut parts = Vec::new();
        render(&s, 3, 0.7, |b| {
            parts.extend_from_slice(b);
            Ok(())
        })
        .unwrap();
        assert_eq!(whole.len(), 20 * 48000);
        assert!(whole == parts);
    }

    #[test]
    fn the_analysis_finds_what_each_scenario_hides() {
        use crate::longterm::LongConfig;
        use crate::steadiness::{self, Config};
        let wheels = crate::periodicity::standard_wheels(BPH, 20);
        for s in all() {
            let log = beat_log(&s, 1);
            let lc = LongConfig {
                wheels: wheels.clone(),
                ..Default::default()
            };
            let long = crate::longrun::analyse(&log, None, &lc);
            let report = steadiness::check(&log, None, &long, &lc, &Config::default());
            let mut shown = Vec::new();
            for c in &report.series {
                let name = c.series.name().replace(' ', "_");
                let want = s.finds.iter().find(|f| f.0 == name);
                match want {
                    Some((_, start)) => assert!(
                        c.headline.starts_with(start),
                        "{}: {name}: {}",
                        s.name,
                        c.headline
                    ),
                    // A series with nothing hidden reads as such.
                    None => assert!(
                        c.cycle.is_none()
                            && !c.headline.contains("shifts")
                            && !c.headline.contains("slides"),
                        "{}: {name}: {}",
                        s.name,
                        c.headline
                    ),
                }
                shown.push(c.headline.clone());
            }
            eprintln!("{}: {}", s.name, shown.join(" | "));
        }
    }
}
