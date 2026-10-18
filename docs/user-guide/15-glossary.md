# 15. Glossary and Sources

## Glossary

**Allan deviation**: the scatter of averages of a reading, against how
long each average is. On independent readings it falls steadily as the
averaging time grows; see [Steadiness](08-over-time.md#the-views).

**Amplitude**: how far the balance swings each way from rest, in degrees.
Worked out from the time between the unlock and the drop, and the lift
angle. See [How a Timegrapher Hears a Watch](01-how-it-works.md#amplitude).

**Average Over**: how many seconds of beats each reading is fitted to; 10
seconds unless you change it. See [Readings](05-readings.md#average-over).

**Beat**: one tick of the watch: the balance knocking the lever across,
once each way in each swing.

**Beat error**: how unevenly the Tick and the Tock are spaced, in
milliseconds. Timed from the unlock (the usual reading), and also from
the drop.

**Beat rate**: beats per hour (bph), counting Ticks and Tocks alike:
28,800 for a balance swinging back and forth 4 times a second.

**bph**: beats per hour.

**Calibre**: the movement's model, such as a maker's number.

**CH, CB, 3H, 6H, 9H, 12H**: the Swiss names of the positions: dial up
(CH), dial down (CB), and for the upright positions the hour that points
up. See [Positions](10-positions.md#the-six-positions).

**Clipping**: the loudest part of the sound cut flat at the top of the
input's range. It moves the amplitude and beat error, not the rate.

**CUSUM**: a running sum of each reading's distance from the mean, which
drifts away when the level changes.

**D**: in a multi-position test, the largest difference between the
positions' rates.

**dBFS**: decibels below the loudest sound the input can carry. 0 dBFS is
the top; −10 dBFS is a third of the way down in amplitude.

**Drop**: the third sound of a beat, when the next escape wheel tooth lands
on the pallet stone. Usually the loudest.

**DU, DD, CU, CD, CL, CR**: dial up, dial down, crown up, crown down,
crown left, crown right.

**DH, DV**: in a multi-position test, the largest difference between the
rates of the flat positions (DH) or of the upright ones (DV).

**Di**: in a multi-position test, the rate crown left (6H) less the rate
dial up (CH).

**DVH**: in a multi-position test, the mean of the upright positions less
the mean of the flat ones, for rate and amplitude.

**FFT**: a spectrum: the size of the cycle at each period in a series of
readings.

**Ie**: in a test made at full wind and again later (usually 24 hours),
the size of the change in the mean rate.

**Im**: in a test made at full wind and again later (usually 24 hours),
the largest change in any position's rate, with crown right left out, as
NIHS 93-10 takes it.

**Impulse**: the second sound of a beat, when the escape wheel tooth
pushes the lever.

**Jitter**: how far single beats land from the straight line the rate is
fitted to, in microseconds. The spread of the dots on the Paper Strip.

**Lift angle**: the angle the balance turns while it is in contact with
the lever, set by the calibre's design. The amplitude is worked out from
it. See [Calibres and Lift Angles](09-calibres.md).

**Ljung–Box test**: a test of whether a series of readings is correlated
over several lags; a small p-value says the readings are not independent.

**NTP**: the Network Time Protocol, by which computers set their clocks
from internet time servers.

**Paper Strip**: the trace of one dot per beat, in the style of a printing
timegrapher. See [The Paper Strip](06-paper-strip.md).

**ppm**: parts per million. A sound card clock 10 ppm off moves a rate by
0.86 s/d.

**Rate**: how many seconds a day the watch would gain (+) or lose (−), in
seconds per day (s/d).

**Session**: everything heard since you started listening, or one
recording opened; also, on the command line, a multi-position test.

**Settling**: the seconds after the watch is turned that are left out of
the readings, while it steadies in its new position: 10 seconds in the
app, 20 seconds in the Positions Test unless you change it, and 30 seconds
of each take made by the command line's `record` (and so by Claude
Desktop's `record_position`).

**Signal**: how far the typical beat stands above the background noise,
such as "signal 30×". Noise alone reads about 2×.

**Tick, Tock**: the two kinds of beat, one each way of the balance's
swing. The Tick is the first beat the software hears. Ticks are blue,
Tocks orange.

**Timing scatter**: how far the beats land from a straight-line fit, as a
share of a beat. At 2% of a beat or more, no regular beat is found.

**True Clock**: the rate corrected for the sound card's own clock error.
See [Readings](05-readings.md#true-clock).

**Unlock**: the first sound of a beat, when the pallet stone lets go of
the escape wheel tooth. The quietest of the three.

**X, XH, XV**: in a multi-position test, the mean rate of all the
positions (X), of the flat ones (XH, shown as H) and of the upright ones
(XV, shown as V).

## Sources

The guide cites these documents. Where it gives a page, the page is the
one the app's own texts cite.

- Witschi, *Measuring Technology and Troubleshooting for Watches*,
  training course, doc. 71.1010D35e, rel. 1.1, 2010: the words early and
  late for the Paper Strip (p. 6), and the 20, 200 and 400 ms spans of the
  scope view (p. 12).
- Witschi, *Testing Methods for Mechanical Watches*, doc. D40e, 2025: the
  20-second stabilisation time used by the Positions Test (measurement
  tips, about p. 34) and the multi-position characteristic values.
- NIHS 93-10: the Im value, the largest change in rate between full wind
  and 24 hours, with crown right left out.
- tg, the open-source timegrapher
  ([github.com/vacaboja/tg](https://github.com/vacaboja/tg)): the reference
  the engine is checked against. Its beat error is timed from the unlock,
  its Paper Strip runs down, and its amplitude uses the same formula.
  Timegrapher contains none of its code. See
  [Validation](../validation.md).
- The calibre list's own sources, given with each calibre in
  [Manage Calibres](09-calibres.md#reading-a-calibres-entry) and in
  `timegrapher calibres NAME`.

The [watchmaker skill](../../skills/watchmaker/SKILL.md) and its
reference pages cite the sources it uses to interpret the figures, with
the page for each.

---

Previous: [Problems and Reports](14-problems-and-reports.md) ·
[Back to the contents](README.md)
