use timegrapher_core::{confidence::standard_error, dsp::median, synth::Rng};
fn main() {
    let n = 300usize;
    let repeats = 2000;
    for rho in [0.0f64, 0.5, 0.9] {
        let mut rng = Rng::new(20261014);
        let mut sum_se = 0.;
        let mut sum_sq = 0.;
        let mut within = 0;
        for _ in 0..repeats {
            let mut x = rng.normal();
            let mut values = Vec::new();
            for _ in 0..n {
                x = rho * x + (1. - rho * rho).sqrt() * rng.normal();
                values.push(x);
            }
            let se = standard_error(&values).unwrap();
            let estimate = median(&mut values);
            sum_se += se;
            sum_sq += estimate * estimate;
            if estimate.abs() <= se {
                within += 1;
            }
        }
        println!("stationary Gaussian AR1 rho={rho}, n={n}, repetitions={repeats}: empirical_RMS_median_error={:.6}, mean_reported_SE={:.6}, fraction_within_plusminus_SE={:.4}",(sum_sq/repeats as f64).sqrt(),sum_se/repeats as f64,within as f64/repeats as f64);
    }
}
