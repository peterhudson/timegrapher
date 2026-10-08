//! Beat shape on synthetic recordings with known sounds.

use timegrapher_core::beats;
use timegrapher_core::dsp::{envelope, EnvelopeConfig};
use timegrapher_core::shape::{self, ShapeConfig, ShapeReport};
use timegrapher_core::synth::{generate, ExtraSound, Sound, SynthConfig};

const AMP: f64 = 270.0;

fn run(cfg: &SynthConfig) -> ShapeReport {
    let audio = generate(cfg, |_| AMP, |_| 0.0);
    let fs = audio.sample_rate as f64;
    let env = envelope(&audio.samples, fs, &EnvelopeConfig::default());
    let (found, _) = beats::detect(&env, fs, cfg.bph);
    shape::analyze(
        &env,
        fs,
        &found,
        3600.0 / cfg.bph as f64,
        &ShapeConfig::default(),
    )
}

/// Unlock to drop in the synthetic signal, ms.
fn unlock_to_drop_ms(cfg: &SynthConfig) -> f64 {
    let osc = 7200.0 / cfg.bph as f64;
    osc / std::f64::consts::PI * (cfg.lift_deg / (2.0 * AMP)).asin() * 1000.0
}

fn sound(gain: f64, freq_hz: f64) -> Sound {
    Sound {
        gain,
        freq_hz,
        decay_s: 0.0003,
    }
}

fn close(name: &str, got: Option<f64>, want: f64, tol: f64) {
    let got = got.unwrap_or_else(|| panic!("{name}: not found"));
    assert!((got - want).abs() <= tol, "{name}: {got:.3} vs {want:.3}");
}

#[test]
fn recovers_sound_spacing_and_levels() {
    for (impulse_at, gains) in [(0.35, [0.4, 0.6]), (0.6, [0.25, 0.45])] {
        let cfg = SynthConfig {
            sounds: [
                sound(gains[0], 5200.0),
                sound(gains[1], 4400.0),
                sound(1.0, 6100.0),
            ],
            impulse_at,
            ..Default::default()
        };
        let tud = unlock_to_drop_ms(&cfg);
        let r = run(&cfg);
        for (side, s) in [("even", &r.even), ("odd", &r.odd)] {
            assert!(
                s.windows_measured >= 10,
                "{side}: {} windows",
                s.windows_measured
            );
            assert_eq!(
                s.windows_with_2, s.windows_measured,
                "{side}: sound 2 missed"
            );
            let whole = s.whole.as_ref().expect("whole-recording shape");
            for (what, m) in [("windows", &s.windows), ("whole", whole)] {
                let tag = format!("{side} {what} at {impulse_at}");
                close(&format!("{tag} 1-2"), m.i12_ms, impulse_at * tud, 0.1);
                close(
                    &format!("{tag} 2-3"),
                    m.i23_ms,
                    (1.0 - impulse_at) * tud,
                    0.1,
                );
                close(&format!("{tag} 1:3"), m.ratio13, gains[0], 0.03);
                close(&format!("{tag} 2:3"), m.ratio23, gains[1], 0.03);
                assert_eq!(m.rises, 3, "{tag}: rises");
                assert!(m.extra_pre.is_empty() && m.extra_post.is_empty(), "{tag}");
                close(&format!("{tag} noise"), m.noise_ratio, 1.0, 0.1);
                assert!(m.tail_ratio < 0.02, "{tag}: tail {}", m.tail_ratio);
            }
            let spread = s.spread.expect("single-beat spread");
            assert!(
                spread.i13_sd_us < 40.0,
                "{side}: spread {}",
                spread.i13_sd_us
            );
        }
    }
}

#[test]
fn finds_an_extra_sound_after_the_drop_on_one_side() {
    let cfg = SynthConfig {
        extra: Some(ExtraSound {
            offset_s: 0.004,
            even: true,
            sound: sound(0.3, 5000.0),
        }),
        ..Default::default()
    };
    let r = run(&cfg);
    let (with, without) = if r.even.windows_with_extra_post > r.odd.windows_with_extra_post {
        (&r.even, &r.odd)
    } else {
        (&r.odd, &r.even)
    };
    assert!(with.windows_with_extra_post * 10 >= with.windows_measured * 9);
    assert_eq!(without.windows_with_extra_post, 0);
    let w = with.whole.as_ref().expect("whole");
    assert_eq!(w.extra_post.len(), 1, "{:?}", w.extra_post);
    let e = w.extra_post[0];
    close("extra time", Some(e.t_ms - w.t3_ms), 4.0, 0.1);
    close("extra level", Some(e.level), 0.3, 0.06);
    let o = without.whole.as_ref().expect("whole");
    assert!(o.extra_post.is_empty() && o.extra_pre.is_empty());
    assert!(w.extra_pre.is_empty());
    // The extra sound raises the tail on its own side only.
    assert!(w.tail_ratio > 2.0 * o.tail_ratio.max(0.005));
}

#[test]
fn merged_unlock_and_impulse_are_not_separated() {
    // The impulse 0.2 ms after the unlock: one sound as far as the envelope can tell.
    let cfg = SynthConfig {
        sounds: [sound(0.4, 5200.0), sound(0.6, 4400.0), sound(1.0, 6100.0)],
        impulse_at: 0.2 / unlock_to_drop_ms(&SynthConfig::default()),
        ..Default::default()
    };
    let r = run(&cfg);
    for s in [&r.even, &r.odd] {
        let w = s.whole.as_ref().expect("whole");
        assert!(w.t1_ms.is_some());
        assert!(w.t2_ms.is_none(), "sound 2 at {:?}", w.t2_ms);
        assert_eq!(w.rises, 2);
    }
}
