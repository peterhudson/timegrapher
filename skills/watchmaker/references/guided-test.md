# The guided test: walking someone through it, step by step

This is the script for testing a watch at home with someone who has never
used a timegrapher. They hold the watch; you run the program and tell them
what to do. Follow it in order. Each step says what to tell them, what to
run, and what has to be true before you move on. Keep each instruction to
one thing they can do with their hands, then wait for them to say it's
done.

Any agent can follow it: one with a shell runs the commands itself; one
that reaches the program through the MCP server calls the tool of the same
name. Nothing here needs the desktop app.

How long it takes: about 25 minutes for the six positions fully wound,
and the same again 24 hours later for the run-down half (optional, but it
is the part that shows a tired mainspring).

## 0. Before you start

Ask, in one message:

- **What is the watch?** Make and model, and the calibre if they know it.
  If they don't, ask them to look in the papers, on the maker's website
  for the model or reference number, and on the case back (an engraving,
  or the movement itself behind a display back), and to send a photo of
  the dial and the case back if they can. The calibre gives the beat rate
  and the lift angle; without the right lift angle the amplitude is off
  in proportion (see SKILL.md).
- **When was it last serviced**, roughly, and **how is it worn**: every
  day, now and then, or in a drawer. You need both to judge the results;
  the numbers alone are not a service verdict.
- **What are you sure of?** If they give a beat rate or a lift angle,
  ask where it came from (the maker, a seller, a forum, a guess). Check it
  against the calibre table before using it.
- **Has anything happened to it?** A knock, a fall, sitting next to a
  speaker, a phone case or a laptop with a magnetic clasp, or a sudden
  change in how it keeps time.

Look the calibre up in the program's calibre table: `timegrapher
calibres NAME --json` (a name not in the table is an error that points
to `timegrapher calibres --search NAME`, which matches by number, maker
or other name), then in
[reading-the-numbers.md](reading-the-numbers.md#lift-angle). Take the lift
angle from the table, never from memory. Note bph and lift. If the calibre is not known, say so: you'll use 52° and let the
beat rate be found, and the amplitude is then only as good as that
guess: say so, and give the range the likely lift angles would give
(SKILL.md, set-up step 5).

Tell a newcomer up front that not knowing is fine: the program finds the
beat rate itself, and you will say what an unknown lift angle costs.
Explain any term (lift angle, calibre, clipping, auto gain) the first
time you use it.

## 1. The microphone (5 minutes)

Tell them: *"Put the watch in the stand's clamp, dial up, the way you'd
lay it on a table. Make sure it's held firmly; a loose watch sounds like a
faulty one."*

Run `timegrapher doctor --json` (with `--device NAME` from
`timegrapher devices` if the stand isn't the default input). It gives
levels and what it detected, with no verdict. Go on when
a regular beat is found, nothing clips, the peak sits below about
−6 dBFS and the ticks stand well clear of the noise (the levels and the
project's rules of thumb are in
[findings.md](findings.md#doctor)). If nothing you try lifts the ticks
further, go on and say in the result that amplitude and beat error are
less sure than the rate.

If `doctor` proposes mixer changes, explain each one and ask before
running it with `--apply`. Issue codes and their fixes are in
[setup-and-microphone.md](setup-and-microphone.md#checking-the-signal-with-doctor)
and [findings.md](findings.md#doctor).

## 2. Wind and let it run (5 minutes)

Tell them: *"Take the watch out and wind it fully. For a hand-wound
watch, turn the crown until it gently stops; never force it. For an
automatic, about 40 turns of the crown, or wear it for the day before.
Then put it back in the clamp, dial up, and leave it for a few minutes
before we start."*

Note the time. It is the start of the 24-hour clock for step 6.

## 3. A first look, dial up (1 minute)

Record 60 s dial up into a folder of its own, so it stays out of the
test (see [Recording](#recording) below):

```sh
timegrapher record FOLDER/first-look --position DU --seconds 60 --calibre NAME --json
```

Its `check` is the quick look: `watch_heard`, beats, rate, amplitude,
beat error and clipping. For a closer look, `timegrapher analyze` the
take's `audio-001.wav` with the same settings.

Stop here if:

- `watch_heard` is false: no watch was heard. Is it running? Is it
  touching the microphone? Run `doctor` again.
- the rate is hundreds of seconds a day fast or erratic: that is
  magnetism until proved otherwise. Demagnetise first
  ([when-to-see-a-watchmaker.md](when-to-see-a-watchmaker.md#demagnetise-first-at-home)),
  wind again, and restart from step 2. Testing positions on a
  magnetised watch measures the magnetism.
- `bph` is not the calibre's beat rate: you have the wrong calibre, or
  the program locked onto something else. Sort that out first.
- `clipping.beat_fraction` is over 0.01: the level is too high; back to
  step 1.

Otherwise tell them what you see in one line (*"It's running, and it
sounds healthy; now let's do the positions"*) and go on. Say that this is
a first look: the figures to keep come from the session in step 5. Say
what one dial-up take can show (the watch runs, its amplitude and beat
error lying flat, its rate in that one position) and what it can't
(anything about positions, such as vertical against horizontal). Before
asking for more positions, ask what their stand holds: whether its clamp
turns, or how they could hold the watch on its edge. Any
figure from `doctor` covers a few seconds and is for the signal only;
never quote it as the watch's rate.

## 4. Six positions (15 minutes)

Each position: they turn the watch, you wait about 10 s, then record 2
minutes. The analysis skips the first 30 s of each recording while the
balance settles to the new position, so they don't need to wait longer.
Ask them not to touch the stand or the table while it records.

| Order | Code | Tell them |
| --- | --- | --- |
| 1 | DU | "Dial up: lying flat, face up." (It's already there.) |
| 2 | DD | "Turn it over: lying flat, face down." |
| 3 | CD | "Stand it on its edge with the crown pointing down to the table." |
| 4 | CL | "Turn it a quarter so the crown points to your left." (The 6 is at the top.) |
| 5 | CU | "Another quarter: crown pointing up." |
| 6 | CR | "Another quarter: crown pointing to your right." (The 12 is at the top.) |

If the crown is not at 3 o'clock, name the position by which number is up
(DU and DD don't change; with the crown at 4, "crown down" is 4 o'clock
down). Many stands have a clamp that turns; if theirs doesn't, they hold
the watch on its edge against the microphone with something soft, as long
as it doesn't move. Tell them a propped-up watch can shift during the
take; if one position reads very differently from the rest, record it
again before believing it.

Record each position into one session folder for this watch. The first
take writes the session file, with the watch and calibre:

```sh
timegrapher record FOLDER --position DU --seconds 120 --calibre NAME --watch "their words" --json
timegrapher record FOLDER --position DD --seconds 120 --json
...
```

Each run says on stderr when it starts and prints `done: the watch can
be moved` at the end; tell the person both. Before you ask them to
turn the watch, read the take's facts: if no regular beat was found
(`watch_heard` false) or the ticks clipped (`clipping`), fix the cause
and record that position again: `record` keeps every take, so the
session then holds both, and `session` reads each. A bad
position is cheaper to redo now than tomorrow.

## 5. Read the session

`record` has written the session file (watch, calibre, bph, lift,
`settle_s = 30` and one entry per take).

Run `timegrapher session FOLDER --json` and read it as
[findings.md](findings.md#session-the-positions-test) says: first
whether the measurement is sound, then the per-position readings and
the characteristic values, then hold them against the tolerance set
that fits the watch
([when-to-see-a-watchmaker.md](when-to-see-a-watchmaker.md#tolerance-sets-by-source)).
The program applies no tolerance: hold the numbers against the tables
yourself. Write the answer as SKILL.md's "How to write the
answer" says, with the service history and wear from step 0 in it.

Then look at the shape of the Tick and the Tock: run `timegrapher analyze
TAKE/audio-001.wav --calibre NAME --json` on each take (`take` in
`record`'s output) and read its `shape` block against
[tick-shape.md](tick-shape.md#rules-for-possible-faults). A pattern
that shows on the same side in several positions is worth passing on to
a watchmaker; one in a single position is more likely the set-up.

If the fully-wound half already shows something clearly outside the
watch's tolerance set (amplitude well under 200° in several positions,
say), tell them now: the 24-hour half will not change that answer.

For someone new to this, give one plain line for each number before the
verdict:

- **Amplitude**: how far the balance swings each way. It is the best
  single sign of whether the watch is getting the power it should.
- **Beat error**: how evenly the tick and the tock are spaced. It is an
  adjustment a watchmaker makes, not wear.
- **Rate**: how many seconds a day it would gain or lose if it stayed in
  that position, at that state of wind. A watch on the wrist moves
  through all of them.

### Checking a claimed rate (a certificate, a seller's ±2 s/d)

A maker's or certificate's rate is a mean over several positions, not
one reading. COSC, for instance, tests five positions: 3 o'clock, 6
o'clock, 9 o'clock, dial up and dial down
([COSC](https://www.cosc.swiss/en/certification/mechanical-movements)),
that is CU, CL, CD, DU and DD here, fully wound, over days and at
several temperatures. To compare with a claim:

1. Record those five positions fully wound, as in step 4.
2. Take the mean of their rates (the `session` report's X, when those
   five are the positions recorded), and compare that with the claim,
   not any one position.
3. Say what this can't match: one sitting at room temperature, minutes
   per position, and the sound card's clock unless it was calibrated.
   A mean a few s/d from the claim doesn't disprove it; one many times
   the claim's size is worth a watchmaker's look (SKILL.md, "A rate far
   outside the maker's figure").

### Comparing with the wrist

To see how it keeps time in real wear, which no timegrapher test gives:

1. Set the watch to a phone's clock (the phone keeps network time) to
   the second.
2. Wear it as usual for a week, and note the difference from the phone
   at the same time each day.
3. The gain or loss over the week, divided by the days, is its rate on
   the wrist. Compare that with the maker's figure (SKILL.md, "Judge the
   rate against the maker's own figure"): a week is seven times the
   daily figure, so Rolex's −2/+2 s/d is −14/+14 s a week. With no
   published figure for the calibre, say so rather than give a general
   number for when to come back.

## 6. Twenty-four hours later (optional, 15 minutes)

Tell them, when you give the first result: *"If you can, don't wind it or
wear it, and leave it face up somewhere still. Come back in 24 hours from
when you wound it and we'll do the same six positions again. That shows
whether the spring still has the strength to keep it going at the end of
a day."* An automatic left off the wrist does not wind itself, so it is
the same test.

Then repeat step 4 into the same folder with `--wind-h 24` on every
take; `record` marks each take with it in the session file.

`session` then gives the isochronism per position, Im and Ie (it
doesn't compute N). What
they mean is in [reading-the-numbers.md](reading-the-numbers.md#isochronism-full-wind-against-24-hours).

## 7. A long run (only if something points at it)

If the trace in a position wanders or waves (the 10 s rates spread
widely, a regular change matches a wheel's turn, or the rate switches
between two levels: see [findings.md](findings.md#session-the-positions-test)),
offer a long run: an hour or more in that one position, read with
`timegrapher series` (is it steady, and if not, which wheel) and
`timegrapher long` (the report). See [long-runs.md](long-runs.md).

## A worked example: a Rolex 3235, dial up for two hours

The take `ym42_DU_2h` in the project's public test corpus is a Rolex
3235 (28,800 bph, the lift angle from the program's calibre table) recorded dial up
for two hours in twelve ten-minute files, one `session` reading each.
What `session` says, and how to reason from it:

| Readings | Rate s/d | Amplitude | Beat error | Also measured |
| --- | --- | --- | --- | --- |
| 1–2 (first 20 min) | +49, +48 | 252° | 0.27 ms | 10 s rates spread from +20 to +75 |
| 3–8 (20–80 min) | +35 falling to +14 | 247–252° | 0.30–0.33 ms | 10 s rates spread over 20 s/d in all six; a change once a minute (the fourth wheel's turn) in four |
| 9–12 (last 40 min) | +8 to +10 | 247–251° | 0.33–0.35 ms | a change once a minute in two |

1. **Is the measurement sound?** Yes: a watch is heard in every reading,
   there is no clipping, the beat rate is the calibre's, and a second
   program (tg, on three of the files) agrees to within a few s/d and a
   few degrees. The session file gives no time since winding (`full?` in
   the report), so the state of wind is not known; say so.
2. **The rate falls from +49 to about +9 s/d over the first 80 minutes,
   then holds.** Amplitude barely moves in that time, so it is unlikely to be the
   mainspring. It looks like the watch settling after being handled. That
   is why step 2 lets the watch run before measuring: a six-position test
   started in the first half hour would have mixed the settling into the
   differences between positions. Judge the rate on the last 40 minutes:
   about +9 s/d dial up, which says little on its own (rate comes last).
3. **Amplitude is 247–252° dial up** at the table's lift angle, at the
   bottom of the horizontal range in Witschi's training course
   (250–330°), and steady over two hours. No Rolex amplitude figure was
   found, and the lift angle for the 32xx is itself not settled (see
   the calibre table's notes). On its own that is a reason to look
   further, not a verdict.
4. **Beat error 0.27–0.35 ms**: within the training course's 0.0–0.5 ms.
5. **A once-a-minute change**: amplitude repeats every 60 s, 5–10° peak to
   peak, in six of the twelve readings (and at a third of a minute in
   three more), and the rate does too in two of them. The fourth wheel turns once a minute. This is already a two-hour
   run, so it's repeatable, not chance. It points at the fourth wheel or
   something meshing with it, typically wear or dirt, and it is a
   watchmaker's job to find.
6. **Verdict.** It weighs the numbers with what the owner says. A modern
   calibre at about 250° dial up, with a repeatable once-a-minute
   amplitude change and several years since its last service, is
   **service soon**. Serviced recently, the same numbers say **take it
   back**: the low amplitude and the once-a-minute change shouldn't be
   there. Either way, say the verdict needs the other five positions to
   be firm. One line first: *"It's running well enough to wear, but it's
   showing signs of wear in the gear train, so I'd book a service."*

## Testing a collection

Someone with several watches usually wants to know which ones need
attention. Keep each watch separate and the set-up fixed:

- **One session folder per watch**, named in their words
  (`record "Seamaster 2008/" ...`). Never put two watches in one session.
- **Check the microphone once** at the start with `doctor`, then leave the
  stand and the input level alone for the rest of the sitting, so the
  watches are measured the same way.
- **Triage first**: a fully wound 60 s first look (step 3) on each watch.
  The ones that are clearly fine dial up may need nothing more today. Do
  the six positions on the ones that look off, the ones not serviced for
  years, and the ones the owner is worried about.
- **Finish with a table** they can keep, one row per watch: the watch and
  calibre, the last service as they remember it, amplitude dial up, the
  rate spread over positions, beat error, the verdict and the next step
  (nothing, re-test in six months, demagnetise and re-test, service).
  Session folders made with `record FOLDER` are kept only where they
  were made; a test recorded with `record --watch NAME` into the desktop
  app's library stays among that watch's takes, which `watches show
  NAME` lists. The folders and this table are the record, and next
  year's test is compared against them.

## Figures for a sale listing

A seller often has only dial-up recordings and wants a line for the
listing. Say plainly what that can and can't support.

- **It supports**: that the watch runs and was heard cleanly; its
  amplitude dial up, fully wound or not (say which), at a named lift
  angle; its beat error (the unlock figure); and its dial-up rate at that
  state of wind.
- **It doesn't support**: how it keeps time on the wrist, its rate in
  other positions or the spread between them, how it holds up at 24
  hours, or any chronometer-like claim. Don't offer general figures for
  the positions in their place.
- **Name the conditions in the line**: the lift angle used (and that it
  is an assumption when the calibre table says so), whether the rate
  was corrected for the sound card's clock (a `session` reading's
  `measurement.calibrated`; for `analyze`, a `clock` block, from a
  stored or given card error or the USB microphone's 1 kHz lines;
  without one it wasn't, and a buyer's timegrapher may read a few s/d
  differently), and that it is one position.
- **A once-a-minute wobble** (a regular change at the fourth wheel's turn) belongs in the listing as
  measured, with its size: "amplitude varies by about 18° once a minute,
  dial up". Tell the seller what it means for a buyer: something worth a
  watchmaker's look at the next service, not a fault that stops the watch.
  Ten minutes in one position can't say more.

An example: *"Timegraphed dial up, fully wound, three 10-minute
recordings: about 240° amplitude (lift angle 52° assumed; the maker
publishes none), beat error about 0.4 ms, rate about −5 to −8 s/d
(sound card clock not calibrated). Amplitude varies about 18° once a
minute. One position only."*

## Recording

`timegrapher record` records one position for `--seconds`, then stops; it
listens only while it runs. Run it only when the person is ready and
knows it is listening. Tell them when it starts and when it ends; they
will want to know when they can move the watch again.

A build without `record` (check `timegrapher --help`): record with the
system's tools, 48 kHz, 16-bit, mono, WAV or FLAC (the commands per OS
are in [setup-and-microphone.md](setup-and-microphone.md#recording)),
name each file after its position (`watch_DU.flac`, ...), and write the
session file with `timegrapher session --init FOLDER`, adding `wind_h =
24` to the 24-hour takes.

## What not to do

- Don't skip step 1 to save time. Most bad results are a bad set-up.
- Don't change their computer's settings without asking, and don't open
  the watch, or tell them to.
- Don't give a verdict on one position. Dial up alone says whether the
  watch is alive and roughly how healthy; the positions say what's wrong.
- Don't read the results until the session is in: individual positions
  drift around as the watch settles, and the comparison between them is
  the point.
