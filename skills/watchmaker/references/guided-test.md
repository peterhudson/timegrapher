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

Record 60 s dial up (see [Recording](#recording) below) and run
`timegrapher analyze FILE --lift L --bph B --json`.

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

Name the files after the position: `watch_DU.flac`, `watch_DD.flac` and
so on, in one folder for this watch. After each one, check the file
quickly (`analyze --json`: `watch_heard`, beats found against expected,
no clipping) before you ask them to turn the watch. A bad position is
cheaper to redo now than tomorrow.

## 5. Read the session

Write the session file once (`timegrapher session --init FOLDER`, then
fill in):

```toml
watch = "their words for the watch"
calibre = "2824-2"
bph = 28800
lift = 52
settle_s = 30
tolerance = "mens"     # "cosc" for a chronometer; "ladies" for a small ladies' watch
```

Run `timegrapher session FOLDER --json` and read it as
[findings.md](findings.md) says: first whether the measurement is sound,
then the findings in order of severity, then the numbers behind them.
Write the answer as SKILL.md's "How to write the answer" says, with the
service history and wear from step 0 in the verdict.

If the fully-wound half already shows a fault (`severity` `fault`), say
so now: the 24-hour half will not change that answer.

## 6. Twenty-four hours later (optional, 15 minutes)

Tell them, when you give the first result: *"If you can, don't wind it or
wear it, and leave it face up somewhere still. Come back in 24 hours from
when you wound it and we'll do the same six positions again. That shows
whether the spring still has the strength to keep it going at the end of
a day."* An automatic left off the wrist does not wind itself, so it is
the same test.

Then repeat step 4 into the same folder with `_24h` in the names
(`watch_DU_24h.flac`, ...) and add `wind_h` to each recording in
session.toml (`0` for the first six, `24` for the second; the file-name
shortcut assumes full wind):

```toml
[[recording]]
file = "watch_DU.flac"
position = "DU"
wind_h = 0

[[recording]]
file = "watch_DU_24h.flac"
position = "DU"
wind_h = 24
```

`session` then gives the isochronism per position, Im, Ie and N. What
they mean is in [reading-the-numbers.md](reading-the-numbers.md#isochronism-full-wind-against-24-hours).

## 7. A long run (only if something points at it)

If the trace in a position wanders or waves (`cycle_wheel`,
`rate_unsteady`, or a two-state note in [findings.md](findings.md)),
offer a long run: an hour or more in that one position, read with
`timegrapher series` (is it steady, and if not, which wheel) and
`timegrapher long` (the report). See [long-runs.md](long-runs.md).

## Recording

Until the program has a `record` command, record with the system's tools:
48 kHz, 16-bit, mono, WAV or FLAC, for the length the step says (the
commands per OS are in
[setup-and-microphone.md](setup-and-microphone.md#recording)). Tell the
person when it starts and when it ends; they will want to know when they
can move the watch again.

## What not to do

- Don't skip step 1 to save time. Most bad results are a bad set-up.
- Don't change their computer's settings without asking, and don't open
  the watch, or tell them to.
- Don't give a verdict on one position. Dial up alone says whether the
  watch is alive and roughly how healthy; the positions say what's wrong.
- Don't read the results until the session is in: individual positions
  drift around as the watch settles, and the comparison between them is
  the point.
