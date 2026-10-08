# Diagnosis: from what you see to what's wrong

A timegrapher can't see inside the watch; it hears it. So the job is
reasoning from patterns to likely areas, and saying how sure you are. Many
causes look alike, and Witschi's own warning applies: similar patterns can
have different mechanical causes. Name the likely area, what would
confirm it, and what a watchmaker would do. Don't name a broken part as
fact.

## How to think about it

Work outwards from the measurement:

1. **The measurement.** Clipping, auto gain, wrong lift angle, wrong beat
   rate, a loose clamp, noise, not settled. Rule these out first, every
   time. See [setup-and-microphone.md](setup-and-microphone.md).
2. **The oscillator** (balance and hairspring): sets the rate. Faults
   show mainly as rate: large gains, positional rate differences, rate
   jumps.
3. **The escapement** (pallet fork and escape wheel): delivers the
   impulse. Faults show as lost amplitude, asymmetry between tick and
   tock, odd tick shapes, scatter.
4. **The power** (mainspring, barrel, gear train): supplies the energy.
   Faults show as amplitude: low overall, falling over the reserve, or
   changing periodically with a wheel's turn.

Then use the positions to split one cause from another.

## What the positions tell you

| Comparison | What differs mechanically | A big difference points at |
| --- | --- | --- |
| Dial up vs dial down | Which balance pivot carries the weight on its end stone | One pivot or end stone (dirty, dry, damaged), end shake, a hairspring out of flat touching in one position, the balance touching something in one position |
| Horizontal vs vertical (amplitude) | Pivots run on tips (horizontal) or on their sides in the hole jewels (vertical) | Normal drop is 20–50°. Much more: worn, bent or dirty pivots, dirty hole jewels, thick oil |
| Horizontal vs vertical (rate) | Regulator pin clearance, amplitude | Regulator pins too open or closed (index watches), or simply low vertical amplitude |
| Vertical vs vertical (rate) | Gravity pulls the balance's heavy side round | Poise error, hairspring off centre |
| Opposite verticals (crown up vs crown down; crown left vs right) | The same axis, heavy side up vs down | Poise along that axis: opposite rate errors in opposite positions |

**Friction or poise?** Both make the vertical positions differ, but:

- **Friction** at the pivots costs amplitude, and the rate follows the
  amplitude. Look at the amplitude first: if one vertical position has
  much lower amplitude than the others, it's friction or something
  touching in that position.
- **Poise** error leaves the amplitude roughly even between vertical
  positions but moves the rate, in opposite directions in opposite
  positions. Its effect depends on amplitude: it vanishes near 220°
  amplitude and reverses sign above it (a property of the balance's motion,
  the zero of the Bessel function J1). A heavy point at the bottom makes
  the watch gain below 220° and lose above (check the sign convention
  before telling anyone). So **a vertical rate spread that changes markedly
  between full wind and 24 hours, while the amplitude falls through the
  low 200s, is the signature of poise.** That's an adjustment for a
  watchmaker with poising tools.

## Magnetism

**Looks like:** a large gain, typically tens to thousands of s/d, often
erratic, the trace jumping about; sometimes beat error that looks wrong or
changes. Amplitude may look normal. Came on suddenly, often after the watch
was near a laptop, speaker, phone case clasp, handbag magnet, induction hob
or a tablet cover.

**Why:** the coils of a steel hairspring become magnetised and cling to
each other, which shortens the working length of the spring, so the watch
gains heavily and unevenly.

**Try first:** demagnetise (a cheap blue-box demagnetiser, used as
instructed: watch on the box, press the button, draw it slowly away to
arm's length before releasing) and re-test. A compass near the watch that
deflects is a rough check that it's magnetised (check: a weak residual
field may not move a compass).

**Conclude:** fixed after demagnetising: tell them, no harm done. Still
hundreds fast: the coils may be physically caught or the hairspring
distorted. Watchmaker.

Silicon, Parachrom (Rolex), Spron (Seiko) and similar hairsprings are much
less affected; on those, a large gain is less likely to be magnetism
(check per make).

## Balance and hairspring

| Pattern | Likely cause | Notes |
| --- | --- | --- |
| Large gain in every position, steady | Coils touching (magnetism, oil on the spring, a shock that tangled it), or simply regulation | Demagnetise first. If not regulation, watchmaker. |
| Large gain or erratic in one or two positions only | Hairspring touching the balance arms, the cock or the regulator in that position; out of flat | Rubbing noise in `shape` (long tail after the drop); trace dots early; beat error drifting |
| Two trace lines drifting apart; beat error changing over a recording | Hairspring rubbing (Witschi), often on the regulator pins or stud | Witschi: "Listen for noise." Watchmaker |
| Vertical rate spread, amplitude even | Poise or hairspring centring | See above; an adjustment |
| Vertical rate spread with uneven vertical amplitude | Pivot friction or a bent pivot in some positions | Usually after a knock |
| Beat error large but steady | Hairspring collet or stud moved (after a shock or a careless service) | An adjustment |
| Very low amplitude in vertical, fine horizontal, after a knock | Bent or broken balance pivot (shock jewels didn't save it) | Stop wearing; watchmaker |
| Stops in one position | Broken pivot, balance fouling, very low amplitude | Stop wearing |

## Escapement

The escapement's health shows in amplitude, in the tick shape, and in the
difference between the two pallet stones (even and odd beats). For the
tick shape see [tick-shape.md](tick-shape.md).

| Pattern | Likely cause |
| --- | --- |
| Even and odd amplitude differ by more than about 10° on a clean signal | Asymmetry between entry and exit pallet: lock or drop unequal, one stone set differently, or simply a large beat error (check: how much beat error alone splits the two measured amplitudes) |
| Late dots on one side, scatter below the line; amplitude low | Dry or gummed pallet stones or escape wheel (Witschi: clean the escapement or replace the escape wheel) |
| A smooth wave with the escape wheel's period (3.75 s at 28,800 vph with 15 teeth; 5 s at 21,600; 6 s at 18,000) | Escape wheel out of round, bent arbor or a pivot problem; a damaged escape tooth gives a sharp blip instead of a wave |
| Long gap unlock-to-impulse (deep lock); low amplitude | Lock too deep: energy lost in unlocking (Witschi "escapement fitting strong") |
| Unlock and impulse almost on top of each other, unlock faint | Lock too shallow: risk of the escapement tripping (Witschi "escapement fitting weak") |
| Steps in the trace, amplitude over about 320–330°, a double tick | Knocking (overbanking): the impulse pin hits the outside of the fork horn. Mainspring too strong, or fork and banking geometry. Watchmaker; it can damage the escapement |
| Permanent double beat | Severe overbanking (Witschi). Stop wearing |

Draw (the angle on the pallet locking face that pulls the fork against
its banking) can't be measured directly on a timegrapher; insufficient
draw shows as a fork that doesn't stay put, giving erratic behaviour after
shocks (check whether anything on the timegrapher reliably shows it).

## Lubrication and dirt

**Looks like:** low amplitude overall, worse in the vertical positions;
amplitude that is low now but used to be fine; a rate that wanders; a
trace that scatters; slow recovery after a position change (Witschi links
this to poor lubrication in the balance or train bearings).

**Why:** oil thickens, dries or migrates over the years; dirt and wear
particles build up. The train and escapement lose more of the
mainspring's energy before it reaches the balance.

**Typical history:** a watch last serviced 5–10 years ago, amplitude 30–60°
lower than it was. That's what a service is for, and it's the commonest
finding by far.

**Odd case:** some watches run *higher* amplitude just after a service
with very fresh oil, or lower for the first few days while the oil beds
in (check). Re-test after a week before judging a freshly serviced watch.

## Mainspring and barrel

| Pattern | Likely cause |
| --- | --- |
| Amplitude low even fully wound, steady, everything else fine | Tired ("set") mainspring, or the train taking too much; a service with a new mainspring usually restores it |
| Large drop from full wind to 24 h (over about 60°), or a short power reserve | Weak mainspring, barrel friction, dry barrel |
| Automatic: amplitude drops briefly when wound past full | Normal slipping of the bridle; a ragged sawtooth or slipping too early is a worn bridle or barrel wall (check how this shows on a timegrapher) |
| Periodic change over hours | Barrel turn (5–7 hours on many calibres; about 6 h on Rolex 32xx (check)) |
| Periodic change over minutes (e.g. every 5 min) | A barrel tooth meshing with the centre pinion: worn barrel arbor hole letting the barrel tilt (a documented Orient 48748 case, 5 min) |
| Amplitude too high, knocking | A mainspring too strong for the calibre (a wrong replacement spring) |

## Gear train: periodic changes

Every wheel turns at a fixed rate, so a fault on a wheel (a damaged tooth,
a bent pivot, an eccentric wheel, dirt in a tooth space) repeats every
turn. The period names the wheel. This is what the `long` command is for;
see [long-runs.md](long-runs.md).

| Wheel | One turn | Notes |
| --- | --- | --- |
| Escape wheel | 2 × teeth beats: 3.75 s at 28,800 vph (15 teeth) | Most escape wheels have 15 teeth; Witschi says 15–21 |
| Fourth wheel (seconds wheel) | 60 s | Carries the seconds hand on centre-seconds and most small-seconds calibres. The commonest periodic culprit |
| Third wheel | Calibre-specific, often around 7–10 min | Depends on centre wheel teeth and third pinion leaves; give it with `--wheel` if known |
| Centre wheel | 60 min | Carries the minute hand (direct-drive calibres; check for indirect) |
| Barrel | Hours (often 5–7 h) | Needs a run of days |

Also look at **tooth-mesh periods**: one leaf of a pinion engaging comes
round at the wheel's period divided by the number of leaves, and watchmakers
report tooth interaction is more often the cause than an eccentric wheel.

**Shape of the cycle:**

- **A sharp dip once per turn** (the average cycle in the `long` report is
  mostly flat with one notch): one damaged or dirty tooth or pinion leaf,
  or something rubbing at one point in the turn.
- **A smooth wave once per turn**: an eccentric wheel, a bent arbor, a
  worn pivot hole letting the wheel wander.

Some meshing variation is normal. Witschi: each mesh varies the torque by
typically 5–10%, and across the train this can give amplitude variations
of up to 30°, "generally considered normal". So a few degrees once a
minute on an otherwise healthy watch is a curiosity, not a fault.

### The hands

A hand touching something is a very common "gear-train fault" that isn't.

- **Seconds hand** touching the dial, the crystal, or the hour or minute
  hand: an amplitude dip once a minute, always at the same point of the
  minute. If it's touching the **dial or crystal**, the dip stays at the
  same seconds position whatever the time. If it's catching the **hour or
  minute hand**, the point in the minute where it happens moves as the
  hour and minute hands move round. Re-running with the hands at a
  different time separates the two.
- **Minute or hour hand** touching: once an hour or at a particular time.
- **Date change**: many calibres load the date wheel over the hours before
  midnight. A slow amplitude dip around then, every day, is the date
  mechanism and usually normal (check how big a dip is acceptable).
- **Chronograph running**: amplitude drops with the chronograph engaged.
  A small drop is expected; a large one wants adjusting (no published
  tolerance found).

A watchmaker confirms a train fault by taking the balance and fork out and
watching the train spin down, or by inspecting the teeth under the
microscope.

## Shock damage

After a drop or a hard knock, compare with any earlier reading:

- Stopped, or stops in some positions: stop wearing.
- Amplitude much lower in some positions than others, new: bent pivot,
  damaged jewel, or the balance fouling. Stop wearing.
- Large gain, new: hairspring caught or tangled. Demagnetise first in case;
  otherwise watchmaker.
- Beat error suddenly large: hairspring shifted. An adjustment, but check
  the rest.

## Decision rules

Before concluding anything is wrong, in this order:

1. Re-check the set-up with `timegrapher doctor`; re-clamp the watch.
2. Check the beat rate and the lift angle are right for the calibre.
3. Fully wind, wait a few minutes, re-test. Wait 30–60 s after each
   position change.
4. If it gains a lot: demagnetise and re-test.
5. Test more than one position; test again the next day without winding.
6. For a regular wave: run `long` for at least half an hour, better two
   hours, and see whether the period matches a wheel.

Then:

- One measurement off, others fine, and it goes away on re-test: the
  set-up or settling. Fine.
- Consistent across re-tests: real. Use the tables above to name the
  likely area, and [when-to-see-a-watchmaker.md](when-to-see-a-watchmaker.md)
  for the verdict.

