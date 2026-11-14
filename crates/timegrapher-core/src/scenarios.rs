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
    let mut all = cycle_scenarios();
    all.extend(change_scenarios());
    all
}

/// The control, and the scenarios with a cycle in them.
fn cycle_scenarios() -> Vec<Scenario> {
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
            name: "line-switches-on",
            summary: "From 60 min on, the rate swings 3 s/d peak to peak every 60 s, one turn of the fourth wheel; before that it is steady.",
            expect: "Rate repeats every 60 s (the fourth wheel) over the take as a whole; the STFT and SST maps (series --json --maps) show the 60 s line only from about 60 min.",
            finds: &[("rate", "Rate repeats every 60 s (the fourth wheel)")],
            duration_s: HOURS_2,
            rate: Box::new(|t| {
                if t < 3600.0 {
                    RATE
                } else {
                    RATE + 1.5 * (std::f64::consts::TAU * t / 60.0).sin()
                }
            }),
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
    ]
}

/// The scenarios where a series slides, steps, wanders or drops for a while.
fn change_scenarios() -> Vec<Scenario> {
    vec![
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
            expect: "Rate wanders with no cycle: the change finder splits the walk into a level shift or two, the autocorrelation falls slowly and the Allan deviation rises above the white line at long averaging times.",
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
    second_by_second(path)
}

/// A track through `path`, one value a second, straight between them.
fn second_by_second(path: Vec<f64>) -> Track {
    Box::new(move |t| {
        let i = (t.max(0.0) as usize).min(path.len() - 2);
        let f = t.max(0.0) - i as f64;
        path[i] + f * (path[i + 1] - path[i])
    })
}

/// A steady watch whose rate, amplitude and beat error each wander as
/// AR(1) noise, no cycle, step or trend (#659): a step each second
/// correlated `rho^(1/10)` with the last, so readings 10 s apart
/// correlate at about `rho` (an Ornstein–Uhlenbeck path). Each size is
/// about three times the readings' own scatter from beat-to-beat spread.
pub fn steady_red(duration_s: f64, rho: f64, seed: u64) -> Scenario {
    let phi = rho.powf(0.1);
    let ar = |v: f64, sd: f64, salt: u64| -> Track {
        let mut rng = Rng::new(seed ^ salt);
        let innov = sd * (1.0 - phi * phi).sqrt();
        let mut x = sd * rng.normal();
        let path = (0..=duration_s.ceil() as usize + 1)
            .map(|_| {
                let here = v + x;
                x = phi * x + innov * rng.normal();
                here
            })
            .collect();
        second_by_second(path)
    };
    Scenario {
        name: "steady-red",
        summary: "Steady, with AR(1) noise in every series and no cycle, step or trend.",
        expect: "Nothing stands out but the correlation between readings.",
        finds: &[],
        duration_s,
        rate: ar(RATE, 3.0, 0xa1),
        amplitude: ar(AMPLITUDE, 1.5, 0xa2),
        beat_error: ar(BEAT_ERROR, 0.15, 0xa3),
    }
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
            t += crate::synth::period(nominal, (s.rate)(t));
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
        t += crate::synth::period(nominal, (s.rate)(t));
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
                culled_ms: None,
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
        snippets: Vec::new(),
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
    fn the_time_frequency_maps_show_when_a_line_switches_on() {
        use crate::longterm::LongConfig;
        let s = find("line-switches-on").unwrap();
        let log = beat_log(&s, 1);
        let lc = LongConfig {
            wheels: crate::periodicity::standard_wheels(BPH, 20),
            ..Default::default()
        };
        let long = crate::longrun::analyse(&log, None, &lc);
        let maps = crate::timefreq::for_take(&log, None, &long, &lc);
        // The swing's timing: 1.5 s/d over 60 s is 0.17 ms of timing
        // either way.
        let size = 1.5 / 86400.0 * 60.0 / std::f64::consts::TAU * 1000.0;
        for m in maps.iter().filter(|m| m.series == "rate") {
            let r = (0..m.period_s.len())
                .min_by(|&a, &b| {
                    let d = |i: usize| (m.period_s[i] / 60.0).ln().abs();
                    d(a).total_cmp(&d(b))
                })
                .unwrap();
            let reach = m.window_s.map_or(300.0, |w| w / 2.0 + 60.0);
            let (mut before, mut after) = (Vec::new(), Vec::new());
            for (t, col) in m.t_s.iter().zip(&m.values) {
                let v = col[r - 1].max(col[r]).max(col[r + 1]);
                if !v.is_finite() {
                    continue;
                }
                if *t < 3600.0 - reach {
                    before.push(v);
                } else if *t > 3600.0 + reach {
                    after.push(v);
                }
            }
            let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
            assert!(before.len() > 2 && after.len() > 2, "{}", m.method);
            assert!(
                mean(&before) < 0.25 * size,
                "{}: {}",
                m.method,
                mean(&before)
            );
            assert!(mean(&after) > 0.6 * size, "{}: {}", m.method, mean(&after));
        }
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

    /// The whole pane on a steady watch (#659): how many 2 h control
    /// takes, of 200 seeds, name anything at all, a finding or a line the
    /// headline adds. The target is 95 in 100 naming nothing (at most 10
    /// of 200): with Holm's correction across the pane and the whitened
    /// search (#723) it leaves 8. This count may only fall.
    #[test]
    fn a_steady_watch_names_nothing_on_the_whole_pane() {
        use crate::longterm::LongConfig;
        use crate::steadiness::{self, Config};
        let wheels = crate::periodicity::standard_wheels(BPH, 20);
        let s = find("control").unwrap();
        let quiet = "no cycle, level change, trend or correlation between readings detected.";
        let lc = LongConfig {
            wheels,
            ..Default::default()
        };
        let names = |seed: u64| {
            let log = beat_log(&s, seed);
            let long = crate::longrun::analyse(&log, None, &lc);
            let r = steadiness::check(&log, None, &long, &lc, &Config::default());
            !r.findings.is_empty() || r.series.iter().any(|c| !c.headline.ends_with(quiet))
        };
        // The seeds in four interleaved sets, side by side (#907).
        let named: usize = std::thread::scope(|sc| {
            let runs: Vec<_> = (1..=4u64)
                .map(|k| sc.spawn(move || (k..=200).step_by(4).filter(|&seed| names(seed)).count()))
                .collect();
            runs.into_iter().map(|h| h.join().unwrap()).sum()
        });
        assert!(named <= 8, "{named} of 200 steady takes name something");
    }

    /// The cycles `long` and a session list are the ones the pane names
    /// (#659). On control seed 130 the period search finds one amplitude
    /// line that Holm's correction across the pane drops (seed 33's, until
    /// the search was calibrated, #1195): with the family
    /// held, no list names it. On takes with real cycles, each list keeps
    /// what the pane names, no more and no less.
    #[test]
    fn every_list_of_cycles_names_what_the_pane_names() {
        use crate::longterm::LongConfig;
        use crate::steadiness::{self, Config, SeriesKind};
        let lc = LongConfig {
            wheels: crate::periodicity::standard_wheels(BPH, 20),
            ..Default::default()
        };
        for (name, seed) in [
            ("control", 130),
            ("fourth-wheel", 1),
            ("escape-wheel", 1),
            ("slow-cycle", 1),
        ] {
            let log = beat_log(&find(name).unwrap(), seed);
            let mut long = crate::longrun::analyse(&log, None, &lc);
            let found = long.amplitude.components.len();
            let r = steadiness::check(&log, None, &long, &lc, &Config::default());
            steadiness::family_long(&log, None, &mut long, &lc);
            let pane = |k: SeriesKind| -> Vec<f64> {
                let p = r.series.iter().find(|c| c.series == k).unwrap();
                p.period.as_ref().map_or(vec![], |q| {
                    std::iter::once(q.period_s)
                        .chain(q.other_periods_s.iter().copied())
                        .collect()
                })
            };
            let rate: Vec<f64> = long
                .rate_components
                .iter()
                .map(|c| c.component.period_s)
                .collect();
            let amp: Vec<f64> = long
                .amplitude
                .components
                .iter()
                .map(|c| c.period_s)
                .collect();
            assert_eq!(rate, pane(SeriesKind::Rate), "{name} rate");
            assert_eq!(amp, pane(SeriesKind::Amplitude), "{name} amplitude");
            let cycles = crate::session::cycles(&long, &lc.wheels);
            assert_eq!(cycles.len(), rate.len() + amp.len(), "{name}");
            // Each carries core's harmonic mark as it is (#762).
            let marks = long
                .rate_components
                .iter()
                .map(|c| c.component.harmonic_of)
                .chain(long.amplitude.components.iter().map(|c| c.harmonic_of));
            for (c, mark) in cycles.iter().zip(marks) {
                assert_eq!(c.harmonic_of, mark, "{name} {}", c.period_s);
            }
            if name == "control" {
                assert_eq!(found, 1, "the line Holm drops");
                assert!(amp.is_empty());
            } else {
                assert!(!cycles.is_empty(), "{name} names its cycle");
            }
        }
    }

    /// How many of the steady takes `seeds` of `duration_s`, their rate,
    /// amplitude and beat error wandering as AR(1) noise of lag-one
    /// correlation `rho` from reading to reading ([`super::steady_red`]),
    /// name anything on the whole pane other than the correlation itself:
    /// a finding (at ρ 0, "wandering" too, since nothing correlates) or a
    /// line the period search "also finds".
    fn red_pane_named(duration_s: f64, rho: f64, seeds: std::ops::RangeInclusive<u64>) -> usize {
        use crate::longterm::LongConfig;
        use crate::steadiness::{self, Config};
        let lc = LongConfig {
            wheels: crate::periodicity::standard_wheels(BPH, 20),
            ..Default::default()
        };
        seeds
            .filter(|&seed| {
                let log = beat_log(&super::steady_red(duration_s, rho, seed), seed);
                let long = crate::longrun::analyse(&log, None, &lc);
                let r = steadiness::check(&log, None, &long, &lc, &Config::default());
                // The report states the setting the pane ran with.
                assert!(!r.config.acf_alone);
                r.findings
                    .iter()
                    .any(|f| f.code != "wandering" || rho == 0.0)
                    || r.series.iter().any(|c| c.headline.contains("also finds"))
            })
            .count()
    }

    /// Each cell's count of 100 seeds against its cap, the cells run
    /// side by side.
    fn red_pane_within(cells: &[(f64, f64, usize)]) {
        let cells: Vec<_> = cells
            .iter()
            .map(|&(d, rho, cap)| (d, rho, 100, cap))
            .collect();
        red_pane_within_of(&cells);
    }

    /// Each cell's count of its first `seeds` seeds against its cap, the
    /// cells, and each hundred seeds of a cell, run side by side. Every
    /// cell over its cap is told at once, so none hides behind another.
    fn red_pane_within_of(cells: &[(f64, f64, u64, usize)]) {
        let named: Vec<usize> = std::thread::scope(|sc| {
            let runs: Vec<Vec<_>> = cells
                .iter()
                .map(|&(d, rho, seeds, _)| {
                    (0..seeds.div_ceil(100))
                        .map(|k| {
                            let range = k * 100 + 1..=((k + 1) * 100).min(seeds);
                            sc.spawn(move || red_pane_named(d, rho, range))
                        })
                        .collect()
                })
                .collect();
            runs.into_iter()
                .map(|hs| hs.into_iter().map(|h| h.join().unwrap()).sum())
                .collect()
        });
        let over: Vec<String> = cells
            .iter()
            .zip(named)
            .filter(|&(&(.., cap), n)| n > cap)
            .map(|(&(d, rho, seeds, cap), n)| {
                format!("{d} s at ρ {rho}: {n} of {seeds} steady takes name something (cap {cap})")
            })
            .collect();
        assert!(over.is_empty(), "{}", over.join("; "));
    }

    /// The whole pane on steady watches whose readings follow themselves
    /// (#659, R9): 100 seeds a cell, the trend's F test and the period
    /// search held to Holm's correction, and the autocorrelation only
    /// refining a cycle the search kept. Each cap is the count the pane
    /// gave once the period search was calibrated against its simulated
    /// null (#1195), and may only fall. The target is 5: 30 min and 2 h
    /// miss it at ρ 0.9 (#916's caveat, #917). 2 h at ρ 0 to 0.5 is the
    /// control test above; 8 h and 24 h are the test below.
    ///
    /// With the one 2 s tiling (#636) three cells are held on 300 seeds at
    /// their measured count, as Peter chose on 2026-11-05: 30 min at ρ 0
    /// names something on 20 of 300 (18 before the tiling), missing the
    /// target before and after (#1195), 2 h at ρ 0.7 on 3 of 300 (2
    /// before), and 30 min at ρ 0.7 on 13 of 300 (6 of the first 100, as
    /// before the tiling). Peter raised 30 min at ρ 0.7 to 15 of 300, the
    /// pane's 5% target, on 2026-11-13 ("Raise it to 15 (5% target)",
    /// #1601): with the sloped timing detrend it names 14, the extra being
    /// chance first lines on a slow wander, and 12 and 10 on fresh seeds.
    #[test]
    fn a_steady_watch_with_red_noise_names_little_on_the_whole_pane() {
        red_pane_within_of(&[
            (1800.0, 0.0, 300, 20),
            (1800.0, 0.7, 300, 15),
            (7200.0, 0.7, 300, 3),
        ]);
        red_pane_within(&[
            (1800.0, 0.3, 0),
            (1800.0, 0.5, 1),
            (1800.0, 0.9, 20),
            (7200.0, 0.9, 7),
        ]);
    }

    /// The rest of the grid, too slow for every run (about 25 minutes on
    /// one core): every cell meets the target of 5. With the one 2 s
    /// tiling (#636), 2 h at ρ 0 is held on 300 seeds at its measured
    /// count, 3 (1 of the first 100, 0 before the tiling).
    #[test]
    #[ignore = "8 h and 24 h takes: run by hand"]
    fn long_steady_watches_with_red_noise_name_little_on_the_whole_pane() {
        red_pane_within_of(&[(7200.0, 0.0, 300, 3)]);
        red_pane_within(&[
            (7200.0, 0.3, 2),
            (7200.0, 0.5, 3),
            (28800.0, 0.0, 1),
            (28800.0, 0.3, 1),
            (28800.0, 0.5, 0),
            (28800.0, 0.7, 0),
            (28800.0, 0.9, 1),
            (86400.0, 0.0, 0),
            (86400.0, 0.3, 0),
            (86400.0, 0.5, 0),
            (86400.0, 0.7, 0),
            (86400.0, 0.9, 1),
        ]);
    }

    /// The caveat that a short take's cycle may be noise goes on the
    /// amplitude and the beat error only (#916). Seed 135 of 30 min of red
    /// noise (seed 35 until the search was calibrated, #1195; 112 until the
    /// timing's detrend took a local line off first) names a cycle
    /// in the rate, whose search read ρ over 0.7, so
    /// the rule would give it one; its series carries none. Seed 37 names
    /// one in the amplitude, which carries it and says so in its headline.
    #[test]
    fn only_the_amplitude_and_beat_error_say_a_short_takes_cycle_may_be_noise() {
        use crate::longterm::LongConfig;
        use crate::measure::Caveat;
        use crate::steadiness::{self, Config};
        let lc = LongConfig {
            wheels: crate::periodicity::standard_wheels(BPH, 20),
            ..Default::default()
        };
        let pane = |seed| {
            let log = beat_log(&super::steady_red(1800.0, 0.9, seed), seed);
            let long = crate::longrun::analyse(&log, None, &lc);
            let r = steadiness::check(&log, None, &long, &lc, &Config::default());
            (long, r)
        };
        let noise = |s: &steadiness::SeriesCheck| {
            s.caveats
                .iter()
                .any(|c| matches!(c, Caveat::PeriodMayBeNoise { .. }))
        };
        let (long, r) = pane(135);
        assert!(r.series[0].period.is_some(), "the rate names a cycle");
        assert!(crate::longterm::period_may_be_noise(&long.timing).is_some());
        assert!(!noise(&r.series[0]));
        let (_, r) = pane(37);
        let amp = &r.series[1];
        assert!(amp.period.is_some() && noise(amp), "{:?}", amp.caveats);
        assert!(
            amp.headline.contains("this cycle may be noise"),
            "{}",
            amp.headline
        );
    }

    /// A cycle the search finds is taken out as sines at its kept
    /// harmonics, not as its fold: a staircase of median per phase bin
    /// left a sawtooth at the bin rate on a steeply wandering timing, and
    /// the next search named its harmonics. Seeds 23 and 154 of 30 min at
    /// ρ 0.7 named 11.8 s and 11.0 s at 16, and 41.6 s, 6.2 s and 7.0 s,
    /// after a slow wander (306.6 s, 209.6 s) that is itself a chance hit
    /// of its null (Norm). Now each names the wander alone.
    #[test]
    fn a_wander_taken_out_leaves_no_lines_at_its_bins_rate() {
        use crate::longterm::LongConfig;
        let lc = LongConfig {
            wheels: crate::periodicity::standard_wheels(BPH, 20),
            ..Default::default()
        };
        for seed in [23, 154] {
            let log = beat_log(&super::steady_red(1800.0, 0.7, seed), seed);
            let long = crate::longrun::analyse(&log, None, &lc);
            let found: Vec<f64> = long.timing.components.iter().map(|c| c.period_s).collect();
            assert!(found.len() <= 1, "seed {seed}: {found:?}");
        }
    }
}
