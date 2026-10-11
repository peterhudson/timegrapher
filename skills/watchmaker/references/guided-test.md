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

- **What is the watch?** Make and model, and the calibre if they know it
  (it's often on the maker's website, or in the papers). The calibre gives
  the beat rate and the lift angle; without the right lift angle the
  amplitude can be 10–15° out.
- **When was it last serviced**, roughly, and **how is it worn**: every
  day, now and then, or in a drawer. You need both to judge the results;
  the numbers alone are not a service verdict.
- **Has anything happened to it?** A knock, a fall, sitting next to a
  speaker, a phone case or a laptop with a magnetic clasp, or a sudden
  change in how it keeps time.

Look the calibre up in the program's calibre table: `timegrapher
calibres NAME --json` (in a build without it, `timegrapher long --calibre
X` lists the known names when X is not one of them), then in
[reading-the-numbers.md](reading-the-numbers.md#lift-angle). Take the lift
angle from the table, never from memory. Note bph and lift. If the calibre is not known, say so: you'll use 52° and let the
beat rate be found, and the amplitude is then good to ±10–15°.

## 1. The microphone (5 minutes)

Tell them: *"Put the watch in the stand's clamp, dial up, the way you'd
lay it on a table. Make sure it's held firmly; a loose watch sounds like a
faulty one."*

Run `timegrapher doctor --json` (with `--device NAME` from
`timegrapher devices` if the stand isn't the default input). Go on only
when `verdict` is `ok` and `check.signal_band` is `good` (or `fair`, if
nothing you try improves it: then the rate is sound but amplitude and
beat error are less sure, and you say so in the result).

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
sounds healthy; now let's do the positions"*) and go on.

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
as it doesn't move.

Record each position into one session folder for this watch. The first
take writes the session file, with the watch and calibre:

```sh
timegrapher record FOLDER --position DU --seconds 120 --calibre NAME --watch "their words" --json
timegrapher record FOLDER --position DD --seconds 120 --json
...
```

Each run says on stderr when it starts and prints `done: the watch can
be moved` at the end; tell the person both. A take that exits 3
(`verdict` `no_watch_heard` or `clipping`) is not added to the session:
fix the cause and record that position again before you ask them to
turn the watch. A bad position is cheaper to redo now than tomorrow.

## 5. Read the session

`record` has written the session file (watch, calibre, bph, lift,
`settle_s = 30` and one entry per take). Pass `--tolerance cosc` for a
chronometer, or `ladies` for a small ladies' watch; men's is the default.

Run `timegrapher session FOLDER --json` and read it as
[findings.md](findings.md) says: first whether the measurement is sound,
then the findings in order of severity, then the numbers behind them.
Write the answer as SKILL.md's "How to write the answer" says, with the
service history and wear from step 0 in the verdict.

Then look for possible faults in the shape of the Tick and the Tock:
`session` doesn't carry them, so run `timegrapher analyze
TAKE/audio-001.wav --calibre NAME --json` on each take (`take` in
`record`'s output) and read `faults[]` as
[findings.md](findings.md#faults-the-program-names) says. One that shows
on the same side in several positions is worth passing on to a
watchmaker; one in a single position is more likely the set-up.

If the fully-wound half already shows a fault (`severity` `fault`), say
so now: the 24-hour half will not change that answer.

## 6. Twenty-four hours later (optional, 15 minutes)

Tell them, when you give the first result: *"If you can, don't wind it or
wear it, and leave it face up somewhere still. Come back in 24 hours from
when you wound it and we'll do the same six positions again. That shows
whether the spring still has the strength to keep it going at the end of
a day."* An automatic left off the wrist does not wind itself, so it is
the same test.

Then repeat step 4 into the same folder with `--wind-h 24` on every
take; `record` marks each take with it in the session file.

`session` then gives the isochronism per position, Im, Ie and N. What
they mean is in [reading-the-numbers.md](reading-the-numbers.md#isochronism-full-wind-against-24-hours).

## 7. A long run (only if something points at it)

If the trace in a position wanders or waves (`cycle_wheel`,
`rate_unsteady`, or a two-state note in [findings.md](findings.md)),
offer a long run: an hour or more in that one position, read with
`timegrapher series` (is it steady, and if not, which wheel) and
`timegrapher long` (the report). See [long-runs.md](long-runs.md).

## A worked example: a Rolex 3235, dial up for two hours

The take `ym42_DU_2h` in the project's public test corpus is a Rolex
3235 (28,800 bph, lift angle 55° from the calibre table) recorded dial up
for two hours in twelve ten-minute files, one `session` reading each.
What `session` says, and how to reason from it:

| Readings | Rate s/d | Amplitude | Beat error | Findings |
| --- | --- | --- | --- | --- |
| 1–2 (first 20 min) | +49, +48 | 252° | 0.27 ms | `rate_unsteady` (10 s rates +20 to +75), `amplitude_tolerance` |
| 3–8 (20–80 min) | +35 falling to +14 | 247–252° | 0.30–0.33 ms | `rate_unsteady` in all six, `amplitude_tolerance`, `cycle_wheel` (fourth wheel) in four |
| 9–12 (last 40 min) | +8 to +10 | 247–251° | 0.33–0.35 ms | `amplitude_tolerance`, `cycle_wheel` in two |

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
3. **Amplitude is 247–252° dial up** at the table's lift angle, below the
   270–310° a recently serviced modern calibre usually shows fully wound,
   and steady over two hours. On its own that is a reason to look further,
   not a verdict.
4. **Beat error 0.27–0.35 ms**: good.
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
  The program keeps no history of its own; the folders and this table
  are the record, and next year's test is compared against them.

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
