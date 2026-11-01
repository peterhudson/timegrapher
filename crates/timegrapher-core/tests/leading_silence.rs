//! A recording that starts before the watch is placed (#476): silence,
//! or a low noise, before the watch reads as the watch alone does, with
//! the rate found or set.

use timegrapher_core::audio::Audio;
use timegrapher_core::beats::Beat;
use timegrapher_core::stream::{analyze_samples, BeatLog, StreamConfig};
use timegrapher_core::synth::{generate, SynthConfig};
use timegrapher_core::timing::fit;
use timegrapher_core::{analyze, AnalysisConfig};

/// 70 s of a watch at `bph`, +5 s/d and 270°, with its first `ramp_s`
/// rising from silence in level.
fn watch(bph: u32, ramp_s: f64) -> Audio {
    let mut a = generate(
        &SynthConfig {
            duration_s: 70.0,
            bph,
            rate_s_per_day: 5.0,
            beat_error_ms: 0.4,
            ..Default::default()
        },
        |_| 270.0,
        |_| 0.0,
    );
    let fs = a.sample_rate as f64;
    for (i, v) in a.samples.iter_mut().enumerate() {
        let t = i as f64 / fs;
        if t < ramp_s {
            *v *= (t / ramp_s) as f32;
        }
    }
    a
}

/// `lead_s` of a noise at `noise` RMS (0: digital silence), then `a`.
fn after_lead(a: &Audio, lead_s: f64, noise: f32) -> Audio {
    let n = (lead_s * a.sample_rate as f64) as usize;
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut uniform = || {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5
    };
    // A sum of four uniforms, near a normal; its RMS is 1/sqrt(3).
    let mut samples: Vec<f32> = (0..n)
        .map(|_| noise * 3f32.sqrt() * (0..4).map(|_| uniform()).sum::<f32>())
        .collect();
    samples.extend_from_slice(&a.samples);
    Audio {
        samples,
        sample_rate: a.sample_rate,
    }
}

/// The watch's own noise floor, RMS: the median sample size over 0.6745.
fn floor(a: &Audio) -> f32 {
    let mut v: Vec<f32> = a.samples.iter().map(|s| s.abs()).collect();
    v.sort_by(f32::total_cmp);
    v[v.len() / 2] / 0.6745
}

/// The recording with a lead reads as the watch alone: the same rate
/// found, and the same readings within a little of the beats' own scatter.
fn reads_as_alone(alone: &Audio, led: &Audio, bph: Option<u32>, what: &str) {
    let cfg = AnalysisConfig {
        bph,
        ..Default::default()
    };
    let want = analyze(alone, &cfg).summary;
    let got = analyze(led, &cfg).summary;
    assert!(want.watch_heard, "{what}: the watch alone is heard");
    assert!(got.watch_heard, "{what}: {:?}", got.caveats);
    assert_eq!(got.bph, want.bph, "{what}");
    assert_eq!(got.bph_heard, None, "{what}: {:?}", got.caveats);
    let near = |a: Option<f64>, b: Option<f64>, tol: f64, name: &str| {
        let (a, b) = (a.unwrap(), b.unwrap());
        assert!((a - b).abs() <= tol, "{what}: {name} {a} against {b}");
    };
    near(got.rate_s_per_day, want.rate_s_per_day, 0.05, "rate");
    near(got.amplitude_deg, want.amplitude_deg, 0.5, "amplitude");
    near(
        got.overall.as_ref().map(|f| f.beat_error_ms),
        want.overall.as_ref().map(|f| f.beat_error_ms),
        0.02,
        "beat error",
    );
}

#[test]
fn digital_silence_before_the_watch_is_passed_over() {
    for bph in [21_600, 28_800] {
        let alone = watch(bph, 0.0);
        for lead_s in [5.0, 49.0, 60.0] {
            let led = after_lead(&alone, lead_s, 0.0);
            for set in [None, Some(bph)] {
                reads_as_alone(
                    &alone,
                    &led,
                    set,
                    &format!("{bph} bph, {lead_s} s, set {set:?}"),
                );
            }
        }
    }
}

/// The watch's stretch of a recording with a noise before it reads as
/// the watch alone over the same stretch: the beats found there give the
/// same rate and beat error, and the amplitude windows the same amplitude.
/// The noise's own beats stay in the whole recording's reading, and where
/// the watch is first heard in a later chunk its first beat is a jump, so
/// no beat count runs across.
fn stretch_reads_as_alone(alone: &Audio, led: &Audio, lead_s: f64, bph: Option<u32>, what: &str) {
    let cfg = AnalysisConfig {
        bph,
        ..Default::default()
    };
    let rate = analyze(alone, &cfg).summary.bph;
    assert_eq!(analyze(led, &cfg).summary.bph, rate, "{what}");
    let stream = StreamConfig {
        analysis: AnalysisConfig {
            bph: Some(rate),
            ..cfg
        },
        ..Default::default()
    };
    let want = analyze_samples(&alone.samples, alone.sample_rate, &stream);
    let got = analyze_samples(&led.samples, led.sample_rate, &stream);
    // From 2 s into the watch, past the jump, to its end.
    let (from, to) = (2.0, alone.duration() - 1.0);
    let beats = |log: &BeatLog, shift: f64| -> Vec<Beat> {
        log.beats
            .iter()
            .filter(|b| b.time - shift >= from && b.time - shift < to)
            .map(|b| Beat {
                time: b.time - shift,
                ..*b
            })
            .collect()
    };
    let amplitude = |log: &BeatLog, shift: f64| -> f64 {
        let v: Vec<f64> = log
            .amplitude_windows
            .iter()
            .filter(|w| w.start_s - shift >= from && w.end_s - shift < to)
            .filter_map(|w| w.mean())
            .collect();
        v.iter().sum::<f64>() / v.len() as f64
    };
    let w = fit(&beats(&want, 0.0), rate).unwrap();
    let g = fit(&beats(&got, lead_s), rate).unwrap();
    assert!(
        (w.rate_s_per_day - g.rate_s_per_day).abs() <= 0.05,
        "{what}: rate {} against {}",
        g.rate_s_per_day,
        w.rate_s_per_day
    );
    assert!(
        (w.beat_error_ms - g.beat_error_ms).abs() <= 0.02,
        "{what}: beat error {} against {}",
        g.beat_error_ms,
        w.beat_error_ms
    );
    let (wa, ga) = (amplitude(&want, 0.0), amplitude(&got, lead_s));
    assert!(
        (wa - ga).abs() <= 0.5,
        "{what}: amplitude {ga} against {wa}"
    );
}

#[test]
fn a_low_noise_before_the_watch_leaves_the_watch_read_as_alone() {
    let alone = watch(21_600, 0.0);
    let noise = 0.5 * floor(&alone);
    for lead_s in [5.0, 49.0] {
        let led = after_lead(&alone, lead_s, noise);
        for set in [None, Some(21_600)] {
            stretch_reads_as_alone(
                &alone,
                &led,
                lead_s,
                set,
                &format!("noise, {lead_s} s, set {set:?}"),
            );
        }
    }
}

#[test]
fn a_watch_rising_in_level_after_silence_reads_as_alone() {
    let alone = watch(21_600, 20.0);
    for lead_s in [5.0, 49.0] {
        let led = after_lead(&alone, lead_s, 0.0);
        reads_as_alone(&alone, &led, None, &format!("ramp, {lead_s} s"));
    }
}
