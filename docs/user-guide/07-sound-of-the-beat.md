# 7. The Sound of the Beat

The readings are worked out from marks the software places on the sound
of each beat: where the unlock starts and where the drop starts. Two
panes show that sound, so you can see what the readings stand on. The
**Tick Tock Profile** shows the typical beat, averaged; the **Scope** shows
single beats as they were heard.

## The Tick Tock Profile

![The Tick Tock Profile, stacked: the Tick in blue above the Tock in orange, each with its Unlock and Drop marks, the three sounds dashed, and a line of figures above each plot](images/tick-tock-profile.png)

The Tick Tock Profile shows the typical sound of the Ticks (blue) and of
the Tocks (orange) over the averaging time (up to 60 seconds, so it
describes the same beats as the readings):

- **The line** is the beats' typical loudness at each moment.
- **The shaded band** is where the middle 80% of beats fall.
- **Time runs from left to right**, in milliseconds, from before the
  unlock to after the drop.
- **Ticks and Tocks share one vertical scale**, so a quieter side shows
  smaller.

### The Marks

The solid lines are the edges the readings come from:

- **Unlock** (green): where the beat first rises above the noise.
- **Drop** (indigo): where the drop starts.
- **Peak** (a thinner indigo line): the drop's loudest point.

The amplitude comes from the time between the Unlock and the Drop, and the
beat error from the Unlocks.

The dashed brown lines, **1 Unlock**, **2 Impulse** and **3 Drop**, mark
where each of the three sounds rises halfway up its own climb. They come
from a separate measurement of the beat's shape, so they sit near, but not
exactly on, the solid edges. A dashed line is missing when that sound
can't be told apart from its neighbour, such as a soft unlock that merges
into the impulse. The flat dashed line is the noise floor: the level
before the beat starts.

Both kinds of mark are measured on the averaged sound, not on single
beats.

### The Figures Above Each Plot

Above each plot is a line of figures, such as:

> **Tick** 40 beats · unlock −9.09 ms · drop −0.61 ms · unlock to drop
> 8.48 ms · amplitude 259°

That is: the number of beats averaged; where the unlock and drop marks
sit, in milliseconds from that side's own drop; the time from unlock to
drop; and the amplitude that time gives with the lift angle in use. This
is where the amplitude on the card comes from. Working it through by hand
with the formula in
[How a Timegrapher Hears a Watch](01-how-it-works.md#amplitude) gives the
same figure.

### The Toolbar

- **Layout**: **Stacked** puts the Tick above the Tock; **Beside** puts
  them side by side.
- **Time From**: **Shared** (the default) draws the Tick and Tock on one
  clock. 0 ms is where a watch in beat would drop, and each side sits half
  the beat error from it, so the two Drop lines stand the beat error from
  the drop apart, and the two Unlock lines the beat error from the unlock.
  **Own Drop** measures each side from its own drop, so both drops sit at
  0 ms and only the Unlock lines show a difference.
- **Scale**: **Linear** shows the loudness as the engine measures it;
  **dB** shows it in decibels below each side's loudest point, which makes
  the quiet unlock easier to see.
- **Edges** and **Sounds 1, 2 and 3** switch the solid and dashed marks on
  and off.
- **Average Over**: **As Readings** unless you pick a time of its own, up
  to 60 seconds. A longer time gives a steadier shape.

### Looking Back

When you look back through a session with the time slider or the Paper
Strip, the Tick Tock Profile shows the sound the session kept nearest
before that moment (it keeps one every 2 seconds). For a recording
analysed all at once it is worked out from the file when you stop on a
moment.

### Why Look at It

The profile is the place to look when a reading seems wrong. The readings
stand on the Unlock and Drop marks, so if a mark sits somewhere odd (in
the middle of a rising slope, say, or on a knock in the background), or
the Tick's and Tock's marks disagree in a way the sound doesn't, the
readings built on them are suspect. `timegrapher profile` draws the same
picture from the command line (see
[The Command Line](12-command-line.md#profile)).

## The Scope

![The Scope at Span 20 ms with Beats 8: the last eight Ticks drawn over each other in one row and the last eight Tocks in another, lined up on the drop, with the Unlock and Drop marks](images/scope.png)

The Scope shows the sound itself, beat by beat, as an oscilloscope on the
microphone would: what the engine heard, high-passed at 1 kHz so hum and
rumble don't hide it, and not averaged. It is a tab beside the Tick Tock
Profile.

**Span** sets how much of each beat it shows. The 20, 200 and 400 ms spans
are the ones Witschi's training course gives for its scope view (Witschi,
doc. 71.1010D35e, p. 12):

| Span | Shows |
| --- | --- |
| **5 ms** | 3.5 ms before the drop to 1.5 ms after: the drop close up |
| **10 ms** | 8 ms before the drop to 2 ms after |
| **20 ms** | 14 ms before the drop to 6 ms after: one whole beat, the Tick and the Tock in rows of their own, lined up on the drop, with the profile's Unlock and Drop marks |
| **50 ms** | 15 ms before the drop to 35 ms after: the beat and what rings on after it |
| **One Beat** | From 15 ms before one beat to 15 ms before the next: 125 ms at 28,800 bph, 200 ms at 18,000 |
| **200 ms** | One row each from 20 ms before the beat to 180 ms after, so the next beat shows too |
| **400 ms** | Every beat in one grey row, three or four beats at once |

**Beats** draws the latest 1, 4, 8 or 16 beats over each other, the older
ones fainter. One beat shows exactly what was heard; several show how
much the beats vary from one to the next, which the Tick Tock Profile
averages away. A beat that differs from the rest stands out.

While listening, the Scope shows the latest beats. Looking back, or on a
recording, it shows the beats up to the moment shown, read from the
recording.

---

Previous: [The Paper Strip](06-paper-strip.md) ·
Next: [Over Time](08-over-time.md)
