# M4. Amplitude and Lift Angle

## What It Is

The **amplitude** is how far the balance turns from rest to the end of its
swing, in degrees.

The **lift angle** is the arc of the balance's swing during which the
impulse pin is in the fork: from the unlock to the drop. It is set by the
movement's design. The calibre table gives it where it is known.

## How It Is Worked Out

The balance swings like a pendulum: its angle follows a sine wave of
amplitude `A` and period `T`, one full oscillation, two beats. Near the
middle of its swing it passes through the lift angle `L`, centred on the
rest point. The time `t` it takes is short when the swing is wide, since
the balance is moving fastest there.

Solving for `A` gives the formula Witschi's timegraphers (Witschi,
_Testing Methods for Mechanical Watches_, 2025, p. 12) and the open tg program use:

```text
A = L ÷ (2 × sin(π × t ÷ T))
```

**An example.** At 28,800 bph, `T` is 250 ms. With a lift angle of 52°
and an unlock-to-drop time of 8.0 ms:

```text
A = 52 ÷ (2 × sin(π × 8.0 ÷ 250)) = 52 ÷ (2 × 0.1004) ≈ 259°
```

**Timing the unlock and the drop:**

1. Beats are grouped into **2 s windows**. In each, the Ticks' sounds are
   averaged into one shape, and the Tocks' into another. Each side needs 3
   good beats. The average keeps the middle half of the beats at each
   moment, which ignores stray clicks.
2. On each shape, both edges are timed where a sound crosses half its
   own height:
   - **The drop** is where the rise into the drop's peak crosses half
     the drop's height. It is found by walking back from the peak, at
     most 2 ms, so an earlier sound near that height isn't taken for it.
   - **The unlock:** the first sound is found as the first steady rise
     above the background by 2% of the drop's height (or 4 times the
     background's noise, if that is more). It is timed where it crosses
     half its own height.
3. Each window gives a Tick and a Tock amplitude. The window's amplitude is
   their mean. A window with one side missing is left out of the reading,
   and kept in that side's own figure.
4. The reading is the **median** of its windows' amplitudes.

## Why This Way

- **Half-height crossings on rising edges, not peaks:** an edge's time is
  sharper and steadier than a peak's, which moves with the sound's shape.
  Taking each sound at half its own height keeps a quiet unlock from
  being timed by the drop's loudness.
- **Averaged beats:** a single beat's unlock is too faint to place.
  Averaging a few seconds of beats brings it out of the noise, while still
  following an amplitude that changes.
- **The median of windows:** an edge marked on the wrong sound moves one
  window, not the reading.

## When You'd Use It

- **After service,** to compare amplitude before and after, in the same
  position and state of wind.
- **Across positions,** to compare the swing lying flat and standing
  (chapter 20).
- **Over the power reserve,** to see the swing fall as the mainspring
  runs down (chapter 21).
- **Against the rate,** to see whether the rate follows the amplitude
  (isochronism, chapter 19).

## What It Helps Find

The amplitude shows how much energy reaches the balance. A low or falling
amplitude can come from the mainspring, the train, the escapement's
lubrication or wear, or the balance's pivots. The amplitude alone doesn't
say which.

## What It Can't Tell You

- **Without the right lift angle, the true amplitude.** Every amplitude is
  in proportion to the lift angle: 52° used for a 55° movement reads every
  amplitude about 5% low. With no calibre set, the app uses 52°, and Lift's
  hover on the Calibre card says it is the default.
- **Through a poor unlock.** The amplitude rests on timing a quiet sound.
  A 1 ms error in the unlock-to-drop time moves it a lot: at 28,800 bph and
  52°, 8.0 ms reads 259°, and 7.0 ms reads 296°. A muffled holder, clipping
  or automatic gain moves the unlock, and with it the amplitude.
- **A co-axial's amplitude, checked.** A co-axial escapement (beta,
  chapter 24) is read with the lever's formula, at the lift angle set for
  it, and the Amplitude card says **Co-axial Escapement Estimate (beta)**.
  Where its lift angle is smaller than 52°, such as the 8500's 38°, the
  52° default reads it high: about 37% high for 38°. No co-axial reading
  has yet been checked against a reference.
- **The swing's two halves exactly.** The Tick and Tock amplitudes in the
  hover can differ because the two sides sound different, not because the
  balance swings further one way.

---

Previous: [M3. The Sound Card's Clock](m3-the-sound-cards-clock.md) ·
Next: [M5. Beat Error](m5-beat-error.md)
