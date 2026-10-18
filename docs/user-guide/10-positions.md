# 10. Positions and the Positions Test

A watch runs differently depending on how it lies, because gravity pulls
on the balance differently in each position. So watchmakers measure a
watch in several positions, and compare. Timegrapher keeps every position
apart, and leads you through a test of all six.

## The Six Positions

![The six positions: Dial Up and Dial Down lying flat, then Crown Down (9H), Crown Left (6H), Crown Up (3H) and Crown Right (12H) standing upright, with the crown drawn in orange](images/positions.svg)

| Position | Short | Swiss name | How the watch lies |
| --- | --- | --- | --- |
| Dial Up | **DU** | CH | Flat, dial facing up |
| Dial Down | **DD** | CB | Flat, dial facing down |
| Crown Up | **CU** | 3H | Upright, crown pointing up |
| Crown Down | **CD** | 9H | Upright, crown pointing down |
| Crown Left | **CL** | 6H | Upright, crown pointing left, seen from the dial |
| Crown Right | **CR** | 12H | Upright, crown pointing right, seen from the dial |

The Swiss names for the upright positions are the hour on the dial that
points up: with the crown at 3 o'clock, crown up puts 3 at the top (3H),
and crown right puts 12 at the top (12H). CH and CB are Witschi's codes for
dial up and dial down. On pocket watches the same upright positions are
often called pendant up, down, left and right (PU, PD, PL, PR). The app
names positions as "Crown Up (CU, 3H)", in words with both codes.

## Setting the Position by Hand

**Position** under Watch says how the watch lies. Changing it is a new
measurement:

- **While listening**, a change of position starts a new stretch. The
  Paper Strip keeps all its beats, and marks and shades each position's
  stretch, with a ticker naming the position along it. The charts mark the
  stretches the same way.
- **The readings** don't reach back past the change, and the Tick Tock
  Profile starts again.
- **The first 10 seconds** in a new position are **settling**: the ticker
  says Settling, the beats are drawn faintly, and they are left out of the
  readings, the charts, the Distributions and Steadiness. They are still
  kept in the sound.
- **The Distributions and Steadiness** take only the stretches in the
  position the watch is in now. Dial up for 5 minutes, dial down for 10,
  then dial up again: the two dial-up stretches are tested together and
  the dial-down one is left out.

A saved session records each position's stretch, so
`timegrapher session` reads the same seconds the app did.

### When the Beats Stop

When the beats stop for more than 3 seconds while listening and then come
back, with no change of position marked, the app asks whether the watch was
moved, with a button for each position and **Not Moved**. The question goes
by itself a minute later.

## The Positions Test

The **Positions Test** pane leads you through all six positions, measuring
each one the same way, and fills in a table as it goes. It is a tab beside
the Tick Tock Profile to begin with. Before anything is listened to, it
sits above the welcome card, so a test can start straight away.

### The Toolbar

- **Start Test** (**Start a New Test** once one has finished) starts the
  test, and starts listening if need be.
- **Load Full-Wind Results…** loads a test made just after winding, for
  the test 24 hours later (below).
- **Settle**: how long the watch settles after each turn before it is
  measured: 20 seconds unless you change it (5 to 600 s). Witschi's
  *Testing Methods for Mechanical Watches* (D40e) gives the same 20 s
  stabilisation time.
- **Measure**: how long each position is measured: 60 seconds unless you
  change it (20 to 3600 s).
- **Save as CSV…**, **Copy** and, after a test just after winding,
  **Save Results for 24 Hours…** (below).

Settle and Measure are kept between runs.

### Running the Test

1. Wind the watch fully and put it on the microphone **dial up**.
2. Click **Start Test**. A banner at the top of the pane names the
   position and shows a progress bar while the watch settles, then while
   it is measured.

   ![The Positions Test measuring Dial Up: the banner says Dial Up (1 of 6), Measuring, 55 of 60 s, with Pause, Restart DU Measurement and Stop](images/positions-measuring.png)

3. When the position is measured, the banner says which position to turn
   the watch to next. Turn it, then press the button naming the position,
   such as **Start DD Measurement**. Nothing is measured until you press
   it.

   ![The Positions Test after Dial Up: the banner says Turn the watch to Dial Down, with Start DD Measurement, and the table shows Dial Up's rate, difference from the mean, amplitude and beat error](images/positions-turn.png)

4. Repeat for each position, in the app's order: Dial Up, Dial Down,
   Crown Down, Crown Left, Crown Up, Crown Right.
5. After the sixth, click **Finish** to read the six together.

While it runs:

- **Restart DD Measurement** (naming the position just measured, or the one
  under way) settles and measures that position again, after a knock for
  instance.
- **Pause** holds the test, leaving out what the position measured so
  far; **Resume** settles and measures it again from the start.
- **Stop** ends the test and clears its table. The session and its
  positions stay.

Only the measured seconds in each position count.

### What the Strip Shows During a Test

From the moment the test starts, the Paper Strip, its position ticker and
the charts mark the test's phases, each in its own grey:

- **Before Test**: the sound before the test began, left plain, with a line
  where the test begins;
- **Settling**: each settle, lightly shaded;
- each position's **measured stretch**, the darkest shade, named by the
  position;
- **Not Used for Positions Test**, hatched: the time after a measurement
  while the watch is being turned, a measurement given up for a restart or
  a pause, the pause itself, and the part of a position measured when the
  test was stopped.

Beats outside the measured stretches are drawn faintly. The marks stay
until a new test or a new session.

### The Results

![The finished Positions Test: a row for each of the six positions with its rate, difference from the mean, amplitude and beat error, then the mean, the largest difference between the rates, and vertical less horizontal](images/positions-results.png)

The table has a row for each position, named in full ("Crown Up (CU,
3H)"), with:

- its **rate**, in s/d (on the microphone's measured clock, when there is
  one; see [True Clock](05-readings.md#true-clock));
- **From Mean**: its rate less the mean of the positions measured;
- its **amplitude**, in degrees;
- its **beat error**, in ms (a size).

Under the six rows:

- **Mean**: the mean of each column;
- **Largest difference between the positions' rates**, the value
  watchmakers call D;
- **Vertical less horizontal**, the mean of the four upright positions less
  the mean of the two flat ones, for rate and amplitude: the value called
  DVH;
- a plain line for each condition found in a position's sound: no regular
  beat (with its timing scatter), clipping, or less than 10 seconds of
  beats.

The figures are the same as `timegrapher session` gives for the same
stretches of sound (see [The Command Line](12-command-line.md#session)).
The test gives no verdict, and no pass or fail: the figures are for you to
judge.

**Save as CSV…** saves the watch's name, calibre, beat rate and lift angle,
when the test began and its settling and measuring times, then the table,
the means, the differences and those lines, as a file any spreadsheet
opens. **Copy** puts the same on the clipboard, with tabs between the
columns, to paste into a spreadsheet.

## Full Wind Against 24 Hours

Watchmakers often compare a watch at full wind with the same watch 24
hours later, as the mainspring lets down. The Positions Test does both
halves:

1. **Just after winding**, run the test. When it is done, click **Save
   Results for 24 Hours…**. (Saving the session keeps the same results too,
   as `positions-test.json` in its folder.)
2. **Leave the watch** for 24 hours, neither wound nor worn.
3. **24 hours later**, click **Load Full-Wind Results…** and pick the file.
   The pane shows those results as a **Full Wind** table, with when that
   test began, the watch and the calibre. **Remove** sets them aside.
4. **Click Start Test** and run the six positions again. The banner names
   the hours since the full-wind test began.

As it goes, the pane adds a **Full Wind Against N Hours** table: each
position's rate and amplitude at full wind and now, and the change; then
**Im**, the largest change in rate with crown right left out, as NIHS 93-10
takes it; and **Ie**, the size of the change in the mean rate. Its CSV and
Copy add the full-wind test's start, the hours since, and that table.

## From the Command Line

`timegrapher record` records one position at a time into a test folder,
and `timegrapher session` reads the folder into a multi-position report in
the style of Witschi's SEQ mode, with the characteristic values X, D, DV,
DH, DVH, Di, Im and Ie and the measurements behind each. A session saved by
the app reads the same way. See
[The Command Line](12-command-line.md#record) and
[Test sessions](../sessions.md).

---

Previous: [Calibres and Lift Angles](09-calibres.md) ·
Next: [Sessions and Recordings](11-sessions-and-recordings.md)
