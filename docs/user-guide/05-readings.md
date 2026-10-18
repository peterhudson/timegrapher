# 5. Readings

The five cards across the top of the window are the readings: what the
watch is doing now, fitted to the last few seconds of beats.

![The five reading cards: Readings with Average Over 10 s, Rate in seconds per day, Amplitude in degrees with the Tick and Tock amplitudes, Beat Error in milliseconds, and Beat Rate in beats per hour](images/readings.png)

## Average Over

The **Readings** card holds one setting, **Average Over**: how many
seconds of beats each reading is fitted to. It is 10 seconds unless you
change it. Pick one of the presets from 2 to 60 seconds, or type any time
from 1 second to an hour.

- A **longer** time gives steadier figures that follow changes more
  slowly, and smaller ± figures.
- A **shorter** time follows the watch more closely, with more scatter.

Early in a session, or after a pause or a gap of more than 2 seconds with
no beats, there are fewer seconds of beats than asked for. The readings
use what there is and say so under the rate ("79 beats in 10 s"). They
never reach back across a pause or a change of position, since the watch
may have been moved or handled meanwhile.

Average Over reaches further than the cards. It also sets the points on
the Rate, Amplitude and Beat Error charts, the Rate Line and the lines
drawn over the Paper Strip, the Distributions, and the Tick Tock Profile.
Each of those panes can take a time of its own instead, in its toolbar's
**Average Over**; a pane with its own time names it on its tab, such as
"Rate · 30 s". Changing Average Over in the Readings card puts every pane
back on it.

## Rate

How many seconds a day the watch would gain (**+**) or lose (**−**) if it
kept running exactly as it did over the beats the reading was fitted to.

It comes from a straight line fitted to the time of every beat, Ticks
and Tocks together, so the beat error doesn't move it. Under the figure:

- **how many beats** the reading used, over how many seconds;
- **jitter**: how far single beats land from that straight line, as a
  robust standard deviation in microseconds (µs). It is the spread of the
  dots across the Paper Strip. Background noise, a muffled sound and the
  stand add to it as well as the watch, so figures from different
  microphones and stands differ.

Beside the figure is its **±**: roughly how far the rate could be off from
the scatter of 10-second rates over the run so far, scaled to the
averaging time. It needs about 30 seconds of unbroken beats, and is left
out when it is under 0.05 s/d.

After you stop listening following a long run, a line under the figure
gives the whole take's mean and spread, such as "whole take +57.0 s/d,
spread 21.1 s/d over 1:38".

### True Clock

Every rate is timed against the sound card's clock, the same clock tg and
other software timegraphers use. A sound card's crystal typically runs
10 to 50 parts per million (ppm) off, which moves every rate read on it by
1 to 4 s/d, always in the same direction for the same card.

The app can measure that error for you:

1. Make sure your computer's clock is set by internet time (NTP), which
   is the usual setting on macOS, Windows and Linux.
2. Listen to a watch for at least 20 minutes. While it listens, the app
   times the microphone's clock against the computer's.
3. Once it knows the error to within half a part per million, the Rate
   card shows the error it timed with a **Keep** button. A busy computer
   delivers sound less evenly, so the offer can take longer.
4. Click **Keep**.

From then on, whenever you listen through that microphone, a line under
the rate gives the rate corrected onto true time, labelled **True
Clock**. The big figure always stays on the card's clock, so it still
compares directly with tg and with `timegrapher analyze` on the same
recording. You can also measure the error from the command line with
`timegrapher clock measure --save` (see
[The Command Line](12-command-line.md#clock)).

## Amplitude

How far the balance swings each way from rest, in degrees, averaged over
the Ticks and Tocks of the averaging time. Under it, with a blue and an
orange dot, are the amplitude measured from the **Ticks** alone and from
the **Tocks** alone, and the **lift angle** in use.

The amplitude is worked out from the time between the unlock and the
drop of each beat and the lift angle (see
[How a Timegrapher Hears a Watch](01-how-it-works.md#amplitude)), so it is
only as right as the lift angle under Watch. Its ± is the scatter of its
2-second pieces alone: precision, not accuracy. 1° of lift angle moves the
amplitude by about 4°, so a lift angle that is a little out matters more
than the ±.

## Beat Error

How unevenly the Tick and the Tock are spaced, in milliseconds.

- The **big figure** is timed from the **unlock**, the first sound of each
  beat, as tg measures it. This is the usual reading.
- The smaller figure, **from the drop**, is timed from the beat as a whole,
  nearer the drop. It is the gap between the two lines of dots on the
  Paper Strip.

The beat error is shown as a size. Which beat is the Tick depends on where
the analysis first locks on, so which side is longer can change from one
run to the next.

## Beat Rate

The beats per hour the readings use. With **Beat Rate** under Watch on
**Auto**, it is the standard rate nearest to the beats of the first few
seconds, and the card says **found from the beats**. If you set it under
Watch, the card says **set under Watch**. Set it by hand when the watch
beats at an unusual rate, or runs so far off that Auto picks the wrong
one. Changing it starts the readings again.

## What Limits a Reading

The ± beside each figure is worked out from the scatter of the beats, and
some things can't show in it. When the app knows of one, it names it as a
plain fact at the right of the card's title, with a sentence when you
rest the pointer on it:

| Shown | Means |
| --- | --- |
| **12 s of beats** | The beats span less than 10 seconds |
| **540 of 600 expected beats found** | Fewer than 95% of the beats expected were found |
| **3% of beats clipped** | More than 1% of the beats clip (amplitude and beat error) |
| **lift angle 52°, the default (calibre not set)** | No calibre or lift angle was given (amplitude) |
| **21600 bph heard** | The beat rate set under Watch differs from the one heard |
| **2 pauses** | The take was paused in this stretch |

None of these is a grade. They say what the figure stands on, so you can
decide how much weight to give it.

## No Regular Beat Found

When the beats found don't come at regular intervals, the cards show no
figures at all, only a line such as:

> **No regular beat found (timing scatter 8.6 ms, 5.2% of a beat)**

The timing scatter is how far the beats land from a straight-line fit.
When it is 2% of a beat or more, which is about what an empty stand
gives (around 5%), any rate, amplitude or beat error would describe noise,
so none is shown. When nothing in the session has had a regular beat after
its first few seconds, the cards and charts give way to one card saying
the same, and they appear once a regular beat is found.

This happens when the watch isn't running, when the microphone doesn't
hear it, or when noise drowns it out.

## The Clipping Banner

Whenever a beat in the readings has a clipped sample in its sound, an
amber banner above the readings says **Clipping on 3% of beats** (or
"under 1%"). See
[Setting Up the Microphone](03-microphone.md#setting-the-level) for what
clipping does and how to stop it. Even a heavily clipped sound is still
read, with the banner, rather than taken for silence.

---

Previous: [The Window](04-the-window.md) ·
Next: [The Paper Strip](06-paper-strip.md)
