# When to see a watchmaker

Whether a watch needs a service is decided by three things together, not
by amplitude alone:

1. **The calibre.** Some run lower amplitude by design and keep good time
   doing it. Judge a reading against what that calibre normally does.
2. **The history.** When was it last serviced (if ever), how hard is it
   worn, how is it stored, and has anything changed recently (a knock,
   a swim, a hot car)?
3. **The measurements**, made properly (see below), and above all how
   they *change*: between positions, from full wind to 24 hours, and
   from one test to the next months later.

Then give one of four verdicts, plainly, at the top of your answer:
**fine**, **keep an eye on it**, **service soon**, **stop wearing it**.
When the measurement can't carry a verdict (no watch heard, a caveat that
makes every grade poor) or there is only one position, say **no verdict
yet** instead: what the reading does show, and the test that would
settle it (usually the six positions in
[guided-test.md](guided-test.md)). A clear fault in one position (it
stops, amplitude under 200°, a rate hundreds out) still gets its verdict.

Working watchmakers are clear that the timegrapher is supporting evidence:
a watch can time beautifully with dry jewels and perished gaskets, and a
watch with modest amplitude can be perfectly healthy for its calibre. Use
the numbers to spot trouble, and the history to decide when a service is
simply due.

## Before you judge

The thresholds below assume:

- a sound measurement: `doctor` clean, the right lift angle and beat rate
  for the calibre (see [reading-the-numbers.md](reading-the-numbers.md));
- the watch fully wound and settled, unless the test is the 24-hour one;
- at least dial up and one vertical position; ideally the full set, and a
  re-test after 24 hours unwound (the 24-hour figures tell you more about
  the state of the oil than a fresh full wind does).

## What normal looks like for the calibre

No maker publishes a "normal" amplitude for a healthy watch: ETA's,
Seiko's and Miyota's technical documents give the lift angle and test
conditions, not a range. What there is:

- **Witschi's tolerances**, which the `session` report judges against,
  fully wound: 260–320° horizontal, 240–280° vertical
  ([sessions.md](https://github.com/peterhudson/timegrapher/blob/main/docs/sessions.md#findings)).
  Witschi is also quoted as "the amplitude values of today's popular
  wristwatches are located at about 260° – 310°"
  ([Caliber Corner](https://calibercorner.com/school/amplitude/); the
  Witschi original wasn't found).
- **Sellita's limits for the SW200** (an ETA 2824-2 equivalent): at most
  315° dial up at full wind and at least 200° crown left (6H, 6 o'clock up) after
  24 hours, for 95% of a production lot
  ([SW200-1](https://www.sellita.ch/scripts/calibres/images/DocTec_SW200-1_7.pdf));
  the SW200-2 sheet gives 320° and 200°
  ([SW200-2](https://www.sellita.ch/scripts/calibres/images/DocTec_SW200-2.pdf)).
- **The project's own thresholds** in the `session` report: under 200°
  is a fault, vertical more than 50° below horizontal a warning, over
  330° (Witschi's knocking limit) a "Very high amplitude" warning.

Use those, and the watch's own earlier tests, rather than a figure per
calibre. For calibres not built like a Swiss 28,800 vph lever movement
(slower beats, older or simpler escapements), say that these tolerances
are for modern watches and lean on the trend between tests. Omega
co-axial and other non-lever escapements can't be measured for
amplitude by most timegraphers, this one included: judge them on rate,
beat error and history.

## How long since the service?

Lubricants age whether or not the watch runs, and heat ages them
faster: for industrial lubricants the common rule of thumb is that the
rate of oxidation roughly doubles for every 10 °C warmer
([Machinery Lubrication](https://www.machinerylubrication.com/Read/32752/how-heat-affects-lubricants-understanding-the-arrhenius-rate-rule));
no figure is published for watch oils, and the makers of watch
lubricants publish no service lifetimes. So a watch kept warm, or worn
daily in a hot climate, is due sooner than its maker's interval.

The makers' own intervals, where they publish one: Seiko every 2 or 3
years in its 6R35 manual
([Seiko 6R35](https://www.seikowatches.com/instructions/html/SEIKO_6R35_D_EN/BONDSYrgdkveiw)),
Grand Seiko 3–4 years for its 9S
([Grand Seiko](https://www.grand-seiko.com/instructions/html/GS_9S_en/AGFISYcisgnzsq)),
Omega 5–8 years
([Omega](https://www.omegawatches.com/en-us/customer-service/preserve-your-omega/watches)),
Longines 6–8 years
([Longines](https://www.longines.com/en-us/customer-service/services)).
Rolex and Tudor say only "periodic" or "regular" servicing in the
documents we could read; about 10 years is widely reported for Rolex
but not confirmed from Rolex itself. With no maker figure, say so and
use the time since service as one input among the measurements, not a
verdict on its own. Factor that in even when the numbers look fine: a watch that
measures well but hasn't been serviced in 12 years is still due.

**A new watch** that measures badly goes back to whoever sold it, under
its warranty, with what was measured; don't advise a service on a new
watch.

## Fine

The watch is healthy. Wear it and enjoy it.

- Amplitude within the normal range for its calibre, and holding up at
  24 hours.
- Beat error within Witschi's 0.5 ms (the report's `beat_error_tolerance`).
- The rate over the positions within the maker's own figure for the
  calibre (SKILL.md, "Judge the rate against the maker's own figure").
- A straight, steady trace across the whole run, and no periodic change
  the program names as a finding (`cycle_wheel` in `session`, or a
  wheel `long` confirms). A period listed in `analyze`'s
  `amplitude_periods` or `timing_periods` is a hint to confirm with
  `long`, not a finding; if the person asks about one, a line is
  enough: the program noted a repeat it can't tie to a fault, so ignore
  it unless a long run confirms it.
- Serviced within the interval above, or new.

What to say: "It's in good order. The amplitude is healthy for this
movement, which is the main thing; the rest is regulation and it's doing
fine."

## Keep an eye on it

Nothing to do now; re-test in six to twelve months or if its timekeeping
changes.

- Amplitude a little below normal for the calibre, but steady.
- Beat error over 0.5 ms but well under the report's 2 ms fault.
- Positional spread wider than the calibre should manage.
- A small regular wave or once-a-minute dip that isn't affecting
  timekeeping.
- Measuring well, but getting near the end of its service interval.

What to say: "It's running fine, but the amplitude is on the low side for
this movement. That's usually the first sign the oil is ageing. Test it
again in six months; if it's dropped further, book a service."

## Service soon

Book it in within the next few months; it's fine to wear meanwhile unless
it gets worse.

- Amplitude clearly below Witschi's tolerance fully wound, or under
  Sellita's 200° minimum crown left (6H, 6 o'clock up) at 24 hours.
- Amplitude falling between tests months apart.
- Dial up and dial down clearly apart in amplitude, or vertical more than
  50° below horizontal (`vh_amplitude_drop`).
- A large drop from full wind to 24 hours, or a power reserve clearly
  short of the maker's figure.
- Rate spread or average the owner can't live with: regulation is usually
  done with a service.
- A mean rate over the positions, or a spread between them, several
  times the maker's figure for the calibre. Say it plainly: it needs a
  watchmaker whatever the cause, whatever demagnetising at home shows. Example: a Rolex
  3235 at +66 s/d crown left and −18 crown right, a spread of 84 s/d,
  and a mean of +23 over six positions (+46 DU, +37 DD, +17 CD, −11 CU,
  +66 CL, −18 CR), against Rolex's −2/+2 for the cased watch.
- Beat error at the report's 2 ms fault (`beat_error_large`) or near it.
- A clear periodic fault confirmed on a long run.
- Still gaining hundreds of seconds a day after demagnetising properly
  (see below).
- Past its service interval, whatever the numbers say.

What to say: "It needs a service. The amplitude is low for this movement,
which means it's working harder than it should, and that's how wear
starts. It'll keep running for now, but the sooner it's done the less
it'll cost."

If it's under the maker's warranty, send it back to the maker and say
what was measured.

## Stop wearing it

Stop wearing it and take it in; carrying on can turn a service into a
repair.

- It stops, or stops in some positions.
- Amplitude far below the report's 200° fault threshold on a modern
  watch, fully wound.
- Signs of knocking: amplitude over about 330° with a jumping or doubled
  trace, or a double tick you can hear.
- A sudden change after a knock or drop.
- A scattered, wandering trace with low amplitude that re-clamping,
  re-winding and demagnetising don't fix.
- Grinding, scraping or rattling, a rotor rubbing, a gritty crown.
- Water or condensation under the crystal: the most urgent of all, same
  day.

What to say: "Stop wearing it and take it to a watchmaker. Something is
catching or has been damaged, and running it like this can wear or break
parts that are expensive to replace."

## Demagnetise first, at home

Magnetism is the commonest cause of a watch that suddenly gains a lot
(tens to hundreds of seconds a day, often erratic), and the owner can and
should fix it themselves with a cheap demagnetiser (the ~$10 "blue box"
kind). Recommend it before any service for that symptom.

How to use one:

1. Take the watch off anything metal; strap or bracelet can stay on.
2. Plug the demagnetiser in, put the watch flat on it, and **press and
   hold** the button.
3. **Still holding the button**, draw the watch slowly and steadily
   straight away from the box to at least 30 cm, better an arm's length.
4. **Only then** let go of the button. Letting go with the watch still on
   or near the box can magnetise it instead.
5. Turn the watch 90° and repeat; two or three passes in different
   orientations is plenty. Keep it short: the cheap units get hot. Unplug
   it afterwards.
6. Re-test on the timegrapher. A magnetised watch comes back to its normal
   rate straight away.

Checking first: a small compass held over the watch that swings as the
watch moves is a rough sign (the steel case can fool it); a phone
magnetometer app is rougher. The timegrapher is the best test: a sudden
large gain or a scattered trace that goes away after demagnetising.

If two careful attempts don't fix it, the field may be shielded by the
case or the hairspring coils may be caught: watchmaker, who will often
demagnetise for free. Silicon, Parachrom (Rolex), Spron (Seiko) and
similar hairsprings are far less affected, so on those a big gain is less
likely to be magnetism. To avoid it again: keep the watch more than a few
centimetres from phones, tablet covers, laptop speakers, power adapters,
bag clasps, speakers, induction hobs and magnetic jewellery.

A quartz watch that has been magnetised usually just needs moving away from
the magnet and resetting.

## Service or replace the movement?

Say this whenever the watch has an inexpensive, widely available movement:
a new movement can cost less than servicing the old one.

Rough prices (vary a lot by country; check locally):

- A service: about what a skilled tradesperson charges for 4–6 hours'
  work where the owner lives. Independents in the UK quote about
  £200–350 for a time-only automatic; in the US $150–500 at an
  independent; maker services more (Rolex about $800–1,200 in the US,
  Omega $600–1,200). Seiko service centres are cheaper.
- A replacement movement: Seiko NH35/NH36 about $40–65, Miyota 8215 about
  $40–65, Miyota 9015 about $75–130, Sea-Gull ST2130 about $60, Sellita
  SW200-1 about $215, ETA 2824-2 (scarce loose) $150–400.

So:

- **Swap** makes sense for an NH35, Miyota, ST2130 and the like in a watch
  whose value is in the case and dial: the swap costs less than a service
  and the new movement starts fresh.
- **Service** for anything genuine and valuable, vintage, or where
  originality matters, and for clone movements whose parts may not be
  available later.
- For an SW200, 2824-2 or 9015 in a microbrand watch, the two cost about
  the same; a service keeps the original.
- A swap still wants a watchmaker: hands and dial transferred, date and
  crown position matched, often a new stem, gaskets and a pressure test.
  It also leaves the case (crystal, gaskets, crown) as it was.

## What not to do

- Don't condemn a watch on one photo of a cheap timegrapher. Test properly
  first.
- Don't call a service on amplitude alone without the calibre and the
  history.
- Don't let a high rate in one position worry anyone: rate is regulation;
  amplitude is health.
- Don't open the case back. Water resistance, dust and a slipped
  screwdriver across a balance cock are all expensive.
