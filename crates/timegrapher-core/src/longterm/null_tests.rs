use super::*;
use crate::longterm::{
    detrend_as, detrend_window, reach, search, slow_for, spectrum, x_scale, Band, Grid, DETREND_S,
};
use crate::synth::Rng;

/// The table's take lengths (s) and grid steps, by model: the steps the
/// pipeline reads each at (timing at `longrun::timing_step`, readings on
/// 2 s windows, or 4 s over 48 h).
fn rows(model: NullModel) -> Vec<(f64, f64)> {
    const SHORT: [f64; 5] = [120.0, 300.0, 600.0, 900.0, 1199.0];
    const TO_12H: [f64; 11] = [
        1200.0, 1800.0, 2700.0, 3600.0, 5400.0, 7200.0, 10800.0, 16200.0, 21600.0, 32400.0, 43200.0,
    ];
    const TO_48H: [f64; 5] = [43200.0, 64800.0, 86400.0, 129600.0, 172800.0];
    const TO_144H: [f64; 4] = [172800.0, 259200.0, 345600.0, 518400.0];
    let mut out = Vec::new();
    match model {
        NullModel::Timing => {
            // At 0.5 s to 80 h (`longrun::timing_step`), past it at 4 s.
            out.extend(SHORT.iter().chain(&TO_12H).map(|&s| (s, 0.25)));
            out.extend(TO_48H.iter().chain(&TO_144H[1..3]).map(|&s| (s, 0.5)));
            out.extend(TO_144H[1..].iter().map(|&s| (s, 4.0)));
        }
        NullModel::Readings => {
            out.extend(
                SHORT
                    .iter()
                    .chain(&TO_12H)
                    .chain(&TO_48H[1..])
                    .chain(&TO_144H[1..])
                    .map(|&s| (s, 2.0)),
            );
            out.extend(TO_144H.iter().map(|&s| (s, 4.0)));
        }
    }
    out
}

fn mixes(model: NullModel, scale: f64, step: f64) -> Vec<f64> {
    match model {
        NullModel::Readings => vec![0.0, 0.3, 0.5, 0.7, 0.8, 0.9, 0.95, 0.99],
        // Up to a crossover of 0.35 of the sampling rate: the fit can't
        // see a floor above that.
        NullModel::Timing => [
            0.0, 1.0, 3.0, 10.0, 30.0, 100.0, 300.0, 1000.0, 3000.0, 10000.0,
        ]
        .into_iter()
        .filter(|&m| m / scale < 0.35 / step)
        .chain([WALK_MIX])
        .collect(),
    }
}

/// One null series: AR(1) at ρ `mix`, or a random walk (1 s^-½) plus
/// white noise with the crossover at `mix / scale`.
fn null_grid(model: NullModel, span: f64, step: f64, mix: f64, scale: f64, seed: u64) -> Grid {
    let n = (span / step).round() as usize;
    let mut rng = Rng::new(seed);
    let y = match model {
        NullModel::Readings => {
            let innov = (1.0 - mix * mix).sqrt();
            let mut x = rng.normal();
            (0..n)
                .map(|_| {
                    let v = x;
                    x = mix * x + innov * rng.normal();
                    v
                })
                .collect()
        }
        NullModel::Timing if mix == 0.0 => (0..n).map(|_| rng.normal()).collect(),
        NullModel::Timing if mix >= WALK_MIX => {
            let mut x = 0.0;
            (0..n)
                .map(|_| {
                    x += step.sqrt() * rng.normal();
                    x
                })
                .collect()
        }
        NullModel::Timing => {
            // a = step (the walk's variance a step), b = r²: the crossover
            // has sin(π f_c Δ) = √a / (2r).
            let f_c = mix / scale;
            let r = step.sqrt() / (2.0 * (std::f64::consts::PI * f_c * step).sin());
            let mut x = 0.0;
            (0..n)
                .map(|_| {
                    x += step.sqrt() * rng.normal();
                    x + r * rng.normal()
                })
                .collect()
        }
    };
    Grid { t0: 0.0, step, y }
}

fn seed_of(parts: &[u64]) -> u64 {
    parts.iter().fold(0x9e37_79b9_7f4a_7c15u64, |h, &p| {
        (h ^ p).wrapping_mul(0x0100_0000_01b3).rotate_left(17)
    })
}

/// One cell as measured, before the row's shape is fitted.
#[derive(Serialize, Deserialize)]
struct Raw {
    model: NullModel,
    span_s: f64,
    step_s: f64,
    mix: f64,
    /// The Gumbel's decades per unit of score, and its location less
    /// log10(trials)/slope.
    slope: f64,
    loc0: f64,
    mean: Vec<Option<f64>>,
    #[serde(default)]
    shape: Vec<Option<f64>>,
    #[serde(default)]
    misfit_p99: Option<f64>,
}

#[derive(Serialize, Deserialize)]
struct Measured {
    cell: Raw,
    fitted_mix: f64,
    smax: Vec<f64>,
    /// Each maximum's take's fitted mix (timing, as the search looks it
    /// up); NaN where the table missed it. Empty before the rebinning.
    #[serde(default)]
    fitted: Vec<f64>,
    /// The drawn mix the rebinned tail was fitted to (its shape group).
    #[serde(default, skip_serializing)]
    tail_from: Option<f64>,
    /// Timing: each mean-pass draw's fitted mix and misfit.
    #[serde(default)]
    misfit_draws: Vec<(f64, f64)>,
    /// Timing: each mean-pass draw's fitted mix and, at each x point,
    /// its whitened power's count, sum and sum of squares, from which
    /// [`rebin_means`] reads each cell's mean and shape by fitted mix.
    #[serde(default)]
    per_x: Vec<PerX>,
    /// The fitted Gumbel's location and scale, score units.
    mu: f64,
    sigma: f64,
}

/// Maximum-likelihood Gumbel (location, scale) of the maxima `x`.
fn gumbel_fit(x: &[f64]) -> (f64, f64) {
    let n = x.len() as f64;
    let mean = x.iter().sum::<f64>() / n;
    let top = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    // The scale solves σ = mean − Σ x e^(−x/σ) / Σ e^(−x/σ).
    let g = |sigma: f64| {
        let (mut a, mut b) = (0.0, 0.0);
        for &v in x {
            let e = (-(v - top) / sigma).exp();
            a += v * e;
            b += e;
        }
        sigma - mean + a / b
    };
    let (mut lo, mut hi) = (1e-3, 20.0);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if g(mid) > 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let sigma = 0.5 * (lo + hi);
    let m = x.iter().map(|&v| (-(v - top) / sigma).exp()).sum::<f64>() / n;
    (top - sigma * m.ln(), sigma)
}

fn x_index(x: f64) -> Option<usize> {
    let k = ((x / 0.01).log2() * 4.0).round();
    (k >= 0.0 && k < X_POINTS as f64).then_some(k as usize)
}

/// The whitened null's mean at each `x_at(k)` over the band from half
/// the span to the Nyquist frequency, so that a take between two lengths
/// finds both cells measured over all it searches; the levelled power's
/// gamma shape there, levelled as a take is, by the table's mean at the
/// mix its own fit reads (so the shape carries that fit's scatter too;
/// none until the table has means); and the median mix the fit reads.
fn null_mean(model: NullModel, span: f64, step: f64, mix: f64, draws: usize) -> NullMean {
    let mi = (mix.min(1e15) * 1000.0).round() as u64;
    let id = [model as u64, span as u64, (step * 100.0) as u64, mi];
    let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
    let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
    let scale = x_scale(&g0, w);
    let mut sum = vec![0.0; X_POINTS];
    let mut cnt = vec![0usize; X_POINTS];
    // Levelled power's count, sum and sum of squares.
    let mut lev_n = vec![0usize; X_POINTS];
    let mut lev_s = vec![0.0; X_POINTS];
    let mut lev_ss = vec![0.0; X_POINTS];
    let own = levelling(model, span, step, mix);
    let mut fitted = Vec::new();
    let mut misfits = Vec::new();
    let mut per_x = Vec::new();
    for d in 0..draws {
        let g = null_grid(
            model,
            span,
            step,
            mix,
            scale,
            seed_of(&[id[0], id[1], id[2], id[3], 1, d as u64]),
        );
        let det = detrend_as(&g, DETREND_S, slow_for(model));
        let sp = spectrum(&det, w);
        let jmin = ((2.0 / span) / sp.df).ceil().max(1.0) as usize;
        let mut here = vec![[0.0; 3]; X_POINTS];
        for j in jmin..sp.white.len() {
            if let Some(k) = x_index(j as f64 * sp.df * scale) {
                sum[k] += sp.white[j];
                cnt[k] += 1;
                here[k][0] += 1.0;
                here[k][1] += sp.white[j];
                here[k][2] += sp.white[j] * sp.white[j];
            }
        }
        let f = match model {
            NullModel::Readings => sp.rho,
            NullModel::Timing => {
                let t = fit_of_draw(&sp, w, step, scale);
                if t.misfit.is_finite() {
                    misfits.push((t.mix, t.misfit));
                }
                per_x.push((t.mix, std::mem::take(&mut here)));
                t.mix
            }
        };
        fitted.push(f);
        if let Some(l) = levelling(model, span, step, f).or_else(|| own.clone()) {
            for j in jmin..sp.white.len() {
                let x = j as f64 * sp.df * scale;
                if let (Some(k), Some(m)) = (x_index(x), l.at(x)) {
                    let v = sp.white[j] / m.max(1e-9);
                    lev_n[k] += 1;
                    lev_s[k] += v;
                    lev_ss[k] += v * v;
                }
            }
        }
    }
    let shape = (0..X_POINTS)
        .map(|k| {
            let n = lev_n[k] as f64;
            let m = lev_s[k] / n.max(1.0);
            let var = lev_ss[k] / n.max(1.0) - m * m;
            (lev_n[k] >= 20 && var > 0.0).then(|| (m * m / var * 1e4).round() / 1e4)
        })
        .collect();
    let mean = sum
        .iter()
        .zip(&cnt)
        .map(|(&s, &c)| (c >= 20).then(|| (s / c as f64 * 1e4).round() / 1e4))
        .collect();
    fitted.sort_by(f64::total_cmp);
    let misfit_p99 = (misfits.len() >= 100).then(|| p99(misfits.iter().map(|m| m.1)));
    NullMean {
        mean,
        shape,
        fitted_mix: fitted[fitted.len() / 2],
        misfit_p99,
        misfit_draws: misfits,
        per_x,
    }
}

/// A null draw's timing fit, read from well above the detrend's corner
/// as the search reads it.
fn fit_of_draw(sp: &crate::longterm::Spectrum, w: Option<f64>, step: f64, scale: f64) -> TimingFit {
    let from_x = if w.is_some() { 2.0 } else { 6.0 };
    let from = (from_x / (sp.df * scale)).ceil() as usize;
    timing_fit(&sp.power, sp.df, step, from, scale)
}

/// Timing's misfit draws alone, more of them than the mean takes: the
/// first `draws` are the mean pass's own, so its seeds carry on.
fn misfit_draws(span: f64, step: f64, mix: f64, draws: usize) -> Vec<(f64, f64)> {
    let mi = (mix.min(1e15) * 1000.0).round() as u64;
    let id = [
        NullModel::Timing as u64,
        span as u64,
        (step * 100.0) as u64,
        mi,
    ];
    let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
    let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
    let scale = x_scale(&g0, w);
    (0..draws)
        .filter_map(|d| {
            let seed = seed_of(&[id[0], id[1], id[2], id[3], 1, d as u64]);
            let g = null_grid(NullModel::Timing, span, step, mix, scale, seed);
            let t = fit_of_draw(
                &spectrum(&detrend_as(&g, DETREND_S, slow_for(NullModel::Timing)), w),
                w,
                step,
                scale,
            );
            t.misfit.is_finite().then_some((t.mix, t.misfit))
        })
        .collect()
}

/// Draws a timing cell for its misfit limit: an empirical 99th
/// percentile from 400 left fresh seeds past it 2 to 3 in 100 at some
/// cells (#1245).
const MISFIT_DRAWS: usize = 2000;

/// A timing cell's misfit draws and percentile from `n` draws.
fn set_misfit(m: &mut Measured, n: usize) {
    let c = &m.cell;
    m.misfit_draws = misfit_draws(c.span_s, c.step_s, c.mix, n);
    m.cell.misfit_p99 = Some(p99(m.misfit_draws.iter().map(|d| d.1)));
}

/// The 99th percentile of `x`, rounded as the table keeps it.
fn p99(x: impl Iterator<Item = f64>) -> f64 {
    let mut v: Vec<f64> = x.collect();
    v.sort_by(f64::total_cmp);
    (v[((v.len() as f64 * 0.99).ceil() as usize).max(1) - 1] * 1e4).round() / 1e4
}

/// What [`null_mean`] measures over a cell's draws.
struct NullMean {
    mean: Vec<Option<f64>>,
    shape: Vec<Option<f64>>,
    /// The draws' median fitted mix.
    fitted_mix: f64,
    /// Timing: the 99th percentile of the draws' misfit.
    misfit_p99: Option<f64>,
    /// Timing: each draw's fitted mix and misfit.
    misfit_draws: Vec<(f64, f64)>,
    /// Timing: each draw's fitted mix and whitened power by x point.
    per_x: Vec<PerX>,
}

/// One draw's fitted mix and, at each x point, the count, sum and sum of
/// squares of its whitened power.
type PerX = (f64, Vec<[f64; 3]>);

/// The maxima of the same draws as [`measure`]'s, each levelled as a take
/// is: by the table's levelling at its own fitted mix, so the fit
/// includes the scatter of that mix (Norm on #1218: timing near white
/// read p 0.001 2.3 times too often when the maxima were levelled at the
/// true mix). A draw the table can't place is levelled at the true mix
/// and counted.
fn maxima_as_takes(
    model: NullModel,
    span: f64,
    step: f64,
    mix: f64,
    draws: usize,
) -> (Vec<(f64, f64)>, usize) {
    let mi = (mix.min(1e15) * 1000.0).round() as u64;
    let id = [model as u64, span as u64, (step * 100.0) as u64, mi];
    let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
    let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
    let scale = x_scale(&g0, w);
    let own = levelling(model, span, step, mix).expect("a table cell");
    let mut missed = 0;
    let smax = (0..draws)
        .map(|d| {
            let seed = seed_of(&[id[0], id[1], id[2], id[3], 2, d as u64]);
            let det = detrend_as(
                &null_grid(model, span, step, mix, scale, seed),
                DETREND_S,
                slow_for(model),
            );
            let band = |given| Band {
                min_p: 3f64.max(8.0 * step),
                max_p: reach(model, span / 3.0, w),
                false_alarm: 0.01,
                detrend_w: w,
                model,
                given,
            };
            let mut s = search(&det, &band(None), &[]);
            let fitted = if s.null.calibrated {
                s.null.mix
            } else {
                f64::NAN
            };
            if !s.null.calibrated {
                missed += 1;
                s = search(&det, &band(Some(&own)), &[]);
            }
            (fitted, s.score.iter().copied().fold(0.0, f64::max))
        })
        .collect();
    (smax, missed)
}

fn measure(model: NullModel, span: f64, step: f64, mix: f64, draws: usize) -> Measured {
    let mi = (mix.min(1e15) * 1000.0).round() as u64;
    let id = [model as u64, span as u64, (step * 100.0) as u64, mi];
    let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
    let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
    let scale = x_scale(&g0, w);
    let min_p = 3f64.max(8.0 * step);
    let NullMean {
        mean,
        shape,
        fitted_mix,
        misfit_p99,
        misfit_draws,
        per_x,
    } = null_mean(model, span, step, mix, draws);
    // The plain trials count's tail; the maxima don't depend on it.
    let lev = Levelling {
        mean: mean.clone(),
        shape: shape.clone(),
        misfit_p99: None,
        mu: 0.0,
        sigma: 1.0 / std::f64::consts::LN_10,
        xi: 0.0,
        log_trials: 0.0,
    };
    // The maximum score over the searched band, levelled by that mean.
    let mut smax = Vec::new();
    let mut log_trials = 0.0;
    for d in 0..draws {
        let g = null_grid(
            model,
            span,
            step,
            mix,
            scale,
            seed_of(&[id[0], id[1], id[2], id[3], 2, d as u64]),
        );
        let det = detrend_as(&g, DETREND_S, slow_for(model));
        let band = Band {
            min_p,
            max_p: reach(model, span / 3.0, w),
            false_alarm: 0.01,
            detrend_w: w,
            model,
            given: Some(&lev),
        };
        let s = search(&det, &band, &[]);
        if !s.null.calibrated {
            let def: Vec<usize> = (0..X_POINTS).filter(|&k| mean[k].is_some()).collect();
            panic!("{model:?} {span} {step} {mix}: band outside the mean; defined {def:?} scale {scale} min_p {min_p}");
        }
        log_trials = s.trials.log10();
        smax.push(s.score.iter().copied().fold(0.0, f64::max));
    }
    let (mu, sigma) = gumbel_fit(&smax);
    let slope = 1.0 / (sigma * std::f64::consts::LN_10);
    Measured {
        cell: Raw {
            model,
            span_s: span,
            step_s: step,
            mix,
            slope: (slope * 1e4).round() / 1e4,
            loc0: ((mu - log_trials / slope) * 1e4).round() / 1e4,
            mean,
            shape,
            misfit_p99,
        },
        fitted_mix,
        smax,
        fitted: Vec::new(),
        tail_from: None,
        misfit_draws,
        per_x,
        mu,
        sigma,
    }
}

/// How the generator gets each cell: measured, or read back from the
/// cache and partly redone.
struct Run {
    draws: usize,
    cache: Option<String>,
    remean: bool,
    refit: Option<String>,
    /// Only timing's misfit percentile measured again, all else kept.
    misfit: bool,
    /// Draws a cell for the misfit (`NULL_MISFIT_DRAWS`, default
    /// [`MISFIT_DRAWS`]), at least `draws`.
    misfit_n: usize,
    /// `NULL_PERX`: a cached timing cell without per-draw powers gets
    /// them from its mean pass's own draws, all else kept.
    perx: bool,
}

impl Run {
    fn cell(&self, model: NullModel, span: f64, step: f64, mix: f64) -> Measured {
        let Run {
            draws,
            cache,
            remean,
            refit,
            misfit,
            misfit_n,
            perx,
        } = self;
        let (draws, remean, misfit, misfit_n) = (*draws, *remean, *misfit, *misfit_n);
        let refit_too = refit
            .as_ref()
            .is_some_and(|r| r == "all" || format!("{model:?}") == *r);
        // A cell already measured (the run was cut short) is
        // read back rather than run again.
        let kept = cache
            .as_ref()
            .map(|d| format!("{d}/{model:?}_{span}_{step}_{mix}_{draws}.json"));
        let old = kept.as_ref().and_then(|f| std::fs::read_to_string(f).ok());
        let old = old.and_then(|t| serde_json::from_str::<Measured>(&t).ok());
        let old = match old {
            Some(mut m) if *perx && model == NullModel::Timing && m.per_x.is_empty() => {
                m.per_x = null_mean(model, span, step, mix, draws).per_x;
                if let Some(f) = &kept {
                    std::fs::write(f, serde_json::to_string(&m).unwrap()).unwrap();
                }
                eprintln!("{model:?} span {span} step {step} mix {mix}: per-draw powers");
                if !refit_too {
                    return m;
                }
                Some(m)
            }
            o => o,
        };
        match old {
            // The mean measured again over its band, the
            // maxima kept (they use the band's mean only,
            // the same draws).
            Some(mut m) if !remean && misfit && model == NullModel::Timing => {
                set_misfit(&mut m, misfit_n.max(draws));
                if let Some(f) = &kept {
                    std::fs::write(f, serde_json::to_string(&m).unwrap()).unwrap();
                }
                m
            }
            Some(mut m) if remean || (misfit && model == NullModel::Timing) => {
                let n = null_mean(model, span, step, mix, draws);
                if remean {
                    (m.cell.mean, m.cell.shape) = (n.mean, n.shape);
                }
                m.cell.misfit_p99 = n.misfit_p99;
                m.misfit_draws = n.misfit_draws;
                if let Some(f) = &kept {
                    std::fs::write(f, serde_json::to_string(&m).unwrap()).unwrap();
                }
                m
            }
            // The maxima drawn again, levelled at each
            // draw's own mix (the means, which the
            // lookup reads, already in the table).
            Some(mut m)
                if refit
                    .as_ref()
                    .is_some_and(|r| r == "all" || format!("{model:?}") == *r) =>
            {
                let (taken, missed) = maxima_as_takes(model, span, step, mix, draws);
                (m.fitted, m.smax) = taken.into_iter().unzip();
                (m.mu, m.sigma) = gumbel_fit(&m.smax);
                eprintln!("{model:?} span {span} step {step} mix {mix}: refit, {missed} placed at the true mix");
                if let Some(f) = &kept {
                    std::fs::write(f, serde_json::to_string(&m).unwrap()).unwrap();
                }
                m
            }
            Some(m) => m,
            None => {
                let mut m = measure(model, span, step, mix, draws);
                if model == NullModel::Timing && misfit_n > draws {
                    set_misfit(&mut m, misfit_n);
                }
                if let Some(f) = &kept {
                    std::fs::write(f, serde_json::to_string(&m).unwrap()).unwrap();
                }
                m
            }
        }
    }
}

/// Writes `data/period-null.json`. About an hour on 4 cores:
/// `cargo test --release -p timegrapher-core --lib write_period_null_table -- --ignored --nocapture`.
#[test]
#[ignore = "writes the table; run by hand"]
fn write_period_null_table() {
    let draws: usize = std::env::var("NULL_DRAWS").map_or(400, |s| s.parse().unwrap());
    let only: Option<String> = std::env::var("NULL_ONLY").ok();
    let run = Run {
        draws,
        cache: std::env::var("NULL_CACHE").ok(),
        remean: std::env::var("NULL_REMEAN").is_ok(),
        // `NULL_REFIT=Timing`: that model's maxima levelled as takes are.
        refit: std::env::var("NULL_REFIT").ok(),
        misfit: std::env::var("NULL_MISFIT").is_ok(),
        misfit_n: std::env::var("NULL_MISFIT_DRAWS").map_or(MISFIT_DRAWS, |s| s.parse().unwrap()),
        perx: std::env::var("NULL_PERX").is_ok(),
    };
    let mut jobs = Vec::new();
    for model in [NullModel::Timing, NullModel::Readings] {
        for (span, step) in rows(model) {
            let g = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
            let w = detrend_window(&g, DETREND_S).map(|w| w as f64 * step);
            let scale = x_scale(&g, w);
            for mix in mixes(model, scale, step) {
                if only
                    .as_deref()
                    .is_some_and(|o| !format!("{model:?} {span} {step}").starts_with(o))
                {
                    continue;
                }
                jobs.push((model, span, step, mix));
            }
        }
    }
    // Longest first, so the threads finish together.
    jobs.sort_by(|a, b| (b.1 / b.2).total_cmp(&(a.1 / a.2)));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut done: Vec<Measured> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads)
            .map(|_| {
                sc.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&(model, span, step, mix)) = jobs.get(i) else { break };
                        let m = run.cell(model, span, step, mix);
                        eprintln!(
                            "{model:?} span {span} step {step} mix {mix}: fitted {:.3} slope {:.3} loc0 {:.3}",
                            m.fitted_mix, m.cell.slope, m.cell.loc0
                        );
                        out.push(m);
                    }
                    out
                })
            })
            .collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    done.sort_by(|a, b| {
        (a.cell.model as u8, a.cell.step_s, a.cell.span_s, a.cell.mix)
            .partial_cmp(&(b.cell.model as u8, b.cell.step_s, b.cell.span_s, b.cell.mix))
            .unwrap()
    });
    rebin(&mut done);
    let cells = fit_shapes(&done);
    // The tail check: with each cell's fit, how often a null maximum reads
    // under 1% and 0.1% (pooled over every cell).
    let (mut n, mut p1, mut p01) = (0usize, 0usize, 0usize);
    for (m, c) in done.iter().zip(&cells) {
        let (mut cn, mut c1, mut c01) = (0, 0, 0);
        for &s in &m.smax {
            let p = gev_p(s, c.mu, c.sigma, c.xi);
            cn += 1;
            c1 += usize::from(p < 0.01);
            c01 += usize::from(p < 0.001);
        }
        eprintln!(
            "tail {:?} {} {} {}: {cn} {c1} {c01}",
            c.model, c.span_s, c.step_s, c.mix
        );
        (n, p1, p01) = (n + cn, p1 + c1, p01 + c01);
    }
    let misfit_n = run.misfit_n.max(draws);
    let note = format!(
        "Generated by write_period_null_table (longterm/null_tests.rs), fixed seeds. {draws} draws a cell for the mean and as many for the maximum. The maximum's generalised extreme value by maximum likelihood: location and scale for each cell (the scale's relative error about {:.0}%); the shape profiled over 0 to {:.1} in steps of 0.02, shared by each take length and step (readings) or each mix (timing). Timing's location and scale are fitted to the maxima grouped by the mix each take's search fitted, which is the coordinate the lookup reads, pooled from every drawn mix of that length and step. The drawn mixes are white, 1 to 10^4 up to 0.35 of the sampling rate, and walk. The drawn mixes reach both ends of the lookup's axis, white and walk, so no fitted mix lies past them. A cell that fewer than {MIN_BIN} draws read takes the {MIN_BIN} read nearest it. Each cell's tail is then the least favourable over the drawn mixes read as it or either neighbour (the lookup blends neighbours): fitted to the drawn mix, of those giving at least {MIN_SOURCE} draws, whose maxima most often pass the fit's p 0.001 and 0.0003 levels, refitted until that mix is the one fitted, and sharing that mix's shape group. So the false-alarm chance holds for every drawn mix, not on average over them. Timing's mean and gamma shape at each x are read the same way: the highest mean and the lowest shape over the drawn mixes (of those with at least {MIN_SOURCE} draws there) read as the cell or either neighbour, since the fit reads the mix only at x of 2 and above. Each bin's whitened power is scored by its own null spread (a gamma shape per x). Each timing cell also gives a misfit limit: the misfit is the rms in decades of the quarter-octave band powers from the walk plus white fitted to them, and the limit is the largest 99th percentile of it ({misfit_n} draws a drawn mix) over the drawn mixes (of those with at least {MIN_SOURCE} draws there) read as this cell or either neighbour. Pooled tail check: {p1} of {n} null maxima under p 0.01 (expected {:.0}), {p01} under 0.001 (expected {:.0}).",
        78.0 / (draws as f64).sqrt(),
        0.02 * XI_STEPS as f64,
        n as f64 * 0.01,
        n as f64 * 0.001
    );
    eprintln!("{note}");
    let mut text = format!(
        "{{\"note\": {},\n\"draws_mean\": {draws},\n\"draws_max\": {draws},\n\"cells\": [\n",
        serde_json::to_string(&note).unwrap()
    );
    for (i, c) in cells.iter().enumerate() {
        text += &serde_json::to_string(c).unwrap();
        text += if i + 1 < cells.len() { ",\n" } else { "\n" };
    }
    text += "]}\n";
    let path = std::env::var("NULL_OUT").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/data/period-null.json").to_string()
    });
    std::fs::write(path, text).unwrap();
}

/// The exact chance a GEV(μ, σ, ξ) maximum exceeds `s`.
fn gev_p(s: f64, mu: f64, sigma: f64, xi: f64) -> f64 {
    let z = (s - mu) / sigma;
    let h = if xi.abs() < 1e-9 {
        (-z).exp()
    } else {
        let t = 1.0 + xi * z;
        if t <= 0.0 {
            return if xi > 0.0 { 1.0 } else { 0.0 };
        }
        t.powf(-1.0 / xi)
    };
    -(-h).exp_m1()
}

/// The GEV log-likelihood of the maxima `x`.
fn gev_ll(x: &[f64], mu: f64, sigma: f64, xi: f64) -> f64 {
    if sigma <= 0.0 {
        return f64::NEG_INFINITY;
    }
    x.iter()
        .map(|&v| {
            let z = (v - mu) / sigma;
            if xi.abs() < 1e-9 {
                return -sigma.ln() - z - (-z).exp();
            }
            let t = 1.0 + xi * z;
            if t <= 0.0 {
                return f64::NEG_INFINITY;
            }
            -sigma.ln() - (1.0 + 1.0 / xi) * t.ln() - t.powf(-1.0 / xi)
        })
        .sum()
}

/// μ and σ at the shape `xi` by maximum likelihood (a pattern search
/// from the Gumbel fit), and the likelihood.
fn gev_fit(x: &[f64], xi: f64, mu0: f64, sigma0: f64) -> (f64, f64, f64) {
    let (mut mu, mut sigma) = (mu0, sigma0);
    let mut best = gev_ll(x, mu, sigma, xi);
    let mut step = (0.1, 0.05);
    while step.0 > 1e-5 {
        let mut moved = false;
        for (dm, ds) in [(step.0, 0.0), (-step.0, 0.0), (0.0, step.1), (0.0, -step.1)] {
            let l = gev_ll(x, mu + dm, sigma + ds, xi);
            if l > best {
                (best, mu, sigma, moved) = (l, mu + dm, sigma + ds, true);
            }
        }
        if !moved {
            step = (step.0 / 2.0, step.1 / 2.0);
        }
    }
    (mu, sigma, best)
}

/// The table's cells: for each take length and step, the shape that
/// fits all its cells' maxima best, and each cell's location and scale at
/// it.
/// Timing's maxima regrouped by the mix the search looks up, not the
/// mix drawn: each take length and step's draws, from every drawn mix,
/// go to the cell nearest their fitted mix on the lookup's axis, and that
/// cell's location and scale are fitted to them. The tail is then the one
/// the deployed lookup meets, calibrated over the table's spread of drawn
/// mixes (#1195, Norm's (e)). Draws the table missed are left out, as
/// the search leaves them uncalibrated.
/// The fewest draws a rebinned cell's tail is fitted to.
const MIN_BIN: usize = 200;
/// The fewest draws of one drawn mix that can set a cell's tail.
const MIN_SOURCE: usize = 50;

/// The least favourable drawn mix among `read` (fitted mix, maximum,
/// drawn mix's index): starting from the pooled fit, the drawn mix (of
/// those with [`MIN_SOURCE`] draws) whose maxima most often pass that
/// fit's p 0.001 and 0.0003 levels (p 0.01 breaking ties), refitted to
/// that mix's maxima, until the worst mix is the one fitted. Returns its
/// maxima, or all of them when no mix has enough.
fn least_favourable(read: &[(f64, f64, usize)], sources: usize) -> (Vec<f64>, Option<usize>) {
    let of = |src: usize| -> Vec<f64> { read.iter().filter(|d| d.2 == src).map(|d| d.1).collect() };
    let pool: Vec<f64> = read.iter().map(|d| d.1).collect();
    let mut fit = gumbel_fit(&pool);
    let mut chosen: Option<usize> = None;
    for _ in 0..8 {
        let excess = |x: &[f64]| {
            let n = x.len() as f64;
            let past = |p: f64| {
                x.iter()
                    .filter(|&&s| gev_p(s, fit.0, fit.1, 0.0) < p)
                    .count() as f64
                    / (n * p)
            };
            (past(0.001) + past(0.0003), past(0.01))
        };
        let worst = (0..sources)
            .map(|src| (src, of(src)))
            .filter(|(_, x)| x.len() >= MIN_SOURCE)
            .map(|(src, x)| (src, excess(&x)))
            .max_by(|a, b| a.1 .0.total_cmp(&b.1 .0).then(a.1 .1.total_cmp(&b.1 .1)))
            .map(|(src, _)| src);
        if worst.is_none() || worst == chosen {
            break;
        }
        chosen = worst;
        fit = gumbel_fit(&of(worst.unwrap()));
    }
    match chosen {
        Some(src) => (of(src), Some(src)),
        None => (pool, None),
    }
}

fn rebin_by_fitted_mix(done: &mut [Measured]) {
    let mut rows: Vec<(f64, f64)> = done
        .iter()
        .filter(|m| m.cell.model == NullModel::Timing)
        .map(|m| (m.cell.span_s, m.cell.step_s))
        .collect();
    rows.dedup();
    for (span, step) in rows {
        let idx: Vec<usize> = (0..done.len())
            .filter(|&i| {
                let c = &done[i].cell;
                c.model == NullModel::Timing && c.span_s == span && c.step_s == step
            })
            .collect();
        let keys: Vec<f64> = idx
            .iter()
            .map(|&i| axis(NullModel::Timing, done[i].cell.mix))
            .collect();
        let mut all: Vec<(f64, f64, usize)> = Vec::new();
        for (src, &i) in idx.iter().enumerate() {
            let m = &done[i];
            assert_eq!(
                m.fitted.len(),
                m.smax.len(),
                "{span} {step} {}: refit first",
                m.cell.mix
            );
            all.extend(
                m.fitted
                    .iter()
                    .zip(&m.smax)
                    .filter(|(f, _)| f.is_finite())
                    .map(|(&f, &x)| (axis(NullModel::Timing, f), x, src)),
            );
        }
        let nearest = |u: f64| {
            (0..keys.len())
                .min_by(|&a, &b| (keys[a] - u).abs().total_cmp(&(keys[b] - u).abs()))
                .unwrap()
        };
        for (k, &i) in idx.iter().enumerate() {
            // The draws read as this cell or either neighbour: the lookup
            // blends a cell with its neighbours, so their drawn mixes reach
            // its tail too.
            let near_k = |d: &&(f64, f64, usize)| nearest(d.0).abs_diff(k) <= 1;
            let mut read: Vec<(f64, f64, usize)> = all.iter().filter(near_k).copied().collect();
            // A mix the fit seldom reads at this length (short takes read
            // most walks near white) takes the draws read nearest it.
            let own = read.len();
            if own < MIN_BIN {
                read = all.clone();
                read.sort_by(|a, b| (a.0 - keys[k]).abs().total_cmp(&(b.0 - keys[k]).abs()));
                read.truncate(MIN_BIN);
            }
            let (b, worst) = least_favourable(&read, idx.len());
            let worst = worst.map_or(f64::NAN, |src| done[idx[src]].cell.mix);
            let m = &mut done[i];
            eprintln!(
                "rebin Timing {span} {step} {}: {own} maxima read there; least favourable drawn mix {worst}, {} maxima",
                m.cell.mix,
                b.len()
            );
            (m.mu, m.sigma) = gumbel_fit(&b);
            m.smax = b;
            m.tail_from = worst.is_finite().then_some(worst);
        }
        rebin_misfit(done, &idx, &keys);
    }
}

/// `NULL_MEAN_REBIN`: timing's means and shapes read by fitted mix,
/// least favourable ([`rebin_means`]); before the tails (`NULL_REBIN`),
/// whose maxima (`NULL_REFIT`) are levelled by the table these means
/// went into.
fn rebin(done: &mut [Measured]) {
    if std::env::var("NULL_MEAN_REBIN").is_ok() {
        rebin_means(done);
    }
    if std::env::var("NULL_REBIN").is_ok() {
        rebin_by_fitted_mix(done);
    }
}

/// A draw's nearest cell and its whitened power by x point.
type ReadAs<'a> = (usize, &'a [[f64; 3]]);

/// A cell's mean and gamma shape at each x point.
type MeanShape = (Vec<Option<f64>>, Vec<Option<f64>>);

/// Each timing cell's mean and shape at each x as the lookup meets them,
/// by the rule the tail follows ([`rebin_by_fitted_mix`]): from the draws
/// of every drawn mix read as this cell or either neighbour, the drawn
/// mix (of those with [`MIN_SOURCE`] draws there) with the highest mean
/// at that x, and the lowest gamma shape (the heaviest spread). The fit
/// reads the mix only at x ≥ 2, so below it, where the sloped detrend's
/// response rises to 1.4 to 1.76 near 0.7 of its window, a take's power
/// is set by its walk while its mix may read white: mix 3 read as white
/// on 35% of 30 min takes, and those passed p 0.001 2.9 times in 100 when
/// levelled by the white draws' own mean (Norm). Means are a take's
/// divisor, so the highest is the least favourable; the shape is
/// unchanged by the divisor. A cell no drawn mix reads keeps its own.
fn rebin_means(done: &mut [Measured]) {
    let mut rows: Vec<(f64, f64)> = done
        .iter()
        .filter(|m| m.cell.model == NullModel::Timing)
        .map(|m| (m.cell.span_s, m.cell.step_s))
        .collect();
    rows.dedup();
    for (span, step) in rows {
        let idx: Vec<usize> = (0..done.len())
            .filter(|&i| {
                let c = &done[i].cell;
                c.model == NullModel::Timing && c.span_s == span && c.step_s == step
            })
            .collect();
        let keys: Vec<f64> = idx
            .iter()
            .map(|&i| axis(NullModel::Timing, done[i].cell.mix))
            .collect();
        let nearest = |u: f64| {
            (0..keys.len())
                .min_by(|&a, &b| (keys[a] - u).abs().total_cmp(&(keys[b] - u).abs()))
                .unwrap()
        };
        // Each source's draws, by the cell they read as.
        let read: Vec<Vec<ReadAs>> = idx
            .iter()
            .map(|&i| {
                assert!(
                    !done[i].per_x.is_empty(),
                    "{span} {step} {}: no per-draw powers; measure again",
                    done[i].cell.mix
                );
                done[i]
                    .per_x
                    .iter()
                    .filter(|d| d.0.is_finite())
                    .map(|d| (nearest(axis(NullModel::Timing, d.0)), d.1.as_slice()))
                    .collect()
            })
            .collect();
        let mut out: Vec<MeanShape> = Vec::new();
        for k in 0..idx.len() {
            let mut mean = vec![None::<f64>; X_POINTS];
            let mut shape = vec![None::<f64>; X_POINTS];
            for r in &read {
                let mine: Vec<_> = r.iter().filter(|d| d.0.abs_diff(k) <= 1).collect();
                if mine.len() < MIN_SOURCE {
                    continue;
                }
                for x in 0..X_POINTS {
                    let (n, s1, s2) = mine.iter().fold((0.0, 0.0, 0.0), |a, d| {
                        (a.0 + d.1[x][0], a.1 + d.1[x][1], a.2 + d.1[x][2])
                    });
                    if n < 20.0 {
                        continue;
                    }
                    let m = s1 / n;
                    let var = s2 / n - m * m;
                    mean[x] = Some(mean[x].map_or(m, |v| v.max(m)));
                    if var > 0.0 {
                        let g = m * m / var;
                        shape[x] = Some(shape[x].map_or(g, |v| v.min(g)));
                    }
                }
            }
            out.push((mean, shape));
        }
        let round = |v: Option<f64>| v.map(|v| (v * 1e4).round() / 1e4);
        for (k, &i) in idx.iter().enumerate() {
            let (mean, shape) = &out[k];
            let c = &mut done[i].cell;
            // A point no source with enough draws reads keeps its own.
            for x in 0..X_POINTS {
                if mean[x].is_some() {
                    c.mean[x] = round(mean[x]);
                }
                if shape[x].is_some() && c.shape.len() == X_POINTS {
                    c.shape[x] = round(shape[x]);
                }
            }
            eprintln!("means Timing {span} {step} {}: read by fitted mix", c.mix);
        }
    }
}

/// Each timing cell's misfit limit as the lookup meets it: the largest
/// 99th percentile over the drawn mixes (of those with [`MIN_SOURCE`]
/// draws there) read as this cell or either neighbour (Norm on #1245).
/// A cell no drawn mix reads keeps its own.
fn rebin_misfit(done: &mut [Measured], idx: &[usize], keys: &[f64]) {
    let nearest = |u: f64| {
        (0..keys.len())
            .min_by(|&a, &b| (keys[a] - u).abs().total_cmp(&(keys[b] - u).abs()))
            .unwrap()
    };
    let read: Vec<Vec<(usize, f64)>> = idx
        .iter()
        .map(|&i| {
            done[i]
                .misfit_draws
                .iter()
                .filter(|d| d.0.is_finite())
                .map(|&(f, m)| (nearest(axis(NullModel::Timing, f)), m))
                .collect()
        })
        .collect();
    if read.iter().all(Vec::is_empty) {
        return;
    }
    for (k, &i) in idx.iter().enumerate() {
        let worst = read
            .iter()
            .map(|r| {
                r.iter()
                    .filter(|d| d.0.abs_diff(k) <= 1)
                    .map(|d| d.1)
                    .collect::<Vec<f64>>()
            })
            .filter(|x| x.len() >= MIN_SOURCE)
            .map(|x| p99(x.into_iter()))
            .fold(f64::NAN, f64::max);
        let c = &mut done[i].cell;
        if worst.is_finite() {
            eprintln!(
                "misfit Timing {} {} {}: own {:?}, limit {worst}",
                c.span_s, c.step_s, c.mix, c.misfit_p99
            );
            c.misfit_p99 = Some(worst);
        }
    }
}

/// A shape group: model, span, step (hundredths of a second), mix.
type GroupKey = (u8, u64, u64, u64);

fn fit_shapes(done: &[Measured]) -> Vec<Cell> {
    let round = |v: f64| (v * 1e4).round() / 1e4;
    // The shape is shared by the cells of a group: for readings each take
    // length and step; for timing each mix (its tail's weight follows the
    // mix, which the fit draws near white and scatters, more than the
    // length). `NULL_SHAPE_BY=row` gives timing the readings' grouping.
    let by_row = std::env::var("NULL_SHAPE_BY").is_ok_and(|v| v == "row");
    let key = |m: &Measured| -> GroupKey {
        let c = &m.cell;
        match c.model {
            // A rebinned cell shares the shape of the drawn mix its tail
            // came from: the least favourable mix's tail, shape and all.
            NullModel::Timing if !by_row => {
                (1, 0, 0, m.tail_from.unwrap_or(c.mix).min(1e15) as u64)
            }
            _ => (c.model as u8, c.span_s as u64, (c.step_s * 100.0) as u64, 0),
        }
    };
    let mut groups: Vec<(GroupKey, Vec<usize>)> = Vec::new();
    for (i, m) in done.iter().enumerate() {
        match groups.iter_mut().find(|g| g.0 == key(m)) {
            Some(g) => g.1.push(i),
            None => groups.push((key(m), vec![i])),
        }
    }
    let mut fitted: Vec<Option<(f64, f64, f64)>> = vec![None; done.len()];
    for (k, idx) in &groups {
        let best = (0..=XI_STEPS)
            .map(|j| {
                let xi = 0.02 * j as f64;
                let fits: Vec<(f64, f64, f64)> = idx
                    .iter()
                    .map(|&i| gev_fit(&done[i].smax, xi, done[i].mu, done[i].sigma))
                    .collect();
                (fits.iter().map(|f| f.2).sum::<f64>(), xi, fits)
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap();
        let (_, xi, fits) = best;
        eprintln!("group {k:?}: shape {xi}");
        for (&i, (mu, sigma, _)) in idx.iter().zip(fits) {
            fitted[i] = Some((mu, sigma, xi));
        }
    }
    done.iter()
        .zip(fitted)
        .map(|(m, f)| {
            let (mu, sigma, xi) = f.expect("every cell in a group");
            let c = &m.cell;
            Cell {
                model: c.model,
                span_s: c.span_s,
                step_s: c.step_s,
                mix: c.mix,
                mu: round(mu),
                sigma: round(sigma),
                xi: round(xi),
                // The Gumbel's location less log10(trials)/slope gave it.
                log_trials: round((m.mu - c.loc0) * c.slope),
                mean: c.mean.clone(),
                shape: c.shape.clone(),
                misfit_p99: c.misfit_p99,
            }
        })
        .collect()
}

/// The shape's profile runs 0 to 0.02·`XI_STEPS`.
const XI_STEPS: usize = 15;

/// Outside the table there is no levelling, never a clamped cell: a
/// take shorter than its shortest, longer than its longest, a ρ past its
/// last, a step it has no cells for, a step it has only at other lengths,
/// a span between the two detrend branches' cells, or no fitted mix
/// (#1195, Norm's condition 2).
#[test]
fn levelling_is_none_outside_the_table() {
    use NullModel::{Readings, Timing};
    for (model, span, step, mix) in [
        (Readings, 60.0, 2.0, 0.3),
        (Readings, 1.0e6, 2.0, 0.3),
        (Readings, 1800.0, 2.0, 0.995),
        (Readings, 1800.0, 1.0, 0.3),
        (Readings, 3600.0, 4.0, 0.3),
        (Readings, 1199.5, 2.0, 0.3),
        (Timing, 60.0, 0.25, 10.0),
        (Timing, 1.0e6, 4.0, 10.0),
        (Timing, 1800.0, 0.1, 10.0),
        (Timing, 1800.0, 0.25, f64::NAN),
    ] {
        assert!(
            levelling(model, span, step, mix).is_none(),
            "{model:?} {span} s at {step} s, mix {mix}"
        );
    }
    // Each side's own edge cells are inside.
    assert!(levelling(Readings, 1199.0, 2.0, 0.3).is_some());
    assert!(levelling(Readings, 1200.0, 2.0, 0.3).is_some());
    assert!(levelling(Readings, 1800.0, 2.0, 0.99).is_some());
}

/// A lookup on a cell returns that cell; one between two take lengths or
/// two mixes is the blend of the cells either side, linear in log span
/// and on the mix axis.
#[test]
fn a_lookup_between_cells_interpolates() {
    use NullModel::Readings;
    let cell = |span: f64, mix: f64| {
        table()
            .cells
            .iter()
            .find(|c| c.model == Readings && c.span_s == span && c.step_s == 2.0 && c.mix == mix)
            .unwrap_or_else(|| panic!("no cell {span} {mix}"))
    };
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9 * (1.0 + b.abs());
    let on = levelling(Readings, 1800.0, 2.0, 0.5).unwrap();
    let c = cell(1800.0, 0.5);
    assert_eq!(
        (on.mu, on.sigma, on.xi, on.log_trials),
        (c.mu, c.sigma, c.xi, c.log_trials)
    );
    assert_eq!(on.mean, c.mean);
    assert_eq!(on.shape, c.shape);
    // Halfway in log span between 1800 and 2700 s.
    let (a, b) = (cell(1800.0, 0.5), cell(2700.0, 0.5));
    let mid = levelling(Readings, (1800.0f64 * 2700.0).sqrt(), 2.0, 0.5).unwrap();
    assert!(
        close(mid.mu, (a.mu + b.mu) / 2.0),
        "{} {} {}",
        mid.mu,
        a.mu,
        b.mu
    );
    assert!(close(mid.sigma, (a.sigma + b.sigma) / 2.0));
    for k in 0..X_POINTS {
        match (a.mean[k], b.mean[k], mid.mean[k]) {
            (Some(x), Some(y), Some(m)) => assert!(close(m, (x + y) / 2.0)),
            (_, _, m) => assert!(m.is_none()),
        }
        match (a.shape[k], b.shape[k], mid.shape[k]) {
            (Some(x), Some(y), Some(m)) => assert!(close(m, (x + y) / 2.0), "shape at {k}"),
            (_, _, m) => assert!(m.is_none(), "shape at {k}"),
        }
    }
    assert!(
        (0..X_POINTS).any(|k| a.shape[k] != b.shape[k]),
        "the shapes differ"
    );
    // A quarter of the way from ρ 0.5 to 0.7 at 1800 s.
    let (a, b) = (cell(1800.0, 0.5), cell(1800.0, 0.7));
    let q = levelling(Readings, 1800.0, 2.0, 0.55).unwrap();
    assert!(
        close(q.mu, a.mu + (b.mu - a.mu) * 0.25),
        "{} {} {}",
        q.mu,
        a.mu,
        b.mu
    );
    assert!(close(q.xi, a.xi + (b.xi - a.xi) * 0.25));
    assert!(q.mu != a.mu && q.mu != b.mu);
    for k in 0..X_POINTS {
        match (a.shape[k], b.shape[k], q.shape[k]) {
            (Some(x), Some(y), Some(m)) => assert!(close(m, x + (y - x) * 0.25), "shape at {k}"),
            (_, _, m) => assert!(m.is_none(), "shape at {k}"),
        }
    }
    assert!(
        (0..X_POINTS).any(|k| a.shape[k] != b.shape[k]),
        "the shapes differ"
    );
}

/// Out of sample, the misfit caveat (#1245): each timing cell's walk plus
/// white, fresh seeds, with a sine the search names, read as a take is.
/// Prints each cell's named count and how many of those carry
/// `timing_model_misfit`; at the 99th percentile, about 1 in 100. Run by
/// hand: `FRESH_DRAWS` a cell (default 200), `FRESH_MIX` one drawn mix,
/// `FRESH_SPAN` one take length, `FRESH_TAG` another batch of seeds,
/// `FRESH_NOSINE` the null alone read at its own fit (every draw counted).
#[test]
#[ignore = "out-of-sample misfit check; run by hand"]
fn fresh_sines_hold_the_misfit() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let draws: usize = std::env::var("FRESH_DRAWS").map_or(200, |s| s.parse().unwrap());
    let only_mix: Option<f64> = std::env::var("FRESH_MIX").ok().map(|s| s.parse().unwrap());
    let only_span: Option<f64> = std::env::var("FRESH_SPAN").ok().map(|s| s.parse().unwrap());
    let tag: u64 = std::env::var("FRESH_TAG").map_or(13, |s| s.parse().unwrap());
    let mut cells: Vec<&Cell> = table()
        .cells
        .iter()
        .filter(|c| {
            c.model == NullModel::Timing
                && only_mix.is_none_or(|x| c.mix == x)
                && only_span.is_none_or(|x| c.span_s == x)
        })
        .collect();
    cells.sort_by(|a, b| (b.span_s / b.step_s).total_cmp(&(a.span_s / a.step_s)));
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|sc| {
        for _ in 0..workers {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(c) = cells.get(i) else { break };
                let g0 = null_grid(NullModel::Readings, c.span_s, c.step_s, 0.0, 1.0, 1);
                let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * c.step_s);
                let scale = x_scale(&g0, w);
                let mi = (c.mix.min(1e15) * 1000.0).round() as u64;
                // A period well inside the band: 60 s, or a fifth of a
                // short take.
                let period = 60f64.min(c.span_s / 5.0).max(16.0 * c.step_s);
                let (mut named, mut caveats) = (0, 0);
                for d in 0..draws {
                    let seed = seed_of(&[
                        1,
                        c.span_s as u64,
                        (c.step_s * 100.0) as u64,
                        mi,
                        tag,
                        d as u64,
                    ]);
                    let mut g =
                        null_grid(NullModel::Timing, c.span_s, c.step_s, c.mix, scale, seed);
                    let det = detrend_as(&g, DETREND_S, slow_for(NullModel::Timing));
                    let ys: Vec<f64> = det.y.iter().copied().filter(|v| v.is_finite()).collect();
                    let rms = (ys.iter().map(|v| v * v).sum::<f64>() / ys.len() as f64).sqrt();
                    if std::env::var("FRESH_NOSINE").is_ok() {
                        let f = fit_of_draw(&spectrum(&det, w), w, c.step_s, scale);
                        let lim = levelling(NullModel::Timing, c.span_s, c.step_s, f.mix)
                            .and_then(|l| l.misfit_p99);
                        named += 1;
                        caveats += usize::from(lim.is_some_and(|l| f.misfit > l));
                        continue;
                    }
                    for (j, y) in g.y.iter_mut().enumerate() {
                        let t = j as f64 * c.step_s;
                        *y += 2.0 * rms * (std::f64::consts::TAU * t / period).sin();
                    }
                    let r = crate::longterm::analyse_timing(
                        &g,
                        &crate::longterm::LongConfig::default(),
                    );
                    if r.components.is_empty() {
                        continue;
                    }
                    named += 1;
                    caveats += usize::from(crate::longterm::timing_misfit(&r).is_some());
                }
                eprintln!(
                    "misfit {} {} {}: {draws} {named} {caveats}",
                    c.span_s, c.step_s, c.mix
                );
            });
        }
    });
}

/// Out of sample: fresh seeds (none the table was fitted on) through the
/// whole search, its own mix fitted and looked up, at every cell; the
/// count of maxima past p 0.01 and 0.001 against the expected. Run by
/// hand: `FRESH_DRAWS` draws a cell (default 100).
#[test]
#[ignore = "out-of-sample tail check; run by hand"]
fn fresh_seeds_hold_the_tail() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let draws: usize = std::env::var("FRESH_DRAWS").map_or(100, |s| s.parse().unwrap());
    let only: Option<String> = std::env::var("FRESH_MODEL").ok();
    // `FRESH_GIVEN`: each take levelled at its true mix, not its fitted one.
    let at_true = std::env::var("FRESH_GIVEN").is_ok();
    // `FRESH_MIX` keeps one drawn mix; `FRESH_TAG` draws an independent
    // batch of seeds.
    let only_mix: Option<f64> = std::env::var("FRESH_MIX").ok().map(|s| s.parse().unwrap());
    let tag: u64 = std::env::var("FRESH_TAG").map_or(7, |s| s.parse().unwrap());
    let mut cells: Vec<&Cell> = table()
        .cells
        .iter()
        .filter(|c| only.as_ref().is_none_or(|m| format!("{:?}", c.model) == *m))
        .filter(|c| only_mix.is_none_or(|x| c.mix == x))
        .collect();
    cells.sort_by(|a, b| (b.span_s / b.step_s).total_cmp(&(a.span_s / a.step_s)));
    let next = AtomicUsize::new(0);
    // Per model: maxima, uncalibrated, past 0.01, past 0.001.
    let totals = Mutex::new([[0usize; 5]; 2]);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|sc| {
        for _ in 0..workers {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(c) = cells.get(i) else { break };
                let g0 = null_grid(NullModel::Readings, c.span_s, c.step_s, 0.0, 1.0, 1);
                let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * c.step_s);
                let scale = x_scale(&g0, w);
                let mi = (c.mix.min(1e15) * 1000.0).round() as u64;
                let id = [
                    c.model as u64,
                    c.span_s as u64,
                    (c.step_s * 100.0) as u64,
                    mi,
                ];
                let mut n = [0usize; 5];
                let own = levelling(c.model, c.span_s, c.step_s, c.mix);
                for d in 0..draws {
                    let seed = seed_of(&[id[0], id[1], id[2], id[3], tag, d as u64]);
                    let g = null_grid(c.model, c.span_s, c.step_s, c.mix, scale, seed);
                    let det = detrend_as(&g, DETREND_S, slow_for(c.model));
                    let band = Band {
                        min_p: 3f64.max(8.0 * c.step_s),
                        max_p: reach(c.model, c.span_s / 3.0, w),
                        false_alarm: 0.01,
                        detrend_w: w,
                        model: c.model,
                        given: if at_true { own.as_ref() } else { None },
                    };
                    let s = search(&det, &band, &[]);
                    n[0] += 1;
                    if !s.null.calibrated {
                        n[1] += 1;
                        continue;
                    }
                    let sig = s
                        .tail
                        .significance(s.score.iter().copied().fold(0.0, f64::max));
                    n[2] += usize::from(sig > 2.0);
                    n[3] += usize::from(sig > 3.0);
                    n[4] += usize::from(sig > -(0.0003f64).log10());
                }
                eprintln!(
                    "cell {:?} {} {} {}: {} {} {} {} {}",
                    c.model, c.span_s, c.step_s, c.mix, n[0], n[1], n[2], n[3], n[4]
                );
                let mut t = totals.lock().unwrap();
                for (tk, nk) in t[c.model as usize].iter_mut().zip(n) {
                    *tk += nk;
                }
            });
        }
    });
    let t = totals.into_inner().unwrap();
    for (m, name) in [(0, "readings"), (1, "timing")] {
        let held = (t[m][0] - t[m][1]) as f64;
        eprintln!(
            "{name}: {} maxima, {} uncalibrated; past p 0.01 {} (expected {:.0}), past 0.001 {} (expected {:.1})",
            t[m][0],
            t[m][1],
            t[m][2],
            held * 0.01,
            t[m][3],
            held * 0.001
        );
    }
}

/// A bin's levelled power read as the exponential with its null tail:
/// as it is at shape 1 (and where the cell has no shapes), and at shapes
/// 0.5 and 2 the chi-square tails of 1 and 4 degrees of freedom, scaled.
#[test]
fn a_bins_power_reads_as_its_own_tail() {
    use crate::steadiness::chi2_tail;
    let lev = |k: Option<f64>| Levelling {
        mean: vec![Some(1.0); X_POINTS],
        shape: k.map_or_else(Vec::new, |k| vec![Some(k); X_POINTS]),
        misfit_p99: None,
        mu: 0.0,
        sigma: 1.0,
        xi: 0.0,
        log_trials: 0.0,
    };
    let x = x_at(10);
    for v in [0.1, 1.0, 5.0, 20.0] {
        assert_eq!(lev(None).as_exponential(x, v), v);
        assert_eq!(lev(Some(1.0)).as_exponential(x, v), v);
        let half = lev(Some(0.5)).as_exponential(x, v);
        assert!(
            (half + chi2_tail(v, 1).ln()).abs() < 1e-6 * half.max(1.0),
            "k 0.5 at {v}: {half}"
        );
        let two = lev(Some(2.0)).as_exponential(x, v);
        assert!(
            (two + chi2_tail(4.0 * v, 4).ln()).abs() < 1e-6 * two.max(1.0),
            "k 2 at {v}: {two}"
        );
    }
}

/// The plain trials count's tail is the old Bonferroni: significance
/// score − log10 N, threshold log10 N − log10 p.
#[test]
fn the_trials_tail_is_bonferroni() {
    for n in [1.0, 37.0, 4096.0] {
        let t = Tail::trials(n);
        for s in [0.5, 3.0, 9.25] {
            assert!((t.significance(s) - (s - f64::log10(n))).abs() < 1e-12);
        }
        for p in [0.01, 0.001] {
            assert!((t.score_at(p) - (f64::log10(n) - f64::log10(p))).abs() < 1e-12);
        }
    }
}

/// `score_at` and `significance` invert each other, with no shape and
/// with one.
#[test]
fn score_at_and_significance_invert() {
    for xi in [0.0, 0.08, 0.2] {
        let t = Tail {
            mu: 3.1,
            sigma: 0.52,
            xi,
        };
        for p in [0.05, 0.01, 0.001, 1e-6] {
            let s = t.score_at(p);
            assert!((t.significance(s) + p.log10()).abs() < 1e-9, "ξ {xi} p {p}");
        }
    }
}

/// A cell's levelling moved to its own trials count is its own tail;
/// to ten times as many, its maximum's location rises as ten draws'.
#[test]
fn a_levellings_tail_at_its_own_trials_is_its_own() {
    for xi in [0.0, 0.12] {
        let l = Levelling {
            mean: vec![Some(1.0); X_POINTS],
            shape: Vec::new(),
            misfit_p99: None,
            mu: 3.4,
            sigma: 0.47,
            xi,
            log_trials: 2.7,
        };
        let t = l.tail(10f64.powf(2.7));
        assert!((t.mu - 3.4).abs() < 1e-12 && (t.sigma - 0.47).abs() < 1e-12 && t.xi == xi);
        // The maximum of ten: P(max ≤ s)^10, so −ln P, the chance the
        // tail states, is ten times as large at every score.
        let ten = l.tail(10f64.powf(3.7));
        let s = t.score_at(0.01);
        assert!(
            (ten.significance(s) - (t.significance(s) - 1.0)).abs() < 1e-9,
            "ξ {xi}"
        );
    }
}

/// A step within 0.1% of the table's is the table's: real takes read a
/// few ppm off the nominal 2 s.
#[test]
fn a_step_a_few_ppm_off_finds_its_cells() {
    let on = levelling(NullModel::Readings, 1800.0, 2.0, 0.5).unwrap();
    let off = levelling(NullModel::Readings, 1800.0, 2.0000378, 0.5).unwrap();
    assert_eq!(on.mu, off.mu);
    assert!(levelling(NullModel::Readings, 1800.0, 2.01, 0.5).is_none());
}

/// The timing's crossover fit on the null itself: about none on white
/// noise, the pure walk's mark on a walk, and close to the crossover a
/// walk plus white noise was made with (the median of 21 draws).
/// A walk plus white sits inside the null's 99th percentile of misfit;
/// flicker timing (power falling as 1/f, a sum of relaxations a decade
/// apart) sits past it, which is when the caveat is given (#1218, #1219).
#[test]
fn flicker_timing_sits_past_the_nulls_misfit() {
    let (span, step) = (5400.0, 0.25);
    let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
    let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
    let scale = x_scale(&g0, w);
    let n = (span / step) as usize;
    let flicker = |seed: u64| -> Grid {
        let mut z = Rng::new(seed);
        let taus = [1.0, 10.0, 100.0, 1000.0, 10000.0];
        let mut x = [0.0; 5];
        let y = (0..n)
            .map(|_| {
                let mut v = 0.0;
                for (xi, tau) in x.iter_mut().zip(taus) {
                    let phi = (-step / tau).exp();
                    *xi = phi * *xi + (1.0 - phi * phi).sqrt() * z.normal();
                    v += *xi;
                }
                v
            })
            .collect();
        Grid { t0: 0.0, step, y }
    };
    let read = |g: &Grid| {
        let sp = spectrum(&detrend_as(g, DETREND_S, slow_for(NullModel::Timing)), w);
        let from = (2.0 / (sp.df * scale)).ceil() as usize;
        let f = timing_fit(&sp.power, sp.df, step, from, scale);
        let limit = levelling(NullModel::Timing, span, step, f.mix)
            .and_then(|l| l.misfit_p99)
            .expect("a covered take");
        (f.misfit, limit)
    };
    let mut past = 0;
    for d in 0..20u64 {
        let (m, limit) = read(&flicker(seed_of(&[11, d])));
        past += usize::from(m > limit);
    }
    assert!(past >= 18, "flicker past the null's misfit on {past} of 20");
    let mut walks = 0;
    for d in 0..100u64 {
        let g = null_grid(
            NullModel::Timing,
            span,
            step,
            100.0,
            scale,
            seed_of(&[12, d]),
        );
        let (m, limit) = read(&g);
        walks += usize::from(m > limit);
    }
    assert!(walks <= 5, "walk plus white past it on {walks} of 100");
}

#[test]
fn timing_mix_recovers_the_crossover() {
    let (span, step) = (7200.0, 0.25);
    let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
    let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
    let scale = x_scale(&g0, w);
    let fit = |mix: f64| {
        let mut v: Vec<f64> = (0..21)
            .map(|d| {
                let g = null_grid(NullModel::Timing, span, step, mix, scale, seed_of(&[9, d]));
                let sp = spectrum(&detrend_as(&g, DETREND_S, slow_for(NullModel::Timing)), w);
                let from = (2.0 / (sp.df * scale)).ceil() as usize;
                timing_mix(&sp.power, sp.df, step, from, scale)
            })
            .collect();
        v.sort_by(f64::total_cmp);
        v[10]
    };
    assert!(fit(0.0) < 1.0, "white: {}", fit(0.0));
    assert_eq!(fit(WALK_MIX), WALK_MIX);
    for mix in [30.0, 300.0, 1000.0] {
        let got = fit(mix);
        assert!((got / mix).ln().abs() < 0.25f64.ln_1p(), "{mix}: {got}");
    }
}

/// The table as the generator writes it: every model, span and step row
/// with its full mix list, a shape on 0 to 0.2, a positive scale, and
/// positive means (a truncated or hand-edited table fails here).
#[test]
fn the_null_table_has_the_generators_shape() {
    let t = table();
    for model in [NullModel::Readings, NullModel::Timing] {
        for (span, step) in rows(model) {
            let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
            let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
            let want = mixes(model, x_scale(&g0, w), step);
            let mut got: Vec<f64> = t
                .cells
                .iter()
                .filter(|c| c.model == model && c.span_s == span && c.step_s == step)
                .map(|c| c.mix)
                .collect();
            got.sort_by(f64::total_cmp);
            assert_eq!(got, want, "{model:?} {span} s at {step} s");
        }
    }
    for c in &t.cells {
        let at = format!("{:?} {} {} {}", c.model, c.span_s, c.step_s, c.mix);
        assert!(
            (0.0..=0.02 * XI_STEPS as f64).contains(&c.xi),
            "{at}: ξ {}",
            c.xi
        );
        assert!(
            c.sigma > 0.0 && c.mu.is_finite() && c.log_trials > 0.0,
            "{at}"
        );
        assert_eq!(c.mean.len(), X_POINTS, "{at}");
        assert!(c.mean.iter().flatten().all(|&m| m > 0.0), "{at}");
        assert!(c.mean.iter().flatten().count() >= 10, "{at}");
        assert_eq!(c.shape.len(), X_POINTS, "{at}");
        assert!(c.shape.iter().flatten().all(|&k| k > 0.0), "{at}");
    }
}

/// The culling the misfit is read with (#1299): a band holding a kept
/// harmonic is left out whole; a further harmonic's bins leave only their
/// band's median, and a band left with under half its bins is dropped.
/// On a flat spectrum (misfit 0) one band lifted 10⁶ times reads a misfit
/// unless the rule takes it out. Bands from bin 8: [8, 12), [12, 16), …
#[test]
fn the_misfit_culls_bands_and_bins_by_its_rules() {
    let read = |lift: &[usize], removed: &[f64], thinned: &[f64]| {
        let mut p = vec![1.0; 200];
        lift.iter().for_each(|&j| p[j] = 1e6);
        let cull = Cull {
            removed,
            thinned,
            half: 0.5,
        };
        timing_fit_culled(&p, 1.0, 1e-3, 8, 1.0, &cull).misfit
    };
    let lifted = [12, 13, 14, 15];
    assert!(read(&[], &[], &[]) < 1e-9);
    assert!(read(&lifted, &[], &[]) > 0.1);
    // A kept harmonic at bin 12 leaves its band out whole.
    assert!(read(&lifted, &[12.0], &[]) < 1e-9);
    // Two of four bins thinned: the band stays, on the other two.
    assert!(read(&[12, 13], &[], &[]) > 0.1);
    assert!(read(&[12, 13], &[], &[12.0, 13.0]) < 1e-9);
    // Three of four thinned: the band is dropped, not read on its last bin.
    assert!(read(&[15], &[], &[12.0, 13.0, 14.0]) < 1e-9);
    assert!(read(&[15], &[], &[]) < 1e-9);
}

/// Mix 3 read as white (a third of 30 min to 2 h takes) is levelled by
/// the white cell's mean, which must cover the walk power the sloped
/// detrend lifts near 0.7 of its window, below where the fit reads the
/// mix. Levelled by the white draws' own mean, 2,000 takes of 30 min
/// passed p 0.001 21 times (Norm's ruling (i): means and shapes read by
/// fitted mix). 1,000 fresh takes a length, each levelled as a take is,
/// must stay within the 99% bound of 1 expected (4).
#[test]
fn a_walk_read_as_white_holds_its_false_alarms() {
    let counts: Vec<(f64, usize)> = std::thread::scope(|sc| {
        let hs: Vec<_> = [1800.0, 3600.0, 7200.0]
            .into_iter()
            .map(|span| {
                sc.spawn(move || {
                    let step = 0.25;
                    let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
                    let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
                    let scale = x_scale(&g0, w);
                    let hits = (0..1000u64)
                        .filter(|&d| {
                            let seed =
                                seed_of(&[NullModel::Timing as u64, span as u64, 25, 3000, 41, d]);
                            let g = null_grid(NullModel::Timing, span, step, 3.0, scale, seed);
                            let det = detrend_as(&g, DETREND_S, slow_for(NullModel::Timing));
                            let band = Band {
                                min_p: 3f64.max(8.0 * step),
                                max_p: reach(NullModel::Timing, span / 3.0, w),
                                false_alarm: 0.01,
                                detrend_w: w,
                                model: NullModel::Timing,
                                given: None,
                            };
                            let s = search(&det, &band, &[]);
                            s.tail
                                .significance(s.score.iter().copied().fold(0.0, f64::max))
                                > 3.0
                        })
                        .count();
                    (span, hits)
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (span, hits) in counts {
        assert!(hits <= 4, "{span} s: {hits} of 1000 past p 0.001 (bound 4)");
    }
}
