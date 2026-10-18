# 6. The Paper Strip

The Paper Strip is the classic timegrapher trace. Bench timegraphers used
to print it on a roll of paper: one dot for every beat, with the paper
moving steadily on. The pattern the dots make shows the rate, the beat
error and how steady the watch is, at a glance.

![The Paper Strip running down: two lines of dots, blue Ticks and orange Tocks, leaning right as they rise, with the red Rate Line over the newest beats, faint red Parallel Guides, and the purple amplitude line drawn over the strip](images/paper-strip.png)

## How to Read It

**Along the strip is time.** With **Direction: Down** (the default, as on
tg), the newest beats are at the top and older ones move down. Time is
labelled in minutes and seconds.

**Across the strip is how early or late each beat came**, in
milliseconds, against a perfect clock beating exactly at the beat rate:
every 125 ms at 28,800 bph. A beat that comes before that clock's beat is
**early**, after it **late**; the words Witschi's training course uses
(Witschi, doc. 71.1010D35e, p. 6). Running down, early is to the right.

From that, the patterns:

![Five small strips: on time, two upright lines; gaining, lines leaning right as they rise; losing, lines leaning left; a bigger beat error, two lines further apart; and a line that runs off one edge coming back on the other](images/strip-patterns.svg)

- **A watch on time** keeps the same distance from the perfect clock, so
  its dots run straight up the strip.
- **A gaining watch** beats a little early each time, so its line leans
  towards early, to the right, as it rises (/). **A losing watch** leans
  left (\\). The steeper the lean, the bigger the rate.
- **Ticks and Tocks** make two lines, blue and orange. **The gap between
  them is the beat error** (timed from the beat as a whole, the smaller
  beat error figure on the card).
- **The spread of the dots** across each line is the jitter.
- **A line that runs off one edge comes back on the other.** So the
  strip's width works as a zoom: narrow it to see small changes, widen it
  to keep a fast or slow watch on the strip.

With **Direction: Across**, the strip lies on its side: time runs left to
right with the newest beats on the right, and early is up. A gaining watch
then rises to the right.

![The Paper Strip lying across: time runs left to right, early is up, and a gaining watch's lines rise to the right, with the amplitude drawn over them in purple](images/paper-strip-across.png)

## The Toolbar

### Width and Length

- **Width** is how far either side of the centre the strip reaches, from
  ±0.1 ms to ±250 ms. Type it, or pick a common value: the list runs from
  ±1 ms, for a fine look at the beat error, to ±62.5 ms, which at 28,800
  bph is half a beat, the widest view in which Ticks and Tocks can't wrap
  onto each other.
- **Length** is how much time the strip shows. Type it as `45` (seconds),
  `5 min`, `2h` or `1:30:00`, or pick from the list, from 10 seconds to two
  hours.

### Direction

**Down** or **Across**, as above. The app keeps your choice between runs.

### Rate Line and Parallel Guides

- **Rate Line** (on unless you turn it off) draws the Rate reading as a
  thin solid red line over the beats it was fitted to, as tg does. If the
  dots follow it, the reading describes them well.
- **Parallel Guides** (on unless you turn it off) adds faint red lines at
  the same slope across the whole strip, one per grid step. The eye judges
  whether lines are parallel far better than it judges a slope, so dots
  that bend away from the guides show the rate changing.

In the current version the guides are drawn with the Rate Line, so
turning the Rate Line off hides them too.

### Draw Over the Strip

**Amplitude** (purple, on unless you turn it off), **Rate** (red) and
**Beat Error** (yellow) readings can be drawn over the strip as lines
against the same time, each on its own scale. Each line's scale is
written in its colour above the strip (beside it when the strip lies
across). With the amplitude drawn beside the dots, you can see at once
whether a wobble in the rate comes with a change in amplitude.

### Auto-Centre, Centre and Clear

- **Auto-Centre** keeps the newest beats on the centre line all the time,
  so the older ones slide away from it as the rate wanders. Sliding the
  trace by hand turns it off.
- **Centre** centres the newest beats once.
- **Clear** starts the readings and the strip again.

### Average Over

**As Readings** unless you pick a time of its own. It sets the averaging
time of the Rate Line and of the lines drawn over the strip. See
[Readings](05-readings.md#average-over).

## Using the Mouse

| Action | Does |
| --- | --- |
| Mouse wheel | Changes the length |
| Ctrl and the mouse wheel, or a pinch | Changes the width |
| Drag along the time axis | Looks back through the session, live or not |
| Drag across | Slides the trace across |
| Double-click | Returns to the newest beats and centres them |

Wherever the pointer is on the strip, a label gives its time from the
start, to the hundredth of a second (such as 1:23.45), and a thin line
marks the same moment on the charts.

## Positions on the Strip

When the watch changes position during a session, the strip keeps all its
beats and marks each position's stretch, shading every other one. A
ticker above the strip (beside it, when the strip runs down) names the
position over and over along its stretch, so you can see it even with both
ends of the stretch scrolled out of sight. During a Positions Test the
strip also marks the test's phases. See
[Positions and the Positions Test](10-positions.md).

## The Strip and the Charts Together

Set the strip to **Across**, put the charts under it, and set the charts'
time to the strip (**Match Strip**, in the charts' tab bar). Every time
axis then lines up, and the pointer's moment is marked on all of them at
once. See [Over Time](08-over-time.md#the-charts).

---

Previous: [Readings](05-readings.md) ·
Next: [The Sound of the Beat](07-sound-of-the-beat.md)
