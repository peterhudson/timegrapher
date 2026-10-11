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
each rate reading, 10 s by default. The beat error is the one measured
from the unlock. A 2 h take takes about 30 s, and a 16 h run about 4
minutes.

All three series are judged on readings of the same length, the rate's.
The amplitude and the beat error are measured in 2 s windows; the
windows that fall in each rate reading are averaged into one reading (a
reading needs at least half its windows). Judging a 2 s series and a
10 s series side by side would compare different things: a 2 s window
is noisier, but it also keeps wander that a 10 s reading averages away,
so the same wander can make one series read `wandering` and the other
`steady`. The headlines name the reading length ("steady from one 10 s
reading to the next").

Readings stepped every 10 s cannot show a change faster than 20 s, and a
10 s average already flattens anything near it (it keeps 93% of a 48 s
cycle, 64% of a 20 s one, none of a 10 s one). So cycles are looked for
in the 2 s windows for the amplitude and the beat error: the folds and
the autocorrelation candidates below use them, and a cycle there can be
as short as 6 s. The rate's faster lines, such as the escape wheel's,
come from the period search on the beat timings and are reported in ms
(see [long-runs.md](long-runs.md)).

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

Across the series, the rate is correlated with the amplitude, each
reading against the same reading. There is no lag search: the rate
answers the amplitude at once, and on a shared cycle a lag search finds
the half-turn where the two anticorrelate. The test uses
the changes within half an hour, with each series' slow change taken
out: two series that both drift correlate whether or not one drives the
other, and a drift leaves too few independent readings to tell. The
p-value allows for each series' memory (an effective number of readings
from their lag-1 correlations). When p is under
0.01 and |r| is at least 0.3, the report says the rate moves with the
amplitude, with the slope in s/d per degree. The whole-take slope, drift
included, is given beside it: on a run-down that is the movement's
isochronism, which explains a rate drift that comes with the mainspring
letting down.

A `periodic` verdict needs one cycle that holds up when the readings are
folded at its period. Candidates come from the two-state finder's
regular low level, the autocorrelation's repeat, every later
autocorrelation peak above three times its band, and every line of the
period search, not only the strongest (a slow cycle can sit behind a
strong escape-wheel line). Each candidate's folded shape must explain at
least 10% of the detrended variance beyond what noise would give. Of the
candidates the one that explains most wins, and then its shortest whole
fraction (down to an eighth) that explains at least 85% as much is kept:
folding at five turns of a cycle explains as much as folding at one, and
on a lag grid of 10 s readings a 48 s cycle lines up best at 240 s.
Every candidate's period is refined by folding: over a take of T
seconds a period off by P²/T drifts a whole turn, so on a 16 h run a
48 s cycle has to be found to a few thousandths of a second. The search
narrows until its step is a twentieth of that. When the longer period
still wins, the cycle inside it is named if it explains at least half
as much on its own: on the Reverso's overnight run the rate repeats
every 240 s as five turns of a 48 s cycle whose shape changes from turn
to turn. With the 48 s shape taken out, folding at 240 s still explains
6% of the rate's variance and 8% of the amplitude's, in both halves of
the night. The cycle must also
last at least three readings (2 s windows for the amplitude and the beat
error) and fit five times in the take (three for
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

## Isochronism and the 0 h / 24 h comparison

As the mainspring lets down, the amplitude falls and the rate moves with
it. The isochronism curve bins the rate's 10 s readings by the amplitude
of the same reading (2° bins, each with at least 10 readings, between the
2nd and 98th percentile of the amplitudes) and gives each bin's median
rate, its quartiles and when in the take its readings fall. A straight
line through the bins, weighted by readings, gives the s/d per degree; a
quadratic gives the curve, and the amplitude where it turns (where the
rate changes least) when that is inside the range. On a short take the
amplitude covers only a few degrees and the curve is mostly noise; a
run-down of many hours is what it is for. The cyclic co-movement inside
half an hour is the rate-to-amplitude line above, not this curve.

The wind-down comparison is the commercial 0 h / 24 h test for one
position. It compares the median rate, amplitude and beat error of the
first half hour of the take with the half hour that starts 24 h after
full wind, or with the take's last half hour when it ends sooner (it
needs at least two hours). `--wind-h H` says how many hours after full
wind the take started (default 0: it started at full wind). For several
positions, record two sessions, at full wind and about 24 h later, with
`wind_h` set in each (see [sessions.md](sessions.md)).

## Distributions

How often each value came up, as the app's Distributions pane draws it:
a histogram with the summary line the app shows beside it, and the values
at the shares marked on probability paper. They are counted over the
whole take and, for a take at least two stretches long, over each stretch
of `--stretch-min` minutes from its start (default 60; 0 for the whole
take only; a last stretch shorter than half joins the one before). Each
stretch and the whole take have five: the rate, amplitude and beat error
on the rate's readings (one per 10 s, as the verdicts judge them), and
the amplitude and beat error on each 2 seconds of beats (quicker changes,
more scatter). The bins follow `timegrapher_core::histogram` with the
app's settings: Freedman–Diaconis widths rounded to 1, 2 or 5 times a
power of ten, never finer than 0.1 s/d, 0.5° or 0.01 ms, and the outer
0.5% each side left off the scale. Two peaks in one histogram suggest
two states; a histogram that moves from stretch to stretch is a drift.
The text output prints the whole take's summary lines.

## JSON (`timegrapher.series/1`)

The usual envelope (see [agent-interface.md](agent-interface.md)), then:

| Field | Contents |
|---|---|
| `duration_s`, `bph` | the take |
| `config` | the thresholds used |
| `series[]` | one entry each for `rate` (s/d), `amplitude` (deg) and `beat_error` (ms, from the unlock), in that order |
| `isochronism` | the rate by amplitude: `step_s`, `bin_deg`, `bins[]` (`amplitude_deg`, `readings`, `rate_s_per_day`, `rate_q25`, `rate_q75`, `from_wind_h`, `to_wind_h`), `amplitude_from_deg`, `amplitude_to_deg`, `s_per_day_per_deg`, `rate_change_s_per_day`, `curve_centre_deg`, `curve` (`[c0, c1, c2]` in A − centre), `curve_explained`, `turn_deg`, `headline`; null with fewer than three bins |
| `wind_down` | `start` and `end` (`from_wind_h`, `to_wind_h`, `readings`, `rate_s_per_day`, `amplitude_deg`, `beat_error_ms`), `hours_apart`, `at_24h`, `rate_change_s_per_day`, `amplitude_change_deg`, `beat_error_change_ms`, `headline`; null under two hours |
| `distributions` | `overall[]`, `stretch_s` (0 when the take is not split) and `stretches[]` (`start_s`, `end_s`, `distributions[]`); each distribution has `series` (`rate`, `amplitude`, `beat_error`), `unit`, `values` (`readings` or `2 s`), `step_s`, `n`, `median`, `p10`, `p90`, `bin_width`, `edges` (one more than `counts`; bin i is from `edges[i]` up to `edges[i+1]`), `counts`, `below` and `above` (off the scale, counted in `n`), `peaks` (bin centres of separate peaks), `normal_line` (`centre`, `spread`: the straight line on probability paper; null with no spread), `quantiles` (`[share, value]` at 0.1%, 1, 5, 10, 25, 50, 75, 90, 95, 99 and 99.9% where the count resolves them), `points` (the probability plot the app draws, `[value, z]` lowest first: each value against the normal score of its rank, every value up to 400 and evenly by rank beyond, the lowest and highest always kept), `summary` (the app's line). A series with fewer than three values has no entry. |
| `rate_amplitude` | the rate against the amplitude: `step_s`, `readings` (pairs), `r`, `effective_readings`, `p_value`, `s_per_day_per_deg` (within half an hour), `take_r` and `take_s_per_day_per_deg` (whole take, no lag, drift included), `moves_with`, `headline` |
| `findings[]` | `code`, `series`, `severity` (`fault`, `warning`, `note`), `title`, `evidence`, `advice`. Codes: `periodic`, `two_states`, `measurement_split`, `shifting_mean`, `drifting`, `wandering`, and `rate_moves_with_amplitude`. A steady series has none. |

Each entry of `series[]`:

| Field | Contents |
|---|---|
| `series`, `unit`, `verdict`, `headline` | what it is, and the answer in one plain sentence |
| `step_s`, `readings`, `outliers` | reading length (the rate's for all three series), readings used, readings left out |
| `median`, `sd`, `short_term_sd` | level, spread, reading-to-reading scatter |
| `autocorrelation` | `lag_s[]`, `r[]` (thinned to about 600 points), `band`, `ljung_box_q`, `ljung_box_lags`, `p_value`, `max_short_lag_r`, `memory_s`, `repeat_lag_s`, `repeat_r` |
| `allan` | `tau_s[]`, `deviation[]`, `white[]`, `pairs[]`, `slope_short`, `best_tau_s`, `best_deviation`, `max_excess` (largest deviation over the white line) |
| `cusum` | `t_s[]`, `s[]` (thinned), `max`, `at_s`, `p_value` |
| `changes` | `block_s`, `block_sigma`, `segments[]` (`start_s`, `end_s`, `mean`), `explained` |
| `trend` | `per_hour`, `explained`, `curve_explained`, `start`, `end`, `turn` (`[time, value]` or null) |
| `period` | the period search's strongest component (`other_periods_s`: the weaker ones, each also tried as a cycle): `period_s`, `significance` (−log10 false alarm), `explained`, `peak_to_peak`, `timing_peak_to_peak_ms` (rate only), `size` and `size_unit` (the figure to show: ms of timing for a rate cycle under 30 s, see [long-runs.md](long-runs.md)), `wheel` |
| `two_state` | the two-state finder's full result (rate and amplitude only) |
| `cycle` | the cycle behind `periodic` (or found under another verdict), from the 2 s windows for the amplitude and the beat error: `period_s`, `peak_to_peak`, `explained`, `source` (`two_state`, `autocorrelation`, `period_search`), `wheel`, and `inner_period_s`, `inner_explained`, `turns` when the period is a whole number of turns of a shorter cycle that explains at least half as much |
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
| Yacht-Master crown right, part a | two states, −20 and −7 s/d, about every 35 s; also a 257 s cycle, 15 s/d | 4 min cycle, 6°, flagged as possibly the unlock mark (Tick split alone) | slow change; it flips sign at the suspend gap (1,557 s), where Tick and Tock swap |
| Yacht-Master crown right, part b | two states, −17 and −4 s/d, about every 30 s; also a 60 s cycle (fourth wheel), 10 s/d | 60 s cycle (fourth wheel), 8° | 60 s cycle (fourth wheel), 0.06 ms |
| Yacht-Master crown up (ym42_CU_8h, 2 h 17 min) | two states, −15 and −8 s/d, about every 50 s; level settles from −18 to −9 s/d | 60 s cycle (fourth wheel), 5° | wandering |
| Yacht-Master dial up (ym42_DU_2h) | 60 s cycle (fourth wheel), 12 s/d, 18% of the variance; the level settles from +55 s/d at the start to +6 s/d | two states, 241° and 250°, about every 40 s; also a 60 s cycle (fourth wheel), 11° | 60 s cycle (fourth wheel), 0.09 ms |
| Dandong 3235 (dd3235_DU_30min) | slow change, −4 s/d in the first 10 min to +2 s/d | steady (±16° from window to window hides any small cycle) | steady |
| Daytona 4130 (`--escape-teeth 20`) | 60 s cycle (fourth wheel), 2.8 s/d, 82% of the variance | 60 s cycle, 17°, flagged as possibly the unlock mark (Tick split alone) | 60 s cycle, 0.35 ms, flagged the same way |
| Reverso, whole 2 h | 48 s cycle, 0.8 s/d; level moves between +18 and +21 s/d | 4 min cycle, 10°, beside a 48 s one; level rises from 237° to 256° and falls back to 242° | wandering |
| Reverso, file 01 | two states, +17 and +22 s/d, about every 50 s; also a 47 s cycle, 2.8 s/d | 48 s cycle, 12° | wandering |

The two-state results agree with `session`, and so does the Daytona's
once-a-minute rate cycle. The Yacht-Master's fourth-wheel cycles are
new. Crown left, the two rates follow the fourth wheel's turn, a cycle
the two-state finder's 10 s blocks cannot resolve as regular. Its
amplitude dips once a minute crown left, crown up and dial up as well.
That points at something that turns once a minute: the seconds hand, or
the fourth wheel and its pinion.
