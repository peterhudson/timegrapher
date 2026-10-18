# 10. Isochronism and a Controlled Run-Down

An oscillator is isochronous if its period is independent of amplitude.
For a watch on the bench, the practical question is how much its rate
changes as balance amplitude changes through the useful part of the
power reserve. A watch can be consistently fast yet have little rate
change with amplitude; a watch regulated to zero at one amplitude can
have substantial isochronism error elsewhere.

The **Isochronism** pane offers two views of a recording: a curve of rate
against amplitude, and a comparison of the opening half hour with a later
half hour. Neither view directly measures mainspring torque or isolates
the hairspring from the rest of the watch.

## Plan the Measurement

For a useful run-down, wind the watch fully, place it in one position and
record it continuously without winding or wearing it. Record the start
time, winding state, position, lift angle, microphone arrangement and
anything else that could affect the result. Keep contact and temperature
as consistent as practical. Prevent the computer from sleeping and save
the session early.

Start with the ordinary Rate and Amplitude charts. Confirm that the
amplitude estimate is credible and that the recording covers a meaningful
range. A long run is useful because it gives more amplitude range and
more observations at each level, not simply because a timer has elapsed.
A few degrees of variation in a short recording can generate a fitted
curve without constituting a useful run-down experiment.

Use a **single continuous position**. The app joins separate visits to
the same position for Steadiness and Isochronism, removing time in other
positions and inserting short separators. That constructed timeline is
unsuitable for claiming elapsed hours since winding. Stop/Resume also
omits stopped time from the app's session display. For a 0 h / 24 h
comparison, avoid these interruptions or use explicitly timed separate
position tests instead.

## What Goes Into the Curve

The pane uses the same background calculation as Steadiness. It starts
becoming available after about five minutes of eligible data and updates
as the run grows. **Average Over** on the Readings card does not alter it.

Rate is measured in 10-second intervals. The available 2-second amplitude
windows are averaged into matching intervals, pairing a rate with the
amplitude measured at the same time. The analysis then groups those pairs
into **2° amplitude bins**.

A bin needs at least **10 paired readings**, and a curve needs at least
**three populated bins**. The bin range is based on the 2nd and 98th
percentiles of amplitude, and the analysis applies its isolated-outlier
rule before pairing. These requirements prevent a fit to just a handful
of points. They do not prove that the resulting relationship is causal
or that the amplitude range is broad enough for your purpose.

A message saying **No isochronism curve yet** can therefore mean that
amplitude has stayed within a narrow range, that too few pairs are
available, or that too few bins have enough readings. Check the ordinary
charts and the recording quality before simply waiting longer.

## Reading the Plot

The horizontal axis is **amplitude in degrees**, increasing to the right.
The vertical axis is **rate in seconds per day**. A watch whose amplitude
falls during run-down usually moves from the right side of this graph
towards the left. Time is not the horizontal axis.

Each **dot** is the median rate in one 2° bin, positioned at the bin
centre. Its **vertical bar** covers the 25th to 75th percentile of rates
in that bin: the middle half of the observed readings. The bar is neither
the full range nor an uncertainty interval for the median. Wide bars can
reflect a periodic rate variation, drift unrelated to amplitude,
measurement scatter, or several conditions occupying the same bin.

The **line** is a quadratic fitted to the bin medians, weighted by the
number of readings in each bin. It summarises the sampled relationship;
it is not a theoretical hairspring or escapement model. It need not pass
through every dot.

A nearly horizontal relationship means little observed rate dependence
over the measured amplitude range. A sloping or curved relationship
means the rate changes with amplitude in this recording. It says nothing
by itself about amplitudes outside that range.

### The Slope

The caption reports a separate weighted **straight-line slope**, in s/d
per degree, over the sampled range. It is an overall summary rather than
the local slope of the quadratic at every point.

For an illustrative slope of **+0.20 s/d per degree**, a fall from 280° to
240° would correspond, under that linear approximation, to:

```text
change in amplitude = 240 − 280 = −40°
change in rate ≈ +0.20 × (−40) = −8 s/d
```

The positive slope means higher amplitude accompanies a more positive
rate. During run-down both decrease. A negative slope would mean the rate
becomes more positive as amplitude falls. Always multiply by the signed
amplitude change rather than equating a positive slope with “the watch
speeds up as it runs down.”

A small overall slope can hide curvature: the rate may fall and then
rise across the range. Read the dots and the fitted curve as well as the
single number.

### The Turning Point

When the quadratic has a turning point sufficiently inside the sampled
range, an upright line marks it. For the fitted curve
`r(A) = c₀ + c₁(A − A₀) + c₂(A − A₀)²`, the local slope is zero at:

```text
A_turn = A₀ − c₁/(2c₂)
```

This is what the headline means by the rate changing least near that
amplitude. It is not an instruction to regulate or alter the movement to
run there. A quadratic fitted to a small or noisy range can have a
turning point without establishing a repeatable mechanical optimum.
Compare a second run and check how well the dots support the bend.

## Full Wind Against 24 Hours Later

The table beneath the curve compares median **Rate**, **Amplitude** and
**Beat Error**, with **Change** calculated as later minus earlier. It
requires at least two hours of analysed rate history. Each quantity also
needs enough valid readings in the chosen interval; a missing value is
not zero.

Read the column headings, including their hour ranges:

| Column | Interval used |
| --- | --- |
| **Start** | The first half hour from the start of the rate grid |
| **At 24 h** | The half hour beginning 24 hours later, when that entire interval is available |
| **Last Half Hour** | The final half hour when the 24-hour comparison interval is unavailable |

A continuous recording starting at full wind therefore needs **slightly
more than 24½ hours** to include a complete 24 h to 24½ h comparison
window, allowing for the initial analysis offset. At exactly 24 hours,
the table normally still uses **Last Half Hour**. A two-hour recording
compares its opening and closing half hours; the heading does not turn
that into a 24-hour test.

The desktop calculation assumes the recording starts at full wind. It
cannot infer winding state from sound, and selecting a calibre does not
supply that information. If the watch was wound six hours before recording
began, describe the comparison as start versus end of that recording,
or use the command line's `--wind-h 6` to supply the known offset.

The beat-error values in this analysis retain the software's sign. A
signed change can differ from a change between magnitudes shown on the
cards. Check side continuity and the actual intervals before treating a
sign change as a mechanical change.

This table is a **one-position comparison within one recording**. For
separate measurements in six positions just after winding and a day
later, use the [Positions Test](12-positions.md#full-wind-against-24-hours).
Its saved full-wind results and Im/Ie summary answer a different question.

## What the Curve Can and Cannot Establish

A run-down changes several things together. Mainspring torque, balance
amplitude, temperature and sometimes calendar or other train loads can
change with elapsed time. Rate and amplitude moving together establishes
an observed association; it does not isolate one of those mechanisms.

There is also a measurement link: amplitude depends on the detected
unlock and drop edges. A changing sound or coupling can move an edge and
produce an apparent amplitude change. Compare the Tick and Tock profiles
at several points in the run, particularly around abrupt bends or steps.

The lift angle sets the amplitude scale. Multiplying the entered lift
angle by a factor multiplies the inferred amplitudes by that factor.
The slope in s/d per degree and the apparent location of a turning point
then change even though the recorded watch has not. Use the same verified
lift angle for comparisons and state it in your notes.

The current desktop pane uses the sample clock. A constant clock offset
primarily shifts the rate level, while changing clock error during a
long take can imitate or alter a trend. A clock log is particularly useful
when comparing small rate changes over many hours.

A sound follow-up is a second continuous run in the same position under
comparable conditions. Compare the amplitude range shared by both runs,
the dot pattern and spread, the rate change across that range, and the
actual time windows. Repeat in another position as a separate test if
that is the next question. Record a difference between positions as a
separate result rather than averaging it away.

## Command-Line Analysis

The `series` command supplies both the curve and the wind-down comparison.
For example, with the lift angle and beat rate independently confirmed:

```sh
timegrapher series run-down/ --bph 28800 --lift 52 --wind-h 0 --json > run-down-analysis.json
```

Add `--clock run-down/clock.csv` when the recording carries a suitable
clock log. `--calibre` supplies the wheel periods and, when you don't
give `--lift`, the calibre's lift angle; it does not set the beat rate.

The JSON includes each amplitude bin's count, quartiles and time range,
the fitted coefficients, and the exact comparison intervals. That makes
it possible to check whether a bin represents many hours of behaviour or
just a short passage through one amplitude. `--views` adds the plot data
for the other Steadiness analyses. See
[The Command Line](14-command-line.md#series) and the
[technical series reference](../series.md#isochronism-and-the-0-h--24-h-comparison).

---

Previous: [Steadiness](09-steadiness.md) ·
Next: [Calibres and Lift Angles](11-calibres.md)
