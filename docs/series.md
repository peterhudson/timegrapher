# Is it steady? The `series` command

A healthy watch on a quiet bench gives readings that scatter about one
level, each independent of the last. A histogram or a probability plot
can show whether the readings have one hump or two, but it cannot show
a cycle, a step or a slow wander, because it throws the time order away.
`timegrapher series` keeps the time order. It reads the rate, the
amplitude and the beat error of a take as three series and tests each
one. Each gets one verdict:

| Verdict | Meaning |
|---|---|
| `steady` | Independent readings about one level. |
| `periodic` | The series repeats on a cycle, named after the wheel whose turn it matches. |
| `two_states` | The series switches between two levels (the two-state finder in `twostate.rs`). |
| `measurement` | There are two levels, but they come from the unlock mark hopping, not from the watch. |
| `shifting_mean` | The level steps once or more and stays at the new level. |
| `drifting` | The level slides or curves slowly, as amplitude does when the mainspring runs down. |
| `wandering` | Each reading remembers the last ones, with no cycle, steps or trend to explain it. |
| `too_short` | There are too few readings to say. |

```sh
timegrapher series take/ --lift 55 --clock take/clocklog.txt
```

It reads the same input as `long`: one recording, several segments in
order, or a folder. `--clock` corrects for the sound card's clock. The
other options are the same as for `long`. `--reading` sets the length of
each rate reading, 10 s by default. The 2 s amplitude windows set the
length of the amplitude and beat-error readings. The beat error is the
one measured from the unlock. A 2 h take takes about 30 s, and a 16 h
run about 4 minutes.

## The views

For each series:

- **Autocorrelation**: how closely a reading matches the one a lag
  later. Independent readings stay inside ±1.96/√n at every lag. The
  Ljung–Box test combines the first 10 lags into one p-value. `memory_s`
  is the lag where the correlation falls below 1/e. `repeat_lag_s` is
  the first lag where the correlation rises back to a peak after falling,
  which means the readings repeat.
- **Allan deviation**: the scatter of averages over τ, drawn beside the
  white line, which is what independent readings would give (the scatter
  at one reading divided by √(τ / reading)). For independent readings
  the two lines stay together. Wander and drift lift the curve at long τ,
  and a cycle puts a bump near half its period. `best_tau_s` is the
  averaging time with the smallest scatter, so the most repeatable
  reading.
- **CUSUM**: the running sum of each reading's distance from the mean,
  in units of σ√n, where σ is the reading-to-reading scatter. For
  independent readings it stays within ±1.36 (95%). A step in the mean
  bends it into a V or a tent, with the corner at the step.
- **Change points**: 30 s block medians split into stretches of constant
  mean, at least 5 minutes each. A change is kept only when it is strong
  evidence against one level: the gain must exceed 3 ln n, and never be
  less than 15.
- **Trend**: a straight line and a quadratic. The quadratic also covers a
  rise followed by a fall.
- **Period search**: the strongest component found by `long`'s search.
  It scores each period together with its harmonics against a
  false-alarm chance (see [long-runs.md](long-runs.md)).
- **Two states**: the two-state finder over 10 s blocks (see
  [sessions.md](sessions.md)).

A `periodic` verdict needs one cycle that holds up when the readings are
folded at its period. Candidates come from the two-state finder's
regular low level, the autocorrelation's repeat and the period search.
Each candidate's folded shape must explain at least 10% of the
detrended variance beyond what noise would give. The cycle must also
last at least three readings and fit five times in the take (three for
the period search). The verdicts are tried in this order: two states or
measurement, then a cycle, then steps or a slide, then wander, and
otherwise steady. When the take shows something the verdict does not
name, the headline adds a second sentence. If one side's unlock edges
split in two (the two-state finder's direct check), the amplitude and
beat-error headlines say that what they show may be the measurement.

Readings far from the slow trend are left out as outliers (a knock, a
lost beat). The limit is 6 σ, using the larger of the reading-to-reading
scatter and the spread about the trend. If more than 3% of the readings
would be left out, none are, because then they are part of what the
watch does.

## JSON (`timegrapher.series/1`)

The usual envelope (see [agent-interface.md](agent-interface.md)), then:

| Field | Contents |
|---|---|
| `duration_s`, `bph` | the take |
| `config` | the thresholds used |
| `series[]` | one entry each for `rate` (s/d), `amplitude` (deg) and `beat_error` (ms, from the unlock), in that order |
| `findings[]` | `code`, `series`, `severity` (`fault`, `warning`, `note`), `title`, `evidence`, `advice`. Codes: `periodic`, `two_states`, `measurement_split`, `shifting_mean`, `drifting`, `wandering`. A steady series has none. |

Each entry of `series[]`:

| Field | Contents |
|---|---|
| `series`, `unit`, `verdict`, `headline` | what it is, and the answer in one plain sentence |
| `step_s`, `readings`, `outliers` | reading length, readings used, readings left out |
| `median`, `sd`, `short_term_sd` | level, spread, reading-to-reading scatter |
| `autocorrelation` | `lag_s[]`, `r[]` (thinned to about 600 points), `band`, `ljung_box_q`, `ljung_box_lags`, `p_value`, `max_short_lag_r`, `memory_s`, `repeat_lag_s`, `repeat_r` |
| `allan` | `tau_s[]`, `deviation[]`, `white[]`, `pairs[]`, `slope_short`, `best_tau_s`, `best_deviation`, `max_excess` (largest deviation over the white line) |
| `cusum` | `t_s[]`, `s[]` (thinned), `max`, `at_s`, `p_value` |
| `changes` | `block_s`, `block_sigma`, `segments[]` (`start_s`, `end_s`, `mean`), `explained` |
| `trend` | `per_hour`, `explained`, `curve_explained`, `start`, `end`, `turn` (`[time, value]` or null) |
| `period` | the period search's strongest component: `period_s`, `significance` (−log10 false alarm), `explained`, `peak_to_peak`, `timing_peak_to_peak_ms` (rate only), `size` and `size_unit` (the figure to show: ms of timing for a rate cycle under 30 s, see [long-runs.md](long-runs.md)), `wheel` |
| `two_state` | the two-state finder's full result (rate and amplitude only) |
| `cycle` | the cycle behind `periodic` (or found under another verdict): `period_s`, `peak_to_peak`, `explained`, `source` (`two_state`, `autocorrelation`, `period_search`), `wheel` |
| `unlock_hopping` | amplitude and beat error only: one side's unlock edges split in two |

Times are seconds from the start of the take, on the true clock when
`--clock` is given. The arrays are meant to be drawn as they are: the
autocorrelation with its band, the Allan deviation against `white` on
log-log axes, and the CUSUM with lines at ±1.36.

## On the stored takes

Run on main plus this command, at the lift angles the takes' READMEs
give, with the clock log where the take has one:

| Take | Rate | Amplitude | Beat error |
|---|---|---|---|
| Yacht-Master crown left (ym42_CL_90min) | two states, +44 and +75 s/d, switching about every 35 s; also a 60 s cycle (fourth wheel), 26 s/d peak to peak, 41% of the variance | 60 s cycle (fourth wheel), 7° | 60 s cycle (fourth wheel), 0.25 ms |
| Yacht-Master crown right, part a | two states, −20 and −7 s/d, about every 35 s | 4 min cycle, 6°, flagged as possibly the unlock mark (Tick split alone) | slow change; it flips sign at the suspend gap (1,557 s), where Tick and Tock swap |
| Yacht-Master crown right, part b | two states, −17 and −4 s/d, about every 30 s | wandering | 60 s cycle (fourth wheel), 0.06 ms |
| Yacht-Master crown up (ym42_CU_8h, 2 h 17 min) | two states, −15 and −8 s/d, about every 50 s; level settles from −18 to −9 s/d | 60 s cycle (fourth wheel), 5° | wandering |
| Yacht-Master dial up (ym42_DU_2h) | slow change, +55 s/d at the start settling to +6 s/d | two states, 241° and 250°, about every 40 s; also a 60 s cycle (fourth wheel), 11° | wandering |
| Dandong 3235 (dd3235_DU_30min) | slow change, −4 s/d in the first 10 min to +2 s/d | steady (±16° from window to window hides any small cycle) | steady |
| Daytona 4130 (`--escape-teeth 20`) | 60 s cycle (fourth wheel), 2.8 s/d, 82% of the variance | 60 s cycle, 18°, flagged as possibly the unlock mark (Tick split alone) | 2 min cycle, 0.28 ms, flagged the same way |
| Reverso, whole 2 h | level moves between +18 and +21 s/d | 2 min cycle, 7°; level rises from 237° to 256° and falls back to 242° | wandering |
| Reverso, file 01 | two states, +17 and +22 s/d, about every 50 s | 48 s cycle, 11° | wandering |

The two-state results agree with `session`, and so does the Daytona's
once-a-minute rate cycle. The Yacht-Master's fourth-wheel cycles are
new. Crown left, the two rates follow the fourth wheel's turn, a cycle
the two-state finder's 10 s blocks cannot resolve as regular. Its
amplitude dips once a minute crown left, crown up and dial up as well.
That points at something that turns once a minute: the seconds hand, or
the fourth wheel and its pinion.
