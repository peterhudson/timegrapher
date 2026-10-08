# Long runs: reading `timegrapher long`

A few minutes on a timegrapher tells you how the watch is doing now. An
hour or a day tells you how it behaves: how the rate and amplitude change
as the mainspring runs down, and whether anything repeats with the turn
of a wheel. A damaged tooth, a worn pinion, an eccentric wheel or a hand
that catches all repeat once per turn of their wheel, and that's how they
give themselves away.

## Recording a long run

- One position, usually dial up (the easiest to keep still), or the
  position where the problem showed.
- Fully wound at the start, unless you're deliberately looking at the end
  of the reserve.
- Don't touch it. A knock, a door slamming, someone picking up the stand:
  each upsets the run.
- **Keep a clock log** if you care about the absolute rate (see below).
- Length: at least 30 minutes for the fourth wheel (once a minute), and an
  hour or two is better; at least 6 hours for the centre wheel; a day or
  more for the barrel and the whole reserve. A component needs to repeat
  several times to be told from the watch's ordinary wandering: five turns
  of the fourth wheel in a 5-minute take is too few.

Recording commands are in
[setup-and-microphone.md](setup-and-microphone.md#recording).

## Running it

```sh
timegrapher long run.flac --clock run-clock.csv --lift 52
timegrapher long seg01.flac seg02.flac seg03.flac      # one capture in segments, in order
timegrapher long capture-folder/                       # every WAV/FLAC in it, by name
timegrapher long run.flac --wheel "third wheel=450"    # name another wheel you know the period of
timegrapher long run.flac --json                       # the summary as JSON
```

Results go to a folder next to the recording (`run_long/`, or `--out DIR`):
`report.html` (open it in a browser: rate and amplitude over the run, the
period searches, and for each component its average cycle and a raster
with one row per cycle), `summary.json`, and CSVs of every beat,
amplitude, slices and the folded cycles. Details in
[cli-reference.md](cli-reference.md#long).

## Reading the summary

```
Recording    30:00 at 48000 Hz, 28800 bph, 14400 beats (100.0% clean)
Clock        not calibrated: absolute rate is only as good as the sound card (use --clock)
Rate         +4.00 s/d over the run; 10 s slices from -19.1 to 8.1 s/d (5th-95th pct)
Amplitude    268 deg median; slices from 253 to 268 deg (lift angle 52 deg)
Periodic changes in rate:
      60.00 s ±2.00   swing 36.8 s/d p-p     explains 100%  false alarm <1e-300  matches the fourth wheel
Periodic changes in amplitude:
      60.03 s ±2.00   swing 16.4 deg p-p     explains  42%  false alarm 4e-52    matches the fourth wheel
```

(That one is synthetic: a watch made to lose 30 s/d and 15° for 8 s of
every minute.)

- **Clean percentage.** Beats matched well. Under about 95%, something
  disturbed the recording or the watch's sound changed; treat the rest
  with care.
- **Clock.** Whether the sound card's error was corrected, and by how
  much.
- **Rate.** The average over the run, and how far the rate wandered from
  slice to slice. A wide 5th–95th range is the "wandering trace".
- **Amplitude.** Median and range over the run. On a full-reserve run,
  the fall from start to end is the isochronism story (see
  [reading-the-numbers.md](reading-the-numbers.md#isochronism-full-wind-against-24-hours)).
- **Periodic changes.** Each line is a component found:
  - **period ± resolution**: how long one cycle is, and how finely this
    run can tell periods apart (a longer run resolves better);
  - **swing**: peak to peak size of the average cycle, s/d or degrees.
    It reads high on a sharp step (the cycle is smoothed to a few
    harmonics, which overshoot);
  - **explains**: how much of the wandering (after slow drift is removed)
    this cycle accounts for. 5% is a ripple; 40% or more dominates;
  - **false alarm**: the chance noise alone would have produced a
    component this strong anywhere in the search. Only components under 1%
    are listed. 1e-5 is convincing; 1e-50 is beyond doubt;
  - **wheel**: which wheel turns at that period, or the nearest one and
    how far off. A 57 s cycle in a two-hour run (resolution ±0.5 s) is
    *not* the fourth wheel.

The **average cycle** in the report shows the *shape*: a sharp notch once
per turn (one tooth, one leaf, something catching at one point) or a
smooth wave (eccentric wheel, bent arbor, worn hole). The **raster** shows
whether every turn does the same thing (a fault on the wheel) or it comes
and goes.

### Which wheels are listed

By default: the escape wheel (2 × teeth beats; set `--escape-teeth` if it
isn't 15), the fourth wheel (60 s) and the centre wheel (3600 s). The third
wheel and the barrel vary by calibre: add them with `--wheel` when you know
them. Barrel periods (hours) are weakened by the trend removal in runs of
a day or less; they need a run of days.

## Calibrating the sound card

A sound card's crystal is typically 10–50 ppm off, which is 1–4 s/d,
and drifts with temperature. For the shape of things (amplitude, periodic
components, rate wander) it doesn't matter. For an absolute rate good to
better than a few s/d, it does.

`--clock` takes a log written during the recording: a timestamp from the
NTP-synchronised system clock against how much audio had been captured,
one line every minute or so. The program fits one against the other and
corrects every beat. The `Clock` line then says how far off the sound card
was. Format details are in the project's `docs/long-runs.md`; a simple
logger is in [setup-and-microphone.md](setup-and-microphone.md#recording).

## Worked example: a Rolex 3235 over two hours

This is a real run from this project, and a good example of how to reason,
not a solved case.

**The watch.** Rolex calibre 3235 (Yacht-Master), 28,800 vph, dial up, 2
hours, C-Media USB stand fixed to no clipping, clock log kept.

**What `long` found.**

- Amplitude dropped by about **8° once every 60.0 s**, matching the
  **fourth wheel** (the seconds wheel). False-alarm chance about 1e-65:
  certainly real.
- The dip came while the **seconds hand was at about 42–50 s** on the
  dial.
- The rate **wandered between about +1 and +52 s/d** over tens of
  seconds. The mean, after correcting the sound card (running about 19
  ppm slow, which reads about 1.7 s/d fast), was **+16.6 s/d**.

**Reasoning.**

1. Is it the measurement? The signal was checked clean, the run was
   undisturbed, and a 60.0 s period with a false-alarm chance that small
   isn't room noise. Nothing in a room repeats exactly once a minute
   (unless a clock is ticking near the microphone; ask).
2. Is it a fault or normal meshing? Witschi says meshing alone can vary
   amplitude by up to 30° across the train and that's normal. An 8° dip
   once a minute is within that, on its own. What makes it interesting is
   that it is **lined up with one part of the seconds hand's turn**, and
   the rate wander is large for a calibre held to −2/+2 s/d.
3. What turns once a minute? The fourth wheel, and with it the seconds
   hand. So the candidates are:
   - a damaged or dirty **tooth on the fourth wheel**, or a leaf of the
     escape pinion it drives, or of the fourth pinion;
   - the **seconds hand rubbing** on something during that part of its
     turn: the dial, the crystal, or the hour or minute hand.
4. What would tell them apart?
   - Run again with the hands set to a different time. If the dip moves
     to a different point in the minute, the seconds hand is catching the
     hour or minute hand. If it stays at 42–50 s, it's the dial, the
     crystal or the wheel.
   - Look closely with a loupe at the seconds hand around 42–50 s for
     contact with the dial or a sign of a bent hand.
   - A watchmaker can take the hands off and run it again: a dip that
     vanishes was the hand; one that stays is in the train.
   - The average cycle's shape: a sharp notch fits one tooth or a rub at
     one point; a broad dip fits a bent or eccentric wheel.
5. Verdict for the owner: the watch runs, the amplitude is moderate for a
   3235, the mean rate dial up is well beyond what a −2/+2 s/d watch
   normally shows (one position isn't the specification, but +16.6 is a
   long way off), and there's a
   repeatable once-a-minute disturbance. That's **service soon** (it's
   under warranty if recent, so back to Rolex), with a note of exactly what
   was measured. Not "stop wearing it".

What not to say: "the fourth wheel has a broken tooth". We don't know
that. We know something turning once a minute takes 8° off the amplitude
for a few seconds of each turn.

## Limits

- So far validated on synthetic recordings; the 3235 runs are the first
  real ones.
- One tick template from the start of the run is used throughout; a
  watch whose sound changes a lot late in the reserve may lose beats late
  on (the clean percentage shows it).
- Periods longer than about 15 minutes are weakened by the 30-minute
  trend removal; the barrel needs a separate slow look over days.
