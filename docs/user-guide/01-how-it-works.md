# 1. How a Timegrapher Hears a Watch

A timegrapher listens to a watch through a microphone pressed against its
case, picks out each beat, and times the beats against a clock it trusts.
Everything it reports comes from two things: **when** each beat arrives,
and **what** each beat sounds like. This chapter explains both, so the
figures in the rest of the guide make sense.

## The Beat

Inside a mechanical watch the balance wheel swings back and forth on its
hairspring. Twice in each full swing, once each way, it knocks the lever
across, the lever lets the escape wheel turn one step, and the escape wheel,
through the lever, gives the balance a small push to keep it swinging. Each of those
moments is a **beat**: the "tick" you hear when you hold a watch to your
ear.

The two beats of a swing are made by different parts of the escapement,
so they sound slightly different and can be timed separately. Timegrapher
calls them the **Tick** and the **Tock**. The sound alone cannot tell
which pallet stone made which beat, so the Tick is simply the first beat
the software hears. Ticks are drawn in blue and Tocks in orange throughout
the app.

## The Three Sounds of a Beat

Listen closely, at the scale of milliseconds, and each beat is three
sounds in quick succession:

1. **Unlock**: the pallet stone lets go of the escape wheel tooth.
2. **Impulse**: the escape wheel tooth pushes the lever, which pushes the
   balance.
3. **Drop**: the next escape wheel tooth lands on the other pallet stone.
   This is usually the loudest of the three.

![A sketch of one beat's loudness over a few milliseconds: a small unlock, a medium impulse and a large drop, with the time from unlock to drop marked](images/beat-sounds.svg)

The [Tick Tock Profile](07-sound-of-the-beat.md) in the app shows these
sounds as the software actually heard them, with the unlock and drop it
measured marked on the plot.

## Beat Rate

The **beat rate** is how many beats the watch makes in an hour, counting
Ticks and Tocks alike, in beats per hour (bph). A balance that swings
back and forth 4 times a second makes 8 beats a second, which is 28,800
bph. Other common rates are 21,600 bph (3 swings a second), 18,000 bph
(2.5) and 36,000 bph (5). The beat rate is fixed by the watch's design.

Timegrapher finds the beat rate by itself from the first few seconds of
beats (**Beat Rate: Auto**), or you can set it. At 28,800 bph a beat is
due every 125 milliseconds; at 21,600 bph every 166.7 ms.

## Rate

The **rate** is how many seconds a day the watch would gain (+) or lose
(−) if it kept running exactly as it did while it was measured, in seconds
per day (s/d).

A timegrapher does not wait a day to find out. It compares each beat's
arrival with a perfect clock beating exactly at the beat rate. A watch
that gains 10 s/d at 28,800 bph brings each beat about 0.015 ms earlier
than the one before (10 s in 86,400 s, of 125 ms). Over a few seconds of
beats that adds up to a steady drift, which Timegrapher measures by
fitting a straight line through the time of every beat. The
[Paper Strip](06-paper-strip.md) draws that drift.

The rate is only as good as the clock the beats are timed against. The
software times them by the computer's sound card, whose own crystal
typically runs 10 to 50 parts per million off, which moves every rate by
1 to 4 s/d. tg and other software timegraphers read the rate on the same
clock. The app can measure your sound card's error and show the rate
corrected for it, labelled **True Clock**: see
[Readings](05-readings.md#true-clock).

## Amplitude

The **amplitude** is how far the balance swings each way from its rest
position, in degrees.

The microphone cannot see the balance, so the amplitude is worked out
from the sound. While the balance is in contact with the lever, between
the unlock and the drop, it turns through a fixed angle set by the
calibre's design: the **lift angle**. The faster the balance swings, the
less time it spends crossing that angle. So, from the time *t* between
unlock and drop, the time *T* of one full swing of the balance (two
beats), and the lift angle *L*:

> amplitude = *L* ÷ (2 × sin(π × *t* ÷ *T*))

This is the formula Witschi's instruments and tg use. For example, in the
[Tick Tock Profile](07-sound-of-the-beat.md) screenshot, the Tick's
unlock-to-drop time is 8.48 ms at 28,800 bph (*T* = 250 ms) with a lift
angle of 55°: 55 ÷ (2 × sin(π × 8.48 ÷ 250)) ≈ 259°.

Two things follow from the formula:

- **The lift angle matters.** A wrong lift angle moves every amplitude in
  proportion: 1° of lift angle moves the amplitude by about 4°. The app
  uses 52° when it doesn't know the calibre and says so beside the
  reading. [Calibres and Lift Angles](09-calibres.md) explains how to set
  it, and where the figures come from.
- **The unlock has to be heard.** The unlock is the quietest of the three
  sounds. A noisy room, a muffled microphone or a soft unlock makes it
  harder to place, and the amplitude less certain.

The app shows the amplitude measured from the Ticks and from the Tocks
separately, as well as their average.

## Beat Error

The **beat error** is how unevenly the Tick and the Tock are spaced, in
milliseconds. If the balance's rest position sits exactly midway between
the points where it unlocks the lever each way, the Ticks and Tocks come
evenly, half a swing apart, and the beat error is zero. If it sits to one
side, one beat comes a little early and the next a little late.

Timegrapher reports the beat error timed from the **unlock**, as tg and
commercial timegraphers measure it, and beside it, smaller, the beat
error timed from the beat as a whole, nearer the drop. The two differ
when the Tick and the Tock take different times from unlock to drop. On
the Paper Strip the beat error is the gap between the line of Ticks and
the line of Tocks.

Because which beat is the Tick depends on where the software first locks
on, the app shows the beat error as a size, without a sign.

## Jitter and Other Figures

Beyond these three, Timegrapher measures things most timegraphers don't:

- **Jitter**: how far single beats land from the straight line the rate is
  fitted to, in microseconds. It is the spread of the dots on the Paper
  Strip. The watch, background noise, the microphone and the stand all
  add to it.
- **How each figure changes over time**, from a few seconds to many hours,
  and whether a change repeats with the turn of a particular wheel. See
  [Over Time](08-over-time.md).
- **The shape of each beat**: the timing and loudness of the three sounds
  on each side. See [The Sound of the Beat](07-sound-of-the-beat.md) and
  `timegrapher shape` in [The Command Line](12-command-line.md).
- **Positions**: the same figures in each of six positions, and the
  differences between them that watchmakers use. See
  [Positions and the Positions Test](10-positions.md).

## What Every Figure Carries

Every figure comes with a **±**: an estimate of how far it could be off
from the scatter of the beats alone. A longer measurement gives a smaller
±. The ± is a property of the measurement, not of the watch, and it
cannot show everything: a wrong lift angle, for instance, moves the
amplitude without changing its ±. So the app also names, beside each
reading, anything it knows limits it: a short stretch of beats, missing
beats, clipped sound, an assumed lift angle. When the beats it finds
don't come at regular intervals at all, it shows no figures, only
**No regular beat found**, with the timing scatter it measured.

[Readings](05-readings.md) explains each of these in turn.

---

Next: [Your First Measurement](02-first-measurement.md)
