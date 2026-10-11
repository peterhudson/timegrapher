# Reading the numbers

What each figure means, why it matters, and what to hold it against.
Read the numbers in this order: is the measurement sound, amplitude,
lift angle, beat error, rate, the trace, then the positions.

Each figure here is either sourced or labelled as a rule of thumb.
Where a maker publishes its own figures for a calibre, theirs win; the
sets, by source, are in
[when-to-see-a-watchmaker.md](when-to-see-a-watchmaker.md#tolerance-sets-by-source).

## First: is the measurement any good?

From `timegrapher analyze --json` (field names in
[cli-reference.md](cli-reference.md)):

- **Beat rate (`bph`)** must be one the calibre runs at. If it guessed
  14,400 for a modern watch, it has latched onto every other tick; give
  `--bph` explicitly.
- **Beats found (`beats_found`)** should be close to duration × bph ÷ 3600.
  A big shortfall means the microphone is missing ticks.
- **Tick and Tock amplitudes** (`amplitude_even_deg`, `amplitude_odd_deg`:
  the even beats are the Tick, the odd the Tock) often differ.
  On clean corpus recordings the split runs from under 1° to about 19°,
  and it does not predict how far the mean is off (docs/validation.md,
  "Tick and Tock amplitudes apart"), so quote `amplitude_deg` (the headline
  figure: the median over 2 s windows, which can sit a little off the
  midpoint of the two sides) and don't treat a split as a fault on its
  own.
  `profile` shows which sounds were marked on each side; a split well
  beyond 20° on a clean signal is worth a look there (see
  [tick-shape.md](tick-shape.md)).
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

**What to judge it against, fully wound.** The maker's figure where
one exists: ETA's 2824-2 and Sellita's SW200-1 sheets allow at most 315°
dial up at full wind and at least 200° at 6H (6 o'clock up) after 24
hours. Otherwise Witschi's training course: 250–330° horizontal and
250–270° vertical, with a worked example going down to 220° vertical.
Under 200° fully wound and vertical more than 50° below horizontal are
the project's rules of thumb for "worth saying"; 330° is the amplitude
Witschi's course names as causing knocking, and only jumps in the trace
confirm it. Sources and the limits of these figures are in
[when-to-see-a-watchmaker.md](when-to-see-a-watchmaker.md#tolerance-sets-by-source).

Vertical positions read lower than horizontal: there the balance pivots
run on their sides in the jewel holes, which is more friction than
running on their tips on the end stones. So judge a vertical reading
against a vertical range.

These tolerances are for modern watches. For older, slower or simpler
calibres no published figure is given here: say so, and lean on the
trend between tests of the same watch and on the trace.

**Too high.** Over 315° dial up (the ETA and Sellita maximum), check in this order:

1. The lift angle (too high a lift angle inflates amplitude).
2. The measurement: a missed unlock or a doubled sound reads as huge
   amplitude. `timegrapher shape` will show it.
3. Real high amplitude: a mainspring too strong for the calibre, or a
   movement short of oil (oil is sticky and costs energy at this
   scale; its main job is to stop wear, so a dry movement can show high
   amplitude while it wears). Around 320–340°, depending
   on the calibre, the impulse pin can strike the outside of the fork horn:
   **knocking**, or rebanking. It upsets the rate and can damage the
   escapement. Witschi's training course names 330°.

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

For the lift angle and beat rate, use the program's calibre table:
`timegrapher calibres NAME --json` (or the MCP server's calibre lookup)
gives them with how sure the table is (`confidence`, `notes`). Give
that standing with any amplitude, in your own words; the table's source
names and links are for checking, not for reading out. For a calibre the table doesn't have,
the most complete list is **WatchGuy's**
(watchguy.co.uk/cgi-bin/lift_angles), with well over a thousand calibres
by maker; look it up there when you can fetch web pages, and say where
the figure came from. Where the two disagree, prefer the program's table:
its figures come from the bench.

Name a particular factory or copy of a movement only when the person
has named it. Never suggest that a watch may be a copy.

An Omega co-axial escapement doesn't make the Swiss-lever three-sound
tick, and this program's amplitude assumes a Swiss lever, so treat a
co-axial's amplitude as unreliable whatever lift angle is set.

Amplitude is in proportion to the lift angle: a reading made at the
wrong lift angle is corrected by multiplying it by right ÷ used (250° at
52° is about 264° at 55°), which is about amplitude ÷ lift angle per
degree. A lift angle of 62–65° set on someone else's machine is a sign of
amplitude hacking.

If the calibre is unknown: use 52°, say so in the answer, and give the
range the likely lift angles would give, worked out as above. The *changes* between positions
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

Witschi's training course gives 0.0–0.5 ms as acceptable; ETA's 2824-2
sheet allows 0.8 ms (Standard, Elaboré) or 0.6 ms (Top, Chronometer)
dial up fully wound. 2 ms or more is the project's rule of thumb for
"large". Between those is a matter for the next service.

A slower watch shows more beat error for the same fault: the same small *angle* of being out
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

- **The average over positions**, for regulating, against the maker's
  figure for the grade where there is one (ETA's 2824-2 sheet gives
  "12 ±12" s/d for Standard, "7 ±7" Elaboré, "4 ±4" Top, quoted as
  printed) or a Witschi class.
- **The spread between positions** (the positional delta), for health
  and quality of adjustment.
- **Whether it's steady**: the trace.

Witschi's training course gives, for the mean rate fully wound: gent's
watches −5 to +15 s/d, lady's −5 to +25, chronometers −2 to +6. Every
set is in
[when-to-see-a-watchmaker.md](when-to-see-a-watchmaker.md#tolerance-sets-by-source).

### Chronometer standards

These are measured over days on the finished movement or watch, not from a
timegrapher snapshot. A COSC watch reading +7 dial up on a timegrapher is
not out of spec.

| Standard | What it requires (mean daily rate) | Note |
| --- | --- | --- |
| COSC / ISO 3159, category 1 (fitting diameter over 20 mm) | −4 to +6 s/d | Uncased movement, five positions (3, 6 and 9 o'clock, dial up, dial down) and several temperatures, 15 days. Also: mean variation 2 s/d, greatest variation 5, horizontal to vertical −6 to +8, largest variation 10, thermal ±0.6, rate resumption ±5 ([COSC](https://www.cosc.swiss/en/certification/mechanical-movements)) |
| COSC, category 2 (20 mm or under) | −5 to +8 s/d | Same tests; the other limits are a little wider (same source) |
| METAS Master Chronometer | 0 to +5 s/d (up to 0 to +7 by calibre) | Cased watch, after chronometer certification of the movement (ISO 3159, COSC in Switzerland); also 15,000 gauss ([Omega](https://www.omegawatches.com/masterchronometercertification)) |
| Rolex Superlative Chronometer | −2 to +2 s/d | Cased, after COSC ([FH](https://www.fhs.swiss/eng/2016_04_28_01_Rolex.html)) |

## Positions and the positional delta

The full test is six positions (see
[setup-and-microphone.md](setup-and-microphone.md#positions)). From them:

- **Delta (D)**: the biggest rate difference between any two positions.
  Hold it against the maker's figure: ETA's 2824-2 sheet allows 30,
  20 or 15 s/d by grade, Miyota's 8215 under 50 s/d, Seiko's NH35 under
  60 s/d, COSC's largest variation 10 s/d. With no maker figure, over
  10 s/d is the project's rule of thumb for "worth a look".
- **Horizontal vs vertical (DVH)**: mean of vertical positions minus mean
  of horizontal. Mostly governed by the regulator pin clearance (on an
  index-regulated watch) and by the amplitude.
- **Dial up vs dial down**: normally within a few s/d and within about
  10–15° of amplitude (this skill's rule of thumb). These two differ only in which pivot carries the
  balance's weight on its end stone, so a big difference points at one
  pivot, one end stone, the end shake, or a hairspring out of flat.
- **Between vertical positions**: differences here come from the balance
  being out of poise (heavier on one side) or the hairspring off centre.

Amplitude across positions tells you as much as the rate. Witschi's
training course puts vertical at 250–270° against 250–330° horizontal,
and more than 50° below horizontal is the project's rule of thumb for
mentioning it. A big difference between the four vertical positions is
worth a second look.

## Isochronism: full wind against 24 hours

A mainspring delivers less torque as it runs down, so the amplitude
falls. Test fully wound, and again 24 hours later without winding, same
positions.

The `session` report gives the change per position between states of
wind (`isochronism`). Witschi's training course: "Decrease of the
values after 24 h: approx -10% to -15% is ok". ETA's 2824-2 sheet gives
the rate change dial up from full wind to 24 h (±20, ±15 or ±10 s/d by
grade), and ETA and Sellita both set a floor of 200° at 6H after 24
hours ([SW200-1](https://www.sellita.ch/scripts/calibres/images/DocTec_SW200-1_7.pdf)). Compare the drop with the same watch's earlier
tests: a drop that grows between tests is the useful sign.

A watch that's fine fully wound but goes badly off at 24 hours is telling
you it lacks power at the balance by the end of a day.

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
