use timegrapher_core::longterm::{self, Grid, LongConfig};
use timegrapher_core::spectrum::amplitude_spectrum;
use timegrapher_core::synth::Rng;

fn main() {
    println!("kind,case,runs,detected_or_peak,other");
    let n = 4096;
    let full: Vec<f64> = (0..n)
        .map(|i| 10.0 * (std::f64::consts::TAU * i as f64 / 64.0).sin())
        .collect();
    for mode in ["full", "every_second_missing", "middle_half_missing"] {
        let y = full
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                if (mode == "every_second_missing" && i % 2 == 1)
                    || (mode == "middle_half_missing" && i >= n / 4 && i < 3 * n / 4)
                {
                    f64::NAN
                } else {
                    v
                }
            })
            .collect();
        let g = Grid {
            t0: 0.0,
            step: 1.0,
            y,
        };
        let s = amplitude_spectrum(&g, "amplitude", "deg", 1.0, 4096).unwrap();
        let peak = s
            .period_s
            .iter()
            .zip(&s.amplitude)
            .filter(|(p, _)| (**p - 64.0).abs() < 1.0)
            .map(|(_, a)| *a)
            .fold(0.0, f64::max);
        println!("spectrum,{mode},1,{peak:.9},input_sine_amplitude_10");
    }
    for (case, rho, missing) in [
        ("white", 0.0, false),
        ("ar1_0_9", 0.9, false),
        ("white_gapped", 0.0, true),
    ] {
        let runs = 200;
        let mut any = 0;
        let mut total = 0;
        let mut worst = 0.0_f64;
        for seed in 0..runs {
            let mut rng = Rng::new(17000 + seed as u64);
            let mut state = 0.0;
            let y = (0..2048)
                .map(|i| {
                    state = rho * state + rng.normal();
                    if missing && i % 128 >= 64 {
                        f64::NAN
                    } else {
                        state
                    }
                })
                .collect();
            let g = Grid {
                t0: 0.0,
                step: 1.0,
                y,
            };
            let r = longterm::analyse(&g, &LongConfig::default());
            if !r.components.is_empty() {
                any += 1;
            }
            total += r.components.len();
            for c in &r.components {
                worst = worst.max(c.significance);
            }
        }
        println!("null,{case},{runs},{any},components={total};maximum_significance={worst:.6}");
    }
}
