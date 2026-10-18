# 8. Over Time

A short reading can be perfectly repeatable and still miss the behaviour
you care about. A rate might rise and fall once a minute, step after a
change of position, or drift over a night's run-down. Start by looking at
that history, then choose the analysis that answers your question.

| Question | Start here |
| --- | --- |
| When did the rate or amplitude change? | The ordinary charts below |
| How widely do the readings vary, or do they form separate groups? | [Distributions](#distributions) |
| Does a pattern repeat, and how large is it? | [Steadiness: Periods, Cycle and FFT](09-steadiness.md) |
| How long should I average, and is the level drifting? | [Steadiness: Autocorrelation, Allan Deviation and CUSUM](09-steadiness.md#autocorrelation) |
| How does rate depend on amplitude during run-down? | [Isochronism](10-isochronism.md) |

The screenshots here show an earlier two-hour dial-up recording. They
illustrate the controls and patterns; they are not acceptance limits for
that watch or another movement.

## The Charts

![Rate, Amplitude and Beat Error charts over a two-hour recording](images/charts.png)

The **Rate**, **Amplitude** and **Beat Error** panes plot readings against
time. Rate is red, amplitude purple and beat error yellow. Additional
traces use dashes: the Tick and Tock amplitudes retain their blue and
orange side cues, and drop-based beat error is a fainter dashed yellow.
The key appears in the tab bar. Beat error in these ordinary charts is
shown as a magnitude.

Each point summarises an interval of beats. At the default **Average
Over: 10 s**, a sudden change is blended over that interval; a smooth
curve does not imply that the watch itself changed smoothly. Use a
shorter interval and the strip to examine a transition.

The automatic vertical range is robust to extreme values, using the
central part of the distribution and limits around the median. This
keeps a single knock from flattening the useful trace, but can leave
extreme excursions outside the visible range. Zoom the vertical axis
when investigating an event rather than assuming every value is visible.

### Moving Around

| Action | Result |
| --- | --- |
| Mouse wheel | Zoom time around the pointer |
| Ctrl + mouse wheel | Zoom the vertical scale |
| Drag | Pan |
| Double-click | Fit the session again and follow new data while listening |
| Click a moment | Select that moment for the readings and Paper Strip |

On macOS, use Cmd in place of Ctrl. The three charts share their time
navigation. After panning or zooming, **Follow Live** or **Show Whole
Session** restores the automatic view. Hovering shows the values near
the pointer and marks the corresponding time on the other charts.

### Match Strip

**Match Strip** makes the chart show the Paper Strip's visible time range.
Set the strip to **Across** and place the charts below it to line up their
time axes. You can then follow a bend in the strip directly to an
amplitude or beat-error change.

While Match Strip is on, the strip controls time navigation and the
chart's mouse controls adjust only its vertical scale. Turn it off for
independent chart navigation over the session.

### Average Over

A chart follows the Readings card unless you choose a separate **Average
Over** in its tab bar, beside **Match Strip**. A different value appears in the tab name. Changing
the interval recalculates its history; changing the lift angle recalculates
amplitude history as well. These are analysis settings, not edits to the
saved audio.

## Distributions

![Probability plots for rate, amplitude and beat error](images/distributions.png)

A distribution answers “how often did each value occur?” It deliberately
forgets their order. A gradual drift from +2 to +12 s/d and repeated
switching between those values may have similar distributions but very
different implications at the bench. Always keep a time chart nearby.

Enable **Rate Distribution**, **Amplitude Distribution** and **Beat Error
Distribution** with their icons or the View menu. Their shared controls
select the interval and presentation:

| Control | Meaning |
| --- | --- |
| **Time Span: Session** | Use eligible readings throughout the session in the currently selected position |
| **Time Span: Strip** | Restrict those readings to the visible Paper Strip interval |
| **Show: Probability** | Plot ordered values on a normal-probability scale |
| **Show: Cumulative** | Plot the fraction at or below each value on a linear percentage scale |
| **Values: Readings** | Use readings averaged over the chosen interval |
| **Values: 2 s** | Use the short amplitude or beat-error windows; rate always uses readings |

The Readings option samples at the averaging interval so ordinary
neighbours do not share beats. That avoids overlap, but it does not make
the measurements statistically independent if the watch or measurement
process has memory.

### Probability

The horizontal coordinate is the reading; the vertical coordinate is its
rank transformed to a normal score, labelled in percentages. If there are
`n` ordered readings, the plot assigns reading `i` the probability
`(i − 0.5)/n`, with `i` starting at 1. These are Hazen plotting positions.

An approximately normal distribution gives an approximately straight
line. The dashed reference passes through the median and takes its spread
from the middle half of the values. Its slope describes the spread, not
a change with time.

Curvature tells you that a single normal distribution is an incomplete
description. A bend or two groups can motivate a search for distinct
levels. It does **not** establish two mechanical states: skew, a cycle,
drift, outliers and changing measurement quality can also bend the plot.
Likewise, a straight line says nothing about time independence; reordering
the same readings leaves this plot unchanged.

### Cumulative

At any horizontal value, the height is the share of readings at or below
it. If 90% is reached at +8 s/d, approximately 90% of the included rate
readings were no greater than +8 s/d. A steep section contains many values
in a small range; a flat section contains few or none.

A worked example helps: suppose half a take sits around +2 s/d and half
around +12 s/d, with little time between them. The cumulative plot rises
near +2, flattens near 50%, then rises again near +12. Use the chart to
find whether those groups alternate, occur in successive halves of the
recording, or follow a change in coupling.

### Counts, Tails and Averaging

The summary gives the count, median, middle 80%, values outside the plot
range and any detected peaks. The horizontal range excludes the outer
0.5% at each end. Those observations still matter; the summary counts
them even when they are off the scale.

Compare **Readings** with **2 s** to see what averaging conceals. A broad
2-second amplitude distribution that narrows at 30 seconds may represent
rapid scatter, a short cycle, or both. A narrow distribution is not itself
proof of correct amplitude: the wrong lift angle moves the whole group.

## Steadiness

The seven Steadiness panes retain time order and examine different
aspects of variation. They begin after about five minutes in one position
and update in the background as the session grows. Their analysis
intervals and selected data differ from the ordinary chart controls.

[Chapter 9](09-steadiness.md) covers **Readings Over Time**, **Periods**,
**Cycle**, **Autocorrelation**, **Allan Deviation**, **CUSUM** and **FFT**
individually, with the meaning of their axes, statistics and reference
lines. It also explains how to investigate a suspected wheel-period
pattern without treating the wheel label as a diagnosis.

## Isochronism

The Isochronism pane groups rate readings by amplitude and compares the
start of a run with a later interval. A controlled run-down in one
position is the useful experiment. Changing position, winding the watch
or changing microphone contact during the run makes the relationship
harder to interpret.

[Chapter 10](10-isochronism.md) explains the dots, quartile bars, fitted
curve, slope and turning point, as well as the exact intervals used in
**Full Wind Against 24 Hours Later**. A shorter take can show a comparison
under that heading; check whether the second column says **At 24 h** or
**Last Half Hour**.

---

Previous: [The Sound of the Beat](07-sound-of-the-beat.md) ·
Next: [Steadiness](09-steadiness.md)
