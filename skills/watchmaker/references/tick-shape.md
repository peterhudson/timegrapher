# Tick shape: reading `timegrapher shape`

A good timegrapher's "scope" view shows what a single tick sounds like.
The rate and amplitude tell you *that* the escapement is losing energy;
the shape can tell you *where*. It is also the best check on the
measurement itself: if the three sounds aren't where they should be, the
amplitude can't be trusted.

Be modest with this. The fault catalogue below follows Witschi's *Testing
methods for mechanical watches* (2025), and Witschi says plainly that the
examples are not exhaustive and that similar patterns can have different
mechanical causes. This program's measurements of each signature have so
far been checked on synthetic ticks, not on watches with confirmed faults.
Use shape to raise a possibility and to decide what a watchmaker should
look at, not to give a diagnosis.

## The three sounds

Each tick of a Swiss lever escapement is three sounds a few milliseconds
apart:

1. **Unlock**: the impulse pin enters the fork slot and knocks the fork,
   which pulls the pallet stone off the escape tooth. Quiet. Its timing is
   what the rate and beat error are measured from.
2. **Impulse**: the escape tooth slides across the pallet stone's impulse
   face and pushes the fork, which pushes the balance. Irregular;
   Witschi doesn't evaluate it.
3. **Drop**: the next escape tooth drops onto the other pallet stone's
   locking face, and the fork comes against its banking. The loudest.

Unlock to drop is the time the balance takes to pass through the lift
angle, which is how amplitude is measured. At 28,800 vph and 52° lift it's
about 7.7 ms at 270°, 8.7 ms at 240° and 10.4 ms at 200°. At 18,000 vph,
multiply by 1.6. (From `t = (T/π)·asin(L / 2A)` with T the full
oscillation period.)

**Even and odd beats are the two pallet stones.** The tick and the tock
are delivered by alternate stones, the entry and the exit pallet. A fault
on one stone, or on one side of the fork, shows on every other beat only.
`shape` reports the two sides separately as "even" and "odd". It can't
tell which is the entry stone and which the exit; that depends on where in
the swing the recording started. What it can tell you is that the two
differ.

A fault on one **escape tooth**, by contrast, shows once per escape-wheel
turn (every 30 beats with 15 teeth), on alternate stones; that's for the
long-run tools, not `shape`.

## Running it

```sh
timegrapher shape take.wav --lift 52
timegrapher shape take.wav --json
timegrapher shape take.wav --templates templates.csv   # the averaged tick, even and odd, for plotting
```

Use a clean recording: no clipping (`doctor` first), the watch clamped
firmly, a minute or two in one position. Clipping or auto gain wrecks the
shape.

## Reading the output

Columns: **even** and **odd** are medians over 2-second windows (the better
reading of timing, as amplitude changes smear the whole-recording
average); **even all** and **odd all** are measured on the
whole-recording averaged tick (where faint extra events show best). Times
are milliseconds from the unlock edge.

| Row | What it means | What to look for |
| --- | --- | --- |
| unlock to drop / amplitude | The amplitude measurement on this side | Even and odd within a few degrees |
| sound 1/2/3 rise, peak | When each sound starts and peaks | 2 between 1 and 3; 3 the loudest |
| interval 1-2, 2-3, 1-3 | Gaps between the sounds | 1-2 short and 1 faint: shallow lock. 1-2 long: deep lock |
| level 1:3, 2:3 | Loudness of the unlock and impulse against the drop | 1:3 near or above 1: unlock too strong |
| valley 1-2, 2-3 | How far the level dips between sounds: near 0 well separated, 1 no dip | Near 1 for 1-2: sounds merging (friction) |
| fill 1-2, 2-3 | Average level between the sounds against their peaks | High: energy smeared between the sounds |
| tail 3-15 ms after 3 | Sound lingering after the drop | High: rubbing after the drop (grazing balance or hairspring), or the case ringing |
| noise between beats | Level between ticks against the silence just before the unlock | Well above 1: continuous noise, e.g. a worn balance pivot, or room noise |
| rises from 1 to 3 | Separate sounds counted, including the drop | 3 is textbook; more means a sound is doubled |
| extra events before 1 / after 3 | Sounds outside the tick, with time and level | See the table below |

Below the table, for each side: in how many windows sound 1 and sound 2
were found, how often an extra event appeared, the spread of the 1-3
interval from beat to beat, and each extra event's time and level.

**There are no reference values yet** for level ratios, valleys, fills
and tails on real watches: they depend on the calibre, the case and the
microphone. Read them by comparison: even against odd, this watch against
the same model, today against last year. A difference between the two
sides on the same recording is the most trustworthy finding, because the
microphone and case are the same for both.

## Fault signatures

From Witschi's scope catalogue, with this program's measurement of each.

| Fault | What the tick shows | In `shape` |
| --- | --- | --- |
| Escapement fitting weak (shallow lock) | 1 and 2 close together, 1 small | Short interval 1-2 for the calibre; low level 1:3; sound 2 often not separable |
| Escapement fitting strong (deep lock) | Three well-separated sounds, a long gap 1→2 | Long interval 1-2 |
| Unlocking too strong | 1 as loud as or louder than 3 | Level 1:3 near or above 1 |
| Additional friction in the escapement | 1 and 2 loud, broad and merged | High fill 1-2, valley 1-2 near 1 |
| Safety pin (dart) touching the roller | Small clusters of sound before 1 and after 3 | Extra events either side, a few ms away, in bursts |
| Too little slot / impulse-pin clearance | An extra small sound just after the drop | An extra event in the drop's tail |
| Weak amplitude | Long gap 1→3 | Amplitude itself |
| Too much end shake in the balance pivots | Sounds doubled, their spacing varying beat to beat | Rises from 1 to 3 above 3; large beat-to-beat spread of 1-3 |
| Impulse pin touching the fork horn (knocking) | A single sharp extra sound before 1 and another after 3 | Isolated extra events either side; amplitude high |
| Damaged or worn balance-staff pivot | Raised continuous noise between beats | Noise between beats well above 1 |
| Grazing balance wheel or hairspring | Sustained rubbing after the drop, ending in another burst | High tail after 3; extra events after 3 |
| Escape tooth lands directly on the impulse plane | No distinct unlock; a small sound where the impulse should be, then the drop | Sound 1 found in few windows; only two sounds |

A fault that shows on **one side only** points at one pallet stone or one
side of the fork and roller. A fault on **both sides** points at
something common: the balance, its pivots, the hairspring, or the room.

## Before you believe a shape

- **Missing sound 1** is far more often a level problem than a fault: the
  unlock is the quietest sound, and a weak signal or noise buries it. Turn
  the level up (without clipping) and re-test before suggesting a fault.
- **Extra events** can be the case ringing, the clamp, or a noise in the
  room. They mean something when they're in the same place, on the same
  side, in most windows, and survive re-clamping the watch.
- **Doubled sounds** can be a reflection in the clamp or case. Again, look
  for consistency and for a difference between the sides.
- The **tail** depends a great deal on the case and the clamp. Compare
  sides, not absolute figures.

