//! The sound card's clock error, read from the recording itself.
//!
//! A USB microphone hears its own bus: the host sends a start-of-frame
//! packet every millisecond, and the card's supply carries it into the
//! audio as faint lines at exact multiples of 1 kHz. Those lines run on the
//! host's clock, so where they land on the card's clock gives the card's
//! error with no system-clock log at all. On the stored takes the lines
//! from 3 to 10 kHz agree with one another within 0.1 ppm and with the
//! NTP clock logs within about 1 ppm (0.1 s/d), the host's own crystal
//! error. A card with no such lines (a non-USB input, a clean supply) gives
//! no answer.

use realfft::num_complex::Complex;
use serde::Serialize;

/// The card's error from the USB lines.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct UsbClock {
    /// Positive: the card's clock runs slow, as in [`crate::clockstore`].
    pub ppm: f64,
    /// Lines that stood clear of the noise and agreed.
    pub lines: usize,
    /// Largest difference between those lines, ppm.
    pub spread_ppm: f64,
}

impl UsbClock {
    /// The correction and source text used in analysis documents.
    pub fn correction(&self) -> (f64, String) {
        (
            self.ppm,
            format!(
                "from {} USB lines in the recording, {:.1} ppm apart",
                self.lines, self.spread_ppm
            ),
        )
    }
}

/// Lines searched: harmonics of 1 kHz in this range, Hz.
const LOWEST_HZ: u32 = 3000;
const HIGHEST_HZ: u32 = 10000;
/// Search this far either side of each harmonic, ppm.
const SEARCH_PPM: f64 = 200.0;
/// A line counts when its peak is this far above the median of its search
/// range, dB.
const MIN_SNR_DB: f64 = 20.0;
/// Lines further than this from their median are dropped, ppm.
const AGREE_PPM: f64 = 1.0;
/// Shortest recording worth measuring, seconds.
pub const MIN_DURATION_S: f64 = 30.0;
/// Longest stretch measured, seconds: ten minutes already gives 0.1 ppm,
/// and the search cost grows with the square of the length.
pub const MAX_DURATION_S: f64 = 600.0;
/// Rate the baseband is decimated to, Hz: wide enough for the search range
/// at 10 kHz (±2 Hz).
const BASEBAND_HZ: f64 = 8.0;

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Frequency offset of the line nearest `f0`, ppm, and its height above
/// the noise, dB.
fn line(x: &[f32], fs: u32, f0: u32) -> Option<(f64, f64)> {
    // Mix down to baseband with an exact table (f0 and fs are whole Hz,
    // so the phase repeats every fs / gcd samples), summing blocks.
    let period = (fs / gcd(fs, f0)) as usize;
    let table: Vec<Complex<f64>> = (0..period)
        .map(|i| {
            let a = -2.0 * std::f64::consts::PI * f0 as f64 * i as f64 / fs as f64;
            Complex::new(a.cos(), a.sin())
        })
        .collect();
    let block = (fs as f64 / BASEBAND_HZ).round() as usize;
    let n = x.len() / block;
    if n < 8 {
        return None;
    }
    let mut z: Vec<Complex<f64>> = Vec::with_capacity(n);
    let mut k = 0usize;
    for b in 0..n {
        let mut acc = Complex::new(0.0, 0.0);
        for &v in &x[b * block..(b + 1) * block] {
            acc += table[k] * v as f64;
            k += 1;
            if k == period {
                k = 0;
            }
        }
        // Hann window over the whole recording.
        let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * (b as f64 + 0.5) / n as f64).cos();
        z.push(acc * w);
    }
    let dt = block as f64 / fs as f64;
    let t_total = n as f64 * dt;
    let power = |df: f64| -> f64 {
        let step = Complex::from_polar(1.0, -2.0 * std::f64::consts::PI * df * dt);
        let mut rot = Complex::from_polar(1.0, -std::f64::consts::PI * df * dt);
        let mut s = Complex::new(0.0, 0.0);
        for &v in &z {
            s += v * rot;
            rot *= step;
        }
        s.norm_sqr()
    };
    // Coarse grid at a quarter of the resolution, then a parabola on the
    // log power through the peak and its neighbours.
    let span = f0 as f64 * SEARCH_PPM * 1e-6;
    let step = 0.25 / t_total;
    let m = (span / step).ceil() as i64;
    let grid: Vec<f64> = (-m..=m).map(|i| i as f64 * step).collect();
    let p: Vec<f64> = grid.iter().map(|&d| power(d)).collect();
    let (i, &peak) = p.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
    let mut sorted = p.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let floor = sorted[sorted.len() / 2];
    if floor <= 0.0 || i == 0 || i + 1 == p.len() {
        return None;
    }
    let snr_db = 10.0 * (peak / floor).log10();
    let (a, b, c) = (p[i - 1].ln(), peak.ln(), p[i + 1].ln());
    let den = a - 2.0 * b + c;
    let frac = if den < 0.0 { 0.5 * (a - c) / den } else { 0.0 };
    let df = grid[i] + frac * step;
    Some((df / f0 as f64 * 1e6, snr_db))
}

/// The card's clock error from the USB lines in `x`, or `None` when fewer
/// than two lines stand clear and agree.
pub fn measure(x: &[f32], fs: u32) -> Option<UsbClock> {
    if (x.len() as f64) < MIN_DURATION_S * fs as f64 {
        return None;
    }
    let x = &x[..x.len().min((MAX_DURATION_S * fs as f64) as usize)];
    let top = HIGHEST_HZ.min(fs / 2 - 1000);
    let mut ppm: Vec<f64> = (LOWEST_HZ..=top)
        .step_by(1000)
        .filter_map(|f0| line(x, fs, f0))
        .filter(|&(_, snr)| snr >= MIN_SNR_DB)
        .map(|(p, _)| p)
        .collect();
    if ppm.len() < 2 {
        return None;
    }
    let mid = crate::dsp::median(&mut ppm.clone());
    ppm.retain(|p| (p - mid).abs() <= AGREE_PPM);
    if ppm.len() < 2 {
        return None;
    }
    let lo = ppm.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = ppm.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Some(UsbClock {
        ppm: crate::dsp::median(&mut ppm.clone()),
        lines: ppm.len(),
        spread_ppm: hi - lo,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, seed: u64) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((s >> 11) as f64 / (1u64 << 53) as f64 - 0.5) as f32
            })
            .collect()
    }

    #[test]
    fn finds_a_slow_card_from_its_lines() {
        // A card 20 ppm slow counts fewer samples per true second, so a
        // true 5, 6 and 7 kHz line lands 20 ppm high on its clock.
        let fs = 48000u32;
        let n = fs as usize * 60;
        let mut x = noise(n, 7);
        for v in x.iter_mut() {
            *v *= 0.02;
        }
        for (f, a) in [(5000.0, 0.002), (6000.0, 0.003), (7000.0, 0.002)] {
            let f = f * (1.0 + 20e-6);
            for (i, v) in x.iter_mut().enumerate() {
                *v += (a * (2.0 * std::f64::consts::PI * f * i as f64 / fs as f64).sin()) as f32;
            }
        }
        let c = measure(&x, fs).expect("lines found");
        assert!((c.ppm - 20.0).abs() < 0.2, "ppm {}", c.ppm);
        assert!(c.lines >= 3, "lines {}", c.lines);
    }

    #[test]
    fn no_lines_no_answer() {
        let fs = 48000u32;
        assert_eq!(measure(&noise(fs as usize * 40, 3), fs), None);
        assert_eq!(measure(&noise(fs as usize * 5, 3), fs), None);
    }
}
