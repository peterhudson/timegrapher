//! The microphone checks on synthetic recordings with known faults.

use timegrapher_core::audio::Audio;
use timegrapher_core::diagnose::{check, DiagnoseConfig, IssueCode};
use timegrapher_core::synth::{generate, SynthConfig};

fn watch(snr_db: f64) -> Audio {
    let cfg = SynthConfig {
        duration_s: 6.0,
        snr_db,
        ..Default::default()
    };
    generate(&cfg, |_| 270.0, |_| 0.0)
}

fn scaled(mut a: Audio, gain: f32) -> Audio {
    for v in a.samples.iter_mut() {
        *v = (*v * gain).clamp(-1.0, 1.0);
    }
    a
}

fn codes(a: &Audio) -> Vec<IssueCode> {
    let c = check(a, &DiagnoseConfig::default());
    println!("{c:#?}");
    c.issues.iter().map(|i| i.code).collect()
}

#[test]
fn clean_signal_is_ok() {
    let a = watch(40.0);
    let c = check(&a, &DiagnoseConfig::default());
    assert!(c.ok(), "{:?}", c.issues);
    assert!(c.issues.is_empty(), "{:?}", c.issues);
    assert_eq!(c.bph, 28800);
    assert!(c.beats_found as f64 > 0.9 * c.beats_expected as f64);
    assert!(c.tick_to_noise_db.unwrap() > 25.0);
    assert!(c.gap_rise_db.unwrap().abs() < 1.5);
    assert!((c.rate_s_per_day.unwrap() - 5.0).abs() < 1.0);
}

#[test]
fn clipping_is_found_and_turned_down() {
    let a = scaled(watch(40.0), 6.0);
    let c = check(&a, &DiagnoseConfig::default());
    assert!(c.has(IssueCode::Clipping), "{:?}", c.issues);
    assert!(!c.ok());
    assert!(c.suggested_gain_change_db.unwrap() < 0.0);
}

#[test]
fn analogue_clipping_below_full_scale_is_found() {
    // Clipped at 0.7 before the converter: flat tops below full scale.
    let mut a = scaled(watch(40.0), 6.0);
    for v in a.samples.iter_mut() {
        *v = v.clamp(-0.7, 0.7);
    }
    assert!(codes(&a).contains(&IssueCode::Clipping));
}

#[test]
fn peaks_within_a_few_db_of_full_scale_are_hot() {
    // The live run on the C-Media at full gain: peak about -3 dBFS.
    let a = watch(40.0);
    let peak = a.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
    let a = scaled(a, 0.7 / peak);
    let c = check(&a, &DiagnoseConfig::default());
    assert!(c.has(IssueCode::Hot), "{:?}", c.issues);
    assert!(!c.has(IssueCode::Clipping));
    assert!(c.ok(), "a warning, not a fault");
    let change = c.suggested_gain_change_db.unwrap();
    assert!((-8.0..=-6.0).contains(&change), "{change}");
}

#[test]
fn quiet_signal_asks_for_more_gain() {
    let a = scaled(watch(40.0), 0.01);
    let c = check(&a, &DiagnoseConfig::default());
    assert!(c.has(IssueCode::TooQuiet), "{:?}", c.issues);
    assert!(c.suggested_gain_change_db.unwrap() > 20.0);
}

#[test]
fn silence_and_noise_only() {
    let silent = Audio {
        samples: vec![0.0; 48000 * 3],
        sample_rate: 48000,
    };
    assert!(codes(&silent).contains(&IssueCode::Silent));
    let mut rng = timegrapher_core::synth::Rng::new(3);
    let noise = Audio {
        samples: (0..48000 * 4)
            .map(|_| (0.05 * rng.normal()) as f32)
            .collect(),
        sample_rate: 48000,
    };
    assert!(codes(&noise).contains(&IssueCode::NoTicks));
}

#[test]
fn noisy_signal_is_flagged() {
    assert!(codes(&watch(12.0)).contains(&IssueCode::Noisy));
}

#[test]
fn automatic_gain_is_suspected() {
    // Automatic gain: after each tick the gain drops and recovers
    // through the gap, so the background swells before the next tick.
    let mut a = watch(30.0);
    let fs = a.sample_rate as f64;
    let beat = 0.125;
    for (i, v) in a.samples.iter_mut().enumerate() {
        let t = i as f64 / fs;
        let phase = ((t - 0.05) / beat).rem_euclid(1.0);
        let g = 0.2 + 0.8 * phase;
        *v *= g as f32;
    }
    assert!(codes(&a).contains(&IssueCode::AgcSuspected));
}
