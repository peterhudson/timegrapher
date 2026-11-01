//! The microphone checks on synthetic recordings with known conditions.

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
fn a_clean_signal_has_no_conditions() {
    let a = watch(40.0);
    let c = check(&a, &DiagnoseConfig::default());
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

/// The signal above noise is reported as a figure, and only under 3×
/// (pure noise reads about 2×) is no regular beat named.
#[test]
fn signal_above_noise_is_a_figure() {
    let cfg = DiagnoseConfig::default();
    // The same figures the bands were drawn at: 3 to 5× at 16 dB, 5 to
    // 10× at 21 dB, 10× and above at 40 dB.
    for (snr, lo, hi) in [
        (16.0, 3.0, 5.0),
        (21.0, 5.0, 10.0),
        (40.0, 10.0, f64::INFINITY),
    ] {
        let c = check(&watch(snr), &cfg);
        let x = c.signal_x.unwrap();
        assert!((lo..hi).contains(&x), "{snr} dB: {x}");
        assert!(!c.has(IssueCode::NoTicks), "{snr}: {:?}", c.issues);
    }
    let none = check(&watch(11.0), &cfg);
    assert!(none.has(IssueCode::NoTicks), "{:?}", none.signal_x);
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

/// The check counts and times the beats every reading does, by core's one
/// good-beat rule ([`timegrapher_core::beats::Beat::is_good`]), not a cut
/// of its own (#506).
#[test]
fn the_check_keeps_the_beats_every_reading_keeps() {
    use timegrapher_core::{beats, dsp};
    // Noisy enough that some beats sit between the 0.4 cut and a 0.5 one.
    let a = generate(
        &SynthConfig {
            duration_s: 30.0,
            snr_db: 3.0,
            ..Default::default()
        },
        |_| 270.0,
        |_| 0.0,
    );
    let cfg = DiagnoseConfig::default();
    let c = check(&a, &cfg);
    let fs = a.sample_rate as f64;
    let env = dsp::envelope(&a.samples, fs, &cfg.envelope);
    let (found, _) = beats::detect(&env, fs, c.bph);
    let between = found
        .iter()
        .filter(|b| b.is_good() && b.quality < 0.5)
        .count();
    assert!(
        between > 0,
        "no beat between the two cuts to tell them apart"
    );
    assert_eq!(c.beats_found, found.iter().filter(|b| b.is_good()).count());
}

/// A watch whose beats are found, half or more of them, but whose ticks
/// barely stand over the noise is named `no_ticks` for its faint signal,
/// the test that fired, and the title says so, not its timing scatter
/// (#772).
#[test]
fn a_faint_watch_is_named_for_its_signal_not_its_timing() {
    let a = generate(
        &SynthConfig {
            duration_s: 30.0,
            snr_db: 0.0,
            ..Default::default()
        },
        |_| 270.0,
        |_| 0.0,
    );
    let c = check(&a, &DiagnoseConfig::default());
    let x = c.signal_x.expect("a signal figure");
    assert!(x < 3.0, "signal {x}");
    assert!(c.beats_found * 2 >= c.beats_expected, "{c:?}");
    let i = c
        .issues
        .iter()
        .find(|i| i.code == IssueCode::NoTicks)
        .expect("no_ticks");
    assert_eq!(
        i.title,
        format!("Ticks too faint over the noise (signal {x:.1}×, under 3×)")
    );
    assert!(
        i.evidence.ends_with(&format!("signal {x:.1}×")),
        "{}",
        i.evidence
    );
}

/// A watch timed over the start of the take and then nearly lost, so under
/// half the beats expected are found though they time, is named for its
/// few beats, whether its signal stands over the noise or is faint too:
/// the beat count is the test that fired first (#772).
#[test]
fn a_watch_with_few_beats_is_named_for_them_faint_or_not() {
    for (keep_s, faint) in [(10.0, false), (4.0, true)] {
        let mut a = generate(
            &SynthConfig {
                duration_s: 30.0,
                snr_db: 0.0,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        );
        let k = (keep_s * f64::from(a.sample_rate)) as usize;
        for v in a.samples[k..].iter_mut() {
            *v *= 0.02;
        }
        let c = check(&a, &DiagnoseConfig::default());
        assert!(c.beats_found > 0, "{keep_s} s: {c:?}");
        assert!(c.beats_found * 2 < c.beats_expected, "{keep_s} s: {c:?}");
        let x = c.signal_x.expect("a signal figure");
        assert_eq!(x < 3.0, faint, "{keep_s} s: signal {x}");
        let i = c
            .issues
            .iter()
            .find(|i| i.code == IssueCode::NoTicks)
            .expect("no_ticks");
        assert_eq!(
            i.title,
            format!(
                "No regular beat found (only {} of about {} beats)",
                c.beats_found, c.beats_expected
            ),
            "{keep_s} s"
        );
    }
}

/// A watch heard for 12 s of a 30 s take, the rest exact zeros: the
/// envelope's median is zero, so the signal has no figure, not one over a
/// vanishing floor (it read 3e305, #784), and the evidence says so.
#[test]
fn digital_silence_gives_no_signal_figure() {
    let mut a = generate(
        &SynthConfig {
            duration_s: 30.0,
            snr_db: 30.0,
            ..Default::default()
        },
        |_| 270.0,
        |_| 0.0,
    );
    let k = (12.0 * f64::from(a.sample_rate)) as usize;
    for v in a.samples[k..].iter_mut() {
        *v = 0.0;
    }
    let c = check(&a, &DiagnoseConfig::default());
    assert_eq!(c.signal_x, None, "{c:?}");
    let i = c
        .issues
        .iter()
        .find(|i| i.code == IssueCode::NoTicks)
        .expect("no_ticks");
    assert!(i.evidence.ends_with("beats found, -"), "{}", i.evidence);
}
