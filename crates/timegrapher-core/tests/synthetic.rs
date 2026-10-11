//! End-to-end checks on synthetic recordings with known answers.

use timegrapher_core::synth::{generate, ExtraSound, Rub, Sound, SynthConfig};
use timegrapher_core::{analyze, AnalysisConfig};

fn run(
    cfg: &SynthConfig,
    amp: impl Fn(f64) -> f64,
    rate: impl Fn(f64) -> f64,
) -> timegrapher_core::Analysis {
    analyze(&generate(cfg, amp, rate), &AnalysisConfig::default())
}

#[test]
fn rate_beat_error_and_amplitude() {
    for (bph, amp) in [
        (28800, 280.0),
        (21600, 250.0),
        (18000, 220.0),
        (36000, 270.0),
    ] {
        let cfg = SynthConfig {
            bph,
            rate_s_per_day: -7.0,
            beat_error_ms: 0.6,
            duration_s: 30.0,
            ..Default::default()
        };
        let a = run(&cfg, |_| amp, |_| 0.0);
        let s = &a.summary;
        assert_eq!(s.bph, bph, "beat rate guessed wrongly");
        let f = s.overall.expect("fit");
        assert!(
            (f.rate_s_per_day + 7.0).abs() < 0.3,
            "{bph}: rate {}",
            f.rate_s_per_day
        );
        assert!(
            (f.beat_error_ms.abs() - 0.6).abs() < 0.05,
            "{bph}: beat error {}",
            f.beat_error_ms
        );
        let measured = s.amplitude_deg.expect("amplitude");
        assert!(
            (measured - amp).abs() < 8.0,
            "{bph}: amplitude {measured} vs {amp}"
        );
    }
}

#[test]
fn noisy_signal_still_tracks() {
    let cfg = SynthConfig {
        snr_db: 12.0,
        duration_s: 30.0,
        ..Default::default()
    };
    let a = run(&cfg, |_| 270.0, |_| 0.0);
    let f = a.summary.overall.expect("fit");
    assert!(
        (f.rate_s_per_day - 5.0).abs() < 0.5,
        "rate {}",
        f.rate_s_per_day
    );
    // 240 beats; at this noise level a few are drowned out and left out
    // rather than placed on a noise peak.
    assert!(a.beats.len() >= 230, "{} beats", a.beats.len());
}

#[test]
fn finds_a_once_a_minute_rate_swing() {
    // The watch loses 40 s/d for 10 s of every minute, like a bad fourth-wheel tooth.
    let cfg = SynthConfig {
        duration_s: 300.0,
        ..Default::default()
    };
    let swing = |t: f64| if (t % 60.0) < 10.0 { -40.0 } else { 0.0 };
    let a = run(&cfg, |_| 270.0, swing);
    let top = &a.summary.timing_periods[0];
    assert!(
        (top.period_s - 60.0).abs() < 4.0,
        "{:?}",
        a.summary.timing_periods
    );
    assert_eq!(top.wheel.as_deref(), Some("fourth wheel"));
}

#[test]
fn finds_an_amplitude_dip_every_minute() {
    let cfg = SynthConfig {
        duration_s: 300.0,
        ..Default::default()
    };
    let dip = |t: f64| {
        if (t % 60.0 - 30.0).abs() < 4.0 {
            245.0
        } else {
            275.0
        }
    };
    let a = run(&cfg, dip, |_| 0.0);
    let top = &a.summary.amplitude_periods[0];
    assert!(
        (top.period_s - 60.0).abs() < 4.0,
        "{:?}",
        a.summary.amplitude_periods
    );
}

#[test]
fn beat_error_from_the_unlock() {
    // Drops 0.4 ms apart, and the even side's unlock 0.6 ms further ahead
    // of its drop than the odd side's. Even beats' drops come 0.2 ms late
    // and their unlocks 0.4 ms early, so measured from the unlock the beat
    // error is 0.2 ms the other way. A correction with the wrong sign
    // would read 1.0 ms.
    let cfg = SynthConfig {
        beat_error_ms: 0.4,
        even_unlock_lead_s: 0.0006,
        duration_s: 30.0,
        ..Default::default()
    };
    let a = run(&cfg, |_| 270.0, |_| 0.0);
    let drop = a.summary.overall.expect("fit").beat_error_ms;
    let unlock = a.summary.beat_error_unlock_ms.expect("unlock beat error");
    assert!((drop.abs() - 0.4).abs() < 0.05, "drop {drop}");
    // Detection may number the beats from the other side, which flips both
    // signs together.
    let unlock = unlock * drop.signum();
    assert!((unlock + 0.2).abs() < 0.05, "unlock {unlock}, drop {drop}");
    let w = &a.amplitude_windows;
    let ok = w
        .iter()
        .filter_map(|w| w.beat_error_unlock_ms)
        .filter(|u| (u * drop.signum() + 0.2).abs() < 0.15)
        .count();
    assert!(ok * 10 >= w.len() * 9, "{ok} of {} windows", w.len());

    // With the sides alike, the two agree.
    let cfg = SynthConfig {
        beat_error_ms: 0.6,
        duration_s: 30.0,
        ..Default::default()
    };
    let s = run(&cfg, |_| 270.0, |_| 0.0).summary;
    let drop = s.overall.expect("fit").beat_error_ms;
    let unlock = s.beat_error_unlock_ms.expect("unlock beat error");
    assert!((unlock - drop).abs() < 0.05, "unlock {unlock}, drop {drop}");
}

#[test]
fn sides_that_differ_in_shape_keep_their_drop() {
    // As on a Patek 324 clone: on one side the impulse is louder than the
    // drop, on the other the unlock is nearly as loud as the drop, and the
    // levels wander from beat to beat, so the loudest sound changes. Beat
    // times on the drop of both sides give the set beat error, steady
    // timing and the same amplitude on each side. Beat times that wander
    // between sounds read milliseconds of beat error and lopsided
    // amplitudes.
    let cfg = SynthConfig {
        beat_error_ms: 0.4,
        duration_s: 60.0,
        sounds: [
            Sound {
                gain: 0.35,
                ..SynthConfig::default().sounds[0]
            },
            Sound {
                gain: 1.3,
                ..SynthConfig::default().sounds[1]
            },
            SynthConfig::default().sounds[2],
        ],
        odd_gains: Some([0.9, 0.45, 1.0]),
        gain_jitter: 0.25,
        ..Default::default()
    };
    let s = run(&cfg, |_| 245.0, |_| 0.0).summary;
    let f = s.overall.expect("fit");
    assert!((f.beat_error_ms.abs() - 0.4).abs() < 0.05, "drop {f:?}");
    let unlock = s.beat_error_unlock_ms.expect("unlock beat error");
    assert!(
        (unlock - f.beat_error_ms).abs() < 0.1,
        "unlock {unlock}, drop {}",
        f.beat_error_ms
    );
    let jitter = s.jitter_us.expect("jitter");
    assert!(jitter < 50.0, "jitter {jitter} us");
    for side in [s.amplitude_even_deg, s.amplitude_odd_deg] {
        let a = side.expect("amplitude");
        assert!((a - 245.0).abs() < 8.0, "{s:?}");
    }
}

#[test]
fn clipping_is_counted_per_beat() {
    let cfg = SynthConfig {
        duration_s: 20.0,
        ..Default::default()
    };
    let mut audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let clean = analyze(&audio, &AnalysisConfig::default()).summary;
    assert_eq!(clean.clipping.samples, 0);
    assert!(!clean.clipping.noted());

    // Drive it 12 dB too hot and clip at full scale, as a converter does.
    let peak = audio.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
    for v in audio.samples.iter_mut() {
        *v = (*v * 4.0 / peak).clamp(-1.0, 1.0);
    }
    let hot = analyze(&audio, &AnalysisConfig::default()).summary;
    let c = hot.clipping;
    assert!(c.noted(), "{c:?}");
    assert!(c.beat_fraction > 0.5, "{c:?}");
    assert!(c.peak_dbfs > -0.1, "{c:?}");
}

#[test]
fn an_empty_stand_is_no_watch() {
    // Hiss with no ticks in it: the tracker still finds "beats", but they
    // scatter across the beat period.
    let fs = 48000u32;
    let mut state = 12345u64;
    let mut uniform = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 11) as f64 / (1u64 << 53) as f64) as f32 - 0.5
    };
    let samples: Vec<f32> = (0..fs as usize * 30).map(|_| 0.01 * uniform()).collect();
    let quiet = timegrapher_core::audio::Audio {
        samples,
        sample_rate: fs,
    };
    let s = analyze(&quiet, &AnalysisConfig::default()).summary;
    assert!(
        !s.watch_heard,
        "noise taken for a watch: jitter {:?} us",
        s.jitter_us
    );

    let cfg = SynthConfig {
        snr_db: 12.0,
        duration_s: 30.0,
        ..Default::default()
    };
    assert!(s.confidence.is_none(), "a ± for readings of noise");
    assert!(
        s.overall.is_none() && s.amplitude_deg.is_none() && s.beat_error_unlock_ms.is_none(),
        "readings of noise kept"
    );
    assert!(run(&cfg, |_| 270.0, |_| 0.0).summary.watch_heard);
    let clean = SynthConfig {
        duration_s: 60.0,
        ..Default::default()
    };
    let watch = run(&clean, |_| 270.0, |_| 0.0).summary;
    // A clean synthetic watch: each reading has its ±, with no USB lines
    // to correct the clock and no lift angle given, both named.
    let c = watch.confidence.expect("a ± for each reading");
    assert!(watch.doubts.clock_unknown && watch.doubts.lift_unknown);
    assert!(watch.usb_clock.is_none());
    assert!(c.beat_error.se.is_some_and(|e| e < 0.05), "{c:?}");
}

/// The shape measures move as each synthetic escapement change is made,
/// on the side it is made on: the program reports them as figures and
/// names nothing. Which side the detector calls the Tick depends on which
/// beat it hears first, so one-sided changes are checked as one-sided.
#[test]
fn shape_measures_follow_the_sound() {
    use timegrapher_core::shape::{Measures, SideMeasures};
    let base = || SynthConfig {
        duration_s: 40.0,
        ..Default::default()
    };
    let measures = |cfg: &SynthConfig| -> Measures {
        run(cfg, |_| 270.0, |_| 0.0)
            .summary
            .shape
            .expect("measured")
    };
    let v = |x: Option<f64>| x.unwrap_or(f64::NAN);
    // The larger and smaller of a measure over the two sides.
    let both = |m: &Measures, f: &dyn Fn(&SideMeasures) -> f64| {
        let (a, b) = (f(&m.even), f(&m.odd));
        (a.max(b), a.min(b))
    };
    let clean = measures(&base());
    println!("clean {clean:?}");
    assert!(both(&clean, &|s| v(s.share_with_sound_1)).1 > 0.9);
    assert!(both(&clean, &|s| v(s.share_with_sound_2)).1 > 0.9);
    assert!(both(&clean, &|s| v(s.ratio13)).0 < 1.0);
    assert!(clean.even.windows_measured >= 5 && clean.odd.windows_measured >= 5);

    let knock = Sound {
        gain: 0.15,
        freq_hz: 4500.0,
        decay_s: 0.0003,
    };
    type Check = Box<dyn Fn(&Measures) -> bool>;
    let cases: Vec<(&str, SynthConfig, Check)> = vec![
        (
            "a loud unlock on one side: sound 1 louder than sound 3 there only",
            SynthConfig {
                odd_gains: Some([1.6, 0.25, 1.0]),
                ..base()
            },
            Box::new(move |m| {
                let (hi, lo) = both(m, &|s| v(s.ratio13));
                hi >= 1.0 && lo < 1.0
            }),
        ),
        (
            "no unlock on one side: sound 1 or 2 rarely found there",
            SynthConfig {
                odd_gains: Some([0.0, 0.25, 1.0]),
                ..base()
            },
            Box::new(move |m| {
                let one = both(m, &|s| v(s.share_with_sound_1));
                let two = both(m, &|s| v(s.share_with_sound_2));
                (one.1 < 0.3 || two.1 < 0.5) && one.0 > 0.9 && two.0 > 0.9
            }),
        ),
        (
            "unlock and impulse merged on both sides: sound 2 rarely apart",
            SynthConfig {
                impulse_at: 0.03,
                ..base()
            },
            Box::new(move |m| both(m, &|s| v(s.share_with_sound_2)).0 < 0.5),
        ),
        (
            "a shallow lock on one side: 1-to-2 far longer on one side",
            SynthConfig {
                odd_impulse_at: Some(0.18),
                ..base()
            },
            Box::new(move |m| {
                let (hi, lo) = both(m, &|s| v(s.i12_ms));
                hi / lo >= 1.8
            }),
        ),
        (
            "a knock before one side's unlock: 1-to-2 far longer on one side",
            SynthConfig {
                extra: Some(ExtraSound {
                    offset_s: -0.016,
                    even: true,
                    sound: knock,
                }),
                ..base()
            },
            Box::new(move |m| {
                let (hi, lo) = both(m, &|s| v(s.i12_ms));
                hi / lo >= 1.8
            }),
        ),
        (
            "rubbing after one side's drop: a longer tail on that side",
            SynthConfig {
                rub: Some(Rub {
                    even: false,
                    gain: 0.07,
                    length_s: 0.014,
                }),
                ..base()
            },
            Box::new(move |m| {
                let (hi, lo) = both(m, &|s| v(s.tail_ratio));
                hi / lo >= 3.0 && hi >= 0.1
            }),
        ),
        (
            "beat error drifting by 2 ms over two minutes",
            SynthConfig {
                duration_s: 120.0,
                beat_error_drift_ms_per_s: 0.017,
                ..base()
            },
            Box::new(move |m| {
                let d = v(m.beat_error_last_quarter_ms) - v(m.beat_error_first_quarter_ms);
                d.abs() >= 1.0
            }),
        ),
        (
            "beat error drifting by 0.4 ms",
            SynthConfig {
                duration_s: 120.0,
                beat_error_drift_ms_per_s: 0.0035,
                ..base()
            },
            Box::new(move |m| {
                let d = v(m.beat_error_last_quarter_ms) - v(m.beat_error_first_quarter_ms);
                (0.2..1.0).contains(&d.abs())
            }),
        ),
    ];
    let wrong: Vec<String> = cases
        .into_iter()
        .filter_map(|(name, cfg, ok)| {
            let m = measures(&cfg);
            (!ok(&m)).then(|| format!("{name}: {m:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A beat rate set wrong is named, with the rate heard.
#[test]
fn a_wrong_set_beat_rate_is_flagged() {
    let audio = generate(&SynthConfig::default(), |_| 270.0, |_| 0.0);
    let wrong = AnalysisConfig {
        bph: Some(18000),
        ..Default::default()
    };
    let s = analyze(&audio, &wrong).summary;
    assert_eq!(s.bph_heard, Some(28800));
    assert!(s
        .caveats
        .contains(&timegrapher_core::measure::Caveat::SetRateDisagrees { heard: 28800 }));
    let right = AnalysisConfig {
        bph: Some(28800),
        ..Default::default()
    };
    assert_eq!(analyze(&audio, &right).summary.bph_heard, None);
}
