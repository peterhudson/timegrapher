# Structure in time: the `series` command

A histogram or a probability plot shows whether a take's readings have
one hump or two, but not a cycle, a step or a slow wander, because it
throws the time order away. `timegrapher series` keeps the time order.
It reads the rate, the amplitude and the beat error of a take as three
series, runs the tests below on each, and states what they find with
the statistics behind it. It does not say whether a watch is good or
bad; that is for the person or the agent reading the facts.

Each series' headline leads with the first of these that the tests
detect, and adds a sentence for the others:

| Detected | Headline states |
|---|---|
| two levels | the two levels and how often the series switches between them (the two-state finder in `twostate.rs`) |
| two levels on one side only | the same, and which side carries them: "Tick windows fall in two clusters; Tock windows do not", or that the beat error from the unlock jumps between the levels while the one from the drop does not |
| a cycle | its period, the wheel whose turn it matches, and its size peak to peak |
| level changes | the number of changes and the level of each stretch; beyond three changes, the number, the range, and the first and last levels (every stretch is in `changes.segments[]`, and the text output lists each with its start and end) |
| a trend | the start and end level and the change per hour |
| correlation between readings | the lag-1 correlation, the Ljung–Box p, how long the correlation lasts, and the Allan excess |
| none of these | the number of readings, the median and the reading-to-reading scatter |
| a period search line faster than the readings | the period, the wheel whose turn it matches, the size peak to peak and that the readings are too coarse to show it, e.g. "the beat timing repeats every 5.0 s, the escape wheel's period, 0.24 ms peak to peak; 10 s readings are too coarse to show it". The "none of these" and correlation headlines carry it in place of "no cycle"; the cycle, two levels, level change and trend headlines add it as a sentence of its own ("The beat timing also repeats every 5.0 s, …" beside a slower cycle). For the rate a line under 30 s is the beat timing's swing in ms, and a slower one (with `--reading 20` or longer) the rate's swing in s/d; "faster than the readings" means under three of the readings cycles are looked for in: the 2 s windows for the amplitude and beat error, the readings for the rate |
| too few readings | N readings over N s (it needs 30 over 120 s) |

After these, the headline names the period search's lines it has not
named yet, as a last sentence: "The amplitude's period search also finds
45 s (7.37° peak to peak)." Only the search's strongest line has a size;
the others are given by period, in seconds. A line counts as named when
it is within the resolution (the period squared over the take, at least
1%) of the cycle or the shorter cycle inside it, or when it is the fast line above. The
search lists only lines that pass its own significance test, so the
sentence adds no threshold. When it follows a lead that would say "no
cycle … detected", the lead leaves cycles out of that list.

The thresholds are detection thresholds, stated with each test below
(a false-alarm chance, a share of the variance explained); none is a
tolerance for a watch.

```sh
timegrapher series take/ --lift 55 --clock take/clocklog.txt
```

It reads the same input as `long`: one recording, several segments in
order, or a folder. `--clock` corrects for the sound card's clock. The
other options are the same as for `long`. `--reading` sets the length of
each rate reading, 10 s by default. The beat error is the one measured
from the unlock; where its sign turns over across a break, its readings
are sizes, given as positive, as the take's beat error is (see
[long-runs.md](long-runs.md)), and a `Conditions` line and the
`beat_error_as_size` caveat say so. Where Tick and Tock were named by the
beat count across a break, not told apart by their sound, a `Sides` line
says so. A 2 h take takes about 30 s, and a 16 h run about 4
minutes.

All three series are tested on readings of the same length, the rate's.
The amplitude and the beat error are measured in 2 s windows; the
windows that fall in each rate reading are averaged into one reading.
No reading is averaged over part of its span: an amplitude or beat
error reading needs every 2 s window its span holds, and a rate reading
needs beats in each 2 s piece of it and must not reach into a break's
gap from either side, so a reading at the edge of a gap or a masked
stretch, or with a window missing, is left out rather than averaged
over what is left. The Allan deviation's averages need every reading of
their block for the same reason. Testing a 2 s series and a 10 s series
side by side would compare different things: a 2 s window is noisier,
but it also keeps wander that a 10 s reading averages away, so the same
wander can show as correlation in one series and not in the
other. The headlines name the reading length.

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
  and a cycle of period P puts a bump near 0.37 P. `best_tau_s` is the
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
from their lag-1 correlations). The slope in s/d
per degree, r and p are always given, with no threshold, and beside
them the whole-take slope and r, drift included.

A cycle is reported when one that holds up when the readings are
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
the period search). A cycle at the alias of a period-search line too
fast for the readings is set aside: readings averaged over 10 s show a
3.75 s line (a 15-tooth escape wheel at 28,800 bph) at 30 s, and fold
almost as well at 60 s, 120 s or 330 s (and partly at half turns, 75 s).
A whole number of the alias's turns, or a half number that is also a
whole number of the line's own turns, matched within the resolution
(the period squared over the take, at least 1%, and at least 5% of the
alias), still counts only when folding there explains more than 1.25
times what the alias does, as the same watch's 60 s fourth wheel does.
On a long take that tolerance covers most periods above a few turns of
the alias (for the 30 s alias, every period above about 160 s on a 1 h
take, and above about 430 s on a 7 h take), so a real slow cycle there
must clearly outfold the alias to count unless the period search lists
it. On takes shorter than
twenty turns of the alias the 5% floor no longer sets the match at one
turn either, so it widens there too. A cycle the period search also
finds in the beat timing as a line of three readings or longer (within
the period squared over the take, at least 1%: the search's own rule for
matching a wheel) always counts, and is not reduced to a shorter
fraction that lands on the alias or its turns: at 19,800 bph a 15-tooth escape wheel turns in 5.45 s,
which aliases to exactly the fourth wheel's 60 s. At a turn and a half
or more of an alias the fold holds the alias as well, so the cycle's share and
size are given net of the alias's own fold (the readings less their
mean shape at the alias), and it must still explain 10%: on the 28,800
bph watch with a 15-tooth escape wheel, a 60 s fourth wheel of 2 s/d
peak to peak beside a 30 s alias of 2 s/d is given its own 2 s/d, not
the fold's larger swing, and a 60 s line the search lists with nothing
of its own in the readings is no cycle (the headline then
names it from the search). On the alias itself the two cannot be parted,
and the size is the fold's.

Folded at a period of an odd number of readings, n, the readings fall
in the same groups as at half of it: reading k is at phase k/n of the
one and 2k/n of the other, and with n odd both visit the same n phases,
in another order. So a 45 s cycle in 10 s readings can be found at
90 s, nine readings a turn. Which of the two the readings follow shows
in the shape's harmonics: a shape that repeats at half the period has
its variance in the even ones. When the period is known to under half
a reading (so that one whole number of readings is within the
resolution) and is seven readings or more, the cycle moves to half its
period if sines at the half and the quarter explain 85% or more of what
all n phases explain (sines at every harmonic up to n/2). At seven
readings a turn the quarter is under two readings and cannot be told
from the third in them, so the half is fitted alone. A line the
period search lists at an odd harmonic, such as a 30 s line beside
90 s, or a listed fast line's alias there, such as the 30 s alias of a
3.75 s escape wheel, is a line of its own sharing those phases and is
left out of the n phases first; a cycle the search lists at its own
period stays. The moved cycle's share and size are those of the sines
(a fold at 4.5 readings a turn puts readings of different phases in one
bin and reads low), and it must still explain 10% and clear a fast
line's alias, net of it at a turn and a half or more, as any cycle must.
Its source is where the period it was moved from was found.

The headline leads
with two levels, then a cycle, then level changes or a trend, then
correlation between readings. When the take shows more than the lead,
the headline adds a sentence. If one side's unlock edges or 2 s windows
fall in two clusters the other side's do not (the two-state finder's
direct check), the amplitude and beat-error headlines add that fact,
e.g. "Tick windows fall in two clusters; Tock windows do not", and
`unlock_hopping` is true.

Readings far from the slow trend are left out as outliers (a knock, a
lost beat). The limit is 6 σ from the 5-minute level, using the larger
of the reading-to-reading scatter and the spread about that level; the
text output states it beside the count. If more than 3% of the readings
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
on the rate's readings (one per 10 s, as the tests use them), and
the amplitude and beat error on each 2 seconds of beats (quicker changes,
more scatter). The bins follow `timegrapher_core::histogram` with the
app's settings: Freedman–Diaconis widths rounded to 1, 2 or 5 times a
power of ten, never finer than 0.1 s/d, 0.5° or 0.01 ms, and the outer
0.5% each side left off the scale. The text output prints the whole take's summary lines.

## JSON (`timegrapher.series/2`)

The usual envelope (see [agent-interface.md](agent-interface.md)), then
the fields below. Version 2 removed the one-word `verdict` of each
series, the findings' `severity` and `advice`, and `rate_amplitude`'s
`moves_with`, and the `rate_moves_with_amplitude` finding with it; the
headlines state facts. Everything else is as in version 1.

| Field | Contents |
|---|---|
| `duration_s`, `bph` | the take |
| `caveats[]` | conditions detected, each with a `code`: `sides_by_count` (`count`: the breaks across which Tick and Tock were named by the beat count, not told apart by their sound) and `beat_error_as_size` (the beat error's sign differs either side of a break, so its readings are sizes, positive, not signed), as for `analyze`; empty when none |
| `config` | the detection settings used |
| `series[]` | one entry each for `rate` (s/d), `amplitude` (deg) and `beat_error` (ms, from the unlock), in that order |
| `isochronism` | the rate by amplitude: `step_s`, `bin_deg`, `bins[]` (`amplitude_deg`, `readings`, `rate_s_per_day`, `rate_q25`, `rate_q75`, `from_wind_h`, `to_wind_h`), `amplitude_from_deg`, `amplitude_to_deg`, `s_per_day_per_deg`, `rate_change_s_per_day`, `curve_centre_deg`, `curve` (`[c0, c1, c2]` in A − centre), `curve_explained`, `turn_deg`, `headline`; null with fewer than three bins |
| `wind_down` | `start` and `end` (`from_wind_h`, `to_wind_h`, `readings`, `rate_s_per_day`, `amplitude_deg`, `beat_error_ms`), `hours_apart`, `at_24h`, `rate_change_s_per_day`, `amplitude_change_deg`, `beat_error_change_ms`, `headline`; null under two hours |
| `distributions` | `overall[]`, `stretch_s` (0 when the take is not split) and `stretches[]` (`start_s`, `end_s`, `distributions[]`); each distribution has `series` (`rate`, `amplitude`, `beat_error`), `unit`, `values` (`readings` or `2 s`), `step_s`, `n`, `median`, `p10`, `p90`, `bin_width`, `edges` (one more than `counts`; bin i is from `edges[i]` up to `edges[i+1]`), `counts`, `below` and `above` (off the scale, counted in `n`), `peaks` (bin centres of separate peaks), `normal_line` (`centre`, `spread`: the straight line on probability paper; null with no spread), `quantiles` (`[share, value]` at 0.1%, 1, 5, 10, 25, 50, 75, 90, 95, 99 and 99.9% where the count resolves them), `points` (with `--views`, on the whole take's distributions only: the probability plot the app draws, `[value, z]` lowest first: each value against the normal score of its rank, every value up to 400 and evenly by rank beyond, the lowest and highest always kept), `summary` (the app's line). A series with fewer than three values has no entry. |
| `rate_amplitude` | the rate against the amplitude: `step_s`, `readings` (pairs), `r`, `effective_readings`, `p_value`, `s_per_day_per_deg` (within half an hour), `take_r` and `take_s_per_day_per_deg` (whole take, no lag, drift included), `headline` (all of these as numbers, whatever their size) |
| `spectra[]` | the plain amplitude spectrum of each series, as a commercial timegrapher's FFT shows it: `series` (`rate`, `amplitude`, `beat_error`), `unit` (`ms` of timing for the rate, `deg`, `ms`), `step_s`, `period_s[]` (longest first) and `amplitude[]` (an estimate of sine-wave size at each period for complete regular inputs, half its peak to peak), and `support` (`finite_bins`, `total_bins`). The rate's comes from the beat timing, so it reaches down to the escape wheel; the amplitude's and the beat error's from their 2 s windows. Each series has its slow change taken off as the period search takes it off, a Hann window is applied and its loss corrected, and the points are thinned to at most 600 on a log period axis, keeping the largest in each slice so narrow lines survive. The period search reads the same Fourier transform but whitens it and sums harmonics to decide what stands out; this keeps the raw sizes, best drawn on log axes. |
| `period_span` | with `--views`: `[lo, hi]`, log10 seconds, the periods every series' search covers between them, so the Periods plots share one axis |
| `findings[]` | one per series where a test detected structure: `code`, `series`, `title` (what was detected), `evidence` (the headline and the statistics). Codes: `periodic`, `two_states`, `measurement_split` (two levels that show on one side only, or in the beat error from the unlock but not from the drop), `shifting_mean` (level changes), `drifting` (a trend), `wandering` (correlation between readings). A series where none was detected has none. |

Each entry of `series[]`:

| Field | Contents |
|---|---|
| `series`, `unit`, `headline` | what it is, and what the tests found in one plain sentence |
| `step_s`, `readings`, `outliers` | reading length (the rate's for all three series), readings used, readings left out |
| `median`, `sd`, `short_term_sd` | level, spread, reading-to-reading scatter |
| `autocorrelation` | `lag_s[]`, `r[]` (thinned to about 600 points), `band`, `ljung_box_q`, `ljung_box_lags`, `p_value`, `max_short_lag_r`, `memory_s`, `repeat_lag_s`, `repeat_r` |
| `allan` | `tau_s[]`, `deviation[]`, `white[]`, `pairs[]`, `slope_short`, `best_tau_s`, `best_deviation`, `max_excess` (largest deviation over the white line) |
| `cusum` | `t_s[]`, `s[]` (thinned), `max`, `at_s`, `p_value` |
| `changes` | `block_s`, `block_sigma`, `segments[]` (`start_s`, `end_s`, `mean`), `explained` |
| `trend` | `per_hour`, `explained`, `curve_explained`, `start`, `end`, `turn` (`[time, value]` or null) |
| `period` | the period search's strongest component (`other_periods_s`: the weaker ones, each also tried as a cycle): `period_s`, `significance` (−log10 false alarm), `explained`, `peak_to_peak`, `timing_peak_to_peak_ms` (rate only), `size` and `size_unit` (the figure to show: ms of timing for a rate cycle under 30 s, see [long-runs.md](long-runs.md)), `wheel` |
| `two_state` | the two-state finder's full result (rate and amplitude only) |
| `cycle` | the cycle found, if any, from the 2 s windows for the amplitude and the beat error: `period_s`, `peak_to_peak`, `explained`, `source` (`two_state`, `autocorrelation`, `period_search`), `wheel`, and `inner_period_s`, `inner_explained`, `turns` when the period is a whole number of turns of a shorter cycle that explains at least half as much |
| `unlock_hopping` | amplitude and beat error only: one side's unlock edges or 2 s windows fall in two clusters the other side's do not |
| `views` | with `--views`: what the Steadiness pane draws for each series besides the arrays above. `readings` (`step_s`, `n`, `stride`, `t_s[]`, `values[]`: the readings the tests were made on, on the tests' time (the clock's, with each pause left empty), 10 s for the rate and 2 s windows for the amplitude and the beat error, at most 3000, every `stride`th); `search` (`period_s[]` longest first, `score[]`, `threshold`: the period search, at most 600 periods; null when there is none); `fold` (`period_s`, `from` (`own`, another series' name, or `wheel` with `wheel` naming it), `profile[]` (median per phase bin: one bin per phase the readings fall on when those repeat within two turns, at most 40; otherwise one per reading in a turn, 6 to 40), `cycles`, `raster[][]` (at most 200 of the cycles, evenly spread, one value per bin, null where empty); null when no period fits) |

`--views` (with `--json`) adds the chart data: the `points` and `views`
above, a few hundred kB on a long take (about 600 kB over 2 h 40 min), so
plain `--json` stays small for agents. The MCP server asks for them and
keeps them out of what the model reads.

The JSON's top-level `clock` describes the correction actually applied to
these readings: `ppm`, `rate_error_s_per_day`, `from` (the source), and
`tracks_drift`. It is `null` when no correction was applied. An explicit
`--clock` log takes precedence; otherwise the stored correction for the
input named by the first recording's provenance is used, then USB lines
from that recording if available. A stored zero correction is still an
applied correction. There is no `--card-ppm` or `--device` option for `series`.
Positive ppm means the card runs slow and uncorrected rates read fast;
`rate_error_s_per_day` is ppm × 0.0864. For a log that follows drift, these
numbers summarize the fit; its time mapping need not be one uniform
correction. These fields do not claim calibration uncertainty or accuracy.

Times are seconds from the start of the take, mapped through the selected
clock fit when `clock` is non-null. The arrays are meant to be drawn as they are: the
autocorrelation with its band, the Allan deviation against `white` on
log-log axes, and the CUSUM with lines at ±1.36.

## Synthetic takes

`timegrapher synth --scenario NAME FOLDER` writes a take with one known
variation hidden in it, to check what each view shows; see
[synthetic-takes.md](synthetic-takes.md).

## On the stored takes

Run on main plus this command, at the lift angles the takes' READMEs
give, with the clock log where the take has one:

| Take | Rate | Amplitude | Beat error |
|---|---|---|---|
| Yacht-Master crown left (ym42_CL_90min) | two states, +44 and +75 s/d, switching about every 35 s; also a 60 s cycle (fourth wheel), 26 s/d peak to peak, 41% of the variance | 60 s cycle (fourth wheel), 7° | 60 s cycle (fourth wheel), 0.25 ms |
| Yacht-Master crown right, part a | two states, −20 and −7 s/d, about every 35 s; also a 257 s cycle, 15 s/d | 4 min cycle, 6°; Tick windows fall in two clusters, Tock windows do not | slow change; it flips sign at the suspend gap (1,557 s), where Tick and Tock swap |
| Yacht-Master crown right, part b | two states, −17 and −4 s/d, about every 30 s; also a 60 s cycle (fourth wheel), 10 s/d | 60 s cycle (fourth wheel), 8° | 60 s cycle (fourth wheel), 0.06 ms |
| Yacht-Master crown up (ym42_CU_8h, 2 h 17 min) | two states, −15 and −8 s/d, about every 50 s; level settles from −18 to −9 s/d | 60 s cycle (fourth wheel), 5° | successive readings correlated |
| Yacht-Master dial up (ym42_DU_2h) | 60 s cycle (fourth wheel), 12 s/d, 18% of the variance; the level settles from +55 s/d at the start to +6 s/d | two states, 241° and 250°, about every 40 s; also a 60 s cycle (fourth wheel), 11° | 60 s cycle (fourth wheel), 0.09 ms |
| Dandong 3235 (dd3235_DU_30min) | slow change, −4 s/d in the first 10 min to +2 s/d | nothing detected (±16° from window to window would hide any small cycle) | nothing detected |
| Daytona 4130 (`--escape-teeth 20`) | 60 s cycle (fourth wheel), 2.8 s/d, 82% of the variance | 60 s cycle, 17°; Tick windows fall in two clusters, Tock windows do not | 60 s cycle, 0.35 ms; the same Tick clusters |
| Reverso, whole 2 h | 48 s cycle, 0.8 s/d; level moves between +18 and +21 s/d | 4 min cycle, 10°, beside a 48 s one; level rises from 237° to 256° and falls back to 242° | successive readings correlated |
| Reverso, file 01 | two states, +17 and +22 s/d, about every 50 s; also a 47 s cycle, 2.8 s/d | 48 s cycle, 12° | successive readings correlated |

The two-state results agree with `session`, and so does the Daytona's
once-a-minute rate cycle. The Yacht-Master's fourth-wheel cycles are
new. Crown left, the two rates follow the fourth wheel's turn, a cycle
the two-state finder's 10 s blocks cannot resolve as regular. Its
amplitude dips once a minute crown left, crown up and dial up as well.
That points at something that turns once a minute: the seconds hand, or
the fourth wheel and its pinion.

### FFT input availability

Each spectrum's `support` counts finite samples in the actual detrended FFT input
grid, before next-power-of-two padding. Nonfinite samples (NaN or infinity) count
as missing. These are grid-sample counts, not acoustic coverage or the percentage
of watch beats heard; complete finite input does not establish continuous sound.
The app and MCP FFT show these counts. Missing-input plots remain available with
a note: missing samples affect peak heights and shapes; values are not corrected
for gaps. Older chart documents without counts state that support was not recorded.

The calculation is unchanged: the finite-data mean is removed, missing samples
are zero-filled, and magnitudes use the full grid's Hann-weight sum. There is no
missing-data renormalization. Equal missing counts with different gap patterns can
produce different heights and shapes. The converted peak-to-peak rate display is
subject to the same limitation; counts are neither a correction factor nor an
uncertainty estimate. Eligibility, numeric arrays and period search are unchanged.

When MCP moves spectrum arrays into chart metadata, compact `spectrum_support`
entries (`series`, `unit`, `support`) remain in the model-visible structured result
and text lead. Legacy spectra have `support: null` there; no completeness is inferred.
