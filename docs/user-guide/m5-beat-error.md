# M5. Beat Error

## What It Is

The **beat error** is how unevenly the two halves of the balance's swing
are timed: half the difference between the Tick-to-Tock and Tock-to-Tick
intervals, in milliseconds.

If one interval is 124.7 ms and the next 125.3 ms, the beat error is
half of 0.6 ms: **0.30 ms**.

It is signed: **+ when the Tick comes late**. The cards and charts show
its size; the sign is in the command line's JSON.

## How It Is Worked Out

There are two figures, timed from two sounds.

**From the unlock** (the big figure):

1. In each 2 s window, the Ticks' sounds are averaged into one shape and
   the Tocks' into another (as for amplitude, M4).
2. The unlock is found on each shape, and timed against the side's beat
   times.
3. The window's beat error is the drop's beat error (below), less the
   Tick's unlock-to-drop time, plus the Tock's.
4. The reading is the **median** of its windows.

**From the drop** (in the Beat Error card's hover):

- The rate fit (M2) puts a line through every beat, with Ticks offset
  `+e/2` and Tocks `−e/2`. `e` is the beat error. A beat's time sits near
  its drop, so this is the beat error timed from the drop.

**Which is shown:** the unlock's, where the unlock was found; else the
drop's, with the chip **Beat Error From the Drop**. The drop's is also
shown instead of the unlock's when the unlock's windows disagree in sign
more than their spread allows: fewer of them fall on the median's side
than the median and their spread predict. A faint unlock found on noise
in part of the windows reads so, a cluster on each side of zero. A beat
error near zero predicts only about half its windows on its side, so a
scatter around zero is not taken for this. With fewer than 3 windows it
is not judged.

**On a co-axial escapement** (beta, chapter 24): the unlock's figure
only. There is no beat error from the drop, in the hover or as a stand-in,
since one side's impulse goes straight to the balance and the other's
through the lever, so the two sides' last sounds are different events.
The Beat Error card says **Co-axial (beta)**. Where the unlock isn't
found, or its windows disagree in sign as above, no beat error is given,
and the chip says which.

**Which side is Tick:** the sound can't tell which pallet stone is which.
Each take starts positive: the side that comes late over the first few
seconds is called Tick. With a beat error nearer zero than 0.1 ms, the
first beat heard is Tick. A beat error that reads negative later has turned
over during the take.

**As an angle (BE Angle):** when the escapement's mid point sits a few
degrees off the balance's centre, one beat is longer and the other
shorter, each by twice the time the balance takes to cover that offset.
The beat error, half the difference between the beats, is so twice that
time. Near its centre the balance swings at its fastest, amplitude × 2π
× frequency degrees a second, with frequency = beat rate / 7200 Hz.
Half that speed times the beat error gives the offset as an angle:

BE Angle (°) = amplitude (°) × π × beat rate / 7200 × beat error (s)

At 28,800 bph, 0.46 ms at 302° is 1.7°, and 0.47 ms at 129° is 0.76°.
It keeps as many significant figures as the fewer of the beat error's
and the amplitude's, as each is written, and its ± takes their two
relative ±s together (in quadrature), rounded up at its place:
0.46 ± 0.05 ms at 302 ± 2° is 1.7 ± 0.2°. The app shows it beside the ms
in the Beat Error figure's hover (chapter 5) and in the Power Reserve
hour table (chapter 21), never in place of the ms.

## Why This Way

- **From the unlock:** the open tg program times the beat error from the
  unlock, and the app is checked against tg in its regression tests, so
  the two can be compared.
- **The drop's figure in the hover:** the drop is louder, so its timing is
  steadier. When the two figures differ, the two sides' unlock-to-drop
  times differ.
- **Signed, but shown as a size:** which side is "Tick" is the app's label,
  so the sign alone says nothing about the watch. A change of sign within
  a take means the two sides' timing turned over, or that Tick and Tock
  swapped names across a break.
- **An angle beside the ms:** the ms grow as the amplitude falls, even
  when nothing in the escapement has moved, because the balance passes
  its centre more slowly. The angle stays the same, so a beat error that
  grows only in ms over a run down is the amplitude, not the escapement.
  The angle rests on the lift angle through the amplitude, so a lift
  angle 7% off makes it 7% off; the ms does not.

## When You'd Use It

- **Setting the beat:** as you adjust it, watch the figure fall. A short
  Average Over follows it quickly.
- **Across positions:** a beat error that changes a lot between positions
  is worth a note.
- **Over a take:** the Steadiness panes show whether it holds.

## What It Helps Find

The beat error shows whether the balance's rest point sits centred in
the escapement, so that the impulse pin is at the fork's centre when the
hairspring is at rest.

## What It Can't Tell You

- **How the watch will keep time.** The rate fit takes the beat error
  out of the rate, so the two are read apart.
- **Through a poor unlock.** Like amplitude, the unlock's figure rests on
  a quiet sound. A muffled or clipped sound moves it.
- **Across a break, a sign.** If the sign differs either side of a cut,
  the whole take gives a size and says so: averaging + and − would read
  near zero.
- **Which pallet stone is late.** Only that the two sides differ.

---

Previous: [M4. Amplitude and Lift Angle](m4-amplitude-and-lift-angle.md) ·
Next: [M6. Averaging, Windows and Cuts](m6-averaging-windows-and-cuts.md)
