# Precision estimates and period-search assumptions

Timegrapher reports short-window precision estimates and searches for periodic
variation. Both use statistical reference models. Neither is a universal error
bound, a probability that a watch is faulty, or a substitute for checking the
recording and measurement context. This page describes the existing calculations;
it does not change thresholds, estimators or reading eligibility.

## What the ± estimates

For time-ordered window values `x[0] … x[n−1]`,
[`amplitude::standard_error`](../crates/timegrapher-core/src/amplitude.rs) computes:

```text
d[i] = x[i+1] − x[i]
s = 1.4826 × median(|d − median(d)|) / sqrt(2)
SE = 1.2533 × s / sqrt(n)
```

Fewer than three values gives no estimate. The constants are Gaussian reference
factors: the MAD estimates a standard deviation, independent successive noise
values have difference variance twice their individual variance, and the median's
asymptotic standard error is about `1.2533 × sigma / sqrt(n)`. The function is
used for different reading units, despite living in the amplitude module.

Amplitude uses the available means of 2 s amplitude windows; unlock beat error
uses the available unlock-error values. Rate uses every other available 10 s
window, then filters non-finite rates before applying the same function
([`confidence`](../crates/timegrapher-core/src/confidence.rs)). Taking every other
window removes ordinary 5 s overlap. It does **not** establish independence of
the readings: the watch, stand or input can produce serial correlation across
non-overlapping windows. Missing windows also affect which observations are
adjacent in the input to the estimator.

For the live averaging-time display,
[`measure::rate_se_over`](../crates/timegrapher-core/src/measure.rs) further uses:

```text
m = ceil(number of supplied rate windows / 2)
a = clamp(average_s / 10, 1, m)
displayed SE = rate_se(rate_windows) × sqrt(m / a)
```

Here `m` counts supplied windows before the finite-rate filter; it is not a
measured effective sample size. The live rate-window history reaches back to a
known break or gap, up to five minutes. This scaling is a window-scatter model,
not the fitted beat-time slope's covariance, nor a calibrated uncertainty model
for every averaging time or missing-data pattern.

Differencing intentionally limits the contribution of slow variation. Adding a
linear drift to the window values shifts every difference by the same amount,
so the centered MAD is unchanged. A slower cycle can contribute less than a
rapid fluctuation; it need not vanish. This preserves the distinction between
true watch variation and short-range scatter. The ± does not describe the full
range of a drifting watch, and the samples alone cannot identify how much
short-range scatter belongs to the watch rather than extraction or the stand.

A supplied lift angle or clock correction can affect accuracy without appearing
in this SE. Correlation can affect precision without being removed by thinning.
The displayed ± is therefore an estimate conditional on the model, not a
confidence interval with guaranteed coverage on another recording.

## Existing precision probe

The archived October 2026 probe applies the actual `standard_error` function to
300 stationary Gaussian AR(1) values, over 2,000 repetitions per correlation
coefficient. Innovations are scaled by `sqrt(1 − rho²)` and the initial value is
standard Gaussian, so each model has unit marginal variance. The reference mean
and median are zero. These are synthetic values, not detected watch beats.

| Lag-one correlation | RMS error of sample median | Mean reported SE | Fraction within ±SE of zero |
| --- | ---: | ---: | ---: |
| 0 | 0.070913 | 0.072424 | 0.7000 |
| 0.5 | 0.107391 | 0.051307 | 0.3750 |
| 0.9 | 0.255929 | 0.022965 | 0.0685 |

Under these positively correlated models, successive differences get smaller
while the median's repeated-sample error grows. The experiment demonstrates a
limit of the estimator, not that real watch recordings follow these models or
have these errors. It directly exercises the generic median SE, not the complete
live rate fit and averaging-time pipeline. It supplies no universal correction
factor. Sources, original output and reproduction instructions are in
[model probes](model-probes/README.md).

## Period-search reference model

[`longterm`](../crates/timegrapher-core/src/longterm.rs) removes a slow trend,
centers finite grid values and replaces missing values by zero after centering
(the finite-data mean before centering). Its power spectrum is divided by a
local-median estimate of the noise level, using `median / ln(2)` because an
exponential mean-1 variable has median `ln(2)`.

The reference for each normalized power bin is exponential with mean 1. A sum
of `K` independent such bins would have a Gamma(`K`, 1) distribution. The search
scores the best of 1, 2, 4 or 8 harmonics against this reference, with harmonics
restricted to the searched band. Its Gamma-tail calculation is:

```text
P(X >= x) = exp(−x) × sum(i = 0 … K−1, x^i / i!)
score = −log10(P)
trials = max(1, searched FFT bins / padding factor × 4)
threshold = log10(trials) − log10(configured false_alarm)
```

`significance` is `score − log10(trials)`. A value of 2 corresponds to a
model-based whole-search false-alarm approximation of 1%, not a posterior
probability that noise caused the observation. Local normalization, a finite
estimated noise floor, zero padding, harmonic trials, gaps and repeated
fold/subtract searches do not establish independent exponential bins. There is
no universal calibration guarantee for arbitrary noise colour or nonstationary
watch behaviour.

Default acceptance also requires a folded component to explain at least 0.5%
of the detrended variance; up to four components are sought. The search refines
periods using folds and subtracts them before further searches. These additional
steps are part of the algorithm, not independent confirmations of the score.
The [long-run guide](long-runs.md#how-the-search-works) describes the workflow.

The reported `period² / run length` is a nominal resolution scale derived from
frequency spacing. It is not a fitted-period confidence interval, even where
shown with ±. Useful period discrimination also depends on signal support,
observed cycles, gaps and noise. Matching a wheel within that scale or 1% is a
catalogue association, not proof of a mechanical fault.

## Existing period and spectrum probes

The archived probe made 200 synthetic series per case, each with 2,048 values
at 1 s spacing, and called `longterm::analyse` with default settings.

| Noise input | Series with at least one component | Total components |
| --- | ---: | ---: |
| Gaussian white noise | 4 / 200 | 4 |
| AR(1), rho = 0.9 | 10 / 200 | 12 |
| White noise, second half of every 128-bin block missing | 5 / 200 | 5 |

The AR(1) recursion here starts at zero and uses **unscaled** unit-Gaussian
innovations, unlike the stationary unit-variance precision probe above. These
bounded counts are observations from the stated seeds and pipeline, not
population false-alarm rates. In particular, 4/200 white-noise detections alone
are not strong evidence against a 1% rate. The correlated example motivates
qualifying the model claim; it does not establish the noise process of a watch
or discredit any particular recorded cycle.

The same source also records a separate raw-FFT control: a 10-unit, 64 s sine
produced peak heights 10.000000000 with a full grid, 5.000000000 with every
second bin missing, and 1.816707781 with the middle half missing. This illustrates
the existing zero-filled, full-window normalization's dependence on support;
it is not a test of the period-search significance model. Current spectrum
support metadata and the [FFT guide](series.md) expose that limitation without
changing the amplitude estimator.

These are historical model observations assessed against
`009dcc6d54638ff4df51563941935ddc1c654a74`, retained with source and output hashes.
They are not new results from the current build. The archived files do not
establish the original compiler version or linked-artifact hashes. See the
[provenance and opt-in commands](model-probes/README.md) before comparing runs.
