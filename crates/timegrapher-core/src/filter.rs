//! Biquad filters (RBJ cookbook) applied forward and backward for zero phase.
//!
//! Offline analysis can afford zero-phase filtering, which keeps the
//! timing of each tick's sub-events where it really is.

use std::f64::consts::PI;

#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    fn normalized(b: [f64; 3], a: [f64; 3]) -> Self {
        Biquad {
            b0: b[0] / a[0],
            b1: b[1] / a[0],
            b2: b[2] / a[0],
            a1: a[1] / a[0],
            a2: a[2] / a[0],
        }
    }

    pub fn highpass(fs: f64, f0: f64, q: f64) -> Self {
        let w0 = 2.0 * PI * f0 / fs;
        let (s, c) = w0.sin_cos();
        let alpha = s / (2.0 * q);
        Self::normalized(
            [(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0],
            [1.0 + alpha, -2.0 * c, 1.0 - alpha],
        )
    }

    pub fn lowpass(fs: f64, f0: f64, q: f64) -> Self {
        let w0 = 2.0 * PI * f0 / fs;
        let (s, c) = w0.sin_cos();
        let alpha = s / (2.0 * q);
        Self::normalized(
            [(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0],
            [1.0 + alpha, -2.0 * c, 1.0 - alpha],
        )
    }

    pub fn notch(fs: f64, f0: f64, q: f64) -> Self {
        let w0 = 2.0 * PI * f0 / fs;
        let (s, c) = w0.sin_cos();
        let alpha = s / (2.0 * q);
        Self::normalized([1.0, -2.0 * c, 1.0], [1.0 + alpha, -2.0 * c, 1.0 - alpha])
    }

    fn run<'a>(&self, x: impl Iterator<Item = &'a mut f32>) {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0, 0.0, 0.0);
        for v in x {
            let x0 = *v as f64;
            let y0 = self.b0 * x0 + self.b1 * x1 + self.b2 * x2 - self.a1 * y1 - self.a2 * y2;
            x2 = x1;
            x1 = x0;
            y2 = y1;
            y1 = y0;
            *v = y0 as f32;
        }
    }

    /// Filter in place forward, then backward (zero phase, squared magnitude).
    pub fn filtfilt(&self, x: &mut [f32]) {
        self.run(x.iter_mut());
        self.run(x.iter_mut().rev());
    }
}

/// Q values of the two sections of a 4th-order Butterworth filter.
pub const BUTTERWORTH4_Q: [f64; 2] = [0.541_196_1, 1.306_563];

/// 4th-order Butterworth high-pass, zero phase.
pub fn highpass4(x: &mut [f32], fs: f64, f0: f64) {
    for q in BUTTERWORTH4_Q {
        Biquad::highpass(fs, f0, q).filtfilt(x);
    }
}
