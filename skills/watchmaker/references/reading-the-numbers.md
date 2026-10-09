# Reading the numbers

What each figure means, why it matters, and what good, fair and poor look
like. Read the numbers in this order: is the measurement sound, amplitude,
lift angle, beat error, rate, the trace, then the positions.

Figures in this file are a working watchmaker's rules of thumb, not
factory tolerances. Where a maker publishes its own figures for a calibre,
theirs win.

## First: is the measurement any good?

From `timegrapher analyze --json` (field names in
[cli-reference.md](cli-reference.md)):

- **Beat rate (`bph`)** must be one the calibre runs at. If it guessed
  14,400 for a modern watch, it has latched onto every other tick; give
  `--bph` explicitly.
- **Beats found (`beats_found`)** should be close to duration × bph ÷ 3600.
  A big shortfall means the microphone is missing ticks.
- **Tick and Tock amplitudes** (`amplitude_even_deg`, `amplitude_odd_deg`:
  the even beats are the Tick, the odd the Tock) should agree within a few
  degrees. A big split on a healthy-sounding
  watch usually means the unlock sound is being missed on one side (level
  too low, or clipping; `profile` shows which sounds were marked);
occasionally it's a real escapement asymmetry
  (see [tick-shape.md](tick-shape.md)).
- **Jitter (`jitter_us`)**: scatter of single beats about the local rate.
  Compare it between recordings of the same watch; a sudden jump with
  nothing else changed points at the set-up (loose clamp, noise).
- **Absolute rate is only as good as the sound card's clock.** A sound
  card is typically 10–50 ppm off, which is 1–4 s/d. For a quick look that
  doesn't matter; for regulating to a chronometer standard, or for long
  runs, calibrate with a clock log (see [long-runs.md](long-runs.md)).

If any of this is off, go back to set-up before reading the watch.

## Amplitude

**What it is.** How far the balance swings either side of its rest point,
in degrees. A balance swinging 270° turns three-quarters of a revolution
each way.

**Why it matters most.** Amplitude is the energy that arrives at the
balance after the mainspring, the gear train, the oil and the escapement
have each taken their share. Dirty or dried oil, a weak mainspring, worn
pivots, a dragging hand: they all show up as lost amplitude before they
show up anywhere else. And the rate is steadier at a healthy amplitude,
because the hairspring's errors and the escapement's disturbance matter
less when the balance is swinging freely.

**How it's measured.** The timegrapher times the gap between the first
sound of a tick (the unlock) and the last (the drop). During that gap the
balance is passing through the lift angle. Knowing the lift angle and the
beat rate, the swing follows: `A = L / (2 sin(π t / T))`. So a wrong lift
angle gives a wrong amplitude, and a missed unlock sound gives a wildly
wrong one.

**Typical values, fully wound** (28,800 vph modern Swiss). Other calibres differ: Seiko's 21,600 vph movements, for one, normally run 20–30° lower. See the calibre table in [when-to-see-a-watchmaker.md](when-to-see-a-watchmaker.md#what-normal-looks-like-for-the-calibre) before judging:

| | Dial up / dial down | Vertical (crown positions) |
| --- | --- | --- |
| Good | 270–310° | 240–280° |
| Fair | 230–270° | 200–240° |
| Poor, wants a service | under about 220° | under about 200° |
| Suspicious, check it | over about 320° | over about 300° |

Witschi's tolerance for a fully wound watch is 260–320° horizontal and
240–280° vertical. A fall of 20–50° from horizontal to vertical is normal:
in the vertical positions the balance pivots run on their sides in the
jewel holes, which is more friction than running on their tips on the end
stones.

**Older, slower watches** (18,000 and 21,600 vph, 1950s–1970s): healthy
figures run a little lower, roughly 240–290° dial up fully wound, and a
well-serviced vintage watch at 220–250° is often perfectly good (check).
Judge them against their own kind.

**Known low-amplitude calibres.** Some modern calibres are known to run
lower than the textbook. Rolex's 32xx family (3230, 3235, 3255) is widely
reported running low after 24 hours, with Rolex reportedly allowing about
200–310° after 24 h (check: secondary sources only). Don't condemn one of
those on amplitude alone; look at the trend and the trace.

**Too high.** Over about 320° dial up, check in this order:

1. The lift angle (too high a lift angle inflates amplitude).
2. The measurement: a missed unlock or a doubled sound reads as huge
   amplitude. `timegrapher shape` will show it.
3. Real high amplitude: a mainspring too strong for the calibre, or a
   movement short of oil (oil is sticky and costs energy at this
   scale; its main job is to stop wear, so a dry movement can show high
   amplitude while it wears). Around 320–340°, depending
   on the calibre, the impulse pin can strike the outside of the fork horn:
   **knocking**, or rebanking. It upsets the rate and can damage the
   escapement. Witschi gives over 330°.

People who know the trick can make a tired watch look good on a photo
by raising the lift angle on the machine. Always ask what lift angle was
set.

## Lift angle

**What it is.** The angle the balance turns through while the impulse pin
is in the fork's slot, from unlock to drop. It is fixed by the escapement's
design.

**Why it matters.** Amplitude scales with it. 2° out at 52° is about 4%:
about ±11° at 280°. 5° out is about ±25°. It doesn't affect rate or beat
error.

Most modern Swiss-lever calibres are in the high 40s to low 50s; Witschi
says "about 51°" for most modern movements, and 52° is the default on
most cheap timegraphers.

### First, find out what the movement really is

Brands decorate and rename common calibres. A "manufacture calibre XYZ"
from a microbrand is very often an ETA 2824-2 or Sellita SW200 with a
custom rotor, and Tudor, Longines, Hamilton and others put their own
numbers on ETA bases. Look the watch's calibre up on **Caliber Corner**
(calibercorner.com) to find its base movement, then use the base's lift
angle and beat rate. Caliber Corner also gives the base's specifications,
such as the ETA 2824-2 grades.

For the lift angle itself, the most complete list is **WatchGuy's**
(watchguy.co.uk/cgi-bin/lift_angles), with well over a thousand calibres by
maker. Look the calibre up there when you can fetch web pages. Where it and
the table below disagree, prefer the table for the calibres the table
names: they come from the bench.

| Calibre | Lift angle | Note |
| --- | --- | --- |
| ETA 2824-2, Sellita SW200-1 | 50° | |
| Rolex 31xx (3135, 3035 and family) | 52° | |
| Rolex 32xx (3230, 3235, 3255) | 55°, possibly 58° | Rolex doesn't publish it; 55° is the best estimate. At 52° a 32xx reads about 13° low at 230° |
| Dandong (DD) and JH clones of the 32xx | 55°, possibly 58° | As the genuine |
| Shanghai (SH) and VR clones of the 32xx | 52° | Same escapement as the 31xx |
| DD, VR and SH clones of the 31xx | 52° | |
| Rolex 4130; SH 4131 | 52° | |
| DD 4131 | 55° | Probably, from the pallet fork geometry; not measured |
| Miyota 9015 | 51° | |
| Patek 324 and 240 (and clones) | 52° | |
| AP 3120 | 53° | |
| AP 4302 | 52° | |
| AP 4401 (chronograph) | 50–52° | Not documented for the genuine; a clone may differ |
| ETA 2836-2 | 50° | WatchGuy |
| ETA 2892-A2, 2893-2 | 52° | WatchGuy |
| Valjoux/ETA 7750 | 49° | WatchGuy; check the SW500 separately |
| ETA/Unitas 6497 | 44° | WatchGuy |
| Seiko NH35/NH36, 4R35/4R36, 7S26 | 53° | Commonly quoted; check WatchGuy |
| Miyota 8215 | 50° | Commonly quoted; check WatchGuy |
| Omega co-axial (8500, 8800, 8900 and others) | 38° | Commonly quoted. A co-axial escapement doesn't make the Swiss-lever three-sound tick, and this program's amplitude assumes a Swiss lever, so treat co-axial amplitude as unreliable |

Rule of thumb: each degree of lift angle moves the amplitude by about
5–7°, so correct a reading made at the wrong lift angle by about 5° per
degree. A lift angle of 62–65° set on someone else's machine is a sign of
amplitude hacking.

If the calibre is unknown: use 52°, and say in the answer that the
amplitude might be 10–15° out either way. The *changes* between positions
and between full wind and 24 hours are still right, because the error is
the same in every reading.

## Beat error

**What it is.** The balance should swing equally either side of the
point where the escapement acts. If the hairspring is pinned slightly
off, the tick and the tock come at unequal intervals. Beat error is half
the difference, in milliseconds. On a trace it's two parallel lines
instead of one.

**Why it matters.** Within reason, not much for timekeeping. But a watch
well out of beat needs more swing to unlock on its weaker side, so at low
amplitude (run down, or dirty) it can stop or be reluctant to start after
it stops. It's corrected by turning the hairspring stud carrier or collet;
a watchmaker's adjustment, not a repair.

| | Modern 28,800 | Older 18,000–21,600 |
| --- | --- | --- |
| Good | under 0.5 ms (Witschi's tolerance) | under 0.8 ms (check) |
| Fine | up to about 0.7 ms | up to about 1.0 ms |
| Adjust at next service | 0.7–1.5 ms | 1.0–2.0 ms |
| Have it seen to | over about 2 ms | over about 3 ms |

Why the slower watch is allowed more: the same small *angle* of being out
of beat takes longer in milliseconds on a slower balance, and on a smaller
swing. For the same reason **beat error in ms rises as amplitude falls**: a
watch at 0.4 ms fully wound may show 0.6 ms at 24 hours. That's physics,
not a fault.

Beat error changing a lot from position to position, or drifting during a
recording, is not normal: see [diagnosis.md](diagnosis.md) (hairspring
rubbing).

## Rate

**What it is.** How many seconds a day the watch would gain (+) or lose (−)
if it ran all day exactly as it did during the measurement.

**Why it matters least in a single reading.** The rate changes with
position, with how far the mainspring is wound, with temperature, and for
a minute or two after handling. A watch's daily accuracy on the wrist is
something like the average of its positions over a day of wear. So a
single dial-up rate of +12 s/d is no verdict; the same watch may be −2 in
crown down and average +4 on the wrist.

What matters:

- **The average over positions**, for regulating. Most watchmakers aim
  slightly fast, say 0 to +5, because a watch usually loses a little as it
  runs down.
- **The spread between positions** (the positional delta), for health
  and quality of adjustment.
- **Whether it's steady**: the trace.

General tolerance (Witschi, fully wound, mean over positions): men's
watches −5 to +15 s/d, ladies' −5 to +25 s/d.

### Chronometer standards

These are measured over days on the finished movement or watch, not from a
timegrapher snapshot. A COSC watch reading +7 dial up on a timegrapher is
not out of spec.

| Standard | What it requires (mean daily rate) | Note |
| --- | --- | --- |
| COSC / ISO 3159, movement over 20 mm | −4 to +6 s/d | Uncased movement, 15 days, five positions, three temperatures (8, 23, 38 °C). Also: mean variation 2 s/d, greatest variation 5 s/d, flat to hanging −6 to +8 s/d, largest variation 10 s/d, thermal ±0.6 s/d per °C, rate resumption ±5 s/d (check these secondary limits) |
| COSC, movement 20 mm or under | −5 to +8 s/d | (check) |
| METAS Master Chronometer (Omega, Tudor some) | 0 to +5 s/d | Cased watch; also magnetic resistance to 15,000 gauss (check scope) |
| Rolex Superlative Chronometer | −2 to +2 s/d | Cased, after COSC (since 2015) |

## Positions and the positional delta

The full test is six positions (see
[setup-and-microphone.md](setup-and-microphone.md#positions)). From them:

- **Delta (D)**: the biggest rate difference between any two positions.
  Good modern Swiss: under about 10 s/d. Decent: 10–20. Over 25–30 on a
  watch that should be better wants adjusting. Clones and cheap Chinese
  calibres often show 20–40 s/d new, because their hairsprings and their
  fitting are less carefully made and the balance less carefully poised.
- **Horizontal vs vertical (DVH)**: mean of vertical positions minus mean
  of horizontal. Mostly governed by the regulator pin clearance (on an
  index-regulated watch) and by the amplitude.
- **Dial up vs dial down**: normally within a few s/d and within about
  10–15° of amplitude. These two differ only in which pivot carries the
  balance's weight on its end stone, so a big difference points at one
  pivot, one end stone, the end shake, or a hairspring out of flat.
- **Between vertical positions**: differences here come from the balance
  being out of poise (heavier on one side) or the hairspring off centre.

Amplitude across positions tells you as much as the rate. A healthy
watch loses 20–50° from horizontal to vertical and is roughly even between
the four vertical positions.

## Isochronism: full wind against 24 hours

A mainspring delivers less torque as it runs down, so the amplitude
falls. Test fully wound, and again 24 hours later without winding, same
positions.

| Amplitude drop, full wind to 24 h | Reading |
| --- | --- |
| Up to about 30° | Good |
| 30–50° | Normal for many calibres; fine |
| Over about 60° | Mainspring, barrel or train friction; mention it |

The rate should change by only a few s/d between 0 and 24 hours. A watch
that's fine fully wound but goes badly off at 24 hours is telling you it
lacks reserve of power at the balance.

(These bands are rules of thumb (check); a long-reserve calibre built for
70 hours or more has a flatter torque curve over its first 24 hours.)

## The trace

The trace (the "paper tape" or diagram) is a dot for every tick, plotted
against when it should have come. What to look for:

| Trace | Meaning |
| --- | --- |
| One straight line, level | On rate, in beat. Good. |
| One straight line, sloping | Steady gain (up) or loss (down). Regulation. |
| Two parallel lines | Beat error; the gap is the size. |
| Gently curving | The rate is changing: still settling after winding or a position change, temperature, or the watch running down. Wait and re-test. |
| Regular waves | Something that turns is uneven. The period points at the wheel; see [diagnosis.md](diagnosis.md) and [long-runs.md](long-runs.md). |
| Wandering, scattered | Usually low amplitude and a watch due a service; also a loose clamp or a noisy room. |
| Steps or jumps | Knocking at high amplitude, or something catching intermittently. |
| Points scattered on one side | Dots early (above the line in Witschi's convention) are a sound before the unlock, e.g. hairspring rubbing, or noise; dots late (below) are missed unlocks: dry or sticky pallets, or the level too low. |
| Two lines drifting apart | Beat error changing: hairspring rubbing. |

**Look at the whole trace, not a few seconds of it.** A cheap timegrapher
screen holds about 10–20 s. The fourth wheel turns once a minute, so a
fault on one of its teeth shows once a minute and can be off the screen
when the photo is taken. That's why a video, or this program's whole-file
analysis, beats a photo. In `analyze` output, `rate_p05` and `rate_p95`
(the spread of the rate over 10 s windows) tell you how much the line
wanders; a few s/d is normal, tens of s/d is a wandering line.
