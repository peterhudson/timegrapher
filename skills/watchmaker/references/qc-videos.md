# Reading a seller's timegrapher QC in 60 seconds

This is the beginner's layer. Use it when
someone shows you a photo or video of a cheap timegrapher's screen,
usually a "Multifunction Timegrapher No. 1000/1900" or a Weishi, taken
before a watch is shipped. The one number that is easy to understand,
rate, is the least important one at QC. Amplitude is the one that
matters.

## The screen, left to right

| Label | What it is | What to do with it at QC |
| --- | --- | --- |
| **RATE** (s/d) | Seconds gained or lost per day, in this one position, at this moment. | One position at one moment says little about how the watch keeps time, so don't judge the watch on it. For scale, the makers' and COSC's own figures are means over several positions and days: ETA grades the 2824-2 at a mean of ±12 s/d (Standard), ±7 (Elaboré) and ±4 (Top) (sources in SKILL.md), and [COSC](https://www.cosc.swiss/en/certification/mechanical-movements) certifies a mean daily rate of −4/+6 s/d over its first ten days of testing. |
| **AMP.** (°) | How far the balance swings each way: like how wide a grandfather clock's pendulum swings. It measures how well energy gets from the mainspring through the train and escapement to the balance. | The number to look at. Dial up and fully wound, judge it against the maker's figure where one exists, otherwise Witschi's training course, 250–330° horizontal ([tolerance sets](when-to-see-a-watchmaker.md#tolerance-sets-by-source)), after correcting for the lift angle. Low amplitude is the usual first sign of ageing oil or wear; very high amplitude is first a reason to check the lift angle and the measurement. |
| **B.E.** (ms) | Beat error: the difference in length between tick and tock. | Witschi's training course gives **0.0–0.5 ms** as acceptable. |
| **L.A.** (°) | Lift angle: a setting, not a measurement. It goes into the amplitude sum. | Check it against the calibre table (`timegrapher calibres NAME`). Amplitude is computed in proportion to the lift angle set, so a lift angle set too high makes a tired watch look healthy. Don't ask a seller to use a particular lift angle; correct the amplitude yourself: shown amplitude × (the calibre's lift angle ÷ the one set). |
| **BEAT** | Beat rate. 28,800 vph is 8 beats a second (4 Hz, counting a full tick-and-tock swing as one oscillation), so the seconds hand makes 8 small steps a second. | Make sure it matches the calibre. |
| **Trace** | A dot per beat; the slope is the rate and the gap between two lines is the beat error. Amplitude isn't drawn at all on these machines. | Look for a **straight** line (not curving up then down) running the **full width** of the screen. A rate that changes within the trace's minute is a problem. A short trace hides that. |
| **Sig.** LED | Signal from the stand. | Must be flashing in a **video**: a photo can't show that the reading came from this watch, live. Always ask for video. |

## Positions and isochronism

The QC shot is dial up, fully wound. Amplitude is highest flat,
because the balance pivots run on their tips; on its side they run on
their sides, with more friction, less amplitude, and a shifted rate.
Keeping the same rate across amplitudes is **isochronism**; how much a
given watch shifts is for its own six-position test to say (see
guided-test.md). A watch on the wrist is rarely fully wound and moves
through every position, so its rate in wear differs from the QC rate.

## What to tell the buyer

1. Is it a video with the signal LED flashing? If not, ask for one.
2. Lift angle the calibre's? If not, correct the amplitude for it.
3. Amplitude within the maker's figure, or Witschi's 250–330°, dial up? That's the health check.
4. Beat error under 0.5 ms?
5. Trace straight and full width?
6. Rate: one position at one moment; note it, but don't judge the watch on it.

For anything beyond that, read the numbers as a watchmaker would:
[reading-the-numbers.md](reading-the-numbers.md).
