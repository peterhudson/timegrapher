# 6. The Paper Strip

The Paper Strip draws one dot for every beat, the way a printing
timegrapher marks its paper. It shows what the readings summarise: how
steadily the watch keeps time, beat by beat.

![The Paper Strip on an opened take, marks 1 to 15](images/paper-strip.png)

## Reading the Strip

- **Along the strip** is time: the take's minutes and seconds.
- **Across the strip** is how early or late each beat came against a
  perfect clock beating exactly at the nominal beat rate, every 125 ms at
  28,800 bph. **Early** is up, **Late** down (with Direction set to
  Down, early is to the right), as Witschi's Diagram view draws them,
  where a rising line is a watch that gains (Witschi, _Testing Methods for Mechanical Watches_,
  2025, p. 18).
- **Tick** dots are blue and **Tock** dots orange (mark 13).
- **The amplitude** is drawn over the dots as a violet line, on its own
  scale at the right (mark 14). The rate and the beat error can be drawn
  the same way (mark 1).

| What you see | What it means |
| --- | --- |
| Two flat lines | The watch keeps the nominal rate. |
| Two lines sloping up | Each beat comes a little sooner: the watch gains. |
| Two lines sloping down | The watch loses. |
| A gap between the two lines | The beat error, timed from the drop. |
| Lines that bend | The rate is changing. |
| Dots scattered across the lines | Jitter: uneven beats, or a poor sound. |
| A sudden step in both lines | A jump: a knock, or lost sound. The readings skip it. |
| Faint grey dots | Beats in the settling time, or masked. They aren't read. |

**Good practice:** two parallel lines of dots are the beat error, not a
fault in the reading: "Occasionally, two parallel lines appear (as in
Example 2), which indicates a beat error" ([Witschi 2025](https://www.witschi.com/wp-content/uploads/firmwares/8Z1UtLGf/20251118_witschi_testing-methods-for-mechanical-watches_en.pdf), p. 18). Dots strewn
off the lines call for a better signal (chapter 3) before any
regulation; Witschi's advice for stray points is to change the signal
level: "If such points occur frequently above the line, it is advisable
to reduce the signal amplification" (p. 18).

**A line that runs off one edge comes back on the other,** like paper
wrapped round a drum. So the strip's **Width** is its zoom.

## The Toolbar

| Mark | Control | What it does | Why you'd use it |
| --- | --- | --- | --- |
| 1 | **Draw Over the Strip** | Three switches: the rate (magenta), amplitude (violet, on at first) and beat error (amber) readings drawn as lines over the dots, each on its own scale, written at the right (above the strip with Direction set to Down). | To see at a glance whether a wobble in the rate comes with a dip in amplitude. |
| 2 | **Width** | How much time fits across: from ±0.1 ms to ±60 s, typed or picked. | Narrow (±1 ms) to see a good watch's beat error; wide to keep a drifting watch from wrapping. |
| 3 | **Length** | How much time fits along: picked from 10 s to three days; typed (`45`, `5 min`, `2h`, `1:30:00`) or zoomed, from 2 s up to 80 hours. | Short to follow the last few seconds; long to see a whole take. |
| 4 | **Fit** | Shows the whole take along the strip, and keeps growing with it. | To see everything at once. Zooming or looking back turns it off; changing the width leaves it on. |
| 5 | **Centre** and **Auto-Centre** | Centre puts the newest beats on the centre line once. Auto-Centre keeps them there. | To bring the trace back after it has wrapped or drifted. |
| 6 | **Rate Line** and **Parallel Guides** | The Rate reading drawn as a magenta line over the beats it was fitted to, and faint lines at the same slope across the strip. Both on at first. | If the dots follow the line, the reading describes them well. The eye judges whether lines are parallel better than it judges a slope. |
| 7 | **Mask** | Leaves stretches out of the readings. See [Masking](#masking). | To take a knock or a handling stretch out of every reading. |
| 8 | **Position Brush** | Marks a stretch as one position. See [The Position Brush](#the-position-brush). | To correct the position after the fact. |
| 9 | **Direction** | **Across** (time left to right, the default) or **Down** (newest at the top, as on the open tg program). The arrow shows the way the paper runs, and a click turns it. | Down, if you're used to a printed strip: a gaining watch leans right as it rises. |
| 15 | **Still Paper** | Off, the paper slides along under the pen, the newest beats at its edge. On, the paper stays put and the pen sweeps along it. See [Still Paper](#still-paper). Kept between runs. | To read a live trace without it moving under your eye. |
| — | **Time of Day** | The time of day on this computer's clock under each of the strip's time labels. See [Time of Day](#time-of-day). Kept between runs. | To match the strip to when something happened. |
| 10 | **Export** | Saves the strip as a PNG or SVG picture. | To share it, or put it in a report. |

**Opening a take** turns Fit on and sets the width to the narrowest listed
width that holds all the take's beats without wrapping (one stray dot in a
thousand at each end left out), so the whole take shows at once.
**Listen, or a new test,** then brings back the width, length and Fit the
strip had before the take was opened. A width or length you changed by
hand meanwhile stays as you set it.

## Time of Day

**Time of Day**, the clock in the Paper Strip's toolbar, writes the time of day on this computer's clock (`14:35:20`) under each of the strip's time labels, so the take's time and the clock's show together; with Direction set to Down, the take's time is above each line and the time of day below it. The cursor shows both too (`1:23.45  14:35:20.45`). With lines six hours or more apart, every time of day names its day (`Thu 06:00`). Closer, the first one after each midnight names it (`Fri 00:04:27`), and so does the take's first when the take runs past midnight; on such a take the cursor names the day too (`Fri 00:04:27.31`). It is kept between runs.

- A take on the microphone counts from when its recording began, and each break in its sound moves the clock on by the time that passed.
- A take opened counts from when its `session.json` says it began, and each break in its sound moves the clock on the same way. One segment of a take opened on its own has no start time of its own.
- A take with no start time shows its own time only.

Why you'd use it: to match the strip to when something happened, such as a knock, a wind or the watch picked up.

## Still Paper

![Still Paper on, 1 min 18 s into a Quick Listen with a Length of 30 s, marks 1 to 3](images/paper-strip-still.png)

**What it shows:** the same beats, with the paper held still. The pen
sweeps along the strip and starts again at the start every Length, as a
scope draws, wiping the last lap just ahead of it.

**Why you'd use it:** on moving paper every dot slides along all the
time. On still paper a dot stays where it was drawn, so the eye can rest
on the trace while you turn the watch or regulate it, and each lap is
drawn over the same place as the last.

| Mark | Part | What it shows |
| --- | --- | --- |
| 1 | **Still Paper** | The switch, lit while on. |
| 2 | The pen | The newest beat, with a blank, 4% of the strip, wiped just ahead of it. |
| 3 | The last lap | Still on the paper ahead of the blank, until the pen comes round to it. |

- **Where a beat goes:** laps start at the take's 0:00, so with a Length
  of 30 s each lap starts on a whole half minute. The time labels give
  the take time of the ink under them, so they jump at the blank.
- **Lines and bands** (the Rate Line, the guides, masks, the track) are
  drawn a lap at a time, and none runs across the blank.
- **Looking back, paused, or on an opened take,** the pen is at the
  strip's end. Dragging or the wheel moves it through time, as on moving
  paper. Zooming time changes the Length about the pen.
- **Fit** is greyed while the paper is still: there is nothing to fit.

**In the picture** a Quick Listen is 1 min 18 s in, with a Length of
30 s. The pen is in its third lap, from 1:00 to 1:18. Ahead of the blank
is the end of the lap before, from about 0:49 to 1:00, and the
amplitude line from that lap carries on where the pen hasn't reached.

## Moving Around

| To | Do |
| --- | --- |
| Look back through the take | The mouse wheel or a swipe, or drag along the time axis. Back at the newest beat it is live again. |
| Zoom time | **Ctrl** and the wheel (**Cmd** on a Mac), or a pinch. |
| Zoom the width | **Alt** and the wheel (**Option** on a Mac). |
| Slide the trace across | Drag across the strip. |
| Return to the newest beats | Double-click. |

**Looking back moves the cards too.** With the strip moved off the
newest beats, the readings cards, the Tick Tock Profile and the Scope
read the moment at the strip's newest edge. The strip's label says when
("at 0:40").

## The Track

The band along the top of the strip shows where the watch was and what
was left out.

| Mark | Part | What it shows |
| --- | --- | --- |
| 11 | The position | Each stretch in one position, in teal, named at its start. |
| 12 | **Settling** | The solid band after each change of position, while the balance settles. Not read. It then fades over 40 s, so each change shows. |

- **Masked** stretches show as grey hatching.
- During and after a Positions Test, the time before the test is left
  plain. On the track the time the test didn't use is plain too; on the
  strip it is hatched.

## Masking

![The Mask tool on, with 15 seconds masked, marks 1 to 5](images/mask.png)

**What it does:** leaves a stretch out of every reading, as if it were a
break. Nothing is deleted: the beats stay, faint under hatching.

| Mark | What you see |
| --- | --- |
| 1 | **Mask**, lit while on. |
| 2 | The masked stretch, named on the track. |
| 3 | Lines drawn over the strip break there. |
| 4 | The sparklines on the cards break too. |
| 5 | **Changes not saved**, on an opened recording. |

**How to use it:**

1. Turn on **Mask** (the scissors).
2. Drag along the strip over the stretch. A drag under half a second is
   ignored, as a slip of the mouse.
3. Drag a masked stretch's edge to move it.
4. Click a masked stretch, or right-click it with Mask on or off, to
   bring it back. **Edit > Remove All Masks…** brings them all back.

**What a mask changes:**

- No reading reaches across one, and nothing inside one is read.
- A chart's line comes back only once a whole Average Over time has been
  heard after it.
- The charts, the whole-take figures, the Distributions, Steadiness and a
  Positions Test all leave the masked beats out.
- A reading whose span takes in masked sound says so in a chip, such as
  **6.8 s Masked**.

**Why you'd use it:** a bump of the stand, picking the watch up, or a
noise in the room shows as a jump or a scatter. Masking it keeps one
moment from moving a whole take's figures.

**Saving masks:** a saved take keeps its masks, and the command line's
`timegrapher session` leaves them out too. On a recording opened from
disk, **File > Save** writes them beside it.

## The Position Brush

**What it does:** marks a stretch of the strip as one position, on a live
take or one opened. It corrects the position record after the fact.

1. Pick a position from the brush's menu (mark 8).
2. Drag along the strip. A teal preview says the position and the times,
   such as "CD 4:30 to 5:30".
3. **Stop Painting**, at the end of its menu, or **Esc**, puts it down.

- **Why:** you turned the watch but forgot to press the position button,
  or a recording from another app has no positions.
- Painting never starts a settling: it is a correction, not a turn of the
  watch.
- The positions recorded stay under the painting. **Right-click** a
  painted stretch on the track to take the painting off.
- Drag a change of position along the track to move it.
- Greyed during a Power Reserve Test, during or after a Positions Test
  (the test's take keeps its own positions), and while nothing is on show
  yet.

---

Previous: [The Readings Cards and the Calibre Card](05-the-readings-cards.md) ·
Next: [The Tick Tock Profile and the Scope](07-profile-and-scope.md)
