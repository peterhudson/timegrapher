# M10. Finding Cycles

## What It Is

The **period search** looks for periods the readings repeat at, scores how
sure it is of each, and names the wheel each one matches. Its scores are
what Periods draws (chapter 11), its lines are what the headline names,
and its cycle is what Cycle folds (chapter 12).

## How It Is Worked Out

1. **The watch's own clock.** Each beat is placed at its number times the
   nominal beat period, not at the time the sound card heard it. A wheel
   turns once in a fixed number of beats: the fourth wheel once a minute
   is 480 beats at 28,800 bph, and the escape wheel's turn is twice its
   teeth in beats:

   ```text
   escape wheel period = teeth × 2 ÷ beats per second
   20 teeth at 28,800 bph (8 beats a second):  40 ÷ 8 = 5 s
   15 teeth at 28,800 bph:                     30 ÷ 8 = 3.75 s
   15 teeth at 21,600 bph (6 beats a second):  30 ÷ 6 = 5 s
   ```

   On this clock a wheel's period is exact, and its cycle doesn't smear
   when the rate changes.

2. **The series.** For the rate, the **timing offset** of every clean beat
   from a steady rate, in 0.25 s bins (0.5 s for takes over 12 hours, 4 s
   over 80 hours). For the amplitude and the beat error, their 2 s
   windows.

3. **Slow change off.** A running median over 30 minutes is taken off
   (a quadratic on a take under 20 minutes), so the mainspring running
   down isn't scored as a long cycle. For the rate, a local straight
   line is taken off first, so a rate far from the take's average
   doesn't take its cycles with it. The take is cut at every stop and
   restart, and each piece is treated on its own.

4. **Score.** The spectrum is divided by the shape the series' own
   noise gives (AR(1), a first-order autoregressive model in which each
   reading carries a share ρ of the one before, for readings; a random walk plus white
   jitter for the beat timing) and by its local median, and each trial
   period sums its first **1, 2, 4 or 8 harmonics**:

   ```text
   score(P, h) = Σₖ₌₁ʰ (normalised power at period P/k)
   ```

   Each sum is scored against the spread the same steps give on
   simulated noise of the take's length and step, and the best score
   against how high the best score over the whole search runs on that
   simulated noise: the false-alarm chance for the whole search. Under
   0.0003, past what the simulation reaches, the text says "beyond the
   simulated range".

5. **Listed** when the false-alarm chance is under 1% across the whole
   search (the dashed line in Periods), the cycle explains at least 0.5%
   of the series' variation, and it passes Holm's correction with every
   other test of the take (M9).

6. **Fold and take off.** The strongest period is refined, the series is
   folded at it (the median at each point of the turn), that average
   cycle is taken off, and the search runs again on what is left.

7. **Name.** A line matches a wheel when their periods agree to within 1%,
   or to the take's resolution, whichever is wider:

   ```text
   resolution = P² ÷ take length
   ```

   At 60 s in a 2-hour take that is 0.5 s. Otherwise the nearest wheel and
   how far off it is are given.

## Why This Way

- **Harmonics:** a smooth swing puts all its power at its period. A short
  event once a turn (a tooth catching, a dip for a few seconds) spreads it
  over many harmonics, and a search at one period would miss it. Summing
  1, 2, 4 or 8 catches both; twice the true period scores worse, since half
  its harmonics are noise.
- **Folding and taking off:** wheels' periods are whole-number ratios of
  each other (60 s, 30 s, 20 s…), so one fault shows at several periods.
  Taking off each cycle found keeps it from being listed again.
- **Dividing by the noise shape:** watch readings wander, so a raw
  spectrum climbs towards long periods; without this, every long period
  would look like a line.
- **The beat timing for the rate:** a 10 s rate reading can't show any
  cycle faster than 20 s. The timing of each beat can: the escape wheel's
  swing is a fraction of a millisecond, beat by beat. A cycle under 30 s
  is given as the timing swing in ms; a slower one as the rate's swing,
  in s/d.
- **The resolution:** a take holding N turns of a period can only place
  it to about one part in N.

## When You'd Use This

- **Matching a line to a wheel** whose period is off by a little.
- **Telling a smooth swing from a once-a-turn knock:** which harmonic
  count scored best.
- **Planning a take:** a cycle needs several turns inside the take.

## What It Helps Find

- **A wheel that catches or rubs once a turn,** at that wheel's period
  or its harmonics.
- **An off-centre wheel or pinion,** at half the period (twice a turn).
- **A disturbance from outside the watch** (the room, the stand) at a
  period no wheel has.

## What It Can't Tell You

- **Which part.** A line at a wheel's period says the readings repeat with
  that wheel's turn: the wheel, its pinion, its arbor or the hand it
  carries.
- **Long periods.** On the amplitude and beat error, periods over about
  15 minutes, half the running median, are weakened by it. On the rate the
  search reaches up to 36 minutes, where half a cycle is still kept. A barrel's turn of hours is beyond it.
- **Periods under the shortest searched:** 3 s for the rate's timing up
  to 12 hours, 4 s from 12 to 80 hours and 32 s beyond (the longer of
  3 s and eight bins of the series), and 16 s for the amplitude and beat
  error. A wheel faster than that can fold into slower cycles that
  aren't there.
- **A guarantee.** The 1% is calibrated on simulated noise of those two
  kinds. Noise of another kind (bursts, flicker) can pass or fail
  differently; the timing-misfit caveat says when the beat timing
  doesn't fit its model.

The project's [long-run notes](../reference/long-runs.md#how-the-search-works) give
the search in full.

---

Previous: [M9. Steadiness Statistics](m9-steadiness-statistics.md) ·
Next: [M11. Time and Frequency Maps](m11-time-and-frequency-maps.md)
