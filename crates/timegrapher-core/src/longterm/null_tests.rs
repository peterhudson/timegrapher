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

/// The flicker shares drawn at each timing mix above white on a row of
/// `span` at `step` (#1219); one, none, for readings and white. The 2 h
/// row also draws 0.003 and 0.3: read between its neighbours there, the
/// mid-cell check fell under its band (Norm, as condition 3 names).
fn flickers(model: NullModel, span: f64, step: f64, mix: f64) -> Vec<f64> {
    match model {
        NullModel::Timing if mix > 0.0 && span == 7200.0 && step == 0.25 => {
            vec![0.0, 0.003, 0.01, 0.1, 0.3, FLICKER_ONLY]
        }
        NullModel::Timing if mix > 0.0 => vec![0.0, 0.01, 0.1, FLICKER_ONLY],
        _ => vec![0.0],
    }
}

/// A cell's seed parts: model, span, step and mix as before #1219, and
/// the flicker share only where there is one, so a cell with none draws
/// the seeds it always drew.
fn cell_id(model: NullModel, span: f64, step: f64, mix: f64, flicker: f64) -> Vec<u64> {
    let mi = (mix.min(1e15) * 1000.0).round() as u64;
    let mut id = vec![model as u64, span as u64, (step * 100.0) as u64, mi];
    if flicker > 0.0 {
        id.push(0xf1 ^ (flicker * 1e6).round() as u64);
    }
    id
}

/// A draw's seed: the cell's parts, then the pass and the draw.
fn draw_seed(id: &[u64], pass: u64, d: usize) -> u64 {
    let mut parts = id.to_vec();
    parts.extend([pass, d as u64]);
    seed_of(&parts)
}

/// One null series with no flicker ([`null_grid_f`]).
fn null_grid(model: NullModel, span: f64, step: f64, mix: f64, scale: f64, seed: u64) -> Grid {
    null_grid_f(model, span, step, mix, 0.0, scale, seed)
}

/// Flicker of the rate (#1219): `n` steps of a timing whose power is
/// `c`·u^1.5, u = 1/(4 sin²(πfΔ)), in the units in which a walk whose
/// steps have variance `a` has power `a`·u and white noise of variance
/// `b` has `b`. White noise shaped by √(u^1.5) through a Fourier
/// transform twice the length or more, its first `n` steps kept so the
/// transform's wrap does not join the ends; nothing at frequency 0.
fn flicker_fm(n: usize, c: f64, rng: &mut Rng) -> Vec<f64> {
    use realfft::RealFftPlanner;
    let l = (2 * n).next_power_of_two();
    let mut planner = RealFftPlanner::<f64>::new();
    let (fwd, inv) = (planner.plan_fft_forward(l), planner.plan_fft_inverse(l));
    let mut buf: Vec<f64> = (0..l).map(|_| rng.normal()).collect();
    let mut spec = fwd.make_output_vec();
    fwd.process(&mut buf, &mut spec).expect("forward");
    for (k, z) in spec.iter_mut().enumerate() {
        let gain = if k == 0 {
            0.0
        } else {
            let s = (std::f64::consts::PI * k as f64 / l as f64).sin();
            (4.0 * s * s).powf(-0.75)
        };
        *z *= gain;
    }
    // The Nyquist bin of a real transform is real.
    if let Some(z) = spec.last_mut() {
        z.im = 0.0;
    }
    let mut out = inv.make_output_vec();
    inv.process(&mut spec, &mut out).expect("inverse");
    out.truncate(n);
    let k = c.sqrt() / l as f64;
    out.iter().map(|v| v * k).collect()
}

/// Norm's condition 1 (#1219): the flicker generator's timing spectrum
/// falls as f^-3 over two decades, on seeds the table never uses.
#[test]
fn flicker_fm_timing_falls_as_the_cube_of_frequency() {
    use realfft::RealFftPlanner;
    let n = 1 << 14;
    let mut planner = RealFftPlanner::<f64>::new();
    let fwd = planner.plan_fft_forward(n);
    let mut power = vec![0.0; n / 2 + 1];
    let draws = 40;
    for d in 0..draws {
        let mut rng = Rng::new(seed_of(&[0x5107e, d]));
        let mut x = flicker_fm(n, 1.0, &mut rng);
        // A Hann window keeps a steep spectrum's leakage off its tail.
        for (i, v) in x.iter_mut().enumerate() {
            let w = (std::f64::consts::PI * i as f64 / n as f64).sin();
            *v *= w * w;
        }
        let mut spec = fwd.make_output_vec();
        fwd.process(&mut x, &mut spec).expect("forward");
        for (p, z) in power.iter_mut().zip(&spec) {
            *p += z.norm_sqr() / draws as f64;
        }
    }
    // Two decades: 1/1000 to 1/10 cycles a sample, fitted on log axes.
    let (lo, hi) = (n / 1000, n / 10);
    let pts: Vec<(f64, f64)> = (lo..=hi)
        .map(|k| ((k as f64 / n as f64).ln(), power[k].ln()))
        .collect();
    let m = pts.len() as f64;
    let (mx, my) = pts
        .iter()
        .fold((0.0, 0.0), |a, p| (a.0 + p.0 / m, a.1 + p.1 / m));
    let (sxy, sxx) = pts.iter().fold((0.0, 0.0), |a, p| {
        (a.0 + (p.0 - mx) * (p.1 - my), a.1 + (p.0 - mx).powi(2))
    });
    let slope = sxy / sxx;
    eprintln!("flicker FM timing slope over two decades: {slope:.3}");
    // The gain is (4 sin^2(pi f))^-0.75, so the exact slope bends from -3
    // by under 0.05 by f = 0.1.
    assert!((slope + 3.0).abs() < 0.1, "slope {slope}");
}

/// One null series: AR(1) at ρ `mix`, or a random walk (1 s^-½) plus
/// white noise with the red-white crossover at `mix / scale`, and, with
/// `flicker` above 0, flicker of the rate meeting the walk at `flicker`
/// times that crossover (the Nyquist frequency where there is no floor),
/// or alone with no walk at [`FLICKER_ONLY`] (#1219). The walk and white
/// draws are those of a series with no flicker; the flicker has its own.
fn null_grid_f(
    model: NullModel,
    span: f64,
    step: f64,
    mix: f64,
    flicker: f64,
    scale: f64,
    seed: u64,
) -> Grid {
    if model == NullModel::Timing && flicker > 0.0 && mix > 0.0 {
        let n = (span / step).round() as usize;
        let u_at = |f: f64| {
            let s = (std::f64::consts::PI * f * step).sin();
            1.0 / (4.0 * s * s)
        };
        let nyquist = 0.5 / step;
        let f_r = if mix >= WALK_MIX {
            nyquist
        } else {
            mix / scale
        };
        // The walk's step variance is `step`, as with no flicker.
        let (a, c) = if flicker >= FLICKER_ONLY {
            (0.0, step)
        } else {
            let u_k = u_at((flicker * f_r).min(0.999 * nyquist));
            (step, step / u_k.sqrt())
        };
        let u_r = u_at(f_r.min(nyquist));
        let b = if mix >= WALK_MIX {
            0.0
        } else {
            c * u_r.powf(1.5) + a * u_r
        };
        let mut rng = Rng::new(seed);
        let mut fl_rng = Rng::new(seed_of(&[seed, 0xf1]));
        let fl = flicker_fm(n, c, &mut fl_rng);
        let (sa, sb) = (a.sqrt(), b.sqrt());
        let mut x = 0.0;
        let y = fl
            .iter()
            .map(|&f| {
                x += sa * rng.normal();
                x + sb * rng.normal() + f
            })
            .collect();
        return Grid { t0: 0.0, step, y };
    }
    null_grid_plain(model, span, step, mix, scale, seed)
}

/// One null series: AR(1) at ρ `mix`, or a random walk (1 s^-½) plus
/// white noise with the crossover at `mix / scale`.
fn null_grid_plain(
    model: NullModel,
    span: f64,
    step: f64,
    mix: f64,
    scale: f64,
    seed: u64,
) -> Grid {
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
    #[serde(default)]
    flicker: f64,
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

/// Pairs written with NaN (as null) read back as NaN, not as an error
/// that would drop the cell from the cache.
fn nan_pairs<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<[f64; 2]>, D::Error> {
    let v: Vec<[Option<f64>; 2]> = Deserialize::deserialize(d)?;
    Ok(v.into_iter()
        .map(|p| p.map(|x| x.unwrap_or(f64::NAN)))
        .collect())
}

#[derive(Serialize, Deserialize)]
struct Measured {
    cell: Raw,
    fitted_mix: f64,
    smax: Vec<f64>,
    /// Each maximum's take's fitted mix and flicker (timing, as the
    /// search looks them up); NaN where the table missed it. Empty before
    /// the rebinning. JSON writes NaN as null, so null reads back as NaN.
    #[serde(default, deserialize_with = "nan_pairs")]
    fitted: Vec<[f64; 2]>,
    /// The drawn mix and flicker the rebinned tail was fitted to (its
    /// shape group).
    #[serde(default, skip_serializing)]
    tail_from: Option<[f64; 2]>,
    /// Timing: each mean-pass draw's fitted mix, flicker and misfit.
    #[serde(default)]
    misfit_draws: Vec<([f64; 2], f64)>,
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
fn null_mean(
    model: NullModel,
    span: f64,
    step: f64,
    mix: f64,
    flicker: f64,
    draws: usize,
) -> NullMean {
    let id = cell_id(model, span, step, mix, flicker);
    let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
    let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
    let scale = x_scale(&g0, w);
    let mut sum = vec![0.0; X_POINTS];
    let mut cnt = vec![0usize; X_POINTS];
    // Levelled power's count, sum and sum of squares.
    let mut lev_n = vec![0usize; X_POINTS];
    let mut lev_s = vec![0.0; X_POINTS];
    let mut lev_ss = vec![0.0; X_POINTS];
    let own = levelling(model, span, step, mix, flicker);
    let mut fitted = Vec::new();
    let mut misfits = Vec::new();
    let mut per_x = Vec::new();
    for d in 0..draws {
        let g = null_grid_f(model, span, step, mix, flicker, scale, draw_seed(&id, 1, d));
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
            NullModel::Readings => [sp.rho, 0.0],
            NullModel::Timing => {
                let t = fit_of_draw(&sp, w, step, scale);
                if t.misfit.is_finite() {
                    misfits.push(([t.mix, t.flicker], t.misfit));
                }
                per_x.push(([t.mix, t.flicker], std::mem::take(&mut here)));
                [t.mix, t.flicker]
            }
        };
        fitted.push(f[0]);
        if let Some(l) = levelling(model, span, step, f[0], f[1]).or_else(|| own.clone()) {
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
fn misfit_draws(
    span: f64,
    step: f64,
    mix: f64,
    flicker: f64,
    draws: usize,
) -> Vec<([f64; 2], f64)> {
    let id = cell_id(NullModel::Timing, span, step, mix, flicker);
    let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
    let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
    let scale = x_scale(&g0, w);
    (0..draws)
        .filter_map(|d| {
            let seed = draw_seed(&id, 1, d);
            let g = null_grid_f(NullModel::Timing, span, step, mix, flicker, scale, seed);
            let t = fit_of_draw(
                &spectrum(&detrend_as(&g, DETREND_S, slow_for(NullModel::Timing)), w),
                w,
                step,
                scale,
            );
            t.misfit
                .is_finite()
                .then_some(([t.mix, t.flicker], t.misfit))
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
    m.misfit_draws = misfit_draws(c.span_s, c.step_s, c.mix, c.flicker, n);
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
    /// Timing: each draw's fitted mix and flicker, and misfit.
    misfit_draws: Vec<([f64; 2], f64)>,
    /// Timing: each draw's fitted mix and whitened power by x point.
    per_x: Vec<PerX>,
}

/// One draw's fitted mix and flicker and, at each x point, the count, sum
/// and sum of squares of its whitened power.
type PerX = ([f64; 2], Vec<[f64; 3]>);

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
    flicker: f64,
    draws: usize,
) -> (Vec<([f64; 2], f64)>, usize) {
    let id = cell_id(model, span, step, mix, flicker);
    let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
    let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
    let scale = x_scale(&g0, w);
    let own = levelling(model, span, step, mix, flicker).expect("a table cell");
    let mut missed = 0;
    let smax = (0..draws)
        .map(|d| {
            let seed = draw_seed(&id, 2, d);
            let det = detrend_as(
                &null_grid_f(model, span, step, mix, flicker, scale, seed),
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
                wheels: &[],
            };
            let mut s = search(&det, &band(None), &[]);
            let fitted = if s.null.calibrated {
                [s.null.mix, s.null.flicker]
            } else {
                [f64::NAN; 2]
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

fn measure(
    model: NullModel,
    span: f64,
    step: f64,
    mix: f64,
    flicker: f64,
    draws: usize,
) -> Measured {
    let id = cell_id(model, span, step, mix, flicker);
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
    } = null_mean(model, span, step, mix, flicker, draws);
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
        let g = null_grid_f(model, span, step, mix, flicker, scale, draw_seed(&id, 2, d));
        let det = detrend_as(&g, DETREND_S, slow_for(model));
        let band = Band {
            min_p,
            max_p: reach(model, span / 3.0, w),
            false_alarm: 0.01,
            detrend_w: w,
            model,
            given: Some(&lev),
            wheels: &[],
        };
        let s = search(&det, &band, &[]);
        if !s.null.calibrated {
            let def: Vec<usize> = (0..X_POINTS).filter(|&k| mean[k].is_some()).collect();
            panic!("{model:?} {span} {step} {mix} {flicker}: band outside the mean; defined {def:?} scale {scale} min_p {min_p}");
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
            flicker,
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
    fn cell(&self, model: NullModel, span: f64, step: f64, mix: f64, flicker: f64) -> Measured {
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
        let kept = cache.as_ref().map(|d| match flicker > 0.0 {
            false => format!("{d}/{model:?}_{span}_{step}_{mix}_{draws}.json"),
            true => format!("{d}/{model:?}_{span}_{step}_{mix}_f{flicker}_{draws}.json"),
        });
        let old = kept.as_ref().and_then(|f| std::fs::read_to_string(f).ok());
        // A cached cell that doesn't read back stops the run: measuring it
        // again would silently drop a pass already made (#1219).
        let old = old.map(|t| {
            serde_json::from_str::<Measured>(&t)
                .unwrap_or_else(|e| panic!("{:?}: {e}", kept.as_deref().unwrap_or("")))
        });
        let old = match old {
            Some(mut m) if *perx && model == NullModel::Timing && m.per_x.is_empty() => {
                m.per_x = null_mean(model, span, step, mix, flicker, draws).per_x;
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
                let n = null_mean(model, span, step, mix, flicker, draws);
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
            // A cell already refitted (a run cut short) is kept.
            Some(mut m)
                if m.fitted.is_empty()
                    && refit
                        .as_ref()
                        .is_some_and(|r| r == "all" || format!("{model:?}") == *r) =>
            {
                let (taken, missed) = maxima_as_takes(model, span, step, mix, flicker, draws);
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
                let mut m = measure(model, span, step, mix, flicker, draws);
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
                for flicker in flickers(model, span, step, mix) {
                    jobs.push((model, span, step, mix, flicker));
                }
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
                        let Some(&(model, span, step, mix, flicker)) = jobs.get(i) else { break };
                        let m = run.cell(model, span, step, mix, flicker);
                        eprintln!(
                            "{model:?} span {span} step {step} mix {mix} flicker {flicker}: fitted {:.3} slope {:.3} loc0 {:.3}",
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
        let k = |m: &Measured| {
            (
                m.cell.model as u8,
                m.cell.step_s,
                m.cell.span_s,
                m.cell.flicker,
                m.cell.mix,
            )
        };
        k(a).partial_cmp(&k(b)).unwrap()
    });
    rebin(&mut done);
    write_table(&done, draws, run.misfit_n.max(draws));
}

/// Fits the shapes to `done`, checks the pooled tail and writes the table
/// with a note built from the passes that ran.
fn write_table(done: &[Measured], draws: usize, misfit_n: usize) {
    let cells = fit_shapes(done);
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
            "tail {:?} {} {} {} {}: {cn} {c1} {c01}",
            c.model, c.span_s, c.step_s, c.mix, c.flicker
        );
        (n, p1, p01) = (n + cn, p1 + c1, p01 + c01);
    }
    // The passes this table had, from what ran, not from fixed text.
    let timing: Vec<&Measured> = done
        .iter()
        .filter(|m| m.cell.model == NullModel::Timing)
        .collect();
    let passes: Vec<&str> = TIMING_PASSES
        .iter()
        .copied()
        .filter(|&p| match p {
            "refit" => !timing.is_empty() && timing.iter().all(|m| !m.fitted.is_empty()),
            "means_rebinned" => std::env::var("NULL_MEAN_REBIN").is_ok(),
            _ => std::env::var("NULL_REBIN").is_ok(),
        })
        .collect();
    let note = table_note(draws, misfit_n, [n, p1, p01], &passes);
    eprintln!("{note}");
    let mut text = format!(
        "{{\"note\": {},\n\"passes\": {},\n\"draws_mean\": {draws},\n\"draws_max\": {draws},\n\"cells\": [\n",
        serde_json::to_string(&note).unwrap(),
        serde_json::to_string(&passes).unwrap()
    );
    for (i, c) in cells.iter().enumerate() {
        text += &serde_json::to_string(c).unwrap();
        text += if i + 1 < cells.len() { ",\n" } else { "\n" };
    }
    text += "]}\n";
    // A table short of a pass may go to a scratch file (`NULL_OUT`), an
    // intermediate the next pass reads, but never over the shipped one.
    let path = std::env::var("NULL_OUT").unwrap_or_else(|_| {
        assert!(
            timing.is_empty() || passes.len() == TIMING_PASSES.len(),
            "a timing table without every pass ({TIMING_PASSES:?}; ran {passes:?}) is not installed"
        );
        concat!(env!("CARGO_MANIFEST_DIR"), "/data/period-null.json").to_string()
    });
    std::fs::write(path, text).unwrap();
}

/// The passes a timing table must have had before it ships (#1219): the
/// maxima refit as takes, the means and the tails read least favourable.
pub(super) const TIMING_PASSES: [&str; 3] = ["refit", "means_rebinned", "tails_rebinned"];

/// The table's note: how it was made, from the passes that ran
/// (`passes`, of [`TIMING_PASSES`]), and the pooled tail check of `tail`
/// (maxima, past p 0.01, past 0.001). A pass that didn't run is named as
/// missing, never described (Norm on #1219).
fn table_note(draws: usize, misfit_n: usize, tail: [usize; 3], passes: &[&str]) -> String {
    let [n, p1, p01] = tail;
    let ran = |p: &str| passes.contains(&p);
    let mut out = vec![
        "Generated by write_period_null_table (longterm/null_tests.rs), fixed seeds".to_string(),
        format!("{draws} draws a cell for the mean and as many for the maximum"),
        format!(
            "The maximum's generalised extreme value by maximum likelihood: location and scale for each cell (the scale's relative error about {:.0}%); the shape profiled over 0 to {:.1} in steps of 0.02, shared by each take length and step (readings) or each mix (timing)",
            78.0 / (draws as f64).sqrt(),
            0.02 * XI_STEPS as f64
        ),
        "The drawn mixes are white, 1 to 10^4 up to 0.35 of the sampling rate, and walk; above white, each is drawn with flicker of the rate at shares 0, 0.01, 0.1 and 1 of the red-white crossover (1: flicker alone, no walk), read between in log share, and linearly from none to 0.01 (#1219)".to_string(),
        "The drawn mixes reach both ends of the lookup's axis, white and walk, so no fitted mix lies past them".to_string(),
    ];
    if ran("refit") {
        out.push("Timing's maxima are each levelled as a take is, at the mix and flicker share its own search fitted".to_string());
    }
    if ran("tails_rebinned") {
        out.push(format!("Timing's location and scale are fitted to the maxima grouped by the mix and flicker share each take's search fitted, which is the coordinate the lookup reads, pooled from every drawn cell of that length and step. A cell that fewer than {MIN_BIN} draws read takes the {MIN_BIN} read nearest it"));
        out.push(format!("Each cell's tail is then the least favourable over the drawn cells (mix and flicker share) read as it or a neighbour on either axis (the lookup blends neighbours): fitted to the drawn cell, of those giving at least {MIN_SOURCE} draws, whose maxima most often pass the fit's p 0.001 and 0.0003 levels, refitted until that cell is the one fitted, and sharing its shape group. So the false-alarm chance holds for every drawn mix and share, not on average over them"));
    }
    if ran("means_rebinned") {
        out.push(format!("Timing's mean and gamma shape at each x are read least favourable the same way: the highest mean and the lowest shape over the drawn cells (of those with at least {MIN_SOURCE} draws there) read as the cell or a neighbour on either axis, since the fit reads the mix only at x of 2 and above"));
    }
    out.push(
        "Each bin's whitened power is scored by its own null spread (a gamma shape per x)"
            .to_string(),
    );
    out.push(format!(
        "Each timing cell also gives a misfit limit: the misfit is the rms in decades of the quarter-octave band powers from the walk, flicker and white fitted to them, and the limit is the 99th percentile of it ({misfit_n} draws a drawn mix){}",
        if ran("tails_rebinned") {
            format!(", the largest over the drawn cells (of those with at least {MIN_SOURCE} draws there) read as this cell or a neighbour")
        } else {
            String::new()
        }
    ));
    let missing: Vec<&str> = TIMING_PASSES.iter().copied().filter(|p| !ran(p)).collect();
    if !missing.is_empty() {
        out.push(format!(
            "Not for release: the timing passes {} did not run",
            missing.join(", ")
        ));
    }
    out.push(format!(
        "Pooled tail check: {p1} of {n} null maxima under p 0.01 (expected {:.0}), {p01} under 0.001 (expected {:.0}).",
        n as f64 * 0.01,
        n as f64 * 0.001
    ));
    out.join(". ")
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

/// A timing row's cells on their grid of mix and flicker (#1219): which
/// cell a fitted (mix, flicker) reads as, and which cells neighbour it.
struct RowGrid {
    mixes: Vec<f64>,
    flicks: Vec<f64>,
    /// Each cell's (mix, flicker) grid position, in the row's order.
    pos: Vec<(usize, usize)>,
}

impl RowGrid {
    fn new(done: &[Measured], idx: &[usize]) -> RowGrid {
        let mut mixes: Vec<f64> = idx
            .iter()
            .map(|&i| axis(NullModel::Timing, done[i].cell.mix))
            .collect();
        let mut flicks: Vec<f64> = idx
            .iter()
            .map(|&i| flicker_axis(done[i].cell.flicker))
            .collect();
        for v in [&mut mixes, &mut flicks] {
            v.sort_by(f64::total_cmp);
            v.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        }
        let find = |v: &[f64], x: f64| v.iter().position(|&y| (y - x).abs() < 1e-12).unwrap();
        let pos = idx
            .iter()
            .map(|&i| {
                let c = &done[i].cell;
                (
                    find(&mixes, axis(NullModel::Timing, c.mix)),
                    find(&flicks, flicker_axis(c.flicker)),
                )
            })
            .collect();
        RowGrid { mixes, flicks, pos }
    }

    /// The cell a fitted mix and flicker read as: the nearest drawn mix,
    /// and on it the nearest drawn flicker (white has only none).
    fn nearest(&self, f: [f64; 2]) -> usize {
        let near = |v: &[f64], x: f64| {
            (0..v.len())
                .min_by(|&a, &b| (v[a] - x).abs().total_cmp(&(v[b] - x).abs()))
                .unwrap()
        };
        let mi = near(&self.mixes, axis(NullModel::Timing, f[0]));
        let fi = near(&self.flicks, flicker_axis(f[1]));
        self.pos
            .iter()
            .position(|&p| p == (mi, fi))
            .or_else(|| self.pos.iter().position(|&p| p.0 == mi))
            .unwrap()
    }

    /// Whether cells `a` and `b` are the same or neighbours on the grid.
    fn near(&self, a: usize, b: usize) -> bool {
        let (p, q) = (self.pos[a], self.pos[b]);
        p.0.abs_diff(q.0) <= 1 && p.1.abs_diff(q.1) <= 1
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
        let grid = RowGrid::new(done, &idx);
        let mut all: Vec<(usize, f64, usize)> = Vec::new();
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
                    .filter(|(f, _)| f[0].is_finite() && f[1].is_finite())
                    .map(|(&f, &x)| (grid.nearest(f), x, src)),
            );
        }
        // How far apart two cells sit on the grid, in steps.
        let apart = |a: usize, b: usize| {
            let (p, q) = (grid.pos[a], grid.pos[b]);
            p.0.abs_diff(q.0) + p.1.abs_diff(q.1)
        };
        for (k, &i) in idx.iter().enumerate() {
            // The draws read as this cell or a neighbour on either axis:
            // the lookup blends a cell with its neighbours, so their drawn
            // mixes and flickers reach its tail too.
            let mut read: Vec<(f64, f64, usize)> = all
                .iter()
                .filter(|d| grid.near(d.0, k))
                .map(|d| (0.0, d.1, d.2))
                .collect();
            // A cell the fit seldom reads at this length (short takes read
            // most walks near white) takes the draws read nearest it.
            let own = read.len();
            if own < MIN_BIN {
                let mut by: Vec<&(usize, f64, usize)> = all.iter().collect();
                by.sort_by_key(|d| apart(d.0, k));
                read = by.iter().take(MIN_BIN).map(|d| (0.0, d.1, d.2)).collect();
            }
            let (b, worst) = least_favourable(&read, idx.len());
            let worst = worst.map(|src| [done[idx[src]].cell.mix, done[idx[src]].cell.flicker]);
            let m = &mut done[i];
            eprintln!(
                "rebin Timing {span} {step} {} {}: {own} maxima read there; least favourable drawn {worst:?}, {} maxima",
                m.cell.mix,
                m.cell.flicker,
                b.len()
            );
            (m.mu, m.sigma) = gumbel_fit(&b);
            m.smax = b;
            m.tail_from = worst;
        }
        rebin_misfit(done, &idx, &grid);
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
        let grid = RowGrid::new(done, &idx);
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
                    .filter(|d| d.0[0].is_finite() && d.0[1].is_finite())
                    .map(|d| (grid.nearest(d.0), d.1.as_slice()))
                    .collect()
            })
            .collect();
        let mut out: Vec<MeanShape> = Vec::new();
        for k in 0..idx.len() {
            let mut mean = vec![None::<f64>; X_POINTS];
            let mut shape = vec![None::<f64>; X_POINTS];
            for r in &read {
                let mine: Vec<_> = r.iter().filter(|d| grid.near(d.0, k)).collect();
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
            eprintln!(
                "means Timing {span} {step} {} {}: read by fitted mix and flicker",
                c.mix, c.flicker
            );
        }
    }
}

/// Each timing cell's misfit limit as the lookup meets it: the largest
/// 99th percentile over the drawn mixes (of those with [`MIN_SOURCE`]
/// draws there) read as this cell or either neighbour (Norm on #1245).
/// A cell no drawn mix reads keeps its own.
fn rebin_misfit(done: &mut [Measured], idx: &[usize], grid: &RowGrid) {
    let read: Vec<Vec<(usize, f64)>> = idx
        .iter()
        .map(|&i| {
            done[i]
                .misfit_draws
                .iter()
                .filter(|d| d.0[0].is_finite() && d.0[1].is_finite())
                .map(|&(f, m)| (grid.nearest(f), m))
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
                    .filter(|d| grid.near(d.0, k))
                    .map(|d| d.1)
                    .collect::<Vec<f64>>()
            })
            .filter(|x| x.len() >= MIN_SOURCE)
            .map(|x| p99(x.into_iter()))
            .fold(f64::NAN, f64::max);
        let c = &mut done[i].cell;
        if worst.is_finite() {
            eprintln!(
                "misfit Timing {} {} {} {}: own {:?}, limit {worst}",
                c.span_s, c.step_s, c.mix, c.flicker, c.misfit_p99
            );
            c.misfit_p99 = Some(worst);
        }
    }
}

/// A shape group: model, span, step (hundredths of a second), mix, and
/// flicker (millionths).
type GroupKey = (u8, u64, u64, u64, u64);

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
                let [mix, flicker] = m.tail_from.unwrap_or([c.mix, c.flicker]);
                (
                    1,
                    0,
                    0,
                    mix.min(1e15) as u64,
                    (flicker * 1e6).round() as u64,
                )
            }
            _ => (
                c.model as u8,
                c.span_s as u64,
                (c.step_s * 100.0) as u64,
                0,
                0,
            ),
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
                flicker: c.flicker,
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
            levelling(model, span, step, mix, 0.0).is_none(),
            "{model:?} {span} s at {step} s, mix {mix}"
        );
    }
    // Each side's own edge cells are inside.
    assert!(levelling(Readings, 1199.0, 2.0, 0.3, 0.0).is_some());
    assert!(levelling(Readings, 1200.0, 2.0, 0.3, 0.0).is_some());
    assert!(levelling(Readings, 1800.0, 2.0, 0.99, 0.0).is_some());
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
    let on = levelling(Readings, 1800.0, 2.0, 0.5, 0.0).unwrap();
    let c = cell(1800.0, 0.5);
    assert_eq!(
        (on.mu, on.sigma, on.xi, on.log_trials),
        (c.mu, c.sigma, c.xi, c.log_trials)
    );
    assert_eq!(on.mean, c.mean);
    assert_eq!(on.shape, c.shape);
    // Halfway in log span between 1800 and 2700 s.
    let (a, b) = (cell(1800.0, 0.5), cell(2700.0, 0.5));
    let mid = levelling(Readings, (1800.0f64 * 2700.0).sqrt(), 2.0, 0.5, 0.0).unwrap();
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
    let q = levelling(Readings, 1800.0, 2.0, 0.55, 0.0).unwrap();
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
    // `FRESH_DONE`: each cell's line is added to it and cells already in
    // it skipped, so a run cut short carries on.
    let done_file: Option<String> = std::env::var("FRESH_DONE").ok();
    let key = |c: &Cell| format!("misfit {} {} {} {}:", c.span_s, c.step_s, c.mix, c.flicker);
    if let Some(f) = &done_file {
        let done = std::fs::read_to_string(f).unwrap_or_default();
        cells.retain(|c| !done.lines().any(|l| l.starts_with(&key(c))));
    }
    let lines = std::sync::Mutex::new(());
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
                    // The flicker share's own seeds where there is one
                    // (#1219); none draws the seeds it always drew.
                    let mut parts = vec![1, c.span_s as u64, (c.step_s * 100.0) as u64, mi];
                    if c.flicker > 0.0 {
                        parts.push((c.flicker.min(1e15) * 1e6).round() as u64);
                    }
                    parts.extend([tag, d as u64]);
                    let seed = seed_of(&parts);
                    let mut g = null_grid_f(
                        NullModel::Timing,
                        c.span_s,
                        c.step_s,
                        c.mix,
                        c.flicker,
                        scale,
                        seed,
                    );
                    let det = detrend_as(&g, DETREND_S, slow_for(NullModel::Timing));
                    let ys: Vec<f64> = det.y.iter().copied().filter(|v| v.is_finite()).collect();
                    let rms = (ys.iter().map(|v| v * v).sum::<f64>() / ys.len() as f64).sqrt();
                    if std::env::var("FRESH_NOSINE").is_ok() {
                        let f = fit_of_draw(&spectrum(&det, w), w, c.step_s, scale);
                        let lim =
                            levelling(NullModel::Timing, c.span_s, c.step_s, f.mix, f.flicker)
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
                let line = format!("{} {draws} {named} {caveats}", key(c));
                eprintln!("{line}");
                if let Some(f) = &done_file {
                    use std::io::Write;
                    let _l = lines.lock().unwrap();
                    let mut o = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(f)
                        .unwrap();
                    writeln!(o, "{line}").unwrap();
                }
            });
        }
    });
}

/// One take drawn on fresh seeds through the whole search, its own mix and
/// flicker fitted and looked up (or, `at_true`, levelled at the drawn
/// ones), `draws` times: [maxima, uncalibrated, past p 0.01, past 0.001,
/// past 0.0003].
#[allow(clippy::too_many_arguments)]
fn fresh_counts(
    model: NullModel,
    span: f64,
    step: f64,
    mix: f64,
    flicker: f64,
    draws: usize,
    tag: u64,
    at_true: bool,
) -> [usize; 5] {
    let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
    let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
    let scale = x_scale(&g0, w);
    let id = cell_id(model, span, step, mix, flicker);
    let mut n = [0usize; 5];
    let own = levelling(model, span, step, mix, flicker);
    for d in 0..draws {
        let mut parts = id.clone();
        parts.extend([tag, d as u64]);
        let g = null_grid_f(model, span, step, mix, flicker, scale, seed_of(&parts));
        let det = detrend_as(&g, DETREND_S, slow_for(model));
        let band = Band {
            min_p: 3f64.max(8.0 * step),
            max_p: reach(model, span / 3.0, w),
            false_alarm: 0.01,
            detrend_w: w,
            model,
            given: if at_true { own.as_ref() } else { None },
            wheels: &[],
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
    n
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
    // `FRESH_DONE`: a file each cell's line is added to, and cells
    // already in it skipped, so a run cut short carries on.
    let done_file: Option<String> = std::env::var("FRESH_DONE").ok();
    let key = |c: &Cell| {
        format!(
            "cell {:?} {} {} {} {}:",
            c.model, c.span_s, c.step_s, c.mix, c.flicker
        )
    };
    if let Some(f) = &done_file {
        let done = std::fs::read_to_string(f).unwrap_or_default();
        cells.retain(|c| !done.lines().any(|l| l.starts_with(&key(c))));
    }
    let next = AtomicUsize::new(0);
    // Per model: maxima, uncalibrated, past 0.01, past 0.001.
    let totals = Mutex::new([[0usize; 5]; 2]);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|sc| {
        for _ in 0..workers {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(c) = cells.get(i) else { break };
                let n = fresh_counts(
                    c.model, c.span_s, c.step_s, c.mix, c.flicker, draws, tag, at_true,
                );
                eprintln!(
                    "cell {:?} {} {} {} {}: {} {} {} {} {}",
                    c.model, c.span_s, c.step_s, c.mix, c.flicker, n[0], n[1], n[2], n[3], n[4]
                );
                let mut t = totals.lock().unwrap();
                if let Some(f) = &done_file {
                    use std::io::Write;
                    let mut o = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(f)
                        .unwrap();
                    writeln!(o, "{} {} {} {} {} {}", key(c), n[0], n[1], n[2], n[3], n[4]).unwrap();
                }
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

/// The two-sided band at level `alpha` for a binomial count of `n` draws
/// at chance `p`: the least and the most counts whose tails each hold at
/// least `alpha`/2.
fn binomial_band(n: usize, p: f64, alpha: f64) -> (usize, usize) {
    let mut pmf = vec![0.0; n + 1];
    let ln = |k: usize| (1..=k).map(|i| (i as f64).ln()).sum::<f64>();
    for (k, v) in pmf.iter_mut().enumerate() {
        *v =
            (ln(n) - ln(k) - ln(n - k) + k as f64 * p.ln() + (n - k) as f64 * (1.0 - p).ln()).exp();
    }
    let (mut lo, mut acc) = (0, 0.0);
    while acc + pmf[lo] < alpha / 2.0 {
        acc += pmf[lo];
        lo += 1;
    }
    let (mut hi, mut acc) = (n, 0.0);
    while acc + pmf[hi] < alpha / 2.0 {
        acc += pmf[hi];
        hi -= 1;
    }
    (lo, hi)
}

/// Norm's condition 3 for the thinned flicker grid (#1219): a flicker
/// share between the drawn ones (0.003, 0.03, 0.3), read off the table by
/// interpolation, holds its false alarms against direct simulation on
/// fresh seeds, at every drawn timing mix above white on three rows (the
/// shortest take, 2 h, 96 h), and at 0.03 on the YM42's 65 h, which reads
/// between rows. Each share and row: the count past p 0.01 over the
/// mixes at or under the binomial band's upper edge at the 1% level; under
/// its lower edge is printed as power lost (Peter, 16 Nov). A share over
/// the upper edge goes back into the grid. Run once, by hand: `MID_DRAWS`
/// draws a mix (default 100), `MID_TAG` its seeds.
#[test]
#[ignore = "mid-cell check on fresh seeds; run by hand once"]
fn flicker_between_cells_holds_the_tail() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let draws: usize = std::env::var("MID_DRAWS").map_or(100, |s| s.parse().unwrap());
    // `MID_TAG`: a fresh batch of seeds for a rerun (0x31d the first).
    let tag: u64 = std::env::var("MID_TAG").map_or(0x31d, |s| s.parse().unwrap());
    let model = NullModel::Timing;
    let checks: [(f64, f64, &[f64]); 4] = [
        (120.0, 0.25, &[0.003, 0.03, 0.3]),
        // 2 h draws 0.003 and 0.3 too (Norm): between those, in log share.
        (7200.0, 0.25, &[0.0055, 0.03, 0.17, 0.55]),
        (345_600.0, 0.5, &[0.003, 0.03, 0.3]),
        (234_000.0, 0.5, &[0.03]),
    ];
    let mut jobs = Vec::new();
    for (span, step, shares) in checks {
        // The mixes of the row drawn nearest it, as the generator draws them.
        let near = rows(model)
            .into_iter()
            .filter(|r| r.1 == step)
            .min_by(|a, b| {
                (a.0.ln() - span.ln())
                    .abs()
                    .total_cmp(&(b.0.ln() - span.ln()).abs())
            })
            .unwrap();
        let g = null_grid(NullModel::Readings, near.0, step, 0.0, 1.0, 1);
        let w = detrend_window(&g, DETREND_S).map(|w| w as f64 * step);
        for mix in mixes(model, x_scale(&g, w), step)
            .into_iter()
            .filter(|&m| m > 0.0)
        {
            for &v in shares {
                jobs.push((span, step, mix, v));
            }
        }
    }
    jobs.sort_by(|a, b| (b.0 / b.1).total_cmp(&(a.0 / a.1)));
    let next = AtomicUsize::new(0);
    let counts = Mutex::new(std::collections::BTreeMap::<(u64, u64), [usize; 5]>::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|sc| {
        for _ in 0..workers {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(&(span, step, mix, v)) = jobs.get(i) else {
                    break;
                };
                let n = fresh_counts(model, span, step, mix, v, draws, tag, false);
                eprintln!("mid {span} {step} mix {mix} flicker {v}: {n:?}");
                let key = (span as u64, (v * 1e6).round() as u64);
                let mut c = counts.lock().unwrap();
                let t = c.entry(key).or_default();
                for (tk, nk) in t.iter_mut().zip(n) {
                    *tk += nk;
                }
            });
        }
    });
    let mut failed = Vec::new();
    for ((span, v), t) in counts.into_inner().unwrap() {
        let held = t[0] - t[1];
        let (lo, hi) = binomial_band(held, 0.01, 0.01);
        // Only the upper edge guards false alarms. The least-favourable
        // tails run below nominal by design, so a count under the lower
        // edge is power lost, said, not failed (Peter, 16 Nov, #1219).
        let ok = t[2] <= hi;
        eprintln!(
            "span {span} flicker {}: {} held of {}; past p 0.01 {} (band {lo} to {hi}), past 0.001 {}{}",
            v as f64 / 1e6,
            held,
            t[0],
            t[2],
            t[3],
            if !ok {
                "  FAILS"
            } else if t[2] < lo {
                "  under the band: power, not false alarms"
            } else {
                ""
            }
        );
        if !ok {
            failed.push((span, v as f64 / 1e6));
        }
    }
    assert!(
        failed.is_empty(),
        "shares to add back to the grid: {failed:?}"
    );
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
    let on = levelling(NullModel::Readings, 1800.0, 2.0, 0.5, 0.0).unwrap();
    let off = levelling(NullModel::Readings, 1800.0, 2.0000378, 0.5, 0.0).unwrap();
    assert_eq!(on.mu, off.mu);
    assert!(levelling(NullModel::Readings, 1800.0, 2.01, 0.5, 0.0).is_none());
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
        let limit = levelling(NullModel::Timing, span, step, f.mix, f.flicker)
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
    eprintln!("misfit: flicker past on {past} of 20, walk plus white on {walks} of 100");
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
/// with its full mix list and, for timing, each mix's flicker shares, a shape on 0 to 0.2, a positive scale, and
/// positive means (a truncated or hand-edited table fails here).
#[test]
fn the_null_table_has_the_generators_shape() {
    let t = table();
    for model in [NullModel::Readings, NullModel::Timing] {
        for (span, step) in rows(model) {
            let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
            let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
            // Every drawn mix with each of its flicker shares (#1219).
            let want: Vec<(f64, f64)> = mixes(model, x_scale(&g0, w), step)
                .into_iter()
                .flat_map(|m| {
                    flickers(model, span, step, m)
                        .into_iter()
                        .map(move |f| (m, f))
                })
                .collect();
            let mut got: Vec<(f64, f64)> = t
                .cells
                .iter()
                .filter(|c| c.model == model && c.span_s == span && c.step_s == step)
                .map(|c| (c.mix, c.flicker))
                .collect();
            got.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
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
    let step = 0.25;
    // Each take length's scale and window, made once; then every draw of
    // every length over the machine's cores, the longest first.
    let spans: Vec<(f64, Option<f64>, f64)> = [1800.0, 3600.0, 7200.0]
        .into_iter()
        .map(|span| {
            let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
            let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
            (span, w, x_scale(&g0, w))
        })
        .collect();
    let draws: Vec<(f64, Option<f64>, f64, u64)> = spans
        .iter()
        .rev()
        .flat_map(|&(span, w, scale)| (0..1000u64).map(move |d| (span, w, scale, d)))
        .collect();
    let hit = crate::cores::on_every_core(&draws, |&(span, w, scale, d)| {
        let seed = seed_of(&[NullModel::Timing as u64, span as u64, 25, 3000, 41, d]);
        let g = null_grid(NullModel::Timing, span, step, 3.0, scale, seed);
        let det = detrend_as(&g, DETREND_S, slow_for(NullModel::Timing));
        let band = Band {
            min_p: 3f64.max(8.0 * step),
            max_p: reach(NullModel::Timing, span / 3.0, w),
            false_alarm: 0.01,
            detrend_w: w,
            model: NullModel::Timing,
            given: None,
            wheels: &[],
        };
        let s = search(&det, &band, &[]);
        s.tail
            .significance(s.score.iter().copied().fold(0.0, f64::max))
            > 3.0
    });
    for &(span, _, _) in &spans {
        let hits = draws
            .iter()
            .zip(&hit)
            .filter(|(d, &h)| d.0 == span && h)
            .count();
        assert!(hits <= 4, "{span} s: {hits} of 1000 past p 0.001 (bound 4)");
    }
}

/// The shipped table had every timing pass (Norm on #1219): a table whose
/// note claims a least-favourable step that never ran is refused here,
/// not found later on fresh seeds.
#[test]
fn the_shipped_timing_table_had_every_pass() {
    let t = table();
    if t.cells.iter().any(|c| c.model == NullModel::Timing) {
        for p in TIMING_PASSES {
            assert!(
                t.passes.iter().any(|q| q == p),
                "period-null.json lacks {p}: {:?}",
                t.passes
            );
        }
        assert!(!t.note.contains("Not for release"), "{}", t.note);
    }
}

/// Timing power, a report for #1219 with no bar (Norm): on an 8 h take
/// at 0.25 s, walk plus white (mix 100, no flicker) and YM42-like (mix
/// 1000, flicker share 0.01), 100 noise draws each with a sine at 300 s
/// and at 1800 s added, swept in amplitude (in the noise's own first
/// difference spread); the smallest amplitude whose period is named
/// (the best score within 5% of it past p 0.01) in at least half the
/// draws. `POWER_DUMP` writes the noise draws to that folder and
/// `POWER_LOAD` reads them back, so the same draws can be read on
/// another table. Run by hand.
#[test]
#[ignore = "power report; run by hand"]
fn timing_power_floor() {
    let (span, step) = (28800.0, 0.25);
    let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
    let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
    let scale = x_scale(&g0, w);
    let draws = 100;
    let ks: Vec<f64> = (-16..=8).map(|i| 10f64.powf(i as f64 / 4.0)).collect();
    for (case, (mix, share)) in [(100.0, 0.0), (1000.0, 0.01)].into_iter().enumerate() {
        let noise: Vec<Vec<f64>> = (0..draws)
            .map(|d| {
                let file = |dir: &str| format!("{dir}/power_{case}_{d}.bin");
                if let Ok(dir) = std::env::var("POWER_LOAD") {
                    let b = std::fs::read(file(&dir)).unwrap();
                    return b
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|c| f64::from_le_bytes(*c))
                        .collect();
                }
                let g = null_grid_f(
                    NullModel::Timing,
                    span,
                    step,
                    mix,
                    share,
                    scale,
                    seed_of(&[31, case as u64, d as u64]),
                );
                if let Ok(dir) = std::env::var("POWER_DUMP") {
                    let b: Vec<u8> = g.y.iter().flat_map(|v| v.to_le_bytes()).collect();
                    std::fs::write(file(&dir), b).unwrap();
                }
                g.y
            })
            .collect();
        for period in [300.0, 1800.0] {
            let named = |k: f64| {
                std::thread::scope(|sc| {
                    let hs: Vec<_> = noise
                        .chunks(draws / 4)
                        .enumerate()
                        .map(|(c, part)| {
                            sc.spawn(move || {
                                part.iter()
                                    .enumerate()
                                    .filter(|(i, y)| {
                                        let d = c * (draws / 4) + i;
                                        let sd = (y
                                            .windows(2)
                                            .map(|p| (p[1] - p[0]).powi(2))
                                            .sum::<f64>()
                                            / (y.len() - 1) as f64
                                            / 2.0)
                                            .sqrt();
                                        let phase = d as f64 * 0.7;
                                        let y: Vec<f64> = y
                                            .iter()
                                            .enumerate()
                                            .map(|(j, v)| {
                                                let t = j as f64 * step;
                                                v + k
                                                    * sd
                                                    * (std::f64::consts::TAU * t / period + phase)
                                                        .sin()
                                            })
                                            .collect();
                                        let g = Grid { t0: 0.0, step, y };
                                        let det =
                                            detrend_as(&g, DETREND_S, slow_for(NullModel::Timing));
                                        let band = Band {
                                            min_p: 3f64.max(8.0 * step),
                                            max_p: reach(NullModel::Timing, span / 3.0, w),
                                            false_alarm: 0.01,
                                            detrend_w: w,
                                            model: NullModel::Timing,
                                            given: None,
                                            wheels: &[],
                                        };
                                        let s = search(&det, &band, &[]);
                                        let best = s
                                            .period_s
                                            .iter()
                                            .zip(&s.score)
                                            .filter(|(p, _)| ((*p / period) - 1.0).abs() < 0.05)
                                            .map(|(_, &sc)| sc)
                                            .fold(0.0, f64::max);
                                        s.null.calibrated && s.tail.significance(best) > 2.0
                                    })
                                    .count()
                            })
                        })
                        .collect();
                    hs.into_iter().map(|h| h.join().unwrap()).sum::<usize>()
                })
            };
            let counts: Vec<(f64, usize)> = ks.iter().map(|&k| (k, named(k))).collect();
            let floor = counts.iter().find(|c| 2 * c.1 >= draws).map(|c| c.0);
            eprintln!("POWER mix {mix} share {share} period {period}: floor {floor:?}; {counts:?}");
        }
    }
}

/// The fitted flicker share recovers the drawn one (#1219), as
/// [`timing_mix_recovers_the_crossover`] does for the mix: the median of
/// 21 draws on 2 h at mix 30, where the flicker, the walk and the floor
/// all show in the bands. No flicker reads as none, and flicker alone
/// (share 1) reads as flicker only.
#[test]
fn timing_flicker_recovers_the_share() {
    let (span, step, mix) = (7200.0, 0.25, 30.0);
    let g0 = null_grid(NullModel::Readings, span, step, 0.0, 1.0, 1);
    let w = detrend_window(&g0, DETREND_S).map(|w| w as f64 * step);
    let scale = x_scale(&g0, w);
    let fit = |share: f64| {
        let mut v: Vec<f64> = (0..21)
            .map(|d| {
                let g = null_grid_f(
                    NullModel::Timing,
                    span,
                    step,
                    mix,
                    share,
                    scale,
                    seed_of(&[9, d]),
                );
                let sp = spectrum(&detrend_as(&g, DETREND_S, slow_for(NullModel::Timing)), w);
                let from = (2.0 / (sp.df * scale)).ceil() as usize;
                timing_fit(&sp.power, sp.df, step, from, scale).flicker
            })
            .collect();
        v.sort_by(f64::total_cmp);
        v[10]
    };
    assert_eq!(fit(0.0), 0.0);
    assert_eq!(fit(FLICKER_ONLY), FLICKER_ONLY);
    for share in [0.01, 0.1] {
        let got = fit(share);
        assert!((got / share).ln().abs() < 2f64.ln(), "{share}: {got}");
    }
}

/// A timing lookup on a flicker cell returns that cell, and between two
/// shares it is the blend of the cells either side on the flicker axis:
/// in log share from 0.01 up, linear in the share from none to 0.01.
#[test]
fn a_timing_lookup_between_shares_interpolates() {
    use NullModel::Timing;
    let (span, step, mix) = (1800.0, 0.25, 100.0);
    let cell = |flicker: f64| {
        table()
            .cells
            .iter()
            .find(|c| {
                c.model == Timing
                    && c.span_s == span
                    && c.step_s == step
                    && c.mix == mix
                    && c.flicker == flicker
            })
            .unwrap_or_else(|| panic!("no cell at share {flicker}"))
    };
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9 * (1.0 + b.abs());
    let check = |flicker: f64, a: &Cell, b: &Cell, t: f64| {
        let l = levelling(Timing, span, step, mix, flicker).unwrap();
        let at = |x: f64, y: f64| x + (y - x) * t;
        assert!(
            close(l.mu, at(a.mu, b.mu)),
            "{flicker}: mu {} {} {}",
            l.mu,
            a.mu,
            b.mu
        );
        assert!(close(l.sigma, at(a.sigma, b.sigma)), "{flicker}: sigma");
        assert!(close(l.xi, at(a.xi, b.xi)), "{flicker}: xi");
        for k in 0..X_POINTS {
            match (a.mean[k], b.mean[k], l.mean[k]) {
                (Some(x), Some(y), Some(m)) => {
                    assert!(close(m, at(x, y)), "{flicker}: mean at {k}")
                }
                (_, _, m) => assert!(m.is_none(), "{flicker}: mean at {k}"),
            }
        }
    };
    let (none, low, mid) = (cell(0.0), cell(0.01), cell(0.1));
    assert!(none.mu != low.mu && low.mu != mid.mu, "the cells differ");
    check(0.01, low, low, 0.0);
    check(0.1, mid, mid, 0.0);
    // Halfway in log share between 0.01 and 0.1.
    check((0.01f64 * 0.1).sqrt(), low, mid, 0.5);
    // A quarter of the way from none to 0.01, linear in the share.
    check(0.0025, none, low, 0.25);
}
