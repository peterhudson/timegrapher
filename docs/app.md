# Desktop app

`timegrapher-app` is the live timegrapher screen: put the watch on the
microphone and it shows the paper-strip trace with rate, amplitude and beat
error, the way a bench timegrapher does. A recording, or a folder of
segments, opens analysed all at once to look back through; it can also be
replayed as if it were live.

```sh
cargo run --release -p timegrapher-app             # live from a microphone
cargo run --release -p timegrapher-app -- rec.flac  # analyse a recording
cargo run --release -p timegrapher-app -- --replay rec.flac  # replay it as if live
```

On Linux, building it needs the ALSA headers (`sudo apt install libasound2-dev`).

## The screen

Along the very top, the menu bar. **File**: Open Recording… (Ctrl+O, or
Cmd+O on a Mac), Open Folder…, Save… or Save Again (Ctrl+S), Save As…
(Ctrl+Shift+S), New Session and Quit (Ctrl+Q). **View**: a tick for the
readings and for each pane, Reset Panes and Appearance (Auto, Light,
Dark). **Help**: Check for Updates…, Check for Updates at Start, Report a
Problem… and, on Linux, **Add to Applications Menu**, which puts the app
and its icon (a black dial with red lines) in the desktop's menu, so the
window shows that icon too (it writes `timegrapher.desktop` and the icon
under `~/.local/share`, pointing at the program running now). Beside the
menus, one icon for each pane that shows or hides it (resting the pointer
on an icon names its pane; a pane showing has its icon lit, and the
charts' and distributions' icons take their colours).

**The Watch pane**, at the left to begin with, is where the sound comes
from and what to do with it, then the watch itself. It is a pane like the
others: drag it by its tab, or hide it and bring it back from its icon or
the View menu. At its top, **Microphone** or **Recording**. For the
microphone: the microphone menu and **Rescan**, **Start** (or **Stop** and
**Resume**), **New Session** when stopped, **Positions Test…**, **Save…**
(then **Save Again** and **Save As…**), and the microphone's Input Level,
peak meter and Show Every Input. For a recording: **Open…**, **Open
Folder…**, the path box, **Replay** (or **Stop**, **Cancel** while one is
being analysed, and **Analyse All**) and **Replay Speed**. Below, under
**Watch**: name, calibre, beat rate, lift angle and position. Opening a
recording, by **Open…**, **Open Folder…**, a path typed into the box and
Enter, or the command line, analyses it all at once, which is how a
watchmaker looks back through a take; **Replay** plays a file through as if
live (a folder's files one after the other, in the order it is analysed
in, with the watch from its session file), at **Replay Speed** (from half to 16 times its own speed, changeable
while it plays; the readings are the same at any speed), and **Analyse
All**, shown during a replay and after it, goes back to the whole
recording analysed at once without opening it again, keeping the settings
chosen under Watch. Dropping a file or folder on the window opens it too
where the desktop passes drops to the app (Windows, macOS and X11; not
Wayland). Clicking a group's name, such as Watch, folds it away to its
caption, and clicking it again opens it.

Every other pane's own settings sit in a toolbar across the top of the
pane, with the pane's **?** at its right; a toolbar too narrow for its
settings carries them onto a second line. The charts carry theirs in the
tab bar: **Match Strip**, on, shows the same stretch of time as the paper
strip, and off, the whole session. A choice between two options, such as
Stacked and Beside, switches to the other option wherever it is clicked,
so it can be flipped back and forth without moving the pointer. Titles and
labels are in Title Case. Across the top, the readings: five cards of one width, Readings (with
**Average Over**), Rate, Amplitude, Beat Error and Beat Rate, and below
them the panes. Along the bottom, the
status line. Every card, pane and reading has a **?** beside its name: resting
the pointer on it shows the plain answer in a line or two, and clicking it
opens the whole explanation in place (click it again, or anywhere else, to
close it). Every setting gives a short hint when the pointer rests on it. Before anything is
open, the main area offers the three ways in, beside the Watch pane: **Listen to a Watch**,
**Open a Recording…** and **Open a Folder…**. While a recording is being
analysed it shows how far the analysis has got, in hours of sound and time
left, with **Cancel**; a problem shows in a red banner floating over the
top of the main area, so nothing below it moves, until it is dismissed or
the next start clears it. A recording that can't be opened leaves the one
shown in place, and the path box goes back to it. Choosing **Recording**
while a stopped microphone session is in hand shows the ways in again
rather than that session's figures, and says the session is kept;
**Microphone** brings it back. The app follows the
desktop's light or dark setting (Appearance, in the View menu, overrides it),
and uses the Inter typeface with figures of even width, so readings don't
shift as they change.

**A folder of segments.** A long take saved as segments (the recorder's
hourly files, or an overnight run split into 10-minute FLACs) opens as one
recording: **Open a Folder…** on the opening screen, **Open Folder…** in
the Watch pane or the File menu, a folder typed into the path box, or `timegrapher-app FOLDER`.
A saved take opens from its own folder or from the folder it was saved
into, when that holds just the one take; with several, the message names
them so one can be picked.
Its WAV and FLAC files are joined end to end in the order its
`session.toml` lists them as `[[recording]]` entries, with any it doesn't
list after them; without such a list, in natural name order, where a run
of digits counts as a number (`part2` before `part10`, `take_part01-099`
before `take_part02-001`). The files' dates never change the order, but
when a file was written earlier than the one joined ahead of it by more
than that file's own length, the window says so, so a wrongly named file
can be spotted. **Replay** plays a folder's files one after the other in
the same order. A 16-hour night takes some minutes to analyse; the
window shows the hours done, the time left and the files in order, and
Steadiness then covers the whole night.

**Readings.** The **Beat Rate** card gives the beats per hour the
readings use: found from the beats while Beat Rate under Watch is Auto,
or the one set there, and says which. Rate (seconds per day gained, +, or lost, −), amplitude (degrees) and beat error (ms), each a
fit over the last few seconds of beats (10 s by default; type any time
from 1 s to an hour, or pick one of the presets from 2 to 60 s; the setting is
Average Over, in the Readings card). Early in a session, or after a pause or a gap of
more than 2 s with no beats, there are fewer seconds of beats than asked
for: the readings use what there is and say so ("190 beats in 24 s of
180 s"); they never fit across a pause or gap, since the watch may have
been moved or have drifted meanwhile. The panes that average (the Paper Strip's
Rate Line, guides and overlays, the Tick Tock Profile, the three charts and the three
distributions) follow Readings' Average Over unless set otherwise: each has
its own **Average Over** in its toolbar (the charts' in their tab bar),
"As Readings" by default or one of the presets, and a pane with its own
time names it in its tab ("Rate · 30 s"). A pane's own time sets everything
in it that Average Over would. Changing Readings' Average Over puts every
pane back on it, dropping their own times. The big amplitude is the average of the two
swings, in whole degrees, with a ± figure: the standard error of the
median of its 2 s windows, roughly how far it could be off from their
scatter alone. It is precision, not accuracy: a wrong lift angle moves
every amplitude together (1° of lift angle moves it about 4°).
Under it are the amplitude from the ticks (blue dot) and from the tocks
(orange dot). The sound can't tell which beat is
which pallet, so the first beat heard is called the tick;
which side is the entry pallet can't be told from the sound, so the sign
of the beat error follows which beat came first, as on tg. The big beat
error is timed from the unlock, as tg measures it (checked against tg
in the regression tests); beside it, smaller, is the beat error timed from the beat as a whole,
nearer the drop, which is the gap between the two lines on the strip.
Beside the rate, when it is at least 0.05 s/d, is its ± figure: the
standard error of the fitted slope, roughly how far the rate could be off
from the scatter of the beats alone; it shrinks with a longer averaging
time. Under the rate: how many beats the readings used and
the jitter: how far single beats land from the steady line the rate and
beat error are fitted to (a robust standard deviation, in microseconds),
which is the spread of the dots across the strip; smaller means less
scatter. Noise, a muffled sound and the stand add to it as well as the
watch. The **?** on
each reading explains it at length.

**What limits a reading.** Each figure carries its ±, worked out as
`analyze` and `session` work out theirs (`timegrapher_core::measure`),
over the averaging time: how far the figure could be off, a property of
the measurement rather than of the watch. The amplitude's and beat error's ± are the
standard errors of their 2 s windows in the averaging time. The rate's ±
comes from the scatter of 10 s windows over the run so far (back to a
pause or a move, up to five minutes), scaled to the averaging time; it
needs about 30 s of unbroken beats. What the ± can't show is named, as a
plain fact, at the right of the reading's caption, with a sentence on
hover: "12 s of beats" when the beats span under 10 s, "540 of 600
expected beats found" when under 95% were found, "3% of beats clipped"
when over 1% clip (amplitude and beat error), "lift angle 52°, the
default (calibre not set)" when no calibre or lift angle is set
(amplitude), a beat rate set that differs from the one heard, and pauses in the stretch. No reading is
graded. That the input's clock hasn't been measured is said in the Rate
card's **?** rather than on the card, with how to measure it. When the beats found don't come at regular intervals (their
timing scatter is 2% of a beat or more, `beats::watch_heard`, as
`analyze` uses), the cards show no figures, only **No regular beat found
(timing scatter 8.6 ms, 5.2% of a beat)**. When nothing in the take has
had a regular beat after its first few seconds, the cards and charts give
way to one card saying the same; they appear once a regular beat is
found.

**Positions Test.** Positions Test… beside the microphone buttons leads
through the six positions in the app's order: Dial Up, Dial Down, Crown
Down, Crown Left, Crown Up, Crown Right. It starts listening if need be.
A banner over the readings names the position and shows a progress bar
while the watch settles (20 s) and is measured (60 s), times the project
chose. It then says which position to turn the watch to; press the
button naming it (**Start CL Measurement**) once it is there. **Restart
DD Measurement**, naming the position just measured (or the one under
way), settles and measures it again, after a knock for instance, and
**Stop** ends the test. Only the 60 s measured in each position count.
From the moment the test starts, the strip, its position ticker and the
charts mark the test's phases, Dial Up included, each in its own grey:
**Before Test** over the sound before the test began (left plain, with a
line where the test begins), **Settling** over each 20 s settle (a light
shade), each position's measured 60 s (the darkest shade, named by the
position), and **Not Used for Positions Test** (hatched) over the time
after a measurement while the watch is being turned, over a measurement
given up for a restart, and over the part of a position measured when
the test was stopped. Beats outside the measured stretches are drawn
faintly. The marks stay after the test until a new test or a new
session. Each position is a
stretch of the one session, as any change of position is, so the strip
and charts mark them and a saved session keeps them for `timegrapher
session`. After the sixth, **Finish** reads the six measured stretches
with core `session::measure`, as `session` does (rate on the input's
measured clock when there is one), and a window shows them as a table:
each position by name ("Crown Up (CU, 3H)"), its rate, its rate
less the mean of the six, its amplitude and its beat error (a size), a
Mean row, the largest difference between the rates (D), and vertical
less horizontal for rate and amplitude (DVH). Under
them, a plain line for each condition found in a position's sound: no
regular beat (with its timing scatter), clipping, or under 10 s of
beats. The figures are `session`'s for the same stretches. **Save as
CSV…** saves the table, the means, the differences and those lines as a
CSV file a spreadsheet opens, and **Copy** puts the same on the
clipboard with tabs between the columns. It gives no verdict and no
pass or fail: the figures are for the watchmaker to judge.

**Isochronism.** A tab beside the Steadiness panes, from the same run of tests: the
rate by amplitude in 2° steps (the median rate as a dot, with bars over the
middle half), the curve through them and where it turns, and under it the
first half hour against the half hour 24 hours later (or the session's last),
the usual 0 h / 24 h comparison, captioned Full Wind Against 24 Hours
Later. It needs the amplitude to fall
through at least three 2° steps, which a run-down over some hours gives.
A note on what it needs is behind the **?** in its toolbar. Until there are 5 minutes of beats in one position, the
Steadiness and Isochronism tabs say so; during a Positions Test they say
that the test never stays that long in one position. Both scroll with a
bar always showing when the pane is too short for every row.

**True Clock.** While it listens, the app times the microphone's clock
against the computer's. After 20 minutes of listening, if no error is
kept for that input yet and the timing is known to half a ppm, the Rate
card shows the error it timed with a **Keep** button (a busy computer
delivers sound less evenly, so the offer can come later); the timing is only right when the computer's clock is
kept by internet time (NTP). When the microphone's own clock error has
been kept (by **Keep**, `timegrapher clock measure --save`, or `clock
from-log --save` after a take with a clock log; see `clock` in
[the agent interface](agent-interface.md#clock)), a
line under the rate gives the rate corrected onto true time, labelled
True Clock. A sound card's crystal runs a steady 10 to 50 ppm off, which
moves every rate read on it by 1 to 4 s/d. The big figure always stays on
the card's clock, the same clock tg and `timegrapher analyze` read, so it
compares directly with them; the True Clock line is beside it, never
instead of it. It is looked up by the input's name when a microphone
session starts, and is not shown for a replayed recording.

**Clipping.** Whenever a beat in the readings has a clipped sample in
its sound, counted as `timegrapher analyze` counts them, an amber banner
above the readings says **Clipping on 3% of beats** (or "under 1%").
Clipping flattens the loudest part of each tick, which moves
the drop and so the amplitude and the beat error; the rate is unaffected.
The banner follows the readings: it shows while the beats being averaged
clip and goes when they no longer do, and looking back through a
recording shows it for the stretches that clipped. The peak meter in the
Watch pane turns red on any clipped sample at once; the banner counts the
beats in the readings. Even a watch driven several times past full scale is
still read, with the banner, rather than taken for silence: when most of
the beats found clip, the flattened peaks no longer count against the
signal.

**Panes.** Below the readings are five panes: the paper strip, the tick tock
profile, and charts of rate, amplitude and beat error over the session. Drag a pane by its tab
to put it beside, above or below another, or onto another's tab to stack
them; drag the gaps between panes to resize them. The selected tab takes
the card's colour, joined to the card below it. The × on a tab hides
that pane (the status line says it comes back from its icon at the top or
the View menu), and the icons and the View menu hide or show each pane
and the readings across the top; a hidden pane comes back where it was,
and changing the strip's direction keeps the choice. **Reset Panes**
(in the View menu) shows them all in their starting places. Periodicity views will join
them.

**Tick Tock Profile.** The typical sound of the ticks (blue) and of
the tocks (orange) over the averaging time (Average Over, up to 60 s, so
it describes the same beats as the readings; a longer time gives a
steadier shape, and after lengthening it the beat count fills in as beats
arrive), from the drop's side of the beat back
through the unlock, with a shaded band where the middle 80% of beats fall
and the marks the engine read: unlock (green) and drop (indigo), from which
amplitude and the unlock-based beat error come, the drop's peak (a thinner
indigo line), the three sounds where the engine tells them apart (dashed brown: 1 unlock,
2 impulse, 3 drop) and the noise floor. Each line is named on the plot.
The solid edges and the dashed sounds are two different measurements of
the same averaged sound, not single beats: the edges are where the
readings come from, and the sounds are where each of the three sounds
rises halfway up its own climb, from a separate measurement of the
beat's shape, so they sit near but not exactly on the edges. A sound line is missing when
that sound can't be told apart from its neighbour (a soft unlock merged
into the impulse, say). The Tick tock
profile card switches the edges and the sounds on and off, sets the scale
and lays the tick above the tock (Stacked) or beside it (Beside). Tick and
tock share one vertical scale, so a quieter side shows smaller, and every
grid line is drawn alike.
Each plot has its own time axis. **Time From: Shared** (the default) draws
tick and tock on one clock: 0 ms is where a watch in beat would drop, and
each side sits half the beat error from it, so the two drop lines stand
the beat error from the drop apart and the two unlock lines the beat error
from the unlock. **Own Drop** measures each side from its own drop, so both
drops sit at 0 ms. The two beat errors differ when tick and tock take
different times from unlock to drop, the imbalance that also gives them
different amplitudes; the unlock figure is the one tg reports. Above each plot are the beat count, the marks in ms
from that side's own drop, the unlock-to-drop time and the amplitude they
give. Linear (the
default) shows the envelope as the engine measures it; dB shows the level
below each side's loudest point, so the quiet unlock shows more clearly. Looking back, the pane shows the sound the session
kept nearest before that moment (one every 2 s); for a recording analysed
all at once it is worked out from the file when you stop on a moment.
When the tick's and tock's marks disagree, or a mark sits mid-ramp, the readings
built on them are suspect.

**Scope.** The sound itself, beat by beat, like the scope view on a
Witschi (whose training course, doc. 71.1010D35e p. 12, gives the
same 20, 200 and 400 ms spans): what the engine heard, high-passed at 1 kHz, not an average. A
tab beside the Tick Tock Profile, set in its toolbar.
**Span 20 ms** shows the Tick and the Tock
in rows of their own, lined up on the drop, from 14 ms before it to 6 ms
after, with the profile's Unlock and Drop marks. **5 ms** (3.5 ms before
the drop to 1.5 ms after) and **10 ms** (8 ms before to 2 ms after) show
the drop closer still, and **50 ms** (15 ms before to 35 ms after) the
sound and what rings on after it, all with the same marks. **One Beat**
runs from 15 ms before one beat to 15 ms before the next, at the
watch's beat rate (125 ms at 28,800 bph, 200 ms at 18,000). **200 ms** shows one
row each from 20 ms before the beat to 180 ms after, so the next beat
shows too. **400 ms** shows every beat in one grey row, three or four
beats at once. **Beats** draws the latest 1, 4, 8 or 16 over each other,
the older ones fainter, so a beat that differs from the rest stands out.
While listening it shows the latest beats; looking back, or on a
recording, it shows the beats up to the moment shown, read from the
recording. Only the few seconds around the beats are read, however long
the recording: a WAV seeks there and a FLAC is searched by its frame
headers (`span.rs`). No crosshair, as on the other sound plots.

**Paper Strip.** One dot per beat, tick and tock in two colours. Across the
strip is how early or late each beat came against a perfect clock beating
exactly at the nominal beat rate (every 125 ms at 28,800 bph): a beat
before that clock's beat is early, after it late, the words Witschi's
training course uses (doc. 71.1010D35e p. 6).
Along it is time; both axes are labelled, in ms across and in minutes and
seconds along, and every grid line is drawn alike. **Rate Line** (on by
default) draws the Rate reading as a thin solid red line over the beats it was
fitted to, on top of them, as tg does: if the dots follow it, the reading
describes them well. **Parallel Guides** (on by default) adds faint red
lines at the same slope across the whole strip, one per grid step, since
the eye judges whether lines are parallel far better than it judges a
slope; dots that bend away from the guides show the rate changing. Under
**Draw over the strip**, amplitude (purple, on by default), rate (red)
and beat error (yellow) readings can be drawn as lines against the same
time, each on its own scale, whose ends are written in its colour above
the strip (beside it when the strip lies across), as some versions of tg
do for amplitude: a wobble in the rate that comes with a dip in amplitude
shows at a glance. Running **down** (the default, as on
tg), the newest beats are at the top, a watch on rate draws a vertical
line, a gaining watch leans right as it rises (/) and a losing one leans
left (\\). Running **across**, time goes left to right with the newest beats
on the right, and early is up. The gap between the two lines is the beat
error. A line that runs off one edge comes back on the other, so the
strip's width is its zoom and its length sets how much history it shows.
Type both (a width of ±0.1 to ±250 ms; a length such as `45`, `5 min`,
`2h` or `1:30:00`) or pick a common value from the list beside each: the
widths run from ±1 ms, for a good watch's beat error, to ±62.5 ms, half a
beat at 28,800 bph and the widest view in which tick and tock can't wrap
onto each other; the lengths run from 10 s of beats to a two-hour run.

On the strip, the mouse wheel changes the length, Ctrl and the wheel (or a
pinch) changes the width, dragging along the time axis looks back through
the session (live or not), dragging across slides the trace, and a double
click returns to the newest beats and centres them. **Auto-Centre** keeps
the newest beats on the centre line all the time, so the older ones slide
away from it as the rate wanders; sliding the trace by hand turns it off.
**Centre** centres once; **Clear** starts the readings and the strip again.

**Charts.** Rate, amplitude (the average, from the ticks and from the tocks) and beat error (from the
unlock and from the drop) over time. Each reading keeps its colour across
the app (rate red, amplitude purple, beat error yellow), so a second line
of the same reading is told apart by its dashes: amplitude from the ticks
dashes purple over blue and from the tocks purple over orange, and beat
error from the drop is a fainter dashed yellow; the key and the box at the
pointer draw them the same way. A chart
with more than one line has its key at the right of its tab bar, where it
covers nothing. Each chart labels its vertical scale at two marks or more,
in small text, however short the chart; a long time at the right end of
the time axis is left out rather than cut in half. **Time Span**, in the Charts and
Distributions cards, sets what they cover: **Session**, everything since the
session started, or **Strip**, the same stretch of time as the paper strip,
moving with it, so that with the charts under a strip lying across every
time axis lines up: the strip and the charts keep the same width of axis
on the left and of overlay scales on the right, so their times sit one
above the other. In Strip mode the strip sets the time, so only the
vertical scale can be dragged or zoomed. Wherever the pointer is on the
strip or a chart, a thin line marks that moment on all of them, and on the
strip a label gives its time from the start to the hundredth of a second
(such as 1:23.45). With the pointer
anywhere over a chart, a vertical line marks that moment and a box lists
every line's value there. Their time axes move together:
the mouse wheel zooms time around the pointer, Ctrl and the wheel zooms
the vertical scale, dragging pans, and a double click fits the whole
session again and, while listening, follows new beats as they come in.
Once a chart has been panned or zoomed a **Follow Live** button
(**Show Whole Session** for a recording) appears on it and does the same,
and the box at the pointer lists these controls. Clicking a moment shows it on the strip and in the
readings. The scale is the 2nd to 98th percentile of the values, kept
within eight robust spreads of the median, so neither one glitch nor a
few seconds of knocking flattens the lines. Beat error is charted as a
size, as on its card. Changing the averaging time or the
lift angle works every point out again, back to the start of the session.

**Distributions.** How the values of rate, amplitude and beat error spread
over the session or the strip's length (Time Span), switched on in the
Distributions card (all off at first). An average gives one number; these
show whether a watch keeps to one state or moves between two, such as a
high and a low amplitude. Amplitude and beat error count either the
readings (**Values: Readings**, the default, averaged over Average Over as
on the charts and the strip) or each 2 seconds of beats (**2 s**, the
finest the engine measures them, with more scatter); rate counts readings.
**Show: Probability** (the default) is a normal probability plot, the
probability paper of flood-frequency analysis: each value against its rank,
on a vertical scale spaced in standard deviations and labelled in percent
(Hazen plotting positions). Values from one steady state fall along the
dashed straight line, drawn through the median with a slope from the
interquartile range; two states draw two straighter stretches joined by a
bend, or an S across the line. **Cumulative** draws the share of values at
or below each value on a plain scale. Neither keeps the time order, so the
strip and the charts show when a change happens. The scale leaves out the
most extreme 0.5% at each end so stray readings can't squeeze the rest
together, and the line above gives the count, the median, the middle 80%,
the outliers left off and the peaks when there is more than one. The
statistics are in `timegrapher_core::histogram`, so the CLI and reports can
share them.

**Steadiness.** How the rate, the amplitude and the beat error (from the
unlock) each change over the session, from the same tests as
`timegrapher series` (see [series.md](series.md)). Each view is a pane of
its own, so two can sit side by side; their icons sit together under
**Steadiness** in the icon bar (and in a Steadiness menu under View), and
they start as tabs beside the Tick Tock Profile, with only Readings Over
Time shown. Each series shows the sentence `series` writes, saying what
was detected (a cycle, two levels, a step, a trend, correlation between
readings, or none of these), with no verdict word or colour, and under it
the statistics: readings, median, standard deviation, the scatter from one
reading to the next, the lag-1 correlation and Ljung–Box p-value, and the
trend and period when found. **Rate**, **Amplitude** and **Beat Error**,
in each pane's toolbar, choose which series that pane shows; at least one
stays on. The views:
**Readings Over Time** (the readings in time order, with the stretches of
constant level the change finder found and, for two states, the two
levels dashed; scaled to the readings with lone wild ones left out),
**Periods** (the period search's score at each period on a logarithmic
axis, with the 1% false-alarm line dashed, the escape, fourth and centre
wheels' periods marked, and the strongest component labelled; **Log
Scale** in its toolbar draws the scores on a log scale, so a small peak
beside a tall one still shows), **Cycle**
(the readings folded at the cycle the tests found: every cycle as dots and
the median shape as a line), **Autocorrelation** (with the ±1.96/√n band
dashed, the repeat marked and the Ljung–Box p-value), **Allan Deviation**
(log-log, against the dashed line independent readings follow, labelled
where it ends, with the averaging time of least scatter labelled),
**CUSUM** (with the ±1.36 lines dashed and the largest excursion
labelled) and **FFT** (the plain amplitude spectrum of each series, like
Witschi's: the size of the sine wave at each period, log-log, in the
series' own units, the rate's as the swing peak to peak it makes in 10 s
rate readings, in s/d, from 20 s up, worked out from the beat timing's
spectrum (a timing sine of *a* ms at period *P* swings the rate by
2·*a*·2π/*P*·86.4 s/d, of which a 10 s reading keeps sin *x*/*x*, *x* =
π·10/*P*); **Timing** in its toolbar draws rate as the beat timing's own
spectrum, in ms, at every period; the wheels' periods are marked and the
tallest point labelled). In Periods and FFT each wheel's
period is a bold line and half of it, twice a turn as an off-centre wheel
shows, a fainter dashed line marked ×2; the grid behind is pale. The tests need 5 minutes
of beats. They run over the whole session in the background, again every
minute while it grows; for a recording they cover what has been replayed,
or the whole file once it is opened. Rows stack and fill the pane when it is
tall enough, sit side by side when it is wide and short, and scroll
otherwise. A line at the top says in plain words what the view
shows. The plots don't pan, zoom or show values under the pointer.

**Remembered settings.** The app keeps its views between runs: the strip's
direction, width and length and what is drawn over it, the profile's
layout, scale and Time From, the panes shown and their arrangement, the
distribution settings, the averaging time, folded groups in the Watch pane, the
appearance, the last microphone (used again when it is plugged in) and the
window's size and place. Nothing about the watch is kept: the lift angle,
beat rate and position start fresh, since the next watch on the pickup may
differ. **Reset Panes** puts the panes back as they started.

**Log and crash records.** The app writes a log, `logs/timegrapher.log`, in
the timegrapher settings folder (`~/.config/timegrapher` on Linux,
`~/Library/Application Support/timegrapher` on macOS,
`%APPDATA%\timegrapher` on Windows; the folder that holds `clocks.json`),
where `timegrapher report` collects it: the version, the microphone
opened, level changes, beat lock gained and lost, position changes and
every message the app shows. It is kept under 1 MB, with the two files
before it. If the app panics it also writes `crash-<time>.txt` there with
the message, where it happened and the last 200 log lines, and the next
start shows **The timegrapher closed unexpectedly last time. Make a
report?** with **Make a Report…** (copies the record and opens a new GitHub
issue to paste it into), **Show the Record** and **Dismiss**. Nothing is
sent anywhere by the app itself.

**Report a Problem.** **Report a Problem…** at the right of the status line
makes the same zip as `timegrapher report` (see [reports.md](reports.md)) to
attach to a GitHub issue. Choose what kind of problem it is (Bug, Reading,
Setup or Idea) and say what happened. **Include the Sound** adds up to five
minutes of the sound, cut without changing a sample: the last five minutes of
a live session (the app keeps the session's sound as it goes), or five
minutes of the recording open, chosen with **Ends At**. Drag the handles on
the waveform to keep only the part that shows the problem, and **Play** it.
**Save Report…** is offered once you tick that you listened and the sound
holds nothing private, such as voices; ticking the CC0 box offers it to the
public test corpus. The zip holds the excerpt and `analyze --json` of it
(which `report --replay` runs again), the app's readings and settings in
`results/app.json`, the log and crash records from the last week and the
stored clock corrections, with the home folder, user name and host name
removed. Saving opens the zip's folder and the matching GitHub form; the app
sends nothing itself.

**Appearance.** In the View menu: Auto follows the desktop's light or dark
setting; Light and Dark override it.

**No input touched at launch.** Listing the inputs asks each one what it
supports, which briefly opens it and can disturb another program recording
from the same microphone (a long take running beside the app, say). So the
app lists them only when the microphone menu opens, **Rescan** is pressed
or **Start** is clicked; until then the menu names the microphone kept from
last time, or System Default. Replaying or analysing a recording never
touches an input.

**Level meter.** In the Watch pane, while listening to a microphone: a bar to
the loudest sample of the last half second, grey, or red while the input
clips, with plain reference marks at −10 and −6 dBFS, and the peak in
dBFS beside it ("peak −14 dBFS", with ", clipping" while it clips).

**Status bar.** The input; the signal, "signal 12×", how far the typical
beat's peak stands above the typical level between beats (noise alone
reads about 2×, and below 4× no beats are logged). It measures
loudness, not the shape of the tick, so hum, knocks or a holder that
smears the sound aren't caught by it. Then how much sound the session
holds and whether it is saved, and on the right the beat rate and the
position, **Check for Updates…** and **Report a Problem…**.

**Updates.** With **Check for Updates at Start** on (the Help menu;
on unless switched off), the app asks GitHub once at start, on a
background thread, whether a newer release is published
(`timegrapher_core::update::check`). If one is, a banner says
"Timegrapher 0.2.0 is available" with **Download**, which opens the
release's page in the browser. Nothing installs itself. At start, a
failed check or an up-to-date version says nothing. **Check for
Updates…** in the status bar or the Help menu makes the same check and always says what it
found: the notice, "up to date", or why it couldn't check ("no release has
been published yet"). The request carries no account or identifier.

## Settings

- **Input.** In the Watch pane: a sound input device, or a WAV or FLAC file chosen with
  **Open…** or typed in. On Linux the menu offers each microphone once:
  **System default (via PipeWire)**, which is whatever input the
  desktop's sound settings choose and works alongside other programs, and
  each card's direct device, marked "(direct)", which goes straight to
  the card but can't open while the desktop sound server holds it. With a
  USB timegrapher microphone set as the desktop's input, choose the system
  default. **Show every input** lists everything the system offers, with
  ids, as `timegrapher devices` does (the switch is under Microphone). If the input can't be opened, stops,
  or sends no sound for 3 s, the app stops and says why above the
  readings, rather than showing an empty strip. The status bar shows the
  sample rate and bit depth the input opened at. Where the desktop passes
  dropped files on (X11, Windows, macOS; not yet Wayland), dropping a file
  or a folder on the window analyses it. The app asks the device for 48 kHz in as few
  channels as it offers, mixed to mono, and takes 16-bit samples when the
  device has them.
- **Input level.** Under Microphone in the Watch pane: the microphone's gain and a peak meter
  with marks at the −10 dBFS target and the −6 dBFS limit. On Linux, for
  the system default the level is the sound server's input volume
  (`wpctl`, or `pactl` on PulseAudio), which the server writes back onto
  the microphone each time it opens it, so setting the card's own control
  there would not last; for a direct device it is the card's capture
  control (`amixer`). The change is made when you let go of the slider.
  On other systems, set the level in the system's sound settings. The
  same code (`timegrapher_core::mixer`) gives `timegrapher doctor` its
  proposals. When the microphone's automatic gain is on, the card says
  **Automatic gain: on** with a **Turn It Off** button, and otherwise
  says nothing about it.
- **Calibre.** Not Set, a calibre from the list (`timegrapher_core::calibres`,
  some 4,500 of them, each with its sources), or one of your own.
  - **The menu.** It offers Not Set, the calibres picked lately
    (**Recent**), the **Short List** by maker and **Your Calibres**, with
    **Manage Calibres…** at its foot and as a button under it.
  - **Manage Calibres.** A window over the app with every calibre in a
    list down the left, by maker (the common and modern makers first, your
    own calibres at the top), and the one selected in full on the right.
    **All**, **Short List** and **Yours** choose what the list shows.
    Typing in its search box searches the whole list. Every word must
    match a calibre's number, one of its other names, or its maker under
    any of the maker's names ("dd 3235", "sw200", "seagull st19"), and the
    best matches come first. **Use for This Watch** sets the selected
    calibre under Watch. **New Custom Calibre…** adds one, named as typed
    in the search box. **Done**, Escape or a click outside closes it.
  - **The short list.** It starts as the list's common and modern
    calibres. In Manage Calibres a tickbox beside every calibre puts it on
    the short list or takes it off, and **Tick All** and **Untick All**
    beside a maker do a whole maker at once: untick the clones, say, or
    keep just one maker's calibres. **Reset the Short List** goes back to
    the one the app comes with. The short list and the recent picks are
    kept with the app's settings.
  - **What a pick does.** It fills in the calibre's lift angle, where the
    list gives one, and names the cycles Steadiness finds after its own
    wheels. If the watch beats at another rate than the calibre, a line
    under the menu says so. With none picked and the lift angle left at
    52°, a line says the amplitude is worked out from that default.
  - **The panel.** Under the menu, a line gives the maker, beat rate, lift
    angle and escape wheel, or says which of them the list doesn't give.
    **Wheels and Sources**, folded until opened, lists each wheel's time
    for one turn, as Manage Calibres does for the calibre selected there:
    - The six every lever movement has (balance, escape, fourth, third
      and centre wheels, and the barrel) come first, then any other the
      list names.
    - The hint on each wheel says where its figure comes from.
    - A wheel nobody has given is blank: type its period and it is kept
      for that calibre, marked "yours", with **Clear** to go back.
    - **Another wheel** adds one by name.
    - Below come the list's sources as links, how its figures are known,
      and its notes.
  - **Your additions.** Wheel periods and custom calibres are kept in
    `calibres.json` in the settings folder (core `calibre_store`), which
    the command line and the MCP server read too. Calibres typed in under
    earlier versions move there the first time the app starts.
  - **Share with the Project….** It opens a GitHub issue in the browser,
    filled in with what you added to the calibre, to look over and send.
    Nothing is sent from the app.
  - **A custom calibre.** Its panel, and Manage Calibres, also have its
    name, maker, beat rate and lift angle, and **Delete Calibre**.
  - **What isn't kept.** The pick itself, like the lift angle, is about
    the watch on the stand and isn't kept. Not Set names cycles after the
    wheels most calibres at the beat rate share.
- **Beat rate.** Auto (guessed from the first seconds) or any standard rate
  from 12,000 to 72,000 bph. Changing it starts the readings again.
- **Lift angle.** 52° by default. Type the calibre's angle and press Enter,
  or pick a common one. Amplitude is worked out again at once, history
  included.
- **Position.** DU, DD, CU, CD, CL, CR, with the Swiss laboratory names
  beside the crown positions, the hour that points up: Crown Up (CU, 3H),
  Crown Down (CD, 9H), Crown Left (CL, 6H) and Crown Right (CR, 12H).
  The Positions Test's table names them the same way. A new position is a new
  measurement, so changing it while listening (or paused) starts a new
  stretch, noted in the saved recording. While listening, when the beats
  stop for more than 3 seconds and come back with no change of position
  marked, a banner asks whether the watch was moved, with a button for
  each position and **Not Moved**; it goes by itself a minute later. The strip keeps its beats and
  marks each stretch, shading every other one, with a ticker above the
  plot (beside it, when the strip runs down) that names the position over
  and over along its stretch, so it shows even with both ends of the
  stretch scrolled out of sight; the Rate, Amplitude and Beat Error charts mark them the same way
  over the session or the strip's length. The readings don't reach back
  past the change, the tick and tock profile starts again, and the
  distributions and Steadiness take only the stretches in the position the
  watch is in now: dial up for 5 minutes, dial down for 10, then dial up
  again tests the two dial-up stretches together and leaves the dial-down
  one out. The first 10 seconds in a new position are settling (from
  Resume, when the position was changed while stopped): the ticker says
  Settling, the beats are drawn faintly, and they are left out of the
  readings, the charts' points, the distributions and Steadiness, but kept
  in the audio. A saved take's `session.toml` starts each later position
  after its settling, so `timegrapher session` reads the same seconds as
  the app. Guided runs through the positions (as `timegrapher
  session` reports them) will build on it.

## Sessions

Opening the app never touches the microphone; only **Start** opens it.
When another program records from the same microphone (a long take with
the recorder, say), don't press Start during its take, and press Stop
before one begins.

**Start** begins a session from the microphone. **Stop** stops
listening and lets go of the microphone, so another program can use it
(Microphone Off shows beside the level meter), and keeps everything: the
readings, strip, charts and the sound so far.
**Resume** carries on in the same session: the strip and charts leave
the stopped time out, and the readings start from the beats after the
stop. **New session**, shown while stopped, clears everything for the
next watch or position. Everything heard is kept as it comes in, so
**Save…** can be pressed at any time, running or stopped: it asks for a
folder and copies the session into a folder of its own inside it.
**Save Again** (Ctrl+S) later brings that copy up to date without asking
again, and **Save As…** (Ctrl+Shift+S) copies the session to a new folder
of your choosing, which Save Again then keeps up to date. Once the take has had a position change, the copy's
folder name ends in `_positions` rather than the first position. Sound that hasn't been saved is never thrown away
without asking: New session, opening or replaying a recording, and
closing the window ask whether to save it first. The microphone's own
settings are kept aside while recordings are open and come back with
**Microphone**. Once stopped, the time slider shows for a microphone
session too; stopped during a move's settling time, the rate card gives
the reading from before the move and says so.

Each saved recording is a folder named after the start time, the watch
and the position, so it can be analysed again later with a newer engine,
holding:

| File | Contents |
| --- | --- |
| `audio-001.wav`, ... | The audio as captured, mono, 16-bit for a 16-bit microphone, in files of at most an hour; listening again after a pause starts a new file |
| `clock.csv` | System time against frames captured, every 10 s, and on both sides of each pause; frames run on across a pause, with no line made up for the gap |
| `session.json` | Watch, position, lift angle, beat rate, device, start and end, position changes, the readings at the end, and (only once the take has been paused) `breaks`: one entry per resume with `frame` (frames in the take before it), `audio_s`, `segment` (its first file), `utc` (host time of its first frame), `gap_s` (host time missing, from the clock log either side) and `device` (the input after it) |
| `session.toml` | The session file `timegrapher session` reads: watch, calibre (with the wheels of a custom one), lift angle, beat rate, and one `[[recording]]` per stretch in one position with its `start_s` and `end_s` (for a take saved during a Positions Test, the test's own measured stretches, after its 20 s settling and for its 60 s, so `timegrapher session` reads the seconds its table did) |

Opening a recording or folder that has a `session.toml` beside it (or in
it) that lists it takes back the watch, the calibre (a custom one joins
Your Calibres), the lift angle, the beat rate and the position stretches,
so the strip and charts mark them and Steadiness tests each position on
its own. Without one, a recording opens with the settings back at their
defaults (no watch name or calibre, 52°, the beat rate found, Dial Up), so
nothing carries over from the last recording, except that a position named as a word of its own in the file's or
folder's name (`reverso_DU_01.flac`) is taken, and the message says so
with the lift angle in use.

The folder reads straight into the long-run analysis, calibrated against
the system clock:

```sh
timegrapher long "2026-10-08T21-30-00_Yacht-Master_DU/" \
  --clock "2026-10-08T21-30-00_Yacht-Master_DU/clock.csv"
```

The WAV header is brought up to date every 10 s, so a recording survives
the program being stopped abruptly.

## Without a window

For scripts, tests and an AI agent helping someone set up:

```sh
timegrapher-app --devices          # sound input devices and their formats, as JSON
timegrapher-app --headless rec.flac --every 10 --average 10
```

`--headless` runs a recording through the live engine as fast as it can and
prints a JSON line of readings every `--every` seconds of audio (also
`--bph`, `--lift`). The same calls are in the core library
(`timegrapher_core::capture` for devices, capture and levels,
`timegrapher_core::live` for the live readings,
`timegrapher_core::recorder` for saving), so the command-line tool can
offer them too. On Windows the app opens without a console window, so
these two print only when their output is redirected to a file or piped
to another program (`timegrapher-app --devices > devices.json`).

## How the live readings are made

Every half second the last 4 s of audio go through the same envelope,
template and beat tracker as a recording (`timegrapher_core::live`). Beats
are added to a running beat log once they are at least a quarter of a
second from the end of the audio, so each beat is measured once, with the
same template from the first seconds onward; that keeps the beat's
reference point still, so the strip doesn't jump. Amplitude is measured on
2 s windows of that log, as in a recording. The readings are fits over the
log, so the live screen and an analysis of the saved recording agree: on
the 5-minute 3235 recording, the live jitter (165 µs median) matches the
command line's 166 µs.

Expect the first reading about 4 s after the watch goes on the microphone.
If nothing is heard for 5 s with the beat rate on Auto, the template is
dropped and the next watch is picked up from scratch, even at a different
beat rate.
