# Reading a seller's timegrapher QC in 60 seconds

This is the beginner's layer, from Peter's guide "How to Understand Your
Timegrapher QC in 60 Seconds" (r/RepTime; also on RWI). Use it when
someone shows you a photo or video of a cheap timegrapher's screen,
usually a "Multifunction Timegrapher No. 1000/1900" or a Weishi, taken
before a watch is shipped. The one number that is easy to understand,
rate, is the least important one at QC. Amplitude is the one that
matters.

## The screen, left to right

| Label | What it is | What to do with it at QC |
| --- | --- | --- |
| **RATE** (s/d) | Seconds gained or lost per day, in this one position, at this moment. | Ignore it anywhere between +30 and −30. The factory regulates dial up and fully wound to near 0, and shipping knocks it about anyway, especially on a regulated (not free-sprung) balance. For scale: a genuine ETA 2824-2 is "in spec" at ±30 s/d in Standard grade, ±20 Elaborate, ±15 Top, and −4/+6 COSC. |
| **AMP.** (°) | How far the balance swings each way: like how wide a grandfather clock's pendulum swings. It measures how well energy gets from the mainspring through the train and escapement to the balance. | The number to look at. Dial up and fully wound, you want **230–300°**, after correcting for the lift angle. Too low can mean dirty oil turning to paste in the pivots. Too high can mean the watch is under-lubricated: oil is sticky and costs energy at this scale; its main job is to stop wear. |
| **B.E.** (ms) | Beat error: the difference in length between tick and tock. | Under **0.7 ms** is fine for a modern movement. |
| **L.A.** (°) | Lift angle: a setting, not a measurement. It goes into the amplitude sum. | Check it. 52° is the usual default and not bad for most movements. Each degree too high adds roughly 5–7° to the amplitude shown. A lift angle of 62–65° is a sign of **amplitude hacking**: making a tired watch look healthy. Don't ask a seller to use a particular lift angle; correct the amplitude yourself at about 5° per degree. |
| **BEAT** | Beat rate. 28,800 vph is 8 beats a second (4 Hz, counting a full tick-and-tock swing as one oscillation), so the seconds hand makes 8 small steps a second. | Make sure it matches the calibre. |
| **Trace** | A dot per beat; the slope is the rate and the gap between two lines is the beat error. Amplitude isn't drawn at all on these machines. | Look for a **straight** line (not curving up then down) running the **full width** of the screen. A rate that changes within the trace's minute is a problem. A short trace hides that. |
| **Sig.** LED | Signal from the stand. | Must be flashing in a **video**. Photos of screens have been faked by covering the LEDs and showing a frozen "good" result for every watch. Always ask for video. |

## Positions and isochronism

The QC shot is dial up, fully wound: the best case. Peter's own stable
VS3235 (Dandong clone of the Rolex 3235) at lift angle 55°, fully wound:

| Position | Rate | Amplitude | B.E. |
| --- | --- | --- | --- |
| Dial up (CH) | 0 s/d | 251° | 0.1 ms |
| Dial down (CB) | −4 | 234° | 0.1 |
| 6 down (12H) | −11 | 222° | 0.1 |
| 12 down (6H) | −4 | 221° | 0.0 |
| 9 down (3H) | −9 | 219° | 0.2 |

Amplitude is highest flat, because the pivots run on their tips; on its
side they run on their sides, more contact, more friction, less
amplitude, and the rate shifts. Keeping the same rate across amplitudes
is **isochronism**. Genuine high-end hairsprings (exotic alloys, Breguet
overcoils) are very good at it; a $5 clone hairspring is not, so a clone
holds its rate over a much narrower band of amplitude. Here 251° to 222°
moved the rate from 0 to −11 s/d, and this was a *good* example; most
clones vary more.

And in daily wear an automatic is rarely fully wound (you'd need about
30,000 steps a day), so the rate on the wrist differs from the QC rate.

## What to tell the buyer

1. Is it a video with the signal LED flashing? If not, ask for one.
2. Lift angle sensible (not above about 58°)? Correct the amplitude for
   it if it's not the calibre's.
3. Amplitude 230–300° dial up? That's the health check.
4. Beat error under 0.7 ms?
5. Trace straight and full width?
6. Rate: ignore it within ±30 s/d.

For anything beyond that, read the numbers as a watchmaker would:
[reading-the-numbers.md](reading-the-numbers.md).
