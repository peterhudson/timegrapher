//! A recording that opens with the stand's noise before the watch is
//! placed (#844): the whole file reads the watch, with the lead left out
//! and named, as long as the watch is heard for long enough after it.

use timegrapher_core::audio::Audio;
use timegrapher_core::cores::on_every_core;
use timegrapher_core::measure::{Caveat, RATE_WINDOW_STEP_S};
use timegrapher_core::stream::StreamConfig;
use timegrapher_core::synth::{generate, noise_rms, Rng, SynthConfig, SCALE};
use timegrapher_core::{analysis::reads_better, analyze, take, AnalysisConfig};

/// `noise_s` of the synthetic stand noise (as the live tests make it),
/// then `watch_s` of a watch at +5 s/d and 270°.
fn noise_then_watch(noise_s: f64, watch_s: f64) -> (Audio, Audio) {
    noise_then(noise_s, watch_s, 28_800, 5)
}

/// [`noise_then_watch`] at `bph`, the noise drawn from `seed`.
fn noise_then(noise_s: f64, watch_s: f64, bph: u32, seed: u64) -> (Audio, Audio) {
    let watch = watch_at(bph, watch_s);
    let led = noise_before(noise_s, &watch, seed);
    (watch, led)
}

/// `watch_s` of a watch at `bph`, +5 s/d and 270°.
fn watch_at(bph: u32, watch_s: f64) -> Audio {
    generate(
        &SynthConfig {
            duration_s: watch_s,
            bph,
            rate_s_per_day: 5.0,
            beat_error_ms: 0.4,
            ..Default::default()
        },
        |_| 270.0,
        |_| 0.0,
    )
}

/// `noise_s` of the synthetic stand noise drawn from `seed`, then `watch`.
fn noise_before(noise_s: f64, watch: &Audio, seed: u64) -> Audio {
    let rms = noise_rms(SynthConfig::default().snr_db) * SCALE;
    let mut rng = Rng::new(seed);
    let n = (noise_s * watch.sample_rate as f64) as usize;
    let mut samples: Vec<f32> = (0..n).map(|_| (rms * rng.normal()) as f32).collect();
    samples.extend_from_slice(&watch.samples);
    Audio {
        samples,
        sample_rate: watch.sample_rate,
    }
}

fn lead_left_out(c: &[Caveat]) -> Option<f64> {
    c.iter().find_map(|c| match c {
        Caveat::LeadLeftOut { seconds } => Some(*seconds),
        _ => None,
    })
}

#[test]
fn a_watch_after_a_long_noise_lead_reads_as_the_watch_with_the_lead_named() {
    for (noise_s, watch_s) in [(49.0, 30.0), (60.0, 60.0), (120.0, 40.0)] {
        let what = format!("{noise_s} s of noise, then {watch_s} s of watch");
        let (alone, led) = noise_then_watch(noise_s, watch_s);
        let cfg = AnalysisConfig::default();
        let want = analyze(&alone, &cfg).summary;
        let got = analyze(&led, &cfg).summary;
        assert!(want.watch_heard, "{what}: the watch alone");
        assert!(
            lead_left_out(&want.caveats).is_none(),
            "{what}: no lead on the watch alone"
        );
        assert!(got.watch_heard, "{what}: {:?}", got.caveats);
        let lead = lead_left_out(&got.caveats).expect("the lead named");
        assert!((lead - noise_s).abs() < 1.0, "{what}: lead {lead}");
        let near = |a: Option<f64>, b: Option<f64>, tol: f64, name: &str| {
            let (a, b) = (a.unwrap(), b.unwrap());
            assert!((a - b).abs() <= tol, "{what}: {name} {a} against {b}");
        };
        // The beats found are counted against those expected over the
        // sound after the lead, the time listened (#1417).
        for s in [&want, &got] {
            let f = s.beats_found_fraction;
            assert!((0.97..1.03).contains(&f), "{what}: found {f}");
        }
        near(got.rate_s_per_day, want.rate_s_per_day, 0.3, "rate");
        near(got.amplitude_deg, want.amplitude_deg, 1.0, "amplitude");
        near(
            got.beat_error_unlock_ms,
            want.beat_error_unlock_ms,
            0.03,
            "beat error",
        );
    }
}

/// At 18,000 and 19,800 bph a 10 s window of the noise can scatter under
/// the doubt line (0.7% of a beat), far more than the watch after it
/// (#1418): the lead runs on to where the scatter drops to the watch's,
/// not to the first such noise window, and the noise's beats stay out of
/// the rate. With the rate set too, as the guess is the same.
#[test]
fn a_noise_lead_before_a_slower_watch_ends_where_the_watch_starts() {
    for (bph, noise_s, seed) in [
        (18_000, 49.0, 5),
        (18_000, 60.0, 5),
        (18_000, 60.0, 21),
        (19_800, 60.0, 5),
        (19_800, 60.0, 9),
        (19_800, 49.0, 5),
        // Seeds the fix was not tuned on.
        (18_000, 60.0, 101),
        (18_000, 75.0, 404),
        (19_800, 60.0, 202),
        (19_800, 49.0, 505),
        (21_600, 60.0, 303),
        (21_600, 49.0, 606),
        (18_000, 60.0, 707),
        (19_800, 60.0, 808),
        (21_600, 60.0, 909),
    ] {
        let what = format!("{bph} bph after {noise_s} s of noise ({seed})");
        let (alone, led) = noise_then(noise_s, 60.0, bph, seed);
        let want = analyze(&alone, &AnalysisConfig::default()).summary;
        for set in [None, Some(bph)] {
            let cfg = AnalysisConfig {
                bph: set,
                ..Default::default()
            };
            let got = analyze(&led, &cfg).summary;
            assert_eq!(got.bph, bph, "{what}");
            let lead = lead_left_out(&got.caveats).expect("the lead named");
            // Within a 5 s window step of the watch's start before it and
            // two after: the windows step 5 s, and none spans the jump
            // onto the watch's beats. The rate says the noise is out.
            assert!(
                (noise_s - 5.0..noise_s + 10.0).contains(&lead),
                "{what}, set {set:?}: lead {lead}"
            );
            let (r, w) = (got.rate_s_per_day.unwrap(), want.rate_s_per_day.unwrap());
            assert!((r - w).abs() <= 0.3, "{what}: rate {r} against {w}");
        }
    }
}

/// A watch heard from the start whose first `rough_s` scatter about
/// `sd_frac` of a beat: each beat there is moved by a draw of that SD,
/// cut at the midpoints between the beats `analyze` finds.
fn rough_start(bph: u32, rough_s: f64, sd_frac: f64, seed: u64) -> Audio {
    scatter(&watch_at(bph, 120.0), bph, rough_s, sd_frac, seed)
}

/// `watch` with each beat before `upto_s` moved by a draw of `sd_frac`
/// of a beat, cut at the midpoints between the beats `analyze` finds.
fn scatter(watch: &Audio, bph: u32, upto_s: f64, sd_frac: f64, seed: u64) -> Audio {
    let fs = f64::from(watch.sample_rate);
    let beats = analyze(watch, &AnalysisConfig::default()).beats;
    let beat_s = 3600.0 / f64::from(bph);
    let mut rng = Rng::new(seed);
    let mut out = watch.samples.clone();
    for b in beats.iter().filter(|b| b.time > beat_s && b.time < upto_s) {
        let (a, z) = (
            ((b.time - beat_s / 2.0) * fs) as usize,
            ((b.time + beat_s / 2.0) * fs) as usize,
        );
        let shift = (rng.normal() * sd_frac * beat_s * fs).round() as isize;
        for (i, v) in out.iter_mut().enumerate().take(z).skip(a) {
            let from = i as isize - shift;
            *v = if (a as isize..z as isize).contains(&from) {
                watch.samples[from as usize]
            } else {
                0.0
            };
        }
    }
    Audio {
        samples: out,
        sample_rate: watch.sample_rate,
    }
}

/// A noise window can scatter under a watch's line when the watch
/// scatters 0.1% of a beat: on this noise (seed 101) the window from 45 s
/// reads 0.34% against a line of 0.40%, and the window after it holds
/// no beat clear enough to fit, so the next window clear of it is the
/// one across the step onto the watch at 60 s, which reads as the
/// watch's (#1418). Such a pair is not taken as the watch's: the lead
/// runs to the watch and the rate stays the watch's alone, guessed and
/// set. With 0.05% of scatter, and every acceptance seed, the same.
#[test]
fn a_noise_window_paired_across_the_step_onto_the_watch_stays_in_the_lead() {
    let bph = 18_000;
    for (sd, seed) in [(0.001, 101), (0.0005, 101), (0.001, 2004), (0.001, 3001)] {
        let what = format!("watch scattering {sd} of a beat after noise {seed}");
        let watch = scatter(&watch_at(bph, 60.0), bph, 60.0 - 0.2, sd, 77);
        let want = analyze(&watch, &AnalysisConfig::default()).summary;
        let led = noise_before(60.0, &watch, seed);
        for set in [None, Some(bph)] {
            let cfg = AnalysisConfig {
                bph: set,
                ..Default::default()
            };
            let got = analyze(&led, &cfg).summary;
            assert_eq!(got.bph, bph, "{what}");
            let lead = lead_left_out(&got.caveats).expect("the lead named");
            assert!((55.0..70.0).contains(&lead), "{what}: lead {lead}");
            let (r, w) = (got.rate_s_per_day.unwrap(), want.rate_s_per_day.unwrap());
            assert!((r - w).abs() <= 0.3, "{what}: rate {r} against {w}");
        }
    }
}

/// A watch heard from the start whose first 30 to 60 s scatter about
/// 0.5% of a beat, under the doubt line, and then 0.01%: that rough start
/// scatters more than 4 times the windows after it, so it is left out as
/// a lead would be (#1418). The rate stays the watch's.
#[test]
fn a_watchs_rough_start_is_left_out_as_a_lead() {
    for (bph, rough_s) in [
        (28_800, 45.0),
        (18_000, 45.0),
        (21_600, 60.0),
        (28_800, 30.0),
    ] {
        let what = format!("{bph} bph, rough for {rough_s} s");
        let s = analyze(
            &rough_start(bph, rough_s, 0.005, 3),
            &AnalysisConfig::default(),
        )
        .summary;
        assert!(s.watch_heard, "{what}");
        let lead = lead_left_out(&s.caveats).expect("the rough start left out");
        assert!(lead > 10.0 && lead <= rough_s + 1.0, "{what}: lead {lead}");
        let r = s.rate_s_per_day.unwrap();
        assert!((r - 5.0).abs() <= 0.3, "{what}: rate {r}");
    }
}

/// Under 20 s of watch after the noise, nothing is left out: the file
/// stays no watch, as a set average over such a stretch does live. The
/// 11 s cases are the ones `LEAD_WATCH_MIN_S` decides: with no floor they
/// would read as a watch with the lead left out.
#[test]
fn a_brief_watch_after_a_long_noise_lead_is_still_no_watch() {
    for (noise_s, watch_s) in [(110.0, 15.0), (60.0, 11.0), (110.0, 11.0)] {
        let what = format!("{noise_s} s of noise, then {watch_s} s of watch");
        let (_, led) = noise_then_watch(noise_s, watch_s);
        let got = analyze(&led, &AnalysisConfig::default()).summary;
        assert!(!got.watch_heard, "{what}: {:?}", got.caveats);
        assert!(lead_left_out(&got.caveats).is_none(), "{what}");
    }
}

/// An entry of a take that starts into its recording names the lead it
/// leaves out from its own start, not the recording's (#756).
#[test]
fn an_entrys_lead_is_counted_from_its_own_start() {
    let (_, led) = noise_then_watch(60.0, 40.0);
    let dir = std::env::temp_dir().join(format!("tg-entry-lead-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    timegrapher_core::audio::write_wav(&dir.join("take.wav"), &led).unwrap();
    let m: timegrapher_core::session::Manifest = toml::from_str(
        "bph = 28800\nsettle_s = 0\n\n[[recording]]\nfile = \"take.wav\"\nposition = \"DU\"\nstart_s = 20.0\n",
    )
    .unwrap();
    let s = take::EntrySettings {
        stream: take::stream_settings(&m, None, take::Given::default()),
        card_clock: false,
        settle_s: None,
    };
    let read =
        take::measure_entry(&dir, &m, &m.recordings[0], &s, |_| Ok(None), |_| Ok(None)).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(read.start_s, 20.0);
    let lead = lead_left_out(&read.measurement.caveats).expect("the lead named");
    assert!((lead - 40.0).abs() < 1.0, "lead {lead}");
}

/// A long lead of stand noise before a slow watch, where the noise is as
/// loud as half the watch's ticks, does not set the rate (#1423): with
/// no rate set the rate is guessed again as session reads the file, and
/// the recording reads as it does with the rate set; with the rate set,
/// the noise is not heard as another rate. Before, `analyze` guessed from
/// the noise's first 10 s (43,200 or 28,800 bph) and found no watch.
/// The lead ends before the start of the first stream chunk at or after
/// the noise's end, plus one lead window step and 0.5 s: at 14,400 after
/// 120 s it runs to 141 s, where the watch's first 21 s, in a chunk that
/// opens on noise, track too loosely to be kept. Its rate is still the
/// watch's. Seeds 5 and 33 are the ones the lead work was built on and
/// keep the bounds read off them. Each later set was run once: 3011 to
/// 3019 found a jump in the noise taken as the watch's start (3012),
/// 3031 to 3039 a watch read at the noise's rate (3034), and 3071 to
/// 3079 and 3091 to 3099, at 120 s and one per rate at 60 to 90 s,
/// passed.
#[test]
fn a_long_noise_lead_does_not_set_the_rate() {
    // The seeds the lead work was built on keep the bounds read off them.
    let mut cases = vec![
        (14_400, 120.0, 5, 141.5),
        (18_000, 120.0, 5, 130.0),
        (19_800, 120.0, 5, 130.0),
        (14_400, 90.0, 33, 100.0),
    ];
    let chunk_s = StreamConfig::default().chunk_s;
    let bound = |noise_s: f64| (noise_s / chunk_s).ceil() * chunk_s + RATE_WINDOW_STEP_S + 0.5;
    for (i, bph) in [14_400, 18_000, 19_800].into_iter().enumerate() {
        let noise_s = [60.0, 75.0, 90.0][i];
        for first in [3071, 3091] {
            cases.push((bph, noise_s, first + 3 * i as u64, bound(noise_s)));
        }
        for first in [3011, 3031, 3071, 3091] {
            cases.extend((0..3).map(|k| (bph, 120.0, first + 3 * i as u64 + k, bound(120.0))));
        }
    }
    // The watch alone, and its reading, are the same for every case at
    // one rate: made once per rate. The cases run over the machine's cores.
    let alone: Vec<(u32, Audio, f64)> = on_every_core(&[14_400, 18_000, 19_800], |&bph| {
        let alone = watch_at(bph, 60.0);
        let want = analyze(&alone, &AnalysisConfig::default()).summary;
        (bph, alone, want.rate_s_per_day.unwrap())
    });
    on_every_core(&cases, |&(bph, noise_s, seed, lead_to)| {
        let what = format!("{bph} bph after {noise_s} s of noise ({seed})");
        let (_, alone, w) = alone.iter().find(|a| a.0 == bph).unwrap();
        let led = noise_before(noise_s, alone, seed);
        for set in [None, Some(bph)] {
            let cfg = AnalysisConfig {
                bph: set,
                ..Default::default()
            };
            let got = analyze(&led, &cfg).summary;
            assert_eq!(got.bph, bph, "{what}, set {set:?}");
            assert!(got.watch_heard, "{what}, set {set:?}: {:?}", got.caveats);
            assert_eq!(got.bph_heard, None, "{what}, set {set:?}");
            assert_eq!(got.bph_set, set, "{what}");
            let r = got.rate_s_per_day.unwrap();
            assert!((r - w).abs() <= 0.3, "{what}: rate {r} against {w}");
            let lead = lead_left_out(&got.caveats).expect("the lead named");
            assert!(
                (noise_s - 5.0..lead_to).contains(&lead),
                "{what}, set {set:?}: lead {lead}"
            );
        }
    });
}

/// Stand noise alone, with no rate set, is still no watch and names no
/// lead: guessing again (#1423) gives the noise a second chance.
#[test]
fn stand_noise_alone_is_no_watch_when_guessed_again() {
    let none = Audio {
        samples: Vec::new(),
        sample_rate: watch_at(28_800, 1.0).sample_rate,
    };
    for noise_s in [60.0, 180.0] {
        for seed in [3021, 3022, 3023] {
            let what = format!("{noise_s} s of noise ({seed})");
            let got = analyze(
                &noise_before(noise_s, &none, seed),
                &AnalysisConfig::default(),
            )
            .summary;
            assert!(!got.watch_heard, "{what}: {:?}", got.caveats);
            assert!(lead_left_out(&got.caveats).is_none(), "{what}");
        }
    }
}

/// A read at a harmonic of the watch's rate loses to the read at its own
/// rate whichever comes first (#1423), on the watch alone and with the
/// stand's noise over it. Seeds 3101 to 3105 found a read at 3:4 that
/// keeps the cleanest third of the beats with smaller residuals; 3111 to
/// 3115 a read at twice the rate that heard a watch at half the share.
/// 3121 to 3130 were run once after the share was compared first, and
/// 3131 to 3140 after its expected count and standard error were set
/// right for shares near 1.
#[test]
fn a_read_at_a_harmonic_loses_to_the_watchs_own_rate() {
    // Each watch, and its read at its own rate, serve both of its
    // harmonics: made once per rate and noise. Those run over the
    // machine's cores.
    let noises = (3101..=3105).chain(3111..=3115).chain(3121..=3140);
    let seeds: Vec<Option<u64>> = std::iter::once(None).chain(noises.map(Some)).collect();
    let jobs: Vec<(u32, [u32; 2], Option<u64>)> =
        [(18_000, [12_000, 36_000]), (28_800, [14_400, 21_600])]
            .into_iter()
            .flat_map(|(bph, wrongs)| seeds.iter().map(move |&seed| (bph, wrongs, seed)))
            .collect();
    on_every_core(&jobs, |&(bph, wrongs, seed)| {
        let watch = watch_at(bph, 60.0);
        let watch = seed.map_or(watch.clone(), |seed| noise_over(&watch, seed));
        let read = |r: u32| {
            let cfg = AnalysisConfig {
                bph: Some(r),
                ..Default::default()
            };
            analyze(&watch, &cfg).summary
        };
        let right = read(bph);
        for wrong in wrongs {
            let harmonic = read(wrong);
            let what = format!("{bph} read at {wrong}, noise {seed:?}");
            assert!(right.watch_heard, "{what}");
            assert!(reads_better(&right, &harmonic), "{what}");
            assert!(!reads_better(&harmonic, &right), "{what}");
        }
    });
}

/// `watch` with the synthetic stand noise drawn from `seed` over it.
fn noise_over(watch: &Audio, seed: u64) -> Audio {
    let rms = noise_rms(SynthConfig::default().snr_db) * SCALE;
    let mut rng = Rng::new(seed);
    let samples = watch
        .samples
        .iter()
        .map(|&v| v + (rms * rng.normal()) as f32)
        .collect();
    Audio {
        samples,
        sample_rate: watch.sample_rate,
    }
}

/// Cases near the window rule (#1432), seeds 3141 to 3149 run once: a
/// watch fading in over its first seconds under the stand's noise, the
/// same after a lead of noise, and a plain noise lead, where windows over
/// the step hold good beats in only some of their parts. Each reads the
/// watch at its rate within 0.3 s/d of the watch alone, and any lead
/// named ends within one rate window of where the watch is fully in.
#[test]
fn a_watch_fading_in_or_after_noise_reads_as_the_watch() {
    use timegrapher_core::measure::RATE_WINDOW_S;
    let cases = [
        (18_000, 0.0, 10.0, 3141),
        (28_800, 0.0, 20.0, 3142),
        (14_400, 0.0, 30.0, 3143),
        (18_000, 60.0, 10.0, 3144),
        (28_800, 60.0, 20.0, 3145),
        (21_600, 60.0, 30.0, 3146),
        (18_000, 60.0, 0.0, 3147),
        (19_800, 60.0, 0.0, 3148),
        (28_800, 60.0, 0.0, 3149),
    ];
    for (bph, noise_s, fade_s, seed) in cases {
        let what = format!("{bph} bph fading in over {fade_s} s after {noise_s} s ({seed})");
        let alone = watch_at(bph, 90.0);
        let want = analyze(&alone, &AnalysisConfig::default()).summary;
        let mut faded = alone.clone();
        let fs = f64::from(faded.sample_rate);
        for (i, v) in faded.samples.iter_mut().enumerate() {
            let t = i as f64 / fs;
            if t < fade_s {
                *v *= (t / fade_s) as f32;
            }
        }
        let quiet = Audio {
            samples: Vec::new(),
            sample_rate: faded.sample_rate,
        };
        let led = noise_before(noise_s, &quiet, seed);
        let mut samples = led.samples;
        samples.extend_from_slice(&noise_over(&faded, seed + 100).samples);
        let audio = Audio {
            samples,
            sample_rate: faded.sample_rate,
        };
        let got = analyze(&audio, &AnalysisConfig::default()).summary;
        assert_eq!(got.bph, bph, "{what}");
        assert!(got.watch_heard, "{what}: {:?}", got.caveats);
        let (r, w) = (got.rate_s_per_day.unwrap(), want.rate_s_per_day.unwrap());
        assert!((r - w).abs() <= 0.3, "{what}: rate {r} against {w}");
        if let Some(lead) = lead_left_out(&got.caveats) {
            assert!(
                lead <= noise_s + fade_s + RATE_WINDOW_S,
                "{what}: lead {lead}"
            );
        }
    }
}
