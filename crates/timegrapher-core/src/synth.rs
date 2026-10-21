//! Synthetic watch signals for tests.
//!
//! Each beat is three decaying bursts: unlock, impulse and drop. The time
//! from unlock to drop follows the amplitude through the lift-angle
//! formula, so a test can check that amplitude comes back out.

use crate::audio::Audio;

/// Small deterministic PRNG (xorshift64*) so tests need no dependencies.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_f64(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn normal(&mut self) -> f64 {
        let u1 = self.next_f64().max(1e-300);
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

/// One of the sounds in a beat: a decaying tone burst.
#[derive(Debug, Clone, Copy)]
pub struct Sound {
    /// Peak level relative to the drop's nominal level of 1.
    pub gain: f64,
    pub freq_hz: f64,
    /// Exponential decay time constant, seconds.
    pub decay_s: f64,
}

/// An extra sound added to every other beat, as a fault would add one.
#[derive(Debug, Clone, Copy)]
pub struct ExtraSound {
    /// Seconds from the drop (negative is before it).
    pub offset_s: f64,
    /// Added to beats with an even (`true`) or odd (`false`) generator count.
    pub even: bool,
    pub sound: Sound,
}

/// Rubbing noise after the drop on one side, as a grazing balance or
/// hairspring makes.
#[derive(Debug, Clone, Copy)]
pub struct Rub {
    /// On beats with an even (`true`) or odd (`false`) generator count.
    pub even: bool,
    /// RMS level relative to the drop's nominal level of 1.
    pub gain: f64,
    /// From 1 ms after the drop, seconds.
    pub length_s: f64,
}

pub struct SynthConfig {
    pub sample_rate: u32,
    pub duration_s: f64,
    pub bph: u32,
    pub lift_deg: f64,
    pub rate_s_per_day: f64,
    pub beat_error_ms: f64,
    /// Change of the beat error per second of recording, ms, as when a
    /// hairspring rubs more and more.
    pub beat_error_drift_ms_per_s: f64,
    /// Peak signal over RMS noise, dB.
    pub snr_db: f64,
    pub seed: u64,
    /// Unlock, impulse and drop.
    pub sounds: [Sound; 3],
    /// Where the impulse sits from unlock (0) to drop (1).
    pub impulse_at: f64,
    /// Extra unlock-to-drop time on beats with an even generator count,
    /// seconds: their unlock comes this much earlier (the impulse in
    /// proportion), as when the two sides' lifts differ.
    pub even_unlock_lead_s: f64,
    /// Gains of the three sounds on beats with an odd generator count,
    /// when the two sides' sounds differ in level; `sounds` sets the even
    /// beats' gains, and the odd beats' when this is `None`.
    pub odd_gains: Option<[f64; 3]>,
    /// Where the impulse sits on beats with an odd generator count, when
    /// the two sides lock to different depths; `impulse_at` otherwise.
    pub odd_impulse_at: Option<f64>,
    /// Spread of each sound's level from beat to beat (SD, relative).
    pub gain_jitter: f64,
    pub extra: Option<ExtraSound>,
    pub rub: Option<Rub>,
}

impl Default for SynthConfig {
    fn default() -> Self {
        SynthConfig {
            sample_rate: 48000,
            duration_s: 30.0,
            bph: 28800,
            lift_deg: 52.0,
            rate_s_per_day: 5.0,
            beat_error_ms: 0.4,
            beat_error_drift_ms_per_s: 0.0,
            snr_db: 30.0,
            seed: 1,
            sounds: [
                Sound {
                    gain: 0.35,
                    freq_hz: 5200.0,
                    decay_s: 0.00025,
                },
                Sound {
                    gain: 0.25,
                    freq_hz: 3800.0,
                    decay_s: 0.00035,
                },
                Sound {
                    gain: 1.0,
                    freq_hz: 6100.0,
                    decay_s: 0.00045,
                },
            ],
            impulse_at: 0.45,
            even_unlock_lead_s: 0.0,
            odd_gains: None,
            odd_impulse_at: None,
            gain_jitter: 0.08,
            extra: None,
            rub: None,
        }
    }
}

/// Generate a recording. `amp_fn(t)` gives the amplitude in degrees and
/// `rate_fn(t)` an extra rate in s/d at time `t` (seconds).
pub fn generate(
    cfg: &SynthConfig,
    amp_fn: impl Fn(f64) -> f64,
    rate_fn: impl Fn(f64) -> f64,
) -> Audio {
    let fs = cfg.sample_rate as f64;
    let n = (cfg.duration_s * fs) as usize;
    let mut x = vec![0.0f64; n];
    let mut rng = Rng::new(cfg.seed);
    let nominal = 3600.0 / cfg.bph as f64;
    let mut t = 0.05;
    let mut k = 0u64;
    while t < cfg.duration_s - 0.05 {
        let beat_error = cfg.beat_error_ms + cfg.beat_error_drift_ms_per_s * t;
        render_beat(cfg, k, t, amp_fn(t), beat_error, &mut rng, |i, v| {
            if let Some(s) = usize::try_from(i).ok().and_then(|i| x.get_mut(i)) {
                *s += v;
            }
        });
        let rate = cfg.rate_s_per_day + rate_fn(t);
        t += nominal * (1.0 - rate / 86400.0);
        k += 1;
    }
    let noise_rms = 10f64.powf(-cfg.snr_db / 20.0);
    let scale = 0.5;
    let samples = x
        .iter()
        .map(|&v| ((v + noise_rms * rng.normal()) * scale) as f32)
        .collect();
    Audio {
        samples,
        sample_rate: cfg.sample_rate,
    }
}

/// The sounds of beat `k`, whose drop is nominally at `t`, at amplitude
/// `amp` (degrees) and beat error `beat_error` (ms): each sample added
/// as `add(sample index, value)`.
pub(crate) fn render_beat(
    cfg: &SynthConfig,
    k: u64,
    t: f64,
    amp: f64,
    beat_error: f64,
    rng: &mut Rng,
    mut add: impl FnMut(isize, f64),
) {
    let fs = cfg.sample_rate as f64;
    let nominal = 3600.0 / cfg.bph as f64;
    let blen = (0.004 * fs) as usize;
    let osc = 2.0 * nominal;
    let mut tud = osc / std::f64::consts::PI * (cfg.lift_deg / (2.0 * amp)).asin();
    let side = if k.is_multiple_of(2) { 0.5 } else { -0.5 };
    if k.is_multiple_of(2) {
        tud += cfg.even_unlock_lead_s;
    }
    // The drop is the reference point; the unlock comes tud earlier.
    let drop = t + side * beat_error / 1000.0;
    let [mut s1, mut s2, mut s3] = cfg.sounds;
    if let Some([g1, g2, g3]) = cfg.odd_gains.filter(|_| k % 2 == 1) {
        (s1.gain, s2.gain, s3.gain) = (g1, g2, g3);
    }
    let impulse_at = match cfg.odd_impulse_at {
        Some(a) if k % 2 == 1 => a,
        _ => cfg.impulse_at,
    };
    let mut events = vec![
        (drop - tud, s1),
        (drop - (1.0 - impulse_at) * tud, s2),
        (drop, s3),
    ];
    if let Some(e) = cfg.extra.filter(|e| e.even == k.is_multiple_of(2)) {
        events.push((drop + e.offset_s, e.sound));
    }
    for (te, snd) in events {
        let (gain, fc, tau) = (snd.gain, snd.freq_hz, snd.decay_s);
        let i0 = (te * fs).round() as isize;
        let ph = rng.next_f64() * 2.0 * std::f64::consts::PI;
        let g = gain * (1.0 + cfg.gain_jitter * rng.normal()).max(0.0);
        for j in 0..blen {
            let tt = j as f64 / fs;
            add(
                i0 + j as isize,
                g * (-tt / tau).exp()
                    * (1.0 - (-tt / 0.00005).exp())
                    * (2.0 * std::f64::consts::PI * fc * tt + ph).sin(),
            );
        }
    }
    if let Some(r) = cfg.rub.filter(|r| r.even == k.is_multiple_of(2)) {
        let i0 = ((drop + 0.001) * fs).round() as isize;
        for j in 0..(r.length_s * fs) as isize {
            add(i0 + j, r.gain * rng.normal());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FNV-1a over the 16-bit samples `write_wav` would write.
    fn wav_hash(audio: &Audio) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for &s in &audio.samples {
            let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            for b in v.to_le_bytes() {
                h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
    }

    #[test]
    fn the_default_output_is_unchanged() {
        // Pinned on main before render_beat(): plain `synth` must write
        // the same sound, with and without a rub.
        let cfg = SynthConfig {
            duration_s: 10.0,
            ..Default::default()
        };
        let plain = generate(&cfg, |_| 270.0, |_| 0.0);
        let rub = SynthConfig {
            rub: Some(Rub {
                even: true,
                length_s: 0.003,
                gain: 0.05,
            }),
            duration_s: 10.0,
            ..Default::default()
        };
        let rubbed = generate(&rub, |t| 270.0 - t, |t| t / 10.0);
        assert_eq!(
            (wav_hash(&plain), wav_hash(&rubbed)),
            (2769891825584145652, 18026172536521054176)
        );
    }
}
